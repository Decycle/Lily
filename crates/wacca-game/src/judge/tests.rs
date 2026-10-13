//! Behaviour tests with hand-built charts and scripted sensor frames.

use super::*;
use wacca_chart::{BeatPos, Bonus, ChartHeader, HoldNode, SlideDir, SnapDir};

const F: f64 = FRAME_SECS;
const T: f64 = 1.0;

fn arc(lane: u8, width: u8) -> Arc {
    Arc { lane, width }
}

fn note(time: f64, a: Arc, body: NoteBody) -> Note {
    Note {
        pos: BeatPos::default(),
        time,
        arc: a,
        bonus: Bonus::None,
        body,
        file_index: 0,
    }
}

fn hold(nodes: &[(f64, Arc)]) -> Note {
    let nodes: Vec<HoldNode> = nodes
        .iter()
        .map(|&(time, arc)| HoldNode {
            pos: BeatPos::default(),
            time,
            arc,
            lane_unwrapped: arc.lane as f32,
            control: true,
        })
        .collect();
    note(nodes[0].time, nodes[0].arc, NoteBody::Hold { nodes })
}

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
        end_time: 0.0,
    }
}

/// Sensors on for `lanes` at `depths`.
fn on(a: Arc, depths: std::ops::Range<usize>) -> SensorState {
    let mut s = SensorState::default();
    for o in 0..a.width {
        for d in depths.clone() {
            s.set((a.lane + o) % 60, d, true);
        }
    }
    s
}

fn fr(time: f64, sensors: SensorState) -> InputFrame {
    InputFrame { time, sensors }
}

fn empty(time: f64) -> InputFrame {
    fr(time, SensorState::default())
}

/// Feed `frames`, then an empty frame 10 s later (resolves misses), then `finish`.
fn run(notes: Vec<Note>, frames: &[InputFrame]) -> Vec<JudgeEvent> {
    let mut judge = Judge::new(&chart(notes), JudgeTable::reverse());
    let last = frames.last().map_or(0.0, |f| f.time);
    let mut events: Vec<JudgeEvent> = frames.iter().flat_map(|f| judge.update(f)).collect();
    events.extend(judge.update(&empty(last + 10.0)));
    events.extend(judge.finish());
    events
}

fn judgements(events: &[JudgeEvent]) -> Vec<(usize, Judgement)> {
    let mut v: Vec<_> = events.iter().map(|e| (e.note, e.judgement)).collect();
    v.sort();
    v
}

/// Single touch note at lane 10 pressed (and released) at `T + frames * F`.
fn tap_touch(frames: f64) -> Judgement {
    let t = T + frames * F;
    let ev = run(
        vec![note(T, arc(10, 4), NoteBody::Touch)],
        &[fr(t, on(arc(11, 1), 0..1)), empty(t + 0.05)],
    );
    assert_eq!(ev.len(), 1);
    ev[0].judgement
}

#[test]
fn touch_tiers() {
    use Judgement::*;
    let cases = [
        (0.0, Marvelous),
        (-3.0, Marvelous),
        (3.0, Marvelous),
        (-3.5, Great),
        (5.0, Great),
        (-5.0, Great),
        (5.5, Good),
        (-6.0, Good),
        (6.0, Good),
        (6.5, Miss),
        (-6.5, Miss),
    ];
    for (frames, want) in cases {
        assert_eq!(tap_touch(frames), want, "offset {frames} frames");
    }
}

#[test]
fn touch_reports_offset_and_bonus() {
    let mut n = note(T, arc(10, 4), NoteBody::Touch);
    n.bonus = Bonus::R;
    let ev = run(vec![n], &[fr(T - 2.0 * F, on(arc(10, 1), 0..1))]);
    let offset = ev[0].offset.unwrap_or(f64::NAN);
    assert!((offset + 2.0 * F).abs() < 1e-9);
    assert_eq!((ev[0].bonus, ev[0].class), (Bonus::R, JudgeClass::Touch));
}

#[test]
fn early_press_outside_window_is_ignored() {
    let ev = run(
        vec![note(T, arc(10, 4), NoteBody::Touch)],
        &[
            fr(T - 7.0 * F, on(arc(10, 1), 0..1)),
            empty(T - 6.5 * F),
            fr(T, on(arc(10, 1), 0..1)),
        ],
    );
    assert_eq!(judgements(&ev), vec![(0, Judgement::Marvelous)]);
}

