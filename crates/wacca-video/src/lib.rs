//! Background movies (06-visuals §1): CRI `.usm` (Sofdec.Prime = MPEG-1 video) demuxed in Rust and decoded by
//! an external `ffmpeg` process into `yuv420p` frames.
//!
//! [`usm`] and [`schedule`] are pure and unit-tested; [`VideoPlayer`] owns the process and threads.

pub mod ffmpeg;
mod player;
pub mod schedule;
pub mod usm;

use std::path::PathBuf;

pub use ffmpeg::{find_ffmpeg, find_ffmpeg_from_env};
pub use player::{Frame, VideoPlayer, yuv420_len};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("USM: {0}")]
    Usm(&'static str),
    #[error("reading USM: {0}")]
    Io(#[from] std::io::Error),
    #[error("{}: {source}", path.display())]
    File {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}: no video stream", .0.display())]
    NoVideo(PathBuf),
    #[error("{}: no MPEG-1 sequence header at the start of the video", .0.display())]
    NoSequenceHeader(PathBuf),
    #[error("starting {}: {source}", path.display())]
    Spawn {
        path: PathBuf,
        source: std::io::Error,
    },
}
