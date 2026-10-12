//! Builds the playfield vertex list for a chart at a given time (pure, no GPU).

use wacca_chart::{Chart, LANES, Note, NoteBody};

use crate::layout::{
    Vertex, depth_to_radius, last_drawable_time, mask_state, push_hold_body, push_radial_line,
    push_sector, raw_depth, sensor_ring,
};
use crate::style::{self, SENSOR_DEPTHS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneOptions {
    /// Player note-speed option (1.0..=6.0).
    pub note_speed: f64,
}

/// Live play state drawn on top of the chart.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveState<'a> {
    /// `judged[i]`: note `i` (index into `Chart::notes`) is decided and no longer drawn.
    /// Shorter than the note list = the rest are not judged.
    pub judged: &'a [bool],
    /// Pressed sensors: `touched[depth]` bit `lane` (as `wacca_game::input::SensorState`).
    pub touched: [u64; SENSOR_DEPTHS],
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self { note_speed: 1.0 }
    }
}

/// Triangle list (back to front) of the playfield at chart time `now`.
pub fn build_scene(chart: &Chart, now: f64, opts: &SceneOptions, live: &LiveState) -> Vec<Vertex> {
    let mut out = Vec::new();
    let lanes = LANES as usize;

    let mask = mask_state(&chart.masks, now);
    for lane in (0..lanes).filter(|&l| mask[l]) {
        push_sector(&mut out, 0.0, 1.0, lane as f32, 1.0, style::GUIDE_LANE);
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

    for (_, note) in chart.notes.iter().enumerate().filter(drawable) {
        if let NoteBody::Hold { nodes } = &note.body {
            push_hold_body(&mut out, nodes, depth_of, style::HOLD_BODY);
        }
    }
    // Far notes first so nearer ones are drawn on top.
    for (_, note) in chart.notes.iter().enumerate().rev().filter(drawable) {
        if let NoteBody::Hold { nodes } = &note.body
            && let Some(end) = nodes.last().filter(|_| nodes.len() > 1)
            && let Some(d) = visible(end.time)
        {
            push_note(&mut out, d, end.arc.lane, end.arc.width, style::HOLD, None);
        }
        if let Some(d) = visible(note.time) {
            let accent = style::accent_color(note.bonus);
            push_note(
                &mut out,
                d,
                note.arc.lane,
                note.arc.width,
                style::note_color(&note.body),
                accent,
            );
        }
    }

    push_sector(
        &mut out,
        1.0,
        1.0 + style::JUDGE_RING_WIDTH,
        0.0,
        LANES as f32,
        style::JUDGE_RING,
    );
    push_sensor_guides(&mut out, live.touched);
    out
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

fn push_note(
    out: &mut Vec<Vertex>,
    depth: f32,
    lane: u8,
    width: u8,
    color: style::Rgba,
    accent: Option<style::Rgba>,
) {
    let r = depth_to_radius(depth);
    let trim = if width < LANES {
        style::NOTE_TRIM_LANES
    } else {
        0.0
    };
    let (start, span) = (lane as f32 + trim, width as f32 - 2.0 * trim);
    push_sector(
        out,
        r * (1.0 - style::NOTE_THICKNESS),
        r,
        start,
        span,
        color,
    );
    if let Some(accent) = accent {
        push_sector(
            out,
            r,
            r * (1.0 + style::ACCENT_THICKNESS),
            start,
            span,
            accent,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::APPROACH_SECS;
    use wacca_chart::{
        Arc, BeatPos, Bonus, ChartHeader, HoldNode, MaskDir, MaskEvent, ReverseSection,
    };

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

    #[test]
    fn empty_chart_has_all_lanes_and_ring() {
        let v = build_scene(
            &chart(vec![]),
            0.0,
            &SceneOptions::default(),
            &LiveState::default(),
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
        let v = build_scene(&c, 5.0, &SceneOptions::default(), &LiveState::default());
        assert_eq!(count(&v, style::GUIDE_LANE), 10 * 6);
        assert_eq!(count(&v, style::LANE_LINE), 11 * 6);
    }

    #[test]
    fn only_visible_notes_drawn_with_accent() {
        let c = chart(vec![
            note(1.0, NoteBody::Touch, Bonus::R),
            note(1.0 + APPROACH_SECS + 1.0, NoteBody::Chain, Bonus::None),
        ]);
        let v = build_scene(&c, 0.5, &SceneOptions::default(), &LiveState::default());
        assert_eq!(count(&v, style::TOUCH), 4 * 6);
        assert_eq!(count(&v, style::R_ACCENT), 4 * 6);
        assert_eq!(count(&v, style::CHAIN), 0);
        // Faster note speed shrinks the window further; the touch note is still inside.
        let v = build_scene(
            &c,
            0.5,
            &SceneOptions { note_speed: 6.0 },
            &LiveState::default(),
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
        let v = build_scene(&c, 11.0, &SceneOptions::default(), &LiveState::default());
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
        };
        let v = build_scene(&c, 0.5, &SceneOptions::default(), &live);
        assert_eq!(count(&v, style::TOUCH), 0);
        assert_eq!(count(&v, style::CHAIN), 4 * 6);
        assert_eq!(count(&v, style::TOUCH_HIGHLIGHT), 3 * 6);
        // Five full guide circles (4 ring starts + the outer edge).
        assert_eq!(count(&v, style::SENSOR_GUIDE), 5 * 60 * 6);
    }
}
