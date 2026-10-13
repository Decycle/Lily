//! One fully automatic real-game experiment: launch the game (unless running), record video (FFmpeg) + loopback
//! audio, play a tap script into the Waccon shared buffer, stop everything, write `runs/<ts>/`.
//!
//! `run_experiment --script <taps.json> [--nav <nav.json>] [--start-nav <nav.json>] [--sync-chart <mer>]
//!  [--assets DIR] [--input-lead-ms N] [--sync-timeout-s 90] [--ffmpeg <path>] [--keep-game] [--no-video]
//!  [--settle-s 30] [--lead-ms 1000] [--tail-ms 2000] [--out-dir runs] [--game-dir C:\Games\WACCA\bin]`
//!
//! `--nav` runs a navigation script ([`wacca_realgame::nav`], e.g. `scripts/nav-input-test.nav.json`) before
//! recording: operator buttons and touches through the shared buffer, screen checks on grabbed frames (saved as
//! `nav-<step>-<name>.png`); a failed check aborts the run. `--start-nav` runs a second one after recording has
//! started (e.g. the touch that starts a song; frames saved as `start-<step>-<name>.png`).
//!
//! Song runs (experiment E2, doc 09): with `--sync-chart`, the script is in chart time. After `--start-nav` the
//! loopback audio is matched live against the chart's BGM (decoded from `--assets`, default `WACCA_ASSETS`) with
//! [`wacca_realgame::sync::Detector`]; once the song start is found, every edge is played at
//! `bgm0 + (chart_t - #OFFSET) - input_lead`. The match, the packet capture times (`audio_packets.csv`) and the
//! files the game changed under `<game-dir>\appdata` (`appdata-changes.txt`) are saved with the run.
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
    use std::sync::mpsc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use thiserror::Error;
    use wacca_realgame::experiment;
    use wacca_realgame::frame::Rgb;
    use wacca_realgame::game::{self, GameConfig, GameError, Launched};
    use wacca_realgame::inject::{self, Record};
    use wacca_realgame::nav::{self, NavScript, Step};
    use wacca_realgame::record::Epoch;
    use wacca_realgame::record::audio::{AudioError, AudioRecorder, Captured};
    use wacca_realgame::record::meta::{AudioMeta, InjectMeta, Meta, SyncMeta, VideoMeta};
    use wacca_realgame::record::video::{self, VideoError, VideoRecorder};
    use wacca_realgame::sync::{self, Detector, DetectorOptions, Packet};
    use wacca_realgame::{shm, timeline};

    const INJECT_ENV: &str = "WACCA_INJECT_OK";
    /// Publishes due sooner than this after playback starts are dropped (the song started too early for them).
    const MIN_AHEAD_MS: f64 = 20.0;

    #[derive(Debug, Error)]
    pub enum Error {
        #[error(
            "{0}\nusage: run_experiment --script <taps.json> [--nav <nav.json>] [--start-nav <nav.json>] [--sync-chart <mer>] [--assets DIR] [--input-lead-ms N] [--sync-timeout-s N] [--ffmpeg <path>] [--keep-game] [--no-video] [--settle-s N] [--lead-ms N] [--tail-ms N] [--out-dir DIR] [--game-dir DIR]"
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
        #[error("song sync: {0}")]
        Sync(String),
    }

    #[derive(Debug)]
    struct Args {
        script: PathBuf,
        nav: Option<PathBuf>,
        start_nav: Option<PathBuf>,
        sync_chart: Option<PathBuf>,
        assets: Option<PathBuf>,
        input_lead_ms: f64,
        sync_timeout_s: f64,
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
            start_nav: None,
            sync_chart: None,
            assets: std::env::var_os("WACCA_ASSETS").map(PathBuf::from),
            input_lead_ms: 0.0,
            sync_timeout_s: 90.0,
            ffmpeg: None,
            keep_game: false,
            no_video: false,
            settle_s: 30.0,
            lead_ms: 1000.0,
            tail_ms: 2000.0,
            out_dir: PathBuf::from("runs"),
            game_dir: None,
        };
        let signed = |flag: &str, v: Option<String>| -> Result<f64, Error> {
            v.and_then(|v| v.parse().ok())
                .filter(|n: &f64| n.is_finite())
                .ok_or_else(|| Error::Args(format!("{flag} needs a number")))
        };
        let path = |flag: &str, v: Option<String>| -> Result<PathBuf, Error> {
            v.map(PathBuf::from)
                .ok_or_else(|| Error::Args(format!("{flag} needs a path")))
        };
        while let Some(flag) = it.next() {
            let nonneg = |v: Option<String>| -> Result<f64, Error> {
                Some(signed(&flag, v)?)
                    .filter(|n| *n >= 0.0)
                    .ok_or_else(|| Error::Args(format!("{flag} needs a non-negative number")))
            };
            match flag.as_str() {
                "--script" => a.script = path(&flag, it.next())?,
                "--nav" => a.nav = Some(path(&flag, it.next())?),
                "--start-nav" => a.start_nav = Some(path(&flag, it.next())?),
                "--sync-chart" => a.sync_chart = Some(path(&flag, it.next())?),
                "--assets" => a.assets = Some(path(&flag, it.next())?),
                "--input-lead-ms" => a.input_lead_ms = signed(&flag, it.next())?,
                "--sync-timeout-s" => a.sync_timeout_s = nonneg(it.next())?,
                "--ffmpeg" => a.ffmpeg = Some(path(&flag, it.next())?),
                "--keep-game" => a.keep_game = true,
                "--no-video" => a.no_video = true,
                "--settle-s" => a.settle_s = nonneg(it.next())?,
                "--lead-ms" => a.lead_ms = nonneg(it.next())?,
                "--tail-ms" => a.tail_ms = nonneg(it.next())?,
                "--out-dir" => a.out_dir = path(&flag, it.next())?,
                "--game-dir" => a.game_dir = Some(path(&flag, it.next())?),
                other => return Err(Error::Args(format!("unknown argument {other:?}"))),
            }
        }
        if a.script.as_os_str().is_empty() {
            return Err(Error::Args("--script is required".into()));
        }
        if a.sync_chart.is_some() && a.assets.is_none() {
            return Err(Error::Args(
                "--sync-chart needs --assets or WACCA_ASSETS".into(),
            ));
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
    /// Script time 0 is at epoch ms `t0_ms`, or `lead_ms` after playback starts if `None`. Returns the records,
    /// the epoch ms of the injector clock's 0 and the delay used.
    fn play(
        plan: &[inject::Publish],
        t0_ms: Option<f64>,
        lead_ms: f64,
        live: Option<&shm::SharedBuffer>,
        epoch: &Epoch,
        log_path: &Path,
    ) -> Result<(Vec<Record>, f64, f64), Error> {
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
        let delay_ms = t0_ms.map_or(lead_ms, |t0| t0 - clock_start_ms);
        let first = plan
            .iter()
            .position(|p| p.t_ms + delay_ms >= MIN_AHEAD_MS)
            .unwrap_or(plan.len());
        if first > 0 {
            log::warn!("{first} publishes were already due and are skipped");
        }
        inject::play(&view, &plan[first..], delay_ms, &clock, &stop, &mut sink)
            .and_then(|()| log.flush())
            .map_err(io_err(log_path))?;
        Ok((records, clock_start_ms, delay_ms))
    }

    /// Runs `script`'s steps against the live buffer; screen checks grab frames into `dir` as `<tag>-<i>-<name>.png`.
    fn navigate(
        script: &NavScript,
        tag: &str,
        buf: &shm::SharedBuffer,
        ffmpeg: &Path,
        window: &game::GameWindow,
        dir: &Path,
    ) -> Result<(), Error> {
        let view = buf.view();
        let grabber = video::choose_grabber(ffmpeg, window.rect);
        let input = |plan: &[inject::Publish], wait_ms: f64| -> Result<(), Error> {
            let clock = inject::RealClock::start();
            inject::play(
                &view,
                plan,
                0.0,
                &clock,
                &AtomicBool::new(false),
                &mut |_| Ok(()),
            )
            .map_err(|e| Error::Nav(e.to_string()))?;
            std::thread::sleep(Duration::from_secs_f64(wait_ms / 1000.0));
            Ok(())
        };
        for (i, step) in script.steps.iter().enumerate() {
            if let Some((plan, wait_ms)) = step.input() {
                log::info!("{tag} {i}: {step:?}");
                input(&plan, wait_ms)?;
            } else if let Step::Seek {
                name,
                image,
                region,
                max_diff,
                then,
                max_steps,
            } = step
            {
                let path = Path::new(image);
                let reference = Rgb::from_png(&std::fs::read(path).map_err(io_err(path))?)
                    .ok_or_else(|| Error::Nav(format!("{image}: not an 8-bit RGB(A) PNG")))?;
                let png = dir.join(format!("{tag}-{i:02}-{name}.png"));
                let (plan, wait_ms) = then.input().ok_or_else(|| {
                    Error::Nav(format!("{name}: `then` must be a press or touch"))
                })?;
                for n in 0..=*max_steps {
                    let frame = video::grab_frame(ffmpeg, grabber, window.rect, &png)?;
                    let d = nav::region_diff(&frame, *region, &reference, 8);
                    if d <= *max_diff {
                        log::info!("{tag} {i}: {name} found after {n} touches (diff {d:.1})");
                        break;
                    }
                    if n == *max_steps {
                        return Err(Error::Nav(format!(
                            "{name:?} not found in {max_steps} touches (last diff {d:.1}, see {})",
                            png.display()
                        )));
                    }
                    log::info!("{tag} {i}: {name} diff {d:.1}");
                    input(&plan, wait_ms)?;
                }
            } else if let Step::Expect {
                name,
                all,
                timeout_ms,
            } = step
            {
                let png = dir.join(format!("{tag}-{i:02}-{name}.png"));
                let deadline = Instant::now() + Duration::from_secs_f64(timeout_ms / 1000.0);
                loop {
                    let frame = video::grab_frame(ffmpeg, grabber, window.rect, &png)?;
                    let fracs: Vec<f64> = all.iter().map(|r| r.fraction(&frame)).collect();
                    if all.iter().all(|r| r.passes(&frame)) {
                        log::info!("{tag} {i}: {name} ok {fracs:.3?}");
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
        Ok(())
    }

    fn read_nav(p: &Path) -> Result<NavScript, Error> {
        let json = std::fs::read_to_string(p).map_err(io_err(p))?;
        nav::parse(&json).map_err(|source| Error::Script {
            path: p.to_path_buf(),
            source,
        })
    }

    /// The chart and its BGM (mono) for live sync.
    struct SongSync {
        chart: PathBuf,
        cue: String,
        offset_s: f64,
        bgm: Vec<f32>,
        rate: u32,
    }

    fn load_song(chart_path: &Path, assets: &Path) -> Result<SongSync, Error> {
        let bytes = std::fs::read(chart_path).map_err(io_err(chart_path))?;
        let chart = wacca_chart::parse(&bytes).map_err(|e| Error::Sync(e.to_string()))?;
        let cue = chart.header.music_file_path.clone();
        let (bgm, rate) = sync::load_bgm(assets, &cue).map_err(|e| Error::Sync(e.to_string()))?;
        log::info!(
            "sync: cue {cue}, {:.1} s of BGM at {rate} Hz, #OFFSET {}",
            bgm.len() as f64 / f64::from(rate),
            chart.header.offset
        );
        Ok(SongSync {
            chart: chart_path.to_path_buf(),
            cue,
            offset_s: chart.header.offset,
            bgm,
            rate,
        })
    }

    /// Feeds loopback packets to a [`Detector`] until it finds the song start; returns the match, the epoch ms of
    /// BGM sample 0 and of the detection.
    fn wait_song_start(
        rx: &mpsc::Receiver<Captured>,
        song: &SongSync,
        channels: usize,
        epoch: &Epoch,
        timeout: Duration,
    ) -> Result<(sync::Match, f64, f64), Error> {
        let opts = DetectorOptions {
            rate: song.rate,
            ..DetectorOptions::default()
        };
        let mut det = Detector::new(&song.bgm, opts)
            .ok_or_else(|| Error::Sync("BGM is silent or too short".into()))?;
        let mut packets = Vec::new();
        let mut frames = 0u64;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let c = rx.recv_timeout(left).map_err(|e| {
                Error::Sync(format!(
                    "no song start within {timeout:?} ({e}); best candidate {:?}",
                    det.best()
                ))
            })?;
            packets.push(Packet {
                frame: frames,
                ms: epoch.ms(c.at),
            });
            let mono = sync::mono(&c.samples, channels);
            frames += mono.len() as u64;
            if let Some(m) = det.push(&mono) {
                let bgm0_ms = sync::frame_ms(&packets, song.rate, m.bgm0 as f64)
                    .ok_or_else(|| Error::Sync("no packets".into()))?;
                let now = epoch.ms(Instant::now());
                log::info!(
                    "sync: song start at epoch {bgm0_ms:.1} ms (coarse r {:.3}, fine r {:.3}), found {:.0} ms later",
                    m.coarse,
                    m.fine,
                    now - bgm0_ms
                );
                return Ok((m, bgm0_ms, now));
            }
        }
    }

    fn write_packets(path: &Path, packets: &[(u64, Instant)], epoch: &Epoch) -> Result<(), Error> {
        let mut csv = String::from("frame,ms\n");
        for (frame, at) in packets {
            csv.push_str(&format!("{frame},{:.4}\n", epoch.ms(*at)));
        }
        std::fs::write(path, csv).map_err(io_err(path))
    }

    pub fn run(argv: impl Iterator<Item = String>) -> Result<(), Error> {
        let args = parse_args(argv)?;
        let json = std::fs::read_to_string(&args.script).map_err(io_err(&args.script))?;
        let script = timeline::parse(&json).map_err(|source| Error::Script {
            path: args.script.clone(),
            source,
        })?;
        let plan = inject::schedule(&timeline::expand(&script.taps));
        let nav_script = args.nav.as_deref().map(read_nav).transpose()?;
        let start_nav = args.start_nav.as_deref().map(read_nav).transpose()?;
        let song = match (&args.sync_chart, &args.assets) {
            (Some(c), Some(a)) => Some(load_song(c, a)?),
            _ => None,
        };
        let needs_nav = nav_script.is_some() || start_nav.is_some();
        let ffmpeg = if args.no_video && !needs_nav {
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
        let appdata = cfg.bin_dir.join("appdata");
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
        let files_before = experiment::list_files(&appdata);
        let live = open_live()?;
        let nav_ctx = || match (live.as_ref(), ffmpeg.as_deref()) {
            (Some(buf), Some(ff)) => Ok((buf, ff)),
            _ => Err(Error::Nav(format!("navigation needs {INJECT_ENV}=1"))),
        };
        if let Some(ns) = &nav_script {
            let (buf, ff) = nav_ctx()?;
            navigate(ns, "nav", buf, ff, &window, &dir)?;
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
        let (tap, rx) = match song {
            Some(_) => {
                let (tx, rx) = mpsc::channel();
                (Some(tx), Some(rx))
            }
            None => (None, None),
        };
        let audio = AudioRecorder::start(&dir.join("audio.wav"), tap)?;
        if let Some(ns) = &start_nav {
            let (buf, ff) = nav_ctx()?;
            navigate(ns, "start", buf, ff, &window, &dir)?;
        }
        let synced = match (&song, &rx) {
            (Some(s), Some(rx)) => {
                while rx.try_recv().is_ok() {}
                let timeout = Duration::from_secs_f64(args.sync_timeout_s);
                let channels = usize::from(audio.channels);
                match wait_song_start(rx, s, channels, &epoch, timeout) {
                    Ok(found) => Some(found),
                    Err(e) => {
                        let _ = audio.stop();
                        return Err(e);
                    }
                }
            }
            _ => None,
        };
        drop(rx);
        let t0_ms = match (&song, &synced) {
            (Some(s), Some((_, bgm0_ms, _))) => {
                Some(bgm0_ms - s.offset_s * 1000.0 - args.input_lead_ms)
            }
            _ => None,
        };

        let inject_path = dir.join("inject.jsonl");
        let played = play(
            &plan,
            t0_ms,
            args.lead_ms,
            live.as_ref(),
            &epoch,
            &inject_path,
        );
        std::thread::sleep(Duration::from_secs_f64(args.tail_ms / 1000.0));

        let device = audio.device.clone();
        let audio = audio.stop()?;
        let video = video.map(VideoRecorder::stop);
        write_packets(&dir.join("audio_packets.csv"), &audio.packets, &epoch)?;
        let changes = experiment::diff_files(&files_before, &experiment::list_files(&appdata));
        let changes_path = dir.join("appdata-changes.txt");
        std::fs::write(&changes_path, changes.join("\n")).map_err(io_err(&changes_path))?;
        log::info!(
            "{} files changed under {}",
            changes.len(),
            appdata.display()
        );
        drop(guard);

        let (records, clock_start_ms, delay_ms) = played?;
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
                script_t0_ms: clock_start_ms + delay_ms,
                taps_sent: sum.taps_sent,
                taps_total: sum.taps_total,
                publishes: sum.publishes,
                acked: sum.acked,
                log: "inject.jsonl".into(),
            }),
            sync: song
                .as_ref()
                .zip(synced)
                .map(|(s, (m, bgm0_ms, detected_ms))| SyncMeta {
                    chart: s.chart.display().to_string(),
                    cue: s.cue.clone(),
                    chart_offset_s: s.offset_s,
                    bgm0_ms,
                    coarse: m.coarse,
                    fine: m.fine,
                    detected_ms,
                    input_lead_ms: args.input_lead_ms,
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
                "--script s.json --nav n.json --start-nav g.json --ffmpeg f.exe --keep-game --settle-s 5 --tail-ms 0 --input-lead-ms -12.5 --sync-chart c.mer --assets A",
            )
            .unwrap();
            assert_eq!(a.script, PathBuf::from("s.json"));
            assert_eq!(a.nav, Some(PathBuf::from("n.json")));
            assert_eq!(a.start_nav, Some(PathBuf::from("g.json")));
            assert_eq!(a.ffmpeg, Some(PathBuf::from("f.exe")));
            assert_eq!(a.sync_chart, Some(PathBuf::from("c.mer")));
            assert_eq!(a.assets, Some(PathBuf::from("A")));
            assert!(a.keep_game && !a.no_video);
            assert_eq!((a.settle_s, a.lead_ms, a.tail_ms), (5.0, 1000.0, 0.0));
            assert_eq!(a.input_lead_ms, -12.5);
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
