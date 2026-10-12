//! `cpal` output: a background thread decodes into a bounded queue of chunks, the device
//! callback drains it and reports what it consumed to the [`Clock`].
//!
//! Seeks bump a generation number; chunks of older generations are dropped by the callback, so a
//! seek takes effect within one device buffer. The device is opened at the decoder's sample rate
//! (48 kHz for WACCA); there is no resampler.
//!
//! Sound effects ([`Player::play_sfx`]) are mixed on top of the BGM in the same callback (see
//! `mixer`); they keep playing while the BGM is paused.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SampleRate, SizedSample, SupportedStreamConfigRange};

use crate::mixer::{self, Mixer};
use crate::{Clock, Decoder, Error, Sfx};

/// Decoded chunks kept ahead of the device (each is one HCA block, 1024 frames ≈ 21 ms).
const QUEUE_CHUNKS: usize = 16;
/// Frames per chunk of lead-in silence (negative seek positions).
const SILENCE_CHUNK: u64 = 1024;
/// Pending sound-effect commands between two device callbacks.
const SFX_QUEUE: usize = 64;
/// Mix buffer preallocated for the callback (samples); larger device buffers grow it once.
const MIX_BUFFER: usize = 16 * 1024;

#[derive(Debug)]
struct Chunk {
    generation: u64,
    samples: Vec<f32>,
}

#[derive(Debug)]
enum Command {
    /// Continue at `frame` (negative = that many frames of silence first).
    Seek { generation: u64, frame: i64 },
}

#[derive(Debug)]
struct Shared {
    clock: Mutex<Clock>,
    generation: AtomicU64,
    playing: AtomicBool,
    volume: AtomicU32,
}

impl Shared {
    fn clock(&self) -> MutexGuard<'_, Clock> {
        self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A playing sound effect, for [`Player::stop_sfx`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoiceId(u64);

/// Streams a [`Decoder`] to the default output device and mixes sound effects on top.
pub struct Player {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    sfx: SyncSender<mixer::Command>,
    next_voice: AtomicU64,
    sample_rate: u32,
    duration: f64,
    _stream: cpal::Stream,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("shared", &self.shared)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

fn device_err(e: impl std::fmt::Display) -> Error {
    Error::Device(e.to_string())
}

impl Player {
    /// Open the default output device at the decoder's rate. Starts paused at position 0.
    pub fn new(decoder: Box<dyn Decoder>) -> Result<Self, Error> {
        let rate = decoder.sample_rate();
        let channels = decoder.channels();
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| Error::Device("no default output device".into()))?;
        let ranges: Vec<_> = device
            .supported_output_configs()
            .map_err(device_err)?
            .collect();
        let range = pick_config(&ranges, rate, channels).ok_or_else(|| {
            Error::Device(format!(
                "no output config at {rate} Hz with a supported sample format (no resampler)"
            ))
        })?;
        let config = range.with_sample_rate(SampleRate(rate));
        log::info!("audio output: {config:?}");

        let shared = Arc::new(Shared {
            clock: Mutex::new(Clock::new(rate)),
            generation: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
        });
        let (chunk_tx, chunk_rx) = mpsc::sync_channel(QUEUE_CHUNKS);
        let (commands, command_rx) = mpsc::channel();
        let duration = decoder.total_frames() as f64 / f64::from(rate);
        let (sfx, sfx_rx) = mpsc::sync_channel(SFX_QUEUE);
        let output = Output {
            feed: Feed::new(chunk_rx, channels.into()),
            mixer: Mixer::new(sfx_rx),
            mix: Vec::with_capacity(MIX_BUFFER),
            channels: usize::from(config.channels()),
        };
        let stream_config = config.config();
        let stream = match config.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, stream_config, output, &shared),
            SampleFormat::I16 => build::<i16>(&device, stream_config, output, &shared),
            SampleFormat::I32 => build::<i32>(&device, stream_config, output, &shared),
            other => Err(Error::Device(format!("unsupported sample format {other}"))),
        }?;
        stream.play().map_err(device_err)?;
        std::thread::Builder::new()
            .name("wacca-audio-decode".into())
            .spawn(move || decode_loop(decoder, command_rx, chunk_tx))?;

        let player = Player {
            shared,
            commands,
            sfx,
            next_voice: AtomicU64::new(0),
            sample_rate: rate,
            duration,
            _stream: stream,
        };
        player.seek(0.0)?;
        Ok(player)
    }

    pub fn play(&self) {
        self.shared.playing.store(true, Ordering::Release);
    }

