//! `cpal` output, opened once for the app's lifetime; BGM sources are swapped through it.
//!
//! [`Player::load`] hands a [`Decoder`] to a background thread that decodes into a bounded queue
//! of chunks; the device callback drains it and reports what it consumed to the [`Clock`]. Loads
//! and seeks bump a generation number; chunks of older generations are dropped by the callback,
//! so a source swap or seek takes effect within one device buffer and never reopens the device.
//! The device runs at [`OUTPUT_RATE`]; there is no resampler, so sources must match it (all WACCA
//! BGM is 48 kHz).
//!
//! Sound effects ([`Player::play_sfx`]) are mixed on top of the BGM in the same callback (see
//! `mixer`); they keep playing while the BGM is paused or swapped.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SampleRate, SizedSample, SupportedStreamConfigRange};

use crate::mixer::{self, Mixer};
use crate::resample::Resampled;
use crate::{Clock, Decoder, Error, Sfx};

/// Output sample rate (the rate of every WACCA BGM and of most sound effects).
pub const OUTPUT_RATE: u32 = 48_000;
/// Preferred device channel count (WACCA BGM is stereo).
const OUTPUT_CHANNELS: u16 = 2;
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
    /// Interleaved channels of `samples` (sources may differ).
    channels: usize,
    samples: Vec<f32>,
}

#[derive(Debug)]
enum Command {
    /// Replace the source and continue at its start.
    Load {
        generation: u64,
        decoder: Box<dyn Decoder>,
    },
    /// Continue at `frame` (negative = that many frames of silence first).
    Seek { generation: u64, frame: i64 },
}

#[derive(Debug)]
struct Shared {
    clock: Mutex<Clock>,
    generation: AtomicU64,
    playing: AtomicBool,
    volume: AtomicU32,
    /// Length of the current source in seconds (`f64` bits).
    duration: AtomicU64,
    /// Frames of the last device callback and its reported output latency (µs).
    callback_frames: AtomicU64,
    latency_us: AtomicU64,
}

