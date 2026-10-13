//! Builds the playfield vertex list for a chart at a given time (pure, no GPU).
//!
//! With an empty [`SpriteSheet`] everything is drawn in flat colours; each loaded sprite switches
//! its element to the original texture.

use std::f32::consts::{PI, TAU};

use wacca_chart::{BeatPos, Bonus, Chart, LANES, Note, NoteBody, SlideDir, SnapDir};

use crate::effects::Effects;
use crate::layout::{
    Paint, UvRect, Vertex, depth_to_radius, last_drawable_time, paint_sector, polar,
    push_oriented_rect, push_rect, push_sector, raw_depth, sensor_ring, three_slice, tiles,
};
use crate::sprites::{DIGIT_PAD, JudgeSprite, Sprite, SpriteSheet, TextSprite};
use crate::style;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneOptions {
    /// Player note-speed option (1.0..=6.0).
    pub note_speed: f64,
    /// A background movie plays: dark translucent guide sectors over it, faint lane lines.
    pub movie: bool,
    /// Measure lines (`BarLineTable` option).
    pub bar_lines: bool,
    /// Touch guide: faint depth-ring circles and pressed sensors (an aid for touchscreens; the
    /// cabinet's rings are physical, the real game draws none).
    pub touch_guide: bool,
    /// Note heads are drawn as 3D meshes ([`crate::notes3d`]) between the two parts of the
    /// scene (see [`build_scene_into`]); the 2D scene leaves them out.
    pub mesh_notes: bool,
}

/// Score row and song progress along the rim.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RimState {
    pub score: u32,
    /// Played fraction of the song, 0..=1.
    pub progress: f32,
    /// Colour of the difficulty/level text.
    pub difficulty_color: style::Rgba,
}

/// Live play state drawn on top of the chart.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveState<'a> {
    /// `judged[i]`: note `i` (index into `Chart::notes`) is decided and no longer drawn.
    /// Shorter than the note list = the rest are not judged.
    pub judged: &'a [bool],
    /// Hit effects, key beams and hold glows.
    pub effects: Option<&'a Effects>,
    /// Last judgement and its age in seconds (judgement word art).
    pub judgement: Option<(JudgeSprite, f32)>,
    /// Current combo (counter and COMBO word show while it is non-zero) and seconds since it
    /// last changed (count animation).
    pub combo: (u32, f32),
    /// Score row and progress on the rim.
    pub rim: Option<RimState>,
    /// Clear gauge `(fill, norma)`, both 0..=1.
    pub gauge: Option<(f32, f32)>,
    /// Pressed sensors per depth ring (bit per lane, innermost first), for the touch guide.
    pub pressed: [u64; style::SENSOR_DEPTHS],
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            note_speed: 1.0,
            movie: false,
            bar_lines: false,
            touch_guide: false,
            mesh_notes: false,
        }
    }
}

/// Judgement sprite opacity `age` seconds after the judgement (E2 recordings: fades in within
/// a frame or two, holds, fades out over the last [`style::JUDGE_FADE_SECS`]; no scale pop);
/// `None` once gone.
pub fn judge_sprite_alpha(age: f32) -> Option<f32> {
    let life = style::JUDGE_SPRITE_SECS;
    if !(0.0..life).contains(&age) {
        return None;
    }
    let fade_in = 0.4 + 0.6 * (age / style::JUDGE_FADE_IN_SECS).min(1.0);
    let fade_out = ((life - age) / style::JUDGE_FADE_SECS).min(1.0);
    Some(fade_in.min(fade_out))
}

/// Combo counter `(scale, split)` `age` seconds after the count changed: the digits pop from
/// [`style::COMBO_POP`]× and show a cyan/magenta split of `split` playfield units that closes
/// over [`style::COMBO_SPLIT_SECS`] (E2 recordings).
pub fn combo_anim(age: f32) -> (f32, f32) {
    let ease = |secs: f32| (1.0 - (age / secs).clamp(0.0, 1.0)).powi(2);
    (
        1.0 + (style::COMBO_POP - 1.0) * ease(style::COMBO_POP_SECS),
        style::COMBO_SPLIT * ease(style::COMBO_SPLIT_SECS),
    )
}

/// Snap chevrons at time `now`: radial shift (fraction of a note thickness, growing in the snap
/// direction) and opacity (fades in and out each [`style::SNAP_ARROW_PERIOD`]).
pub fn snap_arrow_anim(now: f32) -> (f32, f32) {
    let p = (now / style::SNAP_ARROW_PERIOD).rem_euclid(1.0);
    (p, (p * PI).sin())
}

/// Tile length near `len` that splits `span` into whole tiles (at least one).
pub fn even_tile(span: f32, len: f32) -> f32 {
    span / (span / len).round().max(1.0)
}

/// Slide chevron positions (lanes) at `now` along a note of `span` lanes, measured from its
/// back (where the slide starts) towards its front; they travel forwards over time.
pub fn slide_arrow_slots(span: f32, now: f32) -> Vec<f32> {
    let pitch = style::SLIDE_ARROW_LANES;
    let phase = (now * style::SLIDE_ARROW_SPEED / pitch).rem_euclid(1.0);
    (0..)
        .map(|k| (k as f32 + phase) * pitch)
        .take_while(|&pos| pos < span)
        .collect()
}

/// Triangle list (back to front) of the playfield at chart time `now`.
pub fn build_scene(
    chart: &Chart,
    now: f64,
    opts: &SceneOptions,
    live: &LiveState,
    sheet: &SpriteSheet,
) -> Vec<Vertex> {
    let mut out = Vec::new();
    build_scene_into(&mut out, chart, now, opts, live, sheet);
    out
}

