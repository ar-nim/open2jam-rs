//! Sprite renderer: batched colored-quad rendering via wgpu.

use bytemuck::{Pod, Zeroable};

/// Vertex for sprite quads: position in screen coordinates + RGBA color.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct SpriteVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

const VERTEX_ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x4,
];

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<SpriteVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: VERTEX_ATTRIBUTES,
    }
}

/// Resolution uniform padded to 256 bytes for dynamic uniform alignment.
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

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let normalized = input.position / resolution.res;
    let clip_x = normalized.x * 2.0 - 1.0;
    let clip_y = (1.0 - normalized.y) * 2.0 - 1.0;
    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

/// Batched sprite renderer for colored quads.
pub struct SpriteRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    resolution_buffer: wgpu::Buffer,
    max_sprites: usize,

    /// CPU-side staging for vertices before upload.
    vertices: Vec<SpriteVertex>,
}

impl SpriteRenderer {
    /// Create a new sprite renderer.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
    ) -> Self {
        let max_sprites = 4096;
        let vertex_count = max_sprites * 6;
        let vertex_buffer_size = (vertex_count * std::mem::size_of::<SpriteVertex>()) as u64;

        // Create vertex buffer.
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite Vertex Buffer"),
            size: vertex_buffer_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Resolution uniform buffer (256-byte aligned for dynamic offset).
        let resolution_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite Resolution Buffer"),
            size: RESOLUTION_UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Write initial resolution.
        let res = ResolutionUniform {
            width: config.width as f32,
            height: config.height as f32,
            _padding: [0.0; 62],
        };
        queue.write_buffer(&resolution_buffer, 0, bytemuck::bytes_of(&res));

        // Bind group layout and pipeline layout.
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Sprite Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(std::num::NonZeroU64::new(8).unwrap()),
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sprite Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sprite Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sprite Render Pipeline"),
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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Sprite Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &resolution_buffer,
                    offset: 0,
                    size: Some(std::num::NonZeroU64::new(8).unwrap()),
                }),
            }],
        });

        Self {
            pipeline,
            bind_group,
            vertex_buffer,
            resolution_buffer,
            max_sprites,
            vertices: Vec::with_capacity(vertex_count),
        }
    }

    /// Start a new batch, clearing the vertex staging buffer.
    pub fn begin(&mut self, _queue: &wgpu::Queue) {
        self.vertices.clear();
    }

    /// Add a colored quad to the batch.
    ///
    /// `x, y` is the top-left corner in screen coordinates.
    /// `w, h` are the width and height.
    /// `color` is RGBA in [0, 1].
    pub fn draw_quad(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        // Two triangles: (0,1,2) and (0,2,3) covering the quad.
        // Vertex order: top-left, top-right, bottom-left, bottom-right
        let tl = [x, y];
        let tr = [x + w, y];
        let bl = [x, y + h];
        let br = [x + w, y + h];

        // Triangle 1: tl -> tr -> bl
        self.vertices.push(SpriteVertex {
            position: tl,
            color,
        });
        self.vertices.push(SpriteVertex {
            position: tr,
            color,
        });
        self.vertices.push(SpriteVertex {
            position: bl,
            color,
        });

        // Triangle 2: tl -> bl -> br
        self.vertices.push(SpriteVertex {
            position: tl,
            color,
        });
        self.vertices.push(SpriteVertex {
            position: bl,
            color,
        });
        self.vertices.push(SpriteVertex {
            position: br,
            color,
        });
    }

    /// Upload vertex data and issue draw calls.
    ///
    /// Creates a command encoder, begins a render pass targeting `view`,
    /// sets the pipeline and bind group, and draws all queued quads.
    pub fn end(&mut self, view: &wgpu::TextureView, queue: &wgpu::Queue, device: &wgpu::Device) {
        if self.vertices.is_empty() {
            return;
        }

        let vertex_count = self.vertices.len();

        // Upload vertex data.
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Sprite Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Sprite Render Pass"),
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
            render_pass.set_bind_group(0, &self.bind_group, &[0]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.draw(0..vertex_count as u32, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Update the resolution uniform after a window resize.
    pub fn resize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) {
        // Update resolution buffer.
        let res = ResolutionUniform {
            width: width as f32,
            height: height as f32,
            _padding: [0.0; 62],
        };
        queue.write_buffer(&self.resolution_buffer, 0, bytemuck::bytes_of(&res));

        // Note: if the surface format changed, the pipeline would need recreation.
        // This resize method only handles dimension changes with the same format.
        let _ = device; // device kept for potential future pipeline recreation
    }
}