impl Shared {
    fn clock(&self) -> MutexGuard<'_, Clock> {
        self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A playing sound effect, for [`Player::stop_sfx`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoiceId(u64);

/// The default output device: one swappable BGM source plus sound effects on top.
pub struct Player {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    sfx: SyncSender<mixer::Command>,
    next_voice: AtomicU64,
    sample_rate: u32,
    /// `None` only in device-free tests.
    _stream: Option<cpal::Stream>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("shared", &self.shared)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

/// The callback and decoder-thread ends of a [`Player`].
#[derive(Debug)]
struct Ends {
    output: Output,
    commands: Receiver<Command>,
    chunks: SyncSender<Chunk>,
}

fn device_err(e: impl std::fmt::Display) -> Error {
    Error::Device(e.to_string())
}

impl Player {
    /// Open the default output device at [`OUTPUT_RATE`], or else at a rate it supports (BGM is
    /// then resampled, see [`Player::load`]). Starts silent, with no source.
    pub fn new() -> Result<Self, Error> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                Error::Device("no default output device (speakers, headphones)".into())
            })?;
        let ranges: Vec<_> = device
            .supported_output_configs()
            .map_err(device_err)?
            .collect();
        let (range, rate) = pick_rate(&ranges, OUTPUT_CHANNELS).ok_or_else(|| {
            Error::Device(format!(
                "the output device offers no usable format ({} configurations)",
                ranges.len()
            ))
        })?;
        let config = range.with_sample_rate(SampleRate(rate));
        log::info!("audio output: {config:?}");
        if rate != OUTPUT_RATE {
            log::warn!(
                "the output device runs at {rate} Hz: music is resampled from {OUTPUT_RATE} Hz"
            );
        }

        let (mut player, ends) = Player::detached(rate, config.channels());
        let stream_config = config.config();
        let (output, shared) = (ends.output, &player.shared);
        let stream = match config.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, stream_config, output, shared),
            SampleFormat::I16 => build::<i16>(&device, stream_config, output, shared),
            SampleFormat::I32 => build::<i32>(&device, stream_config, output, shared),
            other => Err(Error::Device(format!("unsupported sample format {other}"))),
        }?;
        stream.play().map_err(device_err)?;
        let (commands, chunks) = (ends.commands, ends.chunks);
        std::thread::Builder::new()
            .name("wacca-audio-decode".into())
            .spawn(move || decode_loop(commands, chunks))?;
        player._stream = Some(stream);
        Ok(player)
    }

    /// A player without a device or decoder thread; the caller drives both [`Ends`].
    fn detached(rate: u32, device_channels: u16) -> (Self, Ends) {
        let shared = Arc::new(Shared {
            clock: Mutex::new(Clock::new(rate)),
            generation: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
            duration: AtomicU64::new(0f64.to_bits()),
            callback_frames: AtomicU64::new(0),
            latency_us: AtomicU64::new(0),
        });
        let (chunks, chunk_rx) = mpsc::sync_channel(QUEUE_CHUNKS);
        let (commands, command_rx) = mpsc::channel();
        let (sfx, sfx_rx) = mpsc::sync_channel(SFX_QUEUE);
        let player = Player {
            shared,
            commands,
            sfx,
            next_voice: AtomicU64::new(0),
            sample_rate: rate,
            _stream: None,
        };
        let ends = Ends {
            output: Output {
                feed: Feed::new(chunk_rx),
                mixer: Mixer::new(sfx_rx),
                mix: Vec::with_capacity(MIX_BUFFER),
                channels: usize::from(device_channels),
                rate,
                max_frames: 0,
            },
            commands: command_rx,
            chunks,
        };
        (player, ends)
    }

    /// Replace the BGM source (resampled to the output rate if it differs). Afterwards the player
    /// is paused at position 0 with volume 1, like a fresh one; the old source stops within one
    /// device buffer. Sound effects keep playing.
    pub fn load(&self, decoder: Box<dyn Decoder>) -> Result<(), Error> {
        let decoder: Box<dyn Decoder> = if decoder.sample_rate() == self.sample_rate {
            decoder
        } else {
            Box::new(Resampled::new(decoder, self.sample_rate))
        };
        let rate = decoder.sample_rate();
        self.pause();
        self.set_volume(1.0);
        let duration = decoder.total_frames() as f64 / f64::from(rate);
        self.shared
            .duration
            .store(duration.to_bits(), Ordering::Release);
        let generation = self.reset(0.0);
        self.send(Command::Load {
            generation,
            decoder,
        })
    }

    pub fn play(&self) {
        self.shared.playing.store(true, Ordering::Release);
    }

    /// Pause output; the position is kept.
    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::Release);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }

    /// Jump to `seconds` of song time; negative values play that much silence first.
    pub fn seek(&self, seconds: f64) -> Result<(), Error> {
        let generation = self.reset(seconds);
        let frame = (seconds * f64::from(self.sample_rate)).round() as i64;
        self.send(Command::Seek { generation, frame })
    }

    /// Restart the clock at `seconds`; returns the new generation.
    fn reset(&self, seconds: f64) -> u64 {
        let generation = self.shared.clock().reset(seconds);
        self.shared.generation.store(generation, Ordering::Release);
        generation
    }

    fn send(&self, cmd: Command) -> Result<(), Error> {
        self.commands
            .send(cmd)
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

    /// Length of the loaded source in seconds (0 before the first [`Player::load`]).
    pub fn duration(&self) -> f64 {
        f64::from_bits(self.shared.duration.load(Ordering::Acquire))
    }

    /// Frames per device callback and the device's reported output latency (callback → first
    /// frame audible), as of the last callback; `None` before the first one.
    pub fn output_stats(&self) -> Option<(u64, Duration)> {
        let frames = self.shared.callback_frames.load(Ordering::Relaxed);
        let latency = Duration::from_micros(self.shared.latency_us.load(Ordering::Relaxed));
        (frames > 0).then_some((frames, latency))
    }

    /// Output rate; sources and sound effects must match it (see [`Sfx::resampled`]).
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
/// Output rates to try, best first: the game's own, then common device rates.
const RATES: [u32; 8] = [
    OUTPUT_RATE,
    44_100,
    96_000,
    88_200,
    192_000,
    176_400,
    32_000,
    24_000,
];

/// The device configuration and rate to open: [`OUTPUT_RATE`] if possible, else the first of
/// [`RATES`] it offers. `WACCA_AUDIO_RATE=<Hz>` asks for that rate first (testing the resampler
/// on a 48 kHz device, or working around a driver).
fn pick_rate(
    ranges: &[SupportedStreamConfigRange],
    channels: u16,
) -> Option<(SupportedStreamConfigRange, u32)> {
    let forced = std::env::var("WACCA_AUDIO_RATE")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok());
    forced
        .into_iter()
        .chain(RATES)
        .find_map(|rate| pick_config(ranges, rate, channels).map(|r| (r, rate)))
}

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
    /// Output rate (frames per second), to judge the reported latency.
    rate: u32,
    /// The largest callback seen, in frames: the fallback latency is based on it, so it stays
    /// steady while callback sizes vary (Android's do, 500 to 2200 frames).
    max_frames: usize,
}

