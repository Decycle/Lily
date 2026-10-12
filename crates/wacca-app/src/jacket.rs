//! Song jackets: `data/jackets/<JacketAssetName>.png` (from `wacca-convert --textures`), or a
//! coloured placeholder square derived from the asset name.

use std::path::{Path, PathBuf};

use wacca_render::style::Rgba;

/// `<data>/jackets/<key>.png`, e.g. key `S03/uT_J_S03_30`.
pub fn path(data: &Path, key: &str) -> PathBuf {
    data.join("jackets").join(format!("{key}.png"))
}

/// Stable, distinct-ish colour per jacket (FNV-1a hash → hue).
pub fn placeholder_color(key: &str) -> Rgba {
    let h = key.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    let hue = (h % 360) as f32 / 60.0;
    let x = 1.0 - (hue % 2.0 - 1.0).abs();
    let (r, g, b) = match hue as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let dim = |c: f32| 0.25 + 0.5 * c;
    [dim(r), dim(g), dim(b), 1.0]
}

/// Decoded RGBA8 jacket, or `None` if the file is missing or unreadable.
pub fn load(path: &Path) -> Option<([u32; 2], Vec<u8>)> {
    if !path.exists() {
        return None;
    }
    let bytes = std::fs::read(path)
        .map_err(|e| log::warn!("{}: {e}", path.display()))
        .ok()?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|e| log::warn!("{}: {e}", path.display()))
        .ok()?
        .to_rgba8();
    Some(([img.width(), img.height()], img.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_and_placeholder_are_stable() {
        assert_eq!(
            path(Path::new("data"), "S03/uT_J_S03_30"),
            Path::new("data")
                .join("jackets")
                .join("S03/uT_J_S03_30.png")
        );
        let a = placeholder_color("S03/uT_J_S03_30");
        assert_eq!(a, placeholder_color("S03/uT_J_S03_30"));
        assert_ne!(a, placeholder_color("S03/uT_J_S03_31"));
        assert!(a.iter().all(|c| (0.0..=1.0).contains(c)));
    }

    #[test]
    fn loads_png_and_skips_missing_or_bad_files() {
        let dir = std::env::temp_dir().join(format!("wacca-app-jacket-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir.join("missing.png")).is_none());
        let bad = dir.join("bad.png");
        std::fs::write(&bad, b"not a png").unwrap();
        assert!(load(&bad).is_none());
        let good = dir.join("good.png");
        image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 4]))
            .save(&good)
            .unwrap();
        let (size, rgba) = load(&good).unwrap();
        assert_eq!((size, &rgba[..4]), ([2, 3], &[1u8, 2, 3, 4][..]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
