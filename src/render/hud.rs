//! HUD rendering: score, combo, lifebar, jam bar, pills, and judgment popups.
//!
//! Uses skin XML entity positions for all HUD elements.
//! All positions match the entities defined in resources.xml o2jam skin.

use crate::game_state::{GameState, PendingJudgment};
use crate::gameplay::judgment::JudgmentType;
use crate::render::atlas::{AtlasFrame, SkinAtlas};
use crate::render::textured_renderer::TexturedRenderer;

/// HUD skin entity positions extracted from skin XML.
/// All values match the entity positions in resources.xml exactly.
#[derive(Debug, Clone)]
pub struct HudLayout {
    // Score counter: SCORE_COUNTER x="192" y="567"
    pub score_x: f32,
    pub score_y: f32,
    // Combo counter: COMBO_COUNTER x="99" y="210"
    pub combo_x: f32,
    pub combo_y: f32,
    // Jam counter: JAM_COUNTER x="99" y="80"
    pub jam_x: f32,
    pub jam_y: f32,
    // Timers (minute/second)
    pub minute_x: f32,
    pub minute_y: f32,
    pub second_x: f32,
    pub second_y: f32,
    // Max combo: MAXCOMBO_COUNTER x="671" y="557"
    pub max_combo_x: f32,
    pub max_combo_y: f32,
    // Judgment counts
    pub judgment_perfect_x: f32,
    pub judgment_perfect_y: f32,
    pub judgment_cool_x: f32,
    pub judgment_cool_y: f32,
    pub judgment_good_x: f32,
    pub judgment_good_y: f32,
    pub judgment_bad_x: f32,
    pub judgment_bad_y: f32,
    pub judgment_miss_x: f32,
    pub judgment_miss_y: f32,
    // Judgment popup: EFFECT_JUDGMENT_COOL x="30" y="280"
    pub judgment_popup_x: f32,
    pub judgment_popup_y: f32,
    // Life bar: LIFE_BAR x="203" y="247"
    pub lifebar_x: f32,
    pub lifebar_y: f32,
    // Jam bar: JAM_BAR x="4" y="536"
    pub jam_bar_x: f32,
    pub jam_bar_y: f32,
    // Pills: PILL_1-5
    pub pill_positions: Vec<(f32, f32)>,
}

impl HudLayout {
    /// Create HUD layout from skin XML entity positions.
    /// Values match resources.xml o2jam skin exactly.
    pub fn from_skin() -> Self {
        Self {
            // SCORE_COUNTER: x="192" y="567"
            score_x: 192.0,
            score_y: 567.0,
            // COMBO_COUNTER: x="99" y="210"
            combo_x: 99.0,
            combo_y: 210.0,
            // JAM_COUNTER: x="99" y="80"
            jam_x: 99.0,
            jam_y: 80.0,
            // MINUTE_COUNTER: x="346" y="569"
            minute_x: 346.0,
            minute_y: 569.0,
            // SECOND_COUNTER: x="410" y="569"
            second_x: 410.0,
            second_y: 569.0,
            // MAXCOMBO_COUNTER: x="671" y="557"
            max_combo_x: 671.0,
            max_combo_y: 557.0,
            // COUNTER_JUDGMENT_PERFECT: x="658" y="500"
            judgment_perfect_x: 658.0,
            judgment_perfect_y: 500.0,
            // COUNTER_JUDGMENT_COOL: x="658" y="572"
            judgment_cool_x: 658.0,
            judgment_cool_y: 572.0,
            // COUNTER_JUDGMENT_GOOD: x="728" y="572"
            judgment_good_x: 728.0,
            judgment_good_y: 572.0,
            // COUNTER_JUDGMENT_BAD: x="658" y="581"
            judgment_bad_x: 658.0,
            judgment_bad_y: 581.0,
            // COUNTER_JUDGMENT_MISS: x="728" y="581"
            judgment_miss_x: 728.0,
            judgment_miss_y: 581.0,
            // EFFECT_JUDGMENT_COOL: x="30" y="280"
            judgment_popup_x: 30.0,
            judgment_popup_y: 280.0,
            // LIFE_BAR: x="203" y="247"
            lifebar_x: 203.0,
            lifebar_y: 247.0,
            // JAM_BAR: x="4" y="536"
            jam_bar_x: 4.0,
            jam_bar_y: 536.0,
            // PILL_1 through PILL_5
            pill_positions: vec![
                (200.0, 127.0), // PILL_1
                (200.0, 95.0),  // PILL_2
                (200.0, 64.0),  // PILL_3
                (200.0, 33.0),  // PILL_4
                (200.0, 2.0),   // PILL_5
            ],
        }
    }
}

