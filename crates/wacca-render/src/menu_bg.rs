//! The menus' animated backdrop, the UI material `uM_WCbg000` ported to WGSL (`menu_bg.wgsl`).
//! Drawn as one quad before the menu overlay. Its textures come from `wacca-convert
//! --export-ui UI/Textures`; without them nothing is drawn.

use std::path::Path;

use crate::layout::Rect;

/// Textures of the material: (file under `data/ui/UI/Textures`, sampled with sRGB decode).
const TEXTURES: [(&str, bool); 3] = [
    ("uT_WCbg00", true),
    ("uT_WCbg01", false),
    ("uT_CM19", false),
];

#[derive(Debug)]
pub struct MenuBackdrop {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// Clip-space corners `[x0, y0, x1, y1]` of `rect` on a `size` target.
pub fn clip_rect(rect: Rect, size: [f32; 2]) -> [f32; 4] {
    let x = |px: f32| px / size[0] * 2.0 - 1.0;
    let y = |py: f32| 1.0 - py / size[1] * 2.0;
    [x(rect.x), y(rect.y), x(rect.x + rect.w), y(rect.y + rect.h)]
}

fn load(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    path: &Path,
    srgb: bool,
) -> Option<wgpu::TextureView> {
    let bytes = std::fs::read(path)
        .map_err(|e| log::warn!("menu backdrop {}: {e}", path.display()))
        .ok()?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|e| log::warn!("menu backdrop {}: {e}", path.display()))
        .ok()?
        .to_rgba8();
    let size = wgpu::Extent3d {
        width: img.width(),
        height: img.height(),
        depth_or_array_layers: 1,
    };
    let format = if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("menu backdrop"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        tex.as_image_copy(),
        &img,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * size.width),
            rows_per_image: None,
        },
        size,
    );
    Some(tex.create_view(&wgpu::TextureViewDescriptor::default()))
}

impl MenuBackdrop {
    /// `None` (with a warning) if a texture under `ui_textures` is missing.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        ui_textures: &Path,
    ) -> Option<Self> {
        let views = TEXTURES
            .iter()
            .map(|(name, srgb)| {
                load(
                    device,
                    queue,
                    &ui_textures.join(format!("{name}.png")),
                    *srgb,
                )
            })
            .collect::<Option<Vec<_>>>()?;
        let module = device.create_shader_module(wgpu::include_wgsl!("menu_bg.wgsl"));
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("menu backdrop"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1),
                tex_entry(2),
                tex_entry(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("menu backdrop"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // UE's default sampler wraps; the rotated lookups leave 0..1.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("menu backdrop"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("menu backdrop"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("menu backdrop"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("menu backdrop"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        Some(Self {
            pipeline,
            uniforms,
            group,
        })
    }

    /// Draw the backdrop into `rect` (window pixels) of `target` (`size` pixels) at material
    /// time `time` seconds.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        rect: Rect,
        time: f32,
    ) {
        let c = clip_rect(rect, [size[0] as f32, size[1] as f32]);
        let data: [f32; 8] = [c[0], c[1], c[2], c[3], time, 0.0, 0.0, 0.0];
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&data));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("menu backdrop"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..4, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_rect_maps_window_pixels() {
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        assert_eq!(clip_rect(r, [200.0, 100.0]), [-1.0, 1.0, 0.0, 0.0]);
    }
}
