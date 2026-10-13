//! wgpu side of [`crate::bg`]: renders the background scene into the capture target (1080²,
//! 8-bit UNORM like the game's `cTRT_BackgroundEffect`: linear colour clamped to 0..1, reversed-Z
//! depth) with one pipeline per [`BgShader`] (`bg.wgsl`), then draws the capture as the play
//! view's emissive far quad inside the stage pass (`bg_quad.wgsl`).

use std::mem::offset_of;
use std::path::Path;
use std::sync::Arc;

use crate::bg::{BG_PARAMS, BgFrame, BgLibrary, BgShader, CAPTURE_SIZE};
use crate::particles::FxVertex;
use crate::sprites::{self, Image};
use crate::stage_gpu::{HDR_FORMAT, depth_state, upload};

const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MIN_BUFFER_BYTES: u64 = 64 * 1024;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniform {
    clock: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    p: [[f32; 4]; BG_PARAMS],
}

#[derive(Debug)]
struct Loaded {
    lib: Arc<BgLibrary>,
    /// Per library material: its shader and bind group (`None`: not drawn).
    materials: Vec<Option<(BgShader, wgpu::BindGroup)>>,
}

#[derive(Debug)]
pub(crate) struct BgLayer {
    poly: wgpu::RenderPipeline,
    rings: wgpu::RenderPipeline,
    dots: wgpu::RenderPipeline,
    wall: wgpu::RenderPipeline,
    cubes: wgpu::RenderPipeline,
    triangles: wgpu::RenderPipeline,
    material_layout: wgpu::BindGroupLayout,
    frame: wgpu::Buffer,
    frame_bind: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    /// The capture target (`CAPTURE_SIZE`², linear colour 0..1).
    capture: wgpu::Texture,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    quad: wgpu::RenderPipeline,
    quad_bind: wgpu::BindGroup,
    loaded: Option<Loaded>,
    runs: Vec<(u32, u32, u32)>,
}

