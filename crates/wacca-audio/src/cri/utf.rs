//! CRI `@UTF` table reader (the table format inside ACB, ACF and CPK files).
//!
//! Layout (big-endian); offsets inside the table are relative to `table_start + 8`:
//!
//! ```text
//! 0x00 "@UTF"   0x04 u32 table_size (bytes after this field)
//! 0x08 u16 version  0x0A u16 rows_offset  0x0C u32 strings_offset  0x10 u32 data_offset
//! 0x14 u32 name (string offset)  0x18 u16 num_columns  0x1A u16 row_width  0x1C u32 num_rows
//! 0x20 schema: per column u8 flags, u32 name, [constant value if storage is constant]
//! ```
//!
//! Flags: high nibble = storage (0x10 none, 0x30 constant, 0x50 per row, 0x70 constant),
//! low nibble = type (see [`Value`]). Reference: vgmstream `cri_utf.c`, `tools/research/cri_utf.py`.

use std::borrow::Cow;

use super::{bytes, slice};
use crate::Error;

const MAGIC: &[u8; 4] = b"@UTF";

/// One cell. Strings and data blobs borrow from the input buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    /// Column declared without storage (0x10): no value.
    None,
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(Cow<'a, str>),
    Data(&'a [u8]),
    U128([u8; 16]),
}

impl Value<'_> {
    /// Any non-negative integer type, widened to `u64`.
    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Value::U8(v) => Some(v.into()),
            Value::U16(v) => Some(v.into()),
            Value::U32(v) => Some(v.into()),
            Value::U64(v) => Some(v),
            Value::I8(v) => u64::try_from(v).ok(),
            Value::I16(v) => u64::try_from(v).ok(),
            Value::I32(v) => u64::try_from(v).ok(),
            Value::I64(v) => u64::try_from(v).ok(),
            _ => None,
        }
    }
}

/// Column schema entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Column<'a> {
    pub name: Cow<'a, str>,
    pub flags: u8,
}

/// A parsed `@UTF` table. Cells are indexed `rows[row][column]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Table<'a> {
    pub name: Cow<'a, str>,
    pub version: u16,
    pub columns: Vec<Column<'a>>,
    pub rows: Vec<Vec<Value<'a>>>,
}

/// Offset of the first `@UTF` magic in `buf` (UE4 `.uexp` files carry the ACB as bulk data).
pub fn find(buf: &[u8]) -> Option<usize> {
    buf.windows(MAGIC.len()).position(|w| w == MAGIC)
}

/// Pools and bounds shared while decoding one table.
struct Ctx<'a> {
    table: &'a [u8],
    strings: usize,
    data: usize,
}

impl<'a> Ctx<'a> {
    fn string(&self, offset: u32) -> Result<Cow<'a, str>, Error> {
        let start = self.strings + offset as usize;
        let rest = self.table.get(start..).ok_or(Error::Truncated {
            what: "@UTF string",
            offset: start,
            need: 1,
            have: self.table.len(),
        })?;
        let len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Ok(String::from_utf8_lossy(&rest[..len]))
    }

    /// Decode one value of type `ty` at `pos`; returns the value and its size.
    fn value(&self, pos: usize, ty: u8) -> Result<(Value<'a>, usize), Error> {
        let b = self.table;
        const W: &str = "@UTF value";
        Ok(match ty {
            0x0 => (Value::U8(bytes::<1>(b, pos, W)?[0]), 1),
            0x1 => (Value::I8(i8::from_be_bytes(bytes(b, pos, W)?)), 1),
            0x2 => (Value::U16(u16::from_be_bytes(bytes(b, pos, W)?)), 2),
            0x3 => (Value::I16(i16::from_be_bytes(bytes(b, pos, W)?)), 2),
            0x4 => (Value::U32(u32::from_be_bytes(bytes(b, pos, W)?)), 4),
            0x5 => (Value::I32(i32::from_be_bytes(bytes(b, pos, W)?)), 4),
            0x6 => (Value::U64(u64::from_be_bytes(bytes(b, pos, W)?)), 8),
            0x7 => (Value::I64(i64::from_be_bytes(bytes(b, pos, W)?)), 8),
            0x8 => (Value::F32(f32::from_be_bytes(bytes(b, pos, W)?)), 4),
            0x9 => (Value::F64(f64::from_be_bytes(bytes(b, pos, W)?)), 8),
            0xA => {
                let off = u32::from_be_bytes(bytes(b, pos, W)?);
                (Value::Str(self.string(off)?), 4)
            }
            0xB => {
                let off = u32::from_be_bytes(bytes(b, pos, W)?) as usize;
                let len = u32::from_be_bytes(bytes(b, pos + 4, W)?) as usize;
                let data = slice(b, self.data + off, len, "@UTF data")?;
                (Value::Data(data), 8)
            }
            0xC => (Value::U128(bytes(b, pos, W)?), 16),
            _ => return Err(Error::UnsupportedColumn(ty)),
        })
    }
}

