//! List the SE cues, then play a BGM cue with the default tap sound every 500 ms and one hold.
//!
//! `cargo run -p wacca-audio --example sfx -- <assets_dir> [bgm_cue]`

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use wacca_audio::{BgmBank, DEFAULT_SE_SET, Error, HitSounds, Player, SeBank};

fn run(assets: PathBuf, bgm: &str) -> Result<(), Error> {
    let se = SeBank::open(assets.join("Sound").join("Se"))?;
    for name in se.acb().cue_names() {
        let tracks: Vec<String> = se
            .acb()
            .tracks(name)?
            .iter()
            .map(|t| {
                let w = &t.waveform;
                let codec = if w.encode_type == 0 { "ADX" } else { "HCA" };
                let lp = w
                    .loop_range
                    .map_or(String::new(), |(s, e)| format!(" loop {s}..{e}"));
                format!(
                    "{codec} {}ch {} Hz {} frames{lp}",
                    w.channels, w.sample_rate, w.num_samples
                )
            })
            .collect();
        println!("{name}: {}", tracks.join(" + "));
    }

    let bank = BgmBank::open(assets.join("Sound").join("Bgm"))?;
    let player = Player::new()?;
    player.load(Box::new(bank.open_cue(bgm)?))?;
    let hits = HitSounds::load(&se, &DEFAULT_SE_SET, player.sample_rate())?;
    player.set_volume(0.5);
    player.play();
    std::thread::sleep(Duration::from_secs(2));
    for _ in 0..8 {
        for s in &hits.marvelous {
            player.play_sfx(s, 1.0)?;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("hold for 1.5 s");
    let voices = hits
        .hold_marvelous
        .iter()
        .map(|s| player.play_sfx(s, 1.0))
        .collect::<Result<Vec<_>, _>>()?;
    std::thread::sleep(Duration::from_millis(1500));
    for v in voices {
        player.stop_sfx(v)?;
    }
    std::thread::sleep(Duration::from_secs(1));
    player.pause();
    Ok(())
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(assets) = args.next() else {
        eprintln!("usage: sfx <assets_dir> [bgm_cue]");
        return ExitCode::FAILURE;
    };
    let bgm = args.next().unwrap_or_else(|| "MER_BGM_S00_003".into());
    match run(assets.into(), &bgm) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
