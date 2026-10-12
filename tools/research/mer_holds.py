"""Hold-joint interpolation check and misc edge cases for WACCA .mer files.

Usage: python -I mer_holds.py <MusicData dir>
Read-only.
"""
import os
import re
import sys
from collections import Counter

TPM = 1920


def read_text(path):
    raw = open(path, "rb").read()
    if raw.startswith(b"\xff\xfe") or raw.startswith(b"\xfe\xff"):
        return raw.decode("utf-16")
    if raw.startswith(b"\xef\xbb\xbf"):
        raw = raw[3:]
    return raw.decode("utf-8", errors="replace")


def rows_of(text):
    out = []
    body = False
    for ln in text.splitlines():
        s = ln.strip()
        if not body:
            if s.startswith("#BODY"):
                body = True
            continue
        if not s:
            continue
        t = [re.match(r"-?\d+(\.\d*)?", x).group() for x in s.split()]
        out.append([int(t[0]), int(t[1]), int(t[2])] + [float(x) if "." in x else int(x) for x in t[3:]])
    return out


def main():
    root = sys.argv[1]
    dev_pos = Counter()
    dev_size = Counter()
    n_checked = 0
    no_maskadd = []
    first_mask_tick = Counter()
    start_flag0 = 0
    same_tick_notes_with_hold_end = 0
    chains_total = 0
    for song in sorted(os.listdir(root)):
        d = os.path.join(root, song)
        for f in sorted(os.listdir(d)):
            if not f.endswith(".mer"):
                continue
            rows = rows_of(read_text(os.path.join(d, f)))
            notes = {}
            masks = []
            for r in rows:
                if r[2] == 1:
                    notes[r[4]] = r
                    if r[3] in (12, 13):
                        masks.append((r[0] * TPM + r[1], r[3]))
            if not any(m[1] == 12 for m in masks):
                no_maskadd.append(f)
            elif masks:
                first = min(masks)
                first_mask_tick["tick0" if first[0] == 0 else ("<=1 measure" if first[0] <= TPM else ">1 measure")] += 1
            for r in notes.values():
                if r[3] not in (9, 25):
                    continue
                chains_total += 1
                chain = [r]
                cur = r
                while cur[3] != 11:
                    cur = notes[cur[8]]
                    chain.append(cur)
                # unwrap positions
                pos = [chain[0][5]]
                for c in chain[1:]:
                    p = c[5]
                    prev = pos[-1]
                    while p - prev > 30:
                        p -= 60
                    while prev - p > 30:
                        p += 60
                    pos.append(p)
                t = [c[0] * TPM + c[1] for c in chain]
                key = [i for i, c in enumerate(chain) if c[7] == 1 or c[3] in (9, 11, 25)]
                for a, b in zip(key, key[1:]):
                    for k in range(a + 1, b):
                        if t[b] == t[a]:
                            continue
                        u = (t[k] - t[a]) / (t[b] - t[a])
                        ip = pos[a] + (pos[b] - pos[a]) * u
                        isz = chain[a][6] + (chain[b][6] - chain[a][6]) * u
                        dev_pos[min(int(round(abs(ip - pos[k]))), 10)] += 1
                        dev_size[min(int(round(abs(isz - chain[k][6]))), 10)] += 1
                        n_checked += 1
    print("hold chains:", chains_total)
    print("flag-0 joints checked against linear interp between flag-1 neighbours:", n_checked)
    print("  |pos deviation| (rounded, capped 10):", sorted(dev_pos.items()))
    print("  |size deviation| (rounded, capped 10):", sorted(dev_size.items()))
    print("charts with no MaskAdd:", len(no_maskadd), no_maskadd)
    print("first mask event location:", dict(first_mask_tick))


if __name__ == "__main__":
    main()
