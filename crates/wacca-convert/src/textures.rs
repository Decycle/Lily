//! Texture2D packages → PNG (jackets, UI/Vfx dumps). Spec: docs/research/06-visuals.md §4.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use wacca_ue4::{PixelFormat, Texture, Ue4Error};

use crate::Error;

/// Encodes an RGBA8 texture as PNG.
pub fn encode_png(t: &Texture) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, t.width, t.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&t.rgba8)?;
    writer.finish()?;
    Ok(out)
}

/// Per-run tally: decoded textures per source format, skipped non-textures, failures.
#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub formats: BTreeMap<PixelFormat, usize>,
    pub skipped: usize,
    pub failed: usize,
}

impl Summary {
    fn add(&mut self, other: Summary) {
        for (f, n) in other.formats {
            *self.formats.entry(f).or_default() += n;
        }
        self.skipped += other.skipped;
        self.failed += other.failed;
    }

    /// Counts one export result; non-texture packages are skipped quietly, failures logged.
    fn record(&mut self, source: &Path, result: Result<PixelFormat, Error>) {
        match result {
            Ok(f) => *self.formats.entry(f).or_default() += 1,
            Err(Error::Texture {
                source: Ue4Error::NotATexture,
                ..
            }) => self.skipped += 1,
            Err(e) => {
                log::warn!("{}: {e}", source.display());
                self.failed += 1;
            }
        }
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total: usize = self.formats.values().sum();
        write!(f, "{total} written (")?;
        for (i, (format, n)) in self.formats.iter().enumerate() {
            let sep = if i == 0 { "" } else { ", " };
            write!(f, "{sep}{} {n}", format.name())?;
        }
        write!(
            f,
            "), {} non-textures skipped, {} failed",
            self.skipped, self.failed
        )
    }
}

fn read(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

/// Decodes `uasset` (+ `.uexp`, optional `.ubulk`) and writes it as `png`.
fn export_texture(uasset: &Path, png_path: &Path) -> Result<PixelFormat, Error> {
    let ua = read(uasset)?;
    let ux = read(&uasset.with_extension("uexp"))?;
    let ubulk = uasset.with_extension("ubulk");
    let ub = if ubulk.is_file() {
        Some(read(&ubulk)?)
    } else {
        None
    };
    let texture =
        wacca_ue4::read_texture2d(&ua, &ux, ub.as_deref()).map_err(|source| Error::Texture {
            path: uasset.to_owned(),
            source,
        })?;
    crate::write(png_path, &encode_png(&texture)?)?;
    Ok(texture.format)
}

/// Exports `(uasset, png)` pairs on all cores.
pub fn export_all(jobs: &[(PathBuf, PathBuf)]) -> Summary {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = jobs.len().div_ceil(threads).max(1);
    let mut summary = Summary::default();
    std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|jobs| {
                s.spawn(move || {
                    let mut sum = Summary::default();
                    for (src, dst) in jobs {
                        sum.record(src, export_texture(src, dst));
                    }
                    sum
                })
            })
            .collect();
        for h in handles {
            match h.join() {
                Ok(part) => summary.add(part),
                Err(_) => log::error!("texture export thread panicked"),
            }
        }
    });
    summary
}

/// `S03/uT_J_S03_30` → (`<assets>/UI/Textures/JACKET/S03/uT_J_S03_30.uasset`, `<out>/jackets/S03/uT_J_S03_30.png`).
pub fn jacket_job(assets: &Path, out: &Path, jacket: &str) -> (PathBuf, PathBuf) {
    let src = assets
        .join("UI/Textures/JACKET")
        .join(format!("{jacket}.uasset"));
    (src, out.join("jackets").join(format!("{jacket}.png")))
}

/// True for a non-empty relative path without `..`, root or prefix components.
pub fn is_plain_relative(path: &Path) -> bool {
    path.components().next().is_some()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Every `.uasset` under `<assets>/<subdir>` → `<out>/ui/<path relative to assets>.png`, sorted.
pub fn dir_jobs(
    assets: &Path,
    subdir: &Path,
    out: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, Error> {
    let mut files = Vec::new();
    walk(&assets.join(subdir), &mut files)?;
    files.sort();
    Ok(files
        .into_iter()
        .filter_map(|src| {
            let rel = src.strip_prefix(assets).ok()?.with_extension("png");
            Some((src.clone(), out.join("ui").join(rel)))
        })
        .collect())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Error> {
    let io = |source| Error::Io {
        path: dir.to_owned(),
        source,
    };
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "uasset") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trips() {
        let t = Texture {
            width: 2,
            height: 1,
            format: PixelFormat::B8G8R8A8,
            rgba8: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let png = encode_png(&t).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(buf, t.rgba8);
    }

    #[test]
    fn summary_counts_formats_skips_and_failures() {
        let mut s = Summary::default();
        let p = Path::new("x");
        s.record(p, Ok(PixelFormat::Dxt1));
        s.record(p, Ok(PixelFormat::Dxt1));
        let texture_err = |source| Error::Texture {
            path: p.into(),
            source,
        };
        s.record(p, Err(texture_err(Ue4Error::NotATexture)));
        s.record(p, Err(texture_err(Ue4Error::MissingBulkFile)));
        let mut total = Summary::default();
        total.add(s);
        total.record(p, Ok(PixelFormat::G8));
        assert_eq!(
            total.to_string(),
            "3 written (PF_G8 1, PF_DXT1 2), 1 non-textures skipped, 1 failed"
        );
    }

    #[test]
    fn job_paths() {
        let (src, dst) = jacket_job(Path::new("a"), Path::new("o"), "S03/uT_J_S03_30");
        assert_eq!(
            src,
            Path::new("a/UI/Textures/JACKET/S03/uT_J_S03_30.uasset")
        );
        assert_eq!(dst, Path::new("o/jackets/S03/uT_J_S03_30.png"));
        assert!(is_plain_relative(Path::new("UI/Textures/FONT")));
        assert!(!is_plain_relative(Path::new("../x")));
        assert!(!is_plain_relative(Path::new("")));
        assert!(!is_plain_relative(Path::new("/abs")));
    }

    #[test]
    fn dir_jobs_mirror_the_assets_tree() {
        let root = std::env::temp_dir().join(format!("wacca-convert-test-{}", std::process::id()));
        let sub = root.join("UI/Textures/FONT");
        std::fs::create_dir_all(&sub).unwrap();
        for f in ["uT_A.uasset", "uT_A.uexp", "notes.txt"] {
            std::fs::write(sub.join(f), b"").unwrap();
        }
        let jobs = dir_jobs(&root, Path::new("UI/Textures"), Path::new("o")).unwrap();
        // An empty file is not a valid package: a failure, not a skip.
        let mut s = Summary::default();
        s.record(&jobs[0].0, export_texture(&jobs[0].0, &jobs[0].1));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].1, Path::new("o/ui/UI/Textures/FONT/uT_A.png"));
        assert_eq!(s.failed, 1);
    }
}
