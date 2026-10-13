//! One running play: audio-clocked [`Session`], touch contacts or autoplay, hit sounds.
//!
//! Input timing: every touch/mouse event immediately feeds an `InputFrame` stamped with the chart
//! time read from the audio clock at that moment, and every rendered frame feeds one more with the
//! current contacts so time-driven judgements (misses, holds, chains) advance even without input.
//! With autoplay, the scripted frames that fell due are fed instead and user input is ignored.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use wacca_audio::{BgmBank, HitSounds, Player, VoiceId};
use wacca_chart::{Arc as LaneArc, Chart, Difficulty, NoteBody};
use wacca_game::grade::PlayResult;
use wacca_game::input::{DEPTHS, InputFrame};
use wacca_game::rules::{JudgeClass, JudgeEvent, Judgement};
use wacca_render::effects::hold_arc_at;
use wacca_render::layout::Rect;
use wacca_render::{
    Effects, JudgeSprite, LiveState, MeshVertex, NoteMeshes, RimState, SceneOptions, SpriteSheet,
    Stage, Vertex, build_mesh_notes, build_scene_into, build_stage_into,
};
use wacca_video::{Frame, VideoPlayer};

use crate::hitsound::{self, Cue, HoldStarts};
use crate::hud::Hud;
use crate::labels;
use crate::session::{Autoplay, Session, chart_now, end_time, start_position};
use crate::touch_mapping::{Contacts, sensor_at_pixel};

#[derive(Debug, thiserror::Error)]
pub enum PlayError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("chart {}: {source}", path.display())]
    Chart {
        path: PathBuf,
        source: wacca_chart::ParseError,
    },
    #[error("audio: {0}")]
    Audio(#[from] wacca_audio::Error),
}

/// What to play.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaySpec {
    pub title: String,
    pub jacket: String,
    pub song_id: Option<u32>,
    pub difficulty: Option<Difficulty>,
    pub level: Option<f32>,
    pub chart_path: PathBuf,
    pub norma: f32,
    /// Background movie (`.usm`), if the chart has one.
    pub movie: Option<PathBuf>,
}

/// Player-chosen options for one play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayOptions {
    pub note_speed: f64,
    /// Seconds added to chart time (from the judge-offset setting, see
    /// [`crate::options::offset_secs`]).
    pub offset: f64,
    pub autoplay: bool,
    /// Play the left-right mirrored chart (judging and drawing alike).
    pub mirror: bool,
    pub bonus_effect: bool,
    /// Measure lines on the playfield.
    pub bar_lines: bool,
    /// Depth-ring guides and pressed-sensor highlight (touchscreen aid).
    pub touch_guide: bool,
}

/// What [`PlayState::new`] needs to show the background movie.
#[derive(Clone, Copy, Debug)]
pub struct VideoSetup<'a> {
    pub ffmpeg: &'a std::path::Path,
    /// Decode size (the playfield's side in pixels).
    pub size: u32,
}

/// `<assets>/Movie/<name>.usm` for a catalog movie name (`None`, `""` and `"-"` = no movie).
pub fn movie_path(assets: &std::path::Path, name: Option<&str>) -> Option<PathBuf> {
    name.filter(|n| !n.is_empty() && *n != "-")
        .map(|n| assets.join("Movie").join(format!("{n}.usm")))
}

pub fn load_chart(path: &PathBuf) -> Result<Chart, PlayError> {
    let bytes = std::fs::read(path).map_err(|source| PlayError::Io {
        path: path.clone(),
        source,
    })?;
    wacca_chart::parse(&bytes).map_err(|source| PlayError::Chart {
        path: path.clone(),
        source,
    })
}

