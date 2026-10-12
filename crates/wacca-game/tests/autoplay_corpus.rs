//! End-to-end check over every real chart: parse -> autoplay -> judge -> tally.
//! Run with `WACCA_ASSETS=<assets dir> cargo test -p wacca-game --test autoplay_corpus -- --ignored --nocapture`.
#![allow(clippy::unwrap_used)] // test-only helper code

use std::path::{Path, PathBuf};

use wacca_game::judge::{Judge, JudgeTable, autoplay_frames, class_of};
use wacca_game::score::PlayTally;

fn mer_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            mer_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "mer") {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn autoplay_scores_max_on_every_chart() {
    assert_autoplay_perfect(|c| c);
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn autoplay_scores_max_on_every_mirrored_chart() {
    assert_autoplay_perfect(|c| wacca_chart::mirrored(&c));
}

/// Parse every chart, apply `transform`, autoplay it and require 1,000,000.
fn assert_autoplay_perfect(transform: impl Fn(wacca_chart::Chart) -> wacca_chart::Chart) {
    let assets = PathBuf::from(std::env::var("WACCA_ASSETS").expect("WACCA_ASSETS"));
    let mut files = Vec::new();
    mer_files(&assets.join("MusicData"), &mut files);
    assert!(!files.is_empty());

    let mut imperfect = Vec::new();
    for path in &files {
        let chart = transform(wacca_chart::parse(&std::fs::read(path).unwrap()).unwrap());
        if chart.notes.is_empty() {
            // Placeholder charts of removed songs (e.g. S01-053_00) have no notes.
            continue;
        }
        let notes: Vec<_> = chart.notes.iter().map(|n| (class_of(n), n.bonus)).collect();
        let mut judge = Judge::new(&chart, JudgeTable::reverse());
        let mut tally = PlayTally::new(&notes, true);
        for frame in autoplay_frames(&chart) {
            judge.update(&frame).iter().for_each(|e| tally.apply(e));
        }
        judge.finish().iter().for_each(|e| tally.apply(e));
        let result = tally.finish(0.0);
        if result.score != 1_000_000 {
            imperfect.push(format!(
                "{}: {} ({:?})",
                path.file_name().unwrap().to_string_lossy(),
                result.score,
                result.counts
            ));
        }
    }
    println!(
        "{} charts, {} not perfect under autoplay",
        files.len(),
        imperfect.len()
    );
    for line in imperfect.iter().take(30) {
        println!("  {line}");
    }
    assert!(imperfect.is_empty());
}
