//! wgpu side: one pipeline drawing the [`build_scene`](crate::build_scene) triangle list with 4x MSAA.

use crate::layout::{Vertex, playfield_viewport};
use crate::style::VIEW_EXTENT;

const SAMPLE_COUNT: u32 = 4;
const MIN_BUFFER_BYTES: u64 = 64 * 1024;

#[derive(Debug)]
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    /// Multisampled colour target and its size, recreated when the output size changes.
    msaa: Option<(wgpu::TextureView, [u32; 2])>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("playfield.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("playfield"),
            bind_group_layouts: &[],
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
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        Self {
            device: device.clone(),
            queue: queue.clone(),
            format,
            pipeline,
            vertices: vertex_buffer(device, MIN_BUFFER_BYTES),
            msaa: None,
        }
    }

    /// Clears `target` (size `target_size` px) to black and draws `scene` in the centred 9:16 area.
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_size: [u32; 2],
        scene: &[Vertex],
    ) {
        if target_size.contains(&0) {
            return;
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
        if scene.is_empty() {
            return;
        }
        let vp = playfield_viewport(target_size[0] as f32, target_size[1] as f32);
        pass.set_viewport(vp.x, vp.y, vp.w, vp.h, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline);
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
