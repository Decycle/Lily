//! UI materials: WGSL ports of the game's UMG materials, drawn by the overlay in draw-list order
//! (a quad with its own pipeline between the plain overlay quads). Each port lives in its own
//! WGSL file appended to `material_common.wgsl`; a draw carries up to four textures (exported
//! UI texture keys) and [`PARAMS`] float4 parameters.

/// Float4 parameters per draw after the tint (one 256-byte uniform slot in all).
pub const PARAMS: usize = 15;
/// Textures a material can sample.
pub const TEXTURES: usize = 4;

/// A ported material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UiMaterial {
    /// `uM_Title012`, the title key art's glitching, recoloured layer (`title_glitch.wgsl`).
    TitleGlitch,
    /// `uM_Chara_2`, smoky additive glow (`chara_glow.wgsl`).
    CharaGlow,
    /// `uM_CMcircle001`, the mode select's swirling art ring (`circle_swirl.wgsl`).
    CircleSwirl,
}

/// Colour blending of a material's output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// UE `BLEND_Translucent`: over the target by opacity.
    Translucent,
    /// UE `BLEND_Additive`: added, scaled by opacity.
    Additive,
}

impl UiMaterial {
    pub const ALL: [UiMaterial; 3] = [
        UiMaterial::TitleGlitch,
        UiMaterial::CharaGlow,
        UiMaterial::CircleSwirl,
    ];

    pub(super) fn source(self) -> String {
        let body = match self {
            UiMaterial::TitleGlitch => include_str!("title_glitch.wgsl"),
            UiMaterial::CharaGlow => include_str!("chara_glow.wgsl"),
            UiMaterial::CircleSwirl => include_str!("circle_swirl.wgsl"),
        };
        format!("{}\n{body}", include_str!("material_common.wgsl"))
    }

    pub fn blend(self) -> Blend {
        match self {
            UiMaterial::TitleGlitch | UiMaterial::CircleSwirl => Blend::Translucent,
            UiMaterial::CharaGlow => Blend::Additive,
        }
    }

    pub(super) fn blend_state(self) -> wgpu::BlendState {
        match self.blend() {
            Blend::Translucent => wgpu::BlendState::ALPHA_BLENDING,
            Blend::Additive => wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            },
        }
    }
}

/// Per-draw uniform block: linear tint, then the parameters (unused ones zero).
pub(super) fn uniform_block(tint: [f32; 4], params: &[[f32; 4]]) -> [[f32; 4]; PARAMS + 1] {
    let mut block = [[0.0; 4]; PARAMS + 1];
    block[0] = tint;
    for (slot, p) in block[1..].iter_mut().zip(params) {
        *slot = *p;
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_block_puts_tint_first_and_pads() {
        let b = uniform_block([1.0, 0.5, 0.25, 1.0], &[[2.0; 4], [3.0; 4]]);
        assert_eq!(b[0], [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(b[1], [2.0; 4]);
        assert_eq!(b[2], [3.0; 4]);
        assert_eq!(b[3], [0.0; 4]);
        assert_eq!(size_of_val(&b), 256);
    }

    #[test]
    fn every_material_validates() {
        use wgpu::naga;
        for m in UiMaterial::ALL {
            let module =
                naga::front::wgsl::parse_str(&m.source()).unwrap_or_else(|e| panic!("{m:?}: {e}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{m:?}: {e:?}"));
        }
    }
}
