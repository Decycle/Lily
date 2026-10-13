//! WASAPI loopback capture of the default output device (cpal builds an input stream on an output device as
//! loopback), written as f32 WAV in the device's mix format (48 kHz stereo on this PC). WASAPI delivers no
//! packets while nothing plays, so "first buffer" is the first audible-or-silent packet the engine produced.

use super::wav::WavWriter;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no default output device")]
    NoDevice,
    #[error("default output config: {0}")]
    Config(#[from] cpal::DefaultStreamConfigError),
    #[error("loopback needs an f32 mix format, device uses {0:?}")]
    Format(cpal::SampleFormat),
    #[error("building loopback stream: {0}")]
    Build(#[from] cpal::BuildStreamError),
    #[error("starting loopback stream: {0}")]
    Play(#[from] cpal::PlayStreamError),
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("audio writer thread panicked")]
    Writer,
}

/// Running level statistics of the captured samples.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Level {
    pub peak: f32,
    sum_sq: f64,
    count: u64,
}

impl Level {
    pub fn add(&mut self, samples: &[f32]) {
        for &s in samples {
            self.peak = self.peak.max(s.abs());
            self.sum_sq += f64::from(s) * f64::from(s);
        }
        self.count += samples.len() as u64;
    }

    pub fn rms(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            (self.sum_sq / self.count as f64).sqrt() as f32
        }
    }
}

#[derive(Debug)]
pub struct AudioResult {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub first_buffer: Option<Instant>,
    pub level: Level,
    /// Per packet: index of its first frame in the file and that frame's capture time (WASAPI's QPC stamp).
    pub packets: Vec<(u64, Instant)>,
}

/// One loopback packet as forwarded to a live consumer ([`AudioRecorder::start`]'s `tap`).
#[derive(Debug)]
pub struct Captured {
    /// Capture time of the first frame.
    pub at: Instant,
    /// Index of the first frame in the recording.
    pub frame: u64,
    /// Interleaved samples.
    pub samples: Vec<f32>,
}

type WriterResult = Result<(u64, Level, Vec<(u64, Instant)>), std::io::Error>;

pub struct AudioRecorder {
    stream: cpal::Stream,
    writer: JoinHandle<WriterResult>,
    first: Arc<OnceLock<Instant>>,
    path: PathBuf,
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
}

impl std::fmt::Debug for AudioRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioRecorder")
            .field("path", &self.path)
            .field("device", &self.device)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl AudioRecorder {
    /// Starts capturing into `path`; every packet is also sent to `tap` (if any) after it was written.
    pub fn start(path: &Path, tap: Option<mpsc::Sender<Captured>>) -> Result<Self, AudioError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(AudioError::NoDevice)?;
        let name = device.name().unwrap_or_default();
        let supported = device.default_output_config()?;
        if supported.sample_format() != cpal::SampleFormat::F32 {
            return Err(AudioError::Format(supported.sample_format()));
        }
        let config: cpal::StreamConfig = supported.into();
        let io_err = |source| AudioError::Io {
            path: path.to_path_buf(),
            source,
        };
        let file = File::create(path).map_err(io_err)?;
        let mut wav = WavWriter::new(BufWriter::new(file), config.sample_rate.0, config.channels)
            .map_err(io_err)?;
        let (tx, rx) = mpsc::channel::<(Instant, Vec<f32>)>();
        let channels = u64::from(config.channels.max(1));
        let writer = std::thread::spawn(move || {
            let mut level = Level::default();
            let mut packets = Vec::new();
            let mut frame = 0;
            for (at, buf) in rx {
                level.add(&buf);
                wav.write(&buf)?;
                packets.push((frame, at));
                let n = buf.len() as u64 / channels;
                if let Some(tap) = &tap {
                    // A consumer that went away just stops receiving.
                    let _ = tap.send(Captured {
                        at,
                        frame,
                        samples: buf,
                    });
                }
                frame += n;
            }
            let (frames, _) = wav.finish()?;
            Ok((frames, level, packets))
        });
        let first = Arc::new(OnceLock::new());
        let first_cb = Arc::clone(&first);
        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                let now = Instant::now();
                first_cb.get_or_init(|| now);
                // Back-date to the packet's capture stamp (WASAPI QPC position) when cpal provides it.
                let ts = info.timestamp();
                let at = ts
                    .callback
                    .duration_since(&ts.capture)
                    .and_then(|lag| now.checked_sub(lag))
                    .unwrap_or(now);
                // The receiver only goes away after the stream is dropped.
                let _ = tx.send((at, data.to_vec()));
            },
            |e| log::warn!("loopback stream: {e}"),
            None,
        )?;
        stream.play()?;
        log::info!(
            "loopback capture of {name:?}: {} Hz, {} ch -> {}",
            config.sample_rate.0,
            config.channels,
            path.display()
        );
        Ok(Self {
            stream,
            writer,
            first,
            path: path.to_path_buf(),
            device: name,
            sample_rate: config.sample_rate.0,
            channels: config.channels,
        })
    }

    pub fn stop(self) -> Result<AudioResult, AudioError> {
        drop(self.stream);
        let (frames, level, packets) = self
            .writer
            .join()
            .map_err(|_| AudioError::Writer)?
            .map_err(|source| AudioError::Io {
                path: self.path.clone(),
                source,
            })?;
        Ok(AudioResult {
            sample_rate: self.sample_rate,
            channels: self.channels,
            frames,
            first_buffer: self.first.get().copied(),
            level,
            packets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_stats() {
        let mut l = Level::default();
        assert_eq!(l.rms(), 0.0);
        l.add(&[0.5, -0.5]);
        l.add(&[-1.0, 1.0]);
        assert_eq!(l.peak, 1.0);
        assert!((l.rms() - 0.625f32.sqrt()).abs() < 1e-6);
    }
}
