//! The always-on debug panel under the playfield (doc 09 §0) as a per-note judgement detector: each judgement
//! increments one counter (Marvelous / Great / Good / Miss) and, off-Marvelous, FAST or LATE. The panel is static
//! text in a lossless capture, so a counter changed exactly when its pixels changed; no OCR is needed.

use crate::frame::Rgb;
use serde::Serialize;
use wacca_game::rules::Judgement;

pub const ROWS: usize = 6;
pub const NAMES: [&str; ROWS] = ["marvelous", "great", "good", "miss", "fast", "late"];
/// Capture size the regions are measured in (the game window on this PC).
pub const SIZE: (usize, usize) = (675, 1200);
/// Part of the frame to decode, `[x, y, width, height]`.
pub const CROP: [usize; 4] = [316, 984, 176, 148];
/// Counter digits `[x, y, width, height]` in the full frame: the four judgement counts (without the R-note count
/// in brackets), then FAST and LATE.
pub const REGIONS: [[usize; 4]; ROWS] = [
    [452, 986, 34, 16],
    [452, 1011, 34, 16],
    [452, 1036, 34, 16],
    [452, 1061, 34, 16],
    [318, 1108, 48, 21],
    [391, 1108, 52, 21],
];
/// Per-channel difference that counts as a changed pixel.
const TOL: u8 = 40;

/// Rows whose digits differ between two [`CROP`]s.
pub fn changed(prev: &Rgb, cur: &Rgb) -> [bool; ROWS] {
    let mut out = [false; ROWS];
    for (row, [x, y, w, h]) in REGIONS.iter().enumerate() {
        let (x0, y0) = (x - CROP[0], y - CROP[1]);
        out[row] = (y0..y0 + h).any(|py| {
            (x0..x0 + w).any(|px| {
                let (a, b) = (prev.pixel(px, py), cur.pixel(px, py));
                a.iter().zip(b).any(|(&p, q)| p.abs_diff(q) > TOL)
            })
        });
    }
    out
}

/// A panel change at chart time `t`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Event {
    pub t: f64,
    pub rows: [bool; ROWS],
}

impl Event {
    /// The judgement counters that changed.
    pub fn judgements(&self) -> Vec<Judgement> {
        [
            Judgement::Marvelous,
            Judgement::Great,
            Judgement::Good,
            Judgement::Miss,
        ]
        .into_iter()
        .zip(self.rows)
        .filter_map(|(j, on)| on.then_some(j))
        .collect()
    }

    pub fn fast(&self) -> bool {
        self.rows[4]
    }

    pub fn late(&self) -> bool {
        self.rows[5]
    }
}

/// Collects changes from `(chart time, crop)` frames in order.
#[derive(Debug, Default)]
pub struct Tracker {
    prev: Option<Rgb>,
    pub events: Vec<Event>,
}

impl Tracker {
    pub fn push(&mut self, t: f64, crop: Rgb) {
        if let Some(prev) = &self.prev {
            let rows = changed(prev, &crop);
            if rows.iter().any(|&r| r) {
                self.events.push(Event { t, rows });
            }
        }
        self.prev = Some(crop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crop() -> Rgb {
        Rgb::new(CROP[2], CROP[3], vec![0; CROP[2] * CROP[3] * 3]).unwrap()
    }

    fn lit(mut f: Rgb, x: usize, y: usize) -> Rgb {
        let i = ((y - CROP[1]) * f.width + (x - CROP[0])) * 3;
        f.data[i] = 200;
        f
    }

    #[test]
    fn regions_fit_the_crop() {
        for [x, y, w, h] in REGIONS {
            assert!(x >= CROP[0] && x + w <= CROP[0] + CROP[2]);
            assert!(y >= CROP[1] && y + h <= CROP[1] + CROP[3]);
        }
        assert!(CROP[0] + CROP[2] <= SIZE.0 && CROP[1] + CROP[3] <= SIZE.1);
    }

    #[test]
    fn tracks_counter_changes() {
        let mut tr = Tracker::default();
        tr.push(0.0, crop());
        tr.push(0.1, crop());
        // Great digit and the FAST counter change together.
        tr.push(0.2, lit(lit(crop(), 460, 1015), 330, 1115));
        tr.push(0.3, lit(lit(crop(), 460, 1015), 330, 1115));
        // A pixel outside every region is ignored.
        tr.push(0.4, lit(crop(), 320, 990));
        assert_eq!(tr.events.len(), 2);
        let e = tr.events[0];
        assert_eq!(
            (e.t, e.judgements(), e.fast(), e.late()),
            (0.2, vec![Judgement::Great], true, false)
        );
        // Back to dark: the same rows change again.
        assert_eq!(tr.events[1].judgements(), vec![Judgement::Great]);
    }
}
