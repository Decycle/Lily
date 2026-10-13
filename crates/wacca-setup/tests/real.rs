//! End-to-end imports from the real game files (ignored by default):
//! `WACCA_ARCHIVES=<folder with the nine .7z> cargo test --release -p wacca-setup -- --ignored --nocapture archives`
//! `WACCA_GAME=<installed game folder> cargo test --release -p wacca-setup -- --ignored --nocapture installed`
//! Output goes to `%TEMP%\wacca-setup-e2e-*` and is deleted afterwards; the sources are only read.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use wacca_setup::{Finding, Plan, Progress, Source, Summary, detect, run};

fn env_path(var: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(var).unwrap_or_else(|| panic!("set {var}")))
}

fn import(folder: &Path, tag: &str) -> Summary {
    let found = detect(folder).unwrap();
    println!("detected: version {:?}, {:?}", found.version, found.report);
    let dest = std::env::temp_dir().join(format!("wacca-setup-e2e-{tag}"));
    let _ = std::fs::remove_dir_all(&dest);
    let plan = Plan {
        include_movies: false,
        dest: dest.clone(),
    };
    let start = Instant::now();
    let mut last = None;
    let summary = run(&found.source, &plan, &mut |p: Progress| {
        if last.as_ref() != Some(&(p.phase, p.detail.clone())) {
            println!("{:>7.1?} {:?} {}", start.elapsed(), p.phase, p.detail);
            last = Some((p.phase, p.detail));
        }
        true
    })
    .unwrap();
    println!("total {:.1?}", start.elapsed());
    println!("timings {:?}", summary.timings);
    println!(
        "assets {:.2} GB, data {:.2} GB",
        summary.assets_bytes as f64 / 1e9,
        summary.data_bytes as f64 / 1e9
    );
    println!(
        "songs {} playable ({} skipped), charts {} playable, skipped {:?}",
        summary.songs_playable,
        summary.songs_skipped,
        summary.charts_playable,
        summary.charts_skipped
    );
    println!("warnings {:?}", summary.warnings);
    // The result is itself a valid installed game.
    let again = detect(&dest.join("assets")).unwrap();
    assert!(matches!(again.source, Source::Installed { .. }));
    assert!(again.report.contains(&Finding::Movies {
        present: false,
        bytes: 0
    }));
    assert!(dest.join("data/songs.json").is_file());
    assert!(!dest.join(".setup-tmp").exists());
    std::fs::remove_dir_all(&dest).unwrap();
    summary
}

#[test]
#[ignore = "needs WACCA_ARCHIVES (the nine WACCA Reverse .7z); extracts ~6 GB"]
fn archives_end_to_end() {
    let s = import(&env_path("WACCA_ARCHIVES"), "archives");
    assert!(s.songs_playable >= 390, "{s:?}");
}

#[test]
#[ignore = "needs WACCA_GAME (an installed game folder, read-only)"]
fn installed_end_to_end() {
    let s = import(&env_path("WACCA_GAME"), "installed");
    assert!(s.songs_playable >= 390, "{s:?}");
}
