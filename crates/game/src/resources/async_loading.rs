//! Async loading framework for parsing OJN/OJM/Skin assets in background threads.
//!
//! Reports progress via `LoadingProgress` delivered through `std::sync::mpsc`.

use std::sync::mpsc;
use std::thread;

/// Represents a single step of loading progress.
#[derive(Debug, Clone)]
pub struct LoadingProgress {
    /// Current step index (0-based).
    pub current_step: usize,
    /// Total number of loading steps.
    pub total_steps: usize,
    /// Human-readable description of the current step.
    pub message: String,
}

/// Manages background loading tasks with progress reporting.
pub struct Loader {
    /// Whether loading is currently in progress.
    loading: bool,
    /// The receiver for loading progress events.
    progress_rx: Option<mpsc::Receiver<LoadingProgress>>,
    /// The handle to the loading thread.
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl Loader {
    /// Create a new loader.
    pub fn new() -> Self {
        Self {
            loading: false,
            progress_rx: None,
            thread_handle: None,
        }
    }

    /// Start a loading task. Returns the progress receiver.
    pub fn start_loading<F>(
        &mut self,
        total_steps: usize,
        task: F,
    ) -> &mpsc::Receiver<LoadingProgress>
    where
        F: FnOnce(mpsc::Sender<LoadingProgress>) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            task(tx);
        });
        self.loading = true;
        self.progress_rx = Some(rx);
        self.thread_handle = Some(handle);
        self.progress_rx.as_ref().unwrap()
    }

    /// Poll the loading progress. Returns `true` if loading is still in progress.
    pub fn poll(&self) -> bool {
        self.loading
    }

    /// Check if loading has completed (thread has finished).
    pub fn is_complete(&self) -> bool {
        self.thread_handle
            .as_ref()
            .map(|h| h.is_finished())
            .unwrap_or(false)
    }

    /// Take the thread handle (consuming the loader state).
    pub fn take_handle(&mut self) -> Option<thread::JoinHandle<()>> {
        self.loading = false;
        self.thread_handle.take()
    }
}

impl Default for Loader {
    fn default() -> Self {
        Self::new()
    }
}
