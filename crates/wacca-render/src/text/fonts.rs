//! Font files with fallback: each character comes from the first font that has it.

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontVec, GlyphId, OutlinedGlyph, PxScale, ScaleFont};

#[derive(Debug, thiserror::Error)]
pub enum TextError {
    #[error("no usable font among {0:?}")]
    NoFont(Vec<PathBuf>),
}

/// Loaded fonts in priority order (never empty).
pub struct Fonts {
    faces: Vec<(PathBuf, FontVec)>,
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.faces.iter().map(|(p, _)| p))
            .finish()
    }
}

fn load_face(path: &Path) -> Option<FontVec> {
    let bytes = std::fs::read(path)
        .map_err(|e| log::debug!("font {}: {e}", path.display()))
        .ok()?;
    // TrueType / OpenType (CFF) files and the first face of a .ttc collection.
    FontVec::try_from_vec_and_index(bytes, 0)
        .map_err(|e| log::warn!("font {}: {e}", path.display()))
        .ok()
}

impl Fonts {
    /// Load every readable font of `paths` (missing or broken ones are skipped).
    pub fn load(paths: &[PathBuf]) -> Result<Self, TextError> {
        let faces: Vec<_> = paths
            .iter()
            .filter_map(|p| load_face(p).map(|f| (p.clone(), f)))
            .collect();
        if faces.is_empty() {
            return Err(TextError::NoFont(paths.to_vec()));
        }
        log::info!(
            "fonts: {:?}",
            faces.iter().map(|(p, _)| p.display()).collect::<Vec<_>>()
        );
        Ok(Self { faces })
    }

    /// Face index and glyph of `c`; the first face's missing-glyph box if no face has it.
    fn lookup(&self, c: char) -> (usize, GlyphId) {
        self.faces
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
    pub fn advance(&self, c: char, px: f32) -> f32 {
        let (i, g) = self.lookup(c);
        self.face(i).as_scaled(PxScale::from(px)).h_advance(g)
    }

    /// Distance from the top of a line to its baseline at `px`.
    pub fn ascent(&self, px: f32) -> f32 {
        self.face(0).as_scaled(PxScale::from(px)).ascent()
    }

    /// Glyph key for the cache: `(face, glyph id)`.
    pub(super) fn glyph_key(&self, c: char) -> (usize, u16) {
        let (i, g) = self.lookup(c);
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
        assert!(fonts.advance('ヒ', 20.0) > 10.0);
        assert!(fonts.advance('a', 20.0) > 0.0);
        assert!(fonts.ascent(20.0) > 10.0);
        assert!(fonts.outline(fonts.glyph_key('ヒ'), 20.0).is_some());
    }
}
