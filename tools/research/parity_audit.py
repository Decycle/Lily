"""Systematic visual-parity audit: real-game reference frames vs our renders at the same chart times.

Usage: python -I parity_audit.py <run dir> <out dir> [--every N] [--match REGEX] [--title T] [--level L]
                                 [--env K=V]... [--no-movie] [--latency S] [--hit-latency S]
                                 [--bg-good-at S]

Without a movie the background scene is drawn in its Normal state; `--bg-good-at S` switches it to Good
from chart time S on (the switch measured on the run's video; `--env WACCA_SHOT_BG=off` = no background).

<run dir> is an E2/E3 run (`runs/e2|e3/<ts>`: meta.json or e2.json naming the chart, e2.json `visual_timing`,
reference/frames.csv + PNGs, inject.jsonl, e3.json when present). For every reference frame (every Nth, or those
whose label matches REGEX) it renders ours with the playfield example's `WACCA_SHOT` at the frame's chart time
moved by the run's measured visual lead (`visual_timing`, interpolated), with
* the play replayed: the injected touches (inject.jsonl edges) and, for E3 runs, the real per-note judgements
  (e3.json `real`, hit at note time + probe shift, judgement shown at note time + `shown`); notes without an
  outcome are perfect (`WACCA_SHOT_REPLAY`). Touches are moved by the game's input latency on the picture
  clock (--latency, default 0.025 s: a touch's rim highlight shows 0.025 s after its script time); hit effects
  by --hit-latency (default 0),
* the song's background movie frame behind (`WACCA_SHOT_MOVIE`, planes cut from assets/Movie/mv_<id>.usm),
and compares with `stagecmp compare` (view square 0,228,675,675). Writes <out>/sheets/NNNN-<label>.png
(real | ours | |diff|x4), <out>/audit.csv (mean |diff| per region and per 25 px band) and prints a summary.
Needs `cargo build --release -p wacca-render --example playfield --example stagecmp` and the worktree's data/.
Read-only on the run directory.
"""
import csv
import json
import os
import re
import struct
import subprocess
import sys

FFMPEG = os.environ.get("WACCA_FFMPEG", r"C:\Users\<user>\AppData\Local\Microsoft\WinGet\Packages\Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe\ffmpeg-9.0.2-full_build\bin\ffmpeg.exe")
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
EXAMPLES = os.environ.get("WACCA_AUDIT_EXAMPLES", os.path.join(ROOT, "target", "release", "examples"))
ASSETS = os.environ.get("WACCA_ASSETS", r"C:\Users\<user>\OneDrive\Documents\WACCA\WACCA_slim\assets")
# Regions (px from the centre of the 675 px view; the judgement ring is at 292-315 px).
REGIONS = [("centre", 0, 100), ("inner", 100, 200), ("outer", 200, 275), ("ring", 275, 325), ("rim", 325, 999)]


def lanes_of_cells(hexcells):
    """inject.jsonl `cells` (60 hex digits, bit i of digit j = cell 4j+i) -> lane bit mask (any depth)."""
    mask = 0
    for j, c in enumerate(hexcells):
        nib = int(c, 16)
        for i in range(4):
            if nib >> i & 1:
                cell = 4 * j + i
                side, sector = cell // 120, cell % 30
                lane = 15 + sector if side == 1 else (74 - sector) % 60
                mask |= 1 << lane
    return mask


# `visual_timing` (analyze_e2's 2D note-edge fit) overestimates the picture lead: our 3D notes line up with the
# reference frames 33 ms earlier (best offset of the note annulus, KING −40/−30 ms, Fragile −20…−50 ms, mean −33).
LEAD_BIAS = 0.033


def visual_lead(timing):
    """`visual_timing` [[chart s, delta ms, n]] -> f(chart t) = seconds the picture is ahead of the audio."""
    pts = sorted((t, -d / 1000.0) for t, d, _ in timing)

    def f(t):
        if not pts:
            return 0.09
        if t <= pts[0][0]:
            return pts[0][1]
        for (t0, a), (t1, b) in zip(pts, pts[1:]):
            if t <= t1:
                return a + (b - a) * (t - t0) / (t1 - t0)
        return pts[-1][1]
    return f


