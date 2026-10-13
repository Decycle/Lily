//! wgpu side of [`crate::notes3d`]: the note material pipeline (`notes.wgsl`), its textures and
//! the per-frame vertex buffer. Without meshes or textures nothing is drawn (2D notes instead).

use std::path::Path;

use crate::notes3d::{MATERIALS, MeshVertex, NoteMeshes};
use crate::sprites::{self, Image};

/// The note material textures: file stem under `data/ui/Vfx/Textures` and whether the material
/// samples it as sRGB colour (`SAMPLERTYPE_Color`) or linear (`LinearColor`).
const TEXTURES: [(&str, bool); 2] = [("vT_NoteBase00_00_m", true), ("vT_NoteBase01_00_m", false)];

const MIN_BUFFER_BYTES: u64 = 256 * 1024;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuMaterial {
    color: [f32; 4],
    p0: [f32; 4],
    p1: [f32; 4],
}

fn gpu_materials() -> [GpuMaterial; MATERIALS.len()] {
    MATERIALS.map(|m| GpuMaterial {
        color: [m.color[0], m.color[1], m.color[2], 1.0],
        p0: [m.dark, m.highlight, m.line, m.chain],
        p1: [m.bonus, m.bonus_flip, f32::from(u8::from(m.bar_line)), 0.0],
    })
}

#[derive(Debug)]
pub(crate) struct NoteLayer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    materials: wgpu::Buffer,
    bind: Option<wgpu::BindGroup>,
    vertices: wgpu::Buffer,
    meshes: Option<NoteMeshes>,
}

impl NoteLayer {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("notes.wgsl"));
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("notes"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex(1),
                tex(2),
                sampler(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("notes"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let constants = [("LINEARIZE", if format.is_srgb() { 1.0 } else { 0.0 })];
        let options = wgpu::PipelineCompilationOptions {
            constants: &constants,
            ..Default::default()
        };
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Uint32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("notes"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: options.clone(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<MeshVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: options,
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Masked material: opaque where not discarded.
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let materials = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("note materials"),
            size: size_of::<[GpuMaterial; MATERIALS.len()]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            materials,
            bind: None,
            vertices: vertex_buffer(device, MIN_BUFFER_BYTES),
            meshes: None,
        }
    }

    /// Loads `data_dir/meshes/*.obj` and the material textures from `ui_dir/Vfx/Textures`;
    /// anything missing leaves the layer off.
    pub(crate) fn load(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        meshes_dir: &Path,
        ui_dir: &Path,
    ) {
        self.meshes = None;
        self.bind = None;
        let Some(meshes) = NoteMeshes::load(meshes_dir) else {
            return;
        };
        let dir = ui_dir.join("Vfx").join("Textures");
        let mut views = Vec::new();
        for (stem, srgb) in TEXTURES {
            match sprites::read_png(&dir.join(stem).with_extension("png")) {
                Ok(img) => views.push(upload(device, queue, stem, img, srgb)),
                Err(e) => {
                    log::warn!("note textures: {e}; drawing 2D notes");
                    return;
                }
            }
        }
        queue.write_buffer(&self.materials, 0, bytemuck::bytes_of(&gpu_materials()));
        let clamp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("notes"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: self.materials.as_entire_binding(),
        }];
        for (i, view) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + i as u32,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&clamp),
        });
        self.bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("notes"),
            layout: &self.layout,
            entries: &entries,
        }));
        self.meshes = Some(meshes);
        log::info!("note meshes loaded: notes drawn in 3D");
    }

    /// The meshes, when the layer can draw.
    pub(crate) fn meshes(&self) -> Option<&NoteMeshes> {
        self.bind.as_ref().and(self.meshes.as_ref())
    }

    /// Uploads `notes` (call before the pass).
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        notes: &[MeshVertex],
    ) {
        let bytes: &[u8] = bytemuck::cast_slice(notes);
        if bytes.len() as u64 > self.vertices.size() {
            self.vertices = vertex_buffer(device, (bytes.len() as u64).next_power_of_two());
        }
        queue.write_buffer(&self.vertices, 0, bytes);
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass, count: usize) {
        let Some(bind) = &self.bind else { return };
        if count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        let bytes = (count * size_of::<MeshVertex>()) as u64;
        pass.set_vertex_buffer(0, self.vertices.slice(..bytes));
        pass.draw(0..count as u32, 0..1);
    }
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("note vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Texture with its full mip chain.
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    img: Image,
    srgb: bool,
) -> wgpu::TextureView {
    let mips = sprites::mip_chain(img);
    let [width, height] = mips[0].size;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, mip) in mips.iter().enumerate() {
        let [w, h] = mip.size;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &mip.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_shader_validates() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("notes.wgsl")).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn material_table_layout() {
        let m = gpu_materials();
        assert_eq!(size_of_val(&m), MATERIALS.len() * 48);
        assert_eq!(m[MATERIALS.len() - 1].p1[2], 1.0, "bar line flag");
        assert_eq!(m[0].p0, [0.8, 0.35, 0.08, 0.0]);
    }
}
