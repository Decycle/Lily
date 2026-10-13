//! Builds the playfield vertex list for a chart at a given time (pure, no GPU).
//!
//! With an empty [`SpriteSheet`] everything is drawn in flat colours; each loaded sprite switches
//! its element to the original texture.

use std::f32::consts::PI;

use wacca_chart::{BeatPos, Bonus, Chart, LANES, Note, NoteBody, SlideDir, SnapDir};

use crate::effects::Effects;
use crate::layout::{
    Paint, UvRect, Vertex, depth_to_radius, last_drawable_time, mask_state, paint_sector, polar,
    push_hold_body, push_radial_line, push_rect, push_sector, raw_depth, three_slice, tiles,
};
use crate::sprites::{DIGIT_PAD, JudgeSprite, Sprite, SpriteSheet};
use crate::style;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneOptions {
    /// Player note-speed option (1.0..=6.0).
    pub note_speed: f64,
    /// A background movie plays: no guide sectors over it, only faint lane lines.
    pub movie: bool,
    /// Measure lines (`BarLineTable` option; off for a guest, the E2 frames show none).
    pub bar_lines: bool,
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
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            note_speed: 1.0,
            movie: false,
            bar_lines: false,
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

/// Slide chevron tile phase at `now` (moves towards higher lanes for counter-clockwise).
pub fn slide_arrow_phase(now: f32, dir: SlideDir) -> f32 {
    let p = now * style::SLIDE_ARROW_SPEED / style::SLIDE_ARROW_LANES;
    match dir {
        SlideDir::Ccw => -p,
        SlideDir::Cw => p,
    }
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
    let lanes = LANES as usize;

    let mask = mask_state(&chart.masks, now);
    // Over a movie the real game draws no guide sectors, only faint lane lines (E2 frames).
    let guide = sheet.get(Sprite::GuideLane);
    for lane in (0..lanes).filter(|&l| mask[l] && !opts.movie) {
        match guide {
            Some(uv) => {
                let paint = Paint::image(style::GUIDE_LANE_TINT, uv);
                paint_sector(&mut out, 1.0, 0.0, lane as f32, 1.0, &paint);
            }
            None => push_sector(&mut out, 0.0, 1.0, lane as f32, 1.0, style::GUIDE_LANE),
        }
    }
    let line = if opts.movie {
        style::LANE_LINE_MOVIE
    } else {
        style::LANE_LINE
    };
    for lane in (0..lanes).filter(|&l| mask[l] || mask[(l + lanes - 1) % lanes]) {
        let w = style::LANE_LINE_HALF_WIDTH;
        push_radial_line(&mut out, lane as f32, 0.0, 1.0, w, line);
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

    let hold_paint = match sheet.get(Sprite::HoldBody) {
        // One row of the cross-section texture: u runs across the hold's width.
        Some([u0, v0, u1, v1]) => {
            let v = (v0 + v1) / 2.0;
            Paint::image(style::HOLD_BODY, [u0, v, u1, v])
        }
        None => Paint::flat(style::HOLD_BODY),
    };
    for (_, note) in chart.notes.iter().enumerate().filter(drawable) {
        if let NoteBody::Hold { nodes } = &note.body {
            push_hold_body(&mut out, nodes, depth_of, &hold_paint);
        }
    }
    if opts.bar_lines {
        push_bar_lines(&mut out, chart, now, depth_of);
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
            push_note(&mut out, sheet, t, d, end.arc.lane, end.arc.width, &look);
        }
        if let Some(d) = visible(note.time) {
            let look = NoteLook {
                color: style::note_color(&note.body),
                body: Some(&note.body),
                bonus: note.bonus,
            };
            push_note(&mut out, sheet, t, d, note.arc.lane, note.arc.width, &look);
        }
    }

    push_judge_ring(&mut out, sheet, &mask);
    if let Some(fx) = live.effects {
        fx.draw(&mut out, sheet);
    }
    push_hud_sprites(&mut out, sheet, live);
    out
}

/// Judgement ring colour at `lane`'s start edge: hot pink on the sides, turning purple towards
/// the bottom and (less) the top (E2 frames).
// UNVERIFIED: the game likely samples vT_JudgeColor01 by angle; this is an eyeballed gradient.
pub fn judge_ring_color(lane: u8) -> style::Rgba {
    let s = crate::layout::lane_angle(f32::from(lane)).sin();
    let t = s.abs().powf(1.5)
        * if s > 0.0 {
            style::JUDGE_RING_TOP_PURPLE
        } else {
            1.0
        };
    let (a, b) = (style::JUDGE_RING_SIDE, style::JUDGE_RING_PURPLE);
    [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// On every lane that is on: the thick glowing band from the line outwards (E2: 292–317 px at
/// a 291.8 px line), soft additive glow on both of its sides, then a dark rim with a thin light
/// edge.
fn push_judge_ring(out: &mut Vec<Vertex>, sheet: &SpriteSheet, mask: &[bool]) {
    let band_out = 1.0 + style::JUDGE_BAND_WIDTH;
    let rim_out = band_out + style::RIM_WIDTH;
    // Nothing shows outside the cabinet rim (the movie quad would, E2 frames are black there).
    let corner = style::VIEW_EXTENT * std::f32::consts::SQRT_2;
    push_sector(out, rim_out, corner, 0.0, LANES as f32, style::OUTSIDE);
    let glow = sheet.get(Sprite::GuideLane).map(|[u0, v0, u1, v1]| {
        // A single column of the gradient (bright at v0), stretched along the ring.
        let u = (u0 + u1) / 2.0;
        [u, v0, u, v1]
    });
    for lane in (0..LANES).filter(|&l| mask[usize::from(l)]) {
        let l = f32::from(lane);
        let color = judge_ring_color(lane);
        push_sector(out, band_out, rim_out, l, 1.0, style::RIM);
        let e = style::RIM_EDGE_HALF_WIDTH;
        push_sector(out, rim_out - e, rim_out + e, l, 1.0, style::RIM_EDGE);
        push_sector(out, 1.0, band_out, l, 1.0, color);
        if let Some(uv) = glow {
            let paint = Paint::image([color[0], color[1], color[2], style::JUDGE_GLOW_ALPHA], uv)
                .additive();
            let w = style::JUDGE_GLOW_WIDTH;
            paint_sector(out, 1.0, 1.0 - w, l, 1.0, &paint);
            // Hot core in the middle of the band.
            let mid = 1.0 + style::JUDGE_BAND_WIDTH / 2.0;
            paint_sector(out, mid, 1.0, l, 1.0, &paint);
            paint_sector(out, mid, band_out, l, 1.0, &paint);
        }
    }
}

/// Thin white circles at measure starts, travelling like notes (vMsh_BarLine00_60_s).
fn push_bar_lines(out: &mut Vec<Vertex>, chart: &Chart, now: f64, depth_of: impl Fn(f64) -> f64) {
    let ticks = BeatPos {
        measure: 1,
        tick: 0,
    }
    .abs_tick() as f64;
    let first = (chart.sec_to_tick(chart.view_time(now)) / ticks)
        .floor()
        .max(0.0) as u32;
    for measure in first..first + style::BAR_LINE_MAX {
        let t = chart.tick_to_sec(BeatPos { measure, tick: 0 }.abs_tick());
        if t > chart.end_time {
            break;
        }
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
            push_slide_arrows(out, sheet, now, arrows, (start, span), look.color, *dir)
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

/// Chevrons scrolling along the band `arrows = (outer, inner radius)` in the slide direction.
fn push_slide_arrows(
    out: &mut Vec<Vertex>,
    sheet: &SpriteSheet,
    now: f32,
    (r_a, r_b): (f32, f32),
    (start, span): (f32, f32),
    color: style::Rgba,
    dir: SlideDir,
) {
    let Some([u0, v0, u1, v1]) = sheet.get(Sprite::SlideArrow) else {
        return;
    };
    // The chevron points towards +v; flip it to point clockwise (towards lower lanes).
    let uv = match dir {
        SlideDir::Ccw => [u0, v0, u1, v1],
        SlideDir::Cw => [u0, v1, u1, v0],
    };
    let paint = Paint::mask(color, uv).along_v();
    let phase = slide_arrow_phase(now, dir);
    for (off, len, s0, s1) in tiles(span, style::SLIDE_ARROW_LANES, phase) {
        paint_sector(out, r_a, r_b, start + off, len, &paint.sub_arc(s0, s1));
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
    use crate::layout::{APPROACH_SECS, tiles};
    use wacca_chart::{Arc, BeatPos, ChartHeader, HoldNode, MaskDir, MaskEvent, ReverseSection};

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
    fn empty_chart_has_all_lanes_and_ring() {
        let v = build_scene(
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &LiveState::default(),
            &SpriteSheet::default(),
        );
        assert_eq!(count(&v, style::GUIDE_LANE), 60 * 6);
        assert_eq!(count(&v, style::LANE_LINE), 60 * 6);
        // Lane 15 (top) has a ring colour of its own; the rim goes all the way round.
        assert_eq!(count(&v, judge_ring_color(15)), 6);
        assert_eq!(count(&v, style::RIM), 60 * 6);
        assert_eq!(v.len() % 3, 0);
        let movie = SceneOptions {
            movie: true,
            ..Default::default()
        };
        let none = SpriteSheet::default();
        // Over a movie: no guide sectors (flat or textured), faint lane lines.
        for sheet in [&none, &full_sheet()] {
            let v = build_scene(&chart(vec![]), 0.0, &movie, &LiveState::default(), sheet);
            assert_eq!(count(&v, style::GUIDE_LANE), 0);
            assert_eq!(count(&v, style::GUIDE_LANE_TINT), 0);
            assert_eq!(count(&v, style::LANE_LINE_MOVIE), 60 * 6);
        }
    }

    #[test]
    fn ring_colour_is_pink_on_the_sides_and_purple_at_the_bottom() {
        let near =
            |a: style::Rgba, b: style::Rgba| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        assert!(near(judge_ring_color(45), style::JUDGE_RING_PURPLE));
        assert!(near(judge_ring_color(0), style::JUDGE_RING_SIDE));
        assert!(near(judge_ring_color(30), style::JUDGE_RING_SIDE));
        let top = judge_ring_color(15);
        assert!(top[2] > style::JUDGE_RING_SIDE[2] && top[2] < style::JUDGE_RING_PURPLE[2]);
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
    fn masked_lanes_are_not_drawn() {
        let mut c = chart(vec![]);
        c.masks = vec![MaskEvent {
            pos: BeatPos::default(),
            time: 0.0,
            arc: Arc { lane: 0, width: 10 },
            add: true,
            dir: MaskDir::Plus,
        }];
        let v = build_scene(
            &c,
            5.0,
            &SceneOptions::default(),
            &LiveState::default(),
            &SpriteSheet::default(),
        );
        assert_eq!(count(&v, style::GUIDE_LANE), 10 * 6);
        assert_eq!(count(&v, style::RIM), 10 * 6);
        assert_eq!(count(&v, style::LANE_LINE), 11 * 6);
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
    fn hold_draws_body_head_and_end() {
        let n = |time, lane: u8| HoldNode {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width: 4 },
            lane_unwrapped: lane as f32,
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
        assert!(count(&v, style::HOLD_BODY) > 0);
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
        // Lanes use the guide texture, and the flat guide colour is gone.
        assert_eq!(count(&v, style::GUIDE_LANE), 0);
        assert_eq!(count(&v, style::GUIDE_LANE_TINT), 60 * 6);
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

        // Tile boundaries move towards higher lanes for counter-clockwise slides, lower for clockwise.
        let first = |t, dir| tiles(10.0, style::SLIDE_ARROW_LANES, slide_arrow_phase(t, dir))[0].1;
        assert!(first(0.02, SlideDir::Ccw) > first(0.01, SlideDir::Ccw));
        assert!(first(0.02, SlideDir::Cw) < first(0.01, SlideDir::Cw));
    }
}
