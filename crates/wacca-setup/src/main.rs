//! Test driver: `wacca-setup <folder> [--dest <dir>] [--movies] [--verify]`. Without `--dest`
//! only prints what [`wacca_setup::detect`] found.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use wacca_setup::{Plan, Progress, Source, detect, run, verify};

const USAGE: &str = "usage: wacca-setup <folder> [--dest <dir>] [--movies] [--verify]";

#[derive(Debug, Default)]
struct Args {
    folder: PathBuf,
    dest: Option<PathBuf>,
    movies: bool,
    verify: bool,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Option<Args> {
    let mut out = Args::default();
    let mut folder = None;
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dest" => out.dest = Some(args.next()?.into()),
            "--movies" => out.movies = true,
            "--verify" => out.verify = true,
            f if !f.starts_with("--") && folder.is_none() => folder = Some(f.into()),
            _ => return None,
        }
    }
    out.folder = folder?;
    Some(out)
}

/// Prints progress to stderr at most every half second (and on every phase/detail change).
fn printer() -> impl FnMut(Progress) -> bool {
    let mut last = (Instant::now(), None::<(wacca_setup::Phase, String)>);
    move |p: Progress| {
        let key = Some((p.phase, p.detail.clone()));
        if key != last.1 || last.0.elapsed().as_millis() >= 500 {
            let pct = if p.total > 0 {
                format!("{:5.1}%", p.done as f64 * 100.0 / p.total as f64)
            } else {
                "     ".into()
            };
            eprintln!("{:?} {pct} {}", p.phase, p.detail);
            last = (Instant::now(), key);
        }
        true
    }
}

/// Minimal stderr logger (info and above).
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

fn main() -> ExitCode {
    if log::set_logger(&StderrLogger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    let Some(args) = parse_args(std::env::args().skip(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    match main_inner(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wacca-setup: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main_inner(args: &Args) -> Result<(), wacca_setup::SetupError> {
    let found = detect(&args.folder)?;
    println!("{found:#?}");
    if args.verify
        && let Source::Archives(archives) = &found.source
    {
        for a in archives {
            let ok = verify(a, &mut printer())?;
            println!("{} md5 {}", a.version, if ok { "OK" } else { "MISMATCH" });
        }
    }
    let Some(dest) = &args.dest else {
        return Ok(());
    };
    let plan = Plan {
        include_movies: args.movies,
        dest: dest.clone(),
    };
    let start = Instant::now();
    let summary = run(&found.source, &plan, &mut printer())?;
    println!("{summary:#?}");
    println!("total {:.1?}", start.elapsed());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_args() {
        let a = |s: &str| parse_args(s.split_whitespace().map(str::to_owned));
        let p = a("dir --dest out --movies").unwrap();
        assert_eq!(
            (p.folder, p.dest, p.movies, p.verify),
            ("dir".into(), Some("out".into()), true, false)
        );
        assert!(a("--verify").is_none());
        assert!(a("a b").is_none());
        assert!(a("a --dest").is_none());
    }
}
