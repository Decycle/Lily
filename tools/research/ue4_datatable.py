#!/usr/bin/env python3
"""Minimal UE4 DataTable (.uasset + .uexp) parser, stdlib only.

Handles cooked, split-export packages (legacy version -7, unversioned
FileVersionUE4 == 0) whose exports use *tagged* property serialization.

Usage:
    python -I ue4_datatable.py <in.uasset> [<out.json>]
    python -I ue4_datatable.py --dir <TableDir> <OutDir>     (all tables)

Output JSON: {"table", "row_struct", "names_count", "rows": {RowName: {...}}}
Never writes to the input files.
"""
import json
import os
import struct
import sys
import traceback

PACKAGE_MAGIC = 0x9E2A83C1


class ParseError(Exception):
    pass


class Reader:
    def __init__(self, data, pos=0):
        self.d = data
        self.p = pos

    def need(self, n):
        if self.p + n > len(self.d):
            raise ParseError("read past end at 0x%x (+%d, len %d)" % (self.p, n, len(self.d)))

    def raw(self, n):
        self.need(n)
        b = self.d[self.p:self.p + n]
        self.p += n
        return b

    def u8(self):
        return self.raw(1)[0]

    def i32(self):
        return struct.unpack("<i", self.raw(4))[0]

    def u32(self):
        return struct.unpack("<I", self.raw(4))[0]

    def i64(self):
        return struct.unpack("<q", self.raw(8))[0]

    def u64(self):
        return struct.unpack("<Q", self.raw(8))[0]

    def i16(self):
        return struct.unpack("<h", self.raw(2))[0]

    def u16(self):
        return struct.unpack("<H", self.raw(2))[0]

    def i8(self):
        return struct.unpack("<b", self.raw(1))[0]

    def f32(self):
        return struct.unpack("<f", self.raw(4))[0]

    def f64(self):
        return struct.unpack("<d", self.raw(8))[0]

    def fstring(self):
        n = self.i32()
        if n == 0:
            return ""
        if n > 0:
            if n > 1 << 20:
                raise ParseError("absurd FString length %d at 0x%x" % (n, self.p))
            b = self.raw(n)
            return b[:-1].decode("latin-1") if b[-1:] == b"\0" else b.decode("latin-1")
        n = -n
        if n > 1 << 20:
            raise ParseError("absurd FString length %d" % n)
        b = self.raw(n * 2)
        return b[:-2].decode("utf-16-le", "replace")


def fmt_float(x):
    # keep floats readable (float32 -> shortest repr that round trips to f32)
    for digits in range(6, 10):
        s = "%.*g" % (digits, x)
        if struct.pack("<f", float(s)) == struct.pack("<f", x):
            return float(s)
    return x


