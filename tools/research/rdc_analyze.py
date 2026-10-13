#!/usr/bin/env python3
"""Annotate an rdc_dump.py output: map each draw's VS/PS DXBC to the material + shader type found by
ue4_shaders.py (sha1 match), print the draw list per render pass, write `annotated.json`.

Usage: python -I rdc_analyze.py <dump_dir> <shader_root>     (shader_root = runs/shaders, all */index.json used)
       python -I rdc_analyze.py <dump_dir> <shader_root> --cb <eid> <vs|ps> <slot> [rows]   (print a cbuffer as float4 rows)
"""
import glob
import json
import os
import struct
import sys


def shader_map(root):
    m = {}
    for idx in glob.glob(os.path.join(root, "*", "index.json")):
        for pkg, v in json.load(open(idx)).items():
            for s in v["shaders"]:
                m.setdefault(s["sha1"], []).append("%s/%s" % (pkg, s["file"]))
    return m


def _find_set(root, name):
    """The extraction set (subdir of root) that holds package `name`'s shaders."""
    pkg = name.split("/")[0]
    for s in sorted(os.listdir(root)):
        if os.path.isdir(os.path.join(root, s, pkg)):
            return s
    return ""


def short(names):
    if not names:
        return "?"
    pkgs = sorted({n.split("/")[0] for n in names})
    kind = names[0].split("/")[1].split("_", 2)[-1]
    return "%s:%s" % (",".join(pkgs[:3]) + ("+%d" % (len(pkgs) - 3) if len(pkgs) > 3 else ""), kind[:50])


def cb_rows(dump, blob, rows=None):
    data = open(os.path.join(dump, "cbuffers", blob + ".bin"), "rb").read()
    n = len(data) // 16 if rows is None else min(rows, len(data) // 16)
    return [struct.unpack_from("<4f", data, 16 * i) for i in range(n)]


def main(argv):
    dump, root = argv[0], argv[1]
    draws = json.load(open(os.path.join(dump, "draws.json")))
    if "--cb" in argv:
        i = argv.index("--cb")
        eid, stage, slot = int(argv[i + 1]), argv[i + 2], int(argv[i + 3])
        rows = int(argv[i + 4]) if len(argv) > i + 4 else None
        d = next(x for x in draws if x["eid"] == eid)
        cb = next(c for c in d[stage]["cbs"] if c["slot"] == slot)
        for r, v in enumerate(cb_rows(dump, cb["blob"], rows)):
            print("[%3d] % .6g % .6g % .6g % .6g" % ((r,) + v))
        return 0
    sm = shader_map(root)
    if "--annotate" in argv:
        # write <dump>/annot/<eid>_<vs|ps>.asm: the disassembly with every cbN[i].c operand followed by the value
        # captured at that draw (and the SRV list), so shaders can be translated with the real constants.
        import re
        eid = int(argv[argv.index("--annotate") + 1])
        d = next(x for x in draws if x["eid"] == eid)
        os.makedirs(os.path.join(dump, "annot"), exist_ok=True)
        for stage in ("vs", "ps"):
            names = sm.get(d[stage].get("dxbc"), [])
            if not names:
                continue
            asm = open(os.path.join(root, _find_set(root, names[0]),
                                    names[0] + ".asm"), encoding="utf-8").read()
            cbs = {c["slot"]: cb_rows(dump, c["blob"]) for c in d[stage]["cbs"]}

            def val(m):
                slot, row, comps = int(m.group(1)), int(m.group(2)), m.group(3) or "xyzw"
                rows = cbs.get(slot)
                if rows is None or row >= len(rows):
                    return m.group(0)
                v = ",".join("%.6g" % rows[row]["xyzw".index(c)] for c in comps[:4])
                return "%s{%s}" % (m.group(0), v)
            out = re.sub(r"cb(\d+)\[(\d+)\]\.?([xyzw]*)", val, asm)
            head = "// eid %d %s: %s\n// SRVs: %s\n// samplers: %s\n" % (
                eid, stage, names[:3], [(s["slot"], s["res"], s.get("w"), s.get("h"), s.get("fmt"))
                                        for s in d[stage].get("srvs", [])], d[stage].get("samplers"))
            p = os.path.join(dump, "annot", "%d_%s.asm" % (eid, stage))
            with open(p, "w", encoding="utf-8") as f:
                f.write(head + out)
            print(p)
        return 0
    if "--prims" in argv:
        # one line per (material, primitive transform): LocalToWorld decoded as uniform scale, translation,
        # rotation about world Z/Y; consecutive section draws of one mesh are merged.
        last = None
        for d in draws:
            if "vs" not in d or not d.get("rts"):
                continue
            mat = short(sm.get(d["ps"].get("dxbc"), []))
            if "BasePass" not in mat:
                continue
            cb = next((c for c in d["vs"]["cbs"] if c["slot"] == 2), None)
            if cb is None:
                continue
            m = cb_rows(dump, cb["blob"], 4)
            sx = (m[0][0] ** 2 + m[0][1] ** 2 + m[0][2] ** 2) ** 0.5
            sy = (m[1][0] ** 2 + m[1][1] ** 2 + m[1][2] ** 2) ** 0.5
            import math
            ang = math.degrees(math.atan2(-m[0][2], m[0][0])) % 360
            key = (mat, tuple(round(v, 4) for r in m for v in r))
            if key == last:
                continue
            last = key
            b = d.get("blend", [{}])[0]
            print("%6d %-60s s=%.5f/%.5f T=(%.2f, %.3f, %.2f) rotY=%.2f idx=%d blend=%s" % (
                d["eid"], mat[:60], sx, sy, m[3][0], m[3][1], m[3][2], ang, d.get("indices", 0),
                "opaque" if not b.get("enabled") else b["color"][0] + "/" + b["color"][1]))
        return 0
    texinfo = json.load(open(os.path.join(dump, "textures.json")))
    last_rt = None
    for d in draws:
        if "vs" not in d:
            print("%6d %s" % (d["eid"], d["name"]))
            continue
        d["vs_mat"] = sm.get(d["vs"].get("dxbc"), [])
        d["ps_mat"] = sm.get(d["ps"].get("dxbc"), [])
        rt = tuple(d.get("rts", []))
        if rt != last_rt:
            descr = ["%s %sx%s %s" % (r, texinfo.get(str(r), {}).get("w"), texinfo.get(str(r), {}).get("h"),
                                       texinfo.get(str(r), {}).get("fmt")) for r in rt]
            print("---- RT %s dsv %s vp %s" % (descr, d.get("dsv"), d.get("viewport")))
            last_rt = rt
        b = d.get("blend", [{}])[0]
        bl = "opaque" if not b.get("enabled") else "%s/%s" % (b["color"][0], b["color"][1])
        dp = d.get("depth", {})
        ds = ("z%s%s" % (dp.get("func", "")[:2], "w" if dp.get("write") else "")) if dp.get("enable") else "noz"
        tex = ",".join("%s(%sx%s)" % (s["res"], s.get("w"), s.get("h")) for s in d["ps"].get("srvs", [])
                       if s.get("dim") == 2)
        print("%6d %5d x%-3d %-14s %-7s %-6s PS %s | tex %s" % (
            d["eid"], d.get("indices", 0), d.get("instances", 0), bl, ds, d.get("raster", {}).get("cull", "")[:6],
            short(d["ps_mat"]), tex[:60]))
    with open(os.path.join(dump, "annotated.json"), "w") as f:
        json.dump(draws, f, indent=0)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
