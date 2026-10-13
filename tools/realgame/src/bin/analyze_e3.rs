//! `analyze_e3 <run dir>... [--ffmpeg PATH] [--report <out.md>]`: per-note real judgements of E3 runs (doc 10)
//! from the debug panel, matched to the run's plan (`<script>.plan.json`, found from `meta.json`), then the pooled
//! report ([`wacca_realgame::e3_analysis::report`]).
//!
//! Per run it writes `panel.json` (every panel change, chart time; reused on later calls) and `e3.json` (outcomes).
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use wacca_realgame::e3::{self, Plan};
use wacca_realgame::e3_analysis::{self, RunResult};
use wacca_realgame::frame::Rgb;
use wacca_realgame::inject::{Kind, Record};
use wacca_realgame::panel::{self, Event};
use wacca_realgame::record::video;

#[derive(Debug, Default)]
struct Args {
    runs: Vec<PathBuf>,
    ffmpeg: Option<PathBuf>,
    report: Option<PathBuf>,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Option<Args> {
    let mut a = Args::default();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--ffmpeg" => a.ffmpeg = Some(it.next()?.into()),
            "--report" => a.report = Some(it.next()?.into()),
            _ if arg.starts_with("--") => return None,
            _ => a.runs.push(arg.into()),
        }
    }
    (!a.runs.is_empty()).then_some(a)
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, String> {
    let s = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", p.display()))
}

fn num(v: &serde_json::Value, ptr: &str) -> Result<f64, String> {
    v.pointer(ptr)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| format!("meta.json has no {ptr}"))
}

/// Panel changes over the whole video, in chart time.
fn decode_events(dir: &Path, meta: &serde_json::Value, ff: &Path) -> Result<Vec<Event>, String> {
    let size = (
        num(meta, "/window/width")? as usize,
        num(meta, "/window/height")? as usize,
    );
    if size != panel::SIZE {
        return Err(format!(
            "window {size:?}, panel regions are for {:?}",
            panel::SIZE
        ));
    }
    let first_frame_ms = num(meta, "/video/first_frame_ms")?;
    let bgm0_ms = num(meta, "/sync/bgm0_ms")?;
    let offset = num(meta, "/sync/chart_offset_s")?;
    let mut child = video::spawn_crop_decoder(ff, &dir.join("video.mkv"), "rgb24", panel::CROP)
        .map_err(|e| e.to_string())?;
    let stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Some(p) = video::parse_showinfo_pts(&line) {
                    let _ = tx.send(p);
                }
            }
        }
    });
    let mut out = child.stdout.take().ok_or("no decoder stdout")?;
    let [_, _, w, h] = panel::CROP;
    let mut buf = vec![0u8; w * h * 3];
    let mut tracker = panel::Tracker::default();
    while out.read_exact(&mut buf).is_ok() {
        let pts = rx.recv().map_err(|_| "decoder timestamps ended early")?;
        let t = (first_frame_ms + pts * 1000.0 - bgm0_ms) / 1000.0 + offset;
        tracker.push(t, Rgb::new(w, h, buf.clone()).ok_or("frame size")?);
    }
    let _ = child.wait();
    Ok(tracker.events)
}

