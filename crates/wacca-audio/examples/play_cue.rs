//! Play ~10 s of a BGM cue and print the audio clock.
//!
//! `cargo run -p wacca-audio --example play_cue -- <assets_dir> MER_BGM_S00_003 [start_seconds]`

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use wacca_audio::{BgmBank, Error, Player};

fn run(assets: PathBuf, cue: &str, start: f64) -> Result<(), Error> {
    let bank = BgmBank::open(assets.join("Sound").join("Bgm"))?;
    let loc = bank.locate(cue)?;
    println!(
        "{cue}: {} entry {} bytes {}..{}",
        loc.awb.display(),
        loc.entry.id,
        loc.entry.start,
        loc.entry.end
    );
    let player = Player::new()?;
    player.load(Box::new(bank.open_cue(cue)?))?;
    println!("duration {:.3} s", player.duration());
    player.seek(start)?;
    player.play();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(500));
        println!(
            "wall {:6.3}  position {:7.3}",
            t0.elapsed().as_secs_f64(),
            player.position()
        );
    }
    player.pause();
    Ok(())
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(assets), Some(cue)) = (args.next(), args.next()) else {
        eprintln!("usage: play_cue <assets_dir> <cue_name> [start_seconds]");
        return ExitCode::FAILURE;
    };
    let start = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    match run(assets.into(), &cue, start) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
