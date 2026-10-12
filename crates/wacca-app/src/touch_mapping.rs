//! OS touch points → the virtual 60 × 4 sensor ring (`docs/research/04-gameplay-mechanics.md` §3).
//!
//! Uses the renderer's geometry: playfield space has the judgement line at radius 1, y up, lane 0
//! at 3 o'clock increasing counter-clockwise, and radius [`VIEW_EXTENT`] at the viewport edge.

use std::collections::BTreeMap;
use std::f32::consts::TAU;

use wacca_chart::LANES;
use wacca_game::input::{DEPTHS, SensorState};
use wacca_render::layout::Rect;
// The sensor band is shared with the renderer, which draws its rings.
use wacca_render::style::{SENSOR_INNER, SENSOR_OUTER, VIEW_EXTENT};
/// A contact also presses this many lanes on each side (a fingertip covers ~3 segments).
// UNVERIFIED: doc 04 §3 suggests ±1 segment, "tune".
pub const CONTACT_SPREAD: u8 = 1;

/// One sensor; `depth` 0 = innermost ring, `DEPTHS - 1` = rim (as in `wacca_game::input`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sensor {
    pub lane: u8,
    pub depth: usize,
}

/// Window pixel → playfield space for the square playfield viewport `vp`.
pub fn to_playfield(px: [f32; 2], vp: Rect) -> [f32; 2] {
    let (hw, hh) = (vp.w / 2.0, vp.h / 2.0);
    [
        (px[0] - vp.x - hw) / hw * VIEW_EXTENT,
        -(px[1] - vp.y - hh) / hh * VIEW_EXTENT,
    ]
}

/// Sensor under a playfield-space point, if it is inside the sensor band (or beyond it).
pub fn sensor_at(p: [f32; 2]) -> Option<Sensor> {
    let r = p[0].hypot(p[1]);
    if !r.is_finite() || r < SENSOR_INNER {
        return None;
    }
    let ring = (r - SENSOR_INNER) / (SENSOR_OUTER - SENSOR_INNER) * DEPTHS as f32;
    let depth = (ring as usize).min(DEPTHS - 1);
    let turns = p[1].atan2(p[0]).rem_euclid(TAU) / TAU;
    let lane = ((turns * LANES as f32) as u8).min(LANES - 1);
    Some(Sensor { lane, depth })
}

pub fn sensor_at_pixel(px: [f32; 2], vp: Rect) -> Option<Sensor> {
    sensor_at(to_playfield(px, vp))
}

/// Active contacts by id (touch id, or a reserved id for the mouse).
#[derive(Clone, Debug, Default)]
pub struct Contacts {
    active: BTreeMap<u64, Sensor>,
}

impl Contacts {
    /// Contact `id` is now over `sensor`; `None` (outside the band) lifts it.
    pub fn set(&mut self, id: u64, sensor: Option<Sensor>) {
        match sensor {
            Some(s) => {
                self.active.insert(id, s);
            }
            None => self.release(id),
        }
    }

    pub fn release(&mut self, id: u64) {
        self.active.remove(&id);
    }

    /// Sensor snapshot of all contacts, each widened by [`CONTACT_SPREAD`] lanes.
    pub fn sensors(&self) -> SensorState {
        let mut s = SensorState::default();
        let spread = i16::from(CONTACT_SPREAD);
        for c in self.active.values() {
            for o in -spread..=spread {
                let lane = (i16::from(c.lane) + o).rem_euclid(i16::from(LANES)) as u8;
                s.set(lane, c.depth, true);
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VP: Rect = Rect {
        x: 0.0,
        y: 100.0,
        w: 200.0,
        h: 200.0,
    };

    /// Pixel at playfield radius `r` and angle `deg` (counter-clockwise from 3 o'clock).
    fn px(r: f32, deg: f32) -> [f32; 2] {
        let a = deg.to_radians();
        let s = 100.0 / VIEW_EXTENT;
        [100.0 + r * a.cos() * s, 200.0 - r * a.sin() * s]
    }

    #[test]
    fn viewport_centre_and_edges() {
        let c = to_playfield([100.0, 200.0], VP);
        assert!(c[0].abs() < 1e-6 && c[1].abs() < 1e-6);
        let right = to_playfield([200.0, 200.0], VP);
        assert!((right[0] - VIEW_EXTENT).abs() < 1e-6);
        // Screen top is +y.
        assert!((to_playfield([100.0, 100.0], VP)[1] - VIEW_EXTENT).abs() < 1e-6);
    }

    #[test]
    fn lanes_start_at_three_oclock_counter_clockwise() {
        let at = |deg| sensor_at_pixel(px(1.0, deg), VP).unwrap().lane;
        assert_eq!(at(3.0), 0);
        assert_eq!(at(9.0), 1);
        assert_eq!(at(93.0), 15); // 12 o'clock
        assert_eq!(at(183.0), 30); // 9 o'clock
        assert_eq!(at(273.0), 45); // 6 o'clock
        assert_eq!(at(357.0), 59);
    }

    #[test]
    fn depth_rings_inside_out_and_clamped() {
        let band = SENSOR_OUTER - SENSOR_INNER;
        let depth = |r| sensor_at_pixel(px(r, 45.0), VP).map(|s| s.depth);
        assert_eq!(depth(SENSOR_INNER - 0.01), None);
        assert_eq!(depth(0.0), None);
        assert_eq!(depth(SENSOR_INNER + band * 0.1), Some(0));
        assert_eq!(depth(SENSOR_INNER + band * 0.4), Some(1));
        assert_eq!(depth(SENSOR_INNER + band * 0.6), Some(2));
        assert_eq!(depth(SENSOR_INNER + band * 0.9), Some(3));
        assert_eq!(depth(SENSOR_OUTER + 0.5), Some(3));
    }

    #[test]
    fn contacts_by_id_with_spread_and_wrap() {
        let mut c = Contacts::default();
        c.set(7, Some(Sensor { lane: 0, depth: 3 }));
        c.set(9, Some(Sensor { lane: 30, depth: 1 }));
        let s = c.sensors();
        assert!(s.get(59, 3) && s.get(0, 3) && s.get(1, 3) && !s.get(2, 3));
        assert!(s.get(29, 1) && s.get(30, 1) && s.get(31, 1) && !s.get(30, 3));
        // Moving a contact replaces its old position.
        c.set(7, Some(Sensor { lane: 10, depth: 0 }));
        assert!(!c.sensors().get(0, 3) && c.sensors().get(10, 0));
        // Leaving the band or ending lifts it.
        c.set(7, None);
        c.release(9);
        c.release(42);
        assert!(c.sensors().is_empty());
    }

    #[test]
    fn radial_swipe_crosses_all_depths() {
        assert_eq!(DEPTHS, wacca_render::style::SENSOR_DEPTHS);
        let depths: Vec<usize> = (0..=100)
            .filter_map(|i| sensor_at_pixel(px(0.4 + i as f32 * 0.008, 100.0), VP))
            .map(|s| s.depth)
            .collect();
        assert!(depths.windows(2).all(|w| w[0] <= w[1]));
        for d in 0..DEPTHS {
            assert!(depths.contains(&d), "depth {d} missing");
        }
    }
}