/// Draw a number using individual digit sprites.
/// Numbers are drawn right-to-left from the entity position.
/// The entity position in skin XML is the RIGHT edge of the last digit.
/// All dimensions (width, height, spacing) are scaled.
fn draw_number(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(usize) -> Option<AtlasFrame>,
    value: u32,
    base_x: f32,
    base_y: f32,
    digit_spacing: f32,
    scale_x: f32,
    scale_y: f32,
    color: [f32; 4],
) {
    let digits: Vec<u32> = if value == 0 {
        vec![0]
    } else {
        let mut d = Vec::new();
        let mut v = value;
        while v > 0 {
            d.push(v % 10);
            v /= 10;
        }
        d.reverse();
        d
    };

    // Calculate total width to find starting position (all scaled)
    let mut total_width: f32 = 0.0;
    let mut digit_widths = Vec::new();
    let mut digit_heights = Vec::new();
    for (i, &digit) in digits.iter().enumerate() {
        if let Some(frame) = get_frame(digit as usize) {
            let sw = frame.width as f32 * scale_x;
            digit_widths.push(sw);
            digit_heights.push(frame.height as f32 * scale_y);
            total_width += sw;
            if i < digits.len() - 1 {
                total_width += digit_spacing;
            }
        } else {
            digit_widths.push(0.0);
            digit_heights.push(0.0);
        }
    }

    // Start from the right edge (base_x) and draw leftwards
    let mut x = base_x - total_width;
    
    for (i, &digit) in digits.iter().enumerate() {
        let w = digit_widths[i];
        let h = digit_heights[i];
        if w > 0.0 {
            if let Some(frame) = get_frame(digit as usize) {
                renderer.draw_textured_quad(
                    x, base_y, w, h,
                    frame.uv, color,
                );
            }
        }
        x += w + digit_spacing;
    }
}

/// Draw a number using individual digit sprites.
/// Numbers are drawn left-to-right starting from the base_x position.
/// The entity position in skin XML is the LEFT edge of the first digit.
fn draw_number_left_to_right(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(usize) -> Option<AtlasFrame>,
    value: u32,
    base_x: f32,
    base_y: f32,
    digit_spacing: f32,
    scale_x: f32,
    scale_y: f32,
    color: [f32; 4],
) {
    let digits: Vec<u32> = if value == 0 {
        vec![0]
    } else {
        let mut d = Vec::new();
        let mut v = value;
        while v > 0 {
            d.push(v % 10);
            v /= 10;
        }
        d.reverse();
        d
    };

    // Draw left-to-right from base_x
    let mut x = base_x;
    
    for &digit in digits.iter() {
        if let Some(frame) = get_frame(digit as usize) {
            let w = frame.width as f32 * scale_x;
            let h = frame.height as f32 * scale_y;
            renderer.draw_textured_quad(
                x, base_y, w, h,
                frame.uv, color,
            );
            x += w + digit_spacing;
        }
    }
}

