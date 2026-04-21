use open2jam_rs_core::game_options::{FpsLimiter, VSyncMode};

pub struct FrameLimiter {
    inner: crate::types::FrameLimiter,
}

impl FrameLimiter {
    pub fn new(target_fps: f64) -> Self {
        Self {
            inner: crate::types::FrameLimiter::new(target_fps),
        }
    }

    pub fn wait(&mut self) {
        self.inner.wait();
    }

    pub fn target_frame_duration_ns(&self) -> u64 {
        self.inner.target_frame_duration_ns()
    }
}

pub fn setup_frame_limiter(
    vsync_mode: VSyncMode,
    fps_limiter: FpsLimiter,
    monitor: Option<&winit::monitor::MonitorHandle>,
) -> Option<FrameLimiter> {
    if vsync_mode == VSyncMode::On {
        return None;
    }

    if fps_limiter == FpsLimiter::Unlimited {
        return None;
    }

    let base_hz = monitor
        .and_then(|monitor| {
            let modes: Vec<_> = monitor.video_modes().collect();
            modes
                .into_iter()
                .max_by_key(|vm| vm.refresh_rate_millihertz())
        })
        .map(|vm| vm.refresh_rate_millihertz() as f64 / 1000.0)
        .unwrap_or(60.0);

    let multiplier = match fps_limiter {
        FpsLimiter::X1 => 1.0,
        FpsLimiter::X2 => 2.0,
        FpsLimiter::X4 => 4.0,
        FpsLimiter::X8 => 8.0,
        FpsLimiter::Unlimited => 1.0,
    };

    let target_fps = base_hz * multiplier;
    log::info!(
        "Frame limiter: {:.0} Hz × {:.0} = {:.0} fps",
        base_hz,
        multiplier,
        target_fps
    );
    Some(FrameLimiter::new(target_fps))
}