impl BgLayer {
    /// `samples`: sample count of the stage pass that [`BgLayer::draw_quad`] draws into.
    pub(crate) fn new(device: &wgpu::Device, samples: u32) -> Self {
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: fragment,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: fragment,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bg frame"),
            entries: &[uniform(0, both)],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bg material"),
            entries: &[uniform(0, both), tex(1), tex(2), tex(3), tex(4), sampler(5)],
        });
        let module = device.create_shader_module(wgpu::include_wgsl!("bg.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg"),
            bind_group_layouts: &[&frame_layout, &material_layout],
            push_constant_ranges: &[],
        });
        // The fields of FxVertex the shaders use: position, UV, colour, extra, origin.
        let attribute = |shader_location, format, offset: usize| wgpu::VertexAttribute {
            format,
            offset: offset as u64,
            shader_location,
        };
        let attributes = [
            attribute(0, wgpu::VertexFormat::Float32x3, offset_of!(FxVertex, pos)),
            attribute(1, wgpu::VertexFormat::Float32x2, offset_of!(FxVertex, uv)),
            attribute(
                2,
                wgpu::VertexFormat::Float32x4,
                offset_of!(FxVertex, color),
            ),
            attribute(
                3,
                wgpu::VertexFormat::Float32x4,
                offset_of!(FxVertex, extra),
            ),
            attribute(
                4,
                wgpu::VertexFormat::Float32x3,
                offset_of!(FxVertex, origin),
            ),
        ];
        let pipeline = |vs: &str, entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: size_of::<FxVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    }],
                },
                // Captured: no culling (sprites and the mesh particles).
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::GreaterEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(CAPTURE_FORMAT.into())],
                }),
                multiview: None,
                cache: None,
            })
        };
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg frame"),
            size: size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg frame"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame.as_entire_binding(),
            }],
        });
        let target = |label, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: CAPTURE_SIZE,
                    height: CAPTURE_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let capture = target(
            "bg capture",
            CAPTURE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = target(
            "bg depth",
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .create_view(&wgpu::TextureViewDescriptor::default());
        let color = capture.create_view(&wgpu::TextureViewDescriptor::default());

        let quad_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bg quad"),
            entries: &[tex(0), sampler(1)],
        });
        let quad_module = device.create_shader_module(wgpu::include_wgsl!("bg_quad.wgsl"));
        let quad_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg quad"),
            bind_group_layouts: &[&quad_layout],
            push_constant_ranges: &[],
        });
        let quad = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg quad"),
            layout: Some(&quad_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &quad_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: Some(depth_state(true)),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &quad_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            multiview: None,
            cache: None,
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bg quad"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let quad_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg quad"),
            layout: &quad_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&linear),
                },
            ],
        });
        Self {
            poly: pipeline("vs_main", "fs_poly"),
            rings: pipeline("vs_main", "fs_rings"),
            dots: pipeline("vs_main", "fs_dots"),
            wall: pipeline("vs_main", "fs_wall"),
            cubes: pipeline("vs_cubes", "fs_cubes"),
            triangles: pipeline("vs_main", "fs_triangles"),
            material_layout,
            frame,
            frame_bind,
            vertices: vertex_buffer(device, MIN_BUFFER_BYTES),
            capture,
            color,
            depth,
            quad,
            quad_bind,
            loaded: None,
            runs: Vec::new(),
        }
    }

    /// Loads `data_dir`'s background library and its textures (`data_dir/ui/Vfx/Textures`);
    /// on failure the background stays off (logged).
    pub(crate) fn load(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data_dir: &Path) {
        let lib = match BgLibrary::load(data_dir) {
            Ok(lib) => Arc::new(lib),
            Err(e) => {
                log::warn!("background scene off: {e}");
                return;
            }
        };
        let dir = data_dir.join("ui").join("Vfx").join("Textures");
        let views: Vec<wgpu::TextureView> = lib
            .textures
            .iter()
            .map(|(name, srgb)| {
                let img =
                    sprites::read_png(&dir.join(name).with_extension("png")).unwrap_or_else(|e| {
                        log::warn!("background texture: {e}; drawn white");
                        white()
                    });
                upload(device, queue, name, &img, *srgb)
            })
            .collect();
        let white_view = upload(device, queue, "bg white", &white(), false);
        let wrap = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bg wrap"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let materials = lib
            .materials
            .iter()
            .map(|m| {
                let m = m.as_ref()?;
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("bg material"),
                    size: size_of::<MaterialUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(
                    &buffer,
                    0,
                    bytemuck::bytes_of(&MaterialUniform { p: m.params }),
                );
                let view = |slot: usize| {
                    m.textures[slot]
                        .and_then(|i| views.get(i as usize))
                        .unwrap_or(&white_view)
                };
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bg material"),
                    layout: &self.material_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(view(0)),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(view(1)),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(view(2)),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(view(3)),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::Sampler(&wrap),
                        },
                    ],
                });
                Some((m.shader, bind))
            })
            .collect();
        self.loaded = Some(Loaded { lib, materials });
    }

    /// The loaded library (give it to [`crate::bg::Background::new`]).
    /// The capture target: the last rendered background.
    pub(crate) fn capture(&self) -> &wgpu::Texture {
        &self.capture
    }

    pub(crate) fn library(&self) -> Option<Arc<BgLibrary>> {
        self.loaded.as_ref().map(|l| Arc::clone(&l.lib))
    }

    /// Renders `frame` into the capture target (cleared to black).
    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &BgFrame,
    ) {
        let uniform = FrameUniform {
            clock: [frame.time, frame.bar, frame.beat, 0.0],
        };
        queue.write_buffer(&self.frame, 0, bytemuck::bytes_of(&uniform));
        let bytes: &[u8] = bytemuck::cast_slice(&frame.vertices);
        if bytes.len() as u64 > self.vertices.size() {
            self.vertices = vertex_buffer(device, (bytes.len() as u64).next_power_of_two());
        }
        queue.write_buffer(&self.vertices, 0, bytes);
        self.runs.clone_from(&frame.runs);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bg capture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let Some(loaded) = &self.loaded else { return };
        pass.set_bind_group(0, &self.frame_bind, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        for &(material, start, end) in &self.runs {
            let Some(Some((shader, bind))) = loaded.materials.get(material as usize) else {
                continue;
            };
            pass.set_pipeline(match shader {
                BgShader::Poly => &self.poly,
                BgShader::Rings => &self.rings,
                BgShader::Dots => &self.dots,
                BgShader::Wall => &self.wall,
                BgShader::Cubes => &self.cubes,
                BgShader::Triangles => &self.triangles,
            });
            pass.set_bind_group(1, bind, &[]);
            pass.draw(start..end, 0..1);
        }
    }

    /// Draws the last capture as the far quad of the stage pass.
    pub(crate) fn draw_quad(&self, pass: &mut wgpu::RenderPass) {
        pass.set_pipeline(&self.quad);
        pass.set_bind_group(0, &self.quad_bind, &[]);
        pass.draw(0..4, 0..1);
    }
}

fn white() -> Image {
    Image {
        size: [1, 1],
        rgba: vec![255; 4],
    }
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bg vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
