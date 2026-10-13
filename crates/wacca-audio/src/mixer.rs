//! Sound-effect voices mixed on top of the BGM inside the device callback.
//!
//! The app sends [`Command`]s through a bounded channel; the callback drains it, so a trigger is
//! heard within one device buffer. The voice list is preallocated and never grows, and voices
//! only hold `Arc` clones of their samples, so the callback does not allocate. (If the app drops
//! its last handle to a playing [`Sfx`], the callback frees it when the voice ends; keep the
//! sounds alive, as [`crate::HitSounds`] does.)

use std::sync::mpsc::Receiver;

use crate::Sfx;

/// Simultaneous voices; the oldest is stolen beyond this.
pub const MAX_VOICES: usize = 32;
/// Fade applied by [`Command::Stop`] to avoid a click (≈ 5 ms at 48 kHz).
const FADE_FRAMES: usize = 256;

#[derive(Debug)]
pub(crate) enum Command {
    Play {
        id: u64,
        sfx: Sfx,
        gain: f32,
    },
    /// Fade the voice out (and end any sustain loop).
    Stop {
        id: u64,
    },
}

/// Sample for device channel `c` from one source frame: mono feeds every channel, otherwise
/// channel `c` takes source channel `c` and extra device channels are silent.
pub(crate) fn channel(src: &[f32], c: usize) -> f32 {
    match src {
        [mono] => *mono,
        _ => src.get(c).copied().unwrap_or(0.0),
    }
}

#[derive(Debug)]
struct Voice {
    id: u64,
    sfx: Sfx,
    pos: usize,
    gain: f32,
    /// Frames left in the stop fade.
    fade: Option<usize>,
}

impl Voice {
    /// The sustain loop, while it is still active.
    fn active_loop(&self) -> Option<(usize, usize)> {
        self.sfx
            .loop_range
            .filter(|(start, end)| start < end && self.fade.is_none())
    }

    fn done(&self) -> bool {
        self.fade == Some(0) || (self.pos >= self.sfx.frames() && self.active_loop().is_none())
    }

    /// Add this voice into `out`; `false` once it has finished.
    fn mix(&mut self, out: &mut [f32], out_channels: usize) -> bool {
        let ch = usize::from(self.sfx.channels.max(1));
        for frame in out.chunks_mut(out_channels) {
            if let Some((start, end)) = self.active_loop()
                && self.pos >= end
            {
                self.pos = start;
            }
            if self.done() {
                return false;
            }
            let gain = match &mut self.fade {
                Some(left) => {
                    *left -= 1;
                    self.gain * (*left + 1) as f32 / FADE_FRAMES as f32
                }
                None => self.gain,
            };
            let Some(src) = self.sfx.samples.get(self.pos * ch..(self.pos + 1) * ch) else {
                return false;
            };
            for (c, d) in frame.iter_mut().enumerate() {
                *d += channel(src, c) * gain;
            }
            self.pos += 1;
        }
        !self.done()
    }
}

/// Callback-side voice pool.
#[derive(Debug)]
pub(crate) struct Mixer {
    rx: Receiver<Command>,
    voices: Vec<Voice>,
}

impl Mixer {
    pub(crate) fn new(rx: Receiver<Command>) -> Self {
        Mixer {
            rx,
            voices: Vec::with_capacity(MAX_VOICES),
        }
    }

    fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Play { id, sfx, gain } => {
                let voice = Voice {
                    id,
                    sfx,
                    pos: 0,
                    gain,
                    fade: None,
                };
                if self.voices.len() < MAX_VOICES {
                    self.voices.push(voice);
                } else if let Some(oldest) = self.voices.iter_mut().min_by_key(|v| v.id) {
                    *oldest = voice;
                }
            }
            Command::Stop { id } => {
                if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
                    v.fade.get_or_insert(FADE_FRAMES);
                }
            }
        }
    }

    /// Take pending commands, then add every voice into `out` (interleaved, `out_channels` wide).
    pub(crate) fn mix(&mut self, out: &mut [f32], out_channels: usize) {
        while let Ok(cmd) = self.rx.try_recv() {
            self.apply(cmd);
        }
        let out_channels = out_channels.max(1);
        self.voices.retain_mut(|v| v.mix(out, out_channels));
    }

    #[cfg(test)]
    fn ids(&self) -> Vec<u64> {
        self.voices.iter().map(|v| v.id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sfx::tests::sfx;
    use std::sync::mpsc;

    fn play(id: u64, sfx: &Sfx, gain: f32) -> Command {
        Command::Play {
            id,
            sfx: sfx.clone(),
            gain,
        }
    }

    #[test]
    fn mixes_voices_additively_with_gain_and_channel_mapping() {
        let (tx, rx) = mpsc::sync_channel(8);
        let mut m = Mixer::new(rx);
        tx.send(play(1, &sfx(48000, 1, &[1.0, 0.5]), 0.5)).unwrap();
        tx.send(play(
            2,
            &sfx(48000, 2, &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]),
            1.0,
        ))
        .unwrap();
        let mut out = [1.0f32; 6]; // 2 frames × 3 channels on top of existing BGM
        m.mix(&mut out, 3);
        let want = [1.6, 1.7, 1.5, 1.55, 1.65, 1.25];
        for (o, w) in out.iter().zip(want) {
            assert!((o - w).abs() < 1e-6, "{out:?}");
        }
        // Mono voice ended after 2 frames; the stereo one has one frame left.
        assert_eq!(m.ids(), [2]);
        let mut out = [0.0f32; 6];
        m.mix(&mut out, 3);
        assert_eq!(out, [0.5, 0.6, 0.0, 0.0, 0.0, 0.0]);
        assert!(m.ids().is_empty());
    }

    #[test]
    fn steals_the_oldest_voice_when_full() {
        let (tx, rx) = mpsc::sync_channel(64);
        let mut m = Mixer::new(rx);
        let s = sfx(48000, 1, &[0.0; 100]);
        for id in 0..MAX_VOICES as u64 + 2 {
            tx.send(play(id, &s, 1.0)).unwrap();
        }
        m.mix(&mut [], 1);
        let mut ids = m.ids();
        ids.sort();
        assert_eq!(ids, (2..MAX_VOICES as u64 + 2).collect::<Vec<_>>());
    }

    #[test]
    fn loops_sustain_until_stopped_then_fades_out() {
        let (tx, rx) = mpsc::sync_channel(8);
        let mut m = Mixer::new(rx);
        let mut s = sfx(48000, 1, &[1.0, 2.0, 3.0, 4.0]);
        s.loop_range = Some((1, 3));
        tx.send(play(7, &s, 1.0)).unwrap();
        let mut out = [0.0f32; 6];
        m.mix(&mut out, 1);
        assert_eq!(out, [1.0, 2.0, 3.0, 2.0, 3.0, 2.0]);
        tx.send(Command::Stop { id: 7 }).unwrap();
        tx.send(Command::Stop { id: 99 }).unwrap(); // unknown ids are ignored
        let mut out = vec![0.0f32; FADE_FRAMES + 4];
        m.mix(&mut out, 1);
        // Fading voice plays on past the loop end without looping, then ends.
        assert_eq!(out[0], 3.0);
        assert!((out[1] - 4.0 * (FADE_FRAMES - 1) as f32 / FADE_FRAMES as f32).abs() < 1e-6);
        assert!(out[2..].iter().all(|&v| v == 0.0));
        assert!(m.ids().is_empty());
    }
}
