"""Dump schema (and first rows) of the @UTF header table and all nested tables in a file.
Usage: python -I utf_dump.py <file.uexp|.acb|.acf> [max_rows]"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cri_utf import UtfTable, find_utf  # noqa: E402

TYPES = "u8 s8 u16 s16 u32 s32 u64 s64 f32 f64 str data u128".split()
STOR = {0x10: "zero", 0x30: "const", 0x50: "row", 0x70: "const2"}


def show(t, indent, maxrows):
    pad = "  " * indent
    print("%s[%s] rows=%d cols=%d" % (pad, t.name, len(t.rows), len(t.columns)))
    for (n, s, ty, c) in t.columns:
        cs = ""
        if c is not None:
            cs = " = %r" % (c if not isinstance(c, bytes) else "<%d bytes>" % len(c))
        print("%s  %-28s %-6s %s%s" % (pad, n, STOR.get(s, hex(s)), TYPES[ty], cs))
    for i, r in enumerate(t.rows[:maxrows]):
        items = []
        for k, v in r.items():
            if isinstance(v, bytes):
                v = ("<@UTF %d>" % len(v)) if v[:4] == b"@UTF" else (v[:16].hex() + ("..(%d)" % len(v) if len(v) > 16 else ""))
            items.append("%s=%s" % (k, v))
        print("%s  row%d: %s" % (pad, i, ", ".join(items)))
    for (n, s, ty, c) in t.columns:
        if ty == 0xB and t.rows:
            d = t.rows[0][n]
            if d and d[:4] == b"@UTF" and indent < 3:
                print("%s  -> %s:" % (pad, n))
                show(UtfTable(d), indent + 2, maxrows)


def main():
    data = open(sys.argv[1], "rb").read()
    maxrows = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    show(UtfTable(data, find_utf(data)), 0, maxrows)


if __name__ == "__main__":
    main()
