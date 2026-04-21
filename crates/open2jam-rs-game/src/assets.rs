use std::collections::HashMap;
use std::path::Path;

use crate::gpu::GpuResources;
use crate::render::atlas::SkinAtlas;
use crate::render::hud::HudLayout;
use open2jam_rs_parsers::xml::{parse_file as parse_skin_xml, Resources as SkinResources};

pub fn load_skin_background(
    device: wgpu::Device,
    queue: wgpu::Queue,
    skin_dir: std::path::PathBuf,
    tx: std::sync::mpsc::Sender<super::types::LoadingMessage>,
) {
    std::thread::spawn(move || {
        let result = load_skin_sync(&device, &queue, &skin_dir);
        let (atlas, resources, scale) = result;
        let output = super::types::SkinLoadOutput {
            atlas,
            resources,
            scale,
        };
        let _ = tx.send(super::types::LoadingMessage::SkinLoaded(Ok(output)));
    });
}

pub fn load_skin(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    skin_dir: &std::path::Path,
) -> (Option<SkinAtlas>, Option<SkinResources>, (f32, f32)) {
    load_skin_sync(device, queue, skin_dir)
}

fn load_skin_sync(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    skin_dir: &std::path::Path,
) -> (Option<SkinAtlas>, Option<SkinResources>, (f32, f32)) {
    let xml_path = skin_dir.join("resources.xml");
    if !xml_path.exists() {
        log::info!("Skin XML not found at {}", xml_path.display());
        return (None, None, (1.0, 1.0));
    }

    let resources = match parse_skin_xml(&xml_path) {
        Ok(r) => r,
        Err(e) => {
            log::info!("Failed to parse skin XML: {e:?}");
            return (None, None, (1.0, 1.0));
        }
    };

    let _skin_def = match resources.get_skin("o2jam") {
        Some(s) => s.clone(),
        None => {
            log::info!("Skin 'o2jam' not found in resources.xml");
            return (None, None, (1.0, 1.0));
        }
    };

    let mut sprite_speeds: HashMap<String, u32> = HashMap::new();
    let mut frame_entries: Vec<(String, String, u32, u32, u32, u32)> = Vec::new();
    for (sprite_id, sprite_def) in &resources.sprites {
        sprite_speeds.insert(sprite_id.clone(), sprite_def.frame_speed_ms);
        for frame in &sprite_def.frames {
            frame_entries.push((
                sprite_id.clone(),
                frame.file.to_string_lossy().to_string(),
                frame.x,
                frame.y,
                frame.w,
                frame.h,
            ));
        }
    }

    log::info!(
        "Skin has {} sprite frames to pack into atlas",
        frame_entries.len()
    );

    let speed_map = sprite_speeds;
    let atlas = SkinAtlas::from_frames_with_speed(
        device,
        queue,
        &frame_entries,
        |sprite_id: &str| *speed_map.get(sprite_id).unwrap_or(&50),
        |file: &str| {
            let path = Path::new(file);
            if !path.exists() {
                log::info!("Skin image not found: {}", path.display());
            }
            match image::open(path) {
                Ok(img) => Some(img.into_rgba8()),
                Err(e) => {
                    log::info!("Failed to load skin image {}: {e}", path.display());
                    None
                }
            }
        },
    );

    if let Some(ref a) = atlas {
        log::info!(
            "Atlas built: {} frames in {}x{} texture",
            a.frames.len(),
            a.width,
            a.height
        );
        for key in &[
            "head_note_white",
            "head_note_blue",
            "head_note_yellow",
            "judgmentarea",
            "note_bg",
            "measure_mark",
        ] {
            if let Some(f) = a.get_frame(key) {
                log::info!("  [OK] {} -> uv={:?}, {}x{}", key, f.uv, f.width, f.height);
            } else {
                log::info!("  [MISSING] {}", key);
            }
        }
    } else {
        log::info!("Atlas failed to build — using colored quad fallback");
    }

    log::info!("Skin loaded: 800x600");

    (atlas, Some(resources), (1.0, 1.0))
}

pub fn load_cover_from_ojn(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    config: &wgpu::SurfaceConfiguration,
    ojn_path: &std::path::Path,
) -> (
    Option<wgpu::Texture>,
    Option<wgpu::BindGroup>,
    Option<wgpu::RenderPipeline>,
    Option<wgpu::Sampler>,
) {
    let data = match std::fs::read(ojn_path) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Failed to read OJN file for cover: {e}");
            return (None, None, None, None);
        }
    };

    let jpeg_bytes = match open2jam_rs_parsers::extract_cover_image(&data) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("No cover image in OJN: {e}");
            return (None, None, None, None);
        }
    };

    let img = match image::load_from_memory(&jpeg_bytes) {
        Ok(i) => i,
        Err(e) => {
            log::warn!("Failed to decode cover JPEG: {e}");
            return (None, None, None, None);
        }
    };

    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    log::info!("Cover image: {}x{}", w, h);

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        label: Some("cover_texture"),
        view_formats: &[],
    });

    queue.write_texture(
        texture.as_image_copy(),
        &rgba,
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

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("cover_bind_group_layout"),
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

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cover_bind_group"),
        layout: &bind_group_layout,
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

    let shader = device.create_shader_module(wgpu::include_wgsl!("cover_shader.wgsl"));

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("cover_pipeline_layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cover_pipeline"),
        layout: Some(&pipeline_layout),
        multiview_mask: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        cache: None,
    });

    log::info!("Cover texture uploaded to GPU");
    (
        Some(texture),
        Some(bind_group),
        Some(pipeline),
        Some(sampler),
    )
}

pub fn build_gpu_resources(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    config: &wgpu::SurfaceConfiguration,
    skin_dir: std::path::PathBuf,
    ojn_path: Option<&std::path::Path>,
) -> (GpuResources, (f32, f32)) {
    let (atlas, skin, skin_scale) = load_skin(device, queue, skin_dir.as_path());

    let (cover_texture, cover_bind_group, cover_pipeline, cover_sampler) =
        if let Some(path) = ojn_path {
            load_cover_from_ojn(device, queue, config, path)
        } else {
            (None, None, None, None)
        };

    let hud_layout = skin
        .as_ref()
        .and_then(|res| res.get_skin("o2jam").map(|s| HudLayout::from_skin(s)));

    let mut textured_renderer =
        crate::render::textured_renderer::TexturedRenderer::new(device, queue, config);

    if let Some(ref atlas) = atlas {
        textured_renderer.set_atlas(device, atlas);
    }

    let gpu = GpuResources {
        textured_renderer,
        atlas,
        skin,
        hud_layout,
        cover_texture,
        cover_bind_group,
        cover_pipeline,
        cover_sampler,
    };

    (gpu, skin_scale)
}