impl<'a> Table<'a> {
    /// Parse the table whose `@UTF` magic is at the start of `buf`.
    pub fn parse(buf: &'a [u8]) -> Result<Self, Error> {
        if bytes::<4>(buf, 0, "@UTF header")? != *MAGIC {
            return Err(Error::BadMagic {
                what: "@UTF",
                expected: "@UTF",
                offset: 0,
            });
        }
        let size = u32::from_be_bytes(bytes(buf, 4, "@UTF header")?) as usize;
        let table = slice(buf, 0, size + 8, "@UTF table")?;
        let h = |off| bytes::<4>(table, off, "@UTF header");
        let u16_at = |off| bytes::<2>(table, off, "@UTF header").map(u16::from_be_bytes);
        let version = u16_at(0x08)?;
        let rows_off = u16_at(0x0A)? as usize + 8;
        let ctx = Ctx {
            table,
            strings: u32::from_be_bytes(h(0x0C)?) as usize + 8,
            data: u32::from_be_bytes(h(0x10)?) as usize + 8,
        };
        let name = ctx.string(u32::from_be_bytes(h(0x14)?))?;
        let num_columns = u16_at(0x18)?;
        let row_width = u16_at(0x1A)? as usize;
        let num_rows = u32::from_be_bytes(h(0x1C)?) as usize;

        // Schema: per column, the name and either a constant, a per-row marker, or nothing.
        let mut columns = Vec::with_capacity(num_columns.into());
        let mut constants = Vec::with_capacity(num_columns.into());
        let mut pos = 0x20;
        for _ in 0..num_columns {
            let flags = bytes::<1>(table, pos, "@UTF schema")?[0];
            let col_name = ctx.string(u32::from_be_bytes(bytes(table, pos + 1, "@UTF schema")?))?;
            pos += 5;
            let constant = match flags & 0xF0 {
                0x10 => Some(Value::None),
                0x50 => None,
                0x30 | 0x70 => {
                    let (v, n) = ctx.value(pos, flags & 0x0F)?;
                    pos += n;
                    Some(v)
                }
                _ => return Err(Error::UnsupportedColumn(flags)),
            };
            columns.push(Column {
                name: col_name,
                flags,
            });
            constants.push(constant);
        }

        let mut rows = Vec::with_capacity(num_rows.min(table.len()));
        for r in 0..num_rows {
            let mut pos = rows_off + r * row_width;
            let mut row = Vec::with_capacity(columns.len());
            for (col, constant) in columns.iter().zip(&constants) {
                row.push(match constant {
                    Some(v) => v.clone(),
                    None => {
                        let (v, n) = ctx.value(pos, col.flags & 0x0F)?;
                        pos += n;
                        v
                    }
                });
            }
            rows.push(row);
        }
        Ok(Table {
            name,
            version,
            columns,
            rows,
        })
    }

