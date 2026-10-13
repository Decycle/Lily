//! Background movie layer: `yuv420p` frames uploaded to three R8 textures and drawn as the game's movie
//! quad, the farthest opaque surface of the 3D stage pass (`stage_gpu`). Decoding lives in `wacca-video`.

#[derive(Debug)]
pub(crate) struct VideoLayer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    planes: Option<Planes>,
    pub(crate) visible: bool,
}

#[derive(Debug)]
struct Planes {
    size: [u32; 2],
    textures: [wgpu::Texture; 3],
    bind: wgpu::BindGroup,
}

/// Sizes of the Y, U and V planes of a `width` × `height` `yuv420p` frame.
pub(crate) fn plane_sizes([w, h]: [u32; 2]) -> [[u32; 2]; 3] {
    let c = [w.div_ceil(2), h.div_ceil(2)];
    [[w, h], c, c]
}

impl VideoLayer {
    pub(crate) fn new(device: &wgpu::Device, samples: u32) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("video.wgsl"));
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video"),
            entries: &[
                tex(0),
                tex(1),
                tex(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let options = wgpu::PipelineCompilationOptions::default();
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: options.clone(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: Some(crate::stage_gpu::depth_state(true)),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: options,
                targets: &[Some(crate::stage_gpu::HDR_FORMAT.into())],
            }),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            planes: None,
            visible: false,
        }
    }

    /// Upload one frame (`planes` = Y, U, V, tightly packed); textures are recreated when the size changes.
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        planes: [&[u8]; 3],
    ) {
        let sizes = plane_sizes(size);
        if sizes
            .iter()
            .zip(planes)
            .any(|([w, h], p)| p.len() != *w as usize * *h as usize)
        {
            log::warn!("video frame {size:?}: plane sizes do not match; skipped");
            return;
        }
        if self.planes.as_ref().is_none_or(|p| p.size != size) {
            self.planes = Some(self.create(device, size));
        }
        let Some(p) = &self.planes else { return };
        for ((tex, [w, h]), data) in p.textures.iter().zip(sizes).zip(planes) {
            queue.write_texture(
                tex.as_image_copy(),
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w),
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

    fn create(&self, device: &wgpu::Device, size: [u32; 2]) -> Planes {
        let textures = plane_sizes(size).map(|[width, height]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video plane"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        let views = textures
            .each_ref()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let entries: Vec<_> = views
            .iter()
            .map(wgpu::BindingResource::TextureView)
            .chain([wgpu::BindingResource::Sampler(&self.sampler)])
            .enumerate()
            .map(|(i, resource)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource,
            })
            .collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video"),
            layout: &self.layout,
            entries: &entries,
        });
        Planes {
            size,
            textures,
            bind,
        }
    }

    /// A frame is uploaded and the layer is visible.
    pub(crate) fn shown(&self) -> bool {
        self.visible && self.planes.is_some()
    }

    /// Draw the last uploaded frame over the current viewport (if visible).
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass) {
        if let Some(p) = self.planes.as_ref().filter(|_| self.visible) {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &p.bind, &[]);
            pass.draw(0..4, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chroma_planes_are_half_size() {
        assert_eq!(
            plane_sizes([1080, 1080]),
            [[1080, 1080], [540, 540], [540, 540]]
        );
        assert_eq!(plane_sizes([5, 3]), [[5, 3], [3, 2], [3, 2]]);
    }
}
