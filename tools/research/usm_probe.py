#!/usr/bin/env python3
"""Probe CRI Sofdec2 .usm files (stdlib only, read-only).

Usage: python -I usm_probe.py <file.usm> [more.usm ...] [--scan N]
Prints CRID + per-stream header tables (@SFV video / @SFA audio / @ALP alpha),
chunk statistics (first N MB, default 64) and a guess of the video codec
and whether the video payload looks masked (encrypted).

Chunk layout (big-endian): sig[4] size u32 | u8 r0 u8 payload_off u16 pad_size
 u8 chno u16 r1 u8 data_type(0 data,1 header,2 comment/end,3 seek) u32 frame_time
 u32 frame_rate u64 r2 ; payload at +8+payload_off, length size-payload_off-pad.
Reference: https://github.com/vgmstream/vgmstream (cri_usm handling),
 https://github.com/donmai-me/WannaCRI
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cri_utf import UtfTable  # noqa: E402


def chunks(f, limit):
    pos = 0
    while pos < limit:
        f.seek(pos)
        h = f.read(0x20)
        if len(h) < 0x20:
            return
        sig = h[:4]
        size, = struct.unpack(">I", h[4:8])
        poff = h[9]
        pad, = struct.unpack(">H", h[10:12])
        chno = h[12]
        dtype = h[15] & 3
        ftime, frate = struct.unpack(">II", h[16:24])
        plen = size - poff - pad
        yield pos, sig, chno, dtype, ftime, frate, pos + 8 + poff, plen
        pos += 8 + size


def classify(head):
    if head[:4] == b"\x00\x00\x01\xb3":
        return "MPEG-1/2 video (sequence header)"
    if head[:4] in (b"\x00\x00\x00\x01",) or head[:3] == b"\x00\x00\x01":
        return "H.264 Annex-B (start code)"
    if head[:4] == b"DKIF":
        return "VP9 in IVF container"
    return "unknown (%s)" % head[:16].hex()


def startcode_density(buf):
    # count 00 00 01 occurrences per KiB past the 0x40 unmasked prefix
    b = buf[0x40:]
    n = b.count(b"\x00\x00\x01")
    return n * 1024.0 / max(1, len(b))


def probe(path, scan_mb):
    size = os.path.getsize(path)
    print("=" * 70)
    print(path, "%.1f MB" % (size / 1e6))
    stats = {}
    first_video = []
    with open(path, "rb") as f:
        for pos, sig, chno, dtype, ftime, frate, poff, plen in chunks(f, min(size, scan_mb << 20)):
            key = (sig.decode("latin1"), chno, dtype)
            stats[key] = stats.get(key, 0) + 1
            if dtype == 1 or sig == b"CRID":
                f.seek(poff)
                pl = f.read(plen)
                i = pl.find(b"@UTF")
                if i >= 0:
                    t = UtfTable(pl, i)
                    print("-- %s ch%d header table '%s' (%d rows)" % (sig.decode(), chno, t.name, len(t.rows)))
                    for r in t.rows:
                        print("   ", {k: v for k, v in r.items() if not isinstance(v, bytes)})
            if sig == b"@SFV" and dtype == 0 and len(first_video) < 3:
                f.seek(poff)
                first_video.append((ftime, frate, f.read(min(plen, 4096))))
    print("-- chunk counts (sig, chno, type) in first %d MB:" % scan_mb)
    for k in sorted(stats):
        print("   ", k, stats[k])
    for ftime, frate, d in first_video:
        print("-- video data chunk t=%d rate=%d len>=%d: %s; startcodes/KiB past 0x40 = %.2f"
              % (ftime, frate, len(d), classify(d), startcode_density(d)))


def main(a):
    scan = 64
    if "--scan" in a:
        i = a.index("--scan")
        scan = int(a[i + 1])
        del a[i:i + 2]
    for p in a:
        probe(p, scan)


if __name__ == "__main__":
    main(sys.argv[1:])
