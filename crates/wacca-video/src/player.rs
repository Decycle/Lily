//! [`VideoPlayer`]: demuxes a `.usm` into an `ffmpeg` child process on background threads and presents the
//! decoded frame due at a given movie time.
//!
//! Threads per decoder: a writer (USM pictures → ffmpeg stdin), a reader (stdout → bounded frame queue) and a
//! stderr logger. The bounded queue gives back-pressure: when the game stops pulling (pause), ffmpeg blocks.
//! Dropping the player kills the child and joins the threads.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::thread::JoinHandle;

use crate::Error;
use crate::ffmpeg::decode_args;
use crate::schedule::{Step, frame_index, seek_secs, step};
use crate::usm::{ChunkReader, SequenceHeader, sequence_header};

/// Decoded frames buffered between ffmpeg and the game.
const QUEUE_FRAMES: usize = 4;

/// One decoded picture, `yuv420p` (Y plane, then U and V at half resolution).
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub index: u64,
    pub width: u32,
    pub height: u32,
    pub yuv: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame #{} {}x{}", self.index, self.width, self.height)
    }
}

/// Byte size of a `yuv420p` frame (`width`, `height` even).
pub fn yuv420_len(width: u32, height: u32) -> usize {
    let y = width as usize * height as usize;
    y + y / 2
}

impl Frame {
    /// Y, U, V planes; `None` if the buffer does not match the size.
    pub fn planes(&self) -> Option<[&[u8]; 3]> {
        let y = self.width as usize * self.height as usize;
        if self.yuv.len() != yuv420_len(self.width, self.height) {
            return None;
        }
        let (luma, chroma) = self.yuv.split_at(y);
        let (u, v) = chroma.split_at(y / 4);
        Some([luma, u, v])
    }
}

#[derive(Debug)]
pub struct VideoPlayer {
    path: PathBuf,
    ffmpeg: PathBuf,
    info: SequenceHeader,
    /// Output frame size (scaled or native).
    out: [u32; 2],
    scale: Option<u32>,
    decoder: Option<Decoder>,
    shown: Option<Frame>,
    /// Decoded but not yet due.
    pending: Option<Frame>,
    /// Starting ffmpeg failed; stay blank instead of retrying every frame.
    failed: bool,
}

impl VideoPlayer {
    /// Reads the stream header of `path`; decoding starts at the first [`frame_at`](Self::frame_at).
    /// `size`: scale frames down to `size` × `size` (rounded to even, never up).
    pub fn open(path: &Path, ffmpeg: &Path, size: Option<u32>) -> Result<Self, Error> {
        let file = File::open(path).map_err(|e| Error::File {
            path: path.to_path_buf(),
            source: e,
        })?;
        let first = ChunkReader::new(BufReader::new(file))
            .next_picture()?
            .ok_or_else(|| Error::NoVideo(path.to_path_buf()))?;
        let info = sequence_header(&first.payload)
            .ok_or_else(|| Error::NoSequenceHeader(path.to_path_buf()))?;
        let scale = size
            .map(|s| (s & !1).max(2))
            .filter(|&s| s < info.width || s < info.height);
        let out = scale.map_or([info.width & !1, info.height & !1], |s| [s, s]);
        log::info!(
            "movie {}: {}x{} @ {:.2} fps, output {}x{}",
            path.display(),
            info.width,
            info.height,
            info.fps,
            out[0],
            out[1]
        );
        Ok(Self {
            path: path.to_path_buf(),
            ffmpeg: ffmpeg.to_path_buf(),
            info,
            out,
            scale,
            decoder: None,
            shown: None,
            pending: None,
            failed: false,
        })
    }

    pub fn info(&self) -> SequenceHeader {
        self.info
    }

    /// The frame to show at movie time `t` seconds (`None` before the movie starts or if decoding failed).
    pub fn frame_at(&mut self, t: f64) -> Option<&Frame> {
        let target = frame_index(t, self.info.fps)?;
        if self.failed {
            return None;
        }
        let next = self
            .pending
            .as_ref()
            .map(|f| f.index)
            .or(self.decoder.as_ref().map(|d| d.next));
        let ended = self.decoder.as_ref().is_some_and(|d| d.ended);
        if step(next, ended, target) == Step::Restart {
            self.restart(target);
        }
        let dec = self.decoder.as_mut()?;
        loop {
            let f = match self.pending.take() {
                Some(f) => f,
                None => match dec.rx.try_recv() {
                    Ok(f) => {
                        dec.next = f.index + 1;
                        f
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if !dec.ended {
                            log::info!("movie ended after frame {}", dec.next);
                        }
                        dec.ended = true;
                        break;
                    }
                },
            };
            if f.index <= target {
                self.shown = Some(f);
            } else {
                self.pending = Some(f);
                break;
            }
        }
        self.shown.as_ref()
    }

