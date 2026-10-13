//! Pieces of the real-game harness (`docs/research/07-real-game-harness.md`).
//!
//! Everything here is unit-tested without the game; nothing in this crate starts, attaches to or writes to the
//! real game unless a caller explicitly opens the shared buffer with [`shm::SharedBuffer::open_existing`] (the
//! `wacca-realgame inject` binary does so only when opted in) or calls [`game::launch`] (`run_experiment`).

pub mod cells;
pub mod e1;
pub mod e3;
pub mod e3_analysis;
pub mod experiment;
pub mod frame;
pub mod game;
pub mod gpu;
pub mod inject;
pub mod nav;
pub mod ours;
pub mod panel;
pub mod parity;
pub mod perfect;
pub mod playfield;
pub mod raster;
pub mod record;
pub mod reference;
pub mod shm;
pub mod stats;
pub mod sync;
pub mod testscreen;
pub mod timeline;
pub mod wcon;
