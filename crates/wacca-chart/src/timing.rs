//! Tempo (tick <-> chart seconds), visual scroll distance and audio alignment.
//!
//! Spec: `docs/research/01-chart-format.md` §5. All maths in `f64`, no rounding.

use crate::model::{Chart, ReverseSection, ScrollSegment, TICKS_PER_MEASURE, TempoSegment};

const TPM: f64 = TICKS_PER_MEASURE as f64;

/// Used when a chart lacks a BPM or time signature at tick 0 (§5.1 step 3).
const DEFAULT_TEMPO: TempoSegment = TempoSegment {
    start_tick: 0,
    start_sec: 0.0,
    bpm: 120.0,
    num: 4,
    den: 4,
};

const DEFAULT_SCROLL: ScrollSegment = ScrollSegment {
    start_sec: 0.0,
    start_dist: 0.0,
    speed: 1.0,
};

/// A BPM (kind 2) or time-signature (kind 3) row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TempoChange {
    Bpm(f64),
    Sig(u32, u32),
}

/// A row that changes the visual scroll speed: SPEED_CHANGE (5), STOP_START (9), STOP_END (10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ScrollEvent {
    Speed(f64),
    StopStart,
    StopEnd,
}

impl TempoSegment {
    /// Seconds per 1920-tick measure: `4 * num/den` quarter notes at `bpm`.
    pub fn sec_per_measure(&self) -> f64 {
        240.0 * self.num as f64 / (self.den as f64 * self.bpm)
    }
}

impl ReverseSection {
    /// View time during `(start, end)`: runs backwards from `note_end` to `end` (§5.3, MercuryPlayer model).
    /// The renderer should then draw only notes with `time <= note_end`. `None` outside the section.
    // UNVERIFIED: the game's exact reverse visual.
    pub fn view_time(&self, now: f64) -> Option<f64> {
        (self.start < now && now < self.end).then(|| {
            self.end + (self.end - now) * (self.note_end - self.end) / (self.end - self.start)
        })
    }
}

/// Builds tempo segments from changes sorted stably by tick (§5.1); per tick the last BPM and the last
/// time signature win. Always returns at least one segment, starting at tick 0.
pub(crate) fn build_tempo(changes: &[(u64, TempoChange)]) -> Vec<TempoSegment> {
    let at_zero = || changes.iter().take_while(|(t, _)| *t == 0);
    if !at_zero().any(|(_, c)| matches!(c, TempoChange::Bpm(_)))
        || !at_zero().any(|(_, c)| matches!(c, TempoChange::Sig(..)))
    {
        log::warn!("no BPM or time signature at tick 0; defaulting to 120 BPM 4/4");
    }
    let mut segs = Vec::new();
    let mut cur = DEFAULT_TEMPO;
    for &(tick, change) in changes {
        if tick > cur.start_tick {
            let start_sec =
                cur.start_sec + (tick - cur.start_tick) as f64 / TPM * cur.sec_per_measure();
            segs.push(cur);
            cur.start_tick = tick;
            cur.start_sec = start_sec;
        }
        match change {
            TempoChange::Bpm(bpm) => cur.bpm = bpm,
            TempoChange::Sig(num, den) => (cur.num, cur.den) = (num, den),
        }
    }
    segs.push(cur);
    segs
}

/// Builds the piecewise-constant hi-speed function `hs(t)` from events sorted by time (§5.3):
/// starts at 1, SPEED_CHANGE sets it, STOP_START sets 0, STOP_END restores the last SPEED_CHANGE rate (or 1).
pub(crate) fn build_scroll(events: &[(f64, ScrollEvent)]) -> Vec<ScrollSegment> {
    let mut segs = Vec::new();
    let mut cur = DEFAULT_SCROLL;
    let mut rate = 1.0;
    for &(sec, event) in events {
        let speed = match event {
            ScrollEvent::Speed(r) => {
                rate = r;
                r
            }
            ScrollEvent::StopStart => 0.0,
            ScrollEvent::StopEnd => rate,
        };
        if sec > cur.start_sec {
            let start_dist = cur.start_dist + (sec - cur.start_sec) * cur.speed;
            segs.push(cur);
            cur = ScrollSegment {
                start_sec: sec,
                start_dist,
                speed,
            };
        } else {
            cur.speed = speed;
        }
    }
    segs.push(cur);
    segs
}

