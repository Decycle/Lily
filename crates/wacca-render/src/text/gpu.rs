//! wgpu side of the overlay: glyph atlas texture, image textures and one alpha-blended pipeline.

use std::collections::HashMap;

use super::atlas::Shelves;
use super::fonts::Fonts;
use super::layout::{Align, align_x, corners, fit, measure};
use crate::layout::Rect;
use crate::style::Rgba;

const ATLAS_SIZE: u32 = 2048;
const MIN_BUFFER_BYTES: u64 = 64 * 1024;
/// Side of the opaque white block used for filled rectangles.
const WHITE: u32 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct OverlayVertex {
    /// Clip space.
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

/// A rasterized glyph in the atlas: pixel offset from the pen (baseline) and atlas rectangle.
#[derive(Clone, Copy, Debug)]
struct Placed {
    offset: [f32; 2],
    size: [f32; 2],
    uv: [f32; 4],
}

/// `(face, glyph id, pixel size)`.
type GlyphKey = (usize, u16, u32);

/// Consecutive vertices drawn with one texture (`None` = the glyph atlas).
#[derive(Debug)]
struct Batch {
    image: Option<String>,
    start: u32,
    end: u32,
}

/// Pixel `p` of a `size` target to clip space (y up).
fn to_clip(p: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [p[0] / size[0] * 2.0 - 1.0, 1.0 - p[1] / size[1] * 2.0]
}

/// Draws text, filled rectangles and images in window pixels (origin top-left), queued between
/// [`TextRenderer::begin`] and [`TextRenderer::render`], on top of what the target holds.
#[derive(Debug)]
pub struct TextRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    atlas: wgpu::Texture,
    atlas_group: wgpu::BindGroup,
    shelves: Shelves,
    /// Atlas uv of the white block's centre.
    white_uv: [f32; 2],
    fonts: Fonts,
    /// `None`: glyph without pixels (space).
    glyphs: HashMap<GlyphKey, Option<Placed>>,
    images: HashMap<String, wgpu::BindGroup>,
    vertices: Vec<OverlayVertex>,
    batches: Vec<Batch>,
    buffer: wgpu::Buffer,
    size: [f32; 2],
}

