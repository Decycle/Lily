//! Tagged property serialization (UE 4.19, cooked, not unversioned).

use crate::Ue4Error;
use crate::package::Package;
use crate::reader::Reader;

/// A decoded property value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Int8/16/32/64Property.
    Int(i64),
    /// UInt16/32/64Property.
    UInt(u64),
    Float(f32),
    Double(f64),
    Bool(bool),
    /// ByteProperty without an enum.
    Byte(u8),
    Str(String),
    Name(String),
    /// EnumProperty, or ByteProperty with an enum.
    Enum(String),
    Text(Text),
    /// Package index (negative = import, positive = export).
    Object(i32),
    /// Soft object/class path, `asset` or `asset:subpath`.
    SoftObject(String),
    /// Tagged or native struct as named fields.
    Struct(Vec<(String, Value)>),
    /// Array, Set, and native vector-like structs (Vector, Quat, IntPoint, ...).
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    /// A property that failed to decode; its raw bytes (tag size trusted).
    Unparsed {
        kind: String,
        bytes: Vec<u8>,
    },
}

/// FText, by history type.
#[derive(Clone, Debug, PartialEq)]
pub enum Text {
    /// History −1 (culture invariant / none).
    Literal(String),
    /// History 0 (base).
    Localized {
        namespace: String,
        key: String,
        source: String,
    },
    /// History 11.
    StringTable { table: String, key: String },
}

impl Value {
    /// A field of a struct value.
    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Struct(fields) => fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Name(s) | Value::Enum(s) | Value::SoftObject(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Int(v) => Some(v),
            Value::UInt(v) => i64::try_from(v).ok(),
            Value::Byte(v) => Some(v.into()),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            Value::Float(v) => Some(v),
            _ => None,
        }
    }
}

/// Property tag header.
#[derive(Debug, Default)]
struct Tag {
    name: String,
    kind: String,
    size: usize,
    index: i32,
    struct_name: Option<String>,
    bool_value: bool,
    /// Inner type of Array/Set, key type of Map.
    inner: Option<String>,
    /// Value type of Map.
    value_kind: Option<String>,
}

impl Tag {
    /// Synthetic tag for a container element (no size, no struct name).
    fn element(kind: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            ..Self::default()
        }
    }
}

const GUID_SIZE: usize = 16;

fn read_tag(pkg: &Package, r: &mut Reader) -> Result<Option<Tag>, Ue4Error> {
    let name = pkg.read_fname(r)?;
    if name == "None" {
        return Ok(None);
    }
    let kind = pkg.read_fname(r)?;
    let size = r.count()?;
    let index = r.i32()?;
    let mut tag = Tag {
        name,
        size,
        index,
        ..Tag::default()
    };
    match kind.as_str() {
        "StructProperty" => {
            tag.struct_name = Some(pkg.read_fname(r)?);
            r.bytes(GUID_SIZE)?;
        }
        "BoolProperty" => tag.bool_value = r.u8()? != 0,
        "ByteProperty" | "EnumProperty" => {
            pkg.read_fname(r)?; // enum name
        }
        "ArrayProperty" | "SetProperty" => tag.inner = Some(pkg.read_fname(r)?),
        "MapProperty" => {
            tag.inner = Some(pkg.read_fname(r)?);
            tag.value_kind = Some(pkg.read_fname(r)?);
        }
        _ => {}
    }
    tag.kind = kind;
    if r.u8()? != 0 {
        r.bytes(GUID_SIZE)?; // property GUID
    }
    Ok(Some(tag))
}

/// Reads tagged properties up to the `None` terminator (or `end`, if given).
///
/// Tag sizes are trusted: a value that fails to decode becomes [`Value::Unparsed`] and a size
/// mismatch is logged; either way reading resumes after the tagged size.
pub(crate) fn read_properties(
    pkg: &Package,
    r: &mut Reader,
    end: Option<usize>,
) -> Result<Vec<(String, Value)>, Ue4Error> {
    let mut out = Vec::new();
    while end.is_none_or(|e| r.pos() < e) {
        let Some(tag) = read_tag(pkg, r)? else { break };
        let start = r.pos();
        let value = match read_value(pkg, r, &tag, Some(tag.size)) {
            Ok(v) => {
                if r.pos() != start + tag.size {
                    log::warn!(
                        "{} ({}): consumed {} bytes, tag says {}",
                        tag.name,
                        tag.kind,
                        r.pos() - start,
                        tag.size
                    );
                }
                v
            }
            Err(e) => {
                log::warn!("{} ({}): {e}; keeping raw bytes", tag.name, tag.kind);
                r.seek(start)?;
                Value::Unparsed {
                    kind: tag.kind.clone(),
                    bytes: r.bytes(tag.size)?.to_vec(),
                }
            }
        };
        r.seek(start + tag.size)?;
        let key = if tag.index == 0 {
            tag.name
        } else {
            format!("{}[{}]", tag.name, tag.index)
        };
        out.push((key, value));
    }
    Ok(out)
}

