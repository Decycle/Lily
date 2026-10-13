//! wgpu side of [`crate::particles`]: the effect material pipelines (`fx.wgsl`; additive,
//! translucent, masked), the material table, the effect textures (one array, resampled to
//! [`LAYER_SIZE`]²) and the per-frame vertices. Drawn inside the stage pass (linear HDR,
//! depth-tested; only masked effects write depth).

use std::path::Path;
use std::sync::Arc;

use crate::particles::{EffectLibrary, FxBlend, FxShader, FxVertex};
use crate::sprites::{self, Image};
use crate::stage::Stage;
use crate::stage_gpu::{HDR_FORMAT, depth_state};

/// Side of every texture layer.
const LAYER_SIZE: u32 = 256;
const MIN_BUFFER_BYTES: u64 = 256 * 1024;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuMaterial {
    params: [[f32; 4]; 6],
    info: [u32; 4],
}

fn shader_index(s: FxShader) -> u32 {
    match s {
        FxShader::Tap => 0,
        FxShader::Hold => 1,
        FxShader::Base004 => 2,
        FxShader::Base000 => 3,
        FxShader::Base005 => 4,
        FxShader::Scroll => 5,
        FxShader::Dissolve => 6,
        FxShader::Miss => 7,
        FxShader::Base003 => 9,
        FxShader::RankUp => 10,
        FxShader::Other => 8,
    }
}

fn gpu_materials(lib: &EffectLibrary) -> Vec<GpuMaterial> {
    lib.materials()
        .iter()
        .map(|m| {
            let layer = |t: Option<u32>| t.map_or(0, |i| i + 1);
            let srgb = |t: Option<u32>| {
                t.and_then(|i| lib.textures().get(i as usize))
                    .is_some_and(|t| t.srgb) as u32
            };
            GpuMaterial {
                params: m.params,
                info: [
                    shader_index(m.shader),
                    layer(m.textures[0]),
                    layer(m.textures[1]),
                    srgb(m.textures[0]) | srgb(m.textures[1]) << 1,
                ],
            }
        })
        .collect()
}

