//! wgpu renderer: playfield, notes, effects. Spec: docs/research/04-gameplay-mechanics.md, 06-visuals.md
//!
//! Split: [`layout`], [`scene`] and the baking/packing in [`sprites`] are pure (unit-tested);
//! [`Renderer`] only uploads the sprite atlas and draws the resulting triangle list.

pub mod layout;
mod renderer;
pub mod scene;
pub mod sprites;
pub mod style;
pub mod text;
mod video;

pub use layout::{Paint, Vertex};
pub use renderer::Renderer;
pub use scene::{Flash, LiveState, SceneOptions, build_scene};
pub use sprites::{JudgeSprite, Sprite, SpriteSheet};
