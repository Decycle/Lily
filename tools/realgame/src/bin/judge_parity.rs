//! `judge_parity <run dir>... [--report <out.md>] [--table <out.csv>]`: experiment E4 (doc 10 §8). Feeds the
//! input each E3 run injected (`inject.jsonl`, on the judge clock: drift per 8-note block from `e2.json`, touch
//! centre from `e3.json`) through our judge and compares per-note verdicts with the real ones (`e3.json` for
//! probed notes, `panel.json` for unprobed notes that resolve to one panel change). Run `analyze_e2` and
//! `analyze_e3` on the runs first; run from the checkout root (plans are found via `meta.json`'s script path).
//!
//! Left out: notes next to a clock jump ([`e3_analysis::clock_at`] is `None`), notes within 0.3 s of a late
//! injector write, unresolved notes and, among unprobed notes, those with another note edge within 0.1 s.
//! Holds are also judged along the straight path between their authored control points (`.mer` render flag 1)
//! to see whether following the generated joints matters. `PARITY_DEBUG_NOTE=<index>` prints one note's input.
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use wacca_game::judge::JudgeTable;
use wacca_game::rules::{FRAME_SECS, JudgeClass};
use wacca_realgame::e3::{self, Class, Plan, Probe};
use wacca_realgame::e3_analysis::{self, RunResult};
use wacca_realgame::inject::{Kind, Record};
use wacca_realgame::panel::{self, Event};
use wacca_realgame::parity::{self, Row};

/// Unprobed notes need this much time (s) to every other note's start or end to be resolved from the panel.
const UNPROBED_GAP: f64 = 0.1;

#[derive(Debug, Default)]
struct Args {
    runs: Vec<PathBuf>,
    report: Option<PathBuf>,
    table: Option<PathBuf>,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Option<Args> {
    let mut a = Args::default();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--report" => a.report = Some(it.next()?.into()),
            "--table" => a.table = Some(it.next()?.into()),
            _ if arg.starts_with("--") => return None,
            _ => a.runs.push(arg.into()),
        }
    }
    (!a.runs.is_empty()).then_some(a)
}

fn read(p: &Path) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, String> {
    serde_json::from_str(&read(p)?).map_err(|e| format!("{}: {e}", p.display()))
}

/// One line per two-note group test (E6): `(agrees, markdown row)`.
type GroupLine = (bool, String);

