//! Playfield look: every colour and size in one place. Colours are sRGB, straight alpha.
//!
//! Note colours are the defaults of the material parameter collection `vMPC_NoteCustomize_00`
//! (`NormalNote_Color1`, `ChainNote_Color1`, `HoldNote_Color1`, `SlideL/RNote_Color1`,
//! `SnapUp/DwnNote_Color1`; linear values converted with gamma 2.2, see `docs/research/06-visuals.md` §3).
//! Slide L (orange) is taken as clockwise and Snap Up (red) as inward, matching 04 §2.3.

use wacca_chart::{Bonus, NoteBody, SlideDir, SnapDir};

pub type Rgba = [f32; 4];

pub const GUIDE_LANE: Rgba = [0.08, 0.08, 0.12, 1.0];
/// Tint of the guide-lane gradient texture (bright at the rim, fading to the centre).
// UNVERIFIED: brightness from `GuideLaneBrightnessParamAlphaNormal` 600 (of 1000?) dimmed
// further because our lanes have no movie behind them.
pub const GUIDE_LANE_TINT: Rgba = [0.30, 0.34, 0.50, 0.3];
pub const LANE_LINE: Rgba = [0.22, 0.22, 0.30, 1.0];
pub const JUDGE_RING: Rgba = [0.92, 0.92, 0.98, 1.0];
/// Soft additive glow just inside the judgement ring (textured look only).
pub const JUDGE_GLOW: Rgba = [0.75, 0.85, 1.0, 0.4];
/// Dark band outside the judgement ring (the sensor rim).
pub const RIM: Rgba = [0.10, 0.10, 0.14, 1.0];
/// Tick marks on the rim every [`RIM_TICK_LANES`] lanes.
pub const RIM_TICK: Rgba = [0.35, 0.38, 0.50, 1.0];
pub const RIM_TICK_LANES: u8 = 5;
pub const RIM_WIDTH: f32 = 0.05;
/// Radial extent of the judgement glow inside the ring.
pub const JUDGE_GLOW_WIDTH: f32 = 0.08;
/// Pressed sensors, drawn over their ring.
pub const TOUCH_HIGHLIGHT: Rgba = [0.40, 0.90, 1.0, 0.45];
/// Faint circles at the sensor ring boundaries.
pub const SENSOR_GUIDE: Rgba = [0.40, 0.45, 0.60, 0.35];

pub const TOUCH: Rgba = [1.0, 0.220, 0.935, 1.0];
pub const CHAIN: Rgba = [1.0, 0.891, 0.270, 1.0];
// UNVERIFIED: `HoldNote_Color1` is a dark gold; it is brightened for the head so it reads on black.
pub const HOLD: Rgba = [0.95, 0.80, 0.10, 1.0];
pub const HOLD_BODY: Rgba = [1.0, 0.85, 0.30, 0.4];
pub const SLIDE_CW: Rgba = [1.0, 0.598, 0.0, 1.0];
pub const SLIDE_CCW: Rgba = [0.142, 0.823, 0.115, 1.0];
pub const SNAP_IN: Rgba = [0.867, 0.092, 0.092, 1.0];
pub const SNAP_OUT: Rgba = [0.118, 0.673, 1.0, 1.0];
/// Outer band marking R (2x score) notes.
pub const R_ACCENT: Rgba = [1.0, 1.0, 1.0, 1.0];
/// Outer band marking bonus (gauge) notes.
pub const BONUS_ACCENT: Rgba = [0.55, 0.95, 1.0, 1.0];

