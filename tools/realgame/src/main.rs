//! `wacca-realgame inject --script <taps.json> [--dry-run] [--start-delay-ms N] [--log <path>]`
//!
//! Plays a tap script into `Local\WACCON_SHARED_BUFFER` (doc 07 §1.3, experiment E1) on a dedicated thread and
//! logs every write to `inject.jsonl` (default `runs/<unix-seconds>-inject/inject.jsonl`). Live runs need
//! `WACCA_INJECT_OK=1`, an ABI 1.1 header and a fresh waccon_io.dll heartbeat. `--dry-run` plays into an
//! in-memory buffer and never opens the real one. All cells are released at the end, on Ctrl+C and on errors;
//! if the process is killed outright, the 100 ms lease releases them.
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use wacca_realgame::inject::{self, Kind, Publish, Record};
use wacca_realgame::shm::{self, MemBuffer, ShmError};
use wacca_realgame::timeline;

const USAGE: &str = "usage: wacca-realgame inject --script <taps.json> [--dry-run] [--start-delay-ms N] [--log <path>]";
const OPT_IN_VAR: &str = "WACCA_INJECT_OK";

/// Set by the console Ctrl+C / close handler; playback stops and releases all cells.
static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{USAGE}")]
    Usage,
    #[error(
        "refusing to write to the live game's shared buffer: set {OPT_IN_VAR}=1 to opt in, or use --dry-run"
    )]
    NotOptedIn,
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}: {source}", path.display())]
    Script {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("opening the shared buffer: {0} (is the game with waccon_io.dll running?)")]
    Open(std::io::Error),
    #[error(transparent)]
    Abi(#[from] ShmError),
    #[error(
        "waccon_io.dll heartbeat is missing or older than {} ms",
        inject::HEARTBEAT_MAX_AGE_MS
    )]
    Heartbeat,
    #[error("writing the log: {0}")]
    Log(std::io::Error),
    #[error("injector thread panicked (cells were released)")]
    Panicked,
    #[cfg(not(windows))]
    #[error("live injection needs Windows")]
    Unsupported,
}

#[derive(Debug, PartialEq)]
struct Args {
    script: PathBuf,
    dry_run: bool,
    start_delay_ms: f64,
    log: Option<PathBuf>,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, Error> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("inject") {
        return Err(Error::Usage);
    }
    let (mut script, mut dry_run, mut start_delay_ms, mut log) = (None, false, 0.0, None);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--dry-run" => dry_run = true,
            "--script" => script = Some(PathBuf::from(args.next().ok_or(Error::Usage)?)),
            "--log" => log = Some(PathBuf::from(args.next().ok_or(Error::Usage)?)),
            "--start-delay-ms" => {
                let v: f64 = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or(Error::Usage)?;
                if !(0.0..=600_000.0).contains(&v) {
                    return Err(Error::Usage);
                }
                start_delay_ms = v;
            }
            _ => return Err(Error::Usage),
        }
    }
    Ok(Args {
        script: script.ok_or(Error::Usage)?,
        dry_run,
        start_delay_ms,
        log,
    })
}

/// Live writes need an explicit `WACCA_INJECT_OK=1`.
fn opt_in(dry_run: bool, env: Option<&str>) -> Result<(), Error> {
    if dry_run || env == Some("1") {
        Ok(())
    } else {
        Err(Error::NotOptedIn)
    }
}

fn default_log_path(unix_secs: u64) -> PathBuf {
    Path::new("runs")
        .join(format!("{unix_secs}-inject"))
        .join("inject.jsonl")
}

fn format_record(r: &Record) -> String {
    let ack = r
        .ack_latency_ms()
        .map_or("-".to_owned(), |a| format!("{a:.3}ms"));
    format!(
        "{:<5} seq={:<6} script={:>9.3}ms write_err={:+.3}ms ack={ack} cells={}",
        format!("{:?}", r.kind).to_lowercase(),
        r.seq,
        r.t_script_ms,
        r.write_error_ms(),
        r.cells
    )
}

#[cfg(windows)]
mod os {
    use super::STOP;
    use std::sync::atomic::Ordering;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTickCount64() -> u64;
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    unsafe extern "system" fn on_ctrl(_event: u32) -> i32 {
        STOP.store(true, Ordering::Relaxed);
        1
    }

    pub fn install_ctrl_handler() {
        // SAFETY: registers a handler that only touches an atomic.
        if unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) } == 0 {
            eprintln!(
                "warning: no Ctrl+C handler ({})",
                std::io::Error::last_os_error()
            );
        }
    }

    /// The clock waccon_io.dll stamps its heartbeat with.
    pub fn tick_ms() -> u64 {
        // SAFETY: no arguments, no preconditions.
        unsafe { GetTickCount64() }
    }
}

#[cfg(windows)]
fn play_live(
    plan: &[Publish],
    delay_ms: f64,
    sink: &mut dyn FnMut(&Record) -> std::io::Result<()>,
) -> Result<(), Error> {
    let shared = shm::SharedBuffer::open_existing(shm::NAME).map_err(Error::Open)?;
    let view = shared.view();
    view.check_header()?;
    if !inject::heartbeat_fresh(os::tick_ms(), view.io_heartbeat_ms()) {
        return Err(Error::Heartbeat);
    }
    inject::play(
        &view,
        plan,
        delay_ms,
        &inject::RealClock::start(),
        &STOP,
        sink,
    )
    .map_err(Error::Log)
}

#[cfg(not(windows))]
fn play_live(
    _: &[Publish],
    _: f64,
    _: &mut dyn FnMut(&Record) -> std::io::Result<()>,
) -> Result<(), Error> {
    Err(Error::Unsupported)
}

