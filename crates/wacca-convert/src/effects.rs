//! `--effects`: the Cascade hit-effect systems (`Vfx/Particle/**`) and what they need, under
//! `<out>/effects/` (read at runtime by `wacca-render`; doc 12):
//! `systems/<path below Vfx/Particle>.json` ([`wacca_ue4::ParticleSystem`]),
//! `materials/<name>.json` (instance values over the parent's defaults, plus `master`),
//! `textures.json` (sRGB flag of every texture a material uses), `touch_effects.json`
//! (`TouchEffectTable` rows) and the particle meshes as `<out>/meshes/<name>.obj`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::json;
use wacca_ue4::{MaterialParams, ParticleSystem, Value};

use crate::meshes::to_obj;
use crate::{Error, io_err, read, read_table, write};

const PARTICLE_DIR: &str = "Vfx/Particle";
const MATERIAL_DIR: &str = "Vfx/Material";
const MESH_DIR: &str = "Vfx/Mesh";
const TEXTURE_DIR: &str = "Vfx/Textures";

/// `TouchEffectTable` columns kept (effect path below `Vfx/Particle`, or `OFF`).
const TOUCH_COLUMNS: [&str; 11] = [
    "MarvelousPopEffectName",
    "GreatPopEffectName",
    "GoodEffectName",
    "MissEffectName",
    "BonusMarvelousPopEffectName",
    "BonusGreatPopEffectName",
    "MarvelousShootEffectName",
    "GreatShootEffectName",
    "HoldEffectName",
    "ReHoldEffectName",
    "HoldInputEffectName",
];

/// Every `.uasset` below `dir` (sorted), as paths without extension.
fn packages(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(io_err(&d))? {
            let p = e.map_err(io_err(&d))?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "uasset") {
                out.push(p.with_extension(""));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn read_pair(base: &Path) -> Result<(Vec<u8>, Vec<u8>), Error> {
    Ok((
        read(&base.with_extension("uasset"))?,
        read(&base.with_extension("uexp"))?,
    ))
}

/// `TouchEffectTable` rows as `{id: {column: path}}`.
pub fn touch_effects(table: &wacca_ue4::DataTable) -> serde_json::Value {
    let rows: BTreeMap<&str, BTreeMap<&str, &str>> = table
        .rows
        .iter()
        .map(|(id, row)| {
            let cols = TOUCH_COLUMNS
                .iter()
                .filter_map(|c| Some((*c, row.field(c).and_then(Value::as_str)?)))
                .collect();
            (id.as_str(), cols)
        })
        .collect();
    json!(rows)
}

/// Materials, meshes used by `systems`.
fn references(systems: &[ParticleSystem]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut materials = BTreeSet::new();
    let mut meshes = BTreeSet::new();
    for e in systems.iter().flat_map(|s| &s.emitters) {
        materials.insert(e.material.clone());
        meshes.extend(e.mesh.clone());
    }
    (materials, meshes)
}

/// Material `name` merged over its parents (by name lookup in `index`).
fn merged_material(
    index: &BTreeMap<String, PathBuf>,
    name: &str,
) -> Result<Option<(MaterialParams, String)>, Error> {
    let mut chain = Vec::new();
    let mut next = Some(name.to_owned());
    while let Some(n) = next.take() {
        let Some(base) = index.get(&n) else {
            log::warn!("material {n} not found under {MATERIAL_DIR}");
            break;
        };
        let (ua, ux) = read_pair(base)?;
        match wacca_ue4::read_material(&ua, &ux) {
            Ok(m) => {
                next = m.parent.clone().filter(|_| chain.len() < 8);
                chain.push(m);
            }
            Err(e) => {
                log::warn!("{}: {e}", base.display());
                break;
            }
        }
    }
    let Some(master) = chain.pop() else {
        return Ok(None);
    };
    let master_name = master.name.clone();
    let merged = chain.iter().rev().fold(master, |acc, m| m.over(&acc));
    Ok(Some((merged, master_name)))
}

/// Writes everything (see the module doc); returns `(systems, materials, meshes)` written.
pub fn export(assets: &Path, out: &Path) -> Result<(usize, usize, usize), Error> {
    let dir = out.join("effects");
    let table = read_table(assets, "TouchEffectTable")?;
    write(
        &dir.join("touch_effects.json"),
        &serde_json::to_vec_pretty(&touch_effects(&table))?,
    )?;
    let root = assets.join(PARTICLE_DIR);
    let mut systems = Vec::new();
    for base in packages(&root)? {
        let (ua, ux) = read_pair(&base)?;
        let sys = match wacca_ue4::read_particle_system(&ua, &ux) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("{}: {e}", base.display());
                continue;
            }
        };
        let rel = base.strip_prefix(&root).unwrap_or(&base);
        let path = dir.join("systems").join(rel).with_extension("json");
        write(&path, &serde_json::to_vec_pretty(&sys)?)?;
        systems.push(sys);
    }
    let (materials, meshes) = references(&systems);
    let index: BTreeMap<String, PathBuf> = packages(&assets.join(MATERIAL_DIR))?
        .into_iter()
        .filter_map(|p| Some((p.file_name()?.to_str()?.to_owned(), p)))
        .collect();
    let mut textures = BTreeMap::new();
    let mut written_materials = 0;
    for name in &materials {
        let Some((m, master)) = merged_material(&index, name)? else {
            continue;
        };
        for t in m.textures.values() {
            textures.entry(t.clone()).or_insert_with(|| srgb(assets, t));
        }
        let mut v = serde_json::to_value(&m)?;
        v["master"] = json!(master);
        write(
            &dir.join("materials").join(format!("{name}.json")),
            &serde_json::to_vec_pretty(&v)?,
        )?;
        written_materials += 1;
    }
    let textures: BTreeMap<_, _> = textures
        .into_iter()
        .map(|(k, v)| (k, json!({ "srgb": v })))
        .collect();
    write(
        &dir.join("textures.json"),
        &serde_json::to_vec_pretty(&textures)?,
    )?;
    let mut written_meshes = 0;
    for name in &meshes {
        let path = assets.join(MESH_DIR).join(name).with_extension("uexp");
        match wacca_ue4::read_static_mesh(&read(&path)?) {
            Ok(mesh) => {
                let obj = out.join("meshes").join(format!("{name}.obj"));
                write(&obj, to_obj(name, &mesh).as_bytes())?;
                written_meshes += 1;
            }
            Err(e) => log::warn!("{}: {e}", path.display()),
        }
    }
    Ok((systems.len(), written_materials, written_meshes))
}

