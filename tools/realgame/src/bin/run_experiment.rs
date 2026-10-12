//! One fully automatic real-game experiment: launch the game (unless running), record video (FFmpeg) + loopback
//! audio, play a tap script into the Waccon shared buffer, stop everything, write `runs/<ts>/`.
//!
//! `run_experiment --script <taps.json> [--nav <nav.json>] [--nav <nav.json>] [--ffmpeg <path>] [--keep-game] [--no-video]
//!  [--settle-s 30] [--lead-ms 1000] [--tail-ms 2000] [--out-dir runs] [--game-dir C:\Games\WACCA\bin]`
//!
//! `--nav` runs a navigation script ([`wacca_realgame::nav`], e.g. `scripts/nav-input-test.nav.json`) before
//! recording: operator buttons through the shared buffer, screen checks on grabbed frames (saved as
//! `nav-<step>-<name>.png`); a failed check aborts the run.
//!
//! Input is only written to the live buffer when `WACCA_INJECT_OK=1`; otherwise the script is a dry run (same
//! timing, nothing written) so recording can be tested safely. `RUST_LOG=debug` shows FFmpeg's output.
#![cfg_attr(test, allow(clippy::unwrap_used))]

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match imp::run(std::env::args().skip(1)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("run_experiment only runs on Windows");
}

#[cfg(windows)]
mod imp {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use thiserror::Error;
    use wacca_realgame::experiment;
    use wacca_realgame::game::{self, GameConfig, GameError, Launched};
    use wacca_realgame::inject::{self, Record};
    use wacca_realgame::nav::{self, NavScript, Step};
    use wacca_realgame::record::Epoch;
    use wacca_realgame::record::audio::{AudioError, AudioRecorder};
    use wacca_realgame::record::meta::{AudioMeta, InjectMeta, Meta, VideoMeta};
    use wacca_realgame::record::video::{self, VideoError, VideoRecorder};
    use wacca_realgame::{shm, timeline};

    const INJECT_ENV: &str = "WACCA_INJECT_OK";

