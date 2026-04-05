//! Skin texture atlas: packs all skin PNG frames into a single wgpu texture.

use std::collections::HashMap;

use log::{info, warn};

/// A rectangular frame within the atlas texture.
#[derive(Debug, Clone, Copy)]
pub struct AtlasFrame {
    /// UV coordinates: (u0, v0, u1, v1) normalized to [0, 1].
    pub uv: [f32; 4],
    /// Original pixel dimensions of the frame.
    pub width: u32,
    pub height: u32,
}

/// A packed texture atlas for skin sprites.
pub struct SkinAtlas {
    /// GPU texture containing all packed frames.
    pub texture: wgpu::Texture,
    /// GPU texture view for binding.
    pub view: wgpu::TextureView,
    /// Sampler for the atlas.
    pub sampler: wgpu::Sampler,
    /// Frame lookup by ID.
    pub frames: HashMap<String, AtlasFrame>,
    /// Atlas dimensions.
    pub width: u32,
    pub height: u32,
}

/// A loaded frame ready for packing.
struct LoadedFrame {
    id: String,
    img: image::RgbaImage,
}

impl SkinAtlas {
    /// Build a texture atlas from skin XML frame definitions.
    ///
    /// Loads each referenced PNG file, extracts the specified rectangles,
    /// and packs them into a single atlas texture.
    pub fn from_frames<F: Fn(&str) -> Option<image::RgbaImage>>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frames: &[(String, String, u32, u32, u32, u32)], // (id, file, x, y, w, h)
        image_loader: F,
    ) -> Option<Self> {
        if frames.is_empty() {
            warn!("No frames to pack into atlas");
            return None;
        }

        // Load all frames, skipping ones that fail
        let mut loaded: Vec<LoadedFrame> = Vec::new();
        for (id, file, fx, fy, fw, fh) in frames {
            let img = match image_loader(file) {
                Some(img) => img,
                None => {
                    warn!("Atlas: failed to load image for frame '{}'", id);
                    continue;
                }
            };

            let img_w = img.width();
            let img_h = img.height();

            if *fx + *fw > img_w || *fy + *fh > img_h {
                warn!(
                    "Atlas: frame '{}' out of bounds in {} (rect: {},{},{},{} but image is {}x{})",
                    id, file, fx, fy, fw, fh, img_w, img_h
                );
                continue;
            }

            // Extract the sub-rectangle
            let sub_img = image::imageops::crop_imm(&img, *fx, *fy, *fw, *fh).to_image();
            loaded.push(LoadedFrame { id: id.clone(), img: sub_img });
        }

        if loaded.is_empty() {
            warn!("Atlas: no frames loaded successfully");
            return None;
        }

        info!("Atlas: {} frames loaded successfully", loaded.len());

        // Simple row-packing algorithm: lay out frames left-to-right, top-to-bottom
        let max_atlas_width: u32 = 4096;
        let mut placements: Vec<(String, u32, u32, u32, u32)> = Vec::new(); // (id, x, y, w, h)

        let mut cursor_x: u32 = 0;
        let mut cursor_y: u32 = 0;
        let mut row_height: u32 = 0;

        for lf in &loaded {
            let fw = lf.img.width();
            let fh = lf.img.height();

            // Row packing: move to next row if frame doesn't fit
            if cursor_x > 0 && cursor_x + fw + 1 > max_atlas_width {
                cursor_x = 0;
                cursor_y += row_height + 1; // 1px vertical padding between rows
                row_height = 0;
            }

            // Add 1px padding between frames (horizontal)
            if cursor_x > 0 {
                cursor_x += 1;
            }

            placements.push((lf.id.clone(), cursor_x, cursor_y, fw, fh));
            cursor_x += fw;
            if fh > row_height {
                row_height = fh;
            }
        }

        let atlas_width = max_atlas_width;
        let atlas_height = cursor_y + row_height;

        if atlas_height == 0 {
            warn!("Atlas has zero height");
            return None;
        }

        info!("Atlas size: {}x{} ({} frames)", atlas_width, atlas_height, placements.len());

        // Create atlas image buffer (RGBA8)
        let mut atlas_img: image::RgbaImage = image::ImageBuffer::new(atlas_width, atlas_height);

        // Place frames into atlas — use index-based lookup to match placements with loaded frames
        for (i, (_id, ax, ay, aw, ah)) in placements.iter().enumerate() {
            let lf = &loaded[i];
            for sy in 0..*ah {
                for sx in 0..*aw {
                    let pixel = lf.img.get_pixel(sx, sy);
                    atlas_img.put_pixel(*ax + sx, *ay + sy, *pixel);
                }
            }
        }

        // Create wgpu texture
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Skin Atlas Texture"),
            size: wgpu::Extent3d {
                width: atlas_width,
                height: atlas_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Upload texture data
        let rgba_data = atlas_img.into_raw();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas_width * 4),
                rows_per_image: Some(atlas_height),
            },
            wgpu::Extent3d {
                width: atlas_width,
                height: atlas_height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // Build frame lookup with normalized UVs (inset 0.5px to avoid linear filtering bleed)
        let mut frame_map = HashMap::new();
        for (id, ax, ay, aw, ah) in &placements {
            let inset = 0.5; // 0.5px inset prevents texture bleeding
            let u0 = (*ax as f32 + inset) / atlas_width as f32;
            let v0 = (*ay as f32 + inset) / atlas_height as f32;
            let u1 = (*ax as f32 + *aw as f32 - inset) / atlas_width as f32;
            let v1 = (*ay as f32 + *ah as f32 - inset) / atlas_height as f32;

            // For animated sprites (multiple frames), only store the first frame
            // Future: store all frames and select based on animation time
            if !frame_map.contains_key(id) {
                frame_map.insert(
                    id.clone(),
                    AtlasFrame {
                        uv: [u0, v0, u1, v1],
                        width: *aw,
                        height: *ah,
                    },
                );
            }
        }

        Some(Self {
            texture,
            view,
            sampler,
            frames: frame_map,
            width: atlas_width,
            height: atlas_height,
        })
    }

    /// Look up a frame by ID.
    pub fn get_frame(&self, id: &str) -> Option<&AtlasFrame> {
        self.frames.get(id)
    }
}