/// The last segment whose start satisfies `started` (segments sorted), or the first one.
fn segment<T: Copy>(segs: &[T], started: impl Fn(&T) -> bool) -> Option<T> {
    segs.get(segs.partition_point(started).saturating_sub(1))
        .copied()
}

pub(crate) fn tick_to_sec(tempo: &[TempoSegment], tick: u64) -> f64 {
    let seg = segment(tempo, |s| s.start_tick <= tick).unwrap_or(DEFAULT_TEMPO);
    seg.start_sec + (tick as f64 - seg.start_tick as f64) / TPM * seg.sec_per_measure()
}

impl Chart {
    /// Chart seconds at an absolute tick (`measure * 1920 + tick`).
    pub fn tick_to_sec(&self, tick: u64) -> f64 {
        tick_to_sec(&self.tempo, tick)
    }

    /// Fractional absolute tick at chart seconds `sec` (extrapolates before 0).
    pub fn sec_to_tick(&self, sec: f64) -> f64 {
        let seg = segment(&self.tempo, |s| s.start_sec <= sec).unwrap_or(DEFAULT_TEMPO);
        seg.start_tick as f64 + (sec - seg.start_sec) / seg.sec_per_measure() * TPM
    }

    /// Scroll distance `D(sec) = ∫_0^sec hs(τ) dτ` in "scroll seconds" (§5.3). A note at time `t_n` is drawn at
    /// depth `(dist_at(t_n) - dist_at(now)) / V`.
    pub fn dist_at(&self, sec: f64) -> f64 {
        let seg = segment(&self.scroll, |s| s.start_sec <= sec).unwrap_or(DEFAULT_SCROLL);
        seg.start_dist + (sec - seg.start_sec) * seg.speed
    }

    /// Time to use for scroll positions at chart time `now`: remapped inside reverse sections, else `now`.
    pub fn view_time(&self, now: f64) -> f64 {
        self.reverses
            .iter()
            .find_map(|r| r.view_time(now))
            .unwrap_or(now)
    }

    /// BGM position at chart time `chart_time`: `chart_time - #OFFSET` (§5.2).
    // UNVERIFIED: sign of #OFFSET (inferred from MercuryPlayer and the one-measure lead-in pattern).
    pub fn audio_time(&self, chart_time: f64) -> f64 {
        chart_time - self.header.offset
    }

