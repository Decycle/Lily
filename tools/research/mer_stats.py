"""Statistical survey of WACCA .mer chart files.

Usage: python -I mer_stats.py <MusicData dir>

Read-only: never writes to the asset directory. Prints a report to stdout.
"""
import sys
import os
import re
from collections import Counter, defaultdict

TPM = 1920  # ticks per measure

NOTE_NAMES = {
    1: "Touch", 2: "TouchBonus", 3: "SnapIn(red)", 4: "SnapOut(blue)",
    5: "SlideCW", 6: "SlideCWBonus", 7: "SlideCCW", 8: "SlideCCWBonus",
    9: "HoldStart", 10: "HoldSegment", 11: "HoldEnd", 12: "MaskAdd",
    13: "MaskRemove", 14: "EndOfChart", 16: "Chain", 20: "TouchR",
    21: "SnapInR", 22: "SnapOutR", 23: "SlideCWR", 24: "SlideCCWR",
    25: "HoldStartR", 26: "ChainR",
}
KIND_NAMES = {1: "Note", 2: "BPM", 3: "TimeSig", 4: "?4", 5: "HiSpeed",
              6: "ReverseEffectStart", 7: "ReverseEffectEnd", 8: "ReverseNoteEnd",
              9: "StopStart", 10: "StopEnd"}


def ex(store, key, val, n=3):
    lst = store[key]
    if len(lst) < n:
        lst.append(val)


