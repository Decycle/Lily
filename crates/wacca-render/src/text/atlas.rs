//! Shelf packing of glyph bitmaps into one atlas texture (pure).

/// Gap left around every allocation so bilinear sampling does not bleed between glyphs.
const PAD: u32 = 1;

#[derive(Clone, Copy, Debug)]
struct Shelf {
    y: u32,
    h: u32,
    x: u32,
}

/// Rows ("shelves") filled left to right; a new shelf opens below the last one.
#[derive(Clone, Debug)]
pub struct Shelves {
    size: [u32; 2],
    shelves: Vec<Shelf>,
    next_y: u32,
}

impl Shelves {
    pub fn new(size: [u32; 2]) -> Self {
        Self {
            size,
            shelves: Vec::new(),
            next_y: 0,
        }
    }

    /// Top-left of a free `w × h` region, or `None` when the atlas is full.
    pub fn alloc(&mut self, w: u32, h: u32) -> Option<[u32; 2]> {
        let (pw, ph) = (w + PAD, h + PAD);
        if pw > self.size[0] {
            return None;
        }
        // Reuse a shelf that is tall enough but not wastefully so.
        let width = self.size[0];
        if let Some(s) = self
            .shelves
            .iter_mut()
            .find(|s| s.h >= ph && s.h <= ph + ph / 4 + 2 && s.x + pw <= width)
        {
            let at = [s.x, s.y];
            s.x += pw;
            return Some(at);
        }
        if self.next_y + ph > self.size[1] {
            return None;
        }
        let at = [0, self.next_y];
        self.shelves.push(Shelf {
            y: self.next_y,
            h: ph,
            x: pw,
        });
        self.next_y += ph;
        Some(at)
    }

    pub fn clear(&mut self) {
        self.shelves.clear();
        self.next_y = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_rows_and_reports_full() {
        let mut s = Shelves::new([20, 20]);
        assert_eq!(s.alloc(9, 9), Some([0, 0]));
        assert_eq!(s.alloc(9, 8), Some([10, 0])); // same shelf (height 10 fits 9)
        assert_eq!(s.alloc(9, 9), Some([0, 10])); // row full → new shelf
        assert_eq!(s.alloc(30, 1), None); // wider than the atlas
        let mut full = Shelves::new([10, 10]);
        assert!(full.alloc(9, 9).is_some());
        assert_eq!(full.alloc(9, 9), None);
        full.clear();
        assert_eq!(full.alloc(9, 9), Some([0, 0]));
    }

    #[test]
    fn short_glyphs_do_not_reuse_tall_shelves() {
        let mut s = Shelves::new([100, 100]);
        assert_eq!(s.alloc(10, 40), Some([0, 0]));
        assert_eq!(s.alloc(10, 10), Some([0, 41]));
        assert_eq!(s.alloc(10, 10), Some([11, 41]));
    }
}