/// Per-note rows and the run's group lines.
fn compare(dir: &Path) -> Result<(Vec<Row>, Vec<GroupLine>), String> {
    let name = dir
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let meta: serde_json::Value = read_json(&dir.join("meta.json"))?;
    let script = meta
        .pointer("/inject/script")
        .and_then(serde_json::Value::as_str)
        .ok_or("meta.json has no inject.script")?;
    let plan: Plan = read_json(&e3::plan_path(Path::new(script)))?;
    let run: RunResult = read_json(&dir.join("e3.json"))?;
    if !e3_analysis::writes_ok(&run) {
        return Err(format!(
            "{name}: injector write error p95 {:.1} ms; run excluded",
            run.write_p95_ms
        ));
    }
    let centre = run.centre_ms.ok_or(format!("{name}: no touch centre"))?;
    let bytes = std::fs::read(&plan.chart).map_err(|e| format!("{}: {e}", plan.chart))?;
    let chart = wacca_chart::parse(&bytes).map_err(|e| e.to_string())?;
    let blocks: Vec<(f64, f64)> = read_json::<serde_json::Value>(&dir.join("e2.json"))?
        .get("visual_timing")
        .cloned()
        .and_then(|v| serde_json::from_value::<Vec<(f64, f64, f64)>>(v).ok())
        .ok_or(format!("{name}: e2.json has no visual_timing"))?
        .into_iter()
        .map(|(t, d, _)| (t, d))
        .collect();
    let records: Vec<Record> = read(&dir.join("inject.jsonl"))?
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let late: Vec<f64> = records
        .iter()
        .filter(|r| r.kind == Kind::Edge && r.write_error_ms() > e3_analysis::MAX_WRITE_ERROR_MS)
        .map(|r| r.t_script_ms / 1000.0)
        .collect();

    // Ours on the injected input, with stored joints and with straight control paths.
    let frames = parity::injected_frames(&records, &blocks, centre);
    let ours = e3::judge_frames(&chart, frames.clone());
    if let Ok(dbg) = std::env::var("PARITY_DEBUG_NOTE") {
        let i: usize = dbg.parse().unwrap_or(0);
        if let Some(n) = chart.notes.get(i) {
            eprintln!("DEBUG note {i}: {:?} {:?}", n.arc, n.body);
            eprintln!("  ours {:?}", ours[i]);
            let (s, e) = e3::span(n);
            for f in frames
                .iter()
                .filter(|f| f.time > s - 0.3 && f.time < e + 0.3)
            {
                let lanes: Vec<(u8, usize)> = (0..60u8)
                    .flat_map(|l| (0..4).map(move |d| (l, d)))
                    .filter(|&(l, d)| f.sensors.get(l, d))
                    .collect();
                eprintln!(
                    "  {:.4} ({:+.1} f) {lanes:?}",
                    f.time,
                    (f.time - s) / FRAME_SECS
                );
            }
        }
    }
    let rows_mer = parity::hold_rows(&String::from_utf8_lossy(&bytes));
    let flags: Vec<Option<Vec<bool>>> = chart
        .notes
        .iter()
        .map(|n| match &n.body {
            wacca_chart::NoteBody::Hold { nodes } => {
                parity::control_flags(&rows_mer, n.file_index, nodes.len())
            }
            _ => None,
        })
        .collect();
    let control = parity::with_control_paths(&chart, &flags);
    let ours_control = e3::judge_frames(&control, frames);

    // Real verdicts: probed notes from e3.json, unprobed ones from the panel.
    let mut real: Vec<Option<(Option<Probe>, wacca_game::rules::Judgement)>> =
        vec![None; chart.notes.len()];
    // Groups (E6): all panel changes of both notes against ours for both (`analyze_e3` pooled them).
    let mut groups = Vec::new();
    for o in &run.outcomes {
        let (Some(j), Some(_)) = (o.test.partner, o.clock_ms) else {
            continue;
        };
        let mut real: Vec<_> = o.seen.iter().map(|&x| Some(x)).collect();
        real.sort_by_key(|x| x.map(e3::rank));
        let mine = [o.test.note, j].map(|k| ours.get(k).copied().flatten().map(|e| e.judgement));
        let agree = e3_analysis::judgement_set(real.clone()) == e3_analysis::judgement_set(mine);
        let offs = [o.test.note, j].map(|k| {
            ours.get(k)
                .copied()
                .flatten()
                .and_then(|e| e.offset)
                .map_or("–".into(), |x| format!("{:+.1}", x / FRAME_SECS))
        });
        groups.push((
            agree,
            format!(
                "| {name} | {} | {:?} | {:?} | {real:?} | {mine:?} | {} / {} | {} |",
                o.test.note,
                o.test.class,
                o.test.probe,
                offs[0],
                offs[1],
                if agree { "✔" } else { "✘" }
            ),
        ));
    }
    for o in &run.outcomes {
        if o.test.partner.is_some() {
            continue;
        }
        if let (Some(j), Some(_)) = (o.real, o.clock_ms) {
            real[o.test.note] = Some((Some(o.test.probe), j));
        }
    }
    let events: Vec<Event> = read_json::<Vec<(f64, [bool; panel::ROWS])>>(&dir.join("panel.json"))?
        .into_iter()
        .map(|(t, rows)| Event { t, rows })
        .collect();
    let mut spans: Vec<(f64, f64)> = chart.notes.iter().map(e3::span).collect();
    for t in &plan.tests {
        if let (Probe::Shift { ms }, Some(sp)) = (t.probe, spans.get_mut(t.note)) {
            sp.0 += ms / 1000.0;
            sp.1 = sp.1.max(sp.0);
        }
    }
    let assigned = e3_analysis::assign(&events, &spans, run.lag);
    let tested: Vec<usize> = plan
        .tests
        .iter()
        .flat_map(|t| std::iter::once(t.note).chain(t.partner))
        .collect();
    let ends: Vec<f64> = spans.iter().flat_map(|&(s, e)| [s, e]).collect();
    for (i, hits) in assigned.iter().enumerate() {
        if tested.contains(&i) {
            continue;
        }
        let (s, e) = spans[i];
        let crowded = ends.iter().enumerate().any(|(k, &x)| {
            k / 2 != i && ((x - s).abs() < UNPROBED_GAP || (x - e).abs() < UNPROBED_GAP)
        });
        let late_near = late.iter().any(|&t| t >= s - 0.3 && t <= e + 0.3);
        let clock_ok = e3_analysis::clock_at(&blocks, s).is_some();
        let seen: Vec<_> = hits.iter().flat_map(|&k| events[k].judgements()).collect();
        if !crowded
            && !late_near
            && clock_ok
            && s >= plan.spec.min_time
            && hits.len() == 1
            && seen.len() == 1
        {
            real[i] = Some((None, seen[0]));
        }
    }

    let table = JudgeTable::reverse();
    let rows = real
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let (probe, real) = (*r)?;
            let note = &chart.notes[i];
            // Press probes (E5): the contact's onset stands for the shift.
            let shift = match probe {
                Some(Probe::Shift { ms } | Probe::Press { from_ms: ms, .. }) => ms,
                _ => 0.0,
            };
            // Our judge's measured offset where it has one (chains carry none), else the scripted one.
            let class = parity::judge_class(&chart, i)?;
            let nominal = shift - parity::drift_ms(&blocks, note.time) - centre;
            let offset_ms = match ours[i].and_then(|e| e.offset) {
                Some(o) if class != JudgeClass::Chain => o * 1000.0,
                _ => nominal,
            };
            let offset_f = offset_ms / (FRAME_SECS * 1000.0);
            let clock_f = (nominal - shift) / (FRAME_SECS * 1000.0);
            let path_dev = match (&note.body, &flags[i]) {
                (wacca_chart::NoteBody::Hold { nodes }, Some(f)) => {
                    Some(parity::path_deviation(nodes, f))
                }
                _ => None,
            };
            Some(Row {
                run: name.clone(),
                note: i,
                time: note.time,
                class: e3::class(&note.body),
                probe,
                offset_f,
                real,
                ours: ours[i].map(|e| e.judgement),
                ours_control: ours_control[i].map(|e| e.judgement),
                path_dev,
                clock_f,
                margin_f: parity::margin_frames(&table, class, probe, offset_f, clock_f),
            })
        })
        .collect::<Vec<_>>();
    let holds_unflagged = flags
        .iter()
        .zip(&chart.notes)
        .filter(|(f, n)| f.is_none() && e3::class(&n.body) == Class::Hold)
        .count();
    // Whole chart (compared or not): how far generated joints stray, and where the control path changes ours.
    let max_dev = chart
        .notes
        .iter()
        .zip(&flags)
        .filter_map(|(n, f)| match (&n.body, f) {
            (wacca_chart::NoteBody::Hold { nodes }, Some(f)) => {
                Some(parity::path_deviation(nodes, f))
            }
            _ => None,
        })
        .fold(0.0, f64::max);
    let path_changed: Vec<usize> = (0..chart.notes.len())
        .filter(|&i| ours[i].map(|e| e.judgement) != ours_control[i].map(|e| e.judgement))
        .collect();
    println!(
        "{name}: {} records injected, {} notes compared ({} probed), {} late edges, {holds_unflagged} holds without \
         control flags; generated joints ≤ {max_dev:.1} lanes off the control path, control path changes ours on \
         notes {path_changed:?}",
        records.len(),
        rows.len(),
        rows.iter().filter(|r| r.probe.is_some()).count(),
        late.len()
    );
    Ok((rows, groups))
}

