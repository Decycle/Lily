"""Dump ASCII and UTF-16LE strings from a UE4 asset (read-only).
Usage: python -I dump_strings.py <file> [regex-filter]"""
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
b = open(sys.argv[1], "rb").read()
flt = re.compile(sys.argv[2]) if len(sys.argv) > 2 else None
out = []
for m in re.finditer(rb"(?:[\x20-\x7e]{3,})|(?:(?:[\x20-\x7e]\x00|[\x00-\xff][\x30-\x9f]){2,})", b):
    s = m.group()
    try:
        t = s.decode("utf-16-le") if len(s) % 2 == 0 and b"\x00" in s else s.decode("ascii")
    except Exception:
        t = s.decode("latin-1")
    if flt is None or flt.search(t):
        out.append(t)
print(" | ".join(out))