impl TextRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        fonts: Fonts,
    ) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("text.wgsl"));
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay"),
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
            label: Some("overlay"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });
        let constants = [("LINEARIZE", if format.is_srgb() { 1.0 } else { 0.0 })];
        let options = wgpu::PipelineCompilationOptions {
            constants: &constants,
            ..Default::default()
        };
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: options.clone(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<OverlayVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: options,
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("overlay"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas = texture(device, "glyph atlas", [ATLAS_SIZE, ATLAS_SIZE]);
        let atlas_group = bind_group(device, &bind_layout, &sampler, &atlas);
        let mut me = Self {
            device: device.clone(),
            queue: queue.clone(),
            pipeline,
            bind_layout,
            sampler,
            atlas,
            atlas_group,
            shelves: Shelves::new([ATLAS_SIZE, ATLAS_SIZE]),
            white_uv: [0.0; 2],
            fonts,
            glyphs: HashMap::new(),
            images: HashMap::new(),
            vertices: Vec::new(),
            batches: Vec::new(),
            buffer: vertex_buffer(device, MIN_BUFFER_BYTES),
            size: [1.0, 1.0],
        };
        me.reset_atlas();
        me
    }

    pub fn fonts(&self) -> &Fonts {
        &self.fonts
    }

    /// Width of `text` at pixel height `px`.
    pub fn measure(&self, text: &str, px: f32) -> f32 {
        measure(text, |c| self.fonts.advance(c, px))
    }

    /// Empty the atlas and put the white block back.
    fn reset_atlas(&mut self) {
        self.shelves.clear();
        self.glyphs.clear();
        let at = self.shelves.alloc(WHITE, WHITE).unwrap_or([0, 0]);
        self.upload(at, [WHITE, WHITE], &[255; (WHITE * WHITE * 4) as usize]);
        let half = WHITE as f32 / 2.0;
        self.white_uv = [
            (at[0] as f32 + half) / ATLAS_SIZE as f32,
            (at[1] as f32 + half) / ATLAS_SIZE as f32,
        ];
    }

    fn upload(&self, at: [u32; 2], [w, h]: [u32; 2], rgba: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: at[0],
                    y: at[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
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

    /// Cached glyph, rasterizing it into the atlas on first use.
    fn glyph(&mut self, key: GlyphKey) -> Option<Placed> {
        if let Some(p) = self.glyphs.get(&key) {
            return *p;
        }
        let placed = self.rasterize(key);
        self.glyphs.insert(key, placed);
        placed
    }

    fn rasterize(&mut self, (face, id, px): GlyphKey) -> Option<Placed> {
        let outline = self.fonts.outline((face, id), px as f32)?;
        let b = outline.px_bounds();
        let (w, h) = (b.width() as u32, b.height() as u32);
        if w == 0 || h == 0 {
            return None;
        }
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        outline.draw(|x, y, c| {
            if let Some(a) = rgba.get_mut(((y * w + x) * 4 + 3) as usize) {
                *a = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        let at = match self.shelves.alloc(w, h) {
            Some(at) => at,
            None => {
                log::warn!("glyph atlas full; clearing it");
                self.reset_atlas();
                self.shelves.alloc(w, h)?
            }
        };
        self.upload(at, [w, h], &rgba);
        let s = ATLAS_SIZE as f32;
        Some(Placed {
            offset: [b.min.x, b.min.y],
            size: [w as f32, h as f32],
            uv: [
                at[0] as f32 / s,
                at[1] as f32 / s,
                (at[0] + w) as f32 / s,
                (at[1] + h) as f32 / s,
            ],
        })
    }

    /// Start a frame for a target of `size` pixels.
    pub fn begin(&mut self, size: [u32; 2]) {
        self.size = [size[0].max(1) as f32, size[1].max(1) as f32];
        self.vertices.clear();
        self.batches.clear();
    }

    fn quad(&mut self, image: Option<&str>, r: Rect, uv: [f32; 4], color: Rgba) {
        self.quad_turned(image, r, 0.0, uv, color);
    }

    fn quad_turned(&mut self, image: Option<&str>, r: Rect, angle: f32, uv: [f32; 4], color: Rgba) {
        let [a, b, c, d] = corners([r.x, r.y, r.w, r.h], angle);
        let corners = [
            (a, [uv[0], uv[1]]),
            (b, [uv[2], uv[1]]),
            (c, [uv[2], uv[3]]),
            (d, [uv[0], uv[3]]),
        ];
        let start = self.vertices.len() as u32;
        for i in [0, 1, 2, 0, 2, 3] {
            let (p, uv) = corners[i];
            self.vertices.push(OverlayVertex {
                pos: to_clip(p, self.size),
                uv,
                color,
            });
        }
        let end = self.vertices.len() as u32;
        match self.batches.last_mut() {
            Some(b) if b.image.as_deref() == image => b.end = end,
            _ => self.batches.push(Batch {
                image: image.map(str::to_string),
                start,
                end,
            }),
        }
    }

    pub fn fill(&mut self, rect: Rect, color: Rgba) {
        let [u, v] = self.white_uv;
        self.quad(None, rect, [u, v, u, v], color);
    }

    /// One line of `text` at pixel height `px`. `at` = (anchor x per `align`, top of the line);
    /// with `max_width` the line is cut with an ellipsis to fit.
    pub fn text(
        &mut self,
        text: &str,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
        max_width: Option<f32>,
    ) {
        let px = px.round().max(1.0);
        let fonts = &self.fonts;
        let advance = |c| fonts.advance(c, px);
        let line = match max_width {
            Some(max) => fit(text, max, advance).into_owned(),
            None => text.to_string(),
        };
        for (r, uv) in self.glyph_quads(&line, at, px, align) {
            self.quad(None, r, uv, color);
        }
    }

    /// Like [`TextRenderer::text`] (without truncation), turned by `angle` radians (clockwise)
    /// about `at`.
    pub fn text_turned(
        &mut self,
        text: &str,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
        angle: f32,
    ) {
        let px = px.round().max(1.0);
        let (sin, cos) = angle.sin_cos();
        for (r, uv) in self.glyph_quads(text, at, px, align) {
            let (dx, dy) = (r.x + r.w / 2.0 - at[0], r.y + r.h / 2.0 - at[1]);
            let c = [at[0] + dx * cos - dy * sin, at[1] + dx * sin + dy * cos];
            let turned = Rect {
                x: c[0] - r.w / 2.0,
                y: c[1] - r.h / 2.0,
                ..r
            };
            self.quad_turned(None, turned, angle, uv, color);
        }
    }

    /// Screen rectangle and atlas uv of each visible glyph of one line (`px` already rounded).
    fn glyph_quads(
        &mut self,
        line: &str,
        at: [f32; 2],
        px: f32,
        align: Align,
    ) -> Vec<(Rect, [f32; 4])> {
        let fonts = &self.fonts;
        let advance = |c| fonts.advance(c, px);
        let mut x = align_x(at[0], measure(line, advance), align);
        let baseline = (at[1] + fonts.ascent(px)).round();
        let glyphs: Vec<_> = line
            .chars()
            .map(|c| {
                let pen = x;
                x += fonts.advance(c, px);
                let (face, id) = fonts.glyph_key(c);
                ((face, id, px as u32), pen.round())
            })
            .collect();
        glyphs
            .into_iter()
            .filter_map(|(key, pen)| {
                let g = self.glyph(key)?;
                let r = Rect {
                    x: pen + g.offset[0],
                    y: baseline + g.offset[1],
                    w: g.size[0],
                    h: g.size[1],
                };
                Some((r, g.uv))
            })
            .collect()
    }

    /// Register (or replace) an RGBA8 image under `key`. `rgba.len()` must be `w * h * 4`.
    pub fn set_image(&mut self, key: &str, [w, h]: [u32; 2], rgba: &[u8]) {
        if w == 0 || h == 0 || rgba.len() != (w * h * 4) as usize {
            log::warn!("image {key}: bad size {w}x{h} for {} bytes", rgba.len());
            return;
        }
        let tex = texture(&self.device, key, [w, h]);
        self.queue.write_texture(
            tex.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            tex.size(),
        );
        let group = bind_group(&self.device, &self.bind_layout, &self.sampler, &tex);
        self.images.insert(key.to_string(), group);
    }

    pub fn has_image(&self, key: &str) -> bool {
        self.images.contains_key(key)
    }

    /// Draw image `key` stretched over `rect`, multiplied by `tint`. Unknown keys draw nothing.
    pub fn image(&mut self, key: &str, rect: Rect, tint: Rgba) {
        if self.has_image(key) {
            self.quad(Some(key), rect, [0.0, 0.0, 1.0, 1.0], tint);
        }
    }

    /// Like [`TextRenderer::image`] for the texture region `uv` (`[u0, v0, u1, v1]`; swap the
    /// ends to mirror), turned by `angle` radians (clockwise) about the centre of `rect`.
    pub fn image_region(&mut self, key: &str, rect: Rect, uv: [f32; 4], angle: f32, tint: Rgba) {
        if self.has_image(key) {
            self.quad_turned(Some(key), rect, angle, uv, tint);
        }
    }

    /// Draw everything queued since [`TextRenderer::begin`] onto `target` (kept, not cleared).
    pub fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if self.vertices.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&self.vertices);
        if bytes.len() as u64 > self.buffer.size() {
            self.buffer = vertex_buffer(&self.device, (bytes.len() as u64).next_power_of_two());
        }
        self.queue.write_buffer(&self.buffer, 0, bytes);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
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
        pass.set_vertex_buffer(0, self.buffer.slice(..bytes.len() as u64));
        for b in &self.batches {
            let group = match &b.image {
                None => &self.atlas_group,
                Some(k) => match self.images.get(k) {
                    Some(g) => g,
                    None => continue,
                },
            };
            pass.set_bind_group(0, group, &[]);
            pass.draw(b.start..b.end, 0..1);
        }
    }
}

fn texture(device: &wgpu::Device, label: &str, [width, height]: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    tex: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("overlay"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("overlay vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_map_to_clip_space_y_up() {
        assert_eq!(to_clip([0.0, 0.0], [200.0, 100.0]), [-1.0, 1.0]);
        assert_eq!(to_clip([200.0, 100.0], [200.0, 100.0]), [1.0, -1.0]);
        assert_eq!(to_clip([100.0, 50.0], [200.0, 100.0]), [0.0, 0.0]);
    }
}