    #[derive(Debug, Error)]
    pub enum Error {
        #[error(
            "{0}\nusage: run_experiment --script <taps.json> [--nav <nav.json>] [--ffmpeg <path>] [--keep-game] [--no-video] [--settle-s N] [--lead-ms N] [--tail-ms N] [--out-dir DIR] [--game-dir DIR]"
        )]
        Args(String),
        #[error("{}: {source}", path.display())]
        Io {
            path: PathBuf,
            source: std::io::Error,
        },
        #[error("script {}: {source}", path.display())]
        Script {
            path: PathBuf,
            source: serde_json::Error,
        },
        #[error(transparent)]
        Game(#[from] GameError),
        #[error(transparent)]
        Video(#[from] VideoError),
        #[error(transparent)]
        Audio(#[from] AudioError),
        #[error("shared buffer: {0}")]
        Shm(String),
        #[error("navigation: {0}")]
        Nav(String),
    }

    #[derive(Debug)]
    struct Args {
        script: PathBuf,
        nav: Option<PathBuf>,
        ffmpeg: Option<PathBuf>,
        keep_game: bool,
        no_video: bool,
        settle_s: f64,
        lead_ms: f64,
        tail_ms: f64,
        out_dir: PathBuf,
        game_dir: Option<PathBuf>,
    }

    fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args, Error> {
        let mut a = Args {
            script: PathBuf::new(),
            nav: None,
            ffmpeg: None,
            keep_game: false,
            no_video: false,
            settle_s: 30.0,
            lead_ms: 1000.0,
            tail_ms: 2000.0,
            out_dir: PathBuf::from("runs"),
            game_dir: None,
        };
        let num = |flag: &str, v: Option<String>| -> Result<f64, Error> {
            v.and_then(|v| v.parse().ok())
                .filter(|n: &f64| n.is_finite() && *n >= 0.0)
                .ok_or_else(|| Error::Args(format!("{flag} needs a non-negative number")))
        };
        let path = |flag: &str, v: Option<String>| -> Result<PathBuf, Error> {
            v.map(PathBuf::from)
                .ok_or_else(|| Error::Args(format!("{flag} needs a path")))
        };
        while let Some(flag) = it.next() {
            match flag.as_str() {
                "--script" => a.script = path(&flag, it.next())?,
                "--nav" => a.nav = Some(path(&flag, it.next())?),
                "--ffmpeg" => a.ffmpeg = Some(path(&flag, it.next())?),
                "--keep-game" => a.keep_game = true,
                "--no-video" => a.no_video = true,
                "--settle-s" => a.settle_s = num(&flag, it.next())?,
                "--lead-ms" => a.lead_ms = num(&flag, it.next())?,
                "--tail-ms" => a.tail_ms = num(&flag, it.next())?,
                "--out-dir" => a.out_dir = path(&flag, it.next())?,
                "--game-dir" => a.game_dir = Some(path(&flag, it.next())?),
                other => return Err(Error::Args(format!("unknown argument {other:?}"))),
            }
        }
        if a.script.as_os_str().is_empty() {
            return Err(Error::Args("--script is required".into()));
        }
        Ok(a)
    }

    /// Stops the game on every exit path unless `--keep-game`.
    struct GameGuard {
        cfg: GameConfig,
        launched: Option<Launched>,
        keep: bool,
    }

    impl Drop for GameGuard {
        fn drop(&mut self) {
            if self.keep {
                log::info!("--keep-game: leaving the game running");
                return;
            }
            game::stop(&self.cfg);
            if let Some(l) = self.launched.take() {
                l.reap();
            }
        }
    }

    fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
        move |source| Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    fn git_commit() -> Option<String> {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn tick_count_ms() -> u64 {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetTickCount64() -> u64;
        }
        // SAFETY: no arguments, cannot fail.
        unsafe { GetTickCount64() }
    }

    /// Opens the live buffer only with the opt-in env var, after the ABI and heartbeat checks (doc 07 §6).
    fn open_live() -> Result<Option<shm::SharedBuffer>, Error> {
        if std::env::var(INJECT_ENV).as_deref() != Ok("1") {
            log::warn!(
                "{INJECT_ENV} is not 1: dry run into an in-memory buffer, nothing is written to the game"
            );
            return Ok(None);
        }
        let buf = shm::SharedBuffer::open_existing(shm::NAME)
            .map_err(|e| Error::Shm(format!("open {}: {e}", shm::NAME)))?;
        let view = buf.view();
        view.check_header().map_err(|e| Error::Shm(e.to_string()))?;
        if !inject::heartbeat_fresh(tick_count_ms(), view.io_heartbeat_ms()) {
            return Err(Error::Shm(
                "waccon_io.dll heartbeat missing or stale; is the game running?".into(),
            ));
        }
        Ok(Some(buf))
    }

    /// Plays the script with [`inject::play`] (live buffer or an in-memory one), streaming `inject.jsonl`.
    /// Returns the records and the epoch ms of the injector clock's 0.
    fn play(
        plan: &[inject::Publish],
        delay_ms: f64,
        live: Option<&shm::SharedBuffer>,
        epoch: &Epoch,
        log_path: &Path,
    ) -> Result<(Vec<Record>, f64), Error> {
        let file = std::fs::File::create(log_path).map_err(io_err(log_path))?;
        let mut log = std::io::BufWriter::new(file);
        let mut records = Vec::new();
        let mut sink = |r: &Record| -> std::io::Result<()> {
            serde_json::to_writer(&mut log, r)?;
            writeln!(log)?;
            records.push(r.clone());
            Ok(())
        };
        let mem = shm::MemBuffer::default();
        let view = live.map_or_else(|| mem.view(), shm::SharedBuffer::view);
        let stop = AtomicBool::new(false);
        let clock = inject::RealClock::start();
        let clock_start_ms = epoch.ms(Instant::now());
        inject::play(&view, plan, delay_ms, &clock, &stop, &mut sink)
            .and_then(|()| log.flush())
            .map_err(io_err(log_path))?;
        Ok((records, clock_start_ms))
    }

    /// Runs `script`'s steps against the live buffer; screen checks grab frames into `dir`.
    fn navigate(
        script: &NavScript,
        buf: &shm::SharedBuffer,
        ffmpeg: &Path,
        window: &game::GameWindow,
        dir: &Path,
    ) -> Result<(), Error> {
        let view = buf.view();
        let grabber = video::choose_grabber(ffmpeg, window.rect);
        for (i, step) in script.steps.iter().enumerate() {
            match step {
                Step::Press {
                    buttons,
                    hold_ms,
                    wait_ms,
                } => {
                    log::info!("nav {i}: press {buttons:?} {hold_ms} ms");
                    let plan = nav::press_plan(buttons, *hold_ms);
                    let clock = inject::RealClock::start();
                    inject::play(
                        &view,
                        &plan,
                        0.0,
                        &clock,
                        &AtomicBool::new(false),
                        &mut |_| Ok(()),
                    )
                    .map_err(|e| Error::Nav(e.to_string()))?;
                    std::thread::sleep(Duration::from_secs_f64(wait_ms / 1000.0));
                }
                Step::Expect {
                    name,
                    all,
                    timeout_ms,
                } => {
                    let png = dir.join(format!("nav-{i:02}-{name}.png"));
                    let deadline = Instant::now() + Duration::from_secs_f64(timeout_ms / 1000.0);
                    loop {
                        let frame = video::grab_frame(ffmpeg, grabber, window.rect, &png)?;
                        let fracs: Vec<f64> = all.iter().map(|r| r.fraction(&frame)).collect();
                        if all.iter().all(|r| r.passes(&frame)) {
                            log::info!("nav {i}: {name} ok {fracs:.3?}");
                            break;
                        }
                        if Instant::now() >= deadline {
                            return Err(Error::Nav(format!(
                                "screen {name:?} not reached (rule fractions {fracs:.3?}, see {})",
                                png.display()
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn run(argv: impl Iterator<Item = String>) -> Result<(), Error> {
        let args = parse_args(argv)?;
        let json = std::fs::read_to_string(&args.script).map_err(io_err(&args.script))?;
        let script = timeline::parse(&json).map_err(|source| Error::Script {
            path: args.script.clone(),
            source,
        })?;
        let plan = inject::schedule(&timeline::expand(&script.taps));
        let nav_script = match &args.nav {
            Some(p) => {
                let json = std::fs::read_to_string(p).map_err(io_err(p))?;
                Some(nav::parse(&json).map_err(|source| Error::Script {
                    path: p.clone(),
                    source,
                })?)
            }
            None => None,
        };
        let ffmpeg = if args.no_video && nav_script.is_none() {
            None
        } else {
            let path_var = std::env::var_os("PATH");
            let lad = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
            Some(video::find_ffmpeg(
                args.ffmpeg.as_deref(),
                path_var.as_deref(),
                lad.as_deref(),
            )?)
        };

        game::set_dpi_aware();
        let epoch = Epoch::now();
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let dir = args.out_dir.join(experiment::run_name(unix));
        std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        log::info!("run directory {}", dir.display());

        let mut cfg = GameConfig::default();
        if let Some(d) = &args.game_dir {
            cfg.bin_dir = d.clone();
        }
        let mut guard = GameGuard {
            cfg: cfg.clone(),
            launched: None,
            keep: args.keep_game,
        };
        let window = match game::find_window(&cfg)? {
            Some(w) => {
                log::info!("game already running");
                w
            }
            None => {
                guard.launched = Some(game::launch(&cfg)?);
                game::wait_for_window(&cfg, Duration::from_secs(180))?;
                log::info!("settling {} s", args.settle_s);
                std::thread::sleep(Duration::from_secs_f64(args.settle_s));
                game::wait_for_window(&cfg, Duration::from_secs(10))?
            }
        };
        game::make_topmost(&window);
        let live = open_live()?;
        if let Some(ns) = &nav_script {
            let (Some(buf), Some(ff)) = (live.as_ref(), ffmpeg.as_deref()) else {
                return Err(Error::Nav(format!("--nav needs {INJECT_ENV}=1")));
            };
            navigate(ns, buf, ff, &window, &dir)?;
        }

        let video = match ffmpeg.as_deref().filter(|_| !args.no_video) {
            Some(ff) => {
                let grabber = video::choose_grabber(ff, window.rect);
                let out = dir.join("video.mkv");
                Some(VideoRecorder::start(
                    ff,
                    grabber,
                    window.rect,
                    &out,
                    Duration::from_secs(10),
                )?)
            }
            None => None,
        };
        let audio = AudioRecorder::start(&dir.join("audio.wav"))?;

        let inject_path = dir.join("inject.jsonl");
        let played = play(&plan, args.lead_ms, live.as_ref(), &epoch, &inject_path);
        std::thread::sleep(Duration::from_secs_f64(args.tail_ms / 1000.0));

        let device = audio.device.clone();
        let audio = audio.stop()?;
        let video = video.map(VideoRecorder::stop);
        drop(guard);

        let (records, clock_start_ms) = played?;
        let sum = inject::summary(&script.taps, &records);
        let meta = Meta {
            git_commit: git_commit(),
            epoch: epoch.anchor,
            window: Some(window.rect),
            video: video
                .as_ref()
                .map(|v| VideoMeta::new("video.mkv", v, &epoch)),
            audio: Some(AudioMeta::new("audio.wav", &device, &audio, &epoch)),
            inject: Some(InjectMeta {
                script: args.script.display().to_string(),
                live: live.is_some(),
                clock_start_ms,
                script_t0_ms: clock_start_ms + args.lead_ms,
                taps_sent: sum.taps_sent,
                taps_total: sum.taps_total,
                publishes: sum.publishes,
                acked: sum.acked,
                log: "inject.jsonl".into(),
            }),
        };
        let meta_path = dir.join("meta.json");
        let meta_json = serde_json::to_string_pretty(&meta).unwrap_or_default();
        std::fs::write(&meta_path, &meta_json).map_err(io_err(&meta_path))?;
        println!("{}\n{meta_json}", dir.display());
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn args(s: &str) -> Result<Args, Error> {
            parse_args(s.split_whitespace().map(String::from))
        }

        #[test]
        fn parses_flags() {
            let a = args(
                "--script s.json --nav n.json --ffmpeg f.exe --keep-game --settle-s 5 --tail-ms 0",
            )
            .unwrap();
            assert_eq!(a.script, PathBuf::from("s.json"));
            assert_eq!(a.nav, Some(PathBuf::from("n.json")));
            assert_eq!(a.ffmpeg, Some(PathBuf::from("f.exe")));
            assert!(a.keep_game && !a.no_video);
            assert_eq!((a.settle_s, a.lead_ms, a.tail_ms), (5.0, 1000.0, 0.0));
        }

        #[test]
        fn rejects_bad_args() {
            assert!(matches!(args(""), Err(Error::Args(_))));
            assert!(matches!(args("--script x --bogus"), Err(Error::Args(_))));
            assert!(matches!(
                args("--script x --lead-ms -1"),
                Err(Error::Args(_))
            ));
            assert!(matches!(args("--script"), Err(Error::Args(_))));
        }
    }
}