/// [`build_scene`] into `out` (cleared first), so a caller can reuse one buffer every frame.
/// The guide lanes, judgement ring and hold bodies are 3D ([`crate::stage`]), drawn under it.
pub fn build_scene_into(
    out: &mut Vec<Vertex>,
    chart: &Chart,
    now: f64,
    opts: &SceneOptions,
    live: &LiveState,
    sheet: &SpriteSheet,
) {
    out.clear();
    if opts.touch_guide {
        push_touch_guide(out, &live.pressed);
    }

    let last_time = last_drawable_time(chart, now);
    let now_dist = chart.dist_at(chart.view_time(now));
    let depth_of = |t: f64| raw_depth(chart, t, now_dist, opts.note_speed);
    let judged = |i: usize| live.judged.get(i).copied().unwrap_or(false);
    let drawable =
        |&(i, n): &(usize, &Note)| !judged(i) && last_time.is_none_or(|last| n.time <= last);
    let visible = |t: f64| {
        Some(depth_of(t))
            .filter(|d| (0.0..=1.0).contains(d))
            .map(|d| d as f32)
    };

    if opts.bar_lines && !opts.mesh_notes {
        push_bar_lines(out, chart, now, depth_of);
    }
    let t = now as f32;
    // Far notes first so nearer ones are drawn on top.
    for (_, note) in chart.notes.iter().enumerate().rev().filter(drawable) {
        if let NoteBody::Hold { nodes } = &note.body
            && let Some(end) = nodes.last().filter(|_| nodes.len() > 1)
            && let Some(d) = visible(end.time)
        {
            let look = NoteLook {
                color: style::HOLD,
                body: None,
                bonus: Bonus::None,
            };
            push_note(out, sheet, t, d, end.arc.lane, end.arc.width, &look);
        }
        if let Some(d) = visible(note.time).filter(|_| !opts.mesh_notes) {
            let look = NoteLook {
                color: style::note_color(&note.body),
                body: Some(&note.body),
                bonus: note.bonus,
            };
            push_note(out, sheet, t, d, note.arc.lane, note.arc.width, &look);
        }
    }

    push_rim(out);
    if let Some(rim) = &live.rim {
        push_rim_row(out, sheet, rim);
    }
    if let Some(fx) = live.effects {
        fx.draw(out, sheet);
    }
    push_hud_sprites(out, sheet, live);
}

/// The dark rim (UMG HUD) outside the judgement ring with its thin outer edge, black beyond it
/// (E2: 315–331 px at a 291.8 px line). The ring itself is the guide-lane shader's.
fn push_rim(out: &mut Vec<Vertex>) {
    let band_out = 1.0 + style::JUDGE_BAND_WIDTH;
    let rim_out = band_out + style::RIM_WIDTH;
    let all = LANES as f32;
    // Nothing shows outside the cabinet rim (the movie quad would, E2 frames are black there).
    let corner = style::VIEW_EXTENT * std::f32::consts::SQRT_2;
    push_sector(out, rim_out, corner, 0.0, all, style::OUTSIDE);
    push_sector(out, band_out, rim_out, 0.0, all, style::RIM);
    let e = style::RIM_EDGE_HALF_WIDTH;
    push_sector(out, rim_out - e, rim_out + e, 0.0, all, style::RIM_EDGE);
}

/// Lane at `deg` degrees clockwise from 12 o'clock.
fn lane_at_deg(deg: f32) -> f32 {
    LANES as f32 / 4.0 - deg * LANES as f32 / 360.0
}

/// Image `uv` bent along the ring, upright at the top: centred on `lane` at radius `r`, `size`
/// = `[arc length, height]` (playfield units), its top edge outwards.
fn push_bent(out: &mut Vec<Vertex>, uv: UvRect, lane: f32, r: f32, size: [f32; 2], c: style::Rgba) {
    let [u0, v0, u1, v1] = uv;
    let lanes = size[0] / r * LANES as f32 / TAU;
    let (top, bottom) = (r + size[1] / 2.0, r - size[1] / 2.0);
    // Lanes grow counter-clockwise (right to left at the top): the text's right end first.
    let paint = Paint::image(c, [u1, v0, u0, v1]);
    paint_sector(out, top, bottom, lane - lanes / 2.0, lanes, &paint);
}

/// Song progress on the dark rim, the score row ("SCORE", seven SHINBI digits), the
/// difficulty/level and the song title along the top of the band (layout in `style`).
fn push_rim_row(out: &mut Vec<Vertex>, sheet: &SpriteSheet, rim: &RimState) {
    let (r0, r1) = style::PROGRESS_RADII;
    let progress = rim.progress.clamp(0.0, 1.0) * LANES as f32;
    if progress > 0.0 {
        push_sector(out, r0, r1, LANES as f32 / 4.0, progress, style::PROGRESS);
    }
    let texts = [
        (
            TextSprite::ScoreLabel,
            style::SCORE_LABEL_AT,
            style::SCORE_LABEL_COLOR,
        ),
        (
            TextSprite::Difficulty,
            style::DIFFICULTY_AT,
            rim.difficulty_color,
        ),
        (TextSprite::Title, style::TITLE_AT, style::TITLE_COLOR),
    ];
    for (t, (deg, r, h, stretch), c) in texts {
        let s = Sprite::Text(t);
        if let (Some(uv), Some(aspect)) = (sheet.get(s), sheet.aspect(s)) {
            let max = style::TITLE_MAX_DEG.to_radians() * r;
            let w = (h / aspect * stretch).min(max);
            push_bent(out, uv, lane_at_deg(deg), r, [w, h], c);
        }
    }
    let digits = format!("{:0width$}", rim.score, width = style::SCORE_DIGITS);
    let cell_h = style::SCORE_DIGIT_H * (1.0 + 2.0 * DIGIT_PAD);
    let r = style::SCORE_RADIUS;
    let n = digits.len() as f32;
    for (k, b) in digits.bytes().enumerate() {
        let s = Sprite::Digit(b - b'0');
        if let (Some(uv), Some(aspect)) = (sheet.get(s), sheet.aspect(s)) {
            let arc = (k as f32 - (n - 1.0) / 2.0) * style::SCORE_PITCH;
            let lane = LANES as f32 / 4.0 - arc / r * LANES as f32 / TAU;
            push_bent(
                out,
                uv,
                lane,
                r,
                [cell_h / aspect, cell_h],
                style::SCORE_COLOR,
            );
        }
    }
}