fn analyse(dir: &Path, ff: &Path) -> Result<RunResult, String> {
    let meta: serde_json::Value = read_json(&dir.join("meta.json"))?;
    let script = meta
        .pointer("/inject/script")
        .and_then(serde_json::Value::as_str)
        .ok_or("meta.json has no inject.script")?;
    let plan: Plan = read_json(&e3::plan_path(Path::new(script)))?;
    let cache = dir.join("panel.json");
    let events: Vec<Event> = match read_json::<Vec<(f64, [bool; panel::ROWS])>>(&cache) {
        Ok(v) => v.into_iter().map(|(t, rows)| Event { t, rows }).collect(),
        Err(_) => {
            let ev = decode_events(dir, &meta, ff)?;
            let raw: Vec<(f64, [bool; panel::ROWS])> = ev.iter().map(|e| (e.t, e.rows)).collect();
            let json = serde_json::to_string(&raw).map_err(|e| e.to_string())?;
            std::fs::write(&cache, json).map_err(|e| format!("{}: {e}", cache.display()))?;
            ev
        }
    };
    let bytes = std::fs::read(&plan.chart).map_err(|e| format!("{}: {e}", plan.chart))?;
    let chart = wacca_chart::parse(&bytes).map_err(|e| e.to_string())?;
    let times: Vec<f64> = chart
        .notes
        .iter()
        .flat_map(|n| {
            let (s, e) = e3::span(n);
            [s, e]
        })
        .collect();
    let judged: Vec<f64> = events
        .iter()
        .filter(|e| !e.judgements().is_empty())
        .map(|e| e.t)
        .collect();
    let lag = e3_analysis::estimate_lag(&judged, &times).ok_or("no panel changes near notes")?;
    let inject = std::fs::read_to_string(dir.join("inject.jsonl")).map_err(|e| e.to_string())?;
    let edges: Vec<Record> = inject
        .lines()
        .filter_map(|l| serde_json::from_str::<Record>(l).ok())
        .filter(|r| r.kind == Kind::Edge)
        .collect();
    let errors: Vec<f64> = edges.iter().map(Record::write_error_ms).collect();
    let (write_p95_ms, write_max_ms) = e3_analysis::write_stats(&errors);
    // `t_script_ms` is chart time (ms) for song runs.
    let late: Vec<f64> = edges
        .iter()
        .filter(|r| r.write_error_ms() > e3_analysis::MAX_WRITE_ERROR_MS)
        .map(|r| r.t_script_ms / 1000.0)
        .collect();
    let (dropped, kept): (Vec<_>, Vec<_>) = plan
        .tests
        .iter()
        .partition(|t| e3_analysis::touched_by(t, &late));
    println!(
        "  injector write error p95 {write_p95_ms:.2} ms, max {write_max_ms:.2} ms ({} edges); {} late edges drop tests {:?}",
        errors.len(),
        late.len(),
        dropped.iter().map(|t| t.note).collect::<Vec<_>>()
    );
    // Clock drift from the visual timing check (`analyze_e2` on the same run, `e2.json`).
    let blocks: Vec<(f64, f64)> = read_json::<serde_json::Value>(&dir.join("e2.json"))
        .ok()
        .and_then(|v| v.get("visual_timing").cloned())
        .and_then(|v| serde_json::from_value::<Vec<(f64, f64, f64)>>(v).ok())
        .map(|b| b.into_iter().map(|(t, d, _)| (t, d)).collect())
        .unwrap_or_default();
    if blocks.is_empty() {
        println!(
            "  WARNING: no e2.json visual timing: clock drift not corrected (run analyze_e2 first)"
        );
    } else {
        println!("  visual timing blocks (chart s, δ ms): {blocks:.0?}");
    }
    // Expected spans: every note, test notes with their shift.
    let mut spans: Vec<(f64, f64)> = chart.notes.iter().map(e3::span).collect();
    for t in &plan.tests {
        if let (e3::Probe::Shift { ms }, Some(sp)) = (t.probe, spans.get_mut(t.note)) {
            sp.0 += ms / 1000.0;
            sp.1 = sp.1.max(sp.0);
        }
    }
    // Groups (E6): every change near either note goes to the test note.
    for t in &plan.tests {
        if let Some(j) = t.partner {
            spans[t.note] = (t.time, t.end);
            if let Some(sp) = spans.get_mut(j) {
                *sp = (t.time, t.end);
            }
        }
    }
    let assigned = e3_analysis::assign(&events, &spans, lag);
    let outcomes: Vec<_> = kept
        .into_iter()
        .map(|t| {
            let hits: Vec<&Event> = assigned[t.note].iter().map(|&k| &events[k]).collect();
            e3_analysis::Outcome {
                clock_ms: e3_analysis::clock_at(&blocks, t.time),
                ..e3_analysis::resolve(t, &hits, lag)
            }
        })
        .collect();
    let centre_ms = e3_analysis::touch_centre(&outcomes, 10);
    // Timing check: the touch centre of each half of the song must agree (a clock jump moves it).
    let mid = plan.tests.get(plan.tests.len() / 2).map_or(0.0, |t| t.time);
    let halves: Vec<Option<f64>> = [true, false]
        .iter()
        .map(|&first| {
            let part: Vec<_> = outcomes
                .iter()
                .filter(|o| (o.test.time < mid) == first)
                .cloned()
                .collect();
            e3_analysis::touch_centre(&part, 10)
        })
        .collect();
    let totals = [0, 1, 2, 3].map(|r| events.iter().filter(|e| e.rows[r]).count());
    println!(
        "{}: {} panel changes (M/Gr/Gd/Miss rows {totals:?}), {} judged notes in the chart, lag {:.1} ms, touch centre {centre_ms:.1?} ms (halves {halves:.1?})",
        dir.display(),
        events.len(),
        chart.notes.len(),
        lag * 1000.0
    );
    let run = RunResult {
        run: dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        lag,
        centre_ms,
        write_p95_ms,
        write_max_ms,
        outcomes,
    };
    let json = serde_json::to_string_pretty(&run).map_err(|e| e.to_string())?;
    let path = dir.join("e3.json");
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(run)
}

fn run(a: &Args) -> Result<(), String> {
    let path_var = std::env::var_os("PATH");
    let lad = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let ff = video::find_ffmpeg(a.ffmpeg.as_deref(), path_var.as_deref(), lad.as_deref())
        .map_err(|e| e.to_string())?;
    let runs = a
        .runs
        .iter()
        .map(|d| analyse(d, &ff))
        .collect::<Result<Vec<_>, _>>()?;
    let report = e3_analysis::report(&runs);
    println!("{report}");
    if let Some(p) = &a.report {
        std::fs::write(p, &report).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let Some(args) = parse_args(std::env::args().skip(1)) else {
        eprintln!("usage: analyze_e3 <run dir>... [--ffmpeg PATH] [--report <out.md>]");
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
        let a = parse_args("r1 r2 --report x.md".split(' ').map(String::from)).unwrap();
        assert_eq!(a.runs.len(), 2);
        assert_eq!(a.report, Some(PathBuf::from("x.md")));
        assert!(parse_args("--bogus".split(' ').map(String::from)).is_none());
        assert!(parse_args(std::iter::empty()).is_none());
    }
}