#[derive(Debug)]
pub struct PlayState {
    pub spec: PlaySpec,
    pub opts: PlayOptions,
    /// As played (mirrored when [`PlayOptions::mirror`] is set).
    chart: Chart,
    /// The app's one output device, shared with the song-select preview.
    player: Arc<Player>,
    session: Session,
    autoplay: Option<Autoplay>,
    holds: HoldStarts,
    hits: Option<HitSounds>,
    /// Sustain voices of holds being held, by note index.
    hold_voices: HashMap<usize, Vec<VoiceId>>,
    contacts: Contacts,
    /// Hit effects, key beams and hold glows, advanced in chart time.
    effects: Effects,
    /// Chart time the effects were last advanced to.
    effects_at: f64,
    /// Holds started and not yet judged (note indices): their sustain glow shows.
    holding: Vec<usize>,
    /// Chart time of the last combo change (count animation).
    combo_at: f64,
    /// Sensors of the last input frame (touch guide).
    pressed: [u64; DEPTHS],
    /// Chart time at which the play is over.
    end: f64,
    pub paused: bool,
    video: Option<VideoPlayer>,
    /// Index of the movie frame last handed out by [`PlayState::video_frame`].
    video_shown: Option<u64>,
}

impl PlayState {
    /// Loads the song into `player` (replacing whatever it played). `hits(rate)` supplies the
    /// hit sounds at the output rate (or `None` to play silently).
    pub fn new(
        spec: PlaySpec,
        opts: PlayOptions,
        player: Arc<Player>,
        bgm: &BgmBank,
        hits: impl FnOnce(u32) -> Option<HitSounds>,
        video: Option<VideoSetup>,
    ) -> Result<Self, PlayError> {
        let chart = load_chart(&spec.chart_path)?;
        let chart = if opts.mirror {
            wacca_chart::mirrored(&chart)
        } else {
            chart
        };
        let cue = &chart.header.music_file_path;
        player.load(Box::new(bgm.open_cue(cue)?))?;
        let end = end_time(&chart, player.duration(), opts.offset);
        log::info!(
            "{}: cue {cue}, {} notes, mirror {}, play ends at chart {end:.2} s",
            spec.chart_path.display(),
            chart.notes.len(),
            opts.mirror
        );
        let hits = hits(player.sample_rate());
        let video = video.zip(spec.movie.as_deref()).and_then(|(v, path)| {
            VideoPlayer::open(path, v.ffmpeg, Some(v.size))
                .map_err(|e| log::warn!("background movie: {e}; playing without it"))
                .ok()
        });
        Ok(Self {
            session: Session::new(&chart, opts.bonus_effect),
            autoplay: opts.autoplay.then(|| Autoplay::new(&chart)),
            holds: HoldStarts::new(&chart),
            spec,
            opts,
            chart,
            player,
            hits,
            hold_voices: HashMap::new(),
            contacts: Contacts::default(),
            effects: Effects::default(),
            effects_at: f64::NEG_INFINITY,
            holding: Vec::new(),
            combo_at: f64::NEG_INFINITY,
            pressed: [0; DEPTHS],
            end,
            paused: false,
            video,
            video_shown: None,
        })
    }

    /// Start the music (with lead-in silence before the first note).
    pub fn start(&mut self) -> Result<(), PlayError> {
        let start = start_position(&self.chart, self.opts.offset);
        self.player.seek(start)?;
        self.player.play();
        log::info!("playing from audio {start:.3} s");
        Ok(())
    }

    pub fn now(&self) -> f64 {
        chart_now(&self.chart, self.player.position(), self.opts.offset)
    }

    /// Movie time: chart time minus `#MOVIEOFFSET`.
    // UNVERIFIED: same relation as `#OFFSET` (01-chart-format §5.2); the user judgement offset is not applied.
    fn movie_time(&self) -> f64 {
        self.chart.chart_time(self.player.position()) - self.chart.header.movie_offset
    }

