//! One play of one chart, independent of window/audio: input frames in, judgements and score out.
//!
//! Song time model: the audio clock is master, `chart time = chart.chart_time(audio position) + user offset`.

use wacca_chart::Chart;
use wacca_game::grade::PlayResult;
use wacca_game::input::InputFrame;
use wacca_game::judge::{Judge, JudgeTable, autoplay_frames, class_of};
use wacca_game::rules::JudgeEvent;
use wacca_game::score::PlayTally;

/// Silence before the first note (chart seconds) so it does not arrive at t = 0.
pub const LEAD_IN_SECS: f64 = 3.0;
/// Play continues this long after the chart's end marker before results.
pub const END_TAIL_SECS: f64 = 1.0;
/// The audio clock stops slightly before the stream's length (output latency); end within this.
const AUDIO_END_SLACK: f64 = 0.5;

/// Chart time at audio position `audio_pos` (seconds) with user offset `offset` (seconds).
pub fn chart_now(chart: &Chart, audio_pos: f64, offset: f64) -> f64 {
    chart.chart_time(audio_pos) + offset
}

/// Audio position to start playback at: the start of the song, or earlier (negative = leading
/// silence) so the first note comes at least [`LEAD_IN_SECS`] after the start.
pub fn start_position(chart: &Chart, offset: f64) -> f64 {
    let first = chart.notes.first().map_or(0.0, |n| n.time);
    chart.audio_time(first - LEAD_IN_SECS - offset).min(0.0)
}

/// Chart time at which the play ends: the chart end plus a tail, or when the audio runs out
/// (if `audio_secs > 0`), whichever is first.
pub fn end_time(chart: &Chart, audio_secs: f64, offset: f64) -> f64 {
    let chart_end = chart.end_time + END_TAIL_SECS;
    if audio_secs > 0.0 {
        chart_end.min(chart_now(chart, audio_secs - AUDIO_END_SLACK, offset))
    } else {
        chart_end
    }
}

/// Judge + tally for one play.
#[derive(Debug)]
pub struct Session {
    judge: Judge,
    tally: PlayTally,
    /// Time of the last frame fed (frames must not go backwards).
    now: f64,
    last: Option<JudgeEvent>,
    /// Per note: decided (drawn no more).
    judged: Vec<bool>,
}

impl Session {
    pub fn new(chart: &Chart, bonus_effect: bool) -> Self {
        let notes: Vec<_> = chart.notes.iter().map(|n| (class_of(n), n.bonus)).collect();
        Self {
            judge: Judge::new(chart, JudgeTable::reverse()),
            tally: PlayTally::new(&notes, bonus_effect),
            now: f64::NEG_INFINITY,
            last: None,
            judged: vec![false; chart.notes.len()],
        }
    }

    /// Feed one sensor snapshot. A frame older than the previous one is moved up to its time
    /// (the judge needs non-decreasing times).
    pub fn feed(&mut self, frame: &InputFrame) -> Vec<JudgeEvent> {
        let mut frame = *frame;
        if frame.time < self.now {
            log::trace!(
                "input frame {:.4} before {:.4}; clamped",
                frame.time,
                self.now
            );
            frame.time = self.now;
        }
        self.now = frame.time;
        let events = self.judge.update(&frame);
        self.apply(&events);
        events
    }

    fn apply(&mut self, events: &[JudgeEvent]) {
        for e in events {
            log::debug!(
                "t={:.3} note {} {:?} {:?} offset {:?}",
                e.time,
                e.note,
                e.class,
                e.judgement,
                e.offset.map(|o| (o * 1000.0).round())
            );
            self.tally.apply(e);
            self.last = Some(*e);
            if let Some(j) = self.judged.get_mut(e.note) {
                *j = true;
            }
        }
    }

    /// Most recent judgement (for the HUD popup).
    pub fn last_event(&self) -> Option<JudgeEvent> {
        self.last
    }

    /// `judged()[i]`: note `i` is decided.
    pub fn judged(&self) -> &[bool] {
        &self.judged
    }

    pub fn score(&self) -> u32 {
        self.tally.score()
    }

    pub fn combo(&self) -> u32 {
        self.tally.combo()
    }

    /// Song played to the end: unresolved notes are missed, then the result.
    pub fn finish(&mut self, norma: f32) -> PlayResult {
        let events = self.judge.finish();
        self.apply(&events);
        self.tally.finish(norma)
    }

    /// Abandoned: unjudged notes count as misses and the play is failed.
    pub fn give_up(&self, norma: f32) -> PlayResult {
        self.tally.finish(norma)
    }
}

/// Pre-computed perfect input, released as the clock passes each frame.
#[derive(Debug)]
pub struct Autoplay {
    frames: Vec<InputFrame>,
    next: usize,
}

impl Autoplay {
    pub fn new(chart: &Chart) -> Self {
        Self {
            frames: autoplay_frames(chart),
            next: 0,
        }
    }

    /// Frames with `time <= now` not yet returned.
    pub fn due(&mut self, now: f64) -> &[InputFrame] {
        let start = self.next;
        self.next += self.frames[start..].partition_point(|f| f.time <= now);
        &self.frames[start..self.next]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::{Arc, BeatPos, Bonus, ChartHeader, Note, NoteBody};
    use wacca_game::grade::Lamp;
    use wacca_game::input::SensorState;
    use wacca_game::rules::Judgement;

    fn touch(time: f64, lane: u8) -> Note {
        Note {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width: 4 },
            bonus: Bonus::None,
            body: NoteBody::Touch,
            file_index: 0,
        }
    }