/// A reported output latency above this is taken as a bad timestamp, not a real delay.
const MAX_LATENCY: Duration = Duration::from_millis(500);

/// The latency to use for callbacks of up to `frames` frames at `rate`: the device's report, or,
/// if that is implausible, two buffers. Android's AAudio timestamps (through cpal) have reported
/// 24 s and more, which held the song clock at the start: no previews, charts that never moved.
fn plausible_latency(reported: Duration, frames: usize, rate: u32) -> Duration {
    if reported <= MAX_LATENCY {
        return reported;
    }
    Duration::from_secs_f64(2.0 * frames as f64 / f64::from(rate.max(1))).min(MAX_LATENCY)
}

impl Output {
    /// One device callback: render with the current shared state and report it to the clock.
    /// Returns the number of BGM frames consumed.
    fn callback<T: Sample + FromSample<f32>>(
        &mut self,
        out: &mut [T],
        shared: &Shared,
        latency: Duration,
        now: Instant,
    ) -> u64 {
        let generation = shared.generation.load(Ordering::Acquire);
        let playing = shared.playing.load(Ordering::Acquire);
        let gain = f32::from_bits(shared.volume.load(Ordering::Acquire));
        let written = self.render(out, generation, playing, gain);
        let frames = out.len() / self.channels.max(1);
        self.max_frames = self.max_frames.max(frames);
        let latency = plausible_latency(latency, self.max_frames, self.rate);
        shared
            .callback_frames
            .store(frames as u64, Ordering::Relaxed);
        shared
            .latency_us
            .store(latency.as_micros() as u64, Ordering::Relaxed);
        shared
            .clock()
            .on_callback(generation, written, latency, now);
        written
    }

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
                output.callback(out, &shared, latency, now);
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
}

