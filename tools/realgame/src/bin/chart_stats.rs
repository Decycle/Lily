//! `chart_stats <songs.json> <assets> [--difficulty Normal|Hard|...]`: per chart, the note kinds E6 needs (doc 10
//! §10): isolated snaps by direction and isolated two-note groups (chords and close pairs, arcs disjoint or
//! overlapping), with each song's carousel index (`songs.json` order) and genre. Tab-separated on stdout.

use std::path::PathBuf;
use std::process::ExitCode;
use wacca_chart::{Difficulty, NoteBody, SnapDir};
use wacca_game::catalog::Song;
use wacca_realgame::e3;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(songs), Some(assets)) = (args.first(), args.get(1)) else {
        eprintln!("usage: chart_stats <songs.json> <assets> [--difficulty <name>]");
        return ExitCode::FAILURE;
    };
    let want = args.get(3).cloned().unwrap_or_else(|| "Normal".into());
    let songs: Vec<Song> = match std::fs::read(songs)
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{songs}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "index\tid\tgenre\tlevel\tnotes\tsnap_in\tsnap_out\tiso_snap_in\tiso_snap_out\tchord_disj\tchord_ovl\tclose_disj\tclose_ovl\ttitle"
    );
    for (index, song) in songs.iter().enumerate() {
        let Some(entry) = song
            .charts
            .iter()
            .find(|c| format!("{:?}", c.difficulty) == want)
        else {
            continue;
        };
        let _: Difficulty = entry.difficulty;
        let path = PathBuf::from(assets).join(&entry.chart_path);
        let Ok(chart) = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| wacca_chart::parse(&b).map_err(|e| e.to_string()))
        else {
            continue;
        };
        let snaps = |dir: SnapDir, idx: &mut dyn Iterator<Item = usize>| {
            idx.filter(|&i| chart.notes[i].body == NoteBody::Snap(dir))
                .count()
        };
        let iso = e3::isolated(&chart, 0.35, 5.0);
        let g = e3::isolated_pairs(&chart, 0.3, 5.0, e3::PAIR_DT);
        let count = |sim: bool, ovl: bool| {
            g.iter()
                .filter(|p| {
                    let (a, b) = (&chart.notes[p.0], &chart.notes[p.1]);
                    let touchy = |n: &wacca_chart::Note| {
                        matches!(n.body, NoteBody::Touch | NoteBody::Hold { .. })
                    };
                    touchy(a)
                        && touchy(b)
                        && ((b.time - a.time).abs() < 1e-6) == sim
                        && e3::arcs_overlap(a.arc, b.arc) == ovl
                })
                .count()
        };
        println!(
            "{index}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            song.id,
            song.genre,
            entry.level,
            chart.notes.len(),
            snaps(SnapDir::In, &mut (0..chart.notes.len())),
            snaps(SnapDir::Out, &mut (0..chart.notes.len())),
            snaps(SnapDir::In, &mut iso.iter().copied()),
            snaps(SnapDir::Out, &mut iso.iter().copied()),
            count(true, false),
            count(true, true),
            count(false, false),
            count(false, true),
            song.title
        );
    }
    ExitCode::SUCCESS
}