/// The cooked `SRGB` flag of a Vfx texture (default true).
fn srgb(assets: &Path, name: &str) -> bool {
    let base = assets.join(TEXTURE_DIR).join(name);
    let Ok((ua, ux)) = read_pair(&base) else {
        log::warn!("texture {name} not found under {TEXTURE_DIR}");
        return true;
    };
    wacca_ue4::read_objects(&ua, &ux)
        .ok()
        .and_then(|o| {
            let t = o.exports.iter().rfind(|e| e.class == "Texture2D")?;
            Some(t.get("SRGB") != Some(&Value::Bool(false)))
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_ue4::{DataTable, Emitter};

    #[test]
    fn touch_rows_keep_effect_columns() {
        let row = Value::Struct(vec![
            ("TouchEffectId".into(), Value::Int(312001)),
            (
                "MarvelousPopEffectName".into(),
                Value::Str("tap_0/vo_tap000_03".into()),
            ),
            ("MissEffectName".into(), Value::Str("OFF".into())),
        ]);
        let t = DataTable {
            row_struct: "TouchEffectTableData".into(),
            rows: vec![("312001".into(), row)],
        };
        let v = touch_effects(&t);
        assert_eq!(
            v["312001"]["MarvelousPopEffectName"],
            json!("tap_0/vo_tap000_03")
        );
        assert_eq!(v["312001"]["MissEffectName"], json!("OFF"));
        assert!(v["312001"].get("TouchEffectId").is_none());
    }

    #[test]
    fn references_collect_materials_and_meshes() {
        let e = |m: &str, mesh: Option<&str>| Emitter {
            name: String::new(),
            material: m.into(),
            mesh: mesh.map(Into::into),
            mesh_orientation: None,
            gpu: None,
            alignment: String::new(),
            local_space: true,
            origin: [0.0; 3],
            duration: 1.0,
            delay: 0.0,
            loops: 1,
            sub_images: [1, 1],
            sub_uv_method: String::new(),
            rate: Default::default(),
            rate_scale: Default::default(),
            bursts: vec![],
            burst_scale: Default::default(),
            modules: vec![],
        };
        let sys = ParticleSystem {
            name: "s".into(),
            warmup: 0.0,
            emitters: vec![e("A", Some("M")), e("B", None), e("A", None)],
        };
        let (mats, meshes) = references(&[sys]);
        assert_eq!(mats.into_iter().collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(meshes.into_iter().collect::<Vec<_>>(), ["M"]);
    }
}
