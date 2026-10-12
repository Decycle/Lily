//! `runs/<ts>/meta.json`: every clock anchor of a run, all times in ms since the run's [`super::Epoch`].

use super::audio::AudioResult;
use super::video::{Grabber, VideoResult};
use super::{Epoch, EpochAnchor};
use crate::game::Rect;
use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
pub struct Meta {
    pub git_commit: Option<String>,
    pub epoch: EpochAnchor,
    pub window: Option<Rect>,
    pub video: Option<VideoMeta>,
    pub audio: Option<AudioMeta>,
    pub inject: Option<InjectMeta>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct VideoMeta {
    pub file: String,
    pub grabber: Grabber,
    pub ffmpeg_started_ms: f64,
    /// First frame's capture time from FFmpeg's wall clock (gdigrab `start:`), mapped to the epoch.
    pub first_frame_ms: Option<f64>,
    /// When the first `frame=` progress line arrived, and the frame count it showed.
    pub first_progress_ms: Option<f64>,
    pub first_progress_frame: Option<u64>,
    pub frames: u64,
    pub exit_ok: bool,
}

impl VideoMeta {
    pub fn new(file: &str, v: &VideoResult, e: &Epoch) -> Self {
        Self {
            file: file.into(),
            grabber: v.grabber,
            ffmpeg_started_ms: e.ms(v.started),
            first_frame_ms: v.input_start_unix_s.map(|s| e.unix_to_ms(s)),
            first_progress_ms: v.first_progress.map(|(t, _)| e.ms(t)),
            first_progress_frame: v.first_progress.map(|(_, n)| n),
            frames: v.frames,
            exit_ok: v.exit_ok,
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct AudioMeta {
    pub file: String,
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    /// When the first loopback buffer arrived (its first sample is roughly one buffer earlier).
    pub first_buffer_ms: Option<f64>,
    pub peak: f32,
    pub rms: f32,
}

impl AudioMeta {
    pub fn new(file: &str, device: &str, a: &AudioResult, e: &Epoch) -> Self {
        Self {
            file: file.into(),
            device: device.into(),
            sample_rate: a.sample_rate,
            channels: a.channels,
            frames: a.frames,
            first_buffer_ms: a.first_buffer.map(|t| e.ms(t)),
            peak: a.level.peak,
            rms: a.level.rms(),
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct InjectMeta {
    pub script: String,
    pub live: bool,
    /// Epoch ms of the injector clock's 0 (the `t_*_ms` fields of `inject.jsonl` are relative to it).
    pub clock_start_ms: f64,
    /// Epoch ms of script time 0 (`clock_start_ms` + start delay).
    pub script_t0_ms: f64,
    pub taps_sent: usize,
    pub taps_total: usize,
    pub publishes: usize,
    pub acked: usize,
    pub log: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::audio::Level;
    use std::time::Duration;

    #[test]
    fn json_shape() {
        let e = Epoch::now();
        let v = VideoResult {
            grabber: Grabber::Gdigrab,
            started: e.instant + Duration::from_millis(10),
            input_start_unix_s: Some(e.anchor.unix_s + 0.1),
            first_progress: Some((e.instant + Duration::from_millis(150), 3)),
            frames: 300,
            exit_ok: true,
        };
        let mut level = Level::default();
        level.add(&[0.5]);
        let a = AudioResult {
            sample_rate: 48_000,
            channels: 2,
            frames: 240_000,
            first_buffer: Some(e.instant + Duration::from_millis(20)),
            level,
        };
        let meta = Meta {
            git_commit: Some("abc".into()),
            epoch: e.anchor,
            window: Some(Rect {
                x: 622,
                y: 0,
                width: 675,
                height: 1200,
            }),
            video: Some(VideoMeta::new("video.mkv", &v, &e)),
            audio: Some(AudioMeta::new("audio.wav", "Speakers", &a, &e)),
            inject: None,
        };
        let j = serde_json::to_value(&meta).unwrap();
        let mut keys: Vec<&str> = j.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["audio", "epoch", "git_commit", "inject", "video", "window"]
        );
        assert_eq!(j["video"]["grabber"], "gdigrab");
        assert!((j["video"]["first_frame_ms"].as_f64().unwrap() - 100.0).abs() < 0.01);
        assert!((j["video"]["first_progress_ms"].as_f64().unwrap() - 150.0).abs() < 1e-6);
        assert_eq!(j["video"]["first_progress_frame"], 3);
        assert!((j["audio"]["first_buffer_ms"].as_f64().unwrap() - 20.0).abs() < 1e-6);
        assert_eq!(j["audio"]["sample_rate"], 48_000);
        assert_eq!(j["window"]["width"], 675);
        assert!(j["epoch"]["qpc_freq"].is_i64());
        assert!(j["inject"].is_null());
    }
}