    /// The background-movie frame to show now (`None` = no movie on screen) and whether it is new since the
    /// last call (only new frames need uploading).
    pub fn video_frame(&mut self) -> Option<(&Frame, bool)> {
        let t = self.movie_time();
        let frame = self.video.as_mut().and_then(|v| v.frame_at(t));
        let previous = std::mem::replace(&mut self.video_shown, frame.map(|f| f.index));
        frame.map(|f| (f, previous != Some(f.index)))
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if self.paused {
            self.player.pause();
        } else {
            self.player.play();
        }
        log::info!("paused: {} at chart {:.3}", self.paused, self.now());
    }

    pub fn change_speed(&mut self, step: f64) {
        self.opts.note_speed = (self.opts.note_speed + step).clamp(1.0, 6.0);
        log::info!("note speed {:.1}", self.opts.note_speed);
    }

    fn play_cue(&self, cue: Cue, gain: f32) -> Vec<VoiceId> {
        let Some(h) = &self.hits else {
            return Vec::new();
        };
        cue.sounds(h)
            .iter()
            .filter_map(|s| {
                let id = self
                    .player
                    .play_sfx(s, gain)
                    .map_err(|e| log::warn!("hit sound: {e}"))
                    .ok()?;
                s.loop_range.map(|_| id)
            })
            .collect()
    }

    fn stop_voices(&self, voices: Vec<VoiceId>) {
        for v in voices {
            if let Err(e) = self.player.stop_sfx(v) {
                log::warn!("hit sound: {e}");
            }
        }
    }

    fn feed(&mut self, frame: &InputFrame) {
        for (note, j) in self.holds.update(frame.time, &frame.sensors) {
            let bonus = self.chart.notes[note].bonus;
            let voices = self.play_cue(hitsound::hold_cue(j), hitsound::gain(bonus));
            self.hold_voices.insert(note, voices);
            // The hold head pops like a tap; its body glows until the hold is judged.
            let arc = self.chart.notes[note].arc;
            self.effects.hit(arc.lane, arc.width, judge_sprite(j));
            self.holding.push(note);
        }
        let combo = self.session.combo();
        let events = self.session.feed(frame);
        if self.session.combo() != combo {
            self.combo_at = frame.time;
        }
        self.pressed = frame.sensors.rings;
        self.effects
            .touch(frame.sensors.rings.iter().fold(0, |a, r| a | r));
        for e in &events {
            self.holding.retain(|&n| n != e.note);
            if let Some(arc) = hit_arc(&self.chart, e) {
                self.effects
                    .hit(arc.lane, arc.width, judge_sprite(e.judgement));
            }
        }
        self.sound(&events);
    }

    /// Advance the effects to chart time `now` (at most 0.1 s per frame, never backwards).
    fn advance_effects(&mut self, now: f64) {
        let dt = (now - self.effects_at).clamp(0.0, 0.1);
        self.effects_at = now;
        let arcs = self
            .holding
            .iter()
            .filter_map(|&n| match &self.chart.notes.get(n)?.body {
                NoteBody::Hold { nodes } => hold_arc_at(nodes, now),
                _ => None,
            })
            .collect();
        self.effects.set_holds(arcs);
        self.effects.update(dt as f32);
    }

    fn sound(&mut self, events: &[JudgeEvent]) {
        for e in events {
            if e.class == JudgeClass::Hold {
                if let Some(v) = self.hold_voices.remove(&e.note) {
                    self.stop_voices(v);
                }
            } else if let Some(cue) = hitsound::cue_for(e.class, e.judgement) {
                self.play_cue(cue, hitsound::gain(e.bonus));
            }
        }
    }

    fn feed_contacts(&mut self) {
        if self.paused || self.autoplay.is_some() {
            return;
        }
        let frame = InputFrame {
            time: self.now(),
            sensors: self.contacts.sensors(),
        };
        self.feed(&frame);
    }

    /// Contact `id` moved to window pixel `at` (`None` = lifted) over the playfield `vp`.
    pub fn pointer(&mut self, id: u64, at: Option<[f32; 2]>, vp: Rect) {
        self.contacts
            .set(id, at.and_then(|p| sensor_at_pixel(p, vp)));
        self.feed_contacts();
    }

