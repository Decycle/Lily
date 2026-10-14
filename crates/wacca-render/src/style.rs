//! Playfield look: every colour and size in one place. Colours are sRGB, straight alpha.
//!
//! Note colours are the defaults of the material parameter collection `vMPC_NoteCustomize_00`
//! (`NormalNote_Color1`, `ChainNote_Color1`, `HoldNote_Color1`, `SlideL/RNote_Color1`,
//! `SnapUp/DwnNote_Color1`; linear values converted with gamma 2.2, see `docs/research/06-visuals.md` §3).
//! Slide L (orange) is taken as clockwise and Snap Up (red) as inward, matching 04 §2.3.

use wacca_chart::{Bonus, NoteBody, SlideDir, SnapDir};

pub type Rgba = [f32; 4];

/// The HUD rim (UMG, drawn over the 3D view by the stage composition, `compose.wgsl`), in px of
/// the game's 675 px view. Outside the judgement ring the 3D view shows through (the guide-lane
/// glow fades out to 331 px); the HUD only adds the song-progress gauge and black beyond the
/// round screen. Measured on the E3 Fragile frames against our 3D-only renders (doc 13 §2):
/// * the HUD circle is centred [`HUD_RAISE_PX`] above the 3D view's centre (band edges 4 px
///   further out at 12 o'clock than at 6 o'clock);
/// * black beyond [`HUD_EDGE_PX`] (1 px soft edge);
/// * the time gauge (`BP_Game_time` → `M_time_guage`, material `uM_RGguage000`: ColorAndOpacity
///   black × 0.5, a band `|d − 0.96| < 0.01` of the radial distance `d` of its widget, lit
///   where `angle > 1 − Around`) darkens the part of the song still to play, from 12 o'clock
///   counter-clockwise: display values × (1 − [`GAUGE_SHADE`]) between 321.7 and 327.9 px, so
///   the widget radius is [`GAUGE_RADIUS_PX`].
pub const HUD_RAISE_PX: f32 = 2.0;
pub const HUD_EDGE_PX: f32 = 330.75;
pub const GAUGE_RADIUS_PX: f32 = 338.4;
pub const GAUGE_SHADE: f32 = 0.45;

// Score row along the top of the band (E2, 291.8 px = 1): seven orange SHINBI digits centred
// at 12 o'clock (x-height centre 297 px out, 90 px for the row), "SCORE" 14° left of them,
// the difficulty and level ("NORMAL/Lv.1") 22° right, the song title 46° right.
/// Score digits: zero-padded width, cell height (as [`COMBO_DIGIT_H`]), centre radius, pitch
/// along the arc and colour.
pub const SCORE_DIGITS: usize = 7;
pub const SCORE_DIGIT_H: f32 = 0.042 * 97.0 / 58.0;
pub const SCORE_RADIUS: f32 = 1.018 + SCORE_DIGIT_H * 3.5 / 97.0;
pub const SCORE_PITCH: f32 = 0.044;
pub const SCORE_COLOR: Rgba = [1.0, 0.68, 0.10, 1.0];
/// Rim texts: angle clockwise from 12 o'clock (degrees), centre radius, box height and width
/// stretch (the game spaces its letters wider than the font).
pub const SCORE_LABEL_AT: (f32, f32, f32, f32) = (-13.8, 1.03, 0.036, 1.2);
pub const SCORE_LABEL_COLOR: Rgba = [0.86, 0.42, 0.08, 1.0];
pub const DIFFICULTY_AT: (f32, f32, f32, f32) = (22.0, 1.035, 0.05, 1.25);
pub const TITLE_AT: (f32, f32, f32, f32) = (46.0, 1.03, 0.05, 1.0);
pub const TITLE_COLOR: Rgba = [1.0, 0.55, 0.85, 1.0];
/// Longest title arc (degrees); longer titles are squeezed.
pub const TITLE_MAX_DEG: f32 = 26.0;

/// Clear gauge above the combo, opaque (E3 Fragile frame at 34.4 s, per-pixel, 675 px view):
/// rows 243–247 (centre 92 px above the view centre), x 198.5–477; 1 px border rows
/// (141, 63, 159) over the track, (154, 45, 161) over the fill; interior track (42, 45, 57),
/// fill (255, 149, 218); a 2 px norma mark (251, 2, 168) followed by a gradient from
/// (213, 55, 141) to the track over 18 px; slanted end caps (131, 0, 160), 5 px.
pub const GAUGE_Y: f32 = 92.0 / 291.8;
pub const GAUGE_HALF_W: f32 = 139.25 / 291.8;
pub const GAUGE_HALF_H: f32 = 2.5 / 291.8;
/// Half height of the interior (between the border rows).
pub const GAUGE_INNER_HALF_H: f32 = 1.5 / 291.8;
pub const GAUGE_BORDER: Rgba = [0.553, 0.247, 0.624, 1.0];
pub const GAUGE_TRACK: Rgba = [0.165, 0.176, 0.224, 1.0];
pub const GAUGE_FILL_BORDER: Rgba = [0.604, 0.176, 0.631, 1.0];
pub const GAUGE_FILL: Rgba = [1.0, 0.584, 0.855, 1.0];
pub const GAUGE_MARK: Rgba = [0.984, 0.008, 0.659, 1.0];
pub const GAUGE_MARK_W: f32 = 2.0 / 291.8;
pub const GAUGE_NORMA_FADE: (Rgba, f32) = ([0.835, 0.216, 0.553, 1.0], 18.0 / 291.8);
pub const GAUGE_CAP: (Rgba, f32) = ([0.514, 0.0, 0.627, 1.0], 5.0 / 291.8);

