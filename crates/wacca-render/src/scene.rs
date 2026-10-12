//! Builds the playfield vertex list for a chart at a given time (pure, no GPU).
//!
//! With an empty [`SpriteSheet`] everything is drawn in flat colours; each loaded sprite switches
//! its element to the original texture.

use std::f32::consts::PI;

use wacca_chart::{Bonus, Chart, LANES, Note, NoteBody, SlideDir, SnapDir};

use crate::layout::{
    Paint, Vertex, depth_to_radius, last_drawable_time, mask_state, paint_sector, polar,
    push_hold_body, push_radial_line, push_rect, push_sector, raw_depth, sensor_ring, three_slice,
    tiles,
};
use crate::sprites::{JudgeSprite, Sprite, SpriteSheet};
use crate::style::{self, SENSOR_DEPTHS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneOptions {
    /// Player note-speed option (1.0..=6.0).
    pub note_speed: f64,
}

/// A judgement at the line, drawn as a short additive flash over the note's lanes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flash {
    pub lane: u8,
    pub width: u8,
    pub kind: JudgeSprite,
    /// Seconds since the judgement.
    pub age: f32,
}

/// Live play state drawn on top of the chart.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveState<'a> {
    /// `judged[i]`: note `i` (index into `Chart::notes`) is decided and no longer drawn.
    /// Shorter than the note list = the rest are not judged.
    pub judged: &'a [bool],
    /// Pressed sensors: `touched[depth]` bit `lane` (as `wacca_game::input::SensorState`).
    pub touched: [u64; SENSOR_DEPTHS],
    /// Recent judgements; flashes older than [`style::FLASH_SECS`] are skipped.
    pub flashes: &'a [Flash],
    /// Last judgement and its age in seconds (judgement word art).
    pub judgement: Option<(JudgeSprite, f32)>,
    /// Current combo (the COMBO word shows while it is non-zero).
    pub combo: u32,
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self { note_speed: 1.0 }
    }
}

/// Hit-flash opacity `age` seconds after the judgement (quadratic fade, 0 outside).
pub fn flash_alpha(age: f32) -> f32 {
    if (0.0..style::FLASH_SECS).contains(&age) {
        (1.0 - age / style::FLASH_SECS).powi(2)
    } else {
        0.0
    }
}

