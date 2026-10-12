#!/usr/bin/env python3
"""UE4.19 cooked Texture2D reader -> PNG (stdlib only, read-only on inputs).

Usage:
  python -I ue4_texture.py survey <ContentDir>            # pixel-format / size / bulk stats
  python -I ue4_texture.py png <in.uasset> <out.png> [--mip N]

Cooked layout used (UE 4.19, split .uasset/.uexp, unversioned):
  [tagged UObject properties] FStripDataFlags(2) bCooked(u32)
  FName PixelFormat  int32 SkipOffset            (int32 in 4.19; int64 from 4.20)
  FTexturePlatformData: SizeX SizeY NumSlices FString PixelFormat FirstMip NumMips
    per mip: bCooked(i32) FByteBulkData{u32 Flags, i32 ElementCount, i32 SizeOnDisk,
             i64 OffsetInFile, [payload if inline]} SizeX SizeY
  FName None (end of formats)
BulkData flags: 0x40 ForceInlinePayload, 0x100 PayloadInSeparateFile (.ubulk),
  0x20 Unused (mip stripped), 0x1 PayloadAtEndOfFile.
Supported decode: PF_B8G8R8A8, PF_G8, PF_DXT1 (BC1), PF_DXT5 (BC3).
References: UE4 source Texture.cpp/TextureDerivedData.cpp; CUE4Parse
  (https://github.com/FabianFG/CUE4Parse) UTexture2D.cs; UModel (https://github.com/gildor2/UEViewer).
"""
import os
import re
import struct
import sys
import zlib

PF_RE = re.compile(rb"\x00\x00\x00(PF_[A-Za-z0-9_]+)\x00")


def find_platform_data(ux):
    m = PF_RE.search(ux)
    if not m:
        return None
    fs_start = m.start(1) - 4  # int32 length precedes the string
    n, = struct.unpack_from("<i", ux, fs_start)
    pf = ux[fs_start + 4:fs_start + 4 + n - 1].decode()
    sx, sy, ns = struct.unpack_from("<iii", ux, fs_start - 12)
    p = fs_start + 4 + n
    first_mip, nmips = struct.unpack_from("<ii", ux, p)
    p += 8
    mips = []
    for _ in range(nmips):
        cooked, flags, count, disk, off = struct.unpack_from("<iIiiq", ux, p)
        p += 24
        inline = None
        if not (flags & 0x100) and not (flags & 0x20) and not (flags & 0x1):
            inline = (p, disk)
            p += disk
        mx, my = struct.unpack_from("<ii", ux, p)
        p += 8
        mips.append(dict(flags=flags, count=count, disk=disk, off=off, inline=inline, w=mx, h=my))
    return dict(pf=pf, w=sx, h=sy, slices=ns, first_mip=first_mip, mips=mips)


def mip_bytes(uasset_path, ux, mip):
    if mip["inline"]:
        o, n = mip["inline"]
        return ux[o:o + n]
    if mip["flags"] & 0x100:
        ub = os.path.splitext(uasset_path)[0] + ".ubulk"
        # OffsetInFile is relative to the end of the combined package (uasset+uexp minus the
        # 4-byte trailing package tag), i.e. ubulk_pos = off + len(uasset) + len(uexp) - 4.
        base = os.path.getsize(uasset_path) + len(ux) - 4
        with open(ub, "rb") as f:
            f.seek(mip["off"] + base)
            return f.read(mip["disk"])
    if mip["flags"] & 0x1:  # payload at end of the .uexp; offset is relative to package start
        hdr = os.path.getsize(uasset_path)
        o = mip["off"] - hdr
        return ux[o:o + mip["disk"]]
    return None


def _rgb565(c):
    r = (c >> 11) & 31
    g = (c >> 5) & 63
    b = c & 31
    return [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]