    /// Pause output; the position is kept.
    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::Release);
    }

    /// Pause and rewind to the start.
    pub fn stop(&self) -> Result<(), Error> {
        self.pause();
        self.seek(0.0)
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }

    /// Jump to `seconds` of song time; negative values play that much silence first.
    pub fn seek(&self, seconds: f64) -> Result<(), Error> {
        let generation = self.shared.clock().reset(seconds);
        self.shared.generation.store(generation, Ordering::Release);
        let frame = (seconds * f64::from(self.sample_rate)).round() as i64;
        self.commands
            .send(Command::Seek { generation, frame })
            .map_err(|_| Error::Device("decoder thread stopped".into()))
    }

    /// Linear gain applied to the BGM (clamped to 0..=1); sound effects use their own gain.
    pub fn set_volume(&self, gain: f32) {
        let gain = if gain.is_nan() {
            0.0
        } else {
            gain.clamp(0.0, 1.0)
        };
        self.shared.volume.store(gain.to_bits(), Ordering::Release);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Acquire))
    }

    /// Song time currently audible, in seconds (see [`Clock::position`]).
    pub fn position(&self) -> f64 {
        self.shared.clock().position(Instant::now())
    }

    /// Length of the stream in seconds.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Output rate; sound effects must match it (see [`Sfx::resampled`]).
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Start `sfx` at linear `gain`; heard within one device buffer. At most
    /// [`crate::MAX_VOICES`] play at once (the oldest is cut). A sound with a loop region
    /// sustains until [`Player::stop_sfx`].
    pub fn play_sfx(&self, sfx: &Sfx, gain: f32) -> Result<VoiceId, Error> {
        if sfx.rate != self.sample_rate {
            return Err(Error::SampleRate {
                expected: self.sample_rate,
                got: sfx.rate,
            });
        }
        let id = self.next_voice.fetch_add(1, Ordering::Relaxed);
        let gain = if gain.is_finite() { gain.max(0.0) } else { 0.0 };
        self.sfx
            .try_send(mixer::Command::Play {
                id,
                sfx: sfx.clone(),
                gain,
            })
            .map_err(|_| Error::SfxQueueFull)?;
        Ok(VoiceId(id))
    }

    /// Fade out a sound effect (ends a hold sustain). Unknown or finished voices are ignored.
    pub fn stop_sfx(&self, voice: VoiceId) -> Result<(), Error> {
        self.sfx
            .try_send(mixer::Command::Stop { id: voice.0 })
            .map_err(|_| Error::SfxQueueFull)
    }
}

/// Best device config for `rate`: exact channel count first, then more channels, then fewer;
/// `f32` preferred over integer formats.
fn pick_config(
    ranges: &[SupportedStreamConfigRange],
    rate: u32,
    channels: u16,
) -> Option<SupportedStreamConfigRange> {
    let score = |r: &SupportedStreamConfigRange| {
        let format = match r.sample_format() {
            SampleFormat::F32 => 0,
            SampleFormat::I32 => 1,
            SampleFormat::I16 => 2,
            _ => return None,
        };
        if !(r.min_sample_rate()..=r.max_sample_rate()).contains(&SampleRate(rate))
            || r.channels() == 0
        {
            return None;
        }
        let fit = match r.channels().cmp(&channels) {
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => 2,
        };
        Some((fit, format))
    };
    ranges
        .iter()
        .filter_map(|r| Some((score(r)?, *r)))
        .min_by_key(|(s, _)| *s)
        .map(|(_, r)| r)
}

/// Everything the device callback owns.
#[derive(Debug)]
struct Output {
    feed: Feed,
    mixer: Mixer,
    /// Interleaved `f32` mix of BGM and sound effects, converted to the device format at the end.
    mix: Vec<f32>,
    channels: usize,
}

impl Output {
    /// Render one device buffer; returns the number of BGM frames consumed.
    fn render<T: Sample + FromSample<f32>>(
        &mut self,
        out: &mut [T],
        generation: u64,
        playing: bool,
        gain: f32,
    ) -> u64 {
        if self.mix.capacity() < out.len() {
            log::warn!("device buffer of {} samples; growing mix buffer", out.len());
        }
        self.mix.clear();
        self.mix.resize(out.len(), 0.0);
        let written = self
            .feed
            .fill(&mut self.mix, self.channels, generation, playing, gain);
        self.mixer.mix(&mut self.mix, self.channels.max(1));
        for (d, s) in out.iter_mut().zip(&self.mix) {
            *d = T::from_sample(s.clamp(-1.0, 1.0));
        }
        written
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut output: Output,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream, Error>
where
    T: SizedSample + FromSample<f32>,
{
    let shared = Arc::clone(shared);
    device
        .build_output_stream::<T, _, _>(
            &config,
            move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
                let now = Instant::now();
                let ts = info.timestamp();
                let latency = ts.playback.duration_since(&ts.callback).unwrap_or_default();
                let generation = shared.generation.load(Ordering::Acquire);
                let playing = shared.playing.load(Ordering::Acquire);
                let gain = f32::from_bits(shared.volume.load(Ordering::Acquire));
                let written = output.render(out, generation, playing, gain);
                shared
                    .clock()
                    .on_callback(generation, written, latency, now);
            },
            |e| log::error!("audio stream error: {e}"),
            None,
        )
        .map_err(device_err)
}

/// Callback-side reader of the chunk queue.
#[derive(Debug)]
struct Feed {
    rx: Receiver<Chunk>,
    chunk: Chunk,
    read: usize,
    channels: usize,
}

impl Feed {
    fn new(rx: Receiver<Chunk>, channels: usize) -> Self {
        Feed {
            rx,
            chunk: Chunk {
                generation: 0,
                samples: Vec::new(),
            },
            read: 0,
            channels: channels.max(1),
        }
    }

