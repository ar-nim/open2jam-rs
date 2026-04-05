//! Textured sprite renderer using the skin atlas texture.

use bytemuck::{Pod, Zeroable};

use super::atlas::SkinAtlas;

/// Vertex for textured sprites: position + UV + color tint.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct TexturedVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub tint: [f32; 4],
}

const VERTEX_ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x4,
];

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<TexturedVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: VERTEX_ATTRIBUTES,
    }
}

/// Resolution uniform padded to 256 bytes.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ResolutionUniform {
    width: f32,
    height: f32,
    _padding: [f32; 62],
}

const RESOLUTION_UNIFORM_SIZE: u64 = 256;

const SHADER_SRC: &str = r#"
struct Resolution {
    res: vec2<f32>,
}

@group(0) @binding(0) var<uniform> resolution: Resolution;
@group(0) @binding(1) var skin_atlas: texture_2d<f32>;
@group(0) @binding(2) var skin_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let normalized = input.position / resolution.res;
    let clip_x = normalized.x * 2.0 - 1.0;
    let clip_y = (1.0 - normalized.y) * 2.0 - 1.0;
    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.uv = input.uv;
    output.tint = input.tint;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(skin_atlas, skin_sampler, input.uv);
    return tex_color * input.tint;
}
"#;

/// Batched textured sprite renderer.
pub struct TexturedRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: Option<wgpu::BindGroup>,
    vertex_buffer: wgpu::Buffer,
    resolution_buffer: wgpu::Buffer,
    max_sprites: usize,
    vertices: Vec<TexturedVertex>,
}

impl TexturedRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
    ) -> Self {
        let max_sprites = 8192;
        let vertex_count = max_sprites * 6;
        let vertex_buffer_size = (vertex_count * std::mem::size_of::<TexturedVertex>()) as u64;

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Textured Sprite Vertex Buffer"),
            size: vertex_buffer_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let resolution_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Textured Sprite Resolution Buffer"),
            size: RESOLUTION_UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let res = ResolutionUniform {
            width: config.width as f32,
            height: config.height as f32,
            _padding: [0.0; 62],
        };
        queue.write_buffer(&resolution_buffer, 0, bytemuck::bytes_of(&res));

        let res_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Textured Sprite Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(std::num::NonZeroU64::new(8).unwrap()),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Textured Sprite Pipeline Layout"),
            bind_group_layouts: &[Some(&res_bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Textured Sprite Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Textured Sprite Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group: None,
            vertex_buffer,
            resolution_buffer,
            max_sprites,
            vertices: Vec::with_capacity(vertex_count),
        }
    }

    /// Set the atlas texture and create the bind group.
    pub fn set_atlas(&mut self, device: &wgpu::Device, atlas: &SkinAtlas) {
        let bind_group_layout = self.pipeline.get_bind_group_layout(0);
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Textured Sprite Atlas Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.resolution_buffer,
                        offset: 0,
                        size: Some(std::num::NonZeroU64::new(8).unwrap()),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        }));
    }

    /// Start a new batch.
    pub fn begin(&mut self) {
        self.vertices.clear();
    }

    /// Draw a textured quad.
    ///
    /// `x, y` is the top-left corner in screen coordinates.
    /// `w, h` are the width and height.
    /// `uv` is (u0, v0, u1, v1) from the atlas frame.
    /// `tint` is an RGBA color multiplier (default [1,1,1,1]).
    pub fn draw_textured_quad(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], tint: [f32; 4]) {
        let [u0, v0, u1, v1] = uv;

        // Vertex positions: top-left, top-right, bottom-left, bottom-right
        let positions = [
            [x, y],       // TL
            [x + w, y],   // TR
            [x, y + h],   // BL
            [x + w, y + h], // BR
        ];

        // UV coords (flip Y because texture origin is top-left)
        let uvs = [
            [u0, v1], // TL
            [u1, v1], // TR
            [u0, v0], // BL
            [u1, v0], // BR
        ];

        // Triangle 1: TL, TR, BL
        for i in [0, 1, 2] {
            self.vertices.push(TexturedVertex {
                position: positions[i],
                uv: uvs[i],
                tint,
            });
        }

        // Triangle 2: TL, BL, BR
        for i in [0, 2, 3] {
            self.vertices.push(TexturedVertex {
                position: positions[i],
                uv: uvs[i],
                tint,
            });
        }
    }

    /// Upload and issue draw calls.
    pub fn end(&mut self, view: &wgpu::TextureView, queue: &wgpu::Queue, device: &wgpu::Device) {
        if self.vertices.is_empty() {
            return;
        }
        let Some(ref bind_group) = self.bind_group else {
            return;
        };

        let vertex_count = self.vertices.len();
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Textured Sprite Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Textured Sprite Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, bind_group, &[0]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.draw(0..vertex_count as u32, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Update resolution after resize.
    pub fn resize(&mut self, _device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) {
        let res = ResolutionUniform {
            width: width as f32,
            height: height as f32,
            _padding: [0.0; 62],
        };
        queue.write_buffer(&self.resolution_buffer, 0, bytemuck::bytes_of(&res));
    }
}
