//! Material parameters of a cooked `Material` (parameter expressions' defaults, blend mode) or
//! `MaterialInstanceConstant` (overrides + parent). The compiled shaders are ported by hand;
//! this supplies their constants.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Ue4Error;
use crate::object::{Object, Objects, read_objects};
use crate::property::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MaterialParams {
    pub name: String,
    /// Parent material (instances only).
    pub parent: Option<String>,
    /// `EBlendMode` name, when set here.
    pub blend: Option<String>,
    pub scalars: BTreeMap<String, f32>,
    pub vectors: BTreeMap<String, [f32; 4]>,
    /// Texture per parameter name (or per expression name for non-parameter samples).
    pub textures: BTreeMap<String, String>,
}

impl MaterialParams {
    /// `self` (an instance) over its parent's values.
    pub fn over(&self, parent: &MaterialParams) -> MaterialParams {
        let mut out = parent.clone();
        out.name.clone_from(&self.name);
        out.parent = Some(parent.name.clone());
        if self.blend.is_some() {
            out.blend.clone_from(&self.blend);
        }
        out.scalars.extend(self.scalars.clone());
        out.vectors.extend(self.vectors.clone());
        out.textures.extend(self.textures.clone());
        out
    }
}

fn name_of(objs: &Objects, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Object(i)) => objs.name(*i).map(str::to_owned),
        _ => None,
    }
}

fn vec4(v: Option<&Value>) -> [f32; 4] {
    let mut out = [0.0; 4];
    if let Some(Value::Array(items)) = v {
        for (o, x) in out.iter_mut().zip(items) {
            *o = x.as_f32().unwrap_or(0.0);
        }
    }
    out
}

