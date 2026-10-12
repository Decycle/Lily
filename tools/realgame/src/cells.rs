//! Lane/depth <-> Waccon/segatools touch-cell index.
//!
//! Waccon (and toucca before it) number the 240 cells `side * 120 + ring * 30 + sector`: side 0 = visual right,
//! side 1 = visual left, ring 0 = innermost, sector 0 = top of each half, increasing downwards. segatools packs
//! cells 0..120 into the right touch board (COM3) and 120..240 into the left one (COM4).
//!
//! With lane 0 = the 6 degree segment just above 3 o'clock and lanes counter-clockwise (docs/research/04):
//! right side sector `s` = lane `(14 - s) mod 60`, left side sector `s` = lane `15 + s`.
//! UNVERIFIED: that the game's lane numbering and ring order match this; checked by experiment E1 in doc 07.

use wacca_chart::LANES;
use wacca_game::input::{DEPTHS, SensorState};

pub const CELLS: usize = 240;
const SECTORS: usize = 30;
const SIDE_CELLS: usize = 120;

/// Cell index for a lane (0..60) and depth (0 = innermost .. 3 = rim).
pub fn cell_index(lane: u8, depth: usize) -> usize {
    debug_assert!(lane < LANES && depth < DEPTHS);
    let lane = usize::from(lane);
    let (side, sector) = if (15..45).contains(&lane) {
        (1, lane - 15)
    } else {
        (0, (74 - lane) % 60)
    };
    side * SIDE_CELLS + depth * SECTORS + sector
}

/// Inverse of [`cell_index`]: `(lane, depth)`.
pub fn cell_lane_depth(cell: usize) -> (u8, usize) {
    debug_assert!(cell < CELLS);
    let side = cell / SIDE_CELLS;
    let depth = cell % SIDE_CELLS / SECTORS;
    let sector = cell % SECTORS;
    let lane = if side == 1 {
        15 + sector
    } else {
        (74 - sector) % 60
    };
    (lane as u8, depth)
}

/// Sensor grid -> 240 bytes of 0/1 as used by WCON input snapshots and the shared buffer.
pub fn to_cells(s: &SensorState) -> [u8; CELLS] {
    let mut out = [0; CELLS];
    for depth in 0..DEPTHS {
        for lane in 0..LANES {
            if s.get(lane, depth) {
                out[cell_index(lane, depth)] = 1;
            }
        }
    }
    out
}

/// 240 cell bytes (nonzero = touched) -> sensor grid.
pub fn from_cells(cells: &[u8; CELLS]) -> SensorState {
    let mut s = SensorState::default();
    for (cell, &v) in cells.iter().enumerate() {
        if v != 0 {
            let (lane, depth) = cell_lane_depth(cell);
            s.set(lane, depth, true);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_cells() {
        // Right half, top sector = lane 14; lane 0 is the sector just above 3 o'clock.
        assert_eq!(cell_index(14, 0), 0);
        assert_eq!(cell_index(0, 0), 14);
        assert_eq!(cell_index(59, 0), 15);
        assert_eq!(cell_index(45, 3), 3 * 30 + 29);
        // Left half, top sector = lane 15, bottom = lane 44.
        assert_eq!(cell_index(15, 0), 120);
        assert_eq!(cell_index(44, 3), 120 + 90 + 29);
    }

    #[test]
    fn bijective() {
        let mut seen = [false; CELLS];
        for lane in 0..LANES {
            for depth in 0..DEPTHS {
                let c = cell_index(lane, depth);
                assert!(!seen[c]);
                seen[c] = true;
                assert_eq!(cell_lane_depth(c), (lane, depth));
            }
        }
        assert!(seen.iter().all(|&x| x));
    }

    #[test]
    fn grid_round_trip() {
        let mut s = SensorState::default();
        s.set(0, 3, true);
        s.set(30, 0, true);
        s.set(59, 1, true);
        let cells = to_cells(&s);
        assert_eq!(cells.iter().filter(|&&c| c == 1).count(), 3);
        assert_eq!(from_cells(&cells), s);
    }
}
