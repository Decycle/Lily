"""Extract ASCII and UTF-16LE strings from a binary and print those matching a regex.

Usage: python -I grep_strings.py <binary> <regex> [minlen]
Read-only.
"""
import re
import sys


def main():
    path, pat = sys.argv[1], re.compile(sys.argv[2], re.I)
    minlen = int(sys.argv[3]) if len(sys.argv) > 3 else 4
    data = open(path, "rb").read()
    seen = set()
    for m in re.finditer(rb"[\x20-\x7e]{%d,}" % minlen, data):
        s = m.group().decode("ascii")
        if pat.search(s) and s not in seen:
            seen.add(s)
            print("A", hex(m.start()), s)
    for m in re.finditer(rb"(?:[\x20-\x7e]\x00){%d,}" % minlen, data):
        s = m.group().decode("utf-16le")
        if pat.search(s) and s not in seen:
            seen.add(s)
            print("W", hex(m.start()), s)


if __name__ == "__main__":
    main()