/// Faint circles at the sensor depth-ring edges and the pressed sensors filled.
fn push_touch_guide(out: &mut Vec<Vertex>, pressed: &[u64; style::SENSOR_DEPTHS]) {
    let w = style::TOUCH_GUIDE_RING_HALF_WIDTH;
    for (depth, &bits) in pressed.iter().enumerate() {
        let (r_in, r_out) = sensor_ring(depth);
        for lane in (0..LANES).filter(|&l| bits >> l & 1 == 1) {
            let c = style::TOUCH_GUIDE_PRESSED;
            push_sector(out, r_in, r_out, f32::from(lane), 1.0, c);
        }
        let edges = if depth + 1 == style::SENSOR_DEPTHS {
            vec![r_in, r_out]
        } else {
            vec![r_in]
        };
        for r in edges {
            push_sector(
                out,
                r - w,
                r + w,
                0.0,
                LANES as f32,
                style::TOUCH_GUIDE_RING,
            );
        }
    }
}

/// Clear gauge bar above the combo: dark track, pink fill from the left with a light core,
/// diamond end marks and a mark at the norma line.
fn push_gauge(out: &mut Vec<Vertex>, (fill, norma): (f32, f32)) {
    let (hw, hh, y) = (style::GAUGE_HALF_W, style::GAUGE_HALF_H, style::GAUGE_Y);
    push_rect(out, [0.0, y], [hw, hh], &Paint::flat(style::GAUGE_TRACK));
    let f = fill.clamp(0.0, 1.0) * hw;
    if f > 0.0 {
        push_rect(out, [f - hw, y], [f, hh], &Paint::flat(style::GAUGE_FILL));
        push_rect(
            out,
            [f - hw, y],
            [f, hh * 0.3],
            &Paint::flat(style::GAUGE_CORE),
        );
    }
    let m = style::GAUGE_MARK_HALF;
    let x_norma = (norma.clamp(0.0, 1.0) * 2.0 - 1.0) * hw;
    for x in [-hw, hw, x_norma] {
        let paint = Paint::flat(style::GAUGE_MARK);
        push_oriented_rect(out, [x, y], [m, m], [1.0, 1.0], &paint);
    }
}

/// Chart times of the next measure starts from `now` (at most [`style::BAR_LINE_MAX`]).
pub(crate) fn bar_line_times(chart: &Chart, now: f64) -> impl Iterator<Item = f64> + '_ {
    let ticks = BeatPos {
        measure: 1,
        tick: 0,
    }
    .abs_tick() as f64;
    let first = (chart.sec_to_tick(chart.view_time(now)) / ticks)
        .floor()
        .max(0.0) as u32;
    (first..first + style::BAR_LINE_MAX)
        .map(|measure| chart.tick_to_sec(BeatPos { measure, tick: 0 }.abs_tick()))
        .take_while(|&t| t <= chart.end_time)
}

/// Thin white circles at measure starts, travelling like notes (vMsh_BarLine00_60_s).
fn push_bar_lines(out: &mut Vec<Vertex>, chart: &Chart, now: f64, depth_of: impl Fn(f64) -> f64) {
    for t in bar_line_times(chart, now) {
        let d = depth_of(t);
        if (0.0..=1.0).contains(&d) {
            let r = depth_to_radius(d as f32);
            let w = style::BAR_LINE_HALF_WIDTH;
            push_sector(out, r - w, r + w, 0.0, LANES as f32, style::BAR_LINE);
        }
    }
}

