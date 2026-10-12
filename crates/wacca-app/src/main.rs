//! WACCA_slim. M4: play one chart with audio, touch/mouse input, judgement and score.
//!
//! `wacca-app --assets <dir> --chart MusicData/S00-003/S00-003_02.mer [--autoplay] [--speed 4.0] [--offset-ms N]`
//! Keys: Esc give up, Space pause, Up/Down note speed. `RUST_LOG=debug` logs every judgement.
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

mod args;
mod record;
mod session;
mod shell;
mod touch_mapping;

use std::path::PathBuf;
use std::process::ExitCode;

use wacca_audio::{BgmBank, Player};
use wacca_game::grade::PlayResult;
use wacca_game::save::{self, Settings};

use crate::args::{Args, ArgsError, ChartRef};
use crate::record::RecordError;
use crate::shell::{Outcome, Play, ShellError};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Args(#[from] ArgsError),
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("chart: {0}")]
    Chart(#[from] wacca_chart::ParseError),
    #[error("audio: {0}")]
    Audio(#[from] wacca_audio::Error),
    #[error(transparent)]
    Shell(#[from] ShellError),
    #[error(transparent)]
    Record(#[from] RecordError),
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match args::parse(std::env::args().skip(1))
        .map_err(Error::from)
        .and_then(|a| run(&a))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), Error> {
    let chart_ref = args::resolve_chart(args)?;
    let bytes = std::fs::read(&chart_ref.path).map_err(|source| Error::Io {
        path: chart_ref.path.clone(),
        source,
    })?;
    let chart = wacca_chart::parse(&bytes)?;
    let cue = &chart.header.music_file_path;
    let bank = BgmBank::open(args.assets.join("Sound").join("Bgm"))?;
    let player = Player::new(Box::new(bank.open_cue(cue)?))?;
    log::info!("{}: cue {cue}", chart_ref.path.display());

    let songs = record::load_songs(&args.data.join("songs.json"))?;
    let song = songs
        .as_deref()
        .zip(chart_ref.folder.as_deref())
        .and_then(|(songs, folder)| record::find_song(songs, folder));
    let entry = song.zip(chart_ref.difficulty).and_then(|(s, d)| s.chart(d));
    let norma = entry
        .map(|e| e.norma)
        .or(chart_ref.difficulty.map(record::default_norma))
        .unwrap_or(record::default_norma(wacca_chart::Difficulty::Expert));
    let title = song.map_or_else(
        || chart_ref.folder.clone().unwrap_or_default(),
        |s| s.title.clone(),
    );

    let outcome = shell::run(Play {
        title,
        chart,
        player,
        autoplay: args.autoplay,
        speed: args.speed,
        offset: args.offset,
        norma,
        bonus_effect: Settings::default().bonus_effect,
    })?;
    let (result, completed) = match outcome {
        Outcome::Completed(r) => (r, true),
        Outcome::GaveUp(r) => (r, false),
    };
    print_result(&result, completed);
    if args.autoplay {
        log::info!("autoplay: not saved");
        return Ok(());
    }
    save_play(&chart_ref, song.map(|s| s.id), &result)
}

fn print_result(r: &PlayResult, completed: bool) {
    let c = &r.counts;
    println!(
        "{}",
        if completed {
            "RESULT"
        } else {
            "RESULT (given up)"
        }
    );
    println!("score      {}", r.score);
    println!("grade      {:?}", r.grade);
    println!("lamp       {:?}", r.lamp);
    println!("max combo  {} / {} notes", r.max_combo, r.total_notes);
    println!(
        "judgements marvelous {}  great {}  good {}  miss {}  (fast {} / late {})",
        c.marvelous, c.great, c.good, c.miss, c.fast, c.late
    );
    println!("gauge      {:.1} %", r.gauge * 100.0);
}

fn save_play(chart: &ChartRef, song_id: Option<u32>, r: &PlayResult) -> Result<(), Error> {
    let (Some(id), Some(d)) = (song_id, chart.difficulty) else {
        log::warn!("song id or difficulty unknown (no data/songs.json entry?); result not saved");
        return Ok(());
    };
    let Some(path) = save::default_path() else {
        log::warn!("%APPDATA% not set; result not saved");
        return Ok(());
    };
    let best = record::save_result(&path, id, d, r)?;
    log::info!(
        "saved to {}{}",
        path.display(),
        if best { " (new best)" } else { "" }
    );
    Ok(())
}