/// `size` is the tagged size for top-level properties and `None` for container elements.
fn read_value(
    pkg: &Package,
    r: &mut Reader,
    tag: &Tag,
    size: Option<usize>,
) -> Result<Value, Ue4Error> {
    Ok(match tag.kind.as_str() {
        "Int8Property" => Value::Int(r.i8()?.into()),
        "Int16Property" => Value::Int(r.i16()?.into()),
        "IntProperty" => Value::Int(r.i32()?.into()),
        "Int64Property" => Value::Int(r.i64()?),
        "UInt16Property" => Value::UInt(r.u16()?.into()),
        "UInt32Property" => Value::UInt(r.u32()?.into()),
        "UInt64Property" => Value::UInt(r.u64()?),
        "FloatProperty" => Value::Float(r.f32()?),
        "DoubleProperty" => Value::Double(r.f64()?),
        "BoolProperty" => Value::Bool(match size {
            Some(_) => tag.bool_value,
            None => r.u8()? != 0,
        }),
        "StrProperty" => Value::Str(r.fstring()?),
        "NameProperty" => Value::Name(pkg.read_fname(r)?),
        "TextProperty" => Value::Text(read_text(pkg, r)?),
        "ByteProperty" => match size {
            None | Some(1) => Value::Byte(r.u8()?),
            Some(_) => Value::Enum(pkg.read_fname(r)?),
        },
        "EnumProperty" => Value::Enum(pkg.read_fname(r)?),
        "SoftObjectProperty" | "SoftClassProperty" | "AssetObjectProperty" => {
            Value::SoftObject(read_soft_path(pkg, r)?)
        }
        "ObjectProperty" | "ClassProperty" | "InterfaceProperty" | "WeakObjectProperty"
        | "LazyObjectProperty" => Value::Object(r.i32()?),
        "StructProperty" => match (tag.struct_name.as_deref(), size) {
            // Material graph inputs are natively serialized (editor data); keep them raw.
            (Some(n), Some(size)) if n.ends_with("MaterialInput") || n == "ExpressionInput" => {
                Value::Unparsed {
                    kind: n.to_owned(),
                    bytes: r.bytes(size)?.to_vec(),
                }
            }
            (name, _) => read_struct(pkg, r, name, size)?,
        },
        "ArrayProperty" | "SetProperty" => {
            let inner = tag.inner.as_deref().ok_or_else(|| unsupported(tag))?;
            if tag.kind == "SetProperty" {
                r.i32()?; // number of elements to remove
            }
            let n = r.count()?;
            let mut items = Vec::new();
            if inner == "StructProperty" {
                let inner_tag = read_tag(pkg, r)?.ok_or_else(|| unsupported(tag))?;
                for _ in 0..n {
                    items.push(read_struct(pkg, r, inner_tag.struct_name.as_deref(), None)?);
                }
            } else {
                let element = Tag::element(inner);
                for _ in 0..n {
                    items.push(read_value(pkg, r, &element, None)?);
                }
            }
            Value::Array(items)
        }
        "MapProperty" => {
            let (Some(k), Some(v)) = (&tag.inner, &tag.value_kind) else {
                return Err(unsupported(tag));
            };
            let (key_tag, value_tag) = (Tag::element(k), Tag::element(v));
            r.i32()?; // number of keys to remove
            let n = r.count()?;
            let mut pairs = Vec::new();
            for _ in 0..n {
                let key = read_value(pkg, r, &key_tag, None)?;
                pairs.push((key, read_value(pkg, r, &value_tag, None)?));
            }
            Value::Map(pairs)
        }
        _ => return Err(unsupported(tag)),
    })
}

fn unsupported(tag: &Tag) -> Ue4Error {
    Ue4Error::UnsupportedProperty(tag.kind.clone())
}

fn read_text(pkg: &Package, r: &mut Reader) -> Result<Text, Ue4Error> {
    r.u32()?; // flags
    match r.i8()? {
        -1 => {
            let has_string = r.i32()? != 0;
            Ok(Text::Literal(if has_string {
                r.fstring()?
            } else {
                String::new()
            }))
        }
        0 => Ok(Text::Localized {
            namespace: r.fstring()?,
            key: r.fstring()?,
            source: r.fstring()?,
        }),
        11 => Ok(Text::StringTable {
            table: pkg.read_fname(r)?,
            key: r.fstring()?,
        }),
        h => Err(Ue4Error::UnsupportedTextHistory(h)),
    }
}