    fn column(&self, column: &str) -> Result<usize, Error> {
        self.columns
            .iter()
            .position(|c| c.name == column)
            .ok_or_else(|| Error::MissingColumn {
                table: self.name.to_string(),
                column: column.to_string(),
            })
    }

    /// The cell at (`row`, `column`).
    pub fn get(&self, row: usize, column: &str) -> Result<&Value<'a>, Error> {
        let c = self.column(column)?;
        self.rows
            .get(row)
            .and_then(|r| r.get(c))
            .ok_or_else(|| Error::RowOutOfRange {
                table: self.name.to_string(),
                row,
            })
    }

    fn type_error(&self, row: usize, column: &str) -> Error {
        Error::ColumnType {
            table: self.name.to_string(),
            column: column.to_string(),
            row,
        }
    }

    /// An integer cell of any width, widened to `u64`.
    pub fn uint(&self, row: usize, column: &str) -> Result<u64, Error> {
        self.get(row, column)?
            .as_u64()
            .ok_or_else(|| self.type_error(row, column))
    }

    /// A string cell.
    pub fn str(&self, row: usize, column: &str) -> Result<&str, Error> {
        match self.get(row, column)? {
            Value::Str(s) => Ok(s),
            _ => Err(self.type_error(row, column)),
        }
    }

    /// A data cell (empty for a column without storage).
    pub fn data(&self, row: usize, column: &str) -> Result<&'a [u8], Error> {
        match self.get(row, column)? {
            Value::Data(d) => Ok(d),
            Value::None => Ok(&[]),
            _ => Err(self.type_error(row, column)),
        }
    }

    /// A nested table stored in a data cell; `None` if the cell is empty.
    pub fn sub_table(&self, row: usize, column: &str) -> Result<Option<Table<'a>>, Error> {
        let d = self.data(row, column)?;
        if d.is_empty() {
            return Ok(None);
        }
        Table::parse(d).map(Some)
    }
}

/// Test-only `@UTF` writer, used to build synthetic fixtures.
#[cfg(test)]
pub(crate) mod build {
    /// Owned cell for the fixture builder.
    #[derive(Debug, Clone)]
    pub enum V {
        U8(u8),
        I8(i8),
        U16(u16),
        I16(i16),
        U32(u32),
        I32(i32),
        U64(u64),
        I64(i64),
        F32(f32),
        F64(f64),
        Str(&'static str),
        Data(Vec<u8>),
        U128([u8; 16]),
    }

    impl V {
        fn ty(&self) -> u8 {
            match self {
                V::U8(_) => 0,
                V::I8(_) => 1,
                V::U16(_) => 2,
                V::I16(_) => 3,
                V::U32(_) => 4,
                V::I32(_) => 5,
                V::U64(_) => 6,
                V::I64(_) => 7,
                V::F32(_) => 8,
                V::F64(_) => 9,
                V::Str(_) => 0xA,
                V::Data(_) => 0xB,
                V::U128(_) => 0xC,
            }
        }
    }

