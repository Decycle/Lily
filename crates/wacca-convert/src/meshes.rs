//! `--meshes`: the note meshes (`Vfx/Mesh/Notes/*.uasset`, doc 11 §1.4) as OBJ text under
//! `<out>/meshes/`, read at runtime by `wacca-render`.

use std::fmt::Write as _;
use std::path::Path;

use wacca_ue4::StaticMesh;

use crate::{Error, io_err, read, write};

/// Mesh directory below the assets dir.
pub const NOTES_DIR: &str = "Vfx/Mesh/Notes";

/// OBJ text of `mesh`: raw UE coordinates (cm), `vt` = UV0 **as stored** (v not flipped), one
/// `g section<material>` group per section.
pub fn to_obj(name: &str, mesh: &StaticMesh) -> String {
    let mut s =
        format!("# {name}: UE4 StaticMesh LOD0, raw UE coordinates (cm); vt = UV0 as stored\n");
    for [x, y, z] in &mesh.positions {
        let _ = writeln!(s, "v {x} {y} {z}");
    }
    for [u, v] in &mesh.uv0 {
        let _ = writeln!(s, "vt {u} {v}");
    }
    for sec in &mesh.sections {
        let _ = writeln!(s, "g section{}", sec.material);
        let first = sec.first_index as usize;
        let tris = mesh
            .indices
            .get(first..first + 3 * sec.triangles as usize)
            .unwrap_or_default();
        for t in tris.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| i + 1);
            let _ = writeln!(s, "f {a}/{a} {b}/{b} {c}/{c}");
        }
    }
    s
}

/// Exports every static mesh in [`NOTES_DIR`]; returns `(written, failed)`.
pub fn export_notes(assets: &Path, out: &Path) -> Result<(usize, usize), Error> {
    let dir = assets.join(NOTES_DIR);
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .map_err(io_err(&dir))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "uexp"))
        .collect();
    names.sort();
    let (mut written, mut failed) = (0, 0);
    for path in names {
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        match wacca_ue4::read_static_mesh(&read(&path)?) {
            Ok(mesh) => {
                let obj = out.join("meshes").join(format!("{name}.obj"));
                write(&obj, to_obj(name, &mesh).as_bytes())?;
                written += 1;
            }
            Err(e) => {
                log::warn!("{}: {e}", path.display());
                failed += 1;
            }
        }
    }
    Ok((written, failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_ue4::MeshSection;

    #[test]
    fn obj_keeps_raw_uv_and_groups_sections() {
        let mesh = StaticMesh {
            positions: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.5]],
            uv0: vec![[0.0, 0.25], [1.0, 0.0], [0.5, 1.0]],
            indices: vec![0, 1, 2],
            sections: vec![MeshSection {
                material: 3,
                first_index: 0,
                triangles: 1,
            }],
        };
        let obj = to_obj("m", &mesh);
        assert!(obj.contains("v 7 8 9.5\n"));
        assert!(obj.contains("vt 0 0.25\n"));
        assert!(obj.contains("g section3\nf 1/1 2/2 3/3\n"));
    }
}