/// Slide chevrons: band inside the note (in note thicknesses), chevron length along the arc
/// (lanes) and scroll speed (lanes per second).
// UNVERIFIED: the slide/snap materials have no panner; the arrows are animated by the game code,
// speeds here are eyeballed.
pub const ARROW_BAND: f32 = 2.4;
pub const SLIDE_ARROW_LANES: f32 = 2.0;
pub const SLIDE_ARROW_SPEED: f32 = 8.0;
/// Snap chevrons: one per this many lanes, sliding radially once per period (seconds).
pub const SNAP_ARROW_LANES: f32 = 3.0;
pub const SNAP_ARROW_PERIOD: f32 = 0.5;
/// Chain stripe tile length (lanes) and darkening.
pub const CHAIN_STRIPE_LANES: f32 = 2.0;
pub const CHAIN_STRIPE: Rgba = [0.35, 0.20, 0.0, 0.45];
/// Bonus sparkles: one per this many lanes.
pub const SPARKLE_LANES: f32 = 3.0;
/// End caps of the note face (lanes) and the texture fraction they map to.
pub const NOTE_CAP_LANES: f32 = 0.35;
pub const NOTE_CAP_TEX: f32 = 0.12;

/// Hit flash at the judgement line: duration (s) and radial extent around the ring.
pub const FLASH_SECS: f32 = 0.25;
pub const FLASH_IN: f32 = 0.85;
pub const FLASH_OUT: f32 = 1.10;

/// Judgement sprite (BP_Game_Judge: 0.7 × 256×64 px, 100 px below centre on a 1080 px field)
/// and COMBO word (BP_Game_Combo: 140×35 px, 15 px below centre), in playfield units.
// UNVERIFIED: Slate units mapped with the playfield radius = 540 px.
pub const JUDGE_SPRITE_Y: f32 = -100.0 / 540.0;
pub const JUDGE_SPRITE_W: f32 = 0.7 * 256.0 / 540.0;
pub const COMBO_SPRITE_Y: f32 = -15.0 / 540.0;
pub const COMBO_SPRITE_W: f32 = 140.0 / 540.0;
/// Judgement sprite life (s) and pop-in time.
pub const JUDGE_SPRITE_SECS: f32 = 0.4;
pub const JUDGE_POP_SECS: f32 = 0.06;

/// Note radial thickness as a fraction of its radius (scales with perspective, 04 §2.2).
pub const NOTE_THICKNESS: f32 = 0.07;
/// Accent band thickness as a fraction of the note radius.
pub const ACCENT_THICKNESS: f32 = 0.025;
/// Lanes trimmed from each end of a note narrower than the full ring (MM trims ~1 segment, 04 §2.3).
pub const NOTE_TRIM_LANES: f32 = 0.15;
pub const JUDGE_RING_WIDTH: f32 = 0.02;
/// Radial half-width of a sensor guide circle.
pub const SENSOR_GUIDE_HALF_WIDTH: f32 = 0.002;

/// Touch sensor band (playfield units, judgement line = 1): [`SENSOR_DEPTHS`] equal rings from
/// `SENSOR_INNER` to `SENSOR_OUTER`. Touches nearer the centre hit nothing; touches beyond the
/// outer edge count as the outermost ring. Used by both drawing and touch mapping.
// UNVERIFIED: the cabinet's sensors sit on the rim; this band is sized for fingers on a laptop
// touchscreen so swipes in/out cross all rings comfortably.
pub const SENSOR_INNER: f32 = 0.55;
pub const SENSOR_OUTER: f32 = 1.15;
/// Depth rings (= `wacca_game::input::DEPTHS`).
pub const SENSOR_DEPTHS: usize = 4;
pub const LANE_LINE_HALF_WIDTH: f32 = 0.002;
/// Playfield-space radius mapped to the edge of the square viewport (room for the ring).
pub const VIEW_EXTENT: f32 = 1.17;

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

/// Hit-flash colour per judgement.
// UNVERIFIED: hues taken from the judgement word art (pink / yellow / cyan / grey).
pub fn flash_color(j: crate::sprites::JudgeSprite) -> Rgba {
    use crate::sprites::JudgeSprite;
    match j {
        JudgeSprite::Marvelous => [1.0, 0.45, 0.85, 0.9],
        JudgeSprite::Great => [1.0, 0.90, 0.40, 0.8],
        JudgeSprite::Good => [0.45, 0.85, 1.0, 0.7],
        JudgeSprite::Miss => [0.5, 0.5, 0.55, 0.0],
    }
}
