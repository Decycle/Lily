//! wgpu renderer: playfield, notes, effects. Spec: docs/research/04-gameplay-mechanics.md, 06-visuals.md
//!
//! Split: [`layout`] and [`scene`] are pure geometry (unit-tested); [`Renderer`] only uploads and
//! draws the resulting triangle list.

pub mod layout;
mod renderer;
pub mod scene;
pub mod style;
pub mod text;
mod video;

pub use layout::Vertex;
pub use renderer::Renderer;
pub use scene::{LiveState, SceneOptions, build_scene};
