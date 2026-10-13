//! `e3_script --chart <mer> --spec <spec.json> --out <x.taps.json> [--seed N]`: judgement-probe input for a chart
//! ([`wacca_realgame::e3`]): the perfect script with one probe per isolated test note. Writes the tap script and
//! `x.plan.json` (test notes, probes, our judge's verdicts) for `analyze_e3`.
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use wacca_realgame::e3::{self, Plan, Spec};
use wacca_realgame::timeline::Script;

#[derive(Debug)]
struct Args {
    chart: PathBuf,
    spec: PathBuf,
    out: PathBuf,
    seed: Option<u64>,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Option<Args> {
    let (mut chart, mut spec, mut out, mut seed) = (None, None, None, None);
    while let Some(flag) = it.next() {
        let v = it.next()?;
        match flag.as_str() {
            "--chart" => chart = Some(v.into()),
            "--spec" => spec = Some(v.into()),
            "--out" => out = Some(v.into()),
            "--seed" => seed = Some(v.parse().ok()?),
            _ => return None,
        }
    }
    Some(Args {
        chart: chart?,
        spec: spec?,
        out: out?,
        seed,
    })
}

fn run(a: &Args) -> Result<(), String> {
    let read = |p: &PathBuf| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
    let chart = wacca_chart::parse(&read(&a.chart)?).map_err(|e| e.to_string())?;
    let mut spec: Spec = serde_json::from_slice(&read(&a.spec)?).map_err(|e| e.to_string())?;
    spec.seed = a.seed.unwrap_or(spec.seed);
    let (taps, tests) = e3::build(&chart, &spec);
    let write = |p: &PathBuf, json: String| {
        std::fs::write(p, json).map_err(|e| format!("{}: {e}", p.display()))
    };
    let script = Script { taps };
    write(
        &a.out,
        serde_json::to_string_pretty(&script).map_err(|e| e.to_string())?,
    )?;
    let plan_path = e3::plan_path(&a.out);
    let plan = Plan {
        chart: a.chart.display().to_string(),
        spec,
        tests,
    };
    write(
        &plan_path,
        serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?,
    )?;

    let mut counts: BTreeMap<String, (usize, BTreeMap<String, usize>)> = BTreeMap::new();
    for t in &plan.tests {
        let kind = format!("{:?} {}", t.class, probe_kind(&t.probe));
        let e = counts.entry(kind).or_default();
        e.0 += 1;
        *e.1.entry(format!("{:?}", t.ours)).or_default() += 1;
    }
    println!(
        "{}: {} notes, {} test notes, {} taps -> {}, {}",
        a.chart.display(),
        chart.notes.len(),
        plan.tests.len(),
        script.taps.len(),
        a.out.display(),
        plan_path.display()
    );
    for (kind, (n, ours)) in counts {
        println!("  {kind}: {n} (ours {ours:?})");
    }
    if let Some(first) = script.taps.first() {
        println!("first input at chart {:.3} s", first.at_ms / 1000.0);
    }
    Ok(())
}

fn probe_kind(p: &e3::Probe) -> &'static str {
    match p {
        e3::Probe::Shift { .. } => "shift",
        e3::Probe::HoldGap { .. } => "hold_gap",
        e3::Probe::HoldRelease { .. } => "hold_release",
        e3::Probe::SlideJump { .. } => "slide_jump",
        e3::Probe::SnapMove { .. } => "snap_move",
    }
}

fn main() -> ExitCode {
    let Some(args) = parse_args(std::env::args().skip(1)) else {
        eprintln!(
            "usage: e3_script --chart <mer> --spec <spec.json> --out <x.taps.json> [--seed N]"
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
            "--chart c.mer --spec s.json --out o.taps.json --seed 3"
                .split(' ')
                .map(String::from),
        )
        .unwrap();
        assert_eq!((a.out, a.seed), (PathBuf::from("o.taps.json"), Some(3)));
        assert!(parse_args("--chart c.mer --out o".split(' ').map(String::from)).is_none());
    }
}