def _bc1_colors(c0, c1, force4):
    a, b = _rgb565(c0), _rgb565(c1)
    if c0 > c1 or force4:
        return [a + [255], b + [255],
                [(2 * a[i] + b[i]) // 3 for i in range(3)] + [255],
                [(a[i] + 2 * b[i]) // 3 for i in range(3)] + [255]]
    return [a + [255], b + [255], [(a[i] + b[i]) // 2 for i in range(3)] + [255], [0, 0, 0, 0]]


def decode(pf, w, h, data):
    out = bytearray(w * h * 4)
    if pf == "PF_B8G8R8A8":
        for i in range(w * h):
            b, g, r, a = data[i * 4:i * 4 + 4]
            out[i * 4:i * 4 + 4] = bytes((r, g, b, a))
        return out
    if pf == "PF_G8":
        for i in range(w * h):
            v = data[i]
            out[i * 4:i * 4 + 4] = bytes((v, v, v, 255))
        return out
    if pf in ("PF_DXT1", "PF_DXT5"):
        bs = 8 if pf == "PF_DXT1" else 16
        bw, bh = (w + 3) // 4, (h + 3) // 4
        for by in range(bh):
            for bx in range(bw):
                blk = data[(by * bw + bx) * bs:(by * bw + bx + 1) * bs]
                alpha = None
                if bs == 16:
                    a0, a1 = blk[0], blk[1]
                    bits = int.from_bytes(blk[2:8], "little")
                    if a0 > a1:
                        pal = [a0, a1] + [((7 - i) * a0 + i * a1) // 7 for i in range(1, 7)]
                    else:
                        pal = [a0, a1] + [((5 - i) * a0 + i * a1) // 5 for i in range(1, 5)] + [0, 255]
                    alpha = [pal[(bits >> (3 * i)) & 7] for i in range(16)]
                    blk = blk[8:]
                c0, c1 = struct.unpack_from("<HH", blk, 0)
                cols = _bc1_colors(c0, c1, bs == 16)
                idx, = struct.unpack_from("<I", blk, 4)
                for i in range(16):
                    x, y = bx * 4 + (i & 3), by * 4 + (i >> 2)
                    if x >= w or y >= h:
                        continue
                    c = list(cols[(idx >> (2 * i)) & 3])
                    if alpha is not None:
                        c[3] = alpha[i]
                    o = (y * w + x) * 4
                    out[o:o + 4] = bytes(c)
        return out
    raise ValueError("unsupported pixel format " + pf)


def write_png(path, w, h, rgba):
    raw = b"".join(b"\x00" + bytes(rgba[y * w * 4:(y + 1) * w * 4]) for y in range(h))

    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


def cmd_png(uasset, out, mip_i=0):
    with open(os.path.splitext(uasset)[0] + ".uexp", "rb") as f:
        ux = f.read()
    info = find_platform_data(ux)
    if not info:
        raise SystemExit("no texture platform data found")
    mips = [m for m in info["mips"] if mip_bytes(uasset, ux, m)]
    m = mips[min(mip_i, len(mips) - 1)]
    data = mip_bytes(uasset, ux, m)
    rgba = decode(info["pf"], m["w"], m["h"], data)
    write_png(out, m["w"], m["h"], rgba)
    print("%s: %s %dx%d (mip %dx%d, %d mips) -> %s" % (os.path.basename(uasset), info["pf"], info["w"],
          info["h"], m["w"], m["h"], len(info["mips"]), out))


def cmd_survey(root):
    import collections
    fmt = collections.defaultdict(collections.Counter)
    sizes = collections.Counter()
    bulk = collections.Counter()
    for d, _, fs in os.walk(root):
        rel = os.path.relpath(d, root).replace("\\", "/").split("/")
        if rel[0] in ("Sound", "Movie"):
            continue
        group = "/".join(rel[:2])
        for f in fs:
            if not f.endswith(".uexp"):
                continue
            p = os.path.join(d, f)
            with open(p, "rb") as fh:
                ux = fh.read()
            try:
                info = find_platform_data(ux)
            except struct.error:
                info = None
            if not info:
                continue
            fmt[group][info["pf"]] += 1
            sizes["%dx%d" % (info["w"], info["h"])] += 1
            for m in info["mips"][:1]:
                bulk["ubulk" if m["flags"] & 0x100 else ("inline" if m["inline"] else "flags=0x%x" % m["flags"])] += 1
    for g in sorted(fmt):
        print("%-28s %s" % (g, dict(fmt[g])))
    print("top sizes:", sizes.most_common(15))
    print("mip0 storage:", dict(bulk))


if __name__ == "__main__":
    a = sys.argv[1:]
    if a and a[0] == "survey":
        cmd_survey(a[1])
    elif a and a[0] == "png":
        mip = int(a[a.index("--mip") + 1]) if "--mip" in a else 0
        cmd_png(a[1], a[2], mip)
    else:
        print(__doc__)