#[test]
fn resting_finger_does_not_hit_touch() {
    let ev = run(
        vec![note(T, arc(10, 4), NoteBody::Touch)],
        &[fr(0.5, on(arc(10, 1), 0..1)), fr(T, on(arc(10, 1), 0..1))],
    );
    assert_eq!(judgements(&ev), vec![(0, Judgement::Miss)]);
}

#[test]
fn one_press_hits_one_note() {
    // Overlapping notes 2 frames apart: one press only takes the first.
    let notes = vec![
        note(T, arc(10, 4), NoteBody::Touch),
        note(T + 2.0 * F, arc(12, 4), NoteBody::Touch),
    ];
    let ev = run(notes, &[fr(T, on(arc(12, 2), 0..2))]);
    assert_eq!(
        judgements(&ev),
        vec![(0, Judgement::Marvelous), (1, Judgement::Miss)]
    );
}

#[test]
fn one_press_hits_a_chord() {
    // Stacked notes at the same time (e.g. two holds splitting apart) and side-by-side notes
    // covered by one wide press are all hit.
    let notes = vec![
        hold(&[(T, arc(10, 4)), (T + 0.5, arc(5, 4))]),
        hold(&[(T, arc(10, 4)), (T + 0.5, arc(15, 4))]),
        note(T, arc(14, 4), NoteBody::Touch),
    ];
    let mut s = on(arc(12, 4), 0..1);
    let ev = run(
        notes,
        &[
            fr(T, s),
            fr(T + 0.5, {
                s = on(arc(5, 1), 3..4);
                s.rings[3] |= on(arc(15, 1), 3..4).rings[3];
                s
            }),
        ],
    );
    let all = vec![
        (0, Judgement::Marvelous),
        (1, Judgement::Marvelous),
        (2, Judgement::Marvelous),
    ];
    assert_eq!(judgements(&ev), all);
}

#[test]
fn hold_ignores_input_after_its_end() {
    // Released at the end, then the same lanes are pressed again much later.
    let ev = run(
        vec![hold(&[(T, arc(10, 4)), (T + 0.5, arc(10, 4))])],
        &[
            fr(T, on(arc(10, 1), 0..1)),
            empty(T + 0.49),
            fr(T + 1.5, on(arc(10, 1), 0..1)),
        ],
    );
    assert_eq!(judgements(&ev), vec![(0, Judgement::Marvelous)]);
}

#[test]
fn simultaneous_notes_two_fingers() {
    let notes = vec![
        note(T, arc(10, 4), NoteBody::Touch),
        note(T, arc(40, 4), NoteBody::Touch),
    ];
    let mut s = on(arc(10, 1), 0..1);
    s.rings[0] |= on(arc(41, 1), 0..1).rings[0];
    let ev = run(notes, &[fr(T, s)]);
    assert_eq!(
        judgements(&ev),
        vec![(0, Judgement::Marvelous), (1, Judgement::Marvelous)]
    );
}

#[test]
fn overlapping_windows_split_at_midpoint() {
    // Notes 4 frames apart on the same lane: a press 2.5 frames after the first belongs to
    // the second (cut at +2 frames), so the first misses.
    let notes = vec![
        note(T, arc(10, 4), NoteBody::Touch),
        note(T + 4.0 * F, arc(10, 4), NoteBody::Touch),
    ];
    let ev = run(notes, &[fr(T + 2.5 * F, on(arc(10, 1), 0..1))]);
    assert_eq!(
        judgements(&ev),
        vec![(0, Judgement::Miss), (1, Judgement::Marvelous)]
    );
}

#[test]
fn note_on_hold_end_loses_early_great_and_good() {
    let notes = vec![
        hold(&[(T - 1.0, arc(10, 4)), (T, arc(10, 4))]),
        note(T, arc(10, 4), NoteBody::Touch),
    ];
    let frames = [
        fr(T - 1.0, on(arc(10, 1), 0..1)),
        empty(T - 4.0 * F - 0.001),
        fr(T - 4.0 * F, on(arc(10, 1), 0..1)), // 4 f early: outside Marvelous-only window
        empty(T - 3.5 * F),
        fr(T - 3.0 * F, on(arc(10, 1), 0..1)),
    ];
    let ev = run(notes, &frames);
    assert_eq!(
        judgements(&ev),
        vec![(0, Judgement::Marvelous), (1, Judgement::Marvelous)]
    );
}

