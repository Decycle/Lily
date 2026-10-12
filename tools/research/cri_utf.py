"""Minimal stdlib-only CRI @UTF table reader (ACB/ACF/CPK tables).

Layout (all big-endian), offsets inside the table are relative to table_start+8:
  0x00 "@UTF"  0x04 u32 table_size (bytes after this field)
  0x08 u16 version  0x0A u16 rows_offset  0x0C u32 strings_offset
  0x10 u32 data_offset  0x14 u32 name_offset(into strings)
  0x18 u16 num_columns  0x1A u16 row_width  0x1C u32 num_rows
  0x20 column schema: u8 flags, u32 name_offset, [inline constant value if storage==CONST]
flags: high nibble = storage (0x10 zero/none, 0x30 constant, 0x50 per-row, 0x70 constant2),
       low nibble = type (0 u8,1 s8,2 u16,3 s16,4 u32,5 s32,6 u64,7 s64,8 f32,9 f64,
                          0xA string(u32 offset), 0xB data(u32 offset,u32 size), 0xC u128)
Reference: vgmstream src/meta/cri_utf.c (https://github.com/vgmstream/vgmstream)
"""
import struct

_FMT = {0: ">B", 1: ">b", 2: ">H", 3: ">h", 4: ">I", 5: ">i", 6: ">Q", 7: ">q",
        8: ">f", 9: ">d"}


class UtfTable:
    def __init__(self, buf, base=0):
        if buf[base:base + 4] != b"@UTF":
            raise ValueError("not @UTF at 0x%x" % base)
        size, = struct.unpack_from(">I", buf, base + 4)
        self.raw = bytes(buf[base:base + 8 + size])
        b = self.raw
        o = 8
        (self.version, rows_off, str_off, data_off, name_off, ncols, row_w,
         nrows) = struct.unpack_from(">HHIIIHHI", b, o)
        self.rows_off, self.str_off, self.data_off = rows_off + o, str_off + o, data_off + o
        self.name = self._str(name_off)
        self.columns = []  # (name, storage, type, const_value)
        p = 0x20
        for _ in range(ncols):
            flags = b[p]
            nm, = struct.unpack_from(">I", b, p + 1)
            p += 5
            storage, typ = flags & 0xF0, flags & 0x0F
            const = None
            if storage in (0x30, 0x70):
                const, n = self._val(p, typ)
                p += n
            self.columns.append((self._str(nm), storage, typ, const))
        self.rows = []
        for r in range(nrows):
            p = self.rows_off + r * row_w
            row = {}
            for (cname, storage, typ, const) in self.columns:
                if storage == 0x50:
                    v, n = self._val(p, typ)
                    p += n
                elif storage in (0x30, 0x70):
                    v = const
                else:
                    v = None
                row[cname] = v
            self.rows.append(row)

    def _str(self, off):
        s = self.str_off + off
        e = self.raw.index(b"\0", s)
        return self.raw[s:e].decode("utf-8", "replace")

    def _val(self, p, typ):
        b = self.raw
        if typ in _FMT:
            f = _FMT[typ]
            return struct.unpack_from(f, b, p)[0], struct.calcsize(f)
        if typ == 0xA:
            off, = struct.unpack_from(">I", b, p)
            return self._str(off), 4
        if typ == 0xB:
            off, sz = struct.unpack_from(">II", b, p)
            return b[self.data_off + off:self.data_off + off + sz], 8
        if typ == 0xC:
            return b[p:p + 16], 16
        raise ValueError("unknown column type %d" % typ)

    def sub(self, row, col):
        """Parse a nested @UTF table stored in a data column; None if empty."""
        d = self.rows[row].get(col)
        if not d or d[:4] != b"@UTF":
            return None
        return UtfTable(d)


def find_utf(buf, start=0):
    """Return offset of first @UTF in buf (UE4 .uexp wraps the ACB as bulk data)."""
    i = buf.find(b"@UTF", start)
    if i < 0:
        raise ValueError("no @UTF found")
    return i


def parse_afs2(buf, base=0):
    """AFS2 header: 'AFS2', u8 ver, u8 offset_size, u16 id_size, u32 count, u16 align, u16 subkey,
    then ids[count] (id_size, LE), offsets[count+1] (offset_size, LE). Offsets are absolute in file;
    entry start must be rounded up to `align`. Returns dict(ids, starts, ends, align, subkey)."""
    if buf[base:base + 4] != b"AFS2":
        raise ValueError("not AFS2")
    ver, osz, idsz, count, align, subkey = struct.unpack_from("<BBHIHH", buf, base + 4)
    p = base + 16
    idf = {2: "<H", 4: "<I"}[idsz]
    ids = [struct.unpack_from(idf, buf, p + i * idsz)[0] for i in range(count)]
    p += count * idsz
    of = {2: "<H", 4: "<I"}[osz]
    offs = [struct.unpack_from(of, buf, p + i * osz)[0] for i in range(count + 1)]
    starts = [(o + align - 1) // align * align for o in offs[:-1]]
    ends = offs[1:]
    return dict(version=ver, ids=ids, starts=starts, ends=ends, align=align, subkey=subkey,
                header_size=p + (count + 1) * osz - base)
