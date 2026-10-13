//! Real-movie checks. Need `WACCA_ASSETS`; run with `cargo test -p wacca-video -- --ignored`.
#![allow(clippy::unwrap_used)]

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use wacca_video::usm::{ChunkReader, sequence_header};
use wacca_video::{VideoPlayer, find_ffmpeg_from_env};

fn movies() -> Vec<PathBuf> {
    let assets = PathBuf::from(std::env::var_os("WACCA_ASSETS").expect("WACCA_ASSETS"));
    let mut out = Vec::new();
    for dir in [
        assets.join("Movie"),
        assets.join("Movie").join("SystemMovie"),
    ] {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "usm") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn demuxes_every_movie() {
    let movies = movies();
    assert_eq!(movies.len(), 91);
    for p in movies {
        let mut r = ChunkReader::new(BufReader::with_capacity(1 << 20, File::open(&p).unwrap()));
        let first = r.next_picture().unwrap().unwrap();
        let h = sequence_header(&first.payload).unwrap();
        assert_eq!(
            (h.width, h.height, h.fps),
            (1080, 1080, 60.0),
            "{}",
            p.display()
        );
        let mut frames = 1u64;
        while let Some(c) = r.next_picture().unwrap() {
            // One picture (start code 00 00 01 00) per data chunk.
            assert!(
                c.payload.windows(4).any(|w| w == [0, 0, 1, 0]),
                "{}",
                p.display()
            );
            frames += 1;
        }
        println!(
            "{}: {frames} frames ({:.1} s)",
            p.display(),
            frames as f64 / h.fps
        );
        assert!(frames > 600, "{}", p.display());
    }
}

#[test]
#[ignore = "needs WACCA_ASSETS and ffmpeg"]
fn decodes_frames_through_ffmpeg() {
    let ffmpeg = find_ffmpeg_from_env().expect("ffmpeg");
    let path = movies().into_iter().next().unwrap();
    let mut v = VideoPlayer::open(&path, &ffmpeg, Some(540)).unwrap();
    assert!(v.frame_at(-1.0).is_none());
    let start = std::time::Instant::now();
    let mut got = None;
    while start.elapsed().as_secs() < 10 && got.is_none() {
        got = v
            .frame_at(2.0)
            .map(|f| (f.index, f.width, f.planes().is_some()));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (index, width, ok) = got.unwrap();
    assert!(index <= 120 && width == 540 && ok);
    // Going back restarts the decoder.
    let mut back = None;
    while start.elapsed().as_secs() < 20 && back.is_none_or(|i| i > 30) {
        back = v.frame_at(0.5).map(|f| f.index);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(back, Some(30));
}
