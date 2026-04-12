//! Skin texture atlas: packs all skin PNG frames into a single wgpu texture.
//!
//! Supports both static sprites (single frame) and animated sprites (multiple frames).
//!
//! For animated sprites, frames are stored with unique IDs like "measure_mark_0",
//! "measure_mark_1", etc. The `animations` map tracks which sprites are animated
//! and their frame speed.

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

/// Animation metadata for a sprite with multiple frames.
#[derive(Debug, Clone)]
pub struct SpriteAnimation {
    /// Frame speed in milliseconds per frame.
    pub frame_speed_ms: u32,
    /// Number of frames in the animation.
    pub frame_count: usize,
}

/// A packed texture atlas for skin sprites.
pub struct SkinAtlas {
    /// GPU texture containing all packed frames.
    pub texture: wgpu::Texture,
    /// GPU texture view for binding.
    pub view: wgpu::TextureView,
    /// Sampler for the atlas.
    pub sampler: wgpu::Sampler,
    /// Frame lookup by unique ID (e.g., "measure_mark_0", "score_number_0").
    pub frames: HashMap<String, AtlasFrame>,
    /// Animation metadata for sprites with multiple frames.
    /// Key is the base sprite name (e.g., "measure_mark").
    pub animations: HashMap<String, SpriteAnimation>,
    /// Atlas dimensions.
    pub width: u32,
    pub height: u32,
}

