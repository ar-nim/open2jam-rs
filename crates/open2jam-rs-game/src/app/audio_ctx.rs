pub struct AudioCtx {
    pub manager: crate::audio::manager::AudioManager,
}

impl AudioCtx {
    pub fn new() -> Self {
        let manager = crate::audio::manager::AudioManager::new();
        if manager.is_active() {
            log::info!("Audio manager active.");
        } else {
            log::info!("Audio manager failed to initialise (running headless).");
        }
        Self { manager }
    }

    pub fn is_active(&self) -> bool {
        self.manager.is_active()
    }

    pub fn play(&mut self) {
        self.manager.play();
    }

    pub fn state(&self) -> &std::sync::Arc<crate::audio::manager::AudioState> {
        self.manager.state()
    }

    pub fn time_reader(&self) -> crate::audio::AudioTimeReader {
        self.manager.time_reader()
    }

    pub fn validate_hybrid_clock(
        &self,
        base_instant: std::time::Instant,
        max_jitter_ms: f64,
        prev_time: &mut Option<f64>,
        prev_delta: &mut Option<f64>,
        frame_count: &mut u64,
    ) -> (f64, f64, bool) {
        self.manager.validate_hybrid_clock(
            base_instant,
            max_jitter_ms,
            prev_time,
            prev_delta,
            frame_count,
        )
    }

    pub fn mixer(
        &mut self,
    ) -> Option<&mut oddio::MixerControl<crate::audio::manager::StereoFrame>> {
        self.manager.mixer()
    }

    pub fn callback_cpu_usage(&self) -> (u32, u32, u32, f64) {
        self.manager.callback_cpu_usage()
    }

    pub fn get_hybrid_time_ms(&self, base_instant: std::time::Instant) -> f64 {
        self.manager.get_hybrid_time_ms(base_instant)
    }

    pub fn push_bgm_command(
        &mut self,
        command: crate::audio::bgm_signal::BgmCommand,
    ) -> Result<(), rtrb::PushError<crate::audio::bgm_signal::BgmCommand>> {
        self.manager.push_bgm_command(command)
    }
}
