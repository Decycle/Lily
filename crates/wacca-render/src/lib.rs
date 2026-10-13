//! wgpu renderer: playfield, notes, effects. Spec: docs/research/04-gameplay-mechanics.md, 06-visuals.md
//!
//! Split: [`layout`], [`scene`] and the baking/packing in [`sprites`] are pure (unit-tested);
//! [`Renderer`] only uploads the sprite atlas and draws the resulting triangle list.

pub mod camera;
pub mod effects;
mod fx_gpu;
pub mod layout;
pub mod notes3d;
mod notes_gpu;
pub mod particles;
mod renderer;
pub mod scene;
pub mod sprites;
pub mod stage;
mod stage_gpu;
pub mod style;
pub mod sync;
pub mod text;
mod video;

pub use effects::Effects;
pub use layout::{Paint, Vertex};
pub use notes3d::{MeshVertex, NoteMeshes, build_mesh_notes};
pub use renderer::Renderer;
pub use scene::{LiveState, RimState, SceneOptions, build_scene, build_scene_into};
pub use sprites::{JudgeSprite, Sprite, SpriteSheet, TextSprite};
pub use stage::{Stage, build_stage_into};