/// Runs on the dedicated injector thread (the buffer view is not `Send`, so it is opened here).
fn play_thread(
    args: &Args,
    plan: &[Publish],
    log: &mut impl Write,
    records: &mut Vec<Record>,
) -> Result<(), Error> {
    let mut sink = |r: &Record| -> std::io::Result<()> {
        println!("{}", format_record(r));
        serde_json::to_writer(&mut *log, r)?;
        writeln!(log)?;
        records.push(r.clone());
        Ok(())
    };
    let played = if args.dry_run {
        let buf = MemBuffer::default();
        let clock = inject::RealClock::start();
        inject::play(
            &buf.view(),
            plan,
            args.start_delay_ms,
            &clock,
            &STOP,
            &mut sink,
        )
        .map_err(Error::Log)
    } else {
        play_live(plan, args.start_delay_ms, &mut sink)
    };
    played.and(log.flush().map_err(Error::Log))
}

fn run(args: &Args) -> Result<(), Error> {
    opt_in(args.dry_run, std::env::var(OPT_IN_VAR).ok().as_deref())?;
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    let text = std::fs::read_to_string(&args.script).map_err(io(&args.script))?;
    let script = timeline::parse(&text).map_err(|source| Error::Script {
        path: args.script.clone(),
        source,
    })?;
    let plan = inject::schedule(&timeline::expand(&script.taps));

    let log_path = args.log.clone().unwrap_or_else(|| {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        default_log_path(secs)
    });
    if let Some(dir) = log_path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    let file = std::fs::File::create(&log_path).map_err(io(&log_path))?;
    let mut log = std::io::BufWriter::new(file);

    let edges = plan.iter().filter(|p| p.kind == Kind::Edge).count();
    let end = plan.last().map_or(0.0, |p| p.t_ms);
    println!(
        "{}: {} taps -> {} publishes ({edges} edges, {} renewals), script end {end:.0} ms, start delay {:.0} ms",
        if args.dry_run {
            "DRY RUN (in-memory buffer, no DLL: ack = -)"
        } else {
            "LIVE"
        },
        script.taps.len(),
        plan.len(),
        plan.len() - edges,
        args.start_delay_ms,
    );
    println!("log: {}", log_path.display());

    #[cfg(windows)]
    os::install_ctrl_handler();
    let mut records = Vec::new();
    let played = std::thread::scope(|s| {
        s.spawn(|| play_thread(args, &plan, &mut log, &mut records))
            .join()
            .unwrap_or(Err(Error::Panicked))
    });

    let sum = inject::summary(&script.taps, &records);
    let stats =
        |s: Option<wacca_realgame::stats::Stats>| s.map_or("none".to_owned(), |s| s.to_string());
    println!(
        "summary: taps sent {}/{}, publishes {}, acked {}{}",
        sum.taps_sent,
        sum.taps_total,
        sum.publishes,
        sum.acked,
        if STOP.load(Ordering::Relaxed) {
            " (stopped by Ctrl+C)"
        } else {
            ""
        }
    );
    println!("  write timing (actual - scheduled): {}", stats(sum.timing));
    println!(
        "  ack latency (DLL took record - write): {}",
        stats(sum.ack)
    );
    played
}

fn main() -> ExitCode {
    let result = parse_args(std::env::args().skip(1)).and_then(|a| run(&a));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Result<Args, Error> {
        parse_args(s.split_whitespace().map(String::from))
    }

    #[test]
    fn parses_all_flags() {
        assert_eq!(
            args("inject --script a.json --dry-run --start-delay-ms 2500 --log x/i.jsonl").unwrap(),
            Args {
                script: "a.json".into(),
                dry_run: true,
                start_delay_ms: 2500.0,
                log: Some("x/i.jsonl".into()),
            }
        );
        let a = args("inject --script a.json").unwrap();
        assert_eq!((a.dry_run, a.start_delay_ms, a.log), (false, 0.0, None));
    }

    #[test]
    fn rejects_bad_args() {
        for bad in [
            "",
            "--script a.json",
            "capture --script a.json",
            "inject",
            "inject --script",
            "inject --script a.json --start-delay-ms",
            "inject --script a.json --start-delay-ms soon",
            "inject --script a.json --start-delay-ms -5",
            "inject --script a.json --live",
        ] {
            assert!(matches!(args(bad), Err(Error::Usage)), "{bad:?}");
        }
    }

    #[test]
    fn live_needs_explicit_opt_in() {
        assert!(opt_in(true, None).is_ok());
        assert!(opt_in(false, Some("1")).is_ok());
        for env in [None, Some(""), Some("0"), Some("yes"), Some("true")] {
            assert!(
                matches!(opt_in(false, env), Err(Error::NotOptedIn)),
                "{env:?}"
            );
        }
        assert!(Error::NotOptedIn.to_string().contains("WACCA_INJECT_OK=1"));
    }

    #[test]
    fn default_log_under_runs() {
        assert_eq!(
            default_log_path(1_760_000_000),
            Path::new("runs")
                .join("1760000000-inject")
                .join("inject.jsonl")
        );
    }

    #[test]
    fn record_line() {
        let r = Record {
            kind: Kind::Edge,
            seq: 4,
            t_script_ms: 1000.0,
            t_sched_ms: 1000.0,
            t_write_ms: 1000.25,
            t_ack_ms: Some(1001.25),
            cells: "0".repeat(60),
        };
        let line = format_record(&r);
        assert!(line.starts_with("edge  seq=4 "), "{line}");
        assert!(line.contains("write_err=+0.250ms ack=1.000ms"), "{line}");
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.starts_with(r#"{"kind":"edge","seq":4,"#), "{json}");
    }
}