/// Judgement word art, the combo counter (SHINBI digits) and the COMBO word at the centre
/// (each only with its sprites loaded). Layout measured on the E2 frames, see `style`.
fn push_hud_sprites(out: &mut Vec<Vertex>, sheet: &SpriteSheet, live: &LiveState) {
    if let Some(g) = live.gauge {
        push_gauge(out, g);
    }
    if let Some((j, age)) = live.judgement
        && let Some(alpha) = judge_sprite_alpha(age)
        && let (Some(uv), Some(aspect)) =
            (sheet.get(Sprite::Judge(j)), sheet.aspect(Sprite::Judge(j)))
    {
        let hw = style::JUDGE_SPRITE_W / 2.0;
        let paint = Paint::image([1.0, 1.0, 1.0, alpha], uv);
        push_rect(out, [0.0, style::JUDGE_SPRITE_Y], [hw, hw * aspect], &paint);
    }
    let (combo, age) = live.combo;
    if combo == 0 {
        return;
    }
    let (scale, split) = combo_anim(age);
    // Main copy plus, while the count animation runs, cyan and magenta copies either side.
    let copies = [
        (-split, style::COMBO_SPLIT_CYAN),
        (split, style::COMBO_SPLIT_MAGENTA),
        (0.0, style::COMBO_TINT),
    ];
    let copies = &copies[if split > 1e-4 { 0 } else { 2 }..];
    if let (Some(uv), Some(aspect)) = (sheet.get(Sprite::Combo), sheet.aspect(Sprite::Combo)) {
        let hw = style::COMBO_SPRITE_W / 2.0;
        let half = [hw, hw * aspect];
        let y = style::COMBO_SPRITE_Y;
        let s = style::COMBO_SHADOW_OFFSET;
        push_rect(
            out,
            [s, y - s],
            half,
            &Paint::image(style::COMBO_SHADOW, uv),
        );
        for &(dx, c) in copies {
            push_rect(out, [dx, y], half, &Paint::image(c, uv));
        }
    }
    let cells = digit_cells(sheet, combo, scale);
    for &(dx, c) in copies {
        for &(x, uv, half) in &cells {
            let at = [x + dx, style::COMBO_DIGIT_Y];
            push_rect(out, at, half, &Paint::image(c, uv));
        }
    }
}

/// Centred row of digit cells for `n` at `scale`: `(centre x, uv, half extents)`; empty unless
/// every needed digit is loaded.
fn digit_cells(sheet: &SpriteSheet, n: u32, scale: f32) -> Vec<(f32, UvRect, [f32; 2])> {
    let cell_h = style::COMBO_DIGIT_H * (1.0 + 2.0 * DIGIT_PAD) * scale;
    let pad = style::COMBO_DIGIT_H * DIGIT_PAD * scale;
    let digits: Option<Vec<_>> = n
        .to_string()
        .bytes()
        .map(|b| {
            let s = Sprite::Digit(b - b'0');
            Some((sheet.get(s)?, cell_h / sheet.aspect(s)?))
        })
        .collect();
    let Some(digits) = digits else {
        return Vec::new();
    };
    // Cells overlap by their padding so the glyphs keep the font's advance.
    let total: f32 = digits.iter().map(|(_, w)| w - 2.0 * pad).sum::<f32>() + 2.0 * pad;
    let mut x = -total / 2.0;
    digits
        .into_iter()
        .map(|(uv, w)| {
            let cell = (x + w / 2.0, uv, [w / 2.0, cell_h / 2.0]);
            x += w - 2.0 * pad;
            cell
        })
        .collect()
}

/// What a note looks like: colour, kind (`None` = hold end) and bonus marking.
#[derive(Debug)]
struct NoteLook<'a> {
    color: style::Rgba,
    body: Option<&'a NoteBody>,
    bonus: Bonus,
}

fn push_note(
    out: &mut Vec<Vertex>,
    sheet: &SpriteSheet,
    now: f32,
    depth: f32,
    lane: u8,
    width: u8,
    look: &NoteLook,
) {
    let r = depth_to_radius(depth);
    let trim = if width < LANES {
        style::NOTE_TRIM_LANES
    } else {
        0.0
    };
    let (start, span) = (lane as f32 + trim, width as f32 - 2.0 * trim);
    let r_in = r * (1.0 - style::NOTE_THICKNESS);
    let arrows = (
        r_in,
        r * (1.0 - style::NOTE_THICKNESS * (1.0 + style::ARROW_BAND)),
    );
    match look.body {
        Some(NoteBody::Slide(dir)) => {
            push_slide_arrows(out, sheet, now, r, (start, span), look.color, *dir)
        }
        Some(NoteBody::Snap(dir)) => {
            push_snap_arrows(out, sheet, now, arrows, (start, span), look.color, *dir)
        }
        _ => {}
    }
    match sheet.get(Sprite::NoteFace) {
        Some(uv) => {
            let paint = Paint::mask(look.color, uv);
            for (off, len, s0, s1) in three_slice(span, style::NOTE_CAP_LANES, style::NOTE_CAP_TEX)
            {
                paint_sector(out, r, r_in, start + off, len, &paint.sub_arc(s0, s1));
            }
        }
        None => push_sector(out, r_in, r, start, span, look.color),
    }
    let cap = style::NOTE_END_CAP_LANES;
    if width < LANES && span > 3.0 * cap {
        for s in [start, start + span - cap] {
            push_sector(out, r_in, r, s, cap, style::NOTE_END_CAP);
        }
    }
    if matches!(look.body, Some(NoteBody::Chain))
        && let Some(uv) = sheet.get(Sprite::ChainStripes)
    {
        let paint = Paint::image(style::CHAIN_STRIPE, uv);
        for (off, len, s0, s1) in tiles(span, style::CHAIN_STRIPE_LANES, 0.0) {
            paint_sector(out, r, r_in, start + off, len, &paint.sub_arc(s0, s1));
        }
    }
    if let Some(accent) = style::accent_color(look.bonus) {
        push_sector(
            out,
            r,
            r * (1.0 + style::ACCENT_THICKNESS),
            start,
            span,
            accent,
        );
        if let Some(uv) = sheet.get(Sprite::Sparkle) {
            let size = r * style::NOTE_THICKNESS * 1.2;
            let tiles = tiles(span, even_tile(span, style::SPARKLE_LANES), 0.0);
            for (k, (off, len, ..)) in tiles.into_iter().enumerate() {
                let twinkle = 0.6 + 0.4 * (now * 7.0 + k as f32 * 1.9).sin();
                let c = [accent[0], accent[1], accent[2], twinkle];
                let at = polar((r + r_in) / 2.0, start + off + len / 2.0);
                push_rect(out, at, [size, size], &Paint::image(c, uv).additive());
            }
        }
    }
}