def replay(run, latency, hit_latency):
    touches = []
    path = os.path.join(run, "inject.jsonl")
    if os.path.exists(path):
        for line in open(path):
            r = json.loads(line)
            if r.get("kind") == "edge":
                touches.append([r["t_script_ms"] / 1000.0 + latency, lanes_of_cells(r["cells"])])
    hits = []
    path = os.path.join(run, "e3.json")
    if os.path.exists(path):
        for o in json.load(open(path))["outcomes"]:
            if o.get("real") is None:
                continue  # not seen in the reference (the replay's default applies)
            probe = o.get("probe") or {}
            shift = probe.get("ms", 0.0) / 1000.0 if probe.get("probe") == "shift" else 0.0
            shown = o["time"] + o["shown"] if o.get("shown") is not None else o["end"]
            hit = (shown if o["real"] == "Miss" else o["time"] + shift) + hit_latency
            if o["class"] != "hold":
                shown = hit
            hits.append({"note": o["note"], "kind": o["real"], "hit": hit, "shown": shown})
    return {"hits": hits, "touches": touches, "default": "Marvelous"}


def chart_info(run):
    for name in ("meta.json", "e2.json"):
        p = os.path.join(run, name)
        if os.path.exists(p):
            chart = json.load(open(p)).get("chart")
            if chart:
                break
    text = open(chart, "rb").read().decode("utf-8", "replace")
    m = re.search(r"#MOVIEOFFSET\s+(-?[\d.]+)", text)
    sid = os.path.basename(os.path.dirname(chart)).replace("-", "_")
    movie = os.path.join(ASSETS, "Movie", "mv_%s.usm" % sid)
    return chart, (movie if os.path.exists(movie) else None), float(m.group(1)) if m else 0.0


def demux(usm, m1v):
    if os.path.exists(m1v):
        return
    with open(usm, "rb") as f, open(m1v, "wb") as o:
        while True:
            h = f.read(8)
            if len(h) < 8:
                break
            sig, size = h[:4], struct.unpack(">I", h[4:])[0]
            body = f.read(size)
            if sig == b"@SFV" and body[7] & 3 == 0:
                off, pad = body[1], struct.unpack(">H", body[2:4])[0]
                o.write(body[off:size - pad])


def movie_planes(usm, out, frames):
    """{60 fps frame index: dir with y/u/v.png}, cut in one decoding pass."""
    m1v = os.path.join(out, "movie.m1v")
    demux(usm, m1v)
    want = sorted(set(frames))
    dirs = {n: os.path.join(out, "movie", "%05d" % n) for n in want}
    todo = [n for n in want if not os.path.exists(os.path.join(dirs[n], "v.png"))]
    if todo:
        tmp = os.path.join(out, "movie", "tmp")
        os.makedirs(tmp, exist_ok=True)
        sel = "+".join("eq(n\\,%d)" % n for n in todo)
        outs = []
        for p in "yuv":
            outs += ["-map", "[%s]" % p, "-fps_mode", "passthrough", os.path.join(tmp, p + "_%05d.png")]
        subprocess.run([FFMPEG, "-v", "error", "-y", "-i", m1v, "-filter_complex",
                        "[0:v]select=%s,extractplanes=y+u+v[y][u][v]" % sel] + outs, check=True)
        for k, n in enumerate(todo, 1):
            os.makedirs(dirs[n], exist_ok=True)
            for p in "yuv":
                os.replace(os.path.join(tmp, "%s_%05d.png" % (p, k)), os.path.join(dirs[n], p + ".png"))
    return dirs


def compare(real, ours, sheet):
    out = subprocess.run([os.path.join(EXAMPLES, "stagecmp.exe"), "compare", real, ours, sheet],
                         check=True, capture_output=True, text=True).stdout
    bands = []
    for m in re.finditer(r"r\s+(\d+)-(\d+)\s+mean \|diff\|\s+([\d.]+) px (\d+)", out):
        bands.append((int(m.group(1)), float(m.group(3)), int(m.group(4))))
    return bands