    /// Inverse of [`Chart::audio_time`].
    pub fn chart_time(&self, audio_time: f64) -> f64 {
        audio_time + self.header.offset
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::{BeatPos, ChartHeader};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn chart(tempo: Vec<TempoSegment>, scroll: Vec<ScrollSegment>) -> Chart {
        Chart {
            header: ChartHeader {
                offset: 0.25,
                ..Default::default()
            },
            notes: vec![],
            masks: vec![],
            tempo,
            scroll,
            reverses: vec![],
            tutorial_tags: vec![],
            end: BeatPos::default(),
            end_time: 0.0,
        }
    }

    #[test]
    fn tempo_segments_and_round_trip() {
        // 120 BPM 4/4 (2 s/measure), then 3/4 at measure 2 (1.5 s), then 240 BPM at measure 3 (0.75 s).
        let segs = build_tempo(&[
            (0, TempoChange::Bpm(120.0)),
            (0, TempoChange::Sig(4, 4)),
            (3840, TempoChange::Sig(3, 4)),
            (5760, TempoChange::Bpm(240.0)),
        ]);
        assert_eq!(segs.len(), 3);
        assert!(close(segs[1].start_sec, 4.0) && close(segs[2].start_sec, 5.5));
        let c = chart(segs, vec![]);
        assert!(close(c.tick_to_sec(960), 1.0));
        assert!(close(c.tick_to_sec(3840 + 960), 4.75));
        assert!(close(c.tick_to_sec(5760 + 1920), 6.25));
        for tick in [0, 1, 959, 3840, 4000, 5760, 9999] {
            assert!(close(c.sec_to_tick(c.tick_to_sec(tick)), tick as f64));
        }
        assert!(close(c.sec_to_tick(-1.0), -960.0));
    }

    #[test]
    fn same_tick_changes_last_wins() {
        let segs = build_tempo(&[
            (0, TempoChange::Bpm(100.0)),
            (0, TempoChange::Sig(4, 4)),
            (0, TempoChange::Sig(3, 4)),
            (0, TempoChange::Bpm(120.0)),
            (1920, TempoChange::Sig(7, 8)),
            (1920, TempoChange::Sig(4, 4)),
        ]);
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].bpm, segs[0].num, segs[0].den), (120.0, 3, 4));
        assert_eq!((segs[1].num, segs[1].den), (4, 4));
        assert!(close(segs[1].start_sec, 1.5));
    }

    #[test]
    fn mid_measure_signature_change_counts_from_change_point() {
        let segs = build_tempo(&[
            (0, TempoChange::Bpm(120.0)),
            (0, TempoChange::Sig(4, 4)),
            (960, TempoChange::Sig(2, 4)),
        ]);
        let c = chart(segs, vec![]);
        assert!(close(c.tick_to_sec(960), 1.0));
        assert!(close(c.tick_to_sec(960 + 1920), 2.0));
    }

    #[test]
    fn missing_tick_zero_tempo_defaults() {
        let segs = build_tempo(&[(1920, TempoChange::Bpm(60.0))]);
        assert_eq!((segs[0].bpm, segs[0].num, segs[0].den), (120.0, 4, 4));
        assert!(close(segs[1].start_sec, 2.0));
        assert_eq!(build_tempo(&[]), vec![DEFAULT_TEMPO]);
    }

    #[test]
    fn scroll_hi_speed_and_stops() {
        let segs = build_scroll(&[
            (1.0, ScrollEvent::Speed(2.0)),
            (2.0, ScrollEvent::StopStart),
            (3.0, ScrollEvent::StopEnd),
            (4.0, ScrollEvent::Speed(-0.5)),
            (5.0, ScrollEvent::StopStart),
            (5.0, ScrollEvent::StopEnd),
        ]);
        let c = chart(vec![], segs);
        assert!(close(c.dist_at(-1.0), -1.0));
        assert!(close(c.dist_at(1.0), 1.0));
        assert!(close(c.dist_at(1.5), 2.0));
        assert!(close(c.dist_at(2.5), 3.0)); // stopped
        assert!(close(c.dist_at(3.5), 4.0)); // restored to 2.0
        assert!(close(c.dist_at(4.5), 4.75)); // negative scroll
        assert!(close(c.dist_at(6.0), 4.0)); // stop+end on one tick: back to -0.5
    }

    #[test]
    fn stop_end_without_speed_change_restores_one() {
        let c = chart(
            vec![],
            build_scroll(&[(1.0, ScrollEvent::StopStart), (2.0, ScrollEvent::StopEnd)]),
        );
        assert!(close(c.dist_at(1.5), 1.0));
        assert!(close(c.dist_at(3.0), 2.0));
    }

    #[test]
    fn reverse_view_time() {
        let mut c = chart(vec![], vec![]);
        c.reverses.push(ReverseSection {
            start: 2.0,
            end: 4.0,
            note_end: 8.0,
        });
        assert_eq!(c.view_time(1.0), 1.0);
        assert!(close(c.view_time(2.0 + 1e-12), 8.0));
        assert!(close(c.view_time(3.0), 6.0));
        assert_eq!(c.view_time(4.0), 4.0);
        assert_eq!(c.view_time(5.0), 5.0);
    }

    #[test]
    fn audio_offset() {
        let c = chart(vec![], vec![]);
        assert!(close(c.audio_time(1.0), 0.75));
        assert!(close(c.chart_time(c.audio_time(3.0)), 3.0));
    }
}