fn run(a: &Args) -> Result<(), String> {
    let (mut rows, mut groups) = (Vec::new(), Vec::new());
    for d in &a.runs {
        match compare(d) {
            Ok((r, g)) => {
                rows.extend(r);
                groups.extend(g);
            }
            Err(e) => eprintln!("skipped: {e}"),
        }
    }
    let mut report = parity::report(&rows);
    if !groups.is_empty() {
        report.push_str(&format!(
            "\n## Two-note groups (E6): {} / {} agree\n\nOurs on the injected input; offsets in frames.\n\n\
             | Run | Note | Class | Probe | Real changes | Ours (1st, 2nd) | Offsets | Agree |\n|---|---|---|---|---|---|---|---|\n",
            groups.iter().filter(|g| g.0).count(),
            groups.len()
        ));
        for (_, line) in &groups {
            report.push_str(line);
            report.push('\n');
        }
    }
    println!("{report}");
    if let Some(p) = &a.report {
        std::fs::write(p, &report).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if let Some(p) = &a.table {
        std::fs::write(p, parity::csv(&rows)).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

fn main() -> ExitCode {
    env_logger::init();
    let Some(args) = parse_args(std::env::args().skip(1)) else {
        eprintln!("usage: judge_parity <run dir>... [--report <out.md>] [--table <out.csv>]");
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
        let a = parse_args("r1 r2 --table t.csv".split(' ').map(String::from)).unwrap();
        assert_eq!(a.runs.len(), 2);
        assert_eq!(a.table, Some(PathBuf::from("t.csv")));
        assert!(parse_args("--bogus".split(' ').map(String::from)).is_none());
        assert!(parse_args(std::iter::empty()).is_none());
    }
}