/// Touch guide (option, off by default): faint depth-ring circles and pressed sensors.
/// Touch guide: a faint tint over the whole sensor band, a bright inner border (touches nearer the
/// centre do nothing), thinner circles between the depth rings and a faint line between lanes.
pub const TOUCH_GUIDE_BAND: Rgba = [0.55, 0.75, 1.0, 0.07];
pub const TOUCH_GUIDE_EDGE: Rgba = [0.80, 0.90, 1.0, 0.65];
pub const TOUCH_GUIDE_EDGE_HALF_WIDTH: f32 = 0.006;
pub const TOUCH_GUIDE_RING: Rgba = [0.80, 0.85, 1.0, 0.24];
pub const TOUCH_GUIDE_RING_HALF_WIDTH: f32 = 0.0025;
pub const TOUCH_GUIDE_LANE: Rgba = [0.80, 0.85, 1.0, 0.09];
/// Half width of a lane divider, in lanes.
pub const TOUCH_GUIDE_LANE_HALF_WIDTH: f32 = 0.025;
pub const TOUCH_GUIDE_PRESSED: Rgba = [0.80, 0.90, 1.0, 0.30];
/// Measure lines: thin white circles moving with the notes.
pub const BAR_LINE: Rgba = [1.0, 1.0, 1.0, 0.55];
pub const BAR_LINE_HALF_WIDTH: f32 = 0.0025;
/// Most measure lines drawn per frame.
pub const BAR_LINE_MAX: u32 = 64;

pub const TOUCH: Rgba = [1.0, 0.220, 0.935, 1.0];
pub const CHAIN: Rgba = [1.0, 0.891, 0.270, 1.0];
// UNVERIFIED: `HoldNote_Color1` is a dark gold; it is brightened for the head so it reads on black.
pub const HOLD: Rgba = [0.95, 0.80, 0.10, 1.0];
pub const SLIDE_CW: Rgba = [1.0, 0.598, 0.0, 1.0];
pub const SLIDE_CCW: Rgba = [0.142, 0.823, 0.115, 1.0];
pub const SNAP_IN: Rgba = [0.867, 0.092, 0.092, 1.0];
pub const SNAP_OUT: Rgba = [0.118, 0.673, 1.0, 1.0];
/// Outer band marking R (2x score) notes.
pub const R_ACCENT: Rgba = [1.0, 1.0, 1.0, 1.0];
/// Outer band marking bonus (gauge) notes.
pub const BONUS_ACCENT: Rgba = [0.55, 0.95, 1.0, 1.0];

/// Snap chevrons: band inside the note (in note thicknesses).
pub const ARROW_BAND: f32 = 2.4;
/// Slide chevrons (E2 frames): a row of separate chevrons inside the note at 0.86 of its
/// radius, one per [`SLIDE_ARROW_LANES`], growing from the back of the note to its front (the
/// slide direction) from [`SLIDE_ARROW_MIN`] to [`SLIDE_ARROW_MAX`] of the note radius, and
/// travelling forwards at [`SLIDE_ARROW_SPEED`] lanes per second.
// UNVERIFIED: the travel speed (the 0.1 s reference frames only show the conveyor moved).
pub const SLIDE_ARROW_RADIUS: f32 = 0.86;
pub const SLIDE_ARROW_LANES: f32 = 1.9;
pub const SLIDE_ARROW_SPEED: f32 = 8.0;
pub const SLIDE_ARROW_MIN: f32 = 0.04;
pub const SLIDE_ARROW_MAX: f32 = 0.19;
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

/// Note end caps (lanes) and their colour (cyan blocks at both ends in the E2 frames).
// UNVERIFIED: only touch notes were seen up close; applied to every note.
pub const NOTE_END_CAP_LANES: f32 = 0.2;
pub const NOTE_END_CAP: Rgba = [0.35, 0.80, 1.0, 1.0];