    fn chart(notes: Vec<Note>, offset: f64) -> Chart {
        Chart {
            header: ChartHeader {
                offset,
                ..ChartHeader::default()
            },
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

    fn frame(time: f64, lane: Option<u8>) -> InputFrame {
        let mut sensors = SensorState::default();
        if let Some(l) = lane {
            sensors.set(l, 3, true);
        }
        InputFrame { time, sensors }
    }

    #[test]
    fn hit_and_miss_update_score_combo_and_last() {
        let c = chart(vec![touch(1.0, 0), touch(2.0, 20)], 0.0);
        let mut s = Session::new(&c, true);
        assert!(s.feed(&frame(0.5, None)).is_empty());
        let hit = s.feed(&frame(1.0, Some(1)));
        assert_eq!(hit.len(), 1);
        assert_eq!(
            s.last_event().map(|e| e.judgement),
            Some(Judgement::Marvelous)
        );
        assert_eq!((s.score(), s.combo()), (500_000, 1));
        assert_eq!(s.judged(), [true, false]);
        // Note 2 is never touched: missed once its window passes.
        s.feed(&frame(1.1, None));
        let late = s.feed(&frame(2.5, None));
        assert_eq!(late[0].judgement, Judgement::Miss);
        assert_eq!(s.judged(), [true, true]);
        assert_eq!(s.tally.combo(), 0);
        let r = s.finish(0.0);
        assert_eq!(
            (r.score, r.counts.marvelous, r.counts.miss),
            (500_000, 1, 1)
        );
    }

    #[test]
    fn frames_going_backwards_are_clamped() {
        let c = chart(vec![touch(1.0, 0)], 0.0);
        let mut s = Session::new(&c, true);
        s.feed(&frame(1.0, None));
        // An event stamped earlier than the last render frame is judged at the later time.
        let e = s.feed(&frame(0.99, Some(0)));
        assert_eq!(e[0].time, 1.0);
        assert_eq!(e[0].judgement, Judgement::Marvelous);
    }

    #[test]
    fn finish_resolves_pending_notes_but_give_up_fails() {
        let c = chart(vec![touch(1.0, 0), touch(5.0, 0)], 0.0);
        let mut s = Session::new(&c, true);
        s.feed(&frame(1.0, Some(0)));
        let given_up = s.give_up(0.0);
        assert_eq!(given_up.lamp, Lamp::Failed);
        assert_eq!(given_up.counts.miss, 1);
        let done = s.finish(0.0);
        assert_eq!(done.counts.miss, 1);
        assert_eq!(done.lamp, Lamp::Missless);
    }

    #[test]
    fn autoplay_session_scores_max() {
        let c = chart(vec![touch(1.0, 0), touch(1.5, 30), touch(2.0, 58)], 0.0);
        let mut s = Session::new(&c, true);
        let mut auto = Autoplay::new(&c);
        for now in (0..300).map(|i| f64::from(i) / 60.0) {
            for f in auto.due(now).to_vec() {
                s.feed(&f);
            }
        }
        assert!(auto.due(100.0).is_empty());
        let r = s.finish(0.0);
        assert_eq!((r.score, r.lamp), (1_000_000, Lamp::AllMarvelous));
    }

    #[test]
    fn autoplay_releases_each_frame_once() {
        let c = chart(vec![touch(1.0, 0)], 0.0);
        let mut auto = Autoplay::new(&c);
        assert!(auto.due(0.5).is_empty());
        let n = auto.due(1.0).len();
        assert!(n >= 1);
        assert!(auto.due(1.0).is_empty());
    }

    #[test]
    fn clock_mapping_lead_in_and_end() {
        // offset 0.5: audio = chart - 0.5.
        let c = chart(vec![touch(1.0, 0)], 0.5);
        assert!((chart_now(&c, 0.5, 0.0) - 1.0).abs() < 1e-12);
        assert!((chart_now(&c, 0.5, 0.02) - 1.02).abs() < 1e-12);
        // First note at chart 1.0 → start LEAD_IN before it, in leading silence.
        let start = start_position(&c, 0.0);
        assert!((start - (0.5 - LEAD_IN_SECS)).abs() < 1e-12);
        assert!((chart_now(&c, start, 0.0) - (1.0 - LEAD_IN_SECS)).abs() < 1e-12);
        // The user offset moves the start with it.
        let shifted = start_position(&c, 0.1);
        assert!((chart_now(&c, shifted, 0.1) - (1.0 - LEAD_IN_SECS)).abs() < 1e-12);
        // A late first note: the song simply starts at its beginning.
        let late = chart(vec![touch(30.0, 0)], 0.0);
        assert_eq!(start_position(&late, 0.0), 0.0);
        // End: chart end + tail, unless the audio is shorter.
        assert_eq!(end_time(&c, 0.0, 0.0), 10.0 + END_TAIL_SECS);
        assert_eq!(end_time(&c, 100.0, 0.0), 10.0 + END_TAIL_SECS);
        assert!((end_time(&c, 5.0, 0.0) - (5.0 - AUDIO_END_SLACK + 0.5)).abs() < 1e-12);
    }
}
