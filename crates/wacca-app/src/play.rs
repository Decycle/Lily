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
use wacca_chart::{Chart, Difficulty};
use wacca_game::grade::PlayResult;
use wacca_game::input::InputFrame;
use wacca_game::rules::{JudgeClass, JudgeEvent};
use wacca_render::layout::Rect;
use wacca_render::{LiveState, SceneOptions, Vertex, build_scene};
use wacca_video::{Frame, VideoPlayer};

use crate::hitsound::{self, Cue, HoldStarts};
use crate::hud::Hud;
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
        }
        let events = self.session.feed(frame);
        self.sound(&events);
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

    pub fn scene(&self) -> Vec<Vertex> {
        let touched = if self.autoplay.is_some() {
            [0; 4]
        } else {
            self.contacts.sensors().rings
        };
        build_scene(
            &self.chart,
            self.now(),
            &SceneOptions {
                note_speed: self.opts.note_speed,
                movie: self.video_shown.is_some(),
            },
            &LiveState {
                judged: self.session.judged(),
                touched,
            },
        )
    }

    pub fn hud(&self) -> Hud {
        Hud {
            score: self.session.score(),
            combo: self.session.combo(),
            last: self.session.last_event(),
            now: self.now(),
            note_speed: self.opts.note_speed,
            paused: self.paused,
            autoplay: self.autoplay.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