    /// Make sure the current chunk has unread frames of `generation`, dropping stale chunks.
    fn ready(&mut self, generation: u64) -> bool {
        loop {
            if self.chunk.generation == generation && self.read < self.chunk.samples.len() {
                return true;
            }
            match self.rx.try_recv() {
                Ok(c) => {
                    self.chunk = c;
                    self.read = 0;
                }
                Err(_) => return false,
            }
        }
    }

    /// Fill `out` (interleaved, `out_channels` wide); returns the number of source frames used.
    /// Missing data (paused, underrun, end) is written as silence.
    fn fill(
        &mut self,
        out: &mut [f32],
        out_channels: usize,
        generation: u64,
        playing: bool,
        gain: f32,
    ) -> u64 {
        let mut written = 0;
        for frame in out.chunks_mut(out_channels.max(1)) {
            if playing && self.ready(generation) {
                let end = (self.read + self.channels).min(self.chunk.samples.len());
                map_frame(frame, &self.chunk.samples[self.read..end], gain);
                self.read = end;
                written += 1;
            } else {
                frame.fill(0.0);
            }
        }
        if !playing {
            // Keep the queue moving after a seek so the decoder never blocks on stale chunks.
            self.ready(generation);
        }
        written
    }
}

/// Write one source frame to a device frame: mono goes to every channel, otherwise channel `c`
/// takes source channel `c` and extra device channels are silent.
fn map_frame(dst: &mut [f32], src: &[f32], gain: f32) {
    for (c, d) in dst.iter_mut().enumerate() {
        let v = match src {
            [mono] => *mono,
            _ => src.get(c).copied().unwrap_or(0.0),
        };
        *d = v * gain;
    }
}

/// Decoder thread: waits for a seek, then decodes ahead until the end or the next seek.
fn decode_loop(mut dec: Box<dyn Decoder>, commands: Receiver<Command>, out: SyncSender<Chunk>) {
    let ch = usize::from(dec.channels());
    let mut generation = 0;
    let mut silence = 0u64;
    let mut active = false;
    loop {
        let cmd = if active {
            match commands.try_recv() {
                Ok(c) => Some(c),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            match commands.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        if let Some(Command::Seek {
            generation: g,
            frame,
        }) = cmd
        {
            generation = g;
            silence = frame.min(0).unsigned_abs();
            active = match dec.seek(frame.max(0).unsigned_abs()) {
                Ok(()) => true,
                Err(e) => {
                    log::error!("seek failed: {e}");
                    false
                }
            };
            continue;
        }
        let mut samples = Vec::new();
        if silence > 0 {
            let n = silence.min(SILENCE_CHUNK);
            silence -= n;
            samples.resize(n as usize * ch, 0.0);
        } else {
            match dec.decode(&mut samples) {
                Ok(true) => {}
                Ok(false) => {
                    active = false;
                    continue;
                }
                Err(e) => {
                    log::error!("decode failed: {e}");
                    active = false;
                    continue;
                }
            }
        }
        if out
            .send(Chunk {
                generation,
                samples,
            })
            .is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::SupportedBufferSize;

    /// Decoder producing frame index `i` as the value of every channel, in blocks of 4 frames.
    #[derive(Debug)]
    struct Ramp {
        channels: u16,
        total: u64,
        pos: u64,
    }

    impl Decoder for Ramp {
        fn channels(&self) -> u16 {
            self.channels
        }
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn total_frames(&self) -> u64 {
            self.total
        }
        fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, Error> {
            if self.pos >= self.total {
                return Ok(false);
            }
            let end = (self.pos + 4).min(self.total);
            for i in self.pos..end {
                out.extend(std::iter::repeat_n(i as f32, self.channels.into()));
            }
            self.pos = end;
            Ok(true)
        }
        fn seek(&mut self, frame: u64) -> Result<(), Error> {
            self.pos = frame.min(self.total);
            Ok(())
        }
    }

    fn chunk(generation: u64, samples: &[f32]) -> Chunk {
        Chunk {
            generation,
            samples: samples.to_vec(),
        }
    }

    #[test]
    fn feed_maps_stereo_applies_gain_and_pads_underrun() {
        let (tx, rx) = mpsc::sync_channel(4);
        let mut feed = Feed::new(rx, 2);
        tx.send(chunk(1, &[1.0, 2.0, 3.0, 4.0])).unwrap();
        let mut out = [9.0f32; 9]; // 3 frames of 3 channels
        assert_eq!(feed.fill(&mut out, 3, 1, true, 0.5), 2);
        assert_eq!(out, [0.5, 1.0, 0.0, 1.5, 2.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn render_mixes_sfx_over_mono_bgm_and_clamps_into_the_device_format() {
        let (tx, rx) = mpsc::sync_channel(4);
        let (sfx_tx, sfx_rx) = mpsc::sync_channel(4);
        let mut output = Output {
            feed: Feed::new(rx, 1),
            mixer: Mixer::new(sfx_rx),
            mix: Vec::new(),
            channels: 2,
        };
        tx.send(chunk(1, &[0.75, 0.25])).unwrap();
        let sfx = Sfx {
            rate: 1000,
            channels: 2,
            samples: [0.5, -0.5].into(),
            loop_range: None,
        };
        sfx_tx
            .send(mixer::Command::Play {
                id: 0,
                sfx,
                gain: 1.0,
            })
            .unwrap();
        let mut out = [0i16; 6];
        // BGM gain 1: frame 0 = 0.75 + (0.5, -0.5) → clamped 1.0, 0.25.
        assert_eq!(output.render(&mut out, 1, true, 1.0), 2);
        assert_eq!(out[..2], [i16::MAX, 8192]);
        assert_eq!(out[2..], [8192, 8192, 0, 0]);
    }

    #[test]
    fn feed_drops_stale_generations_and_is_silent_when_paused() {
        let (tx, rx) = mpsc::sync_channel(4);
        let mut feed = Feed::new(rx, 1);
        tx.send(chunk(1, &[1.0, 1.0])).unwrap();
        tx.send(chunk(2, &[2.0])).unwrap();
        let mut out = [5.0f32; 2];
        // Paused: silence, nothing consumed, but stale chunks are drained.
        assert_eq!(feed.fill(&mut out, 1, 2, false, 1.0), 0);
        assert_eq!(out, [0.0, 0.0]);
        assert_eq!(feed.fill(&mut out, 1, 2, true, 1.0), 1);
        assert_eq!(out, [2.0, 0.0]);
    }

    #[test]
    fn decode_loop_seeks_with_lead_in_silence_and_stops_at_end() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (tx, rx) = mpsc::sync_channel(64);
        let dec = Box::new(Ramp {
            channels: 1,
            total: 10,
            pos: 0,
        });
        cmd_tx
            .send(Command::Seek {
                generation: 3,
                frame: -2,
            })
            .unwrap();
        let t = std::thread::spawn(move || decode_loop(dec, cmd_rx, tx));
        let mut got = Vec::new();
        while got.len() < 12 {
            let c = rx.recv().unwrap();
            assert_eq!(c.generation, 3);
            got.extend(c.samples);
        }
        let want: Vec<f32> = [0.0, 0.0]
            .into_iter()
            .chain((0..10).map(|i| i as f32))
            .collect();
        assert_eq!(got, want);
        drop(cmd_tx);
        t.join().unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn picks_exact_channels_and_float_at_the_requested_rate() {
        let r = |ch, min, max, fmt| {
            SupportedStreamConfigRange::new(
                ch,
                SampleRate(min),
                SampleRate(max),
                SupportedBufferSize::Unknown,
                fmt,
            )
        };
        let ranges = [
            r(2, 44100, 44100, SampleFormat::F32),
            r(6, 48000, 48000, SampleFormat::F32),
            r(2, 48000, 48000, SampleFormat::I16),
            r(2, 8000, 192000, SampleFormat::F32),
            r(2, 48000, 48000, SampleFormat::U8),
        ];
        let best = pick_config(&ranges, 48000, 2).unwrap();
        assert_eq!(
            (best.channels(), best.sample_format()),
            (2, SampleFormat::F32)
        );
        assert_eq!(best.max_sample_rate(), SampleRate(192000));
        let best = pick_config(&ranges[..2], 48000, 2).unwrap();
        assert_eq!(best.channels(), 6);
        assert!(pick_config(&ranges[..1], 48000, 2).is_none());
    }
}