// Centre HUD, measured on the E2 frames (line radius 291.8 px = 1.0, y up):
// "Marvelous" glyphs 120 px wide centred 60 px below the centre; "100" digits 21 px tall
// centred 55 px above; COMBO glyphs 59 px wide centred 30 px above.
/// Judgement sprite: centre and full sprite width (`uT_FTdecision01` glyphs span 250/256).
pub const JUDGE_SPRITE_Y: f32 = -0.205;
pub const JUDGE_SPRITE_W: f32 = 0.421;
/// COMBO word (`uT_FTcombo01`, glyphs span 932/1024): centre and full width.
pub const COMBO_SPRITE_Y: f32 = 0.099;
pub const COMBO_SPRITE_W: f32 = 0.222;
/// Combo digits: centre and height of the full digit cell (without padding). SHINBI has
/// old-style figures: "100" (x-height, 21 px) is 58/97 of the full ascender-to-descender
/// height, whose centre sits 3.5/97 of it above the x-height centre.
pub const COMBO_DIGIT_H: f32 = 0.072 * 97.0 / 58.0;
pub const COMBO_DIGIT_Y: f32 = 0.188 + COMBO_DIGIT_H * 3.5 / 97.0;
/// Lavender tint of the digits and the COMBO word, and the soft shadow under the word.
pub const COMBO_TINT: Rgba = [0.82, 0.78, 1.0, 1.0];
pub const COMBO_SHADOW: Rgba = [0.05, 0.02, 0.10, 0.6];
pub const COMBO_SHADOW_OFFSET: f32 = 0.004;
/// Count animation: pop scale and its duration (s), chromatic split width and duration.
pub const COMBO_POP: f32 = 1.5;
pub const COMBO_POP_SECS: f32 = 0.25;
pub const COMBO_SPLIT: f32 = 0.012;
pub const COMBO_SPLIT_SECS: f32 = 0.25;
pub const COMBO_SPLIT_CYAN: Rgba = [0.25, 0.90, 1.0, 0.8];
pub const COMBO_SPLIT_MAGENTA: Rgba = [1.0, 0.30, 0.80, 0.8];
/// Judgement sprite life (s), fade-in and fade-out times (E2: visible ~0.27 s).
pub const JUDGE_SPRITE_SECS: f32 = 0.27;
pub const JUDGE_FADE_IN_SECS: f32 = 0.025;
pub const JUDGE_FADE_SECS: f32 = 0.05;

/// Note radial thickness as a fraction of its radius (scales with perspective, 04 §2.2).
pub const NOTE_THICKNESS: f32 = 0.07;
/// Accent band thickness as a fraction of the note radius.
pub const ACCENT_THICKNESS: f32 = 0.025;
/// Lanes trimmed from each end of a note narrower than the full ring (MM trims ~1 segment, 04 §2.3).
pub const NOTE_TRIM_LANES: f32 = 0.15;

/// Touch sensor band (playfield units, judgement line = 1): [`SENSOR_DEPTHS`] equal rings from
/// `SENSOR_INNER` to `SENSOR_OUTER`. Touches nearer the centre hit nothing; touches beyond the
/// outer edge count as the outermost ring. Used by both drawing and touch mapping.
// UNVERIFIED: the cabinet's sensors sit on the rim; this band is sized for fingers on a laptop
// touchscreen so swipes in/out cross all rings comfortably.
pub const SENSOR_INNER: f32 = 0.55;
pub const SENSOR_OUTER: f32 = 1.15;
/// Depth rings (= `wacca_game::input::DEPTHS`).
pub const SENSOR_DEPTHS: usize = 4;
/// Playfield-space radius mapped to the edge of the square viewport: the game's view is a
/// 675 px square (NDC 1 = 337.5 px, doc 11 §2.2) whose rim inner edge (radius 1) is at 291.8 px (E2).
pub const VIEW_EXTENT: f32 = 337.5 / 291.8;
/// Playfield-space radius of the rim's outer edge: the edge of the visible round screen.
pub const SCREEN_EDGE: f32 = HUD_EDGE_PX / 291.8;

/// Time-gauge coverage at `r` px (675 px view) from the HUD centre: the band of
/// `uM_RGguage000`, `sat(10·(1 − 100·|d − 0.96|))` with `d = r / GAUGE_RADIUS_PX` (the
/// material's `frac(d + 0.54) − 0.5` is `d − 0.96` near the band). Mirrored in `compose.wgsl`.
pub fn gauge_band(r: f32) -> f32 {
    let d = r / GAUGE_RADIUS_PX;
    (10.0 * (1.0 - 100.0 * (d - 0.96).abs())).clamp(0.0, 1.0)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_band_is_where_the_recordings_darken() {
        // E3 Fragile: × 0.55 from 321.7 to 327.9 px, untouched at 320.5 and 329.
        for r in [322.5, 324.8, 327.0] {
            assert_eq!(gauge_band(r), 1.0, "{r}");
        }
        for r in [320.5, 329.0, 300.0] {
            assert_eq!(gauge_band(r), 0.0, "{r}");
        }
        assert!((SCREEN_EDGE * 291.8 - HUD_EDGE_PX).abs() < 1e-3);
    }
}
