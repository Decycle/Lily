#!/usr/bin/env python3
"""Extract LOD0 of a cooked UE4.19 StaticMesh (.uasset/.uexp) to OBJ + JSON. Stdlib only, read-only on input.

Cooked FStaticMeshLODResources layout used (verified on WACCA's Vfx meshes):
  Sections: i32 n, n x {MaterialIndex, FirstIndex, NumTriangles, MinVertexIndex, MaxVertexIndex,
            bEnableCollision, bCastShadow} (7 x i32), then i32 (MaxDeviation/flags)
  FPositionVertexBuffer: Stride=12, NumVertices, bulk {elem 12, count, float3[]}
  FStaticMeshVertexBuffer: StripFlags(2B), NumTexCoords, NumVertices, bFullPrecisionUVs, bHighPrecisionTangents,
            bulk tangents {elem 8|16, n, (TangentX, TangentZ) packed u8x4 or i16x4}, bulk UVs {elem = one UV (4|8), NumVertices*NumTexCoords, interleaved per vertex}
  FColorVertexBuffer: StripFlags(2B), Stride, NumVertices, [bulk {4, n, BGRA8}] if NumVertices
  FRawStaticIndexBuffer: b32Bit (i32), bulk {1, bytes, u16|u32 []}
The position buffer is located by its header (12, N, 12, N); sections are read backwards from it.

Usage: python -I ue4_mesh.py <mesh.uasset> <out_prefix>     -> out_prefix.obj, out_prefix.json
Units: UE centimetres, left-handed (X forward, Y right, Z up); OBJ keeps raw UE coordinates.
"""
import json
import os
import re
import struct
import sys


def half(u):
    return struct.unpack("<e", struct.pack("<H", u))[0]


def unpack_normal(b):
    return [round(c / 127.5 - 1.0, 4) for c in b]


def parse(uexp):
    d = uexp
    m = None
    for m in re.finditer(rb"\x0c\x00\x00\x00(....)\x0c\x00\x00\x00\1", d, re.S):
        break
    if m is None:
        raise ValueError("no position buffer found")
    pos = m.start()
    nv = struct.unpack("<I", m.group(1))[0]
    # sections: walk backwards over 7-int records until the count matches
    sections = None
    for n in range(1, 256):
        start = pos - 4 - n * 28 - 4
        if start < 0:
            break
        if struct.unpack_from("<i", d, start)[0] == n:
            recs = [struct.unpack_from("<7i", d, start + 4 + i * 28) for i in range(n)]
            firsts = [sum(3 * q[2] for q in recs[:i]) for i in range(n)]
            if all(r[0] == i and r[1] == firsts[i] for i, r in enumerate(recs)):
                sections = [dict(material=r[0], first_index=r[1], triangles=r[2], min_vertex=r[3], max_vertex=r[4])
                            for r in recs]
                break
    p = pos + 16
    positions = [struct.unpack_from("<3f", d, p + 12 * i) for i in range(nv)]
    p += 12 * nv
    p += 2  # strip flags
    ntex, nv2, full_uv, high_tb = struct.unpack_from("<4I", d, p)
    p += 16
    assert nv2 == nv, (nv2, nv)
    telem, tcount = struct.unpack_from("<2I", d, p)
    p += 8
    tangents = []
    for i in range(tcount):
        if telem == 8:
            tx, tz = d[p + 8 * i:p + 8 * i + 4], d[p + 8 * i + 4:p + 8 * i + 8]
            tangents.append((unpack_normal(tx), unpack_normal(tz)))
        else:
            v = struct.unpack_from("<8h", d, p + 16 * i)
            tangents.append(([round(x / 32767, 4) for x in v[:4]], [round(x / 32767, 4) for x in v[4:]]))
    p += telem * tcount
    uelem, ucount = struct.unpack_from("<2I", d, p)
    p += 8
    uvs = []
    for i in range(ucount // ntex):  # bulk element = one UV; vertex i holds UV0..UV(n-1)
        if full_uv:
            f = struct.unpack_from("<%df" % (2 * ntex), d, p + uelem * ntex * i)
        else:
            f = [half(x) for x in struct.unpack_from("<%dH" % (2 * ntex), d, p + uelem * ntex * i)]
        uvs.append([[round(f[2 * k], 5), round(f[2 * k + 1], 5)] for k in range(ntex)])
    p += uelem * ucount
    p += 2  # color strip flags
    _stride, ncol = struct.unpack_from("<2I", d, p)
    p += 8
    colors = None
    if ncol:
        celem, ccount = struct.unpack_from("<2I", d, p)
        p += 8
        bgra = [d[p + 4 * i:p + 4 * i + 4] for i in range(ccount)]
        colors = [[c[2], c[1], c[0], c[3]] for c in bgra]  # FColor BGRA -> RGBA
        p += celem * ccount
    b32, ielem, ibytes = struct.unpack_from("<3I", d, p)
    p += 12
    fmt = "<%dI" % (ibytes // 4) if b32 else "<%dH" % (ibytes // 2)
    indices = list(struct.unpack_from(fmt, d, p))
    return dict(vertex_count=nv, sections=sections, positions=[[round(c, 4) for c in v] for v in positions],
                tangent_x=[t[0] for t in tangents], normals=[t[1] for t in tangents], uvs=uvs, colors=colors,
                indices=indices, num_texcoords=ntex)


def write_obj(mesh, path):
    with open(path, "w", encoding="utf-8") as f:
        f.write("# UE4 StaticMesh LOD0, raw UE coordinates (cm, X fwd, Y right, Z up); vt = UV0\n")
        for v in mesh["positions"]:
            f.write("v %g %g %g\n" % tuple(v))
        for uv in mesh["uvs"]:
            f.write("vt %g %g\n" % (uv[0][0], 1.0 - uv[0][1]))
        for n in mesh["normals"]:
            f.write("vn %g %g %g\n" % tuple(n[:3]))
        idx = mesh["indices"]
        for s in mesh["sections"] or [dict(material=0, first_index=0, triangles=len(idx) // 3)]:
            f.write("g section%d\n" % s["material"])
            for t in range(s["triangles"]):
                a, b, c = (idx[s["first_index"] + 3 * t + k] + 1 for k in range(3))
                f.write("f %d/%d/%d %d/%d/%d %d/%d/%d\n" % (a, a, a, b, b, b, c, c, c))


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 1
    with open(os.path.splitext(argv[0])[0] + ".uexp", "rb") as f:
        mesh = parse(f.read())
    write_obj(mesh, argv[1] + ".obj")
    with open(argv[1] + ".json", "w", encoding="utf-8") as f:
        json.dump(mesh, f)
    xs = [v[0] for v in mesh["positions"]]
    ys = [v[1] for v in mesh["positions"]]
    zs = [v[2] for v in mesh["positions"]]
    print("%s: %d verts, %d tris, %d uv sets, sections %s, bounds x[%.2f,%.2f] y[%.2f,%.2f] z[%.2f,%.2f]" % (
        os.path.basename(argv[0]), mesh["vertex_count"], len(mesh["indices"]) // 3, mesh["num_texcoords"],
        [s["triangles"] for s in mesh["sections"] or []], min(xs), max(xs), min(ys), max(ys), min(zs), max(zs)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
