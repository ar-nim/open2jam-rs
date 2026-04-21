use crate::game_state::GameState;
use crate::gameplay;
use crate::gpu::GpuResources;
use crate::render::hud::render_hud_with_atlas;
use crate::render::textured_renderer::BlendMode;
use crate::types::RenderMetrics;

pub fn render_game(
    gpu: &mut GpuResources,
    state: &GameState,
    config_width: u32,
    config_height: u32,
    metrics: &RenderMetrics,
    #[allow(unused_variables)] audio_mgr: Option<&mut crate::audio::manager::AudioManager>,
    #[allow(unused_variables)] game_start_instant: Option<std::time::Instant>,
    #[allow(unused_variables)] hybrid_clock_prev: &mut Option<f64>,
    #[allow(unused_variables)] hybrid_clock_prev_delta: &mut Option<f64>,
    #[allow(unused_variables)] hybrid_clock_frame_count: &mut u64,
) -> bool {
    let config_width = config_width as f32;
    let config_height = config_height as f32;
    let skin_scale_x = metrics.scale.x;
    let skin_scale_y = metrics.scale.y;
    let offset_x = metrics.offset.x;
    let offset_y = metrics.offset.y;
    let skin_judgment_line_y = metrics.judgment_line_y;

    let render_time = state.clock.render_time();
    let _bpm = state.clock.bpm();
    let judgment_line_y = skin_judgment_line_y as f64;
    let measure_basis = judgment_line_y;

    if let Some(atlas) = &gpu.atlas {
        if let Some(skin_res) = &gpu.skin {
            if let Some(skin) = skin_res.get_skin("o2jam") {
                if state.is_rendering {
                    if let Some(measure_frame) = atlas
                        .get_frame_at_time("measure_mark", render_time as f64)
                        .or_else(|| atlas.get_frame("measure_mark").copied())
                    {
                        let mw = measure_frame.width as f32 * skin_scale_x;
                        let mh = measure_frame.height as f32 * skin_scale_y;
                        let mark_skin_x: f32 = 5.0;
                        let mx = offset_x + mark_skin_x * skin_scale_x;

                        for event in &state.chart.events {
                            if let open2jam_rs_parsers::TimedEvent::Measure(ev) = event {
                                if render_time > ev.time_ms {
                                    continue;
                                }
                                let y = gameplay::scroll::note_y_position_bpm_aware(
                                    render_time,
                                    ev.time_ms,
                                    &state.timing,
                                    judgment_line_y,
                                    measure_basis,
                                    state.scroll_speed,
                                );
                                let screen_y = offset_y + y as f32 * skin_scale_y - mh / 2.0;
                                if screen_y > -mh && screen_y < config_height + mh {
                                    gpu.textured_renderer.draw_textured_quad(
                                        mx,
                                        screen_y,
                                        mw,
                                        mh,
                                        measure_frame.uv,
                                        [1.0, 1.0, 1.0, 0.5],
                                    );
                                }
                            }
                        }
                    }

                    for lane in 0..7 {
                        if state.pressed_lanes[lane] {
                            for (sprite_id, x_pos, y_pos) in
                                &state.note_prefabs.pressed_lane_effects[lane]
                            {
                                if let Some(pressed_frame) = atlas
                                    .get_frame_at_time(sprite_id, render_time as f64)
                                    .or_else(|| atlas.get_frame(sprite_id).copied())
                                {
                                    let sprite_w = pressed_frame.width as f32 * skin_scale_x;
                                    let sprite_h = pressed_frame.height as f32 * skin_scale_y;
                                    let x = offset_x + *x_pos as f32 * skin_scale_x;
                                    let y = offset_y + *y_pos as f32 * skin_scale_y;
                                    gpu.textured_renderer.draw_textured_quad(
                                        x,
                                        y,
                                        sprite_w,
                                        sprite_h,
                                        pressed_frame.uv,
                                        [1.0, 1.0, 1.0, 0.6],
                                    );
                                }
                            }
                        }
                    }

                    for note in &state.active_notes {
                        let y = gameplay::scroll::note_y_position_bpm_aware(
                            render_time,
                            note.target_time_ms,
                            &state.timing,
                            judgment_line_y,
                            measure_basis,
                            state.scroll_speed,
                        );

                        let lane_prefab = &state.note_prefabs.lanes[note.lane];
                        let lane_x = offset_x + lane_prefab.x as f32 * skin_scale_x;

                        let head_frame_name =
                            lane_prefab
                                .sprite_id
                                .as_deref()
                                .unwrap_or_else(|| match note.lane {
                                    0 | 1 | 2 => "head_note_white",
                                    3 => "head_note_blue",
                                    _ => "head_note_yellow",
                                });

                        let head_frame = atlas
                            .get_frame_at_time(head_frame_name, render_time as f64)
                            .or_else(|| atlas.get_frame(head_frame_name).copied());
                        if let Some(head_frame) = head_frame {
                            let note_w = head_frame.width as f32 * skin_scale_x;
                            let note_h = head_frame.height as f32 * skin_scale_y;
                            let x = lane_x;
                            let y = offset_y + y as f32 * skin_scale_y - note_h / 2.0;
                            gpu.textured_renderer.draw_textured_quad(
                                x,
                                y,
                                note_w,
                                note_h,
                                head_frame.uv,
                                [1.0, 1.0, 1.0, 1.0],
                            );
                        }
                    }

                    for long_note in &state.active_long_notes {
                        let lane_prefab = &state.note_prefabs.lanes[long_note.lane];
                        let lane_x = offset_x + lane_prefab.x as f32 * skin_scale_x;

                        let head_y = gameplay::scroll::note_y_position_bpm_aware(
                            render_time,
                            long_note.head_time_ms,
                            &state.timing,
                            judgment_line_y,
                            measure_basis,
                            state.scroll_speed,
                        );

                        let tail_y = gameplay::scroll::note_y_position_bpm_aware(
                            render_time,
                            long_note.tail_time_ms,
                            &state.timing,
                            judgment_line_y,
                            measure_basis,
                            state.scroll_speed,
                        );

                        let head_frame_name = lane_prefab
                            .head_sprite
                            .as_deref()
                            .or(lane_prefab.sprite_id.as_deref())
                            .unwrap_or_else(|| match long_note.lane {
                                0 | 1 | 2 => "head_note_white",
                                3 => "head_note_blue",
                                _ => "head_note_yellow",
                            });

                        let body_frame_name = lane_prefab
                            .body_sprite
                            .as_deref()
                            .or(lane_prefab.sprite_id.as_deref())
                            .unwrap_or_else(|| match long_note.lane {
                                0 | 1 | 2 => "body_note_white",
                                3 => "body_note_blue",
                                _ => "body_note_yellow",
                            });

                        let tail_frame_name = lane_prefab
                            .tail_sprite
                            .as_deref()
                            .or(lane_prefab.sprite_id.as_deref())
                            .unwrap_or(head_frame_name);

                        if let (Some(head_frame), Some(body_frame), Some(tail_frame)) = (
                            atlas.get_frame(head_frame_name),
                            atlas.get_frame(body_frame_name),
                            atlas.get_frame(tail_frame_name),
                        ) {
                            let note_w = head_frame.width as f32 * skin_scale_x;
                            let head_h = head_frame.height as f32 * skin_scale_y;
                            let tail_h = tail_frame.height as f32 * skin_scale_y;

                            let judgment_line_screen_y =
                                offset_y + judgment_line_y as f32 * skin_scale_y;
                            let head_unclamped_screen_y =
                                offset_y + head_y as f32 * skin_scale_y - head_h / 2.0;
                            let tail_screen_y =
                                offset_y + tail_y as f32 * skin_scale_y - tail_h / 2.0;

                            let head_past_judgment = head_y > judgment_line_y;

                            let effective_head_y = if head_past_judgment {
                                judgment_line_y
                            } else {
                                head_y
                            };
                            let effective_head_screen_y =
                                offset_y + effective_head_y as f32 * skin_scale_y;

                            let body_top = tail_screen_y.min(effective_head_screen_y);
                            let body_bottom = tail_screen_y.max(effective_head_screen_y);
                            let body_pixel_height = (body_bottom - body_top).max(0.0);

                            if body_pixel_height > 0.5 {
                                let body_x = lane_x;
                                let body_y = body_top;

                                gpu.textured_renderer.draw_textured_quad(
                                    body_x,
                                    body_y,
                                    note_w,
                                    body_pixel_height,
                                    body_frame.uv,
                                    [1.0, 1.0, 1.0, 1.0],
                                );

                                if !head_past_judgment || tail_y < judgment_line_y {
                                    gpu.textured_renderer.draw_textured_quad(
                                        lane_x,
                                        tail_screen_y,
                                        note_w,
                                        tail_h,
                                        tail_frame.uv,
                                        [1.0, 1.0, 1.0, 1.0],
                                    );
                                }

                                if !head_past_judgment {
                                    gpu.textured_renderer.draw_textured_quad(
                                        lane_x,
                                        head_unclamped_screen_y - head_h,
                                        note_w,
                                        head_h,
                                        head_frame.uv,
                                        [1.0, 1.0, 1.0, 1.0],
                                    );
                                }
                            }
                        }
                    }

                    if let Some(skin) = gpu.skin.as_ref().and_then(|s| s.get_skin("o2jam")) {
                        for entity in &skin.entities {
                            let sprite_id = match &entity.sprite {
                                Some(s) => s,
                                None => continue,
                            };
                            let first = sprite_id.split(',').next().unwrap_or(sprite_id).trim();
                            if first != "static_keyboard" {
                                continue;
                            }
                            if let Some(frame) = atlas.get_frame(first) {
                                let fw = frame.width as f32 * skin_scale_x;
                                let fh = frame.height as f32 * skin_scale_y;
                                let fx = offset_x + entity.x as f32 * skin_scale_x;
                                let fy = offset_y + entity.y as f32 * skin_scale_y;
                                gpu.textured_renderer.draw_textured_quad(
                                    fx,
                                    fy,
                                    fw,
                                    fh,
                                    frame.uv,
                                    [1.0, 1.0, 1.0, 1.0],
                                );
                            }
                        }
                    }

                    for lane in 0..7 {
                        if state.pressed_lanes[lane] {
                            for (sprite_id, x_pos, y_pos) in
                                &state.note_prefabs.pressed_keyboard_overlays[lane]
                            {
                                if let Some(pressed_frame) = atlas
                                    .get_frame_at_time(sprite_id, render_time as f64)
                                    .or_else(|| atlas.get_frame(sprite_id).copied())
                                {
                                    let sprite_w = pressed_frame.width as f32 * skin_scale_x;
                                    let sprite_h = pressed_frame.height as f32 * skin_scale_y;
                                    let x = offset_x + *x_pos as f32 * skin_scale_x;
                                    let y = offset_y + *y_pos as f32 * skin_scale_y;
                                    gpu.textured_renderer.draw_textured_quad(
                                        x,
                                        y,
                                        sprite_w,
                                        sprite_h,
                                        pressed_frame.uv,
                                        [1.0, 1.0, 1.0, 0.6],
                                    );
                                }
                            }
                        }
                    }
                }

                if !state.is_rendering {
                    for lane in 0..7 {
                        if state.pressed_lanes[lane] {
                            for (sprite_id, x_pos, y_pos) in
                                &state.note_prefabs.pressed_lane_effects[lane]
                            {
                                if let Some(pressed_frame) = atlas
                                    .get_frame_at_time(sprite_id, render_time as f64)
                                    .or_else(|| atlas.get_frame(sprite_id).copied())
                                {
                                    let sprite_w = pressed_frame.width as f32 * skin_scale_x;
                                    let sprite_h = pressed_frame.height as f32 * skin_scale_y;
                                    let x = offset_x + *x_pos as f32 * skin_scale_x;
                                    let y = offset_y + *y_pos as f32 * skin_scale_y;
                                    gpu.textured_renderer.draw_textured_quad(
                                        x,
                                        y,
                                        sprite_w,
                                        sprite_h,
                                        pressed_frame.uv,
                                        [1.0, 1.0, 1.0, 0.6],
                                    );
                                }
                            }
                            for (sprite_id, x_pos, y_pos) in
                                &state.note_prefabs.pressed_keyboard_overlays[lane]
                            {
                                if let Some(pressed_frame) = atlas
                                    .get_frame_at_time(sprite_id, render_time as f64)
                                    .or_else(|| atlas.get_frame(sprite_id).copied())
                                {
                                    let sprite_w = pressed_frame.width as f32 * skin_scale_x;
                                    let sprite_h = pressed_frame.height as f32 * skin_scale_y;
                                    let x = offset_x + *x_pos as f32 * skin_scale_x;
                                    let y = offset_y + *y_pos as f32 * skin_scale_y;
                                    gpu.textured_renderer.draw_textured_quad(
                                        x,
                                        y,
                                        sprite_w,
                                        sprite_h,
                                        pressed_frame.uv,
                                        [1.0, 1.0, 1.0, 0.6],
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if state.is_rendering {
        if let Some(atlas) = &gpu.atlas {
            let render_time_f64 = state.clock.render_time() as f64;

            if let Some(ref click_sprite) = state.effect_click_sprite {
                if let Some(anim) = atlas.animations.get(click_sprite) {
                    let frame_speed_ms = anim.frame_speed_ms;
                    for effect in &state.note_click_effects {
                        let frame_idx =
                            effect.frame_index(render_time_f64, frame_speed_ms, anim.frame_count);
                        let atlas_id = format!("{}_{}", click_sprite, frame_idx);
                        if let Some(frame) = atlas.get_frame(&atlas_id) {
                            let lane_prefab = &state.note_prefabs.lanes[effect.lane];
                            let note_sprite =
                                lane_prefab.sprite_id.as_deref().unwrap_or_else(|| {
                                    match effect.lane {
                                        0 | 1 | 2 => "head_note_white",
                                        3 => "head_note_blue",
                                        _ => "head_note_yellow",
                                    }
                                });
                            let note_width = atlas
                                .get_frame(note_sprite)
                                .map(|f| f.width as f32)
                                .unwrap_or(50.0);
                            let effect_x = offset_x
                                + lane_prefab.x as f32 * skin_scale_x
                                + (note_width * skin_scale_x / 2.0)
                                - (frame.width as f32 * skin_scale_x / 2.0);
                            let effect_y = offset_y + skin_judgment_line_y as f32 * skin_scale_y
                                - (frame.height as f32 * skin_scale_y / 2.0);

                            gpu.textured_renderer.draw_textured_quad(
                                effect_x,
                                effect_y,
                                frame.width as f32 * skin_scale_x,
                                frame.height as f32 * skin_scale_y,
                                frame.uv,
                                [1.0, 1.0, 1.0, 1.0],
                            );
                        }
                    }
                }
            }

            if let Some(ref flare_sprite) = state.effect_longflare_sprite {
                if let Some(anim) = atlas.animations.get(flare_sprite) {
                    let frame_speed_ms = anim.frame_speed_ms;
                    for effect in &state.long_flare_effects {
                        let frame_idx =
                            effect.frame_index(render_time_f64, frame_speed_ms, anim.frame_count);
                        let atlas_id = format!("{}_{}", flare_sprite, frame_idx);
                        if let Some(frame) = atlas.get_frame(&atlas_id) {
                            let lane_prefab = &state.note_prefabs.lanes[effect.lane];
                            let note_sprite =
                                lane_prefab.sprite_id.as_deref().unwrap_or_else(|| {
                                    match effect.lane {
                                        0 | 1 | 2 => "head_note_white",
                                        3 => "head_note_blue",
                                        _ => "head_note_yellow",
                                    }
                                });
                            let note_width = atlas
                                .get_frame(note_sprite)
                                .map(|f| f.width as f32)
                                .unwrap_or(50.0);
                            let flare_x = offset_x
                                + lane_prefab.x as f32 * skin_scale_x
                                + (note_width * skin_scale_x / 2.0)
                                - (frame.width as f32 * skin_scale_x / 2.0);
                            let flare_y = offset_y + state.effect_longflare_y as f32 * skin_scale_y;

                            gpu.textured_renderer.set_blend_mode(BlendMode::Additive);
                            gpu.textured_renderer.draw_textured_quad(
                                flare_x,
                                flare_y,
                                frame.width as f32 * skin_scale_x,
                                frame.height as f32 * skin_scale_y,
                                frame.uv,
                                [1.0, 1.0, 1.0, 1.0],
                            );
                            gpu.textured_renderer.set_blend_mode(BlendMode::Alpha);
                        }
                    }
                }
            }
        }
    }

    if let Some(ref layout) = gpu.hud_layout {
        let atlas_ref = gpu.atlas.as_ref();
        render_hud_with_atlas(
            &mut gpu.textured_renderer,
            atlas_ref,
            state,
            layout,
            (skin_scale_x, skin_scale_y),
            (offset_x, offset_y),
            render_time as f64,
        );
    } else {
        log::warn!("HUD layout not loaded, skipping HUD render");
    }

    state.is_song_ended()
}