impl Feed {
    fn new(rx: Receiver<Chunk>) -> Self {
        Feed {
            rx,
            chunk: Chunk {
                generation: 0,
                channels: 1,
                samples: Vec::new(),
            },
            read: 0,
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
                let end = (self.read + self.chunk.channels.max(1)).min(self.chunk.samples.len());
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

/// Write one source frame to a device frame (see [`mixer::channel`]).
fn map_frame(dst: &mut [f32], src: &[f32], gain: f32) {
    for (c, d) in dst.iter_mut().enumerate() {
        *d = mixer::channel(src, c) * gain;
    }
}

/// Seek `source` to `frame`; whether there is something to decode afterwards.
fn rewind(source: &mut Option<Box<dyn Decoder>>, frame: u64) -> bool {
    let Some(dec) = source else { return false };
    match dec.seek(frame) {
        Ok(()) => true,
        Err(e) => {
            log::error!("seek failed: {e}");
            false
        }
    }
}

/// Decoder thread: waits for a load or seek, then decodes ahead until the end or the next
/// command. Exits when the [`Player`] or the callback side is dropped.
fn decode_loop(commands: Receiver<Command>, out: SyncSender<Chunk>) {
    let mut source: Option<Box<dyn Decoder>> = None;
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
        match cmd {
            Some(Command::Load {
                generation: g,
                decoder,
            }) => {
                generation = g;
                silence = 0;
                source = Some(decoder);
                active = rewind(&mut source, 0);
                continue;
            }
            Some(Command::Seek {
                generation: g,
                frame,
            }) => {
                generation = g;
                silence = frame.min(0).unsigned_abs();
                active = rewind(&mut source, frame.max(0).unsigned_abs());
                continue;
            }
            None => {}
        }
        let Some(dec) = source.as_mut() else {
            active = false;
            continue;
        };
        let channels = usize::from(dec.channels().max(1));
        let mut samples = Vec::new();
        if silence > 0 {
            let n = silence.min(SILENCE_CHUNK);
            silence -= n;
            samples.resize(n as usize * channels, 0.0);
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
                channels,
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

    /// Decoder producing `base + i` for frame `i` on every channel, in blocks of 4 frames.
    #[derive(Debug)]
    struct Ramp {
        rate: u32,
        channels: u16,
        total: u64,
        base: f32,
        pos: u64,
    }

    impl Decoder for Ramp {
        fn channels(&self) -> u16 {
            self.channels
        }
        fn sample_rate(&self) -> u32 {
            self.rate
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
                out.extend(std::iter::repeat_n(
                    self.base + i as f32,
                    self.channels.into(),
                ));
            }
            self.pos = end;
            Ok(true)
        }
        fn seek(&mut self, frame: u64) -> Result<(), Error> {
            self.pos = frame.min(self.total);
            Ok(())
        }
    }

    fn ramp(channels: u16, total: u64, base: f32) -> Box<dyn Decoder> {
        Box::new(Ramp {
            rate: 1000,
            channels,
            total,
            base,
            pos: 0,
        })
    }

    fn chunk(generation: u64, channels: usize, samples: &[f32]) -> Chunk {
        Chunk {
            generation,
            channels,
            samples: samples.to_vec(),
        }
    }

    /// Run one-frame device callbacks (stereo device) until `n` BGM frames were heard; returns
    /// the left channel of each, divided by the volume (keeps ramp values below the clamp).
    fn pull(output: &mut Output, p: &Player, n: usize) -> Vec<f32> {
        let mut got = Vec::new();
        for _ in 0..1_000_000 {
            if got.len() == n {
                break;
            }
            let mut out = [0.0f32; 2];
            if output.callback(&mut out, &p.shared, Duration::ZERO, Instant::now()) == 1 {
                got.push((out[0] / p.volume()).round());
            } else {
                std::thread::yield_now();
            }
        }
        got
    }

    #[test]
    fn callbacks_report_their_size_and_latency() {
        let (p, ends) = Player::detached(1000, 2);
        let mut output = ends.output;
        assert_eq!(p.output_stats(), None);
        let mut out = [0.0f32; 2 * 48];
        output.callback(
            &mut out,
            &p.shared,
            Duration::from_millis(3),
            Instant::now(),
        );
        assert_eq!(p.output_stats(), Some((48, Duration::from_millis(3))));
    }

    #[test]
    fn load_swaps_the_source_on_one_device_and_resets_the_clock() {
        let (p, ends) = Player::detached(1000, 2);
        let mut output = ends.output;
        let t = std::thread::spawn(move || decode_loop(ends.commands, ends.chunks));

        assert_eq!(p.duration(), 0.0);
        p.load(ramp(1, 100, 0.0)).unwrap();
        assert!(!p.is_playing());
        assert!((p.duration() - 0.1).abs() < 1e-12);
        p.set_volume(0.001);
        p.play();
        assert_eq!(pull(&mut output, &p, 3), [0.0, 1.0, 2.0]);
        assert!(p.position() > 0.0);

        // Swap mid-stream: paused at 0 with full volume, no stale frames of the old source.
        p.set_volume(0.5);
        p.load(ramp(2, 50, 100.0)).unwrap();
        assert_eq!(p.position(), 0.0);
        assert!(!p.is_playing());
        assert_eq!(p.volume(), 1.0);
        assert!((p.duration() - 0.05).abs() < 1e-12);
        let mut out = [7.0f32; 2];
        assert_eq!(
            output.callback(&mut out, &p.shared, Duration::ZERO, Instant::now()),
            0
        );
        assert_eq!(out, [0.0, 0.0]);
        p.set_volume(0.001);
        p.play();
        assert_eq!(pull(&mut output, &p, 2), [100.0, 101.0]);

        // Seek within the new source restarts the clock at the target.
        p.seek(0.01).unwrap();
        assert_eq!(p.position(), 0.01);
        assert_eq!(pull(&mut output, &p, 1), [110.0]);
        assert!(p.position() >= 0.01);

        // Another rate is resampled to the output's: 2000 frames at 2 kHz last 1 s here too.
        let other_rate = Box::new(Ramp {
            rate: 2000,
            channels: 1,
            total: 2000,
            base: 0.0,
            pos: 0,
        });
        p.load(other_rate).unwrap();
        assert!((p.duration() - 1.0).abs() < 1e-9);
        drop(output);
        drop(p);
        t.join().unwrap();
    }

    #[test]
    fn implausible_latency_falls_back_to_two_buffers() {
        let ms = Duration::from_millis;
        assert_eq!(plausible_latency(ms(20), 480, 48_000), ms(20));
        // 24 s reported for 960-frame callbacks at 48 kHz: two buffers = 40 ms.
        assert_eq!(
            plausible_latency(Duration::from_secs(24), 960, 48_000),
            ms(40)
        );
        assert_eq!(
            plausible_latency(Duration::MAX, 48_000, 48_000),
            MAX_LATENCY
        );
    }

    #[test]
    fn feed_maps_stereo_applies_gain_and_pads_underrun() {
        let (tx, rx) = mpsc::sync_channel(4);
        let mut feed = Feed::new(rx);
        tx.send(chunk(1, 2, &[1.0, 2.0, 3.0, 4.0])).unwrap();
        let mut out = [9.0f32; 9]; // 3 frames of 3 channels
        assert_eq!(feed.fill(&mut out, 3, 1, true, 0.5), 2);
        assert_eq!(out, [0.5, 1.0, 0.0, 1.5, 2.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn render_mixes_sfx_over_mono_bgm_and_clamps_into_the_device_format() {
        let (tx, rx) = mpsc::sync_channel(4);
        let (sfx_tx, sfx_rx) = mpsc::sync_channel(4);
        let mut output = Output {
            feed: Feed::new(rx),
            mixer: Mixer::new(sfx_rx),
            mix: Vec::new(),
            channels: 2,
            rate: 1000,
            max_frames: 0,
        };
        tx.send(chunk(1, 1, &[0.75, 0.25])).unwrap();
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
        let mut feed = Feed::new(rx);
        tx.send(chunk(1, 1, &[1.0, 1.0])).unwrap();
        tx.send(chunk(2, 1, &[2.0])).unwrap();
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
        cmd_tx
            .send(Command::Load {
                generation: 2,
                decoder: ramp(1, 10, 0.0),
            })
            .unwrap();
        cmd_tx
            .send(Command::Seek {
                generation: 3,
                frame: -2,
            })
            .unwrap();
        let t = std::thread::spawn(move || decode_loop(cmd_rx, tx));
        let mut got = Vec::new();
        while got.len() < 12 {
            let c = rx.recv().unwrap();
            if c.generation == 2 {
                continue; // decoded before the seek arrived
            }
            assert_eq!((c.generation, c.channels), (3, 1));
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
    fn decode_loop_ignores_seeks_before_any_source() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (tx, rx) = mpsc::sync_channel(4);
        cmd_tx
            .send(Command::Seek {
                generation: 1,
                frame: 0,
            })
            .unwrap();
        drop(cmd_tx);
        decode_loop(cmd_rx, tx);
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
