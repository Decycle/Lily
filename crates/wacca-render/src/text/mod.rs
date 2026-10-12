//! 2D overlay for menus and the HUD: UTF-8 text (incl. Japanese) from a glyph atlas, plus filled
//! rectangles and images, in window pixels.
//!
//! Split: [`layout`] (measure / fit / wrap / align) and the atlas packer are pure; [`Fonts`]
//! loads font files with per-character fallback; [`TextRenderer`] rasterizes and draws.

mod atlas;
mod fonts;
mod gpu;
pub mod layout;

pub(crate) use atlas::Shelves;
pub use fonts::{Fonts, TextError};
pub use gpu::TextRenderer;
pub use layout::Align;