#[test]
fn wraparound_arc() {
    let n = || vec![note(T, arc(58, 4), NoteBody::Touch)];
    let hit = run(n(), &[fr(T, on(arc(0, 1), 3..4))]);
    assert_eq!(judgements(&hit), vec![(0, Judgement::Marvelous)]);
    let wrap_press = run(n(), &[fr(T, on(arc(59, 2), 3..4))]);
    assert_eq!(judgements(&wrap_press), vec![(0, Judgement::Marvelous)]);
    let outside = run(n(), &[fr(T, on(arc(2, 1), 3..4))]);
    assert_eq!(judgements(&outside), vec![(0, Judgement::Miss)]);
}

#[test]
fn chain_hit_by_resting_finger_at_note_time() {
    let n = || vec![note(T, arc(20, 4), NoteBody::Chain)];
    let held = on(arc(21, 1), 2..3);
    let ev = run(n(), &[fr(0.0, held), fr(T - 2.0 * F, held), fr(T, held)]);
    assert_eq!(judgements(&ev), vec![(0, Judgement::Marvelous)]);
    assert_eq!(ev[0].time, T);
    // Touched 4 frames late: still Marvelous; 5 frames late: Miss.
    let late = run(n(), &[fr(T + 4.0 * F, held)]);
    assert_eq!(judgements(&late), vec![(0, Judgement::Marvelous)]);
    let too_late = run(n(), &[fr(T + 5.0 * F, held)]);
    assert_eq!(judgements(&too_late), vec![(0, Judgement::Miss)]);
    // Touched only before the window opens: Miss.
    let early = run(n(), &[fr(T - 5.0 * F, held), empty(T - 4.5 * F)]);
    assert_eq!(judgements(&early), vec![(0, Judgement::Miss)]);
}

/// Swipe across `a`'s lanes `from`→`to` (offsets in the arc), arriving at `T + frames * F`.
fn swipe(dir: SlideDir, frames: f64, from: u8, to: u8) -> Judgement {
    let a = arc(30, 6);
    let t = T + frames * F;
    let ev = run(
        vec![note(T, a, NoteBody::Slide(dir))],
        &[
            fr(t - 0.5 * F, on(arc(a.lane + from, 1), 0..1)),
            fr(t, on(arc(a.lane + to, 1), 0..1)),
        ],
    );
    ev[0].judgement
}

#[test]
fn slide_windows_are_asymmetric() {
    use Judgement::*;
    let ccw = |f| swipe(SlideDir::Ccw, f, 0, 3);
    assert_eq!(ccw(-5.0), Marvelous);
    assert_eq!(ccw(7.0), Marvelous);
    assert_eq!(ccw(-5.5), Great);
    assert_eq!(ccw(7.5), Great);
    assert_eq!(ccw(-8.0), Great);
    assert_eq!(ccw(10.0), Great);
    assert_eq!(ccw(-9.0), Good);
    assert_eq!(ccw(10.5), Miss);
    assert_eq!(swipe(SlideDir::Cw, 0.0, 5, 2), Marvelous);
}

#[test]
fn slide_needs_direction_and_distance() {
    assert_eq!(swipe(SlideDir::Cw, 0.0, 0, 3), Judgement::Miss);
    assert_eq!(swipe(SlideDir::Ccw, 0.0, 3, 0), Judgement::Miss);
    assert_eq!(swipe(SlideDir::Ccw, 0.0, 0, 1), Judgement::Miss);
}

#[test]
fn full_circle_slide_crosses_lane_zero() {
    let a = arc(0, 60);
    let ev = run(
        vec![note(T, a, NoteBody::Slide(SlideDir::Ccw))],
        &[fr(T - F, on(arc(58, 1), 0..1)), fr(T, on(arc(1, 1), 0..1))],
    );
    assert_eq!(judgements(&ev), vec![(0, Judgement::Marvelous)]);
}