/// Draw a number using individual digit sprites, centered on center_x.
/// The entity position in skin XML is the CENTER of the number display.
fn draw_number_centered(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(usize) -> Option<AtlasFrame>,
    value: u32,
    center_x: f32,
    base_y: f32,
    digit_spacing: f32,
    scale_x: f32,
    scale_y: f32,
    color: [f32; 4],
) {
    let digits: Vec<u32> = if value == 0 {
        vec![0]
    } else {
        let mut d = Vec::new();
        let mut v = value;
        while v > 0 {
            d.push(v % 10);
            v /= 10;
        }
        d.reverse();
        d
    };

    // Calculate total width (all scaled)
    let mut total_width: f32 = 0.0;
    for (i, &digit) in digits.iter().enumerate() {
        if let Some(frame) = get_frame(digit as usize) {
            total_width += frame.width as f32 * scale_x;
            if i < digits.len() - 1 {
                total_width += digit_spacing;
            }
        }
    }

    // Start from center - total_width/2
    let mut x = center_x - total_width / 2.0;
    for (i, &digit) in digits.iter().enumerate() {
        if let Some(frame) = get_frame(digit as usize) {
            let w = frame.width as f32 * scale_x;
            let h = frame.height as f32 * scale_y;
            renderer.draw_textured_quad(
                x, base_y, w, h,
                frame.uv, color,
            );
            x += w;
            if i < digits.len() - 1 {
                x += digit_spacing;
            }
        }
    }
}

/// Draw the score using skin XML entity positions.
/// SCORE_COUNTER: x="192" y="567", score numbers are 24x18 pixels
pub fn draw_score(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    score: u32,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    let x = ox + layout.score_x * sx;
    let y = oy + layout.score_y * sy;
    
    draw_number(
        renderer,
        &|d| get_frame(&format!("score_number_{}", d)),
        score,
        x, y,
        0.0, // No spacing between score numbers
        sx, sy,
        [1.0, 1.0, 1.0, 1.0],
    );
}

/// Draw the max combo counter using skin XML entity positions.
/// MAXCOMBO_COUNTER: x="671" y="557" - left-aligned position in the dashboard.
pub fn draw_max_combo(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    max_combo: u32,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    if max_combo == 0 {
        return;
    }

    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    let x = ox + layout.max_combo_x * sx;
    let y = oy + layout.max_combo_y * sy;

    // MAXCOMBO_COUNTER x="671" is the RIGHT edge, so draw right-to-left
    draw_number(
        renderer,
        &|d| get_frame(&format!("maxcombo_number_{}", d)),
        max_combo,
        x, y,
        1.0, // minimal spacing between maxcombo digits
        sx, sy,
        [1.0, 1.0, 1.0, 1.0],
    );
}

/// Draw the combo counter using skin XML entity positions.
/// COMBO_COUNTER: x="99" y="210" - centered position (same as jam counter).
///
/// Animation: Drops 10px on increment, slides back up in 20ms.
/// Visible for 4s total, then hidden until next combo.
pub fn draw_combo(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    combo: u32,
    combo_y: f32,
    combo_visible: bool,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    if combo == 0 || !combo_visible {
        return;
    }

    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    let cx = ox + layout.combo_x * sx; // center X (same alignment as jam counter)
    let cy = oy + combo_y * sy; // Use animated Y position

    draw_number_centered(
        renderer,
        &|d| get_frame(&format!("combo_number_{}", d)),
        combo,
        cx, cy,
        0.0, // no spacing between combo digits (same sprite width)
        sx, sy,
        [1.0, 1.0, 1.0, 1.0],
    );

    // NOTE: COMBO title is drawn separately with its own visibility timer
}

/// Draw the combo title using skin XML entity positions.
/// COMBO_TITLE visibility: 2 seconds timeout.
pub fn draw_combo_title(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    combo: u32,
    combo_y: f32,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    if combo == 0 {
        return;
    }

    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    let cx = ox + layout.combo_x * sx;
    let cy = oy + combo_y * sy;

    if let Some(title_frame) = get_frame("combo_title") {
        renderer.draw_textured_quad(
            cx - (title_frame.width as f32 * sx) / 2.0,
            cy - title_frame.height as f32 * sy - 5.0 * sy,
            title_frame.width as f32 * sx,
            title_frame.height as f32 * sy,
            title_frame.uv,
            [1.0, 1.0, 1.0, 0.9],
        );
    }
}

