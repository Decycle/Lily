"""Inspect / extract WACCA jacket textures (UE4 cooked Texture2D, .uasset + .uexp [+ .ubulk]).

Usage:
  python -I jacket_extract.py survey <JACKET dir>              # stats over all jackets
  python -I jacket_extract.py info   <path/uT_J_S00_000.uexp>
  python -I jacket_extract.py png    <path/uT_J_S00_000.uexp> <out.png>

Cooked FTexturePlatformData layout found in the .uexp (after the tagged UObject properties
and the 'None' terminator + strip flags), little-endian:
  i32 SizeX, i32 SizeY, u32 PackedData (NumSlices | bHasOptData<<30 | bIsCubemap<<31),
  FString PixelFormat ("PF_B8G8R8A8"), i32 FirstMipToSerialize, i32 NumMips,
  per mip: i32 bCooked, FByteBulkData{u32 BulkDataFlags, i32 ElementCount, i32 SizeOnDisk,
           i64 OffsetInFile, [payload if inline]}, i32 SizeX, i32 SizeY
BulkDataFlags: 0x40 ForceInlinePayload (payload follows), 0x100 PayloadInSeparateFile (.ubulk),
0x20 Unused (no payload / mip stripped), 0x1 StoreInSeparateFile. Inline OffsetInFile is relative
to the start of the *combined* uasset+uexp; for .ubulk (flags 0x501) it is relative to
Summary.BulkDataStartOffset (add uasset+uexp-4 to get the .ubulk position).
This locator is heuristic: it finds the 'PF_' FString and walks from there.
"""
import os
import struct
import sys
import zlib

BPP = {"PF_B8G8R8A8": 4, "PF_R8G8B8A8": 4, "PF_G8": 1}
BLOCK = {"PF_DXT1": 8, "PF_DXT5": 16, "PF_BC5": 16, "PF_BC7": 16, "PF_BC4": 8}


def parse(uexp_path, prefer=("PF_B8G8R8A8",)):
    """A cooked texture may carry several FTexturePlatformData blocks (one per cooked pixel
    format, loop terminated by FName 'None'); return the preferred one, else the first."""
    b = open(uexp_path, "rb").read()
    found, i = [], b.find(b"PF_")
    while i >= 0:
        try:
            found.append(parse_at(uexp_path, b, i))
        except (struct.error, UnicodeDecodeError):
            pass
        i = b.find(b"PF_", i + 3)
    if not found:
        raise ValueError("no PF_ string")
    for f in found:
        f["all_formats"] = [x["fmt"] + "@%d" % x["w"] for x in found]
    for f in found:
        if f["fmt"] in prefer:
            return f
    return found[0]


def parse_at(uexp_path, b, i):
    uasset_size = os.path.getsize(uexp_path[:-5] + ".uasset")
    slen, = struct.unpack_from("<i", b, i - 4)
    fmt = b[i:i + slen - 1].decode()
    sx, sy, packed = struct.unpack_from("<iiI", b, i - 16)
    p = i + slen
    first_mip, nmips = struct.unpack_from("<ii", b, p)
    p += 8
    mips = []
    for m in range(nmips):
        cooked, flags, count, size, off = struct.unpack_from("<iIiiq", b, p)
        p += 24
        where = "?"
        payload_pos = None
        if flags & 0x20:
            where = "none"
        elif flags & 0x100 or flags & 0x1:
            where = "ubulk"
        else:
            where = "inline"
            payload_pos = p
            p += size
        mx, my = struct.unpack_from("<ii", b, p)
        p += 8
        mips.append(dict(cooked=cooked, flags=flags, count=count, size=size, offset=off,
                         where=where, payload_pos=payload_pos, w=mx, h=my,
                         inline_offset_ok=(where != "inline" or off == uasset_size + payload_pos)))
    return dict(path=uexp_path, fmt=fmt, w=sx, h=sy, slices=packed & 0x3FFFFFFF,
                first_mip=first_mip, mips=mips, data=b, tail=b[p:p + 16].hex())


def mip_bytes(info, idx):
    m = info["mips"][idx]
    if m["where"] == "inline":
        return info["data"][m["payload_pos"]:m["payload_pos"] + m["size"]]
    if m["where"] == "ubulk":
        # OffsetInFile for .ubulk payloads is stored relative to the package summary's
        # BulkDataStartOffset (== uasset+uexp size - 4 here: 1014+265782-4 = 266792 for
        # uT_J_S03_52); observed values: mip0 -266792 -> 0 in .ubulk.
        base = os.path.getsize(info["path"][:-5] + ".uasset") + os.path.getsize(info["path"]) - 4
        with open(info["path"][:-5] + ".ubulk", "rb") as f:
            f.seek(m["offset"] + base)
            return f.read(m["size"])
    return None


def write_png(path, w, h, rgba):
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    stride = w * 4
    raw = b"".join(b"\0" + rgba[y * stride:(y + 1) * stride] for y in range(h))
    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)


def bgra_to_rgba(px):
    out = bytearray(px)
    out[0::4], out[2::4] = px[2::4], px[0::4]
    return bytes(out)


def main():
    cmd = sys.argv[1]
    if cmd == "survey":
        stats, odd = {}, []
        for root, _, files in os.walk(sys.argv[2]):
            for fn in sorted(files):
                if not fn.endswith(".uexp"):
                    continue
                pth = os.path.join(root, fn)
                try:
                    inf = parse(pth)
                except Exception as e:  # noqa: BLE001
                    odd.append((fn, "ERR %s" % e))
                    continue
                key = (inf["fmt"], inf["w"], inf["h"], len(inf["mips"]),
                       "/".join("%s:0x%x" % (m["where"], m["flags"]) for m in inf["mips"])
                       + "  formats=" + ",".join(inf["all_formats"]))
                stats[key] = stats.get(key, 0) + 1
                if not all(m["inline_offset_ok"] for m in inf["mips"]) or len(inf["all_formats"]) > 1:
                    odd.append((fn, key, [(m["w"], m["h"], m["size"], m["offset"]) for m in inf["mips"]]))
        for k, v in sorted(stats.items(), key=lambda kv: -kv[1]):
            print("%5d  fmt=%s %dx%d mips=%d  %s" % ((v,) + k))
        for o in odd:
            print("note:", o)
    elif cmd == "info":
        inf = parse(sys.argv[2])
        print({k: v for k, v in inf.items() if k != "data"})
    elif cmd == "png":
        inf = parse(sys.argv[2])
        # pick the largest mip that actually has data
        for idx, m in enumerate(inf["mips"]):
            px = mip_bytes(inf, idx)
            if px:
                break
        if inf["fmt"] not in ("PF_B8G8R8A8", "PF_R8G8B8A8"):
            raise SystemExit("format %s needs a BCn decoder (not in this prototype)" % inf["fmt"])
        if len(px) != m["w"] * m["h"] * 4:
            raise SystemExit("size mismatch %d vs %dx%dx4" % (len(px), m["w"], m["h"]))
        rgba = bgra_to_rgba(px) if inf["fmt"] == "PF_B8G8R8A8" else px
        write_png(sys.argv[3], m["w"], m["h"], rgba)
        print("wrote %s (%dx%d, %s, mip %d, %s)" % (sys.argv[3], m["w"], m["h"], inf["fmt"], idx, m["where"]))


if __name__ == "__main__":
    main()