/// Move a finger radially `from`→`to` depth in the snap's arc, arriving at `T + frames * F`.
fn snap(dir: SnapDir, frames: f64, from: usize, to: usize) -> Judgement {
    let a = arc(5, 4);
    let t = T + frames * F;
    let ev = run(
        vec![note(T, a, NoteBody::Snap(dir))],
        &[
            fr(t - 0.5 * F, on(arc(6, 1), from..from + 1)),
            fr(t, on(arc(6, 1), to..to + 1)),
        ],
    );
    ev[0].judgement
}

#[test]
fn snap_in_vs_out() {
    use Judgement::*;
    assert_eq!(snap(SnapDir::In, 0.0, 3, 1), Marvelous);
    assert_eq!(snap(SnapDir::In, 0.0, 1, 3), Miss);
    assert_eq!(snap(SnapDir::Out, 0.0, 0, 2), Marvelous);
    assert_eq!(snap(SnapDir::Out, 0.0, 2, 0), Miss);
}

#[test]
fn snap_windows_are_asymmetric() {
    use Judgement::*;
    let inward = |f| snap(SnapDir::In, f, 3, 0);
    let outward = |f| snap(SnapDir::Out, f, 0, 3);
    assert_eq!(inward(7.0), Marvelous);
    assert_eq!(inward(-6.0), Great);
    assert_eq!(outward(-7.0), Marvelous);
    assert_eq!(outward(6.0), Great);
    assert_eq!(outward(9.0), Good);
    assert_eq!(outward(-10.5), Miss);
}

/// Hold on lanes 10..14 from `T` to `T + 1`, curving to lanes 20..24 at `T + 0.5`.
fn hold_note() -> Note {
    hold(&[
        (T, arc(10, 4)),
        (T + 0.5, arc(20, 4)),
        (T + 1.0, arc(20, 4)),
    ])
}

#[test]
fn hold_start_judged_and_body_followed() {
    let first = on(arc(11, 1), 0..1);
    let second = on(arc(21, 1), 0..1);
    let frames = [
        fr(T + 4.0 * F, first),
        fr(T + 0.5, second),
        fr(T + 1.0, second),
        empty(T + 1.1),
    ];
    let mut judge = Judge::new(&chart(vec![hold_note()]), JudgeTable::reverse());
    let ev: Vec<_> = frames[..2].iter().flat_map(|f| judge.update(f)).collect();
    assert!(ev.is_empty(), "hold is judged at its end");
    let ev = judge.update(&frames[2]);
    assert_eq!(judgements(&ev), vec![(0, Judgement::Great)]);
    assert_eq!(ev[0].class, JudgeClass::Hold);
    assert!(judge.update(&frames[3]).is_empty());
}

#[test]
fn hold_drop_allowance() {
    let held = |a| on(a, 0..1);
    // Released 0.15 s mid-hold (within the 175 ms allowance): keeps the start judgement.
    let short = run(
        vec![hold_note()],
        &[
            fr(T, held(arc(10, 1))),
            empty(T + 0.2),
            fr(T + 0.35, held(arc(10, 1))),
            fr(T + 0.5, held(arc(20, 1))),
            empty(T + 1.0),
        ],
    );
    assert_eq!(judgements(&short), vec![(0, Judgement::Marvelous)]);
    // Released 0.25 s: Good.
    let long = run(
        vec![hold_note()],
        &[
            fr(T, held(arc(10, 1))),
            empty(T + 0.2),
            fr(T + 0.45, held(arc(10, 1))),
            fr(T + 0.5, held(arc(20, 1))),
            empty(T + 1.0),
        ],
    );
    assert_eq!(judgements(&long), vec![(0, Judgement::Good)]);
    // Wrong lanes after the curve count as released.
    let off_path = run(
        vec![hold_note()],
        &[
            fr(T, held(arc(10, 1))),
            fr(T + 0.5, held(arc(10, 1))),
            empty(T + 1.0),
        ],
    );
    assert_eq!(judgements(&off_path), vec![(0, Judgement::Good)]);
    // Released for the final 0.2 s: fine.
    let early_release = run(
        vec![hold_note()],
        &[
            fr(T, held(arc(10, 1))),
            fr(T + 0.5, held(arc(20, 1))),
            empty(T + 0.8),
        ],
    );
    assert_eq!(judgements(&early_release), vec![(0, Judgement::Marvelous)]);
}