/// Draw the jam counter using skin XML entity positions.
/// JAM_COUNTER: x="99" y="80" - this position is the CENTER of the number display.
pub fn draw_jam_counter(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    jam_combo: u32,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    if jam_combo == 0 {
        return;
    }
    
    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    let cx = ox + layout.jam_x * sx;
    let cy = oy + layout.jam_y * sy;
    
    draw_number_centered(
        renderer,
        &|d| get_frame(&format!("jam_number_{}", d)),
        jam_combo,
        cx, cy,
        2.0 * sx, // Small spacing between jam digits
        sx, sy,
        [1.0, 1.0, 1.0, 1.0],
    );

    // Draw "JAM" title above the jam number
    if let Some(title_frame) = get_frame("jam_title") {
        renderer.draw_textured_quad(
            cx - (title_frame.width as f32 * sx) / 2.0,
            cy - title_frame.height as f32 * sy - 5.0 * sy,
            title_frame.width as f32 * sx,
            title_frame.height as f32 * sy,
            title_frame.uv,
            [1.0, 1.0, 1.0, 0.9],
        );
    }
}

/// Draw the lifebar using skin XML entity positions.
/// LIFE_BAR: x="203" y="247", sprite is 11x301 pixels
/// Fill direction: up_to_down
pub fn draw_lifebar(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    life_percent: f32,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    
    let bg_x = ox + layout.lifebar_x * sx;
    let bg_y = oy + layout.lifebar_y * sy;
    let bar_w = 11.0 * sx;
    let bar_h = 301.0 * sy;
    
    // Draw lifebar background
    if let Some(bg_frame) = get_frame("lifebar_bg") {
        renderer.draw_textured_quad(
            bg_x, bg_y,
            bg_frame.width as f32 * sx,
            bg_frame.height as f32 * sy,
            bg_frame.uv,
            [1.0, 1.0, 1.0, 1.0],
        );
    }
    
    // Draw lifebar fill (clipped based on life_percent)
    // Fill from bottom (up_to_down means empty at bottom when 0%)
    if let Some(bar_frame) = get_frame("lifebar") {
        let fill_height = bar_h * life_percent;
        let fill_y = bg_y + bar_h - fill_height;
        
        if fill_height > 0.5 {
            // Clip the UV to show only the filled portion
            // For up_to_down, we clip from the top of the sprite
            let uv_u = bar_frame.uv[0];
            let uv_v = bar_frame.uv[1] + bar_frame.uv[3] * (1.0 - life_percent);
            let uv_w = bar_frame.uv[2];
            let uv_h = bar_frame.uv[3] * life_percent;
            
            renderer.draw_textured_quad(
                bg_x, fill_y,
                bar_w, fill_height,
                [uv_u, uv_v, uv_w, uv_h],
                [1.0, 1.0, 1.0, 1.0],
            );
        }
    }
}

/// Draw the jam bar using skin XML entity positions.
/// JAM_BAR: x="4" y="536", sprite is 191x12 pixels
/// Fill direction: left_to_right
/// The jam_bar sprite (green) is clipped relative to the empty jam bar background.
/// jam_progress is the raw jam_counter value (0-100+), normalized here.
pub fn draw_jam_bar(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    jam_counter: u32,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    
    let bar_x = ox + layout.jam_bar_x * sx;
    let bar_y = oy + layout.jam_bar_y * sy;
    let bar_w = 191.0 * sx;
    let bar_h = 12.0 * sy;
    
    // jam_counter is now 0-99 (resets at 100), so direct normalization
    let progress = (jam_counter as f32) / 100.0;
    
    // Draw the jam_bar sprite clipped to show only the filled portion
    // left_to_right: clip from the right side
    if let Some(bar_frame) = get_frame("jam_bar") {
        let fill_width = bar_w * progress;
        
        if fill_width > 0.5 {
            // Clip the UV to show only the left portion
            // UV is [u0, v0, u1, v1], so interpolate u0..u1 by progress
            let uv_u = bar_frame.uv[0];
            let uv_v = bar_frame.uv[1];
            let uv_u1 = uv_u + (bar_frame.uv[2] - uv_u) * progress;
            let uv_v1 = bar_frame.uv[3];
            
            renderer.draw_textured_quad(
                bar_x, bar_y,
                fill_width, bar_h,
                [uv_u, uv_v, uv_u1, uv_v1],
                [1.0, 1.0, 1.0, 1.0],
            );
        }
    }
}