fn read_soft_path(pkg: &Package, r: &mut Reader) -> Result<String, Ue4Error> {
    let asset = pkg.read_fname(r)?;
    let sub = r.fstring()?;
    Ok(if sub.is_empty() {
        asset
    } else {
        format!("{asset}:{sub}")
    })
}

fn floats(r: &mut Reader, n: usize) -> Result<Value, Ue4Error> {
    (0..n)
        .map(|_| r.f32().map(Value::Float))
        .collect::<Result<_, _>>()
        .map(Value::Array)
}

/// Native structs have fixed binary layouts; anything else is nested tagged properties.
fn read_struct(
    pkg: &Package,
    r: &mut Reader,
    name: Option<&str>,
    size: Option<usize>,
) -> Result<Value, Ue4Error> {
    Ok(match name {
        Some("Vector2D") => floats(r, 2)?,
        Some("Vector" | "Rotator") => floats(r, 3)?,
        Some("Vector4" | "Quat" | "LinearColor" | "Margin") => floats(r, 4)?,
        Some("Color") => {
            let [b, g, red, a] = [r.u8()?, r.u8()?, r.u8()?, r.u8()?];
            Value::Struct(
                [("R", red), ("G", g), ("B", b), ("A", a)]
                    .map(|(n, v)| (n.to_owned(), Value::Byte(v)))
                    .to_vec(),
            )
        }
        Some("Box") => {
            let v = floats(r, 6)?;
            r.u8()?; // IsValid
            v
        }
        Some("IntPoint") => Value::Array(vec![
            Value::Int(r.i32()?.into()),
            Value::Int(r.i32()?.into()),
        ]),
        Some("Guid") => Value::Str(
            r.bytes(GUID_SIZE)?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        ),
        Some("DateTime" | "Timespan") => Value::Int(r.i64()?),
        Some("SoftObjectPath" | "SoftClassPath" | "StringAssetReference") => {
            Value::SoftObject(read_soft_path(pkg, r)?)
        }
        _ => {
            let end = size.map(|s| r.pos() + s);
            Value::Struct(read_properties(pkg, r, end)?)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    fn parse(fx: &Fixture, body: &[u8]) -> Vec<(String, Value)> {
        let pkg = Package::parse(&fx.uasset()).unwrap();
        let mut r = Reader::new(body);
        let props = read_properties(&pkg, &mut r, None).unwrap();
        assert!(r.remaining().is_empty(), "terminator not consumed");
        props
    }

    #[test]
    fn scalar_properties() {
        let mut fx = Fixture::default();
        let mut b = Vec::new();
        fx.tag(&mut b, "I", "IntProperty", &(-3i32).to_le_bytes());
        fx.tag(&mut b, "U", "UInt32Property", &7u32.to_le_bytes());
        fx.tag(&mut b, "L", "Int64Property", &2021081007i64.to_le_bytes());
        fx.tag(&mut b, "Q", "UInt64Property", &0u64.to_le_bytes());
        fx.tag(&mut b, "F", "FloatProperty", &12.6f32.to_le_bytes());
        fx.tag(&mut b, "S", "StrProperty", &Fixture::fstring("S03-030"));
        fx.tag(&mut b, "Y", "ByteProperty", &[3]);
        fx.bool_tag(&mut b, "B", true);
        fx.none(&mut b);
        let props = parse(&fx, &b);
        let v = Value::Struct(props);
        assert_eq!(v.field("I"), Some(&Value::Int(-3)));
        assert_eq!(v.field("U"), Some(&Value::UInt(7)));
        assert_eq!(v.field("L").and_then(Value::as_i64), Some(2021081007));
        assert_eq!(v.field("Q"), Some(&Value::UInt(0)));
        assert_eq!(v.field("F").and_then(Value::as_f32), Some(12.6));
        assert_eq!(v.field("S").and_then(Value::as_str), Some("S03-030"));
        assert_eq!(v.field("Y"), Some(&Value::Byte(3)));
        assert_eq!(v.field("B"), Some(&Value::Bool(true)));
    }

    #[test]
    fn name_enum_text_and_object() {
        let mut fx = Fixture::default();
        let mut b = Vec::new();
        let name = fx.fname("Hello");
        fx.tag(&mut b, "N", "NameProperty", &name);
        let e = fx.fname("EFoo::Bar");
        fx.tag(&mut b, "E", "EnumProperty", &e); // enum-name extra handled by Fixture
        let mut text = vec![0, 0, 0, 0, 0xff, 1, 0, 0, 0];
        text.extend(Fixture::fstring("hi"));
        fx.tag(&mut b, "T", "TextProperty", &text);
        fx.tag(&mut b, "O", "ObjectProperty", &(-1i32).to_le_bytes());
        let mut soft = fx.fname("/Game/UI/Tex");
        soft.extend(Fixture::fstring(""));
        fx.tag(&mut b, "P", "SoftObjectProperty", &soft);
        fx.none(&mut b);
        let v = Value::Struct(parse(&fx, &b));
        assert_eq!(v.field("N"), Some(&Value::Name("Hello".into())));
        assert_eq!(v.field("E"), Some(&Value::Enum("EFoo::Bar".into())));
        assert_eq!(v.field("T"), Some(&Value::Text(Text::Literal("hi".into()))));
        assert_eq!(v.field("O"), Some(&Value::Object(-1)));
        assert_eq!(
            v.field("P"),
            Some(&Value::SoftObject("/Game/UI/Tex".into()))
        );
    }

    #[test]
    fn native_and_tagged_structs() {
        let mut fx = Fixture::default();
        let mut b = Vec::new();
        let vec3: Vec<u8> = [0.0f32, -600.0, 0.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        fx.struct_tag(&mut b, "Pos", "Vector", &vec3);
        let mut inner = Vec::new();
        fx.tag(&mut inner, "X", "IntProperty", &5i32.to_le_bytes());
        fx.none(&mut inner);
        fx.struct_tag(&mut b, "Nested", "MyStruct", &inner);
        fx.none(&mut b);
        let v = Value::Struct(parse(&fx, &b));
        let pos = Value::Array(vec![
            Value::Float(0.0),
            Value::Float(-600.0),
            Value::Float(0.0),
        ]);
        assert_eq!(v.field("Pos"), Some(&pos));
        assert_eq!(
            v.field("Nested").and_then(|n| n.field("X")),
            Some(&Value::Int(5))
        );
    }

    #[test]
    fn arrays_and_maps() {
        let mut fx = Fixture::default();
        let mut b = Vec::new();
        let mut ints = 2i32.to_le_bytes().to_vec();
        ints.extend(1i32.to_le_bytes());
        ints.extend(2i32.to_le_bytes());
        fx.array_tag(&mut b, "A", "IntProperty", &ints);
        // Array of structs: count, inner tag, then tagged structs.
        let mut structs = 1i32.to_le_bytes().to_vec();
        fx.struct_tag_header(&mut structs, "A2", "MyStruct", 0);
        fx.tag(&mut structs, "X", "IntProperty", &9i32.to_le_bytes());
        fx.none(&mut structs);
        fx.array_tag(&mut b, "A2", "StructProperty", &structs);
        let mut map = 0i32.to_le_bytes().to_vec();
        map.extend(1i32.to_le_bytes());
        map.extend(Fixture::fstring("k"));
        map.extend(4.5f32.to_le_bytes());
        fx.map_tag(&mut b, "M", "StrProperty", "FloatProperty", &map);
        fx.none(&mut b);
        let v = Value::Struct(parse(&fx, &b));
        assert_eq!(
            v.field("A"),
            Some(&Value::Array(vec![Value::Int(1), Value::Int(2)]))
        );
        let s = Value::Struct(vec![("X".into(), Value::Int(9))]);
        assert_eq!(v.field("A2"), Some(&Value::Array(vec![s])));
        let m = Value::Map(vec![(Value::Str("k".into()), Value::Float(4.5))]);
        assert_eq!(v.field("M"), Some(&m));
    }

    #[test]
    fn array_index_suffixes_key() {
        let mut fx = Fixture::default();
        let mut b = Vec::new();
        fx.tag_indexed(&mut b, "I", "IntProperty", 2, &1i32.to_le_bytes());
        fx.none(&mut b);
        assert_eq!(parse(&fx, &b)[0].0, "I[2]");
    }

    #[test]
    fn unknown_type_is_kept_raw_and_reading_continues() {
        let mut fx = Fixture::default();
        let mut b = Vec::new();
        fx.tag(&mut b, "W", "WeirdProperty", &[1, 2, 3]);
        fx.tag(&mut b, "I", "IntProperty", &4i32.to_le_bytes());
        fx.none(&mut b);
        let v = Value::Struct(parse(&fx, &b));
        let raw = Value::Unparsed {
            kind: "WeirdProperty".into(),
            bytes: vec![1, 2, 3],
        };
        assert_eq!(v.field("W"), Some(&raw));
        assert_eq!(v.field("I"), Some(&Value::Int(4)));
    }

    #[test]
    fn size_mismatch_trusts_tag_size() {
        let mut fx = Fixture::default();
        let mut b = Vec::new();
        fx.tag(&mut b, "I", "IntProperty", &[7, 0, 0, 0, 0xaa, 0xbb]);
        fx.none(&mut b);
        assert_eq!(parse(&fx, &b)[0].1, Value::Int(7));
    }
}