/// Reads the material or material instance export of a package.
pub fn read_material(uasset: &[u8], uexp: &[u8]) -> Result<MaterialParams, Ue4Error> {
    let objs = read_objects(uasset, uexp)?;
    let main = objs
        .exports
        .iter()
        .find(|o| {
            o.outer == 0 && matches!(o.class.as_str(), "Material" | "MaterialInstanceConstant")
        })
        .ok_or(Ue4Error::NotAMaterial)?;
    let mut m = MaterialParams {
        name: main.name.clone(),
        ..MaterialParams::default()
    };
    if main.class == "Material" {
        m.blend = Some(
            main.get("BlendMode")
                .and_then(Value::as_str)
                .unwrap_or("BLEND_Opaque")
                .to_owned(),
        );
        for e in &objs.exports {
            read_expression(&objs, e, &mut m);
        }
    } else {
        m.parent = name_of(&objs, main.get("Parent"));
        if let Some(o) = main.get("BasePropertyOverrides")
            && let Some(b) = o.field("BlendMode").and_then(Value::as_str)
            && o.field("bOverride_BlendMode") == Some(&Value::Bool(true))
        {
            m.blend = Some(b.to_owned());
        }
        for (list, kind) in [
            ("ScalarParameterValues", 0),
            ("VectorParameterValues", 1),
            ("TextureParameterValues", 2),
        ] {
            let Some(Value::Array(items)) = main.get(list) else {
                continue;
            };
            for p in items {
                let Some(name) = p
                    .field("ParameterInfo")
                    .and_then(|i| i.field("Name"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let value = p.field("ParameterValue");
                match kind {
                    0 => {
                        let v = value.and_then(Value::as_f32).unwrap_or(0.0);
                        m.scalars.insert(name.to_owned(), v);
                    }
                    1 => {
                        m.vectors.insert(name.to_owned(), vec4(value));
                    }
                    _ => {
                        if let Some(t) = name_of(&objs, value) {
                            m.textures.insert(name.to_owned(), t);
                        }
                    }
                }
            }
        }
    }
    Ok(m)
}

fn read_expression(objs: &Objects, e: &Object, m: &mut MaterialParams) {
    let param = e.get("ParameterName").and_then(Value::as_str);
    match (e.class.as_str(), param) {
        ("MaterialExpressionScalarParameter", Some(p)) => {
            let v = e.get("DefaultValue").and_then(Value::as_f32).unwrap_or(0.0);
            m.scalars.insert(p.to_owned(), v);
        }
        ("MaterialExpressionVectorParameter", Some(p)) => {
            m.vectors.insert(p.to_owned(), vec4(e.get("DefaultValue")));
        }
        (c, p) if c.starts_with("MaterialExpressionTextureSample") => {
            if let Some(t) = name_of(objs, e.get("Texture")) {
                m.textures.insert(p.unwrap_or(&e.name).to_owned(), t);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    #[test]
    fn instance_overrides_parent_defaults() {
        let mut fx = Fixture::default();
        let c_mat = fx.import("/Script/CoreUObject", "Class", "Material");
        let c_scalar = fx.import(
            "/Script/CoreUObject",
            "Class",
            "MaterialExpressionScalarParameter",
        );
        let c_tex = fx.import(
            "/Script/CoreUObject",
            "Class",
            "MaterialExpressionTextureSampleParameter2D",
        );
        let tex = fx.import("/Script/CoreUObject", "Texture2D", "vT_Ramp");
        let mut mat = Vec::new();
        let blend = fx.fname("BLEND_Translucent");
        fx.tag(&mut mat, "BlendMode", "EnumProperty", &blend);
        fx.none(&mut mat);
        let mut s = Vec::new();
        fx.tag(
            &mut s,
            "DefaultValue",
            "FloatProperty",
            &2.0f32.to_le_bytes(),
        );
        let n = fx.fname("Gamma");
        fx.tag(&mut s, "ParameterName", "NameProperty", &n);
        fx.none(&mut s);
        let mut t = Vec::new();
        let n = fx.fname("ColorTexture");
        fx.tag(&mut t, "ParameterName", "NameProperty", &n);
        fx.tag(&mut t, "Texture", "ObjectProperty", &tex.to_le_bytes());
        fx.none(&mut t);
        fx.export(c_mat, "vMX_Base", mat.len());
        fx.export_in(c_scalar, "e0", s.len(), 1);
        fx.export_in(c_tex, "e1", t.len(), 1);
        let parent = read_material(&fx.uasset(), &[mat, s, t].concat()).unwrap();
        assert_eq!(parent.blend.as_deref(), Some("BLEND_Translucent"));
        assert_eq!(parent.scalars["Gamma"], 2.0);
        assert_eq!(parent.textures["ColorTexture"], "vT_Ramp");
        let mut inst = MaterialParams {
            name: "MI".into(),
            blend: Some("BLEND_Additive".into()),
            ..MaterialParams::default()
        };
        inst.scalars.insert("Gamma".into(), 4.0);
        let merged = inst.over(&parent);
        assert_eq!(merged.parent.as_deref(), Some("vMX_Base"));
        assert_eq!(merged.blend.as_deref(), Some("BLEND_Additive"));
        assert_eq!(merged.scalars["Gamma"], 4.0);
        assert_eq!(merged.textures["ColorTexture"], "vT_Ramp");
    }

    #[test]
    fn reads_instance_values() {
        let mut fx = Fixture::default();
        let c_mi = fx.import("/Script/CoreUObject", "Class", "MaterialInstanceConstant");
        let parent = fx.import("/Script/CoreUObject", "Material", "vMX_Base");
        let mut b = Vec::new();
        fx.tag(&mut b, "Parent", "ObjectProperty", &parent.to_le_bytes());
        // ScalarParameterValues: [{ParameterInfo: {Name}, ParameterValue}]
        let mut arr = 1i32.to_le_bytes().to_vec();
        fx.struct_tag_header(&mut arr, "ScalarParameterValues", "ScalarParameterValue", 0);
        let mut info = Vec::new();
        let n = fx.fname("Gamma");
        fx.tag(&mut info, "Name", "NameProperty", &n);
        fx.none(&mut info);
        fx.struct_tag(&mut arr, "ParameterInfo", "MaterialParameterInfo", &info);
        fx.tag(
            &mut arr,
            "ParameterValue",
            "FloatProperty",
            &1.5f32.to_le_bytes(),
        );
        fx.none(&mut arr);
        fx.array_tag(&mut b, "ScalarParameterValues", "StructProperty", &arr);
        fx.none(&mut b);
        fx.export(c_mi, "MI", b.len());
        let m = read_material(&fx.uasset(), &b).unwrap();
        assert_eq!(m.parent.as_deref(), Some("vMX_Base"));
        assert_eq!(m.scalars["Gamma"], 1.5);
        assert_eq!(m.blend, None);
    }
}