/// Draw pill indicators. Pills increment every 15 consecutive Cools (max 5).
/// PILL_1 through PILL_5 entities are drawn at their XML entity positions.
pub fn draw_pills(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    pill_count: u32,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    
    for i in 0..pill_count.min(5) {
        if let Some(pos) = layout.pill_positions.get(i as usize) {
            let x = ox + pos.0 * sx;
            let y = oy + pos.1 * sy;
            
            if let Some(pill_frame) = get_frame("pill") {
                renderer.draw_textured_quad(
                    x, y,
                    pill_frame.width as f32 * sx,
                    pill_frame.height as f32 * sy,
                    pill_frame.uv,
                    [1.0, 1.0, 1.0, 1.0],
                );
            }
        }
    }
}

/// Draw judgment popup (COOL/GOOD/BAD/MISS text at judgment line).
/// EFFECT_JUDGMENT_COOL: x="30" y="280", sprite is 128x128 pixels
///
/// Animation: Pop-in from 50%→100% scale over first 100ms, then full size for 3s.
pub fn draw_judgment_popup(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    judgment: &PendingJudgment,
    current_time_ms: f64,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    let sprite_name = match judgment.judgment_type {
        JudgmentType::Cool => "judgment_cool",
        JudgmentType::Good => "judgment_good",
        JudgmentType::Bad => "judgment_bad",
        JudgmentType::Miss => "judgment_miss",
    };

    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;

    // Get animation values
    let scale = judgment.scale_factor(current_time_ms) as f32;
    let alpha = judgment.alpha(current_time_ms) as f32;
    let elapsed = current_time_ms - judgment.time_created_ms;

    // DEBUG: Log first few frames of each judgment to verify animation
    if elapsed < 150.0 && elapsed >= 0.0 {
        log::info!(
            "JUDGMENT {:?} elapsed={:.1}ms scale={:.3} alpha={:.3}",
            judgment.judgment_type, elapsed, scale, alpha
        );
    }

    if alpha < 0.01 {
        return;
    }

    // Base position from skin XML
    let base_x = ox + layout.judgment_popup_x * sx;
    let base_y = oy + layout.judgment_popup_y * sy;

    if let Some(frame) = get_frame(sprite_name) {
        // Scaled dimensions
        let w = frame.width as f32 * sx * scale;
        let h = frame.height as f32 * sy * scale;

        // Center the scaled sprite around the base position
        let x = base_x + (frame.width as f32 * sx - w) / 2.0;
        let y = base_y + (frame.height as f32 * sy - h) / 2.0;

        renderer.draw_textured_quad(x, y, w, h, frame.uv, [1.0, 1.0, 1.0, alpha]);
    }
}

/// Draw all judgment count numbers (COOL/GOOD/BAD/MISS counters).
/// Uses counter_number sprites (9x9 pixels)
pub fn draw_judgment_counts(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    cool: u32,
    good: u32,
    bad: u32,
    miss: u32,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
) {
    let (sx, sy) = skin_scale;
    let (ox, oy) = offset;
    
    // COOL count: COUNTER_JUDGMENT_COOL x="658" y="572"
    let cx = ox + layout.judgment_cool_x * sx;
    let cy = oy + layout.judgment_cool_y * sy;
    draw_number(renderer, &|d| get_frame(&format!("counter_number_{}", d)),
        cool, cx, cy, 1.0 * sx, sx, sy, [1.0, 1.0, 1.0, 1.0]);
    
    // GOOD count: COUNTER_JUDGMENT_GOOD x="728" y="572"
    let gx = ox + layout.judgment_good_x * sx;
    let gy = oy + layout.judgment_good_y * sy;
    draw_number(renderer, &|d| get_frame(&format!("counter_number_{}", d)),
        good, gx, gy, 1.0 * sx, sx, sy, [1.0, 1.0, 1.0, 1.0]);
    
    // BAD count: COUNTER_JUDGMENT_BAD x="658" y="581"
    let bx = ox + layout.judgment_bad_x * sx;
    let by = oy + layout.judgment_bad_y * sy;
    draw_number(renderer, &|d| get_frame(&format!("counter_number_{}", d)),
        bad, bx, by, 1.0 * sx, sx, sy, [1.0, 1.0, 1.0, 1.0]);
    
    // MISS count: COUNTER_JUDGMENT_MISS x="728" y="581"
    let mx = ox + layout.judgment_miss_x * sx;
    let my = oy + layout.judgment_miss_y * sy;
    draw_number(renderer, &|d| get_frame(&format!("counter_number_{}", d)),
        miss, mx, my, 1.0 * sx, sx, sy, [1.0, 1.0, 1.0, 1.0]);
}

