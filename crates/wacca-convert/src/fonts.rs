//! `UI/Font/*.ufont` → `fonts/<name>.<ttf|otf|ttc>`. Spec: docs/research/06-visuals.md §5
//! (`.ufont` files are raw font files without a wrapper).

use std::collections::BTreeMap;
use std::path::Path;

use crate::{Error, io_err, read};

/// File extension for a font file, from its magic: TrueType, CFF OpenType or a collection.
pub fn sniff(data: &[u8]) -> Option<&'static str> {
    match data.get(..4)? {
        [0, 1, 0, 0] | b"true" => Some("ttf"),
        b"OTTO" => Some("otf"),
        b"ttcf" => Some("ttc"),
        _ => None,
    }
}

/// Copies every `.ufont` under `<assets>/UI/Font` to `<out>/fonts`; returns (count per extension, failures).
pub fn copy_fonts(
    assets: &Path,
    out: &Path,
) -> Result<(BTreeMap<&'static str, usize>, usize), Error> {
    let dir = assets.join("UI/Font");
    let (mut counts, mut failed) = (BTreeMap::new(), 0);
    for entry in std::fs::read_dir(&dir).map_err(io_err(&dir))? {
        let path = entry.map_err(io_err(&dir))?.path();
        if path.extension().is_none_or(|e| e != "ufont") {
            continue;
        }
        let data = read(&path)?;
        let (Some(ext), Some(stem)) = (sniff(&data), path.file_stem()) else {
            log::warn!("{}: unknown font magic, skipped", path.display());
            failed += 1;
            continue;
        };
        let name = format!("{}.{ext}", stem.to_string_lossy());
        crate::write(&out.join("fonts").join(name), &data)?;
        *counts.entry(ext).or_default() += 1;
    }
    Ok((counts, failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_font_magic() {
        assert_eq!(sniff(b"\0\x01\0\0rest"), Some("ttf"));
        assert_eq!(sniff(b"true"), Some("ttf"));
        assert_eq!(sniff(b"OTTO\0\x0a"), Some("otf"));
        assert_eq!(sniff(b"ttcf\0\x01"), Some("ttc"));
        assert_eq!(sniff(b"wOFF"), None);
        assert_eq!(sniff(b"OT"), None);
    }

    #[test]
    #[ignore = "needs WACCA_ASSETS"]
    fn real_fonts_all_sniff() {
        let assets = std::env::var_os("WACCA_ASSETS").expect("set WACCA_ASSETS");
        let dir = Path::new(&assets).join("UI/Font");
        let mut n = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "ufont") {
                let data = std::fs::read(&path).unwrap();
                assert!(sniff(&data).is_some(), "{}", path.display());
                n += 1;
            }
        }
        assert_eq!(n, 15);
    }
}
