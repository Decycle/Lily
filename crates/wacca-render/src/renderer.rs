//! wgpu side: the 3D stage pass (movie, 3D notes, [`crate::stage`] elements; linear HDR, see
//! `stage_gpu`) composed with the game's final 1/2.2 power, then one pipeline drawing the
//! [`build_scene`](crate::build_scene) triangle list over it with 4x MSAA, sampling one
//! premultiplied sprite atlas (premultiplied-alpha blending; additive vertices write alpha 0).

use std::path::Path;

use crate::layout::{ATLAS_SIZE, Vertex, playfield_viewport};
use crate::notes_gpu::NoteLayer;
use crate::notes3d::{MeshVertex, NoteMeshes};
use crate::sprites::{self, Sprite, SpriteSheet, TextSprite};
use crate::stage::Stage;
use crate::stage_gpu::StageLayer;
use crate::style::VIEW_EXTENT;
use crate::video::VideoLayer;

const SAMPLE_COUNT: u32 = 4;
const MIN_BUFFER_BYTES: u64 = 64 * 1024;

#[derive(Debug)]
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    atlas: wgpu::Texture,
    atlas_group: wgpu::BindGroup,
    sheet: SpriteSheet,
    /// Sprites from [`Renderer::load_sprites`], kept to re-pack with new rim texts.
    base: Vec<(Sprite, sprites::Image)>,
    /// Font of the rim texts.
    text_font: Option<ab_glyph::FontVec>,
    vertices: wgpu::Buffer,
    /// Multisampled colour target and its size, recreated when the output size changes.
    msaa: Option<(wgpu::TextureView, [u32; 2])>,
    /// Background movie, drawn under the playfield.
    video: VideoLayer,
    /// 3D notes (off until meshes and their textures load).
    notes: NoteLayer,
    /// Guide lanes, hold ribbons, chevrons; the HDR pass and its composition.
    stage: StageLayer,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("playfield.wgsl"));
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("playfield atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("playfield"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });
        let linearize = if format.is_srgb() { 1.0 } else { 0.0 };
        let constants = [
            ("VIEW_EXTENT", VIEW_EXTENT as f64),
            ("LINEARIZE", linearize),
        ];
        let options = wgpu::PipelineCompilationOptions {
            constants: &constants,
            ..Default::default()
        };
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x2
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("playfield"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: options.clone(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: SAMPLE_COUNT,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: options,
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("playfield atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("playfield atlas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("playfield atlas"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let me = Self {
            device: device.clone(),
            queue: queue.clone(),
            format,
            pipeline,
            atlas,
            atlas_group,
            sheet: SpriteSheet::default(),
            base: Vec::new(),
            text_font: None,
            vertices: vertex_buffer(device, MIN_BUFFER_BYTES),
            msaa: None,
            video: VideoLayer::new(device, SAMPLE_COUNT),
            notes: NoteLayer::new(device, SAMPLE_COUNT),
            stage: StageLayer::new(device, queue, format, SAMPLE_COUNT),
        };
        // The white block alone: flat colours work without any sprites.
        me.upload(sprites::pack(Vec::new()).1);
        me
    }

    /// Load the playfield sprites from `ui_dir` (the `data/ui` export). Missing textures are
    /// logged and drawn in flat colours. Combo digits come from the SHINBI font next to it
    /// (`data/fonts/SHINBI.otf`, 06 §3).
    /// The rim texts use the UI font `data/fonts/DF-KaGei-W6.ttc`. The note meshes come from
    /// `data/meshes` (`wacca-convert --meshes`) with their textures from `ui_dir/Vfx/Textures`;
    /// without them notes stay 2D.
    pub fn load_sprites(&mut self, ui_dir: &Path) {
        self.stage.load(&self.device, &self.queue, ui_dir);
        self.notes.load(
            &self.device,
            &self.queue,
            &ui_dir.with_file_name("meshes"),
            ui_dir,
        );
        let fonts = ui_dir.with_file_name("fonts");
        let mut all = sprites::load_sources(ui_dir);
        all.extend(sprites::load_digits(&fonts.join("SHINBI.otf")));
        self.text_font = sprites::load_font(&fonts.join("DF-KaGei-W6.ttc"));
        if let Some(img) = self
            .text_font
            .as_ref()
            .and_then(|f| sprites::bake_text(f, "SCORE"))
        {
            all.push((Sprite::Text(TextSprite::ScoreLabel), img));
        }
        self.base = all;
        self.repack(Vec::new());
    }

    /// Bake the play screen's rim texts (difficulty/level and song title) into the atlas.
    pub fn set_rim_texts(&mut self, difficulty: &str, title: &str) {
        let Some(font) = &self.text_font else { return };
        let texts = [
            (TextSprite::Difficulty, difficulty),
            (TextSprite::Title, title),
        ]
        .into_iter()
        .filter_map(|(s, t)| Some((Sprite::Text(s), sprites::bake_text(font, t)?)))
        .collect();
        self.repack(texts);
    }

    fn repack(&mut self, extra: Vec<(Sprite, sprites::Image)>) {
        let all = self.base.iter().cloned().chain(extra).collect();
        let (sheet, placed) = sprites::pack(all);
        self.upload(placed);
        self.sheet = sheet;
    }

    /// Note meshes for [`build_mesh_notes`](crate::build_mesh_notes), when loaded (then build the
    /// scene with [`SceneOptions::mesh_notes`](crate::SceneOptions) and draw with
    /// [`Renderer::render_with_notes`]).
    pub fn note_meshes(&self) -> Option<&NoteMeshes> {
        self.notes.meshes()
    }

    /// Sprites available to [`build_scene`](crate::build_scene).
    pub fn sprites(&self) -> &SpriteSheet {
        &self.sheet
    }

    fn upload(&self, placed: Vec<sprites::Placed>) {
        for p in placed {
            let [w, h] = p.image.size;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: p.at[0],
                        y: p.at[1],
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &p.image.rgba,
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
    }

    /// New background-movie frame (`yuv420p` planes Y, U, V of a `size` frame); shown until replaced.
    pub fn set_video_frame(&mut self, size: [u32; 2], planes: [&[u8]; 3]) {
        self.video.upload(&self.device, &self.queue, size, planes);
    }

    /// Show or hide the background movie (behind the playfield, filling its square viewport).
    pub fn show_video(&mut self, visible: bool) {
        self.video.visible = visible;
    }

    /// Clears `target` (size `target_size` px) to black and draws `scene` in the centred 9:16 area
    /// (over the movie, when shown).
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_size: [u32; 2],
        scene: &[Vertex],
    ) {
        self.render_with_notes(encoder, target, target_size, scene, &[], &Stage::default());
    }

    /// [`Renderer::render`] with the 3D play view under `scene`: the movie, the 3D `notes` and
    /// the `stage` elements, rendered like the game's main view (linear, depth-tested) and
    /// composed with its final `pow(x, 1/2.2)`.
    pub fn render_with_notes(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_size: [u32; 2],
        scene: &[Vertex],
        notes: &[MeshVertex],
        stage: &Stage,
    ) {
        if target_size.contains(&0) {
            return;
        }
        let vp = playfield_viewport(target_size[0] as f32, target_size[1] as f32);
        let staged = self.video.shown() || !notes.is_empty() || !stage.is_empty();
        if staged {
            self.notes.prepare(&self.device, &self.queue, notes);
            self.stage.prepare(&self.device, &self.queue, stage, vp);
            let mut pass = self.stage.begin(&self.device, encoder, target_size, vp);
            self.video.draw(&mut pass);
            self.notes.draw(&mut pass, notes.len());
            self.stage.draw_opaque(&mut pass);
            self.stage.draw_translucent(&mut pass);
        }
        let bytes: &[u8] = bytemuck::cast_slice(scene);
        if bytes.len() as u64 > self.vertices.size() {
            let size = (bytes.len() as u64).next_power_of_two();
            log::debug!("growing vertex buffer to {size} bytes");
            self.vertices = vertex_buffer(&self.device, size);
        }
        self.queue.write_buffer(&self.vertices, 0, bytes);

        let msaa = match &self.msaa {
            Some((view, size)) if *size == target_size => view,
            _ => {
                &self
                    .msaa
                    .insert((
                        msaa_view(&self.device, self.format, target_size),
                        target_size,
                    ))
                    .0
            }
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("playfield"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: msaa,
                depth_slice: None,
                resolve_target: Some(target),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            ..Default::default()
        });
        pass.set_viewport(vp.x, vp.y, vp.w, vp.h, 0.0, 1.0);
        if staged {
            self.stage.compose(&mut pass);
        }
        if scene.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.atlas_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..bytes.len() as u64));
        pass.draw(0..scene.len() as u32, 0..1);
    }
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("playfield vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn msaa_view(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    [width, height]: [u32; 2],
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("playfield msaa"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
