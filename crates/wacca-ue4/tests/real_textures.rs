//! Tests over the real game textures; run with `WACCA_ASSETS=<assets dir> cargo test -- --ignored --nocapture`.
#![allow(clippy::unwrap_used)] // tests may unwrap (CONTRIBUTING)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wacca_ue4::{PixelFormat, Texture, Ue4Error, read_datatable, read_texture2d};

fn assets() -> PathBuf {
    PathBuf::from(
        std::env::var_os("WACCA_ASSETS").expect("set WACCA_ASSETS to the assets directory"),
    )
}

fn load(uasset: &Path) -> Result<Texture, Ue4Error> {
    let ua = std::fs::read(uasset).unwrap();
    let ux = std::fs::read(uasset.with_extension("uexp")).unwrap();
    let ub = std::fs::read(uasset.with_extension("ubulk")).ok();
    read_texture2d(&ua, &ux, ub.as_deref())
}

fn uassets(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            uassets(&path, out);
        } else if path.extension().is_some_and(|e| e == "uasset") {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn all_jackets_decode() {
    let mut files = Vec::new();
    uassets(&assets().join("UI/Textures/JACKET"), &mut files);
    let mut sizes = BTreeMap::new();
    for f in &files {
        let t = load(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert_eq!(t.format, PixelFormat::B8G8R8A8, "{}", f.display());
        assert_eq!(t.rgba8.len(), (t.width * t.height * 4) as usize);
        *sizes.entry((t.width, t.height)).or_insert(0) += 1;
    }
    println!("jacket sizes: {sizes:?}");
    assert_eq!(files.len(), 497);
    // 03-audio-and-jackets §4: 417 × 256², 4 × 2048², 1 × 512², 75 stripped placeholders.
    assert_eq!(sizes[&(256, 256)], 417);
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn table_jackets_are_256() {
    let table = assets().join("Table/MusicParameterTable");
    let ua = std::fs::read(table.with_extension("uasset")).unwrap();
    let ux = std::fs::read(table.with_extension("uexp")).unwrap();
    let table = read_datatable(&ua, &ux).unwrap();
    assert!(table.rows.len() >= 395);
    for (_, row) in &table.rows {
        let jacket = row
            .field("JacketAssetName")
            .and_then(|v| v.as_str())
            .unwrap();
        let path = assets().join(format!("UI/Textures/JACKET/{jacket}.uasset"));
        let t = load(&path).unwrap();
        // The tutorial row uses the 512² S00_000 (03-audio-and-jackets §4).
        let size = if jacket == "S00/uT_J_S00_000" {
            512
        } else {
            256
        };
        assert_eq!((t.width, t.height), (size, size), "{jacket}");
    }
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn sample_textures_decode() {
    let cases = [
        ("UI/Textures/FONT/uT_FTdecision01", PixelFormat::B8G8R8A8),
        ("UI/Textures/uT_RGback00", PixelFormat::B8G8R8A8),
        ("UI/Material/Glitch/uT_GlitchMaskH", PixelFormat::Dxt1),
        (
            "UI/Textures/CHANGE/uT_CH_icn_difficulty00",
            PixelFormat::Dxt5,
        ),
        ("Vfx/Textures/vT_SNote00_00_m", PixelFormat::G8),
    ];
    for (name, format) in cases {
        let t = load(&assets().join(format!("{name}.uasset"))).unwrap();
        println!("{name}: {:?} {}x{}", t.format, t.width, t.height);
        assert_eq!(t.format, format, "{name}");
        assert!(t.width > 0 && t.height > 0);
        assert_eq!(t.rgba8.len(), (t.width * t.height * 4) as usize);
    }
}

/// Survey of every Texture2D under UI/ and Vfx/: counts per format and failures.
#[test]
#[ignore = "needs WACCA_ASSETS"]
fn ui_and_vfx_textures_decode() {
    let mut files = Vec::new();
    uassets(&assets().join("UI"), &mut files);
    uassets(&assets().join("Vfx"), &mut files);
    let (mut formats, mut failures) = (BTreeMap::new(), BTreeMap::new());
    let mut skipped = 0;
    for f in &files {
        match load(f) {
            Ok(t) => *formats.entry(t.format.name()).or_insert(0) += 1,
            Err(Ue4Error::NotATexture) => skipped += 1,
            Err(e) => {
                println!("FAIL {}: {e}", f.display());
                *failures.entry(e.to_string()).or_insert(0) += 1;
            }
        }
    }
    println!("decoded {formats:?}, non-textures {skipped}, failures {failures:?}");
    assert!(failures.is_empty());
    assert_eq!(formats.values().sum::<usize>(), 2268);
}
