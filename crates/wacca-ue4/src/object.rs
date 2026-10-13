//! Every export of a package as tagged properties (e.g. the Cascade `ParticleSystem` object
//! graph: system → emitters → LOD levels → modules → distributions). Native data after an
//! export's properties is ignored.

use crate::Ue4Error;
use crate::package::Package;
use crate::property::{Value, read_properties};
use crate::reader::Reader;

/// One export: its class (import name), its owner and its tagged properties.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub class: String,
    /// Package index of the owner (0 = the package).
    pub outer: i32,
    pub properties: Vec<(String, Value)>,
}

impl Object {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.properties
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }
}

/// The exports of a package plus what object references resolve to.
#[derive(Clone, Debug, PartialEq)]
pub struct Objects {
    pub exports: Vec<Object>,
    package: Package,
}

impl Objects {
    /// The export a package index points at (positive indices only).
    pub fn export(&self, index: i32) -> Option<&Object> {
        let i = usize::try_from(index).ok()?.checked_sub(1)?;
        self.exports.get(i)
    }

    /// Name of the object a package index points at (import or export).
    pub fn name(&self, index: i32) -> Option<&str> {
        match index {
            i if i < 0 => self.package.import_name(i),
            i => self.export(i).map(|o| o.name.as_str()),
        }
    }

    /// Package path of an import, e.g. `/Game/Vfx/Mesh/vMsh_tap00_00_m` for a mesh it uses.
    pub fn import_package(&self, index: i32) -> Option<&str> {
        let mut i = index;
        for _ in 0..16 {
            let imp = self
                .package
                .imports
                .get(usize::try_from(-i64::from(i) - 1).ok()?)?;
            if imp.outer_index == 0 {
                return Some(&imp.object_name);
            }
            i = imp.outer_index;
        }
        None
    }
}

/// Reads the tagged properties of every export. An export whose properties fail to decode is
/// logged and kept with what was read before the failure.
pub fn read_objects(uasset: &[u8], uexp: &[u8]) -> Result<Objects, Ue4Error> {
    let package = Package::parse(uasset)?;
    let mut exports = Vec::new();
    for e in &package.exports {
        let class = package.import_name(e.class_index).unwrap_or("").to_owned();
        let start = usize::try_from(e.serial_offset)
            .ok()
            .and_then(|o| o.checked_sub(uasset.len()))
            .ok_or(Ue4Error::BadExportOffset(e.serial_offset))?;
        let end = usize::try_from(e.serial_size)
            .ok()
            .and_then(|s| start.checked_add(s));
        let properties = match Reader::at(uexp, start)
            .and_then(|mut r| read_properties(&package, &mut r, end))
        {
            Ok(p) => p,
            Err(err) => {
                log::warn!("{} ({class}): {err}", e.object_name);
                Vec::new()
            }
        };
        exports.push(Object {
            name: e.object_name.clone(),
            class,
            outer: e.outer_index,
            properties,
        });
    }
    Ok(Objects { exports, package })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    #[test]
    fn reads_every_export_with_class_outer_and_references() {
        let mut fx = Fixture::default();
        let sys = fx.import("/Script/CoreUObject", "Class", "ParticleSystem");
        let emitter = fx.import("/Script/CoreUObject", "Class", "ParticleSpriteEmitter");
        let mesh = fx.import("/Script/CoreUObject", "Package", "/Game/Vfx/Mesh/vMsh_a");
        let mut a = Vec::new();
        fx.array_tag(
            &mut a,
            "Emitters",
            "ObjectProperty",
            &[1, 0, 0, 0, 2, 0, 0, 0],
        );
        fx.none(&mut a);
        let mut b = Vec::new();
        fx.tag(&mut b, "Mesh", "ObjectProperty", &mesh.to_le_bytes());
        fx.tag(&mut b, "Life", "FloatProperty", &0.25f32.to_le_bytes());
        fx.none(&mut b);
        fx.export(sys, "vo_tap", a.len());
        fx.export_in(emitter, "Emitter", b.len(), 1);
        let objs = read_objects(&fx.uasset(), &[a, b].concat()).unwrap();
        assert_eq!(objs.exports.len(), 2);
        assert_eq!(objs.exports[0].class, "ParticleSystem");
        assert_eq!(objs.exports[1].outer, 1);
        assert_eq!(objs.export(1).map(|o| o.name.as_str()), Some("vo_tap"));
        assert_eq!(objs.export(0), None);
        assert_eq!(objs.name(2), Some("Emitter"));
        assert_eq!(objs.name(sys), Some("ParticleSystem"));
        let em = &objs.exports[1];
        assert_eq!(em.get("Life").and_then(Value::as_f32), Some(0.25));
        assert_eq!(em.get("Mesh"), Some(&Value::Object(mesh)));
        assert_eq!(objs.import_package(mesh), Some("/Game/Vfx/Mesh/vMsh_a"));
    }

    #[test]
    fn bad_properties_keep_the_export() {
        let mut fx = Fixture::default();
        let class = fx.import("/Script/CoreUObject", "Class", "Thing");
        fx.export(class, "T", 3);
        let ua = fx.uasset();
        let objs = read_objects(&ua, &[0xff, 0xff, 0xff]).unwrap();
        assert_eq!(objs.exports[0].properties, []);
    }
}
