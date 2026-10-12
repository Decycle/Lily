//! UDataTable export (`.uexp` of a DataTable package). Spec: docs/research/02-data-tables.md §1.

use crate::Ue4Error;
use crate::package::{PACKAGE_MAGIC, Package};
use crate::property::{Value, read_properties};
use crate::reader::Reader;

#[derive(Clone, Debug, PartialEq)]
pub struct DataTable {
    /// Row struct name, e.g. `MusicParameterTableData`.
    pub row_struct: String,
    /// Rows in file order; each value is a [`Value::Struct`].
    pub rows: Vec<(String, Value)>,
}

impl DataTable {
    pub fn row(&self, name: &str) -> Option<&Value> {
        self.rows.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

/// Parses a cooked DataTable from its `.uasset` header and `.uexp` export data.
pub fn read_datatable(uasset: &[u8], uexp: &[u8]) -> Result<DataTable, Ue4Error> {
    let pkg = Package::parse(uasset)?;
    let mut r = Reader::new(uexp);
    let header = read_properties(&pkg, &mut r, None)?;
    let row_struct = match header.iter().find(|(n, _)| n == "RowStruct") {
        Some((_, Value::Object(i))) => pkg.import_name(*i).ok_or(Ue4Error::BadRowStruct(*i))?,
        _ => return Err(Ue4Error::MissingRowStruct),
    }
    .to_owned();
    r.i32()?; // object GUID flag
    let count = r.count()?;
    let mut rows = Vec::new();
    for _ in 0..count {
        let name = pkg.read_fname(&mut r)?;
        rows.push((name, Value::Struct(read_properties(&pkg, &mut r, None)?)));
    }
    if r.remaining() != PACKAGE_MAGIC.to_le_bytes() {
        log::warn!(
            "{row_struct}: expected package magic after rows, found {} trailing bytes",
            r.remaining().len()
        );
    }
    Ok(DataTable { row_struct, rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    fn table(rows: &[(&str, i32)], trailer: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut fx = Fixture::default();
        let idx = fx.import("/Script/Mercury", "ScriptStruct", "TestRowData");
        let mut b = Vec::new();
        fx.tag(&mut b, "RowStruct", "ObjectProperty", &idx.to_le_bytes());
        fx.none(&mut b);
        b.extend(0i32.to_le_bytes());
        b.extend(i32::try_from(rows.len()).unwrap().to_le_bytes());
        for (name, v) in rows {
            let n = fx.fname(name);
            b.extend(n);
            fx.tag(&mut b, "MaxScore", "IntProperty", &v.to_le_bytes());
            fx.none(&mut b);
        }
        b.extend(trailer);
        (fx.uasset(), b)
    }

    #[test]
    fn reads_row_struct_and_rows() {
        let (ua, ux) = table(
            &[("Data", 1_000_000), ("Other", 5)],
            &PACKAGE_MAGIC.to_le_bytes(),
        );
        let t = read_datatable(&ua, &ux).unwrap();
        assert_eq!(t.row_struct, "TestRowData");
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[1].0, "Other");
        let max = t.row("Data").and_then(|r| r.field("MaxScore"));
        assert_eq!(max, Some(&Value::Int(1_000_000)));
    }

    #[test]
    fn tolerates_unexpected_trailer() {
        let (ua, ux) = table(&[], &[]);
        assert!(read_datatable(&ua, &ux).unwrap().rows.is_empty());
    }

    #[test]
    fn truncated_rows_are_an_error() {
        let (ua, mut ux) = table(&[("Data", 1)], &[]);
        ux.truncate(ux.len() - 3);
        assert!(matches!(
            read_datatable(&ua, &ux),
            Err(Ue4Error::Eof { .. })
        ));
    }

    #[test]
    fn missing_row_struct_is_an_error() {
        let mut fx = Fixture::default();
        let mut b = Vec::new();
        fx.none(&mut b);
        assert!(matches!(
            read_datatable(&fx.uasset(), &b),
            Err(Ue4Error::MissingRowStruct)
        ));
    }
}
