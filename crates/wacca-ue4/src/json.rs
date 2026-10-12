//! JSON view of decoded values, for debugging dumps. Shape matches `tools/research/ue4_datatable.py`.

use serde_json::{Map, Value as Json, json};

use crate::datatable::DataTable;
use crate::property::{Text, Value};

/// `f32` via its shortest decimal form, so 12.6 stays 12.6 rather than 12.600000381….
fn float(v: f32) -> Json {
    v.to_string()
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or(Json::Null, Json::Number)
}

impl Value {
    pub fn to_json(&self) -> Json {
        match self {
            Value::Int(v) => json!(v),
            Value::UInt(v) => json!(v),
            Value::Float(v) => float(*v),
            Value::Double(v) => json!(v),
            Value::Bool(v) => json!(v),
            Value::Byte(v) => json!(v),
            Value::Str(s) | Value::Name(s) | Value::Enum(s) | Value::SoftObject(s) => json!(s),
            Value::Text(Text::Literal(s)) => json!({ "text": s }),
            Value::Text(Text::Localized {
                namespace,
                key,
                source,
            }) => {
                json!({ "ns": namespace, "key": key, "text": source })
            }
            Value::Text(Text::StringTable { table, key }) => {
                json!({ "string_table": table, "key": key })
            }
            Value::Object(i) => json!({ "object_index": i }),
            Value::Struct(fields) => fields_json(fields),
            Value::Array(items) => Json::Array(items.iter().map(Value::to_json).collect()),
            Value::Map(pairs) => pairs
                .iter()
                .map(|(k, v)| json!([k.to_json(), v.to_json()]))
                .collect(),
            Value::Unparsed { kind, bytes } => {
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                json!({ "__unparsed__": kind, "hex": hex })
            }
        }
    }
}

fn fields_json(fields: &[(String, Value)]) -> Json {
    Json::Object(
        fields
            .iter()
            .map(|(k, v)| (k.clone(), v.to_json()))
            .collect::<Map<_, _>>(),
    )
}

impl DataTable {
    /// `{"row_struct": ..., "rows": {RowName: {...}}}`.
    pub fn to_json(&self) -> Json {
        json!({ "row_struct": self.row_struct, "rows": fields_json(&self.rows) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_to_json() {
        let v = Value::Struct(vec![
            ("Lv".into(), Value::Float(12.6)),
            ("Id".into(), Value::UInt(3030)),
            ("Pos".into(), Value::Array(vec![Value::Float(-600.0)])),
            ("O".into(), Value::Object(-1)),
            ("T".into(), Value::Text(Text::Literal("x".into()))),
        ]);
        let expected = json!({"Lv": 12.6, "Id": 3030, "Pos": [-600.0], "O": {"object_index": -1}, "T": {"text": "x"}});
        assert_eq!(v.to_json(), expected);
    }

    #[test]
    fn table_to_json() {
        let t = DataTable {
            row_struct: "R".into(),
            rows: vec![(
                "Data".into(),
                Value::Struct(vec![("A".into(), Value::Bool(true))]),
            )],
        };
        assert_eq!(
            t.to_json(),
            json!({"row_struct": "R", "rows": {"Data": {"A": true}}})
        );
    }
}
