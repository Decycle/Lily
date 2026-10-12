"""Extract ASCII and UTF-16LE strings from a binary that match gameplay-related keywords.

Usage: python -I gameplay_strings.py <binary> <out.txt> [min_len]
Read-only on the input. Used for docs/research/04-gameplay-mechanics.md.
"""
import re
import sys

KEYWORDS = [
    "judge", "judg", "marvelous", "great", "good", "miss", "combo", "norma", "gauge",
    "hold", "snap", "slide", "chain", "swipe", "touch", "rnote", "r_note", "bonus",
    "notespeed", "note_speed", "hispeed", "hi_speed", "mask", "mirror", "offset",
    "timing", "fast", "late", "early", "release", "score", "achieve", "clear",
    "fullcombo", "allmarv", "missless", "rate", "rhythmgame", "noteobject", "sensor",
    "scroll", "reverse", "sync", "guide", "beam", "keybeam", "barline", "giveup",
]


def iter_ascii(data, min_len):
    for m in re.finditer(rb"[\x20-\x7e]{%d,}" % min_len, data):
        yield m.start(), m.group().decode("ascii")


def iter_utf16(data, min_len):
    for m in re.finditer(rb"(?:[\x20-\x7e]\x00){%d,}" % min_len, data):
        yield m.start(), m.group().decode("utf-16le")


def main():
    src, out = sys.argv[1], sys.argv[2]
    min_len = int(sys.argv[3]) if len(sys.argv) > 3 else 5
    with open(src, "rb") as f:
        data = f.read()
    seen = set()
    rows = []
    for kind, it in (("A", iter_ascii(data, min_len)), ("U", iter_utf16(data, min_len))):
        for off, s in it:
            low = s.lower()
            if any(k in low for k in KEYWORDS):
                key = (kind, s)
                if key in seen:
                    continue
                seen.add(key)
                rows.append((off, kind, s))
    rows.sort()
    with open(out, "w", encoding="utf-8") as f:
        for off, kind, s in rows:
            f.write(f"{off:08x} {kind} {s}\n")
    print(f"{len(rows)} strings -> {out}")


if __name__ == "__main__":
    main()