/// `hold_note` pressed at `T` (Marvelous), released mid-hold at `T + 0.1` for `gap` seconds.
fn hold_with_gap(gap: f64) -> Judgement {
    let ev = run(
        vec![hold_note()],
        &[
            fr(T, on(arc(10, 1), 0..1)),
            empty(T + 0.1),
            fr(T + 0.1 + gap, on(arc(10, 1), 0..1)),
            fr(T + 0.5, on(arc(20, 1), 0..1)),
            fr(T + 1.0, on(arc(20, 1), 0..1)),
        ],
    );
    assert_eq!(ev.len(), 1);
    ev[0].judgement
}

/// `hold_note` pressed at `T + start_frames * F`, followed, released `before_end` s before its end.
fn hold_released_before_end(start_frames: f64, before_end: f64) -> Judgement {
    let ev = run(
        vec![hold_note()],
        &[
            fr(T + start_frames * F, on(arc(10, 1), 0..1)),
            fr(T + start_frames.max(0.0) * F, on(arc(10, 1), 0..1)),
            fr(T + 0.5, on(arc(20, 1), 0..1)),
            empty(T + 1.0 - before_end),
        ],
    );
    assert_eq!(ev.len(), 1);
    ev[0].judgement
}

#[test]
fn hold_gap_167ms_keeps_start_judgement() {
    // Doc 10 §0: a mid-hold release of ≤ 167 ms keeps the start judgement.
    assert_eq!(hold_with_gap(0.167), Judgement::Marvelous);
}

#[test]
fn hold_gap_183ms_is_good() {
    // Doc 10 §0: ≥ 183 ms is a drop.
    assert_eq!(hold_with_gap(0.183), Judgement::Good);
}

#[test]
fn hold_release_233ms_before_end_keeps_start_judgement() {
    // Checked until end − 7 f: the gap is 233 − 117 = 117 ms (doc 10 §6.2).
    assert_eq!(hold_released_before_end(0.0, 0.233), Judgement::Marvelous);
}

#[test]
fn hold_release_267ms_before_end_keeps_start_judgement() {
    // Gap 267 − 117 = 150 ms.
    assert_eq!(hold_released_before_end(0.0, 0.267), Judgement::Marvelous);
}

#[test]
fn hold_release_300ms_before_end_is_good() {
    // Gap 300 − 117 = 183 ms: dropped.
    assert_eq!(hold_released_before_end(0.0, 0.300), Judgement::Good);
}

#[test]
fn hold_input_in_last_seven_frames_is_ignored() {
    // A release 6 f before the end and a re-press after it do not count.
    let ev = run(
        vec![hold_note()],
        &[
            fr(T, on(arc(10, 1), 0..1)),
            fr(T + 0.5, on(arc(20, 1), 0..1)),
            empty(T + 1.0 - 6.0 * F),
            fr(T + 1.0, on(arc(20, 1), 0..1)),
        ],
    );
    assert_eq!(judgements(&ev), vec![(0, Judgement::Marvelous)]);
}

#[test]
fn hold_missed_start_and_dropped_is_miss() {
    // Doc 10 §0: start at −8 f (outside the window), released 300 ms before the end.
    assert_eq!(hold_released_before_end(-8.0, 0.300), Judgement::Miss);
}

#[test]
fn hold_missed_start_but_held_is_good() {
    // Start at −8 f, body held to the end.
    assert_eq!(hold_released_before_end(-8.0, 0.0), Judgement::Good);
}

#[test]
fn hold_start_missed_or_never_touched() {
    // Resting finger (no new press) on the start, followed through: Good.
    let rest = on(arc(10, 1), 0..1);
    let ev = run(
        vec![hold_note()],
        &[
            fr(0.5, rest),
            fr(T + 0.5, on(arc(20, 1), 0..1)),
            empty(T + 1.0),
        ],
    );
    assert_eq!(judgements(&ev), vec![(0, Judgement::Good)]);
    let none = run(vec![hold_note()], &[empty(T + 1.0)]);
    assert_eq!(judgements(&none), vec![(0, Judgement::Miss)]);
}