    /// Advance one rendered frame; returns the result once the song is over.
    pub fn frame(&mut self) -> Option<PlayResult> {
        if self.paused {
            return None;
        }
        let now = self.now();
        match &mut self.autoplay {
            Some(auto) => {
                let due = auto.due(now).to_vec();
                for f in &due {
                    self.feed(f);
                }
            }
            None => self.feed_contacts(),
        }
        self.advance_effects(now);
        if now < self.end {
            return None;
        }
        log::info!("song over at chart {now:.3}");
        let r = self.session.finish(self.spec.norma);
        self.stop_all();
        Some(r)
    }

    pub fn give_up(&mut self) -> PlayResult {
        log::info!("given up");
        self.stop_all();
        self.session.give_up(self.spec.norma)
    }

    fn stop_all(&mut self) {
        self.video = None;
        self.video_shown = None;
        let voices: Vec<_> = self.hold_voices.drain().flat_map(|(_, v)| v).collect();
        self.stop_voices(voices);
        self.player.pause();
    }

    /// The playfield now, into `out` (2D, over the 3D view) and `stage` (guide lanes, hold
    /// ribbons, chevrons); with `meshes` the notes go to `notes` (3D). Buffers are reused.
    pub fn scene(
        &self,
        sprites: &SpriteSheet,
        meshes: Option<&NoteMeshes>,
        out: &mut Vec<Vertex>,
        notes: &mut Vec<MeshVertex>,
        stage: &mut Stage,
    ) {
        let now = self.now();
        let judgement = self
            .session
            .last_event()
            .map(|e| (judge_sprite(e.judgement), (now - e.time) as f32));
        let opts = SceneOptions {
            note_speed: self.opts.note_speed,
            movie: self.video_shown.is_some(),
            bar_lines: self.opts.bar_lines,
            touch_guide: self.opts.touch_guide,
            mesh_notes: meshes.is_some(),
        };
        let live = LiveState {
            judged: self.session.judged(),
            effects: Some(&self.effects),
            judgement,
            combo: (self.session.combo(), (now - self.combo_at) as f32),
            rim: Some(RimState {
                score: self.session.score(),
                progress: progress(now, self.end),
                difficulty_color: labels::difficulty_color(
                    self.spec.difficulty.unwrap_or(Difficulty::Normal),
                ),
            }),
            gauge: Some((self.session.gauge() as f32, self.spec.norma)),
            pressed: self.pressed,
        };
        notes.clear();
        if let Some(meshes) = meshes {
            build_mesh_notes(notes, meshes, &self.chart, now, &opts, &live);
        }
        build_stage_into(stage, &self.chart, now, &opts, &live, meshes);
        build_scene_into(out, &self.chart, now, &opts, &live, sprites);
    }

    /// `art`: the playfield draws the judgement word art and the combo counter (no text).
    pub fn hud(&self, art: bool) -> Hud {
        Hud {
            score: self.session.score(),
            combo: self.session.combo(),
            last: self.session.last_event(),
            now: self.now(),
            note_speed: self.opts.note_speed,
            paused: self.paused,
            autoplay: self.autoplay.is_some(),
            art,
        }
    }
}

