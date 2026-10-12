//! Parses every real chart. Run with `WACCA_ASSETS=<assets dir> cargo test -p wacca-chart -- --ignored`.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use wacca_chart::{Difficulty, parse};

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
fn parses_all_charts() {
    let music =
        PathBuf::from(std::env::var("WACCA_ASSETS").expect("WACCA_ASSETS")).join("MusicData");
    let mut files = Vec::new();
    mer_files(&music, &mut files);
    assert!(!files.is_empty());
    for path in &files {
        let chart =
            parse(&std::fs::read(path).unwrap()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert!(
            (10.0..300.0).contains(&chart.end_time),
            "{path:?}: length {} s",
            chart.end_time
        );
        assert!(
            chart.notes.iter().all(|n| n.time <= chart.end_time),
            "{path:?}: note after EOF"
        );
    }
    println!("parsed {} charts", files.len());

    let counts: Vec<usize> = [Difficulty::Normal, Difficulty::Hard, Difficulty::Expert]
        .iter()
        .map(|d| {
            let path = music.join("S01-032").join(d.chart_file_name("S01-032"));
            parse(&std::fs::read(path).unwrap()).unwrap().notes.len()
        })
        .collect();
    assert_eq!(counts, [200, 333, 692]);
}
