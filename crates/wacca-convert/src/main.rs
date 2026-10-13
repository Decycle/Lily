//! `wacca-convert --assets <dir> --out <dir> [--textures] [--fonts] [--meshes] [--effects] [--export-ui <subdir>]...`
//! (see the library docs for what each flag writes).
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

use std::path::PathBuf;
use std::process::ExitCode;

use wacca_convert::{Options, convert, is_plain_relative};

const USAGE: &str = "usage: wacca-convert --assets <dir> --out <dir> [--textures] [--fonts] [--meshes] [--effects] [--export-ui <subdir under assets>]...";

#[derive(Debug, Default, PartialEq)]
struct Args {
    assets: PathBuf,
    out: PathBuf,
    opts: Options,
}

/// `None` on a usage error.
fn parse_args(args: impl IntoIterator<Item = String>) -> Option<Args> {
    let (mut assets, mut out) = (None, None);
    let mut opts = Options::default();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let mut value = || args.next().map(PathBuf::from);
        match flag.as_str() {
            "--assets" => assets = Some(value()?),
            "--out" => out = Some(value()?),
            "--textures" => opts.textures = true,
            "--fonts" => opts.fonts = true,
            "--meshes" => opts.meshes = true,
            "--effects" => opts.effects = true,
            "--export-ui" => {
                let dir = value()?;
                if !is_plain_relative(&dir) {
                    return None;
                }
                opts.export_ui.push(dir);
            }
            _ => return None,
        }
    }
    Some(Args {
        assets: assets?,
        out: out?,
        opts,
    })
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
        eprintln!("wacca-convert: {USAGE}");
        return ExitCode::FAILURE;
    };
    match convert(&args.assets, &args.out, &args.opts, &mut |_| true) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wacca-convert: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Option<Args> {
        parse_args(s.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn parses_flags_in_any_order() {
        let expected = Args {
            assets: "a".into(),
            out: "o".into(),
            ..Args::default()
        };
        assert_eq!(args("--assets a --out o").unwrap(), expected);
        assert_eq!(args("--out o --assets a").unwrap(), expected);
    }

    #[test]
    fn parses_texture_font_and_export_flags() {
        let a =
            args("--textures --assets a --export-ui UI/Textures --fonts --meshes --effects --out o --export-ui Vfx")
                .unwrap();
        let o = &a.opts;
        assert!(o.textures && o.fonts && o.meshes && o.effects);
        assert_eq!(o.export_ui, [PathBuf::from("UI/Textures"), "Vfx".into()]);
        assert_eq!(args("--assets a --out o --export-ui ../x"), None);
        assert_eq!(args("--assets a --out o --export-ui"), None);
    }

    #[test]
    fn rejects_missing_or_unknown_flags() {
        assert_eq!(args(""), None);
        assert_eq!(args("--assets a"), None);
        assert_eq!(args("--assets a --out"), None);
        assert_eq!(args("--assets a --out o --x y"), None);
    }
}