def regions(bands):
    out = {}
    for name, r0, r1 in REGIONS:
        sel = [(d, n) for r, d, n in bands if r0 <= r < r1]
        n = sum(n for _, n in sel)
        out[name] = sum(d * n for d, n in sel) / n if n else 0.0
    n = sum(n for _, _, n in bands)
    out["all"] = sum(d * n for _, d, n in bands) / n if n else 0.0
    return out


def main():
    args = sys.argv[1:]
    run, out = args[0], args[1]
    every, match, title, level, movie_on, latency, hit_latency = 1, None, "", "NORMAL/Lv.1", True, 0.025, 0.0
    good_at = None
    env = dict(os.environ)
    i = 2
    while i < len(args):
        a = args[i]
        if a == "--every":
            every = int(args[i + 1]); i += 2
        elif a == "--match":
            match = re.compile(args[i + 1]); i += 2
        elif a == "--title":
            title = args[i + 1]; i += 2
        elif a == "--level":
            level = args[i + 1]; i += 2
        elif a == "--env":
            k, v = args[i + 1].split("=", 1); env[k] = v; i += 2
        elif a == "--hit-latency":
            hit_latency = float(args[i + 1]); i += 2
        elif a == "--latency":
            latency = float(args[i + 1]); i += 2
        elif a == "--no-movie":
            movie_on = False; i += 1
        elif a == "--bg-good-at":
            good_at = float(args[i + 1]); i += 2
        else:
            sys.exit("unknown argument " + a)
    os.makedirs(os.path.join(out, "sheets"), exist_ok=True)
    os.makedirs(os.path.join(out, "ours"), exist_ok=True)
    chart, usm, movie_offset = chart_info(run)
    timing = json.load(open(os.path.join(run, "e2.json"))).get("visual_timing") or []
    lead = visual_lead(timing)
    rp = os.path.join(out, "replay.json")
    json.dump(replay(run, latency, hit_latency), open(rp, "w"))
    refs = list(csv.DictReader(open(os.path.join(run, "reference", "frames.csv"))))
    refs = [r for k, r in enumerate(refs) if (match.search(r["label"]) if match else k % every == 0)]
    times = {r["file"]: float(r["chart_t"]) + lead(float(r["chart_t"])) - LEAD_BIAS for r in refs}
    planes = {}
    if usm and movie_on:
        frames = {f: max(round((t - movie_offset) * 60), 0) for f, t in times.items()}
        dirs = movie_planes(usm, out, frames.values())
        planes = {f: dirs[n] for f, n in frames.items()}
    env.update({"WACCA_SHOT_SIZE": "675", "WACCA_SHOT_SPEED": "1.5", "WACCA_SHOT_REPLAY": rp,
                "WACCA_SHOT_TITLE": title, "WACCA_SHOT_LEVEL": level})
    rows = []
    for r in refs:
        f = r["file"]
        t = times[f]
        ours = os.path.join(out, "ours", f)
        e = dict(env, WACCA_SHOT="%s@%.4f" % (ours, t))
        if f in planes:
            e["WACCA_SHOT_MOVIE"] = planes[f]
        if good_at is not None and t >= good_at:
            e["WACCA_SHOT_BG"] = "good"
        subprocess.run([os.path.join(EXAMPLES, "playfield.exe"), chart, os.path.join(ROOT, "data", "ui")],
                       env=e, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        bands = compare(os.path.join(run, "reference", f), ours, os.path.join(out, "sheets", f))
        reg = regions(bands)
        rows.append(dict({"file": f, "label": r["label"], "chart_t": r["chart_t"], "ours_t": "%.4f" % t},
                         **{k: "%.2f" % v for k, v in reg.items()},
                         **{"b%03d" % b: "%.2f" % d for b, d, _ in bands}))
        print("%-40s %s" % (f, " ".join("%s %5.2f" % kv for kv in reg.items())), flush=True)
    keys = list(rows[0].keys()) if rows else []
    with open(os.path.join(out, "audit.csv"), "w", newline="") as fh:
        w = csv.DictWriter(fh, keys)
        w.writeheader()
        w.writerows(rows)
    if rows:
        print("mean over %d frames: %s" % (len(rows), " ".join(
            "%s %.2f" % (k, sum(float(x[k]) for x in rows) / len(rows)) for k in [n for n, _, _ in REGIONS] + ["all"])))


if __name__ == "__main__":
    main()