class Package:
    def __init__(self, uasset_path):
        self.path = uasset_path
        with open(uasset_path, "rb") as f:
            self.ua = f.read()
        uexp_path = os.path.splitext(uasset_path)[0] + ".uexp"
        with open(uexp_path, "rb") as f:
            self.ux = f.read()
        self._summary()

    def _summary(self):
        r = Reader(self.ua)
        if r.u32() != PACKAGE_MAGIC:
            raise ParseError("bad magic")
        legacy = r.i32()
        if legacy != -4:
            r.i32()  # LegacyUE3Version
        self.file_ver = r.i32()
        self.licensee_ver = r.i32()
        if legacy <= -2:
            cnt = r.i32()
            for _ in range(cnt):
                r.raw(20)  # FGuid + int32 (optimized custom version format)
        self.total_header = r.i32()
        r.fstring()  # FolderName
        self.pkg_flags = r.u32()
        name_count = r.i32()
        name_off = r.i32()
        # (rest of summary is not needed for the DataTable payload)
        # Remaining summary fields we use: gatherable text, exports, imports
        r.fstring() if False else None
        self.names = []
        nr = Reader(self.ua, name_off)
        for _ in range(name_count):
            self.names.append(nr.fstring())
            nr.u32()  # case-preserving/non-case hashes (2x u16)
        # export/import tables
        # Summary layout continues: [LocalizationId FString if editor-only not filtered]
        rr = Reader(self.ua, r.p)
        if not (self.pkg_flags & 0x80000000):
            rr.fstring()
        self.gatherable_count = rr.i32(); rr.i32()
        self.export_count = rr.i32(); self.export_off = rr.i32()
        self.import_count = rr.i32(); self.import_off = rr.i32()

    def name(self, idx, num=0):
        if idx < 0 or idx >= len(self.names):
            raise ParseError("bad name index %d" % idx)
        n = self.names[idx]
        return n if num == 0 else "%s_%d" % (n, num - 1)

    def read_fname(self, r):
        return self.name(r.i32(), r.i32())

    # ------------------------------------------------------------- properties
    def read_tag(self, r):
        name = self.read_fname(r)
        if name == "None":
            return None
        tag = {"name": name, "type": self.read_fname(r)}
        tag["size"] = r.i32()
        tag["index"] = r.i32()
        t = tag["type"]
        if t == "StructProperty":
            tag["struct"] = self.read_fname(r)
            r.raw(16)
        elif t == "BoolProperty":
            tag["bool"] = r.u8() != 0
        elif t in ("ByteProperty", "EnumProperty"):
            tag["enum"] = self.read_fname(r)
        elif t in ("ArrayProperty", "SetProperty"):
            tag["inner"] = self.read_fname(r)
        elif t == "MapProperty":
            tag["inner"] = self.read_fname(r)
            tag["value"] = self.read_fname(r)
        if r.u8():
            r.raw(16)  # property guid
        return tag

    def read_props(self, r, end=None):
        out = {}
        while True:
            if end is not None and r.p >= end:
                break
            tag = self.read_tag(r)
            if tag is None:
                break
            start = r.p
            try:
                val = self.read_value(r, tag, tag["type"], tag["size"])
            except ParseError as e:
                r.p = start
                val = {"__unparsed__": tag["type"], "error": str(e),
                       "hex": r.raw(tag["size"]).hex()}
            if r.p != start + tag["size"] and tag["type"] != "BoolProperty":
                # trust the tag size
                val = {"__size_mismatch__": tag["type"], "consumed": r.p - start,
                       "size": tag["size"], "value": val}
                r.p = start + tag["size"]
            key = tag["name"] if tag["index"] == 0 else "%s[%d]" % (tag["name"], tag["index"])
            out[key] = val
        return out

    def read_text(self, r):
        flags = r.u32()
        hist = r.i8()
        if hist == -1:
            has = r.i32()
            return {"text": r.fstring()} if has else {"text": ""}
        if hist == 0:
            ns = r.fstring(); key = r.fstring(); src = r.fstring()
            return {"ns": ns, "key": key, "text": src}
        if hist == 11:
            tid = self.read_fname(r); key = r.fstring()
            return {"string_table": tid, "key": key}
        raise ParseError("unsupported FText history %d (flags %d)" % (hist, flags))

    def read_soft(self, r):
        asset = self.read_fname(r)
        sub = r.fstring()
        return asset + (":" + sub if sub else "")

    def read_struct(self, r, sname, size):
        if sname in ("Vector2D",):
            return [fmt_float(r.f32()), fmt_float(r.f32())]
        if sname in ("Vector", "Rotator"):
            return [fmt_float(r.f32()) for _ in range(3)]
        if sname in ("Vector4", "Quat", "LinearColor"):
            return [fmt_float(r.f32()) for _ in range(4)]
        if sname == "Color":
            b, g, rr, a = r.raw(4)
            return {"R": rr, "G": g, "B": b, "A": a}
        if sname == "IntPoint":
            return [r.i32(), r.i32()]
        if sname == "Guid":
            return r.raw(16).hex()
        if sname in ("DateTime", "Timespan"):
            return r.i64()
        if sname in ("SoftObjectPath", "SoftClassPath", "StringAssetReference"):
            return self.read_soft(r)
        if sname == "Box2D":
            return {"min": [r.f32(), r.f32()], "max": [r.f32(), r.f32()], "valid": r.u8()}
        if sname == "Margin":
            return [fmt_float(r.f32()) for _ in range(4)]
        if sname == "GameplayTagContainer":
            return [self.read_fname(r) for _ in range(r.i32())]
        end = r.p + size if size is not None else None
        v = self.read_props(r, end)
        return v

    def read_value(self, r, tag, t, size, in_array=False):
        if t == "IntProperty":
            return r.i32()
        if t == "UInt32Property":
            return r.u32()
        if t == "Int64Property":
            return r.i64()
        if t == "UInt64Property":
            return r.u64()
        if t == "Int16Property":
            return r.i16()
        if t == "UInt16Property":
            return r.u16()
        if t == "Int8Property":
            return r.i8()
        if t == "FloatProperty":
            return fmt_float(r.f32())
        if t == "DoubleProperty":
            return r.f64()
        if t == "BoolProperty":
            return (r.u8() != 0) if in_array else tag["bool"]
        if t == "StrProperty":
            return r.fstring()
        if t == "NameProperty":
            return self.read_fname(r)
        if t == "TextProperty":
            return self.read_text(r)
        if t == "ByteProperty":
            if in_array:
                return r.u8()
            if size == 1:
                return r.u8()
            return self.read_fname(r)
        if t == "EnumProperty":
            return self.read_fname(r)
        if t in ("SoftObjectProperty", "SoftClassProperty", "AssetObjectProperty"):
            return self.read_soft(r)
        if t in ("ObjectProperty", "ClassProperty", "InterfaceProperty", "WeakObjectProperty",
                 "LazyObjectProperty"):
            return {"object_index": r.i32()}
        if t == "StructProperty":
            return self.read_struct(r, tag.get("struct"), None if in_array else size)
        if t in ("ArrayProperty", "SetProperty"):
            inner = tag["inner"]
            if t == "SetProperty":
                r.i32()  # num to remove
            n = r.i32()
            if inner == "StructProperty":
                itag = self.read_tag(r)
                out = []
                for _ in range(n):
                    out.append(self.read_struct(r, itag["struct"], None))
                    # nested tagged structs end with None, which read_props consumed
                return out
            if inner == "ByteProperty" and n and size - 4 == n:
                return list(r.raw(n))
            return [self.read_value(r, {"type": inner}, inner, None, in_array=True) for _ in range(n)]
        if t == "MapProperty":
            r.i32()  # keys to remove
            n = r.i32()
            out = []
            kt, vt = tag["inner"], tag["value"]
            for _ in range(n):
                k = self.read_value(r, {"type": kt, "struct": None}, kt, None, in_array=True)
                v = self.read_value(r, {"type": vt, "struct": None}, vt, None, in_array=True)
                out.append([k, v])
            return out
        raise ParseError("unknown property type " + t)

    # -------------------------------------------------------------- datatable
    def parse_datatable(self):
        r = Reader(self.ux)
        header = self.read_props(r)
        row_struct = header.get("RowStruct")
        r.i32()  # object guid flag (bool as int32)
        n = r.i32()
        if n < 0 or n > 100000:
            raise ParseError("bad row count %d" % n)
        rows = {}
        order = []
        for _ in range(n):
            rn = self.read_fname(r)
            rows[rn] = self.read_props(r)
            order.append(rn)
        trailing = len(self.ux) - r.p
        return {"header": header, "row_count": n, "rows": rows, "trailing_bytes": trailing}

    def row_struct_name(self):
        # Import table: find the ScriptStruct import (name ends with 'Data' usually)
        out = []
        ir = Reader(self.ua, self.import_off)
        for _ in range(self.import_count):
            cpkg = self.read_fname(ir); cls = self.read_fname(ir)
            outer = ir.i32(); obj = self.read_fname(ir)
            out.append((cpkg, cls, outer, obj))
            if ir.p - self.import_off >= 28 * self.import_count:
                pass
        for cpkg, cls, outer, obj in out:
            if cls == "ScriptStruct":
                return obj
        return None


