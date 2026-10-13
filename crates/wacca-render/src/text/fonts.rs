//! Font files with fallback: each character comes from the first font that has it.

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontVec, GlyphId, OutlinedGlyph, PxScale, ScaleFont};

#[derive(Debug, thiserror::Error)]
pub enum TextError {
    #[error("no usable font among {0:?}")]
    NoFont(Vec<PathBuf>),
}

/// Which typeface a line asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Face {
    /// The fallback chain of [`Fonts::load`].
    #[default]
    Ui,
    /// The numeral face of [`Fonts::with_numerals`] for the characters it has, else [`Face::Ui`].
    Numerals,
    /// The `Kakugo_kana` UMG font (player name) of [`Fonts::with_face`], else [`Face::Ui`].
    Kakugo,
}

impl Face {
    /// Slot in [`Fonts`]'s special faces (`None` for the fallback chain).
    fn slot(self) -> Option<usize> {
        match self {
            Face::Ui => None,
            Face::Numerals => Some(0),
            Face::Kakugo => Some(1),
        }
    }
}

/// Loaded fonts in priority order (never empty), plus an optional numeral face.
pub struct Fonts {
    faces: Vec<(PathBuf, FontVec)>,
    /// Indices into `faces` of the special faces ([`Face::slot`]), kept after the fallback chain.
    special: [Option<usize>; 2],
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.faces.iter().map(|(p, _)| p))
            .finish()
    }
}

fn load_face(path: &Path, index: u32) -> Option<FontVec> {
    let bytes = std::fs::read(path)
        .map_err(|e| log::debug!("font {}: {e}", path.display()))
        .ok()?;
    // TrueType / OpenType (CFF) files and face `index` of a .ttc collection.
    FontVec::try_from_vec_and_index(bytes, index)
        .map_err(|e| log::warn!("font {}: {e}", path.display()))
        .ok()
}

impl Fonts {
    /// Load every readable font of `paths` (missing or broken ones are skipped).
    pub fn load(paths: &[PathBuf]) -> Result<Self, TextError> {
        let faces: Vec<_> = paths
            .iter()
            .filter_map(|p| load_face(p, 0).map(|f| (p.clone(), f)))
            .collect();
        if faces.is_empty() {
            return Err(TextError::NoFont(paths.to_vec()));
        }
        log::info!(
            "fonts: {:?}",
            faces.iter().map(|(p, _)| p.display()).collect::<Vec<_>>()
        );
        Ok(Self {
            faces,
            special: [None; 2],
        })
    }

    /// Use face `index` of `path` for [`Face::Numerals`] (skipped with a warning if unreadable).
    pub fn with_numerals(self, path: &Path, index: u32) -> Self {
        self.with_face(Face::Numerals, path, index)
    }

    /// Use face `index` of `path` for `face` (not [`Face::Ui`]; skipped with a warning if
    /// unreadable).
    pub fn with_face(mut self, face: Face, path: &Path, index: u32) -> Self {
        let Some(slot) = face.slot() else {
            return self;
        };
        match load_face(path, index) {
            Some(f) => {
                log::info!("{face:?}: {} #{index}", path.display());
                self.faces.push((path.to_path_buf(), f));
                self.special[slot] = Some(self.faces.len() - 1);
            }
            None => log::warn!("{face:?} font {} #{index} not loaded", path.display()),
        }
        self
    }

    /// End of the fallback chain in `faces`.
    fn chain_len(&self) -> usize {
        self.special
            .iter()
            .flatten()
            .copied()
            .min()
            .unwrap_or(self.faces.len())
    }

    fn special(&self, face: Face) -> Option<usize> {
        face.slot().and_then(|s| self.special[s])
    }

    /// Face index and glyph of `c`; the first face's missing-glyph box if no face has it.
    fn lookup(&self, c: char, face: Face) -> (usize, GlyphId) {
        if let Some(n) = self.special(face) {
            let g = self.faces[n].1.glyph_id(c);
            if g.0 != 0 {
                return (n, g);
            }
        }
        self.faces[..self.chain_len()]
            .iter()
            .enumerate()
            .map(|(i, (_, f))| (i, f.glyph_id(c)))
            .find(|&(_, g)| g.0 != 0)
            .unwrap_or((0, GlyphId(0)))
    }

    fn face(&self, i: usize) -> &FontVec {
        &self.faces[i.min(self.faces.len() - 1)].1
    }

    /// Horizontal advance of `c` at pixel height `px`.
    pub fn advance(&self, c: char, px: f32, face: Face) -> f32 {
        let (i, g) = self.lookup(c, face);
        self.face(i).as_scaled(PxScale::from(px)).h_advance(g)
    }

    /// Distance from the top of a line to its baseline at `px`.
    pub fn ascent(&self, px: f32, face: Face) -> f32 {
        let i = self.special(face).unwrap_or(0);
        self.face(i).as_scaled(PxScale::from(px)).ascent()
    }

    /// Glyph key for the cache: `(face, glyph id)`.
    pub(super) fn glyph_key(&self, c: char, face: Face) -> (usize, u16) {
        let (i, g) = self.lookup(c, face);
        (i, g.0)
    }

    /// Outline of glyph `(face, id)` at `px` with its origin at `(0, 0)`.
    pub(super) fn outline(&self, (face, id): (usize, u16), px: f32) -> Option<OutlinedGlyph> {
        let f = self.face(face);
        f.outline_glyph(GlyphId(id).with_scale(PxScale::from(px)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_font_is_an_error() {
        let r = Fonts::load(&[PathBuf::from("does/not/exist.ttf")]);
        assert!(matches!(r, Err(TextError::NoFont(p)) if p.len() == 1));
    }

    /// Needs a Japanese system font (Windows); run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn japanese_falls_back_and_measures() {
        let fonts = Fonts::load(&[
            PathBuf::from("does/not/exist.ttf"),
            PathBuf::from(r"C:\Windows\Fonts\meiryo.ttc"),
        ])
        .unwrap();
        assert!(fonts.advance('ヒ', 20.0, Face::Ui) > 10.0);
        assert!(fonts.advance('a', 20.0, Face::Ui) > 0.0);
        assert!(fonts.ascent(20.0, Face::Ui) > 10.0);
        assert!(
            fonts
                .outline(fonts.glyph_key('ヒ', Face::Ui), 20.0)
                .is_some()
        );
    }

    /// Needs Windows fonts; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn numeral_face_only_for_numerals() {
        let fonts = Fonts::load(&[PathBuf::from(r"C:\Windows\Fonts\meiryo.ttc")])
            .unwrap()
            .with_numerals(Path::new(r"C:\Windows\Fonts\arial.ttf"), 0)
            .with_numerals(Path::new("does/not/exist.ttf"), 0);
        assert_eq!(fonts.glyph_key('1', Face::Numerals).0, 1);
        assert_eq!(fonts.glyph_key('1', Face::Ui).0, 0);
        // not in Arial: falls back to the chain
        assert_eq!(fonts.glyph_key('ヒ', Face::Numerals).0, 0);
    }
}