/// Bilinear resample of `img` to `side`² RGBA8.
fn resample(img: &Image, side: u32) -> Vec<u8> {
    let [w, h] = img.size;
    let mut out = Vec::with_capacity((side * side * 4) as usize);
    let at = |x: u32, y: u32, c: usize| f32::from(img.rgba[((y * w + x) * 4) as usize + c]);
    for y in 0..side {
        let fy = ((y as f32 + 0.5) * h as f32 / side as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (fy.floor() as u32, fy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..side {
            let fx = ((x as f32 + 0.5) * w as f32 / side as f32 - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (fx.floor() as u32, fx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
                let bottom = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
                out.push((top * (1.0 - ty) + bottom * ty).round() as u8);
            }
        }
    }
    out
}

#[derive(Debug)]
pub(crate) struct FxLayer {
    additive: wgpu::RenderPipeline,
    translucent: wgpu::RenderPipeline,
    masked: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    bind: Option<wgpu::BindGroup>,
    vertices: wgpu::Buffer,
    runs: Vec<(FxBlend, u32, u32)>,
    /// Runs drawn before the guide lanes.
    behind: Vec<(FxBlend, u32, u32)>,
    library: Option<Arc<EffectLibrary>>,
}

impl FxLayer {
    pub(crate) fn new(device: &wgpu::Device, samples: u32) -> Self {
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fx"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: fragment,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: fragment,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: fragment,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: fragment,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::include_wgsl!("fx.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fx"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4,
            5 => Uint32
        ];
        let pipeline = |entry: &str, blend, write| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: size_of::<FxVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    }],
                },
                // UNVERIFIED: captured mesh particles cull back faces; sprites are built
                // without a fixed winding, so nothing is culled.
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(depth_state(write)),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };
        let keep_alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let additive = pipeline(
            "fs_additive",
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: keep_alpha,
            }),
            false,
        );
        let translucent = pipeline(
            "fs_translucent",
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: keep_alpha,
            }),
            false,
        );
        let masked = pipeline("fs_masked", None, true);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fx uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            additive,
            translucent,
            masked,
            layout,
            uniform,
            bind: None,
            vertices: vertex_buffer(device, MIN_BUFFER_BYTES),
            runs: Vec::new(),
            behind: Vec::new(),
            library: None,
        }
    }

    /// Loads the effect library from `data_dir` (`effects/`, `meshes/`) and its textures from
    /// `ui_dir/Vfx/Textures`; without the library the layer stays off (2D fallback effects).
    pub(crate) fn load(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data_dir: &Path,
        ui_dir: &Path,
    ) {
        self.bind = None;
        self.library = None;
        let lib = match EffectLibrary::load(data_dir) {
            Ok(l) => l,
            Err(e) => {
                log::warn!("hit effects: {e}; drawing the 2D fallback");
                return;
            }
        };
        let layers = lib.textures().len().max(1) as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fx textures"),
            size: wgpu::Extent3d {
                width: LAYER_SIZE,
                height: LAYER_SIZE,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let dir = ui_dir.join("Vfx").join("Textures");
        for (i, t) in lib.textures().iter().enumerate() {
            let img =
                sprites::read_png(&dir.join(&t.name).with_extension("png")).unwrap_or_else(|e| {
                    log::warn!("hit effects: {e}; drawn white");
                    Image {
                        size: [1, 1],
                        rgba: vec![255; 4],
                    }
                });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: i as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &resample(&img, LAYER_SIZE),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * LAYER_SIZE),
                    rows_per_image: Some(LAYER_SIZE),
                },
                wgpu::Extent3d {
                    width: LAYER_SIZE,
                    height: LAYER_SIZE,
                    depth_or_array_layers: 1,
                },
            );
        }
        let materials = gpu_materials(&lib);
        let table = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fx materials"),
            size: (size_of::<GpuMaterial>() * materials.len().max(1)) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&table, 0, bytemuck::cast_slice(&materials));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fx"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        self.bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fx"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: table.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }));
        log::info!(
            "hit effects: {} materials, {} textures",
            materials.len(),
            lib.textures().len()
        );
        self.library = Some(Arc::new(lib));
    }

    pub(crate) fn library(&self) -> Option<Arc<EffectLibrary>> {
        self.bind.as_ref().and(self.library.clone())
    }

    /// Uploads the effect vertices of `stage`.
    pub(crate) fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stage: &Stage) {
        self.runs.clear();
        self.behind.clear();
        if self.bind.is_none() || stage.effects.is_empty() {
            return;
        }
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&[stage.time, 0.0, 0.0, 0.0]),
        );
        let bytes: &[u8] = bytemuck::cast_slice(&stage.effects);
        if bytes.len() as u64 > self.vertices.size() {
            self.vertices = vertex_buffer(device, (bytes.len() as u64).next_power_of_two());
        }
        queue.write_buffer(&self.vertices, 0, bytes);
        self.runs.clone_from(&stage.effect_runs);
        self.behind.clone_from(&stage.behind_runs);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass, masked: bool, behind: bool) {
        let Some(bind) = &self.bind else { return };
        let runs = if behind { &self.behind } else { &self.runs };
        for &(blend, start, end) in runs {
            let pipeline = match (blend, masked) {
                (FxBlend::Masked, true) => &self.masked,
                (FxBlend::Additive, false) => &self.additive,
                (FxBlend::Translucent, false) => &self.translucent,
                _ => continue,
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(start..end, 0..1);
        }
    }

    /// Masked effects (the Miss shards), with the opaque elements.
    pub(crate) fn draw_masked(&self, pass: &mut wgpu::RenderPass) {
        self.draw(pass, true, false);
        self.draw(pass, true, true);
    }

    /// Translucent effects before the guide lanes (the R-note hit).
    pub(crate) fn draw_behind(&self, pass: &mut wgpu::RenderPass) {
        self.draw(pass, false, true);
    }

    /// Additive and translucent effects, last in the translucency.
    pub(crate) fn draw_translucent(&self, pass: &mut wgpu::RenderPass) {
        self.draw(pass, false, false);
    }
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fx vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fx_shader_validates() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("fx.wgsl")).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn resample_keeps_flat_colour_and_gradients() {
        let img = Image {
            size: [1, 2],
            rgba: vec![0, 0, 0, 255, 200, 100, 50, 255],
        };
        let out = resample(&img, 4);
        assert_eq!(out.len(), 4 * 4 * 4);
        // Rows: clamped ends, a blend in between; columns identical.
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
        assert_eq!(&out[12 * 4..12 * 4 + 4], &[200, 100, 50, 255]);
        assert_eq!(out[4 * 4], out[4 * 4 + 12]);
        assert!(out[4 * 4] > 0 && out[4 * 4] < 200);
    }
}
