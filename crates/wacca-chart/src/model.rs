//! Parsed chart data model (shared by the judge engine and the renderer).
//!
//! Times are chart seconds (`f64`), see `docs/research/01-chart-format.md` §5.
//! Lanes: 60 segments of 6°, lane 0 at 3 o'clock, increasing counter-clockwise (UNVERIFIED on screen).

pub const TICKS_PER_MEASURE: u32 = 1920;
pub const LANES: u8 = 60;

/// Position in the chart grid. `tick < TICKS_PER_MEASURE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BeatPos {
    pub measure: u32,
    pub tick: u16,
}

impl BeatPos {
    pub fn abs_tick(self) -> u64 {
        self.measure as u64 * TICKS_PER_MEASURE as u64 + self.tick as u64
    }
}

/// File suffix `_00`..`_03`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Difficulty {
    Normal = 0,
    Hard = 1,
    Expert = 2,
    Inferno = 3,
}

impl Difficulty {
    pub const ALL: [Difficulty; 4] = [Self::Normal, Self::Hard, Self::Expert, Self::Inferno];

    /// From the `_NN` chart file suffix (`0..=3`).
    pub fn from_file_suffix(suffix: u8) -> Option<Self> {
        Self::ALL.get(usize::from(suffix)).copied()
    }

    /// Chart file name `{song_id}_{NN}.mer` (the game's `%s/%s_%02d%s`).
    pub fn chart_file_name(self, song_id: &str) -> String {
        format!("{song_id}_{:02}.mer", self as u8)
    }
}

/// Score/gauge variant of a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bonus {
    None,
    /// Kinds 2/6/8: extra gauge, different hit sound, no score effect.
    Bonus,
    /// "R" kinds 20..26: worth 2x score.
    R,
}

/// Game "LEFT" (kinds 5/6/23) and "RIGHT" (7/8/24).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlideDir {
    Cw,
    Ccw,
}

/// Game "UP" (3/21, toward centre, red) and "DOWN" (4/22, toward rim, blue).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapDir {
    In,
    Out,
}

/// `GuideLaneDirection` 0/1/2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaskDir {
    Plus,
    Minus,
    Center,
}

/// Span of lanes `lane .. lane + width` (mod 60). `width` is 1..=60.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Arc {
    pub lane: u8,
    pub width: u8,
}

impl Arc {
    /// True if `lane` (0..60) lies inside this arc.
    pub fn contains(self, lane: u8) -> bool {
        let rel = (lane as i32 - self.lane as i32).rem_euclid(LANES as i32);
        rel < self.width as i32
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoldNode {
    pub pos: BeatPos,
    pub time: f64,
    pub arc: Arc,
    /// Lane after shortest-path unwrapping relative to the previous node (start node = its lane).
    pub lane_unwrapped: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NoteBody {
    Touch,
    Snap(SnapDir),
    Slide(SlideDir),
    Chain,
    /// `nodes[0]` = start, last = end, joints in between (as stored in the file).
    Hold {
        nodes: Vec<HoldNode>,
    },
}

/// One judged object. A hold is a single `Note` (one judgement).
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub pos: BeatPos,
    pub time: f64,
    pub arc: Arc,
    pub bonus: Bonus,
    pub body: NoteBody,
    /// `NOTE_INDEX` of the head row in the file (debugging / stable ids).
    pub file_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskEvent {
    pub pos: BeatPos,
    pub time: f64,
    pub arc: Arc,
    pub add: bool,
    pub dir: MaskDir,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoSegment {
    pub start_tick: u64,
    pub start_sec: f64,
    pub bpm: f64,
    pub num: u32,
    pub den: u32,
}

/// Piecewise-constant visual scroll speed (hi-speed / stop). Distance is in "scroll seconds".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollSegment {
    pub start_sec: f64,
    pub start_dist: f64,
    pub speed: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverseSection {
    pub start: f64,
    pub end: f64,
    pub note_end: f64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ChartHeader {
    pub music_score_id: i32,
    pub music_score_version: i32,
    pub game_version: String,
    /// BGM cue name, e.g. `MER_BGM_S00_003`.
    pub music_file_path: String,
    /// `audio_time = chart_time - offset` (seconds). Sign UNVERIFIED.
    pub offset: f64,
    pub movie_offset: f64,
    /// Unknown tags, kept for debugging.
    pub extra: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub header: ChartHeader,
    /// Judged notes sorted by time (stable by file order).
    pub notes: Vec<Note>,
    pub masks: Vec<MaskEvent>,
    pub tempo: Vec<TempoSegment>,
    pub scroll: Vec<ScrollSegment>,
    pub reverses: Vec<ReverseSection>,
    /// Kind 18 (tutorial only).
    pub tutorial_tags: Vec<BeatPos>,
    /// EOF row (kind 14).
    pub end: BeatPos,
    pub end_time: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_contains_wraps() {
        let a = Arc { lane: 58, width: 4 };
        assert!(a.contains(58) && a.contains(59) && a.contains(0) && a.contains(1));
        assert!(!a.contains(2) && !a.contains(57));
        assert!((0..60).all(|l| Arc { lane: 7, width: 60 }.contains(l)));
    }

    #[test]
    fn difficulty_file_suffix() {
        assert_eq!(Difficulty::from_file_suffix(2), Some(Difficulty::Expert));
        assert_eq!(Difficulty::from_file_suffix(4), None);
        assert_eq!(
            Difficulty::Inferno.chart_file_name("S03-011"),
            "S03-011_03.mer"
        );
    }

    #[test]
    fn abs_tick() {
        assert_eq!(
            BeatPos {
                measure: 2,
                tick: 5
            }
            .abs_tick(),
            3845
        );
    }
}