/// A loaded frame ready for packing.
struct LoadedFrame {
    /// Unique ID in the atlas (e.g., "measure_mark_0" or "score_number_0").
    id: String,
    /// Base sprite name for animation lookup (e.g., "measure_mark" or "score_number_0").
    sprite_name: String,
    /// Frame index within the animation (0 for single-frame sprites).
    frame_index: usize,
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
        Self::from_frames_with_speed(device, queue, frames, |_| 50, image_loader)
    }

    /// Build a texture atlas with animation frame speed.
    ///
    /// `get_frame_speed` returns the frame speed in ms for a given sprite ID.
    /// For multi-frame sprites, frames are stored with unique IDs like "sprite_0", "sprite_1".
    /// Single-frame sprites keep their original ID.
    pub fn from_frames_with_speed<F: Fn(&str) -> Option<image::RgbaImage>>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frames: &[(String, String, u32, u32, u32, u32)], // (id, file, x, y, w, h)
        get_frame_speed: impl Fn(&str) -> u32,
        image_loader: F,
    ) -> Option<Self> {
        if frames.is_empty() {
            warn!("No frames to pack into atlas");
            return None;
        }

        // First pass: count frames per sprite to identify animated sprites
        let mut sprite_frame_counts: HashMap<String, usize> = HashMap::new();
        for (sprite_id, _, _, _, _, _) in frames {
            *sprite_frame_counts.entry(sprite_id.clone()).or_insert(0) += 1;
        }

        // Load all frames
        let mut loaded: Vec<LoadedFrame> = Vec::new();
        for (sprite_id, file, fx, fy, fw, fh) in frames {
            let img = match image_loader(file) {
                Some(img) => img,
                None => {
                    warn!("Atlas: failed to load image for frame '{}'", sprite_id);
                    continue;
                }
            };

            let img_w = img.width();
            let img_h = img.height();

            if *fx + *fw > img_w || *fy + *fh > img_h {
                warn!(
                    "Atlas: frame '{}' out of bounds in {} (rect: {},{},{},{} but image is {}x{})",
                    sprite_id, file, fx, fy, fw, fh, img_w, img_h
                );
                continue;
            }

            // Extract the sub-rectangle
            let sub_img = image::imageops::crop_imm(&img, *fx, *fy, *fw, *fh).to_image();

            // Determine unique atlas ID and animation metadata
            let frame_count = sprite_frame_counts.get(sprite_id).copied().unwrap_or(1);
            let (atlas_id, sprite_name, frame_index) = if frame_count > 1 {
                // Multi-frame sprite: use "name_0", "name_1" etc.
                // We need to track which frame index this is
                let existing_count = loaded.iter().filter(|l| l.sprite_name == *sprite_id).count();
                (
                    format!("{}_{}", sprite_id, existing_count),
                    sprite_id.clone(),
                    existing_count,
                )
            } else {
                // Single-frame sprite: keep original ID
                (sprite_id.clone(), sprite_id.clone(), 0)
            };

            loaded.push(LoadedFrame {
                id: atlas_id,
                sprite_name,
                frame_index,
                img: sub_img,
            });
        }

        if loaded.is_empty() {
            warn!("Atlas: no frames loaded successfully");
            return None;
        }

        info!("Atlas: {} frames loaded successfully", loaded.len());

        // Row-packing
        let max_atlas_width: u32 = 4096;
        let mut placements: Vec<(String, u32, u32, u32, u32)> = Vec::new();
        let mut cursor_x: u32 = 0;
        let mut cursor_y: u32 = 0;
        let mut row_height: u32 = 0;

        for lf in &loaded {
            let fw = lf.img.width();
            let fh = lf.img.height();

            if cursor_x > 0 && cursor_x + fw + 1 > max_atlas_width {
                cursor_x = 0;
                cursor_y += row_height + 1;
                row_height = 0;
            }
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

        // Pack into atlas image
        let mut atlas_img: image::RgbaImage = image::ImageBuffer::new(atlas_width, atlas_height);
        for (i, (_id, ax, ay, aw, ah)) in placements.iter().enumerate() {
            let lf = &loaded[i];
            for sy in 0..*ah {
                for sx in 0..*aw {
                    atlas_img.put_pixel(*ax + sx, *ay + sy, *lf.img.get_pixel(sx, sy));
                }
            }
        }

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

        // Build frame lookup and animation metadata
        let mut frame_map: HashMap<String, AtlasFrame> = HashMap::new();
        let mut anim_map: HashMap<String, (u32, usize)> = HashMap::new(); // (speed, max_frame_count)

        for (i, (id, ax, ay, aw, ah)) in placements.iter().enumerate() {
            let lf = &loaded[i];
            let inset = 0.5;
            let u0 = (*ax as f32 + inset) / atlas_width as f32;
            let v0 = (*ay as f32 + inset) / atlas_height as f32;
            let u1 = (*ax as f32 + *aw as f32 - inset) / atlas_width as f32;
            let v1 = (*ay as f32 + *ah as f32 - inset) / atlas_height as f32;

            // Store all frames in the map (unique IDs like "measure_mark_0")
            frame_map.insert(
                id.clone(),
                AtlasFrame {
                    uv: [u0, v0, u1, v1],
                    width: *aw,
                    height: *ah,
                },
            );

            // Also store first frame under base sprite name for backward compatibility
            if lf.frame_index == 0 {
                frame_map.insert(
                    lf.sprite_name.clone(),
                    AtlasFrame {
                        uv: [u0, v0, u1, v1],
                        width: *aw,
                        height: *ah,
                    },
                );
            }

            // Track animation metadata
            if lf.frame_index > 0 {
                // This is a multi-frame sprite
                let speed = get_frame_speed(&lf.sprite_name);
                let entry = anim_map.entry(lf.sprite_name.clone()).or_insert((speed, 0));
                if lf.frame_index + 1 > entry.1 {
                    entry.1 = lf.frame_index + 1;
                }
            }
        }

        // Build animations map
        let animations: HashMap<String, SpriteAnimation> = anim_map
            .into_iter()
            .map(|(name, (speed, count))| {
                (
                    name,
                    SpriteAnimation {
                        frame_speed_ms: speed,
                        frame_count: count,
                    },
                )
            })
            .collect();

        if !animations.is_empty() {
            info!("Atlas: {} animated sprites", animations.len());
            for (id, a) in &animations {
                info!("  {} -> {} frames @ {}ms", id, a.frame_count, a.frame_speed_ms);
            }
        }

        Some(Self {
            texture,
            view,
            sampler,
            frames: frame_map,
            animations,
            width: atlas_width,
            height: atlas_height,
        })
    }

    /// Look up a frame by its unique atlas ID.
    pub fn get_frame(&self, id: &str) -> Option<&AtlasFrame> {
        self.frames.get(id)
    }

    /// Get the current animation frame for a sprite at the given elapsed time.
    /// Returns None if the sprite is not animated.
    pub fn get_frame_at_time(&self, sprite_id: &str, time_ms: f64) -> Option<AtlasFrame> {
        let anim = self.animations.get(sprite_id)?;
        let frame_idx = ((time_ms / anim.frame_speed_ms as f64) as usize) % anim.frame_count;
        let atlas_id = format!("{}_{}", sprite_id, frame_idx);
        self.frames.get(&atlas_id).copied()
    }
}