//! `e2_script --chart <mer> --out <taps.json> [--min-on-ms 34] [--min-gap-ms 17]`: perfect input for a chart as a
//! tap script in chart time ([`wacca_realgame::perfect`]), for `run_experiment --sync-chart`. Also plays the
//! script through our own judge (`wacca-game`) and prints the result, which must be all Marvelous.
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::path::PathBuf;
use std::process::ExitCode;
use wacca_game::judge::{Judge, JudgeTable, autoplay_frames, class_of};
use wacca_game::score::PlayTally;
use wacca_realgame::perfect;
use wacca_realgame::timeline::{self, Script};

#[derive(Debug)]
struct Args {
    chart: PathBuf,
    out: PathBuf,
    min_on_ms: f64,
    min_gap_ms: f64,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Option<Args> {
    let mut a = Args {
        chart: PathBuf::new(),
        out: PathBuf::new(),
        min_on_ms: 34.0,
        min_gap_ms: 17.0,
    };
    while let Some(flag) = it.next() {
        let v = it.next()?;
        match flag.as_str() {
            "--chart" => a.chart = v.into(),
            "--out" => a.out = v.into(),
            "--min-on-ms" => a.min_on_ms = v.parse().ok()?,
            "--min-gap-ms" => a.min_gap_ms = v.parse().ok()?,
            _ => return None,
        }
    }
    (!a.chart.as_os_str().is_empty() && !a.out.as_os_str().is_empty()).then_some(a)
}

fn run(a: &Args) -> Result<(), String> {
    let bytes = std::fs::read(&a.chart).map_err(|e| format!("{}: {e}", a.chart.display()))?;
    let chart = wacca_chart::parse(&bytes).map_err(|e| e.to_string())?;
    let contacts = perfect::contacts(&autoplay_frames(&chart));
    let stretched = perfect::stretch(&contacts, a.min_on_ms / 1000.0, a.min_gap_ms / 1000.0);
    let script = Script {
        taps: perfect::to_taps(&stretched),
    };
    let json = serde_json::to_string_pretty(&script).map_err(|e| e.to_string())?;
    std::fs::write(&a.out, json).map_err(|e| format!("{}: {e}", a.out.display()))?;

    let notes: Vec<_> = chart.notes.iter().map(|n| (class_of(n), n.bonus)).collect();
    let mut judge = Judge::new(&chart, JudgeTable::reverse());
    let mut tally = PlayTally::new(&notes, true);
    let mut frames = timeline::expand(&script.taps);
    // Let our clock run past the last release so holds ending there are resolved (the real game keeps running).
    if let Some(&last) = frames.last() {
        frames.push(wacca_game::input::InputFrame {
            time: last.time + 1.0,
            ..last
        });
    }
    let mut events = Vec::new();
    for frame in &frames {
        events.extend(judge.update(frame));
    }
    events.extend(judge.finish());
    for e in &events {
        tally.apply(e);
        if e.judgement != wacca_game::rules::Judgement::Marvelous {
            let n = &chart.notes[e.note];
            println!(
                "  note {} at {:.3} s {:?} {:?}: {:?} {:?}",
                e.note, n.time, n.arc, n.body, e.judgement, e.offset
            );
        }
    }
    let result = tally.finish(0.0);
    let short = stretched
        .iter()
        .filter(|c| c.end - c.start < a.min_on_ms / 1000.0 - 1e-9)
        .count();
    println!(
        "{}: {} notes, {} contacts ({short} shorter than {} ms), {} taps -> {}",
        a.chart.display(),
        chart.notes.len(),
        stretched.len(),
        a.min_on_ms,
        script.taps.len(),
        a.out.display()
    );
    println!(
        "our judge on this script: score {} {:?}",
        result.score, result.counts
    );
    if let Some(assets) = std::env::var_os("WACCA_ASSETS") {
        // Live sync needs the template (first sound + envelope length + confirm) before the first input.
        let (bgm, rate) =
            wacca_realgame::sync::load_bgm(assets.as_ref(), &chart.header.music_file_path)
                .map_err(|e| e.to_string())?;
        let opts = wacca_realgame::sync::DetectorOptions::default();
        let first = wacca_realgame::sync::first_sound(&bgm, opts.silence).unwrap_or(0);
        let ready = first as f64 / f64::from(rate) + (opts.envelope_ms + opts.confirm_ms) / 1000.0;
        let first_input =
            script.taps.first().map_or(0.0, |t| t.at_ms / 1000.0) - chart.header.offset;
        println!(
            "BGM {}: first sound at {:.3} s, live sync ready ~{ready:.2} s, first input at BGM {first_input:.3} s",
            chart.header.music_file_path,
            first as f64 / f64::from(rate)
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    let Some(args) = parse_args(std::env::args().skip(1)) else {
        eprintln!(
            "usage: e2_script --chart <mer> --out <taps.json> [--min-on-ms 34] [--min-gap-ms 17]"
        );
        return ExitCode::FAILURE;
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args() {
        let a = parse_args(
            "--chart c.mer --out o.json --min-on-ms 20"
                .split(' ')
                .map(String::from),
        )
        .unwrap();
        assert_eq!((a.min_on_ms, a.min_gap_ms), (20.0, 17.0));
        assert!(parse_args("--chart c.mer".split(' ').map(String::from)).is_none());
        assert!(parse_args("--chart".split(' ').map(String::from)).is_none());
    }
}
