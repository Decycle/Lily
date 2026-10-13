//! wgpu side of the 3D stage: the play view rendered like the game's main view (doc 11 §2.2,
//! §3.0) into a linear HDR target with reversed-Z depth — movie quad, note bodies, chevrons
//! (opaque), then guide lanes and hold ribbons (translucent, depth-tested, no depth write) — and
//! composed onto the output with the game's final `pow(x, 1/2.2)` ([`StageLayer::compose`]).
//! Pipelines for [`crate::stage`] live here; the note and movie layers draw into the same pass.

use std::path::Path;

use crate::layout::Rect;
use crate::notes3d::MeshVertex;
use crate::sprites::{self, Image};
use crate::stage::Stage;

/// Linear scene colour (the game's main view is R16G16B16A16F).
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MIN_BUFFER_BYTES: u64 = 256 * 1024;

/// Guide-lane fade `(PercentageAlpha, WidthGradation)` from `RhythmGameAppearanceTable`
/// (`GuideLaneBrightnessParamAlpha*`): with a movie (captured in the material) and without.
const LANE_FADE_MOVIE: [f32; 2] = [290.0, 6.0];
// UNVERIFIED: the normal (no movie) values are the table's, not seen in a capture.
const LANE_FADE_NORMAL: [f32; 2] = [600.0, 3.0];

/// Stage textures under `data/ui/Vfx/Textures`: file stem and whether the texture is sRGB (the
/// cooked `SRGB` flag; only `vT_SnapNote00_00_m` has it off). Binding = index + 1.
const TEXTURES: [(&str, bool); 6] = [
    ("vT_JudgeLine00_00_m", true),
    ("vT_JudgeColor01_00_m", true),
    ("vT_HoldGrd00_00_m", true),
    ("vT_SlideNote00_00_m", true),
    ("vT_SlideMsk00_00_m", true),
    ("vT_SnapNote00_00_m", false),
];

/// Depth state of the stage pass: reversed Z (`GreaterEqual`, cleared to 0).
pub(crate) fn depth_state(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: write,
        depth_compare: wgpu::CompareFunction::GreaterEqual,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    viewport: [f32; 4],
    params: [f32; 4],
}

#[derive(Debug)]
struct Targets {
    size: [u32; 2],
    color: wgpu::TextureView,
    resolved: wgpu::TextureView,
    depth: wgpu::TextureView,
    compose: wgpu::BindGroup,
}

/// Vertex ranges of the uploaded [`Stage`].
#[derive(Clone, Copy, Debug, Default)]
struct Ranges {
    chevrons: (u32, u32),
    lanes: (u32, u32),
    holds: (u32, u32),
}

#[derive(Debug)]
pub(crate) struct StageLayer {
    lanes: wgpu::RenderPipeline,
    holds: wgpu::RenderPipeline,
    chevrons: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
    view: wgpu::Buffer,
    vertices: wgpu::Buffer,
    ranges: Ranges,
    compose: wgpu::RenderPipeline,
    compose_layout: wgpu::BindGroupLayout,
    targets: Option<Targets>,
    samples: u32,
}

