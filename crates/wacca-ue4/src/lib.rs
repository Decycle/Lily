//! UE4 4.19 cooked package reader (DataTables, textures, widgets). Spec: docs/research/02-data-tables.md, 06-visuals.md
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

mod datatable;
#[cfg(test)]
mod fixture;
mod json;
pub mod package;
mod property;
mod reader;

pub use datatable::{DataTable, read_datatable};
pub use property::{Text, Value};

#[derive(Debug, thiserror::Error)]
pub enum Ue4Error {
    #[error("unexpected end of data at offset {offset:#x} (need {need} bytes, length {len})")]
    Eof {
        offset: usize,
        need: usize,
        len: usize,
    },
    #[error("bad package magic {0:#010x}")]
    BadMagic(u32),
    #[error("invalid string length {len} at offset {offset:#x}")]
    BadStringLength { len: i32, offset: usize },
    #[error("invalid count {count} at offset {offset:#x}")]
    BadCount { count: i32, offset: usize },
    #[error("name index {0} out of range")]
    BadNameIndex(i32),
    #[error("unsupported property type {0}")]
    UnsupportedProperty(String),
    #[error("unsupported FText history {0}")]
    UnsupportedTextHistory(i8),
    #[error("DataTable has no RowStruct object property")]
    MissingRowStruct,
    #[error("RowStruct index {0} is not an import")]
    BadRowStruct(i32),
}