#[test]
fn misses_emitted_once_late_limit_passes() {
    let mut judge = Judge::new(
        &chart(vec![note(T, arc(10, 4), NoteBody::Touch)]),
        JudgeTable::reverse(),
    );
    assert!(judge.update(&empty(T + 6.0 * F)).is_empty());
    let ev = judge.update(&empty(T + 6.5 * F));
    assert_eq!(judgements(&ev), vec![(0, Judgement::Miss)]);
    assert_eq!(ev[0].offset, None);
    assert!(judge.finish().is_empty());
}

#[test]
fn finish_misses_everything_left() {
    let notes = vec![
        note(T, arc(10, 4), NoteBody::Touch),
        hold_note(),
        note(T + 5.0, arc(0, 4), NoteBody::Chain),
    ];
    let mut judge = Judge::new(&chart(notes), JudgeTable::reverse());
    let hit = judge.update(&fr(T, on(arc(10, 1), 0..1)));
    assert_eq!(judgements(&hit), vec![(0, Judgement::Marvelous)]);
    let ev = judge.finish();
    assert_eq!(
        judgements(&ev),
        vec![(1, Judgement::Miss), (2, Judgement::Miss)]
    );
}

#[test]
fn autoplay_is_all_marvelous() {
    let notes = vec![
        note(1.0, arc(0, 4), NoteBody::Touch),
        note(1.0, arc(30, 4), NoteBody::Touch),
        note(1.1, arc(58, 6), NoteBody::Chain),
        note(1.2, arc(10, 8), NoteBody::Slide(SlideDir::Cw)),
        note(1.2, arc(40, 8), NoteBody::Slide(SlideDir::Ccw)),
        note(1.45, arc(5, 60), NoteBody::Slide(SlideDir::Ccw)),
        note(1.7, arc(20, 6), NoteBody::Snap(SnapDir::In)),
        note(1.75, arc(20, 6), NoteBody::Snap(SnapDir::Out)),
        hold(&[(2.0, arc(0, 4)), (2.4, arc(5, 4)), (2.9, arc(5, 4))]),
        note(2.0, arc(30, 4), NoteBody::Touch),
        note(2.9, arc(5, 4), NoteBody::Touch),
        note(2.91, arc(5, 4), NoteBody::Chain),
    ];
    assert_autoplay_perfect(notes);
}

fn assert_autoplay_perfect(notes: Vec<Note>) {
    let n = notes.len();
    let c = chart(notes);
    let mut judge = Judge::new(&c, JudgeTable::reverse());
    let mut ev: Vec<_> = autoplay_frames(&c)
        .iter()
        .flat_map(|f| judge.update(f))
        .collect();
    ev.extend(judge.finish());
    let want: Vec<_> = (0..n).map(|i| (i, Judgement::Marvelous)).collect();
    assert_eq!(judgements(&ev), want);
}

#[test]
fn autoplay_taps_inside_a_held_hold() {
    // The resting hold finger must not mask presses (or slides) on notes in its lanes.
    assert_autoplay_perfect(vec![
        hold(&[(1.0, arc(10, 12)), (2.0, arc(10, 12))]),
        note(1.3, arc(11, 10), NoteBody::Touch),
        note(1.5, arc(10, 12), NoteBody::Chain),
        note(1.7, arc(10, 12), NoteBody::Slide(SlideDir::Ccw)),
        hold(&[(1.8, arc(12, 4)), (1.9, arc(12, 4))]),
    ]);
}

#[test]
fn autoplay_hold_finger_waits_before_entering_a_tap() {
    // A narrow hold walks into a touch's lanes 6 frames before it: the finger must not arrive
    // early (fresh sensors are presses), nor move while the path stays under it.
    let walk: Vec<(f64, Arc)> = (0..=6)
        .map(|k| (1.0 + 0.05 * k as f64, arc(30 - k as u8, 1)))
        .chain([(1.5, arc(24, 1))])
        .collect();
    assert_autoplay_perfect(vec![
        hold(&walk),
        note(1.3, arc(15, 12), NoteBody::Touch),
        hold(&[(2.0, arc(40, 10)), (2.5, arc(41, 10)), (3.0, arc(44, 10))]),
        note(2.9, arc(45, 4), NoteBody::Touch),
    ]);
}