/// Resolve a sprite frame, using animation if available.
fn resolve_frame(atlas: Option<&SkinAtlas>, sprite_id: &str, time_ms: f64) -> Option<AtlasFrame> {
    if let Some(a) = atlas {
        // Try animated sprite first (for sprites with multiple frames)
        if let Some(animated_frame) = a.get_frame_at_time(sprite_id, time_ms) {
            return Some(animated_frame);
        }
        // Fall back to single frame
        a.get_frame(sprite_id).copied()
    } else {
        None
    }
}

/// Draw all HUD elements in the correct order, with animation support.
/// Call this after rendering notes but before flushing.
pub fn render_hud(
    renderer: &mut TexturedRenderer,
    get_frame: &dyn Fn(&str) -> Option<AtlasFrame>,
    game_state: &GameState,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
    current_time_ms: f64,
) {
    render_hud_with_atlas(
        renderer, None, game_state, layout, skin_scale, offset, current_time_ms,
    )
}

/// Draw all HUD elements with atlas-based animation support.
pub fn render_hud_with_atlas(
    renderer: &mut TexturedRenderer,
    atlas: Option<&SkinAtlas>,
    game_state: &GameState,
    layout: &HudLayout,
    skin_scale: (f32, f32),
    offset: (f32, f32),
    current_time_ms: f64,
) {
    let stats = &game_state.stats;

    // Create a closure that resolves sprite IDs with animation support
    let get_frame = |sprite_id: &str| resolve_frame(atlas, sprite_id, current_time_ms);

    // 1. Draw static/background elements first
    draw_lifebar(renderer, &get_frame, stats.life_percent(), layout, skin_scale, offset);
    draw_jam_bar(renderer, &get_frame, stats.jam_counter, layout, skin_scale, offset);
    draw_pills(renderer, &get_frame, stats.pill_count, layout, skin_scale, offset);

    // 2. Draw counters
    draw_score(renderer, &get_frame, stats.score, layout, skin_scale, offset);
    draw_combo(
        renderer, &get_frame, stats.combo,
        game_state.combo_counter.current_y(),
        game_state.combo_counter.visible,
        layout, skin_scale, offset,
    );
    // Combo title: only visible for 2 seconds after combo changes
    if game_state.is_combo_title_visible() {
        draw_combo_title(
            renderer, &get_frame, stats.combo,
            game_state.combo_counter.current_y(),
            layout, skin_scale, offset,
        );
    }
    // Jam counter: only visible for 1 second after jam combo increases
    if game_state.is_jam_counter_visible() {
        draw_jam_counter(renderer, &get_frame, stats.jam_combo, layout, skin_scale, offset);
    }
    // Max combo: only visible for 2 seconds after max combo increases
    if game_state.is_max_combo_counter_visible() {
        draw_max_combo(renderer, &get_frame, stats.max_combo, layout, skin_scale, offset);
    }
    draw_judgment_counts(
        renderer, &get_frame,
        stats.cool_count, stats.good_count, stats.bad_count, stats.miss_count,
        layout, skin_scale, offset,
    );

    // 3. Draw active judgment popups
    for judgment in &game_state.pending_judgments {
        draw_judgment_popup(
            renderer, &get_frame,
            judgment, current_time_ms as f64,
            layout, skin_scale, offset,
        );
    }
}
