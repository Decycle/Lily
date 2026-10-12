//! CRI ACB/AWB/HCA decoding and audio output with a playback clock.
//!
//! Spec: `docs/research/03-audio-and-jackets.md`.
//!
//! - [`cri`]: `@UTF` tables, AFS2 (`.awb`) indexes and the ACB cue map.
//! - [`bank`]: opens a cue from `Sound/Bgm` as a streaming [`Decoder`].
//! - [`hca`]: the [`Decoder`] trait and the HCA implementation (`cridecoder`).
//! - [`clock`] / [`player`]: `cpal` output and the device-driven song clock.
//!
//! Out of scope for now: ADX decoding and the in-memory AWB of `MER_SE` (sound effects).

pub mod bank;
pub mod clock;
pub mod cri;
mod error;
pub mod hca;
pub mod player;

pub use bank::BgmBank;
pub use clock::Clock;
pub use error::Error;
pub use hca::{Decoder, HcaStream};
pub use player::Player;
