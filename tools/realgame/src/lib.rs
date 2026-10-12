//! Pieces of the real-game harness (`docs/research/07-real-game-harness.md`).
//!
//! Everything here is pure and unit-tested; nothing in this crate starts, attaches to or writes to the real game
//! unless a caller explicitly opens the shared buffer with [`shm::SharedBuffer::open_existing`].

pub mod cells;
pub mod shm;
pub mod timeline;
pub mod wcon;