/// Played fraction of a song that ends at chart time `end` (the rim's progress arc).
pub fn progress(now: f64, end: f64) -> f32 {
    if end > 0.0 {
        (now / end).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// Difficulty and level as written along the rim, e.g. `NORMAL/Lv.1`.
pub fn rim_difficulty(spec: &PlaySpec) -> String {
    let name = labels::difficulty(spec.difficulty.unwrap_or(Difficulty::Normal));
    match spec.level {
        Some(l) => format!("{name}/Lv.{}", labels::level(l)),
        None => name.to_string(),
    }
}

pub fn judge_sprite(j: Judgement) -> JudgeSprite {
    match j {
        Judgement::Marvelous => JudgeSprite::Marvelous,
        Judgement::Great => JudgeSprite::Great,
        Judgement::Good => JudgeSprite::Good,
        Judgement::Miss => JudgeSprite::Miss,
    }
}

/// Arc where the hit effect for `e` shows: the note's arc. Holds have none: their head pops
/// when it is pressed, and the E2 frames show nothing when a hold is judged at its end.
pub fn hit_arc(chart: &Chart, e: &JudgeEvent) -> Option<LaneArc> {
    match e.class {
        JudgeClass::Hold | JudgeClass::HoldEnd => None,
        _ => chart.notes.get(e.note).map(|n| n.arc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use wacca_chart::{BeatPos, Bonus, ChartHeader, HoldNode, Note};

    #[test]
    fn hit_effects_cover_the_judged_arc() {
        let node = |lane| HoldNode {
            pos: BeatPos::default(),
            time: 0.0,
            arc: LaneArc { lane, width: 4 },
            lane_unwrapped: f32::from(lane),
            control: true,
        };
        let chart = Chart {
            header: ChartHeader::default(),
            notes: vec![Note {
                pos: BeatPos::default(),
                time: 1.0,
                arc: LaneArc { lane: 10, width: 4 },
                bonus: Bonus::None,
                body: NoteBody::Hold {
                    nodes: vec![node(10), node(20)],
                },
                file_index: 0,
            }],
            masks: vec![],
            tempo: vec![],
            scroll: vec![],
            reverses: vec![],
            tutorial_tags: vec![],
            end: BeatPos::default(),
            end_time: 5.0,
        };
        let e = |class| JudgeEvent {
            note: 0,
            class,
            bonus: Bonus::None,
            judgement: Judgement::Great,
            offset: None,
            time: 1.0,
        };
        // Holds show nothing when judged (their head popped when pressed).
        assert_eq!(hit_arc(&chart, &e(JudgeClass::Hold)), None);
        assert_eq!(hit_arc(&chart, &e(JudgeClass::HoldEnd)), None);
        let a = hit_arc(&chart, &e(JudgeClass::Touch)).unwrap();
        assert_eq!((a.lane, a.width), (10, 4));
        // Unknown note index: no effect, not a panic.
        let mut bad = e(JudgeClass::Touch);
        bad.note = 7;
        assert_eq!(hit_arc(&chart, &bad), None);
    }

    #[test]
    fn rim_texts_and_progress() {
        let spec = PlaySpec {
            title: "Fragile".into(),
            jacket: String::new(),
            song_id: None,
            difficulty: Some(Difficulty::Normal),
            level: Some(1.0),
            chart_path: PathBuf::new(),
            norma: 0.5,
            movie: None,
        };
        assert_eq!(rim_difficulty(&spec), "NORMAL/Lv.1");
        let hard = PlaySpec {
            difficulty: Some(Difficulty::Expert),
            level: Some(12.7),
            ..spec.clone()
        };
        assert_eq!(rim_difficulty(&hard), "EXPERT/Lv.12+");
        assert_eq!(
            rim_difficulty(&PlaySpec {
                level: None,
                ..spec
            }),
            "NORMAL"
        );
        assert_eq!(progress(20.0, 100.0), 0.2);
        assert_eq!(progress(-3.0, 100.0), 0.0);
        assert_eq!(progress(150.0, 100.0), 1.0);
        assert_eq!(progress(1.0, 0.0), 0.0);
    }

    #[test]
    fn movie_paths() {
        let a = Path::new("assets");
        assert_eq!(
            movie_path(a, Some("mv_S02_204")),
            Some(a.join("Movie").join("mv_S02_204.usm"))
        );
        assert_eq!(movie_path(a, Some("-")), None);
        assert_eq!(movie_path(a, Some("")), None);
        assert_eq!(movie_path(a, None), None);
    }
}
