"""Dump length-prefixed UE4 FStrings (ASCII or UTF-16) from an asset (read-only).
Usage: python -I dump_fstrings.py <file>"""
import struct
import sys

sys.stdout.reconfigure(encoding="utf-8")
b = open(sys.argv[1], "rb").read()
i = 0
out = []
while i < len(b) - 4:
    n = struct.unpack_from("<i", b, i)[0]
    if 1 < n < 400 and i + 4 + n <= len(b) and b[i + 3 + n] == 0:
        s = b[i + 4:i + 3 + n]
        if all(0x20 <= c < 0x7f or c in (9, 10, 13) for c in s):
            out.append(s.decode()); i += 4 + n; continue
    if -400 < n < -1 and i + 4 - 2 * n <= len(b):
        s = b[i + 4:i + 4 - 2 * n]
        if s[-2:] == b"\0\0":
            try:
                t = s[:-2].decode("utf-16-le")
                if t.isprintable() or "\n" in t or "\r" in t:
                    out.append(t.replace("\r\n", "\n").replace("\n", "\n")); i += 4 - 2 * n; continue
            except UnicodeDecodeError:
                pass
    i += 1
print("\n".join(out))