/// Separate chevrons inside a slide note at radius `r`, pointing and travelling in the slide
/// direction and growing from its back to its front (see `style::SLIDE_ARROW_*`).
fn push_slide_arrows(
    out: &mut Vec<Vertex>,
    sheet: &SpriteSheet,
    now: f32,
    r: f32,
    (start, span): (f32, f32),
    color: style::Rgba,
    dir: SlideDir,
) {
    let Some(uv) = sheet.get(Sprite::SlideArrow) else {
        return;
    };
    let rc = r * style::SLIDE_ARROW_RADIUS;
    let pitch = style::SLIDE_ARROW_LANES;
    for pos in slide_arrow_slots(span, now) {
        let f = pos / span;
        let size =
            r * (style::SLIDE_ARROW_MIN + (style::SLIDE_ARROW_MAX - style::SLIDE_ARROW_MIN) * f);
        // Fade in at the back, out at the front edge.
        let alpha = (pos / pitch)
            .min((span - pos) / (pitch * 0.5))
            .clamp(0.0, 1.0);
        // Counter-clockwise slides run towards higher lanes.
        let (lane, sign) = match dir {
            SlideDir::Ccw => (start + pos, 1.0),
            SlideDir::Cw => (start + span - pos, -1.0),
        };
        let a = crate::layout::lane_angle(lane);
        let tangent = [-a.sin() * sign, a.cos() * sign];
        let paint = Paint::mask([color[0], color[1], color[2], alpha], uv);
        push_oriented_rect(
            out,
            polar(rc, lane),
            [size / 2.0, size * 0.6],
            tangent,
            &paint,
        );
    }
}

