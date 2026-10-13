//! Tests over the real game particle systems; run with `WACCA_ASSETS=<assets dir> cargo test -- --ignored`.
#![allow(clippy::unwrap_used)] // tests may unwrap (CONTRIBUTING)

use std::path::PathBuf;

use wacca_ue4::{Module, read_particle_system};

fn assets() -> PathBuf {
    PathBuf::from(
        std::env::var_os("WACCA_ASSETS").expect("set WACCA_ASSETS to the assets directory"),
    )
}

/// The R-note hit's square grid reads its wave radii and colour turn from a
/// `ParticleModuleParameterDynamic`; RenderDoc KING frame 9800 (eid 2782, relative time ≈ 0.5)
/// shows DynamicParameter (0.459, 0.8, 0.376, −0.513).
#[test]
#[ignore]
fn r_note_hit_dynamic_parameters() {
    let base = assets().join("Vfx").join("Particle").join("vb_SnoteHit_00");
    let ua = std::fs::read(base.with_extension("uasset")).unwrap();
    let ux = std::fs::read(base.with_extension("uexp")).unwrap();
    let ps = read_particle_system(&ua, &ux).unwrap();
    let e = ps
        .emitters
        .iter()
        .find(|e| e.material == "vMX_3_RankUp_000_TUS_Snote000")
        .unwrap();
    let params = e
        .modules
        .iter()
        .find_map(|m| match m {
            Module::DynamicParam(p) => Some(p),
            _ => None,
        })
        .unwrap();
    assert_eq!(params.len(), 4);
    let at = |k: usize, t: f32| params[k].value.eval1(t, 0.0);
    assert!((at(0, 0.5) - 0.464).abs() < 0.01, "{}", at(0, 0.5));
    assert!((at(1, 0.5) - 0.8).abs() < 1e-6);
    assert!((at(3, 0.5) + 0.5).abs() < 1e-3);
    assert!(params.iter().all(|p| !p.spawn_only && !p.emitter_time));
}
