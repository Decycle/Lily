//! WACCA `.mer` chart model, parser and timing.
//!
//! Spec: `docs/research/01-chart-format.md`.

mod mirror;
pub mod model;
mod parse;
mod timing;

pub use mirror::{mirror_arc, mirrored};
pub use model::*;
pub use parse::{ParseError, parse};
