//! Song jackets: `data/jackets/<JacketAssetName>.png` (from `wacca-convert --textures`), or a
//! coloured placeholder square derived from the asset name.

use std::path::Path;

use wacca_render::style::Rgba;

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

/// A decoded image for `key` (`None`: missing or unreadable).
pub type Loaded = (String, Option<([u32; 2], Vec<u8>)>);

/// Decodes images with [`load`] on a background thread, so first use of a jacket or UI texture
/// does not stall a frame (measured: ~9 ms per UI texture on the UI thread).
#[derive(Debug)]
pub struct Loader {
    requests: std::sync::mpsc::Sender<(String, std::path::PathBuf)>,
    done: std::sync::mpsc::Receiver<Loaded>,
}

impl Loader {
    pub fn spawn() -> Self {
        let (requests, rx) = std::sync::mpsc::channel::<(String, std::path::PathBuf)>();
        let (tx, done) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("wacca-images".into())
            .spawn(move || {
                for (key, path) in rx {
                    if tx.send((key, load(&path))).is_err() {
                        break;
                    }
                }
            });
        if let Err(e) = spawned {
            log::error!("image loader thread: {e}; images stay placeholders");
        }
        Self { requests, done }
    }

    /// Decode `path` for `key`; the result comes back from [`Loader::finished`].
    pub fn request(&self, key: &str, path: std::path::PathBuf) {
        // A dead thread was already logged; the image just never arrives.
        let _ = self.requests.send((key.to_string(), path));
    }

    /// Images decoded since the last call.
    pub fn finished(&self) -> impl Iterator<Item = Loaded> + '_ {
        self.done.try_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_is_stable() {
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

    #[test]
    fn loader_decodes_in_the_background() {
        let dir = std::env::temp_dir().join(format!("wacca-app-loader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([9, 8, 7, 6]))
            .save(&good)
            .unwrap();
        let loader = Loader::spawn();
        loader.request("good", good);
        loader.request("missing", dir.join("missing.png"));
        let mut got = Vec::new();
        for _ in 0..500 {
            got.extend(loader.finished());
            if got.len() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            got,
            vec![
                ("good".to_string(), Some(([1, 1], vec![9, 8, 7, 6]))),
                ("missing".to_string(), None)
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
