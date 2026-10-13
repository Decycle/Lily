//! Checks against the real game files. Run with
//! `WACCA_ASSETS=<path to assets> cargo test -p wacca-audio -- --ignored`.
#![allow(clippy::unwrap_used)] // test helpers

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use wacca_audio::cri::afs2;
use wacca_audio::{BgmBank, DEFAULT_SE_SET, Decoder, Error, HitSounds, SeBank};

fn se_bank() -> SeBank {
    SeBank::open(assets().join("Sound").join("Se")).unwrap()
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn every_se_cue_decodes() {
    let bank = se_bank();
    let names: Vec<_> = bank.acb().cue_names().map(str::to_string).collect();
    assert_eq!(names.len(), 381);
    let (mut hca, mut adx, mut looped, mut silent) = (0, 0, 0, Vec::new());
    for name in &names {
        let tracks = bank.acb().tracks(name).unwrap();
        if tracks.is_empty() {
            silent.push(name.clone());
            continue;
        }
        for (t, sfx) in tracks.iter().zip(bank.decode_cue(name).unwrap()) {
            let w = &t.waveform;
            match w.encode_type {
                0 => {
                    // Wrong coefficients or nibble order would blow up into clipped noise.
                    let peak = sfx.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
                    let clipped = sfx.samples.iter().filter(|v| v.abs() >= 0.999).count();
                    assert!(peak > 0.05 && clipped * 1000 < sfx.samples.len(), "{name}");
                    adx += 1;
                }
                2 => hca += 1,
                other => panic!("{name}: encode type {other}"),
            }
            assert_eq!(
                (sfx.rate, sfx.channels, sfx.frames()),
                (w.sample_rate, w.channels.into(), w.num_samples as usize),
                "{name}"
            );
            assert!(sfx.samples.iter().all(|v| v.is_finite()), "{name}");
            assert_eq!(sfx.loop_range.is_some(), w.loop_range.is_some(), "{name}");
            looped += usize::from(sfx.loop_range.is_some());
        }
    }
    eprintln!("SE tracks: {hca} HCA, {adx} ADX, {looped} looped; silent cues {silent:?}");
    // 393 waveforms; hold cues reuse the tap waveform as their attack track.
    assert_eq!(adx, 20);
    assert_eq!(hca + adx, 407);
    assert_eq!(silent, ["blank"]);
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn default_hit_sounds_resolve() {
    let hits = HitSounds::load(&se_bank(), &DEFAULT_SE_SET, 48000).unwrap();
    for one_shot in [
        &hits.marvelous,
        &hits.good,
        &hits.slide_marvelous,
        &hits.slide_good,
        &hits.snap_marvelous,
        &hits.snap_good,
        &hits.chain,
    ] {
        assert_eq!(one_shot.len(), 1);
        assert_eq!(one_shot[0].rate, 48000);
        assert!(one_shot[0].loop_range.is_none());
        let peak = one_shot[0].samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.05, "peak {peak}");
    }
    // Hold = looping sustain + one-shot attack (the tap sound); re-hold is only the sustain.
    assert_eq!(hits.hold_good.len(), 2);
    assert_eq!(hits.hold_good[0].loop_range, Some((80825, 202520)));
    assert_eq!(hits.hold_good[1], hits.good[0]);
    assert_eq!(hits.hold_marvelous[0].loop_range, Some((78381, 171936)));
    assert_eq!(hits.re_hold.len(), 1);
    assert_eq!(hits.re_hold[0].loop_range, Some((92875, 163251)));
}

fn assets() -> PathBuf {
    PathBuf::from(std::env::var("WACCA_ASSETS").expect("set WACCA_ASSETS to the assets directory"))
}

fn bank() -> BgmBank {
    BgmBank::open(assets().join("Sound").join("Bgm")).unwrap()
}

/// `#MUSIC_FILE_PATH` of every chart (the tutorial chart is UTF-16LE with BOM).
fn chart_cues() -> BTreeSet<String> {
    let mut cues = BTreeSet::new();
    for dir in std::fs::read_dir(assets().join("MusicData")).unwrap() {
        for f in std::fs::read_dir(dir.unwrap().path()).unwrap() {
            let path = f.unwrap().path();
            if path.extension().is_none_or(|e| e != "mer") {
                continue;
            }
            let raw = std::fs::read(&path).unwrap();
            let text = match raw.strip_prefix(&[0xFF, 0xFE]) {
                Some(utf16) => String::from_utf16_lossy(
                    &utf16
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&c| u16::from_le_bytes(c))
                        .collect::<Vec<_>>(),
                ),
                None => String::from_utf8_lossy(&raw).into_owned(),
            };
            for line in text.lines() {
                if let Some(cue) = line.strip_prefix("#MUSIC_FILE_PATH") {
                    cues.insert(cue.trim().to_string());
                }
            }
        }
    }
    cues
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn all_bgm_cues_resolve_to_hca_entries() {
    let bank = bank();
    let names: Vec<_> = bank.acb().cue_names().map(str::to_string).collect();
    assert_eq!(names.len(), 417);
    let mut resolved = 0;
    for name in &names {
        match bank.locate(name) {
            Ok(loc) => {
                let mut f = File::open(&loc.awb).unwrap();
                assert!(loc.entry.end <= f.metadata().unwrap().len(), "{name}");
                let mut sig = [0; 4];
                f.seek(SeekFrom::Start(loc.entry.start)).unwrap();
                f.read_exact(&mut sig).unwrap();
                assert_eq!(sig.map(|b| b & 0x7F), *b"HCA\0", "{name}");
                assert_eq!(loc.waveform.sample_rate, 48000, "{name}");
                resolved += 1;
            }
            Err(Error::CueSilent(_)) => assert_eq!(name, "blank"),
            Err(e) => panic!("{name}: {e}"),
        }
    }
    assert_eq!(resolved, 416);
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn songs_use_the_speaker_mix() {
    let bank = bank();
    let t = bank.acb().speaker_track("MER_BGM_S00_003").unwrap();
    assert!(
        t.sends
            .iter()
            .any(|s| s.bus == "BGM_SPEAKER" && s.level == 1.0)
    );
    assert_eq!(bank.acb().tracks("MER_BGM_S00_003").unwrap().len(), 2);
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn embedded_awb_headers_match_files() {
    let bank = bank();
    for awb in bank.acb().stream_awbs() {
        let path = bank.dir().join(format!("{}.awb", awb.name));
        let on_disk = afs2::Index::read(&mut File::open(&path).unwrap()).unwrap();
        assert_eq!(awb.index.as_ref(), Some(&on_disk), "{}", awb.name);
    }
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn reports_the_six_removed_chart_cues() {
    let bank = bank();
    let missing: Vec<_> = chart_cues()
        .into_iter()
        .filter(|c| matches!(bank.locate(c), Err(Error::CueNotFound(_))))
        .collect();
    assert_eq!(
        missing,
        [
            "MER_BGM_S01_007",
            "MER_BGM_S01_025",
            "MER_BGM_S01_028",
            "MER_BGM_S01_053",
            "MER_BGM_S01_096",
            "MER_BGM_S01_277"
        ]
    );
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn decodes_two_seconds_of_s00_003() {
    let mut s = bank().open_cue("MER_BGM_S00_003").unwrap();
    assert_eq!((s.sample_rate(), s.channels()), (48000, 2));
    assert_eq!(s.total_frames(), 5592383);
    let mut pcm = Vec::new();
    while pcm.len() < 2 * 48000 * 2 {
        assert!(s.decode(&mut pcm).unwrap());
    }
    pcm.truncate(2 * 48000 * 2);
    assert!(pcm.iter().all(|v| v.is_finite() && v.abs() <= 2.0));
    // The song fades in: 0–1.5 s peak ≈ 6e-4, 1.5–2 s peak ≈ 0.027, full level from ~2.5 s.
    let peak = pcm.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.01, "peak {peak}");
    assert!(
        pcm[..48000].iter().any(|&v| v != 0.0),
        "intro is digital silence"
    );
}
