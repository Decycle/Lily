//! Video capture of the game's client rect through an external `ffmpeg` process.
//!
//! Prefers `ddagrab` (Desktop Duplication) when a 1-frame probe works on this PC, else `gdigrab` (measured in
//! doc 07 §4.3: ddagrab keeps every one of the game's ~60 frames/s, gdigrab ~46 unique frames/s). ddagrab runs at
//! 120 fps without duplicate padding (`dup_frames=0`) and the output keeps the capture timestamps
//! (`-fps_mode passthrough`), because at 60 fps it misses game frames on this 120 Hz display. On hybrid-graphics
//! laptops Desktop Duplication only works from the GPU that drives the display; FFmpeg is therefore started with
//! `SHIM_MCCOMPAT=0x800000000` (NVIDIA Optimus: run this process on the integrated GPU). Encodes lossless RGB
//! H.264 (`libx264rgb -preset ultrafast -qp 0`) into MKV.
//!
//! Alignment: both grabbers stamp frames with the wall clock (gdigrab natively, ddagrab via
//! `-use_wallclock_as_timestamps`) and FFmpeg prints the first one as the input `start:` (Unix seconds); frame
//! `pts` are relative to it. We also note the [`Instant`] of the first `frame=` progress line
//! (`-stats_period 0.05`, so ±50 ms). Stopped gracefully by sending `q` on stdin.

