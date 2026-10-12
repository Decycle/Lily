"""Resolve every cue in a CRI ACB (embedded in a UE4 .uexp) down to AWB file + entry + byte range.

Usage:
  python -I acb_map.py <MER_BGM.uexp> [awb_dir] [--charts <MusicData dir>] [--csv out.csv] [--quiet]

Chain: CueName(CueName -> CueIndex) -> Cue(ReferenceType, ReferenceIndex)
       -> [Sequence(TrackIndex[]) -> Track(EventIndex) -> TrackEvent(Command: 0x07D0 noteOn type/index)]
       -> Synth(ReferenceItems: (u16 type, u16 index)*) -> Waveform
       -> Waveform.Streaming/StreamAwbPortNo/StreamAwbId (or MemoryAwbId)
       -> StreamAwbHash[port].Name + ".awb" -> AFS2 id table -> entry index -> [start, end)
If awb_dir is given, the AFS2 headers of the real files are read and each entry's first bytes are
checked for an HCA signature. Only headers are read (AWBs are opened, seeked, never loaded whole).
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cri_utf import UtfTable, find_utf, parse_afs2  # noqa: E402

REF_NAMES = {1: "Waveform", 2: "Synth", 3: "Sequence", 8: "BlockSequence"}


def parse_commands(blob):
    """TLV command list: u16 code, u8 size, payload[size]. Returns [(code, payload)]."""
    out, p = [], 0
    while p + 3 <= len(blob):
        code, size = struct.unpack_from(">HB", blob, p)
        out.append((code, blob[p + 3:p + 3 + size]))
        p += 3 + size
    return out


class Acb:
    def __init__(self, data):
        self.h = UtfTable(data, find_utf(data))
        g = lambda n: self.h.sub(0, n)  # noqa: E731
        self.name = self.h.rows[0].get("Name")
        self.cue = g("CueTable")
        self.cuename = g("CueNameTable")
        self.wave = g("WaveformTable")
        self.synth = g("SynthTable")
        self.seq = g("SequenceTable")
        self.track = g("TrackTable")
        self.tevent = g("TrackEventTable")
        self.tcmd = g("TrackCommandTable")
        self.strings = g("StringValueTable")
        self.wext = g("WaveformExtensionDataTable")
        self.awbhash = g("StreamAwbHash")
        self.awbhdr = g("StreamAwbAfs2Header")
        self.memawb = self.h.rows[0].get("AwbFile") or b""

    # --- walkers return list of (waveform_index, path_description, track_index_or_None)
    def walk(self, rtype, idx, depth=0, track=None):
        if depth > 8:
            return []
        if rtype == 1:
            return [(idx, track)]
        if rtype == 2:
            items = self.synth.rows[idx]["ReferenceItems"] or b""
            out = []
            for i in range(0, len(items) - 3, 4):
                t, j = struct.unpack_from(">HH", items, i)
                out += self.walk(t, j, depth + 1, track)
            return out
        if rtype == 3:
            r = self.seq.rows[idx]
            ti = r["TrackIndex"] or b""
            out = []
            for k in range(0, len(ti) - 1, 2):
                tidx, = struct.unpack_from(">H", ti, k)
                ev = self.track.rows[tidx]["EventIndex"]
                if ev == 0xFFFF:
                    continue
                for code, pl in parse_commands(self.tevent.rows[ev]["Command"] or b""):
                    if code == 0x07D0 and len(pl) >= 4:  # noteOn: u16 ref type, u16 ref index
                        t, j = struct.unpack_from(">HH", pl, 0)
                        out += self.walk(t, j, depth + 1, tidx)
            return out
        return []

    def track_bus(self, tidx):
        """Track command 0x006F = bus send (u16 StringValueTable index, u16 level 0..10000)."""
        if tidx is None or self.tcmd is None:
            return ""
        ci = self.track.rows[tidx]["CommandIndex"]
        if ci == 0xFFFF:
            return ""
        sends = []
        for code, pl in parse_commands(self.tcmd.rows[ci]["Command"] or b""):
            if code == 0x006F and len(pl) == 4:
                bus, lvl = struct.unpack(">HH", pl)
                if lvl:
                    sends.append("%s=%.2f" % (self.strings.rows[bus]["StringValue"], lvl / 10000))
        return "+".join(sends)


def main():
    args = sys.argv[1:]
    quiet = "--quiet" in args
    args = [a for a in args if a != "--quiet"]
    charts = csvp = None
    if "--charts" in args:
        i = args.index("--charts"); charts = args[i + 1]; del args[i:i + 2]
    if "--csv" in args:
        i = args.index("--csv"); csvp = args[i + 1]; del args[i:i + 2]
    acb = Acb(open(args[0], "rb").read())
    awb_dir = args[1] if len(args) > 1 else None
    w = acb.wave
    print("ACB %s: %d cues, %d cue names, %d waveforms, %d synths, %d sequences, %d tracks"
          % (acb.name, len(acb.cue.rows), len(acb.cuename.rows), len(w.rows),
             len(acb.synth.rows) if acb.synth else 0, len(acb.seq.rows) if acb.seq else 0,
             len(acb.track.rows) if acb.track else 0))

    # AWB tables: port -> name, afs2 parsed from the ACB copy and (optionally) from disk
    ports = {}
    if acb.awbhash:
        for i, r in enumerate(acb.awbhash.rows):
            ports[i] = r["Name"]
    awbs = {}
    for i, name in ports.items():
        emb = parse_afs2(acb.awbhdr.rows[i]["Header"]) if acb.awbhdr else None
        disk = None
        if awb_dir:
            pth = os.path.join(awb_dir, name + ".awb")
            if os.path.exists(pth):
                with open(pth, "rb") as f:
                    head = f.read(16)
                    cnt, = struct.unpack_from("<I", head, 8)
                    f.seek(0)
                    disk = parse_afs2(f.read(16 + cnt * 4 + (cnt + 1) * 4 + 64))
                    disk["path"] = pth
                    disk["filesize"] = os.path.getsize(pth)
        same = emb and disk and emb["ids"] == disk["ids"] and emb["starts"] == disk["starts"]
        awbs[i] = dict(name=name, emb=emb, disk=disk)
        print("  port %d: %-14s entries=%s align=%s subkey=%s on-disk=%s header-match=%s"
              % (i, name, len(emb["ids"]) if emb else "?", emb and emb["align"],
                 emb and emb["subkey"], "yes" if disk else "MISSING", bool(same)))
    if acb.memawb:
        m = parse_afs2(acb.memawb)
        print("  memory AWB (AwbFile column): %d entries" % len(m["ids"]))
        awbs["mem"] = dict(name="<memory AwbFile>", emb=m, disk=None)

    rows, unresolved = [], []
    hca_ok = hca_bad = 0
    used_entries = {}
    for r in acb.cuename.rows:
        cname, ci = r["CueName"], r["CueIndex"]
        c = acb.cue.rows[ci]
        wl = acb.walk(c["ReferenceType"], c["ReferenceIndex"])
        if not wl:
            unresolved.append((cname, ci, c["ReferenceType"], c["ReferenceIndex"], c.get("Length")))
            continue
        for (wi, tidx) in wl:
            wr = w.rows[wi]
            streaming = wr.get("Streaming")
            if streaming == 0:
                port, awbid = "mem", wr.get("MemoryAwbId")
            else:
                port, awbid = wr.get("StreamAwbPortNo", 0), wr.get("StreamAwbId")
                if port == 0xFFFF:
                    port = 0
            a = awbs.get(port)
            ent = start = end = None
            sig = ""
            if a and a["emb"] and awbid in a["emb"]["ids"]:
                hdr = a["disk"] or a["emb"]
                ent = hdr["ids"].index(awbid)
                start, end = hdr["starts"][ent], hdr["ends"][ent]
                used_entries.setdefault(port, set()).add(ent)
                if a["disk"]:
                    with open(a["disk"]["path"], "rb") as f:
                        f.seek(start)
                        s = f.read(4)
                    sig = s.hex()
                    if s[:3] == b"HCA" or (s[0] & 0x7F, s[1] & 0x7F, s[2] & 0x7F) == (0x48, 0x43, 0x41):
                        hca_ok += 1
                    else:
                        hca_bad += 1
                elif port == "mem":
                    s = acb.memawb[start:start + 4]
                    sig = s.hex()
            loop = ""
            ext = wr.get("ExtensionData")
            if acb.wext is not None and ext is not None and ext != 0xFFFF:
                e = acb.wext.rows[ext]
                loop = "%d-%d" % (e["LoopStart"], e["LoopEnd"])
            rows.append(dict(cue=cname, cue_index=ci, length_ms=c.get("Length"), waveform=wi,
                             track=tidx, bus=acb.track_bus(tidx),
                             awb=(a["name"] if a else "?port%s" % port), awb_id=awbid, entry=ent,
                             start=start, end=end, size=(end - start) if start is not None else None,
                             ch=wr.get("NumChannels"), rate=wr.get("SamplingRate"),
                             samples=wr.get("NumSamples"), enc=wr.get("EncodeType"),
                             loopflag=wr.get("LoopFlag"), loop=loop, sig=sig))
            if ent is None:
                unresolved.append((cname, ci, "waveform %d awb id %s not in port %s" % (wi, awbid, port)))

    if not quiet:
        print("\n%-22s %4s %8s %4s %-28s %-14s %5s %4s %11s %10s %2s %9s %-15s %s"
              % ("cue", "ci", "len_ms", "wav", "bus", "awb", "id", "ent", "offset", "size", "ch",
                 "samples", "loop", "sig"))
        for x in rows:
            print("%-22s %4d %8s %4d %-28s %-14s %5s %4s %11s %10s %2s %9s %-15s %s"
                  % (x["cue"], x["cue_index"], x["length_ms"], x["waveform"], x["bus"], x["awb"],
                     x["awb_id"], x["entry"], x["start"], x["size"], x["ch"], x["samples"],
                     x["loop"], x["sig"]))

    print("\n== summary")
    print("cue names: %d, resolved waveform refs: %d, unresolved: %d" % (len(acb.cuename.rows), len(rows), len(unresolved)))
    per_awb = {}
    for x in rows:
        per_awb.setdefault(x["awb"], set()).add(x["cue"])
    for k, v in sorted(per_awb.items()):
        print("  %-16s cues=%d" % (k, len(v)))
    for port, a in awbs.items():
        if a["emb"]:
            n = len(a["emb"]["ids"])
            print("  %-16s entries=%d referenced=%d unreferenced=%d"
                  % (a["name"], n, len(used_entries.get(port, ())), n - len(used_entries.get(port, ()))))
    if awb_dir:
        print("HCA signature check: ok=%d bad=%d" % (hca_ok, hca_bad))
    buses = {}
    for x in rows:
        buses[x["bus"]] = buses.get(x["bus"], 0) + 1
    print("track bus sends:", buses)
    encs = {}
    for x in rows:
        k = (x["enc"], {"8000": "ADX", "4843": "HCA", "c8c3": "HCA(masked)"}.get(x["sig"][:4], x["sig"][:4]),
             x["ch"], x["rate"])
        encs[k] = encs.get(k, 0) + 1
    print("(EncodeType, signature, channels, rate) counts:", encs)
    for u in unresolved:
        print("UNRESOLVED:", u)

    if charts:
        names = {r["CueName"] for r in acb.cuename.rows}
        refs = {}
        for root, _, files in os.walk(charts):
            for fn in files:
                if fn.endswith(".mer"):
                    with open(os.path.join(root, fn), "rb") as f:
                        raw = f.read(8192)
                        # most charts are UTF-8; the tutorial (S99-001) is UTF-16LE with BOM
                        enc = "utf-16" if raw[:2] in (b"\xff\xfe", b"\xfe\xff") else "utf-8"
                        for line in raw.decode(enc, "replace").splitlines():
                            if line.startswith("#MUSIC_FILE_PATH"):
                                refs.setdefault(line.split(None, 1)[1].strip(), []).append(fn)
        miss = sorted(k for k in refs if k not in names)
        print("charts: %d distinct #MUSIC_FILE_PATH values, %d found in ACB, %d missing: %s"
              % (len(refs), len(refs) - len(miss), len(miss), miss))
        unused = sorted(n for n in names if n not in refs)
        print("cues not referenced by any chart: %d e.g. %s" % (len(unused), unused[:30]))

    if csvp:
        import csv
        with open(csvp, "w", newline="") as f:
            wr = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
            wr.writeheader()
            wr.writerows(rows)
        print("wrote", csvp)


if __name__ == "__main__":
    main()
