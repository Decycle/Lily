"""Heuristic UE4 tagged-property scan of a DataTable .uexp (read-only).
Finds 4-byte scalar properties (size=4, arrayIndex=0, hasGuid=0) and prints
(nameIndex, typeIndex, int, float). Name indices refer to the .uasset name map;
the name map is parsed from the package summary (UE4.2x layout, best effort).
Usage: python -I peek_props.py <dir> <TableName> [...]
"""
import struct
import sys
from pathlib import Path


def read_fstring(b, o):
    n = struct.unpack_from("<i", b, o)[0]
    o += 4
    if n == 0:
        return "", o
    if n > 0:
        s = b[o:o + n - 1].decode("latin-1")
        return s, o + n
    n = -n
    s = b[o:o + 2 * n - 2].decode("utf-16-le")
    return s, o + 2 * n


def name_map(ua):
    # Find NameCount/NameOffset: brute-force for an offset where names parse cleanly.
    for hdr in range(0, 200, 4):
        cnt, off = struct.unpack_from("<ii", ua, hdr)
        if not (3 < cnt < 2000 and 20 < off < len(ua)):
            continue
        try:
            o = off
            names = []
            for _ in range(cnt):
                s, o = read_fstring(ua, o)
                if not s or not s.isprintable():
                    raise ValueError
                o += 4  # hash
                names.append(s)
            return names
        except Exception:
            continue
    return []


def main():
    d = Path(sys.argv[1])
    for t in sys.argv[2:]:
        ua = (d / f"{t}.uasset").read_bytes()
        ux = (d / f"{t}.uexp").read_bytes()
        names = name_map(ua)
        nm = lambda i: names[i] if 0 <= i < len(names) else f"#{i}"
        print(f"== {t}")
        pat = b"\x04\x00\x00\x00\x00\x00\x00\x00\x00"
        i = ux.find(pat)
        while i != -1:
            if i >= 16:
                ni, _, ti, _ = struct.unpack_from("<iiii", ux, i - 16)
                iv = struct.unpack_from("<i", ux, i + 9)[0]
                fv = struct.unpack_from("<f", ux, i + 9)[0]
                tn = nm(ti)
                val = f"{fv:g}" if tn == "FloatProperty" else str(iv if tn != "UInt32Property" else iv & 0xffffffff)
                print(f"  {nm(ni)} ({tn}) = {val}")
            i = ux.find(pat, i + 1)
        # string properties
        print()


if __name__ == "__main__":
    main()
