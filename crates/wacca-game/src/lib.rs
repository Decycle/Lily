//! WACCA game rules. Pure, deterministic logic driven by chart time.
//!
//! Specs: `docs/research/02-data-tables.md`, `04-gameplay-mechanics.md`, `05-save-and-song-select.md`.

pub mod catalog;
pub mod grade;
pub mod input;
pub mod rating;
pub mod rules;
pub mod save;
pub mod score;

pub use wacca_chart::Difficulty;