impl StageLayer {
    /// `format`/`samples`: the output pass that [`StageLayer::compose`] draws into (the stage
    /// pass uses the same sample count).
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
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
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        entries.extend((1..=TEXTURES.len() as u32).map(tex));
        entries.extend([sampler(7), sampler(8)]);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stage"),
            entries: &entries,
        });
        let module = device.create_shader_module(wgpu::include_wgsl!("stage.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stage"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Uint32];
        let pipeline = |label, entry, blend, write| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    }],
                },
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
        // Separate translucency, `SrcAlpha / InvSrcAlpha` (alpha: Zero / InvSrcAlpha).
        let alpha = Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        });
        let lanes = pipeline("guide lanes", "fs_lane", alpha, false);
        let holds = pipeline("hold ribbons", "fs_hold", alpha, false);
        let chevrons = pipeline("chevrons", "fs_chevron", None, true);

        let compose_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stage compose"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let compose_module = device.create_shader_module(wgpu::include_wgsl!("compose.wgsl"));
        let constants = [("SRGB_TARGET", if format.is_srgb() { 1.0 } else { 0.0 })];
        let options = wgpu::PipelineCompilationOptions {
            constants: &constants,
            ..Default::default()
        };
        let compose_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("stage compose"),
                bind_group_layouts: &[&compose_layout],
                push_constant_ranges: &[],
            });
        let compose = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stage compose"),
            layout: Some(&compose_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &compose_module,
                entry_point: Some("vs_main"),
                compilation_options: options.clone(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &compose_module,
                entry_point: Some("fs_main"),
                compilation_options: options,
                targets: &[Some(format.into())],
            }),
            multiview: None,
            cache: None,
        });
        let view = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stage view"),
            size: size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Until `load`: plain white textures (shapes visible, colours wrong).
        let white = || Image {
            size: [1, 1],
            rgba: vec![255; 4],
        };
        let views = TEXTURES.map(|(stem, srgb)| upload(device, queue, stem, &white(), srgb));
        let bind = bind_group(device, &layout, &view, &views);
        Self {
            lanes,
            holds,
            chevrons,
            layout,
            bind,
            view,
            vertices: vertex_buffer(device, MIN_BUFFER_BYTES),
            ranges: Ranges::default(),
            compose,
            compose_layout,
            targets: None,
            samples,
        }
    }

    /// Loads the stage textures from `ui_dir/Vfx/Textures`; missing ones stay white (logged).
    pub(crate) fn load(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, ui_dir: &Path) {
        let dir = ui_dir.join("Vfx").join("Textures");
        let views = TEXTURES.map(|(stem, srgb)| {
            let img =
                sprites::read_png(&dir.join(stem).with_extension("png")).unwrap_or_else(|e| {
                    log::warn!("stage texture: {e}; drawn white");
                    Image {
                        size: [1, 1],
                        rgba: vec![255; 4],
                    }
                });
            upload(device, queue, stem, &img, srgb)
        });
        self.bind = bind_group(device, &self.layout, &self.view, &views);
    }

    /// Uploads `stage` for a stage pass over viewport `vp` (target pixels).
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stage: &Stage,
        vp: Rect,
    ) {
        let [alpha, gradation] = if stage.movie {
            LANE_FADE_MOVIE
        } else {
            LANE_FADE_NORMAL
        };
        let uniform = ViewUniform {
            viewport: [vp.x, vp.y, 1.0 / vp.w, 1.0 / vp.h],
            params: [stage.time, stage.pulse, alpha, gradation],
        };
        queue.write_buffer(&self.view, 0, bytemuck::bytes_of(&uniform));
        let mut at = 0;
        let mut range = |len: usize| {
            let r = (at, at + len as u32);
            at = r.1;
            r
        };
        self.ranges = Ranges {
            chevrons: range(stage.chevrons.len()),
            lanes: range(stage.lanes.len()),
            holds: range(stage.holds.len()),
        };
        let all: Vec<MeshVertex> = [&stage.chevrons, &stage.lanes, &stage.holds]
            .into_iter()
            .flatten()
            .copied()
            .collect();
        let bytes: &[u8] = bytemuck::cast_slice(&all);
        if bytes.len() as u64 > self.vertices.size() {
            self.vertices = vertex_buffer(device, (bytes.len() as u64).next_power_of_two());
        }
        queue.write_buffer(&self.vertices, 0, bytes);
    }

    /// Starts the stage pass over a `size` target (cleared: black, depth 0), viewport `vp`.
    pub(crate) fn begin<'e>(
        &mut self,
        device: &wgpu::Device,
        encoder: &'e mut wgpu::CommandEncoder,
        size: [u32; 2],
        vp: Rect,
    ) -> wgpu::RenderPass<'e> {
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            self.targets = Some(self.create_targets(device, size));
        }
        let Some(t) = &self.targets else {
            unreachable!("targets created above")
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stage"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &t.color,
                depth_slice: None,
                resolve_target: Some(&t.resolved),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &t.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_viewport(vp.x, vp.y, vp.w, vp.h, 0.0, 1.0);
        pass
    }

    fn draw(&self, pass: &mut wgpu::RenderPass, pipeline: &wgpu::RenderPipeline, r: (u32, u32)) {
        if r.0 == r.1 {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(r.0..r.1, 0..1);
    }

    /// The opaque (masked) stage elements: chevrons and snap arrows.
    pub(crate) fn draw_opaque(&self, pass: &mut wgpu::RenderPass) {
        self.draw(pass, &self.chevrons, self.ranges.chevrons);
    }

    /// The translucent elements, in the game's order: guide lanes, then hold ribbons.
    pub(crate) fn draw_translucent(&self, pass: &mut wgpu::RenderPass) {
        self.draw(pass, &self.lanes, self.ranges.lanes);
        self.draw(pass, &self.holds, self.ranges.holds);
    }

    /// Draws the last stage pass's result, tonemapped, over the current viewport.
    pub(crate) fn compose(&self, pass: &mut wgpu::RenderPass) {
        if let Some(t) = &self.targets {
            pass.set_pipeline(&self.compose);
            pass.set_bind_group(0, &t.compose, &[]);
            pass.draw(0..4, 0..1);
        }
    }

    fn create_targets(&self, device: &wgpu::Device, [width, height]: [u32; 2]) -> Targets {
        let texture = |label, format, samples| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let resolved = texture("stage resolved", HDR_FORMAT, 1);
        let compose = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stage compose"),
            layout: &self.compose_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&resolved),
            }],
        });
        Targets {
            size: [width, height],
            color: texture("stage colour", HDR_FORMAT, self.samples),
            resolved,
            depth: texture("stage depth", DEPTH_FORMAT, self.samples),
            compose,
        }
    }
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::Buffer,
    textures: &[wgpu::TextureView],
) -> wgpu::BindGroup {
    let sampler = |label, v| {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: if v == wgpu::AddressMode::MirrorRepeat {
                wgpu::AddressMode::ClampToEdge
            } else {
                wgpu::AddressMode::Repeat
            },
            address_mode_v: v,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        })
    };
    let wrap = sampler("stage wrap", wgpu::AddressMode::Repeat);
    // The hold ribbon's sampler: clamp u, mirror v.
    let hold = sampler("stage hold", wgpu::AddressMode::MirrorRepeat);
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: view.as_entire_binding(),
    }];
    entries.extend(
        textures
            .iter()
            .enumerate()
            .map(|(i, t)| wgpu::BindGroupEntry {
                binding: 1 + i as u32,
                resource: wgpu::BindingResource::TextureView(t),
            }),
    );
    entries.extend([
        wgpu::BindGroupEntry {
            binding: 7,
            resource: wgpu::BindingResource::Sampler(&wrap),
        },
        wgpu::BindGroupEntry {
            binding: 8,
            resource: wgpu::BindingResource::Sampler(&hold),
        },
    ]);
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("stage"),
        layout,
        entries: &entries,
    })
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("stage vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// One-level texture (the captured stage textures have no mips).
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    img: &Image,
    srgb: bool,
) -> wgpu::TextureView {
    let [width, height] = img.size;
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
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
    queue.write_texture(
        texture.as_image_copy(),
        &img.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
