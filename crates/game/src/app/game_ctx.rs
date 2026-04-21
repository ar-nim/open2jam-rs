use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use crate::types::LoadingMessage;

pub struct LoadingState {
    receiver: mpsc::Receiver<LoadingMessage>,
    _thread: thread::JoinHandle<()>,
}

pub struct GameCtx {
    pub game_state: Option<crate::game_state::GameState>,
    pub loading_state: Option<LoadingState>,
    pub start_load_game_state: bool,
    pub game_start_instant: Option<Instant>,
    pub hybrid_clock_prev: Option<f64>,
    pub hybrid_clock_prev_delta: Option<f64>,
    pub hybrid_clock_frame_count: u64,
}

impl GameCtx {
    pub fn new() -> Self {
        Self {
            game_state: None,
            loading_state: None,
            start_load_game_state: false,
            game_start_instant: None,
            hybrid_clock_prev: None,
            hybrid_clock_prev_delta: None,
            hybrid_clock_frame_count: 0,
        }
    }

    pub fn take_game_state(&mut self) -> Option<crate::game_state::GameState> {
        self.game_state.take()
    }

    pub fn cleanup(&mut self) {
        self.game_state.take();
        self.loading_state.take();
    }

    pub fn poll_loading(&mut self) -> Option<crate::game_state::GameState> {
        if let Some(ref loading) = self.loading_state {
            if let Ok(msg) = loading.receiver.try_recv() {
                if let LoadingMessage::GameLoaded(result) = msg {
                    match result {
                        Ok(gs) => {
                            log::info!(
                                "Game loaded: {} ({:.1}ms spawn lead)",
                                gs.chart.header.title,
                                gs.spawn_lead_time_ms
                            );
                            let gs = Some(gs);
                            self.loading_state.take();
                            self.game_start_instant = Some(Instant::now());
                            self.hybrid_clock_prev = None;
                            self.hybrid_clock_prev_delta = None;
                            self.hybrid_clock_frame_count = 0;
                            return gs;
                        }
                        Err(e) => {
                            log::info!("Failed to load game state: {e:?}");
                        }
                    }
                    self.loading_state.take();
                }
            }
        }
        None
    }

    pub fn start_loading(
        &mut self,
        path: std::path::PathBuf,
        scroll_speed: f64,
        auto_play: bool,
        difficulty: open2jam_rs_core::Difficulty,
        skin_res: Option<open2jam_rs_parsers::xml::Resources>,
    ) {
        if self.loading_state.is_some() {
            return;
        }
        self.start_load_game_state = false;

        log::info!(
            "Starting background game state load from: {}",
            path.display()
        );

        let (tx, rx) = mpsc::channel();
        let thread_handle = thread::spawn(move || {
            let result = crate::game_state::GameState::load(
                &path,
                scroll_speed,
                auto_play,
                difficulty,
                skin_res.as_ref(),
            );
            let _ = tx.send(LoadingMessage::GameLoaded(result));
        });
        self.loading_state = Some(LoadingState {
            receiver: rx,
            _thread: thread_handle,
        });
    }

    pub fn set_start_load(&mut self, should_load: bool) {
        self.start_load_game_state = should_load;
    }
}
