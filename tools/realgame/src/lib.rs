//! Pieces of the real-game harness (`docs/research/07-real-game-harness.md`).
//!
//! Everything here is unit-tested without the game; nothing in this crate starts, attaches to or writes to the
//! real game unless a caller explicitly opens the shared buffer with [`shm::SharedBuffer::open_existing`] (the
//! `wacca-realgame inject` binary does so only when opted in).

pub mod cells;
pub mod inject;
pub mod shm;
pub mod stats;
pub mod timeline;
pub mod wcon;