/// Double chevrons pointing (and drifting) inwards or outwards across the band `arrows`.
fn push_snap_arrows(
    out: &mut Vec<Vertex>,
    sheet: &SpriteSheet,
    now: f32,
    (r_outer, r_inner): (f32, f32),
    (start, span): (f32, f32),
    color: style::Rgba,
    dir: SnapDir,
) {
    let Some(uv) = sheet.get(Sprite::SnapArrow) else {
        return;
    };
    let (shift, alpha) = snap_arrow_anim(now);
    let d = (r_outer - r_inner) * 0.35 * shift;
    // The chevrons point towards −u, i.e. towards the band edge at t = 0.
    let (r_a, r_b) = match dir {
        SnapDir::In => (r_inner - d, r_outer - d),
        SnapDir::Out => (r_outer + d, r_inner + d),
    };
    let paint = Paint::mask([color[0], color[1], color[2], alpha], uv).along_v();
    for (off, len, s0, s1) in tiles(span, even_tile(span, style::SNAP_ARROW_LANES), 0.0) {
        paint_sector(out, r_a, r_b, start + off, len, &paint.sub_arc(s0, s1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{APPROACH_SECS, lane_angle, tiles};
    use wacca_chart::{Arc, BeatPos, ChartHeader, HoldNode, ReverseSection};

    fn chart(notes: Vec<Note>) -> Chart {
        Chart {
            header: ChartHeader::default(),
            notes,
            masks: vec![],
            tempo: vec![],
            scroll: vec![],
            reverses: vec![],
            tutorial_tags: vec![],
            end: BeatPos::default(),
            end_time: 10.0,
        }
    }

    fn note(time: f64, body: NoteBody, bonus: Bonus) -> Note {
        Note {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane: 10, width: 4 },
            bonus,
            body,
            file_index: 0,
        }
    }

    fn count(v: &[Vertex], color: style::Rgba) -> usize {
        v.iter().filter(|v| v.color == color).count()
    }

    /// Every sprite at a distinct fake atlas rectangle.
    fn full_sheet() -> SpriteSheet {
        let mut s = SpriteSheet::default();
        let digits = (0..10).map(Sprite::Digit);
        for (i, sprite) in Sprite::ALL.into_iter().chain(digits).enumerate() {
            let u = i as f32 / 32.0;
            s.insert(sprite, [u, 0.5, u + 0.02, 0.75], [256, 64]);
        }
        s
    }

    fn uv_range(v: &[Vertex], uv: crate::layout::UvRect) -> usize {
        let inside = |x: f32, a: f32, b: f32| x >= a.min(b) - 1e-6 && x <= a.max(b) + 1e-6;
        v.iter()
            .filter(|p| inside(p.uv[0], uv[0], uv[2]) && inside(p.uv[1], uv[1], uv[3]))
            .count()
    }

    #[test]
    fn empty_chart_has_only_the_rim() {
        let none = SpriteSheet::default();
        let v = build_scene(
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &LiveState::default(),
            &none,
        );
        // Lanes and the judgement ring are 3D (stage); the dark rim goes all the way round.
        assert_eq!(count(&v, style::RIM), 60 * 6);
        assert_eq!(count(&v, style::OUTSIDE), 60 * 6);
        assert_eq!(v.len(), 3 * 60 * 6, "outside, rim, rim edge");
        // Into a reused buffer: cleared first, same triangles, no reallocation.
        let mut reused = v.clone();
        let capacity = reused.capacity();
        build_scene_into(
            &mut reused,
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &LiveState::default(),
            &none,
        );
        assert_eq!((reused == v, reused.capacity()), (true, capacity));
    }

    #[test]
    fn rim_row_progress_and_gauge() {
        let mut sheet = full_sheet();
        for t in [
            TextSprite::ScoreLabel,
            TextSprite::Difficulty,
            TextSprite::Title,
        ] {
            sheet.insert(Sprite::Text(t), [0.9, 0.0, 0.95, 0.1], [400, 40]);
        }
        let live = |progress| LiveState {
            rim: Some(RimState {
                score: 82_759,
                progress,
                difficulty_color: [0.1, 0.4, 1.0, 1.0],
            }),
            gauge: Some((0.5, 0.7)),
            ..LiveState::default()
        };
        let v = build_scene(
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &live(0.25),
            &sheet,
        );
        // Progress: a quarter of the ring from 12 o'clock counter-clockwise (to 9 o'clock).
        let lit: Vec<_> = v.iter().filter(|p| p.color == style::PROGRESS).collect();
        assert_eq!(lit.len(), 15 * 6);
        assert!(lit.iter().all(|p| p.pos[0] <= 1e-4 && p.pos[1] >= -1e-4));
        // Seven score digits "0082759", all at the top, orange.
        let digits: Vec<_> = v.iter().filter(|p| p.color == style::SCORE_COLOR).collect();
        assert!(digits.len() >= 7 * 6);
        assert!(
            digits
                .iter()
                .all(|p| p.pos[1] > 0.85 && p.pos[0].abs() < 0.45)
        );
        let zero = sheet.get(Sprite::Digit(0)).unwrap();
        let zeros = digits
            .iter()
            .filter(|p| p.uv[0] >= zero[0] && p.uv[0] <= zero[2]);
        assert!(zeros.count() >= 2 * 6);
        // Texts: SCORE left of the top, difficulty and title right of it.
        let cx = |c: style::Rgba| {
            let p: Vec<_> = v.iter().filter(|p| p.color == c).collect();
            p.iter().map(|p| p.pos[0]).sum::<f32>() / p.len() as f32
        };
        assert!(cx(style::SCORE_LABEL_COLOR) < 0.0);
        assert!(cx([0.1, 0.4, 1.0, 1.0]) > 0.0);
        assert!(cx(style::TITLE_COLOR) > cx([0.1, 0.4, 1.0, 1.0]));
        // Gauge: track, half fill (and its core), three marks.
        assert_eq!(count(&v, style::GAUGE_TRACK), 6);
        let fill: Vec<_> = v.iter().filter(|p| p.color == style::GAUGE_FILL).collect();
        assert_eq!(fill.len(), 6);
        assert!(fill.iter().all(|p| p.pos[0] <= 1e-5));
        assert_eq!(count(&v, style::GAUGE_MARK), 3 * 6);
        let none = build_scene(
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &live(0.0),
            &sheet,
        );
        assert_eq!(count(&none, style::PROGRESS), 0);
    }

    #[test]
    fn touch_guide_draws_rings_and_pressed_sensors() {
        let mut pressed = [0u64; style::SENSOR_DEPTHS];
        pressed[1] = 1 << 7 | 1 << 8;
        let live = LiveState {
            pressed,
            ..LiveState::default()
        };
        let on = SceneOptions {
            touch_guide: true,
            ..Default::default()
        };
        let none = SpriteSheet::default();
        let v = build_scene(&chart(vec![]), 0.0, &on, &live, &none);
        assert_eq!(count(&v, style::TOUCH_GUIDE_PRESSED), 2 * 6);
        assert_eq!(
            count(&v, style::TOUCH_GUIDE_RING),
            (style::SENSOR_DEPTHS + 1) * 60 * 6
        );
        let off = build_scene(&chart(vec![]), 0.0, &SceneOptions::default(), &live, &none);
        assert_eq!(count(&off, style::TOUCH_GUIDE_PRESSED), 0);
        assert_eq!(count(&off, style::TOUCH_GUIDE_RING), 0);
    }

    #[test]
    fn bar_lines_at_visible_measures() {
        // Default tempo: 120 BPM 4/4, 2 s per measure; at note speed 1 the window is 3.3 s.
        let on = SceneOptions {
            bar_lines: true,
            ..Default::default()
        };
        let none = SpriteSheet::default();
        let v = build_scene(&chart(vec![]), 1.0, &on, &LiveState::default(), &none);
        // Measures at 2 s and 4 s are ahead; 0 s has passed.
        assert_eq!(count(&v, style::BAR_LINE), 2 * 60 * 6);
        let off = SceneOptions::default();
        let v = build_scene(&chart(vec![]), 1.0, &off, &LiveState::default(), &none);
        assert_eq!(count(&v, style::BAR_LINE), 0, "an option, off by default");
        let mut c = chart(vec![]);
        c.end_time = 3.0;
        let v = build_scene(&c, 1.0, &on, &LiveState::default(), &none);
        assert_eq!(
            count(&v, style::BAR_LINE),
            60 * 6,
            "none after the chart ends"
        );
    }

    #[test]
    fn only_visible_notes_drawn_with_accent() {
        let c = chart(vec![
            note(1.0, NoteBody::Touch, Bonus::R),
            note(1.0 + APPROACH_SECS + 1.0, NoteBody::Chain, Bonus::None),
        ]);
        let v = build_scene(
            &c,
            0.5,
            &SceneOptions::default(),
            &LiveState::default(),
            &SpriteSheet::default(),
        );
        assert_eq!(count(&v, style::TOUCH), 4 * 6);
        assert_eq!(count(&v, style::R_ACCENT), 4 * 6);
        assert_eq!(count(&v, style::CHAIN), 0);
        // Faster note speed shrinks the window further; the touch note is still inside.
        let v = build_scene(
            &c,
            0.5,
            &SceneOptions {
                note_speed: 6.0,
                ..Default::default()
            },
            &LiveState::default(),
            &SpriteSheet::default(),
        );
        assert_eq!(count(&v, style::TOUCH), 4 * 6);
    }

    #[test]
    fn hold_draws_head_and_end() {
        let n = |time, lane: u8| HoldNode {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width: 4 },
            lane_unwrapped: lane as f32,
            control: true,
        };
        let hold = NoteBody::Hold {
            nodes: vec![n(1.0, 10), n(2.0, 12)],
        };
        let v = build_scene(
            &chart(vec![note(1.0, hold, Bonus::None)]),
            0.5,
            &SceneOptions::default(),
            &LiveState::default(),
            &SpriteSheet::default(),
        );
        assert_eq!(count(&v, style::HOLD), 2 * 4 * 6);
    }

    #[test]
    fn reverse_hides_notes_after_note_end() {
        let mut c = chart(vec![
            note(13.0, NoteBody::Touch, Bonus::None),
            note(15.0, NoteBody::Chain, Bonus::None),
        ]);
        c.reverses = vec![ReverseSection {
            start: 10.0,
            end: 12.0,
            note_end: 14.0,
        }];
        let v = build_scene(
            &c,
            11.0,
            &SceneOptions::default(),
            &LiveState::default(),
            &SpriteSheet::default(),
        );
        assert!(count(&v, style::TOUCH) > 0);
        assert_eq!(count(&v, style::CHAIN), 0);
    }

    #[test]
    fn judged_notes_vanish_and_notes_get_end_caps() {
        let c = chart(vec![
            note(1.0, NoteBody::Touch, Bonus::None),
            note(1.2, NoteBody::Chain, Bonus::None),
        ]);
        let live = LiveState {
            judged: &[true],
            ..LiveState::default()
        };
        let v = build_scene(
            &c,
            0.5,
            &SceneOptions::default(),
            &live,
            &SpriteSheet::default(),
        );
        assert_eq!(count(&v, style::TOUCH), 0);
        assert_eq!(count(&v, style::CHAIN), 4 * 6);
        assert_eq!(count(&v, style::NOTE_END_CAP), 2 * 6);
    }

    #[test]
    fn textured_notes_use_their_sprites() {
        let sheet = full_sheet();
        let c = chart(vec![
            note(1.0, NoteBody::Slide(SlideDir::Cw), Bonus::None),
            note(1.0, NoteBody::Snap(SnapDir::Out), Bonus::None),
            note(1.0, NoteBody::Chain, Bonus::Bonus),
        ]);
        let v = build_scene(
            &c,
            0.6,
            &SceneOptions::default(),
            &LiveState::default(),
            &sheet,
        );
        let face = sheet.get(Sprite::NoteFace).unwrap();
        // Three notes, each a three-slice face; masks flagged.
        assert!(uv_range(&v, face) >= 3 * 3 * 6);
        assert!(v.iter().filter(|p| p.fx[0] == 1.0).count() >= 3 * 3 * 6);
        for s in [
            Sprite::SlideArrow,
            Sprite::SnapArrow,
            Sprite::ChainStripes,
            Sprite::Sparkle,
        ] {
            assert!(uv_range(&v, sheet.get(s).unwrap()) > 0, "{s:?} missing");
        }
        // Sparkles are additive.
        assert!(v.iter().any(|p| p.fx[1] == 1.0));
    }

    #[test]
    fn effects_and_judgement_sprites() {
        let sheet = full_sheet();
        let mut fx = Effects::default();
        fx.hit(5, 3, JudgeSprite::Great);
        let live = LiveState {
            effects: Some(&fx),
            judgement: Some((JudgeSprite::Marvelous, 0.1)),
            combo: (3, 1.0),
            ..LiveState::default()
        };
        let v = build_scene(&chart(vec![]), 0.0, &SceneOptions::default(), &live, &sheet);
        let mut alone = Vec::new();
        fx.draw(&mut alone, &sheet);
        let without = build_scene(
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &LiveState {
                effects: None,
                ..live
            },
            &sheet,
        );
        assert_eq!(v.len(), without.len() + alone.len());
        let judge = sheet.get(Sprite::Judge(JudgeSprite::Marvelous)).unwrap();
        let quad: Vec<_> = v
            .iter()
            .filter(|p| p.uv[0] >= judge[0] && p.uv[0] <= judge[2])
            .collect();
        assert_eq!(quad.len(), 6);
        // Centred below the middle, 256x64 aspect.
        let ys: Vec<f32> = quad.iter().map(|p| p.pos[1]).collect();
        let xs: Vec<f32> = quad.iter().map(|p| p.pos[0]).collect();
        let (w, h) = (
            xs.iter().cloned().fold(f32::MIN, f32::max)
                - xs.iter().cloned().fold(f32::MAX, f32::min),
            ys.iter().cloned().fold(f32::MIN, f32::max)
                - ys.iter().cloned().fold(f32::MAX, f32::min),
        );
        assert!((w / h - 4.0).abs() < 1e-3);
        assert!(ys.iter().all(|y| *y < 0.0));
        assert!((w - style::JUDGE_SPRITE_W).abs() < 1e-5);
        // COMBO word and its shadow; the digit (3) above it in the main tint.
        assert_eq!(uv_range(&v, sheet.get(Sprite::Combo).unwrap()), 2 * 6);
        let three = sheet.get(Sprite::Digit(3)).unwrap();
        let digit: Vec<_> = v
            .iter()
            .filter(|p| p.uv[0] >= three[0] && p.uv[0] <= three[2])
            .collect();
        assert_eq!(digit.len(), 6);
        assert!(
            digit
                .iter()
                .all(|p| p.pos[1] > 0.1 && p.color == style::COMBO_TINT)
        );
        // Right after a change the count shows two coloured copies more.
        let fresh = LiveState {
            combo: (3, 0.0),
            ..live
        };
        let v = build_scene(
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &fresh,
            &sheet,
        );
        assert_eq!(uv_range(&v, three), 3 * 6);
    }

    #[test]
    fn digit_rows_are_centred_and_need_every_digit() {
        let sheet = full_sheet();
        let cells = digit_cells(&sheet, 120, 1.0);
        assert_eq!(cells.len(), 3);
        let left = cells[0].0 - cells[0].2[0];
        let right = cells[2].0 + cells[2].2[0];
        assert!((left + right).abs() < 1e-5, "centred: {left} {right}");
        assert!(cells[1].0.abs() < 1e-5);
        let big = digit_cells(&sheet, 120, 2.0);
        assert!((big[0].2[1] - 2.0 * cells[0].2[1]).abs() < 1e-6);
        let mut partial = SpriteSheet::default();
        partial.insert(Sprite::Digit(1), [0.0; 4], [10, 10]);
        assert!(digit_cells(&partial, 12, 1.0).is_empty());
    }

    #[test]
    fn even_tiles_fill_the_arc() {
        assert_eq!(even_tile(9.0, 3.0), 3.0);
        assert_eq!(even_tile(10.0, 3.0), 10.0 / 3.0);
        assert_eq!(even_tile(1.0, 3.0), 1.0);
        let t = tiles(10.0, even_tile(10.0, 3.0), 0.0);
        assert!(t.iter().all(|p| (p.3 - p.2 - 1.0).abs() < 1e-5), "{t:?}");
    }

    #[test]
    fn animation_curves() {
        let life = style::JUDGE_SPRITE_SECS;
        assert!((judge_sprite_alpha(0.0).unwrap() - 0.4).abs() < 1e-6);
        assert_eq!(judge_sprite_alpha(life / 2.0), Some(1.0));
        let late = judge_sprite_alpha(life - style::JUDGE_FADE_SECS / 2.0).unwrap();
        assert!((late - 0.5).abs() < 1e-3);
        assert!(judge_sprite_alpha(life).is_none() && judge_sprite_alpha(-0.01).is_none());

        let (s0, d0) = combo_anim(0.0);
        assert!((s0 - style::COMBO_POP).abs() < 1e-6 && (d0 - style::COMBO_SPLIT).abs() < 1e-6);
        assert_eq!(combo_anim(style::COMBO_SPLIT_SECS), (1.0, 0.0));
        let (s, d) = combo_anim(0.05);
        assert!(s > 1.0 && s < s0 && d > 0.0 && d < d0);

        let (shift, alpha) = snap_arrow_anim(style::SNAP_ARROW_PERIOD * 1.5);
        assert!((shift - 0.5).abs() < 1e-5 && (alpha - 1.0).abs() < 1e-5);
        assert!(snap_arrow_anim(0.0).1.abs() < 1e-6);

        // Slide chevrons sit one pitch apart inside the note and travel forwards.
        let a = slide_arrow_slots(10.0, 0.0);
        assert_eq!(a.len(), (10.0 / style::SLIDE_ARROW_LANES).ceil() as usize);
        assert!(
            a.windows(2)
                .all(|w| (w[1] - w[0] - style::SLIDE_ARROW_LANES).abs() < 1e-5)
        );
        let b = slide_arrow_slots(10.0, 0.01);
        assert!(b[0] > a[0] && b.iter().all(|p| (0.0..10.0).contains(p)));
    }

    #[test]
    fn slide_chevrons_grow_towards_the_slide_direction() {
        let sheet = full_sheet();
        let arrow = sheet.get(Sprite::SlideArrow).unwrap();
        // A slide over lanes 10..20 at the line: `(angle, size)` of each chevron, by angle.
        let chevrons = |dir| {
            let mut v = Vec::new();
            let body = NoteBody::Slide(dir);
            let look = NoteLook {
                color: style::SLIDE_CW,
                body: Some(&body),
                bonus: Bonus::None,
            };
            push_note(&mut v, &sheet, 0.3, 0.0, 10, 10, &look);
            let arrows: Vec<Vertex> = v
                .into_iter()
                .filter(|p| uv_range(std::slice::from_ref(p), arrow) == 1)
                .collect();
            let mut out: Vec<(f32, f32)> = arrows
                .chunks(6)
                .map(|q| {
                    let c = [0, 1].map(|k| q.iter().map(|p| p.pos[k]).sum::<f32>() / 6.0);
                    let ext = q
                        .iter()
                        .map(|p| (p.pos[0] - c[0]).hypot(p.pos[1] - c[1]))
                        .fold(0.0, f32::max);
                    (c[1].atan2(c[0]), ext)
                })
                .collect();
            out.sort_by(|a, b| a.0.total_cmp(&b.0));
            out
        };
        let ccw = chevrons(SlideDir::Ccw);
        assert!(ccw.len() >= 4);
        assert!(ccw[0].1 < ccw[ccw.len() - 1].1, "{ccw:?}");
        let cw = chevrons(SlideDir::Cw);
        assert!(cw[0].1 > cw[cw.len() - 1].1, "{cw:?}");
        let (a0, a1) = (lane_angle(10.0), lane_angle(20.0));
        assert!(ccw.iter().all(|&(a, _)| a > a0 && a < a1), "{ccw:?}");
    }
}
