//! Test-only builder for tiny synthetic packages (name map, imports, tagged properties).

use crate::package::PACKAGE_MAGIC;

#[derive(Debug, Default)]
pub(crate) struct Fixture {
    names: Vec<String>,
    /// (class package, class name, object name) as name indices.
    imports: Vec<[i32; 3]>,
}

fn i32le(b: &mut Vec<u8>, v: i32) {
    b.extend(v.to_le_bytes());
}

fn len_i32(n: usize) -> i32 {
    i32::try_from(n).expect("fixture too large")
}

impl Fixture {
    fn index(&mut self, s: &str) -> i32 {
        let i = match self.names.iter().position(|n| n == s) {
            Some(i) => i,
            None => {
                self.names.push(s.to_owned());
                self.names.len() - 1
            }
        };
        len_i32(i)
    }

    pub(crate) fn fname(&mut self, s: &str) -> Vec<u8> {
        let mut b = Vec::new();
        i32le(&mut b, self.index(s));
        i32le(&mut b, 0);
        b
    }

    pub(crate) fn fstring(s: &str) -> Vec<u8> {
        let mut b = Vec::new();
        i32le(&mut b, len_i32(s.len() + 1));
        b.extend(s.as_bytes());
        b.push(0);
        b
    }

    /// Adds an import; returns its package index (negative).
    pub(crate) fn import(&mut self, class_package: &str, class_name: &str, object: &str) -> i32 {
        let entry = [
            self.index(class_package),
            self.index(class_name),
            self.index(object),
        ];
        self.imports.push(entry);
        -len_i32(self.imports.len())
    }

    fn header(&mut self, b: &mut Vec<u8>, name: &str, kind: &str, size: usize, index: i32) {
        let n = self.fname(name);
        b.extend(n);
        let k = self.fname(kind);
        b.extend(k);
        i32le(b, len_i32(size));
        i32le(b, index);
    }

    pub(crate) fn tag_indexed(
        &mut self,
        b: &mut Vec<u8>,
        name: &str,
        kind: &str,
        index: i32,
        value: &[u8],
    ) {
        self.header(b, name, kind, value.len(), index);
        if kind == "ByteProperty" || kind == "EnumProperty" {
            let e = self.fname("None");
            b.extend(e);
        }
        b.push(0); // no property GUID
        b.extend(value);
    }

    pub(crate) fn tag(&mut self, b: &mut Vec<u8>, name: &str, kind: &str, value: &[u8]) {
        self.tag_indexed(b, name, kind, 0, value);
    }

    pub(crate) fn bool_tag(&mut self, b: &mut Vec<u8>, name: &str, v: bool) {
        self.header(b, name, "BoolProperty", 0, 0);
        b.push(u8::from(v));
        b.push(0);
    }

    pub(crate) fn struct_tag_header(
        &mut self,
        b: &mut Vec<u8>,
        name: &str,
        struct_name: &str,
        size: usize,
    ) {
        self.header(b, name, "StructProperty", size, 0);
        let s = self.fname(struct_name);
        b.extend(s);
        b.extend([0; 16]);
        b.push(0);
    }

    pub(crate) fn struct_tag(
        &mut self,
        b: &mut Vec<u8>,
        name: &str,
        struct_name: &str,
        value: &[u8],
    ) {
        self.struct_tag_header(b, name, struct_name, value.len());
        b.extend(value);
    }

    pub(crate) fn array_tag(&mut self, b: &mut Vec<u8>, name: &str, inner: &str, value: &[u8]) {
        self.header(b, name, "ArrayProperty", value.len(), 0);
        let i = self.fname(inner);
        b.extend(i);
        b.push(0);
        b.extend(value);
    }

    pub(crate) fn map_tag(
        &mut self,
        b: &mut Vec<u8>,
        name: &str,
        key: &str,
        val: &str,
        value: &[u8],
    ) {
        self.header(b, name, "MapProperty", value.len(), 0);
        let k = self.fname(key);
        b.extend(k);
        let v = self.fname(val);
        b.extend(v);
        b.push(0);
        b.extend(value);
    }

    pub(crate) fn none(&mut self, b: &mut Vec<u8>) {
        let n = self.fname("None");
        b.extend(n);
    }

    /// Package summary (legacy −7, filter-editor-only), name map, import table.
    pub(crate) fn uasset(&self) -> Vec<u8> {
        let summary = |name_off: i32, import_off: i32| {
            let mut b = Vec::new();
            b.extend(PACKAGE_MAGIC.to_le_bytes());
            for v in [-7, 0, 0, 0, 0, 0] {
                i32le(&mut b, v); // legacy, UE3, UE4, licensee, custom versions, header size
            }
            b.extend(Self::fstring("None"));
            b.extend(0x8000_0000u32.to_le_bytes());
            let rest = [len_i32(self.names.len()), name_off, 0, 0, 1, 0];
            rest.iter().for_each(|&v| i32le(&mut b, v));
            i32le(&mut b, len_i32(self.imports.len()));
            i32le(&mut b, import_off);
            b
        };
        let mut names = Vec::new();
        for n in &self.names {
            names.extend(Self::fstring(n));
            names.extend([0; 4]);
        }
        let name_off = summary(0, 0).len();
        let import_off = name_off + names.len();
        let mut b = summary(len_i32(name_off), len_i32(import_off));
        b.extend(names);
        for [pkg, class, object] in &self.imports {
            for v in [*pkg, 0, *class, 0, 0, *object, 0] {
                i32le(&mut b, v);
            }
        }
        b
    }
}