def main():
    root = sys.argv[1]
    files = []
    for song in sorted(os.listdir(root)):
        d = os.path.join(root, song)
        if not os.path.isdir(d):
            continue
        for f in sorted(os.listdir(d)):
            if f.lower().endswith(".mer"):
                files.append((song, os.path.join(d, f)))

    kind_count = Counter()
    kind_cols = defaultdict(Counter)
    note_count = Counter()
    note_cols = defaultdict(Counter)
    note_flag = defaultdict(Counter)       # col8 value per note type
    note_files = Counter()                 # number of files containing note type
    mask_dir = Counter()
    pos_range = [999, -999]
    size_hist = Counter()
    size_by_type = defaultdict(Counter)
    tick_max = 0
    bad_tick = 0
    header_tags = Counter()
    header_vals = defaultdict(Counter)
    examples = defaultdict(list)
    bpm_vals = Counter()
    ts_vals = Counter()
    hs_vals = Counter()
    suffix = Counter()
    songs_by_suffix = defaultdict(set)
    eoc_per_file = Counter()
    notes_after_eoc = 0
    files_notes_after_eoc = 0
    hold_issues = Counter()
    hold_lengths = Counter()
    hold_seg_flag0 = 0
    index_issues = Counter()
    multi_bpm_same_tick = 0
    first_event_issue = 0
    rev_triplet_ok = 0
    rev_triplet_bad = 0
    stop_pairs_ok = 0
    stop_pairs_bad = 0
    files_with = Counter()
    crlf_files = 0
    bom_files = 0
    nonint_lines = 0
    blank_body = 0
    durations = []
    chain_in_hold = 0
    seg_backward = 0
    seg_same_tick = 0
    eoc_cols = Counter()
    lines_after_body_nonnum = Counter()
    offsets = []
    timesig_partial = 0
    unsorted_files = 0
    hold_wrap = 0
    total_rows = 0
    utf16_files = []
    hold_seg_pos_jump = Counter()

    for song, path in files:
        m = re.search(r"_(\d+)\.mer$", path)
        sfx = m.group(1) if m else "?"
        suffix[sfx] += 1
        songs_by_suffix[sfx].add(song)
        raw = open(path, "rb").read()
        if raw.startswith(b"\xff\xfe") or raw.startswith(b"\xfe\xff"):
            utf16_files.append(os.path.basename(path))
            raw = raw.decode("utf-16").encode("utf-8")
            if b"\r\n" in raw:
                crlf_files += 1
                raw = raw.replace(b"\r\n", b"\n")
        if raw.startswith(b"\xef\xbb\xbf"):
            bom_files += 1
            raw = raw[3:]
        if b"\r\n" in raw:
            crlf_files += 1
        text = raw.decode("utf-8", errors="replace")
        lines = text.splitlines()
        in_body = False
        rows = []
        for ln in lines:
            s = ln.strip()
            if not in_body:
                if s.startswith("#"):
                    parts = s.split(None, 1)
                    tag = parts[0]
                    val = parts[1] if len(parts) > 1 else ""
                    header_tags[tag] += 1
                    if tag == "#BODY":
                        in_body = True
                    elif tag != "#MUSIC_FILE_PATH":
                        header_vals[tag][val] += 1
                    if tag == "#OFFSET":
                        try:
                            offsets.append(float(val))
                        except ValueError:
                            pass
                elif s:
                    ex(examples, "nonheader_before_body", (path, s))
                continue
            if not s:
                blank_body += 1
                continue
            toks = s.split()
            if not all(re.fullmatch(r"-?\d+(\.\d+)?", t) for t in toks):
                nonint_lines += 1
                ex(examples, "nonnumeric_token_lines(lenient atoi-style parse applied)", (path, s))
            vals = []
            for i, t in enumerate(toks):
                m2 = re.match(r"-?\d+(\.\d*)?", t)
                v = m2.group() if m2 else "0"
                vals.append(float(v) if (i >= 3 and "." in v) else int(float(v)))
            rows.append(vals)
        total_rows += len(rows)

        # sortedness
        keys = [r[0] * TPM + r[1] for r in rows]
        if keys != sorted(keys):
            unsorted_files += 1
            ex(examples, "unsorted", path)

        notes = {}
        order = []
        eocs = []
        gimmicks = []
        kinds_here = set()
        types_here = set()
        for r in rows:
            meas, tick, kind = r[0], r[1], r[2]
            kind_count[kind] += 1
            kinds_here.add(kind)
            kind_cols[kind][len(r)] += 1
            if tick >= TPM or tick < 0:
                bad_tick += 1
                ex(examples, "tick_out_of_range", (path, r))
            tick_max = max(tick_max, tick)
            t = meas * TPM + tick
            if kind == 1:
                nt = r[3]
                note_count[nt] += 1
                types_here.add(nt)
                note_cols[nt][len(r)] += 1
                if len(r) > 7:
                    note_flag[nt][r[7]] += 1
                idx, pos, size = r[4], r[5], r[6]
                pos_range[0] = min(pos_range[0], pos)
                pos_range[1] = max(pos_range[1], pos)
                size_by_type[nt][size] += 1
                if nt in (12, 13):
                    mask_dir[r[8] if len(r) > 8 else None] += 1
                if nt == 14:
                    eocs.append(t)
                    eoc_cols[tuple(r[3:])] += 0  # placeholder
                    ex(examples, "eoc_row", (path, r), 5)
                if idx in notes:
                    index_issues["duplicate_index"] += 1
                notes[idx] = (t, nt, r)
                order.append(idx)
                if nt not in NOTE_NAMES:
                    ex(examples, "unknown_note_type", (path, r))
            else:
                gimmicks.append((t, kind, r))
                if kind == 2:
                    bpm_vals[r[3]] += 1
                elif kind == 3:
                    if len(r) < 5:
                        timesig_partial += 1
                        ex(examples, "timesig_partial", (path, r))
                    ts_vals[tuple(r[3:5])] += 1
                elif kind == 5:
                    hs_vals[r[3] if len(r) > 3 else None] += 1
                if kind not in KIND_NAMES:
                    ex(examples, "unknown_kind", (path, r))
                if kind in (6, 7, 8, 9, 10) or kind == 5:
                    ex(examples, "kind_%d" % kind, (path, r), 4)
        for k in kinds_here:
            files_with["kind%d" % k] += 1
        for nt in types_here:
            note_files[nt] += 1

        # index checks
        if order and order != list(range(len(order))):
            if sorted(order) == list(range(len(order))):
                index_issues["indices_permuted"] += 1
            else:
                index_issues["indices_noncontiguous_files"] += 1
                ex(examples, "noncontig_index", path)

        # EOC
        eoc_per_file[len(eocs)] += 1
        if eocs:
            e = max(eocs)
            after = [1 for (t, nt, r) in notes.values() if t > e and nt != 14]
            if after:
                notes_after_eoc += len(after)
                files_notes_after_eoc += 1

        # first events
        g0 = [g for g in gimmicks if g[0] == 0]
        if not any(g[1] == 2 for g in g0) or not any(g[1] == 3 for g in g0):
            first_event_issue += 1
            ex(examples, "missing_tick0_bpm_or_ts", path)
        bt = Counter(g[0] for g in gimmicks if g[1] == 2)
        multi_bpm_same_tick += sum(1 for v in bt.values() if v > 1)

        # reverse triplets / stops
        rs = sorted(g[0] for g in gimmicks if g[1] == 6)
        re_ = sorted(g[0] for g in gimmicks if g[1] == 7)
        rn = sorted(g[0] for g in gimmicks if g[1] == 8)
        if rs or re_ or rn:
            if len(rs) == len(re_) == len(rn) and all(a <= b <= c for a, b, c in zip(rs, re_, rn)):
                rev_triplet_ok += 1
                ex(examples, "reverse_triplet_ticks", (path, list(zip(rs, re_, rn))), 6)
            else:
                rev_triplet_bad += 1
                ex(examples, "reverse_bad", (path, rs, re_, rn))
        ss = sorted(g[0] for g in gimmicks if g[1] == 9)
        se = sorted(g[0] for g in gimmicks if g[1] == 10)
        if ss or se:
            if len(ss) == len(se) and all(a <= b for a, b in zip(ss, se)):
                stop_pairs_ok += 1
            else:
                stop_pairs_bad += 1
                ex(examples, "stop_bad", (path, ss, se))

        # holds
        referenced = set()
        for idx, (t, nt, r) in notes.items():
            if nt in (9, 10, 25):
                if len(r) < 9:
                    hold_issues["start/seg_missing_next"] += 1
                    ex(examples, "hold_missing_next", (path, r))
                    continue
                nxt = r[8]
                if nxt in referenced:
                    hold_issues["next_referenced_twice"] += 1
                referenced.add(nxt)
                if nxt not in notes:
                    hold_issues["next_missing_target"] += 1
                    ex(examples, "hold_dangling", (path, r))
                    continue
                tn, ntn, rn_ = notes[nxt]
                if ntn not in (10, 11):
                    hold_issues["next_not_seg_or_end"] += 1
                    ex(examples, "hold_next_badtype", (path, r, rn_))
                if tn < t:
                    seg_backward += 1
                    ex(examples, "hold_backward", (path, r, rn_))
                elif tn == t:
                    seg_same_tick += 1
                    ex(examples, "hold_same_tick", (path, r, rn_))
                d = (rn_[5] - r[5]) % 60
                d = min(d, 60 - d)
                hold_seg_pos_jump[min(d, 30) // 5 * 5] += 1
                if abs(rn_[5] - r[5]) > 30:
                    hold_wrap += 1
            if nt == 11 and len(r) > 8:
                hold_issues["end_has_next"] += 1
            if nt == 10 and len(r) > 7 and r[7] == 0:
                hold_seg_flag0 += 1
        for idx, (t, nt, r) in notes.items():
            if nt in (10, 11) and idx not in referenced:
                hold_issues["orphan_seg_or_end"] += 1
                ex(examples, "hold_orphan", (path, r))
        # chain walk from starts
        for idx, (t, nt, r) in notes.items():
            if nt in (9, 25):
                n = 1
                cur = r
                seen = {idx}
                while cur[3] != 11:
                    if len(cur) < 9 or cur[8] not in notes:
                        hold_issues["chain_broken"] += 1
                        break
                    if cur[8] in seen:
                        hold_issues["chain_cycle"] += 1
                        break
                    seen.add(cur[8])
                    cur = notes[cur[8]][2]
                    n += 1
                hold_lengths[min(n, 50)] += 1

        # duration via simple tempo/timesig integration
        mets = sorted(((g[0], g[1], g[2]) for g in gimmicks if g[1] in (2, 3)), key=lambda x: (x[0], x[1]))
        if eocs and mets:
            bpm, num, den = None, 4, 4
            last_t = 0
            sec = 0.0
            for (t, k, r) in mets:
                if t > max(eocs):
                    break
                if bpm is not None:
                    sec += (t - last_t) / TPM * (4.0 * num / den) * 60.0 / bpm
                last_t = t
                if k == 2:
                    bpm = r[3]
                else:
                    num = int(r[3])
                    den = int(r[4]) if len(r) > 4 else 4
            sec += (max(eocs) - last_t) / TPM * (4.0 * num / den) * 60.0 / bpm
            durations.append((sec, path))

    P = print
    P("files:", len(files), " songs:", len({s for s, _ in files}), " rows:", total_rows)
    P("difficulty suffix counts:", dict(sorted(suffix.items())))
    for k in sorted(songs_by_suffix):
        if len(songs_by_suffix[k]) < 50:
            P("  songs with suffix", k, ":", sorted(songs_by_suffix[k]))
    P("UTF-16 files:", utf16_files)
    P("BOM files:", bom_files, " CRLF files:", crlf_files, " non-numeric body lines:", nonint_lines, " blank body lines:", blank_body, " unsorted files:", unsorted_files)
    P("\nheader tags:", dict(header_tags))
    for tag, c in header_vals.items():
        top = c.most_common(8)
        P(" ", tag, "distinct:", len(c), "top:", top)
    if offsets:
        P("  OFFSET min/max:", min(offsets), max(offsets))
    P("\nrow kinds (col3):")
    for k in sorted(kind_count):
        P("  %2d %-20s rows=%7d files=%4d colcounts=%s" % (k, KIND_NAMES.get(k, "?"), kind_count[k], files_with["kind%d" % k], dict(kind_cols[k])))
    P("\nnote types (col4 when kind=1):")
    for nt in sorted(note_count):
        P("  %2d %-14s rows=%7d files=%4d colcounts=%s col8=%s" % (nt, NOTE_NAMES.get(nt, "?"), note_count[nt], note_files[nt], dict(note_cols[nt]), dict(note_flag[nt])))
    P("\nmask direction (col9 for 12/13):", dict(mask_dir))
    P("position range:", pos_range, " tick max:", tick_max, " ticks out of range:", bad_tick)
    for nt in sorted(size_by_type):
        c = size_by_type[nt]
        P("  size type %2d: min %d max %d top %s" % (nt, min(c), max(c), c.most_common(5)))
    P("\nBPM distinct:", len(bpm_vals), " min/max:", min(bpm_vals), max(bpm_vals))
    P("time signatures:", dict(ts_vals), " partial (numerator only):", timesig_partial)
    P("hi-speed values:", sorted(hs_vals.items())[:40], " distinct:", len(hs_vals))
    P("multiple BPM rows at same tick (count):", multi_bpm_same_tick, " files missing tick-0 BPM or TS:", first_event_issue)
    P("reverse triplets ok files:", rev_triplet_ok, " bad:", rev_triplet_bad)
    P("stop pairs ok files:", stop_pairs_ok, " bad:", stop_pairs_bad)
    P("\nEOC per file:", dict(eoc_per_file), " notes after EOC:", notes_after_eoc, "in files:", files_notes_after_eoc)
    P("index issues:", dict(index_issues))
    P("\nhold issues:", dict(hold_issues))
    P("hold chain lengths (notes incl start/end, capped 50):", sorted(hold_lengths.items()))
    P("hold segments with col8=0:", hold_seg_flag0, " links backward in time:", seg_backward, " links at same tick:", seg_same_tick)
    P("hold link position delta histogram (bucket of 5 lanes):", sorted(hold_seg_pos_jump.items()), " raw |dpos|>30 (wrap):", hold_wrap)
    if durations:
        durations.sort()
        P("\nchart duration to EOC (s): min %.1f %s / max %.1f %s / median %.1f" % (
            durations[0][0], durations[0][1], durations[-1][0], durations[-1][1], durations[len(durations)//2][0]))
    P("\nexamples:")
    for k, v in examples.items():
        P(" ", k)
        for e in v:
            P("     ", e)


if __name__ == "__main__":
    main()
