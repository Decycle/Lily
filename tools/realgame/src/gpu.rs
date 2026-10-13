//! Headless render of our real playfield renderer ([`wacca_render::Renderer`], with sprites) into a square RGBA
//! texture read back to the CPU, as in the `playfield` example's `WACCA_SHOT` path.

use crate::frame::Rgb;
use std::path::Path;
use wacca_render::{Renderer, SpriteSheet, Vertex};

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("no GPU adapter: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("no GPU device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("readback: {0}")]
    Poll(#[from] wgpu::PollError),
}

pub struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    target: wgpu::Texture,
    buffer: wgpu::Buffer,
    side: u32,
}

impl std::fmt::Debug for Offscreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Offscreen")
            .field("side", &self.side)
            .finish_non_exhaustive()
    }
}

impl Offscreen {
    /// `side` is rounded up to a multiple of 64 (256-byte readback rows). Sprites come from `ui_dir`
    /// (`data/ui`); without them the renderer falls back to flat colours.
    pub fn new(ui_dir: &Path, side: u32) -> Result<Self, GpuError> {
        let side = side.div_ceil(64) * 64;
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen playfield"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen readback"),
            size: u64::from(side * side * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut renderer = Renderer::new(&device, &queue, format);
        renderer.load_sprites(ui_dir);
        Ok(Self {
            device,
            queue,
            renderer,
            target,
            buffer,
            side,
        })
    }

    pub fn sprites(&self) -> &SpriteSheet {
        self.renderer.sprites()
    }

    /// Renders `scene` and returns the `side x side` RGB image.
    pub fn render(&mut self, scene: &[Vertex]) -> Result<Rgb, GpuError> {
        let side = self.side;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let view = self.target.create_view(&Default::default());
        self.renderer
            .render(&mut encoder, &view, [side, side], scene);
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(side * 4),
                    rows_per_image: Some(side),
                },
            },
            self.target.size(),
        );
        self.queue.submit([encoder.finish()]);
        let slice = self.buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        let data: Vec<u8> = slice
            .get_mapped_range()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        self.buffer.unmap();
        Ok(Rgb {
            width: side as usize,
            height: side as usize,
            data,
        })
    }
}