use crate::game::Rect;
use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VideoError {
    #[error(transparent)]
    Find(#[from] wacca_video::ffmpeg::FindError),
    #[error("starting ffmpeg: {0}")]
    Spawn(std::io::Error),
    #[error("ffmpeg exited before the first frame; last output:\n{0}")]
    NoFrames(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Grabber {
    Ddagrab,
    Gdigrab,
}

/// ffmpeg discovery is shared with the game (`wacca_video::ffmpeg`).
pub use wacca_video::ffmpeg::find_ffmpeg;

/// Makes Desktop Duplication work on hybrid-graphics laptops (see module docs); harmless elsewhere.
const GPU_ENV: (&str, &str) = ("SHIM_MCCOMPAT", "0x800000000");

fn ffmpeg_command(ffmpeg: &Path) -> Command {
    let mut c = Command::new(ffmpeg);
    c.env(GPU_ENV.0, GPU_ENV.1);
    c
}

/// Input-side arguments for the grabber.
fn input_args(grabber: Grabber, r: Rect) -> Vec<String> {
    let size = format!("{}x{}", r.width, r.height);
    match grabber {
        Grabber::Gdigrab => [
            "-f",
            "gdigrab",
            "-framerate",
            "60",
            "-offset_x",
            &r.x.to_string(),
            "-offset_y",
            &r.y.to_string(),
            "-video_size",
            &size,
            "-draw_mouse",
            "0",
            "-i",
            "desktop",
        ]
        .map(String::from)
        .to_vec(),
        Grabber::Ddagrab => vec![
            "-use_wallclock_as_timestamps".into(),
            "1".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!(
                "ddagrab=output_idx=0:framerate=120:dup_frames=0:draw_mouse=0:video_size={size}:offset_x={}:offset_y={},hwdownload,format=bgra",
                r.x, r.y
            ),
        ],
    }
}

/// Full argument list for a recording of `rect` into `out`.
pub fn record_args(grabber: Grabber, rect: Rect, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-y", "-stats_period", "0.05"]
        .map(String::from)
        .to_vec();
    a.extend(input_args(grabber, rect));
    a.extend(
        [
            "-fps_mode",
            "passthrough",
            "-c:v",
            "libx264rgb",
            "-preset",
            "ultrafast",
            "-qp",
            "0",
        ]
        .map(String::from),
    );
    a.push(out.to_string_lossy().into_owned());
    a
}

/// Arguments for a 1-frame probe that fails if the grabber cannot capture `rect` here.
pub fn probe_args(grabber: Grabber, rect: Rect) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error"]
        .map(String::from)
        .to_vec();
    a.extend(input_args(grabber, rect));
    a.extend(["-frames:v", "1", "-f", "null", "-"].map(String::from));
    a
}

/// Arguments for grabbing one frame as raw RGB24 on stdout and, at the same time, as a PNG file.
pub fn grab_args(grabber: Grabber, rect: Rect, png: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y"]
        .map(String::from)
        .to_vec();
    a.extend(input_args(grabber, rect));
    a.extend(
        [
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ]
        .map(String::from),
    );
    a.extend(["-frames:v", "1"].map(String::from));
    a.push(png.to_string_lossy().into_owned());
    a
}

/// Grabs one frame of `rect` (see [`grab_args`]).
pub fn grab_frame(
    ffmpeg: &Path,
    grabber: Grabber,
    rect: Rect,
    png: &Path,
) -> Result<crate::frame::Rgb, VideoError> {
    let out = ffmpeg_command(ffmpeg)
        .args(grab_args(grabber, rect, png))
        .stdin(Stdio::null())
        .output()
        .map_err(VideoError::Spawn)?;
    crate::frame::Rgb::new(rect.width as usize, rect.height as usize, out.stdout)
        .ok_or_else(|| VideoError::NoFrames(String::from_utf8_lossy(&out.stderr).into_owned()))
}

/// Arguments for decoding `video` to raw frames of `pix_fmt` on stdout, every captured frame once, with a
/// `showinfo` line (see [`parse_showinfo_pts`]) per frame on stderr.
pub fn decode_args(video: &Path, pix_fmt: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-nostats", "-i"]
        .map(String::from)
        .to_vec();
    a.push(video.to_string_lossy().into_owned());
    a.extend(
        [
            "-fps_mode",
            "passthrough",
            "-vf",
            "showinfo",
            "-f",
            "rawvideo",
            "-pix_fmt",
            pix_fmt,
            "pipe:1",
        ]
        .map(String::from),
    );
    a
}

/// Spawns ffmpeg decoding `video` (see [`decode_args`]) with stdout and stderr piped.
pub fn spawn_decoder(ffmpeg: &Path, video: &Path, pix_fmt: &str) -> Result<Child, VideoError> {
    ffmpeg_command(ffmpeg)
        .args(decode_args(video, pix_fmt))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(VideoError::Spawn)
}

/// `[Parsed_showinfo_0 @ 0x..] n:   3 pts:     25 pts_time:0.025   duration:..` -> 0.025.
pub fn parse_showinfo_pts(line: &str) -> Option<f64> {
    if !line.contains("Parsed_showinfo") {
        return None;
    }
    let rest = &line[line.find("pts_time:")? + "pts_time:".len()..];
    rest.split_whitespace().next()?.parse().ok()
}

/// `Duration: N/A, start: 1791798777.721283, bitrate: ...` -> the start time.
pub fn parse_input_start(line: &str) -> Option<f64> {
    let line = line.trim_start();
    if !line.starts_with("Duration:") {
        return None;
    }
    let rest = &line[line.find("start:")? + "start:".len()..];
    rest.split(',').next()?.trim().parse().ok()
}

/// `frame=   12 fps=...` -> 12.
pub fn parse_progress_frame(line: &str) -> Option<u64> {
    let rest = line.trim_start().strip_prefix("frame=")?;
    rest.split_whitespace().next()?.parse().ok()
}

/// Picks ddagrab if a probe capture succeeds, else gdigrab.
pub fn choose_grabber(ffmpeg: &Path, rect: Rect) -> Grabber {
    let probe = ffmpeg_command(ffmpeg)
        .args(probe_args(Grabber::Ddagrab, rect))
        .stdin(Stdio::null())
        .output();
    match probe {
        Ok(o) if o.status.success() => Grabber::Ddagrab,
        Ok(o) => {
            log::info!(
                "ddagrab unavailable, using gdigrab: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            Grabber::Gdigrab
        }
        Err(e) => {
            log::warn!("ddagrab probe: {e}");
            Grabber::Gdigrab
        }
    }
}

#[derive(Debug, Default)]
struct StderrState {
    input_start_unix_s: Option<f64>,
    first_progress: Option<(Instant, u64)>,
    last_frame: u64,
    tail: Vec<String>,
}

impl StderrState {
    fn line(&mut self, line: &str, at: Instant) {
        if let Some(s) = parse_input_start(line) {
            self.input_start_unix_s.get_or_insert(s);
        }
        if let Some(n) = parse_progress_frame(line) {
            self.last_frame = n;
            if n > 0 && self.first_progress.is_none() {
                self.first_progress = Some((at, n));
            }
        } else {
            log::debug!("ffmpeg: {line}");
        }
        self.tail.push(line.to_owned());
        if self.tail.len() > 20 {
            self.tail.remove(0);
        }
    }
}

#[derive(Debug)]
pub struct VideoRecorder {
    child: Child,
    stdin: Option<ChildStdin>,
    state: Arc<Mutex<StderrState>>,
    reader: Option<std::thread::JoinHandle<()>>,
    pub grabber: Grabber,
    pub started: Instant,
}

/// What a finished recording reports; times are `Instant`s for the caller to map onto its epoch.
#[derive(Debug)]
pub struct VideoResult {
    pub grabber: Grabber,
    pub started: Instant,
    pub input_start_unix_s: Option<f64>,
    /// When the first `frame=` line with n > 0 was read, and that n.
    pub first_progress: Option<(Instant, u64)>,
    pub frames: u64,
    pub exit_ok: bool,
}

impl VideoRecorder {
    /// Starts ffmpeg and waits (up to `first_frame_timeout`) until it reports its first encoded frame.
    pub fn start(
        ffmpeg: &Path,
        grabber: Grabber,
        rect: Rect,
        out: &Path,
        first_frame_timeout: Duration,
    ) -> Result<Self, VideoError> {
        let args = record_args(grabber, rect, out);
        log::info!("ffmpeg {}", args.join(" "));
        let started = Instant::now();
        let mut child = ffmpeg_command(ffmpeg)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(VideoError::Spawn)?;
        let state = Arc::new(Mutex::new(StderrState::default()));
        let reader = child.stderr.take().map(|err| {
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                // ffmpeg ends progress lines with '\r'; split on both.
                for chunk in BufReader::new(err).split(b'\r') {
                    let Ok(chunk) = chunk else { break };
                    let at = Instant::now();
                    for line in String::from_utf8_lossy(&chunk).lines() {
                        if !line.trim().is_empty()
                            && let Ok(mut s) = state.lock()
                        {
                            s.line(line, at);
                        }
                    }
                }
            })
        });
        let mut rec = Self {
            stdin: child.stdin.take(),
            child,
            state,
            reader,
            grabber,
            started,
        };
        let deadline = Instant::now() + first_frame_timeout;
        loop {
            if rec.state.lock().is_ok_and(|s| s.first_progress.is_some()) {
                return Ok(rec);
            }
            let exited = matches!(rec.child.try_wait(), Ok(Some(_)));
            if exited || Instant::now() >= deadline {
                let _ = rec.child.kill();
                let _ = rec.child.wait();
                if let Some(r) = rec.reader.take() {
                    let _ = r.join();
                }
                let tail = rec
                    .state
                    .lock()
                    .map(|s| s.tail.join("\n"))
                    .unwrap_or_default();
                return Err(VideoError::NoFrames(tail));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Sends `q`, waits up to 15 s for ffmpeg to finalise the file, kills it otherwise.
    pub fn stop(mut self) -> VideoResult {
        if let Some(mut stdin) = self.stdin.take()
            && let Err(e) = stdin.write_all(b"q").and_then(|()| stdin.flush())
        {
            log::warn!("ffmpeg stdin: {e}");
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut exit_ok = false;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    exit_ok = status.success();
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Ok(None) | Err(_) => {
                    log::warn!("ffmpeg did not stop on 'q'; killing it");
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        let s = self.state.lock().map(|s| {
            if !exit_ok {
                log::warn!("ffmpeg output:\n{}", s.tail.join("\n"));
            }
            (s.input_start_unix_s, s.first_progress, s.last_frame)
        });
        let (input_start_unix_s, first_progress, frames) = s.unwrap_or_default();
        VideoResult {
            grabber: self.grabber,
            started: self.started,
            input_start_unix_s,
            first_progress,
            frames,
            exit_ok,
        }
    }
}

impl Drop for VideoRecorder {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: Rect = Rect {
        x: 622,
        y: 0,
        width: 675,
        height: 1200,
    };

    #[test]
    fn gdigrab_args() {
        let a = record_args(Grabber::Gdigrab, RECT, Path::new("runs/x/video.mkv"));
        let s = a.join(" ");
        assert_eq!(
            s,
            "-hide_banner -y -stats_period 0.05 -f gdigrab -framerate 60 -offset_x 622 -offset_y 0 \
             -video_size 675x1200 -draw_mouse 0 -i desktop -fps_mode passthrough -c:v libx264rgb \
             -preset ultrafast -qp 0 runs/x/video.mkv"
        );
    }

    #[test]
    fn ddagrab_args() {
        let a = probe_args(Grabber::Ddagrab, RECT);
        assert!(a.contains(
            &"ddagrab=output_idx=0:framerate=120:dup_frames=0:draw_mouse=0:video_size=675x1200:offset_x=622:offset_y=0,hwdownload,format=bgra"
                .to_string()
        ));
        assert_eq!(a[3..5], ["-use_wallclock_as_timestamps", "1"]);
        assert_eq!(a[a.len() - 5..], ["-frames:v", "1", "-f", "null", "-"]);
    }

    #[test]
    fn grab_and_decode_args() {
        let g = grab_args(Grabber::Gdigrab, RECT, Path::new("nav.png")).join(" ");
        assert!(
            g.ends_with("-frames:v 1 -f rawvideo -pix_fmt rgb24 pipe:1 -frames:v 1 nav.png"),
            "{g}"
        );
        let d = decode_args(Path::new("v.mkv"), "gray").join(" ");
        assert_eq!(
            d,
            "-hide_banner -nostats -i v.mkv -fps_mode passthrough -vf showinfo -f rawvideo -pix_fmt gray pipe:1"
        );
    }

    #[test]
    fn parses_showinfo() {
        let l =
            "[Parsed_showinfo_0 @ 000001] n:   3 pts:     25 pts_time:0.025   duration:8 fmt:gray";
        assert_eq!(parse_showinfo_pts(l), Some(0.025));
        assert_eq!(
            parse_showinfo_pts("[Parsed_showinfo_0 @ 1] config in time_base: 1/1000"),
            None
        );
        assert_eq!(parse_showinfo_pts("frame=1 pts_time:3"), None);
    }

    #[test]
    fn parses_stderr() {
        assert_eq!(
            parse_input_start("  Duration: N/A, start: 1791798777.721283, bitrate: 76825 kb/s"),
            Some(1_791_798_777.721283)
        );
        assert_eq!(
            parse_input_start("  Stream #0:0: Video: bmp, start 1791798777.721283"),
            None
        );
        assert_eq!(
            parse_progress_frame("frame=   12 fps=0.0 q=0.0 size=       0KiB time=00:00:00.18"),
            Some(12)
        );
        assert_eq!(parse_progress_frame("Press [q] to stop"), None);
    }

    #[test]
    fn stderr_state_keeps_first_frame() {
        let mut s = StderrState::default();
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(50);
        s.line("  Duration: N/A, start: 10.5, bitrate: 1 kb/s", t0);
        s.line("frame=    0 fps=0.0", t0);
        s.line("frame=    3 fps=0.0", t1);
        s.line("frame=    6 fps=0.0", t1 + Duration::from_millis(50));
        assert_eq!(s.input_start_unix_s, Some(10.5));
        assert_eq!(s.first_progress, Some((t1, 3)));
        assert_eq!(s.last_frame, 6);
    }
}
