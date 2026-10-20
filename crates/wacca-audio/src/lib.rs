//! CRI ACB/AWB/HCA/ADX decoding and audio output with a playback clock.
//!
//! Spec: `docs/research/03-audio-and-jackets.md`.
//!
//! - [`cri`]: `@UTF` tables, AFS2 (`.awb`) indexes and the ACB cue map.
//! - [`bank`]: opens a cue from `Sound/Bgm` as a streaming [`Decoder`].
//! - [`hca`]: the [`Decoder`] trait and the HCA implementation (`cridecoder`).
//! - [`adx`]: CRI ADX decoding (20 of the sound effects).
//! - [`se`]: the sound-effect bank (`MER_SE`, in-memory AWB) and the hit sounds of an SE set.
//! - [`clock`] / [`player`]: `cpal` output, the device-driven song clock and sound-effect voices.

pub mod adx;
pub mod bank;
pub mod clock;
pub mod cri;
mod error;
pub mod hca;
mod mixer;
pub mod player;
pub mod resample;
pub mod se;
mod sfx;

pub use bank::BgmBank;
pub use clock::Clock;
pub use error::Error;
pub use hca::{Decoder, HcaStream};
pub use mixer::MAX_VOICES;
pub use player::{OUTPUT_RATE, Player, VoiceId};
pub use se::{DEFAULT_SE_SET, HitSounds, SeBank, SeSet};
pub use sfx::Sfx;