def parse_file(uasset, out_json=None):
    pkg = Package(uasset)
    dt = pkg.parse_datatable()
    try:
        rs = pkg.row_struct_name()
    except Exception:
        rs = None
    res = {
        "table": os.path.splitext(os.path.basename(uasset))[0],
        "row_struct": rs,
        "row_count": dt["row_count"],
        "trailing_bytes": dt["trailing_bytes"],
        "rows": dt["rows"],
    }
    if out_json:
        with open(out_json, "w", encoding="utf-8") as f:
            json.dump(res, f, ensure_ascii=False, indent=1)
    return res


def count_unparsed(obj):
    if isinstance(obj, dict):
        c = 1 if ("__unparsed__" in obj or "__size_mismatch__" in obj) else 0
        return c + sum(count_unparsed(v) for v in obj.values())
    if isinstance(obj, list):
        return sum(count_unparsed(v) for v in obj)
    return 0


def main(argv):
    if len(argv) >= 3 and argv[0] == "--dir":
        src, dst = argv[1], argv[2]
        os.makedirs(dst, exist_ok=True)
        ok, bad = [], []
        for fn in sorted(os.listdir(src)):
            if not fn.endswith(".uasset"):
                continue
            base = fn[:-7]
            try:
                res = parse_file(os.path.join(src, fn), os.path.join(dst, base + ".json"))
                u = count_unparsed(res["rows"])
                status = "OK" if (u == 0 and res["trailing_bytes"] == 4) else "PARTIAL"
                (ok if status == "OK" else bad).append(base)
                print("%-40s %-7s rows=%-5d struct=%s unparsed=%d trailing=%d" % (
                    base, status, res["row_count"], res["row_struct"], u, res["trailing_bytes"]))
            except Exception as e:
                bad.append(base)
                print("%-40s FAIL    %s" % (base, e))
                if os.environ.get("UEDEBUG"):
                    traceback.print_exc()
        print("\nOK: %d  partial/failed: %d -> %s" % (len(ok), len(bad), ", ".join(bad)))
        return 0
    if not argv:
        print(__doc__)
        return 1
    res = parse_file(argv[0], argv[1] if len(argv) > 1 else None)
    if len(argv) == 1:
        print(json.dumps(res, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
