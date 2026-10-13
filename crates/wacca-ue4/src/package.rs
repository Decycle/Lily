//! Cooked package header (`.uasset`): summary, name map and import table.
//!
//! Only what DataTables and textures need is decoded (names, imports, export table).

use crate::Ue4Error;
use crate::reader::Reader;

pub(crate) const PACKAGE_MAGIC: u32 = 0x9E2A_83C1;
const PKG_FILTER_EDITOR_ONLY: u32 = 0x8000_0000;
/// Size of one custom-version entry (FGuid + i32) in the optimized format.
const CUSTOM_VERSION_SIZE: usize = 20;

/// One entry of the import table.
#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    pub class_package: String,
    pub class_name: String,
    pub outer_index: i32,
    pub object_name: String,
}

/// One entry of the export table (UE 4.19 layout, 104 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct Export {
    /// Package index of the class (usually an import, e.g. `Texture2D`).
    pub class_index: i32,
    /// Package index of the owning object (0 = the package itself).
    pub outer_index: i32,
    pub object_name: String,
    pub serial_size: u64,
    /// Offset in the combined `.uasset` + `.uexp` file.
    pub serial_offset: u64,
}

/// Export entry bytes after SerialOffset: 3 bools, package GUID, package flags, 2 bools, 5 dependency ints.
const EXPORT_TRAILER_SIZE: usize = 12 + 16 + 4 + 8 + 20;

/// The parts of a `.uasset` header needed to decode its exports.
#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    pub names: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

impl Package {
    pub fn parse(uasset: &[u8]) -> Result<Self, Ue4Error> {
        let mut r = Reader::new(uasset);
        let magic = r.u32()?;
        if magic != PACKAGE_MAGIC {
            return Err(Ue4Error::BadMagic(magic));
        }
        let legacy = r.i32()?;
        if legacy != -4 {
            r.i32()?; // LegacyUE3Version
        }
        r.i32()?; // FileVersionUE4
        r.i32()?; // FileVersionLicenseeUE4
        if legacy <= -2 {
            let n = r.count()?;
            r.bytes(n.saturating_mul(CUSTOM_VERSION_SIZE))?;
        }
        r.i32()?; // TotalHeaderSize
        r.fstring()?; // FolderName
        let flags = r.u32()?;
        let name_count = r.count()?;
        let name_offset = r.count()?;
        if flags & PKG_FILTER_EDITOR_ONLY == 0 {
            r.fstring()?; // LocalizationId
        }
        r.i32()?; // GatherableTextDataCount
        r.i32()?; // GatherableTextDataOffset
        let export_count = r.count()?;
        let export_offset = r.count()?;
        let import_count = r.count()?;
        let import_offset = r.count()?;

        let mut nr = Reader::at(uasset, name_offset)?;
        let mut names = Vec::new();
        for _ in 0..name_count {
            names.push(nr.fstring()?);
            nr.u32()?; // case-insensitive + case-sensitive hashes
        }

        let mut pkg = Self {
            names,
            imports: Vec::new(),
            exports: Vec::new(),
        };
        let mut ir = Reader::at(uasset, import_offset)?;
        for _ in 0..import_count {
            let import = Import {
                class_package: pkg.read_fname(&mut ir)?,
                class_name: pkg.read_fname(&mut ir)?,
                outer_index: ir.i32()?,
                object_name: pkg.read_fname(&mut ir)?,
            };
            pkg.imports.push(import);
        }
        let mut er = Reader::at(uasset, export_offset)?;
        for _ in 0..export_count {
            let class_index = er.i32()?;
            er.bytes(8)?; // SuperIndex, TemplateIndex
            let outer_index = er.i32()?;
            let object_name = pkg.read_fname(&mut er)?;
            er.u32()?; // ObjectFlags
            let serial_size = er.u64()?;
            let serial_offset = er.u64()?;
            er.bytes(EXPORT_TRAILER_SIZE)?;
            pkg.exports.push(Export {
                class_index,
                outer_index,
                object_name,
                serial_size,
                serial_offset,
            });
        }
        Ok(pkg)
    }

    /// Exports whose class is the import named `class`, in table order.
    pub fn exports_of_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a Export> {
        self.exports
            .iter()
            .filter(move |e| self.import_name(e.class_index) == Some(class))
    }

    /// Resolves an `FName` (index + instance number) to text, e.g. `Foo` or `Foo_2` (number 3).
    pub fn name(&self, index: i32, number: i32) -> Result<String, Ue4Error> {
        let name = usize::try_from(index)
            .ok()
            .and_then(|i| self.names.get(i))
            .ok_or(Ue4Error::BadNameIndex(index))?;
        Ok(if number == 0 {
            name.clone()
        } else {
            format!("{name}_{}", number - 1)
        })
    }

    pub(crate) fn read_fname(&self, r: &mut Reader) -> Result<String, Ue4Error> {
        let index = r.i32()?;
        let number = r.i32()?;
        self.name(index, number)
    }

    /// Object name of a package index; only imports (negative indices) are resolved.
    pub fn import_name(&self, index: i32) -> Option<&str> {
        let i = usize::try_from(-i64::from(index) - 1).ok()?;
        self.imports.get(i).map(|imp| imp.object_name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    #[test]
    fn parses_names_and_imports() {
        let mut fx = Fixture::default();
        fx.import("/Script/CoreUObject", "ScriptStruct", "MyRow");
        let pkg = Package::parse(&fx.uasset()).unwrap();
        assert!(pkg.names.contains(&"MyRow".to_owned()));
        assert_eq!(pkg.imports.len(), 1);
        assert_eq!(pkg.imports[0].class_name, "ScriptStruct");
        assert_eq!(pkg.import_name(-1), Some("MyRow"));
        assert_eq!(pkg.import_name(0), None);
        assert_eq!(pkg.import_name(-2), None);
    }

    #[test]
    fn parses_exports_and_filters_by_class() {
        let mut fx = Fixture::default();
        let tex = fx.import("/Script/CoreUObject", "Class", "Texture2D");
        let mat = fx.import("/Script/CoreUObject", "Class", "Material");
        fx.export(tex, "A", 10);
        fx.export(mat, "B", 20);
        fx.export(tex, "C", 30);
        let ua = fx.uasset();
        let pkg = Package::parse(&ua).unwrap();
        assert_eq!(pkg.exports.len(), 3);
        assert_eq!(pkg.exports[1].object_name, "B");
        assert_eq!(pkg.exports[1].serial_size, 20);
        assert_eq!(pkg.exports[0].serial_offset, ua.len() as u64);
        assert_eq!(pkg.exports[2].serial_offset, ua.len() as u64 + 30);
        let names: Vec<_> = pkg
            .exports_of_class("Texture2D")
            .map(|e| &e.object_name)
            .collect();
        assert_eq!(names, ["A", "C"]);
    }

    #[test]
    fn name_numbers_become_suffixes() {
        let pkg = Package {
            names: vec!["Foo".into()],
            imports: vec![],
            exports: vec![],
        };
        assert_eq!(pkg.name(0, 0).unwrap(), "Foo");
        assert_eq!(pkg.name(0, 3).unwrap(), "Foo_2");
        assert!(matches!(pkg.name(1, 0), Err(Ue4Error::BadNameIndex(1))));
        assert!(pkg.name(-1, 0).is_err());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut data = Fixture::default().uasset();
        data[0] = 0;
        assert!(matches!(Package::parse(&data), Err(Ue4Error::BadMagic(_))));
    }
}