/// Judgement sprite `(scale, alpha)` `age` seconds after the judgement: pops in from 1.25×,
/// holds, fades over the last 40 %; `None` once gone.
pub fn judge_sprite_anim(age: f32) -> Option<(f32, f32)> {
    let life = style::JUDGE_SPRITE_SECS;
    if !(0.0..life).contains(&age) {
        return None;
    }
    let scale = 1.0 + 0.25 * (1.0 - age / style::JUDGE_POP_SECS).max(0.0);
    let fade_from = 0.6 * life;
    let alpha = if age < fade_from {
        1.0
    } else {
        1.0 - (age - fade_from) / (life - fade_from)
    };
    Some((scale, alpha))
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
    let guide = sheet.get(Sprite::GuideLane);
    for lane in (0..lanes).filter(|&l| mask[l]) {
        match guide {
            Some(uv) => {
                let paint = Paint::image(style::GUIDE_LANE_TINT, uv);
                paint_sector(&mut out, 1.0, 0.0, lane as f32, 1.0, &paint);
            }
            None => push_sector(&mut out, 0.0, 1.0, lane as f32, 1.0, style::GUIDE_LANE),
        }
    }
    for lane in (0..lanes).filter(|&l| mask[l] || mask[(l + lanes - 1) % lanes]) {
        push_radial_line(
            &mut out,
            lane as f32,
            0.0,
            1.0,
            style::LANE_LINE_HALF_WIDTH,
            style::LANE_LINE,
        );
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

    push_judge_ring(&mut out, sheet);
    push_flashes(&mut out, sheet, live.flashes);
    push_sensor_guides(&mut out, live.touched);
    push_hud_sprites(&mut out, sheet, live);
    out
}

/// Dark rim with ticks outside the line, the bright judgement ring and a glow inside it.
fn push_judge_ring(out: &mut Vec<Vertex>, sheet: &SpriteSheet) {
    let ring_out = 1.0 + style::JUDGE_RING_WIDTH;
    let rim_out = 1.0 + style::RIM_WIDTH;
    push_sector(out, ring_out, rim_out, 0.0, LANES as f32, style::RIM);
    for lane in (0..LANES).step_by(style::RIM_TICK_LANES.into()) {
        push_radial_line(
            out,
            f32::from(lane),
            ring_out,
            rim_out,
            style::LANE_LINE_HALF_WIDTH * 1.5,
            style::RIM_TICK,
        );
    }
    if let Some([u0, v0, u1, v1]) = sheet.get(Sprite::GuideLane) {
        // A single column of the gradient, stretched around the ring.
        let u = (u0 + u1) / 2.0;
        let paint = Paint::image(style::JUDGE_GLOW, [u, v0, u, v1]).additive();
        let r_in = 1.0 - style::JUDGE_GLOW_WIDTH;
        paint_sector(out, 1.0, r_in, 0.0, LANES as f32, &paint);
    }
    push_sector(out, 1.0, ring_out, 0.0, LANES as f32, style::JUDGE_RING);
}

fn push_flashes(out: &mut Vec<Vertex>, sheet: &SpriteSheet, flashes: &[Flash]) {
    let glow = sheet.get(Sprite::Glow);
    for f in flashes {
        let mut color = style::flash_color(f.kind);
        color[3] *= flash_alpha(f.age);
        if color[3] <= 0.0 {
            continue;
        }
        let paint = match glow {
            Some(uv) => Paint::image(color, uv),
            None => Paint::flat(color),
        }
        .additive();
        let (lane, width) = (f32::from(f.lane), f32::from(f.width));
        paint_sector(out, style::FLASH_OUT, style::FLASH_IN, lane, width, &paint);
    }
}

/// Judgement word art and the COMBO word at the centre (only with their sprites loaded).
fn push_hud_sprites(out: &mut Vec<Vertex>, sheet: &SpriteSheet, live: &LiveState) {
    if let Some((j, age)) = live.judgement
        && let Some((scale, alpha)) = judge_sprite_anim(age)
        && let (Some(uv), Some(aspect)) =
            (sheet.get(Sprite::Judge(j)), sheet.aspect(Sprite::Judge(j)))
    {
        let hw = style::JUDGE_SPRITE_W / 2.0 * scale;
        let paint = Paint::image([1.0, 1.0, 1.0, alpha], uv);
        push_rect(out, [0.0, style::JUDGE_SPRITE_Y], [hw, hw * aspect], &paint);
    }
    if live.combo > 0
        && let (Some(uv), Some(aspect)) = (sheet.get(Sprite::Combo), sheet.aspect(Sprite::Combo))
    {
        let hw = style::COMBO_SPRITE_W / 2.0;
        let paint = Paint::image([1.0; 4], uv);
        push_rect(out, [0.0, style::COMBO_SPRITE_Y], [hw, hw * aspect], &paint);
    }
}

/// Faint sensor ring boundaries and the pressed sensors.
fn push_sensor_guides(out: &mut Vec<Vertex>, touched: [u64; SENSOR_DEPTHS]) {
    let w = style::SENSOR_GUIDE_HALF_WIDTH;
    for (depth, &ring) in touched.iter().enumerate() {
        let (r_in, r_out) = sensor_ring(depth);
        push_sector(
            out,
            r_in - w,
            r_in + w,
            0.0,
            LANES as f32,
            style::SENSOR_GUIDE,
        );
        if depth + 1 == SENSOR_DEPTHS {
            push_sector(
                out,
                r_out - w,
                r_out + w,
                0.0,
                LANES as f32,
                style::SENSOR_GUIDE,
            );
        }
        for lane in (0..LANES).filter(|&l| ring >> l & 1 == 1) {
            push_sector(
                out,
                r_in,
                r_out,
                f32::from(lane),
                1.0,
                style::TOUCH_HIGHLIGHT,
            );
        }
    }
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
        for (i, sprite) in Sprite::ALL.into_iter().enumerate() {
            let u = i as f32 / 16.0;
            s.insert(sprite, [u, 0.5, u + 0.05, 0.75], [256, 64]);
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
        assert_eq!(count(&v, style::JUDGE_RING), 60 * 6);
        assert_eq!(v.len() % 3, 0);
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
            &SceneOptions { note_speed: 6.0 },
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
    fn judged_notes_vanish_and_touched_lanes_light_up() {
        let c = chart(vec![
            note(1.0, NoteBody::Touch, Bonus::None),
            note(1.2, NoteBody::Chain, Bonus::None),
        ]);
        let live = LiveState {
            judged: &[true],
            touched: [1 | 1 << 59, 0, 0, 1 << 5],
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
        assert_eq!(count(&v, style::TOUCH_HIGHLIGHT), 3 * 6);
        // Five full guide circles (4 ring starts + the outer edge).
        assert_eq!(count(&v, style::SENSOR_GUIDE), 5 * 60 * 6);
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
    fn flashes_and_judgement_sprites() {
        let sheet = full_sheet();
        let flashes = [
            Flash {
                lane: 5,
                width: 3,
                kind: JudgeSprite::Great,
                age: 0.05,
            },
            Flash {
                lane: 9,
                width: 3,
                kind: JudgeSprite::Good,
                age: 5.0,
            },
        ];
        let live = LiveState {
            flashes: &flashes,
            judgement: Some((JudgeSprite::Marvelous, 0.1)),
            combo: 3,
            ..LiveState::default()
        };
        let v = build_scene(&chart(vec![]), 0.0, &SceneOptions::default(), &live, &sheet);
        let glow = sheet.get(Sprite::Glow).unwrap();
        assert_eq!(uv_range(&v, glow), 3 * 6, "only the fresh flash");
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
        assert_eq!(uv_range(&v, sheet.get(Sprite::Combo).unwrap()), 6);
        // Without sprites: no flash texture, no judgement art, but the flash still draws flat.
        let flat = build_scene(
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &live,
            &SpriteSheet::default(),
        );
        let mut c = style::flash_color(JudgeSprite::Great);
        c[3] *= flash_alpha(0.05);
        assert_eq!(count(&flat, c), 3 * 6);
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
        assert_eq!(flash_alpha(0.0), 1.0);
        assert!(flash_alpha(style::FLASH_SECS / 2.0) < 0.5);
        assert_eq!(flash_alpha(style::FLASH_SECS), 0.0);
        assert_eq!(flash_alpha(-0.1), 0.0);

        let (s0, a0) = judge_sprite_anim(0.0).unwrap();
        assert!((s0 - 1.25).abs() < 1e-6 && a0 == 1.0);
        assert_eq!(judge_sprite_anim(style::JUDGE_POP_SECS).unwrap().0, 1.0);
        let (_, late) = judge_sprite_anim(style::JUDGE_SPRITE_SECS * 0.9).unwrap();
        assert!(late > 0.0 && late < 0.5);
        assert!(judge_sprite_anim(style::JUDGE_SPRITE_SECS).is_none());

        let (shift, alpha) = snap_arrow_anim(style::SNAP_ARROW_PERIOD * 1.5);
        assert!((shift - 0.5).abs() < 1e-5 && (alpha - 1.0).abs() < 1e-5);
        assert!(snap_arrow_anim(0.0).1.abs() < 1e-6);

        // Tile boundaries move towards higher lanes for counter-clockwise slides, lower for clockwise.
        let first = |t, dir| tiles(10.0, style::SLIDE_ARROW_LANES, slide_arrow_phase(t, dir))[0].1;
        assert!(first(0.02, SlideDir::Ccw) > first(0.01, SlideDir::Ccw));
        assert!(first(0.02, SlideDir::Cw) < first(0.01, SlideDir::Cw));
    }
}
