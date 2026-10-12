"""Quick peek at small UE4 DataTable assets: print name-map strings (.uasset)
and the .uexp payload as int32/float32 words. Read-only. Usage:
  python -I peek_table.py <dir> <TableName> [...]
"""
import re
import struct
import sys
from pathlib import Path


def main():
    d = Path(sys.argv[1])
    for name in sys.argv[2:]:
        ua = (d / f"{name}.uasset").read_bytes()
        ux = (d / f"{name}.uexp").read_bytes()
        names = [m.decode("ascii", "replace") for m in re.findall(rb"[\x20-\x7e]{3,}", ua)]
        print(f"== {name}  uexp={len(ux)}B")
        print("names:", " | ".join(names))
        words = []
        for i in range(0, len(ux) - 3, 4):
            iv = struct.unpack_from("<i", ux, i)[0]
            fv = struct.unpack_from("<f", ux, i)[0]
            if -100000 < iv < 100000:
                words.append(str(iv))
            elif fv == fv and 1e-4 < abs(fv) < 1e7:
                words.append(f"f{fv:g}")
            else:
                words.append(f"x{iv & 0xffffffff:08x}")
        print("uexp:", " ".join(words[:600]))
        print()


if __name__ == "__main__":
    main()
