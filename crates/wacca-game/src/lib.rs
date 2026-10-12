//! WACCA game rules. Pure, deterministic logic driven by chart time.
//!
//! Specs: `docs/research/02-data-tables.md`, `04-gameplay-mechanics.md`, `05-save-and-song-select.md`.

pub mod catalog;
pub mod input;
pub mod judge;
pub mod rules;

pub use wacca_chart::Difficulty;
