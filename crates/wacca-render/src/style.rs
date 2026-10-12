//! Flat placeholder look (M3): every colour and size in one place. Colours are sRGB, straight alpha.
//!
//! Hues follow `docs/research/04-gameplay-mechanics.md` §2.3 / `06-visuals.md` §3 (touch magenta/pink,
//! chain yellow, hold gold, slide CW orange / CCW green, snap in red / out blue). Exact RGB values are
//! approximations until `NotesColorTable` / `vMPC_NoteCustomize_00` are dumped (M6).

use wacca_chart::{Bonus, NoteBody, SlideDir, SnapDir};

pub type Rgba = [f32; 4];

pub const GUIDE_LANE: Rgba = [0.08, 0.08, 0.12, 1.0];
pub const LANE_LINE: Rgba = [0.22, 0.22, 0.30, 1.0];
pub const JUDGE_RING: Rgba = [0.92, 0.92, 0.98, 1.0];

pub const TOUCH: Rgba = [0.95, 0.25, 0.75, 1.0];
pub const CHAIN: Rgba = [1.0, 0.85, 0.10, 1.0];
pub const HOLD: Rgba = [1.0, 0.70, 0.15, 1.0];
pub const HOLD_BODY: Rgba = [1.0, 0.78, 0.30, 0.45];
pub const SLIDE_CW: Rgba = [1.0, 0.50, 0.05, 1.0];
pub const SLIDE_CCW: Rgba = [0.20, 0.85, 0.30, 1.0];
pub const SNAP_IN: Rgba = [0.95, 0.15, 0.15, 1.0];
pub const SNAP_OUT: Rgba = [0.15, 0.40, 1.0, 1.0];
/// Outer band marking R (2x score) notes.
pub const R_ACCENT: Rgba = [1.0, 1.0, 1.0, 1.0];
/// Outer band marking bonus (gauge) notes.
pub const BONUS_ACCENT: Rgba = [0.55, 0.95, 1.0, 1.0];

/// Note radial thickness as a fraction of its radius (scales with perspective, 04 §2.2).
pub const NOTE_THICKNESS: f32 = 0.07;
/// Accent band thickness as a fraction of the note radius.
pub const ACCENT_THICKNESS: f32 = 0.025;
/// Lanes trimmed from each end of a note narrower than the full ring (MM trims ~1 segment, 04 §2.3).
pub const NOTE_TRIM_LANES: f32 = 0.15;
pub const JUDGE_RING_WIDTH: f32 = 0.02;
pub const LANE_LINE_HALF_WIDTH: f32 = 0.002;
/// Playfield-space radius mapped to the edge of the square viewport (room for the ring).
pub const VIEW_EXTENT: f32 = 1.06;

pub fn note_color(body: &NoteBody) -> Rgba {
    match body {
        NoteBody::Touch => TOUCH,
        NoteBody::Chain => CHAIN,
        NoteBody::Hold { .. } => HOLD,
        NoteBody::Slide(SlideDir::Cw) => SLIDE_CW,
        NoteBody::Slide(SlideDir::Ccw) => SLIDE_CCW,
        NoteBody::Snap(SnapDir::In) => SNAP_IN,
        NoteBody::Snap(SnapDir::Out) => SNAP_OUT,
    }
}

pub fn accent_color(bonus: Bonus) -> Option<Rgba> {
    match bonus {
        Bonus::None => None,
        Bonus::Bonus => Some(BONUS_ACCENT),
        Bonus::R => Some(R_ACCENT),
    }
}
