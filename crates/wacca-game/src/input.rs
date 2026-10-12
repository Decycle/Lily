//! Touch input as the cabinet sees it: 60 lanes x 4 depth rings of sensors.
//!
//! The app maps OS touch points onto this grid; the judge engine only ever sees `InputFrame`s,
//! so tests can script input exactly.

use wacca_chart::LANES;

/// Depth rings, 0 = innermost, 3 = outermost (rim).
pub const DEPTHS: usize = 4;

/// One bit per sensor: `rings[depth]` bit `lane`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SensorState {
    pub rings: [u64; DEPTHS],
}

impl SensorState {
    pub fn get(&self, lane: u8, depth: usize) -> bool {
        debug_assert!(lane < LANES && depth < DEPTHS);
        self.rings[depth] >> lane & 1 == 1
    }

    pub fn set(&mut self, lane: u8, depth: usize, on: bool) {
        debug_assert!(lane < LANES && depth < DEPTHS);
        if on {
            self.rings[depth] |= 1 << lane;
        } else {
            self.rings[depth] &= !(1 << lane);
        }
    }

    /// Any depth touched in `lane`.
    pub fn lane_touched(&self, lane: u8) -> bool {
        self.rings.iter().any(|r| r >> lane & 1 == 1)
    }

    pub fn is_empty(&self) -> bool {
        self.rings.iter().all(|&r| r == 0)
    }
}

/// Sensor snapshot at a chart time (seconds). Frames must be fed in non-decreasing time order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputFrame {
    pub time: f64,
    pub sensors: SensorState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get() {
        let mut s = SensorState::default();
        s.set(59, 3, true);
        s.set(0, 0, true);
        assert!(s.get(59, 3) && s.get(0, 0) && !s.get(59, 2));
        assert!(s.lane_touched(59) && !s.lane_touched(1));
        s.set(59, 3, false);
        s.set(0, 0, false);
        assert!(s.is_empty());
    }
}