    /// Column storage for the builder.
    #[derive(Debug, Clone)]
    pub enum Col {
        /// Per-row value; the type comes from the first row's cell.
        Row(&'static str),
        Const(&'static str, V),
        /// No storage (0x10) with the given type nibble.
        Zero(&'static str, u8),
    }

    struct Pools {
        strings: Vec<u8>,
        data: Vec<u8>,
    }

    impl Pools {
        fn string(&mut self, s: &str) -> u32 {
            let off = self.strings.len() as u32;
            self.strings.extend_from_slice(s.as_bytes());
            self.strings.push(0);
            off
        }

        fn put(&mut self, out: &mut Vec<u8>, v: &V) {
            match v {
                V::U8(x) => out.push(*x),
                V::I8(x) => out.extend(x.to_be_bytes()),
                V::U16(x) => out.extend(x.to_be_bytes()),
                V::I16(x) => out.extend(x.to_be_bytes()),
                V::U32(x) => out.extend(x.to_be_bytes()),
                V::I32(x) => out.extend(x.to_be_bytes()),
                V::U64(x) => out.extend(x.to_be_bytes()),
                V::I64(x) => out.extend(x.to_be_bytes()),
                V::F32(x) => out.extend(x.to_be_bytes()),
                V::F64(x) => out.extend(x.to_be_bytes()),
                V::Str(s) => {
                    let off = self.string(s);
                    out.extend(off.to_be_bytes());
                }
                V::Data(d) => {
                    out.extend((self.data.len() as u32).to_be_bytes());
                    out.extend((d.len() as u32).to_be_bytes());
                    self.data.extend_from_slice(d);
                }
                V::U128(b) => out.extend(b),
            }
        }
    }

    /// Serialize a table. `rows[r]` lists only the per-row cells, in column order.
    pub fn table(name: &str, cols: &[Col], rows: &[Vec<V>]) -> Vec<u8> {
        let mut pools = Pools {
            strings: Vec::new(),
            data: Vec::new(),
        };
        let name_off = pools.string(name);
        let mut schema = Vec::new();
        for (i, col) in cols.iter().enumerate() {
            let (flags, cname) = match col {
                Col::Row(n) => (0x50 | rows.first().map_or(0, |r| r[i_row(cols, i)].ty()), n),
                Col::Const(n, v) => (0x30 | v.ty(), n),
                Col::Zero(n, ty) => (0x10 | ty, n),
            };
            schema.push(flags);
            let off = pools.string(cname);
            schema.extend(off.to_be_bytes());
            if let Col::Const(_, v) = col {
                pools.put(&mut schema, v);
            }
        }
        let mut row_bytes = Vec::new();
        let mut row_width = 0;
        for row in rows {
            let start = row_bytes.len();
            for v in row {
                pools.put(&mut row_bytes, v);
            }
            row_width = row_bytes.len() - start;
        }
        let rows_off = 0x20 + schema.len();
        let strings_off = rows_off + row_bytes.len();
        let data_off = strings_off + pools.strings.len();
        let size = data_off + pools.data.len() - 8;

        let mut out = b"@UTF".to_vec();
        out.extend((size as u32).to_be_bytes());
        out.extend(1u16.to_be_bytes());
        out.extend(((rows_off - 8) as u16).to_be_bytes());
        out.extend(((strings_off - 8) as u32).to_be_bytes());
        out.extend(((data_off - 8) as u32).to_be_bytes());
        out.extend(name_off.to_be_bytes());
        out.extend((cols.len() as u16).to_be_bytes());
        out.extend((row_width as u16).to_be_bytes());
        out.extend((rows.len() as u32).to_be_bytes());
        out.extend(schema);
        out.extend(row_bytes);
        out.extend(pools.strings);
        out.extend(pools.data);
        out
    }

    /// Index of column `i` among the per-row columns.
    fn i_row(cols: &[Col], i: usize) -> usize {
        cols[..i]
            .iter()
            .filter(|c| matches!(c, Col::Row(_)))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::build::{Col, V, table};
    use super::*;

    fn all_types() -> Vec<u8> {
        let cols = [
            Col::Row("u8"),
            Col::Row("i8"),
            Col::Row("u16"),
            Col::Row("i16"),
            Col::Row("u32"),
            Col::Row("i32"),
            Col::Row("u64"),
            Col::Row("i64"),
            Col::Row("f32"),
            Col::Row("f64"),
            Col::Row("str"),
            Col::Row("data"),
            Col::Row("u128"),
            Col::Const("const", V::U16(7)),
            Col::Zero("none", 0x4),
        ];
        let row = |k: u8| {
            vec![
                V::U8(k),
                V::I8(-1),
                V::U16(0x1234),
                V::I16(-2),
                V::U32(0xDEADBEEF),
                V::I32(-3),
                V::U64(1 << 40),
                V::I64(-4),
                V::F32(1.5),
                V::F64(-2.25),
                V::Str(if k == 0 { "zero" } else { "one" }),
                V::Data(vec![k, k, k]),
                V::U128([k; 16]),
            ]
        };
        table("Types", &cols, &[row(0), row(1)])
    }

    #[test]
    fn parses_all_column_types_and_storage() {
        let buf = all_types();
        let t = Table::parse(&buf).unwrap();
        assert_eq!(t.name, "Types");
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.uint(1, "u8").unwrap(), 1);
        assert_eq!(t.get(0, "i8").unwrap(), &Value::I8(-1));
        assert_eq!(t.uint(0, "u16").unwrap(), 0x1234);
        assert_eq!(t.get(0, "i16").unwrap(), &Value::I16(-2));
        assert_eq!(t.uint(0, "u32").unwrap(), 0xDEADBEEF);
        assert_eq!(t.get(0, "i32").unwrap(), &Value::I32(-3));
        assert_eq!(t.uint(0, "u64").unwrap(), 1 << 40);
        assert_eq!(t.get(0, "i64").unwrap(), &Value::I64(-4));
        assert_eq!(t.get(0, "f32").unwrap(), &Value::F32(1.5));
        assert_eq!(t.get(0, "f64").unwrap(), &Value::F64(-2.25));
        assert_eq!(t.str(0, "str").unwrap(), "zero");
        assert_eq!(t.str(1, "str").unwrap(), "one");
        assert_eq!(t.data(1, "data").unwrap(), &[1, 1, 1]);
        assert_eq!(t.get(1, "u128").unwrap(), &Value::U128([1; 16]));
        assert_eq!(t.uint(1, "const").unwrap(), 7);
        assert_eq!(t.get(0, "none").unwrap(), &Value::None);
        assert_eq!(t.data(0, "none").unwrap(), &[] as &[u8]);
    }

    #[test]
    fn negative_signed_is_not_uint() {
        let buf = all_types();
        let t = Table::parse(&buf).unwrap();
        assert!(matches!(t.uint(0, "i8"), Err(Error::ColumnType { .. })));
        assert!(matches!(t.str(0, "u8"), Err(Error::ColumnType { .. })));
    }

    #[test]
    fn missing_column_and_row_are_errors() {
        let buf = all_types();
        let t = Table::parse(&buf).unwrap();
        assert!(matches!(t.get(0, "nope"), Err(Error::MissingColumn { .. })));
        assert!(matches!(t.get(5, "u8"), Err(Error::RowOutOfRange { .. })));
    }

    #[test]
    fn nested_table_and_find() {
        let inner = table("Inner", &[Col::Row("x")], &[vec![V::U8(9)]]);
        let outer = table(
            "Outer",
            &[Col::Row("t"), Col::Row("e")],
            &[vec![V::Data(inner), V::Data(vec![])]],
        );
        let mut wrapped = vec![0xAA; 5];
        wrapped.extend(&outer);
        let at = find(&wrapped).unwrap();
        assert_eq!(at, 5);
        let t = Table::parse(&wrapped[at..]).unwrap();
        let sub = t.sub_table(0, "t").unwrap().unwrap();
        assert_eq!(sub.name, "Inner");
        assert_eq!(sub.uint(0, "x").unwrap(), 9);
        assert!(t.sub_table(0, "e").unwrap().is_none());
    }

    #[test]
    fn bad_magic_truncation_and_unknown_flags() {
        assert!(matches!(
            Table::parse(b"@UTX\0\0\0\0"),
            Err(Error::BadMagic { .. })
        ));
        let buf = all_types();
        assert!(matches!(
            Table::parse(&buf[..buf.len() - 1]),
            Err(Error::Truncated { .. })
        ));
        let mut bad = table("T", &[Col::Row("x")], &[vec![V::U8(1)]]);
        bad[0x20] = 0x90; // unknown storage nibble
        assert!(matches!(
            Table::parse(&bad),
            Err(Error::UnsupportedColumn(0x90))
        ));
        assert_eq!(find(b"no table here"), None);
    }
}
