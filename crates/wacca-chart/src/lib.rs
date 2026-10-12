//! WACCA `.mer` chart model, parser and timing.
//!
//! Spec: `docs/research/01-chart-format.md`.

pub mod model;
mod parse;
mod timing;

pub use model::*;
pub use parse::{ParseError, parse};