    fn restart(&mut self, k: u64) {
        self.decoder = None;
        self.pending = None;
        let seek = seek_secs(k, self.info.fps);
        log::debug!("movie: decoding from frame {k} ({seek:.3} s)");
        match Decoder::spawn(self, k, &decode_args(seek, self.scale)) {
            Ok(d) => self.decoder = Some(d),
            Err(e) => {
                log::warn!("movie {}: {e}; playing without it", self.path.display());
                self.failed = true;
            }
        }
    }
}

/// One running ffmpeg process.
#[derive(Debug)]
struct Decoder {
    child: Child,
    rx: Receiver<Frame>,
    /// Index of the next frame expected from the queue.
    next: u64,
    ended: bool,
    threads: Vec<JoinHandle<()>>,
}

impl Decoder {
    fn spawn(p: &VideoPlayer, first: u64, args: &[String]) -> Result<Self, Error> {
        let file = File::open(&p.path).map_err(|e| Error::File {
            path: p.path.clone(),
            source: e,
        })?;
        let mut cmd = Command::new(&p.ffmpeg);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|source| Error::Spawn {
            path: p.ffmpeg.clone(),
            source,
        })?;
        let (tx, rx) = sync_channel(QUEUE_FRAMES);
        let mut threads = Vec::new();
        if let Some(stdin) = child.stdin.take() {
            threads.push(std::thread::spawn(move || feed(file, stdin)));
        }
        if let Some(stdout) = child.stdout.take() {
            let [w, h] = p.out;
            threads.push(std::thread::spawn(move || {
                read_frames(stdout, tx, first, w, h)
            }));
        }
        if let Some(stderr) = child.stderr.take() {
            threads.push(std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::warn!("ffmpeg: {line}");
                }
            }));
        }
        Ok(Self {
            child,
            rx,
            next: first,
            ended: false,
            threads,
        })
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        if let Err(e) = self.child.kill() {
            log::debug!("killing ffmpeg: {e}");
        }
        if let Err(e) = self.child.wait() {
            log::warn!("waiting for ffmpeg: {e}");
        }
        // Unblock a reader waiting on a full queue.
        drop(std::mem::replace(&mut self.rx, sync_channel(0).1));
        for t in self.threads.drain(..) {
            if t.join().is_err() {
                log::warn!("movie thread panicked");
            }
        }
    }
}

/// Writer thread: every USM picture into ffmpeg; closing stdin at the end lets it flush the last frames.
fn feed(file: File, mut stdin: impl Write) {
    let mut usm = ChunkReader::new(BufReader::with_capacity(1 << 20, file));
    loop {
        match usm.next_picture() {
            Ok(Some(c)) => {
                if stdin.write_all(&c.payload).is_err() {
                    return; // ffmpeg exited or was killed
                }
            }
            Ok(None) => return,
            Err(e) => {
                log::warn!("movie demux: {e}");
                return;
            }
        }
    }
}

/// Reader thread: fixed-size raw frames from ffmpeg into the queue, numbered from `first`.
fn read_frames(mut stdout: impl Read, tx: SyncSender<Frame>, first: u64, width: u32, height: u32) {
    for index in first.. {
        let mut yuv = vec![0; yuv420_len(width, height)];
        if stdout.read_exact(&mut yuv).is_err() {
            return;
        }
        let frame = Frame {
            index,
            width,
            height,
            yuv,
        };
        if tx.send(frame).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_planes() {
        let f = Frame {
            index: 0,
            width: 4,
            height: 2,
            yuv: (0..12).collect(),
        };
        let [y, u, v] = f.planes().unwrap();
        assert_eq!((y.len(), u, v), (8, &[8, 9][..], &[10, 11][..]));
        let bad = Frame {
            yuv: vec![0; 11],
            ..f
        };
        assert!(bad.planes().is_none());
    }

    #[test]
    fn reader_numbers_frames_and_stops_at_eof() {
        let (tx, rx) = sync_channel(8);
        let data = vec![7u8; yuv420_len(2, 2) * 2 + 3];
        read_frames(data.as_slice(), tx, 5, 2, 2);
        let got: Vec<u64> = rx.iter().map(|f| f.index).collect();
        assert_eq!(got, [5, 6]);
    }
}
