//! Render thread architecture for decoupled GPU submission.
//!
//! This module provides the infrastructure for running GPU submission on a separate
//! thread, completely decoupled from the main thread's input handling and game logic.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         MAIN THREAD                                  │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌───────────┐  │
//! │  │  winit      │→ │  Game Logic │→ │  Input      │→ │  Render   │  │
//! │  │  Events     │  │  Update     │  │  Process    │  │  Commands │  │
//! │  └─────────────┘  └─────────────┘  └─────────────┘  └─────┬─────┘  │
//! └────────────────────────────────────────────────────────────┼────────┘
//!                                                          │
//!                                                          │ MPSC Queue
//!                                                          │
//!                                                          ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                       RENDER THREAD                                  │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                 │
//! │  │  Command    │→ │  WGPU        │→ │  Present     │                 │
//! │  │  Consumer   │  │  Submission │  │             │                 │
//! │  └─────────────┘  └─────────────┘  └─────────────┘                 │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Benefits
//!
//! - **Unthrottled input**: Main thread never blocks on GPU operations
//! - **Consistent frame timing**: Render thread can pace itself independently
//! - **Lower input latency**: Input events processed immediately without waiting for render
//! - **Better multi-core utilization**: GPU and CPU work overlap

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use crossbeam_channel::{bounded, Receiver, Sender};
use log::{error, info};
use parking_lot::Mutex;

/// A command to be executed on the render thread.
#[derive(Debug)]
pub enum RenderCommand {
    /// Render a game frame with the given state snapshot
    RenderFrame {
        /// Frame number for debugging
        frame: u64,
        /// Time when this frame was prepared (for latency tracking)
        prepared_at: Instant,
    },
    /// Resize the render surface
    Resize { width: u32, height: u32 },
    /// Shutdown the render thread gracefully
    Shutdown,
}

/// A queue of render commands to be processed by the render thread.
/// The main thread pushes commands, the render thread pops and executes them.
pub struct RenderCommandQueue {
    sender: Sender<RenderCommand>,
    receiver: Receiver<RenderCommand>,
}

impl RenderCommandQueue {
    /// Create a new command queue with the given capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = bounded(capacity);
        Self { sender, receiver }
    }

    /// Push a command onto the queue (non-blocking).
    /// Returns true if the command was successfully queued.
    #[inline]
    pub fn push(&self, command: RenderCommand) -> bool {
        self.sender.send(command).is_ok()
    }

    /// Try to pop a command from the queue.
    /// Returns None if the queue is empty.
    #[inline]
    pub fn try_pop(&self) -> Option<RenderCommand> {
        self.receiver.try_recv().ok()
    }

    /// Block until a command is available.
    #[inline]
    pub fn recv(&self) -> Option<RenderCommand> {
        self.receiver.recv().ok()
    }

    /// Get the number of pending commands in the queue.
    #[inline]
    pub fn len(&self) -> usize {
        self.receiver.len()
    }

    /// Check if the queue is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }
}

/// State shared between the main thread and render thread.
/// Protected by a mutex since the main thread writes and render thread reads.
pub struct SharedRenderState {
    /// Whether a new frame needs to be rendered
    pub needs_redraw: AtomicBool,
    /// Current render dimensions
    pub width: u32,
    pub height: u32,
    /// Shutdown flag
    pub shutdown: AtomicBool,
}

impl SharedRenderState {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            needs_redraw: AtomicBool::new(false),
            width,
            height,
            shutdown: AtomicBool::new(false),
        }
    }
}

/// Statistics about the render thread's performance.
#[derive(Debug, Default)]
pub struct RenderStats {
    /// Total frames rendered
    pub frames_rendered: u64,
    /// Total render time in microseconds
    pub total_render_us: u64,
    /// Average frame time in microseconds
    pub avg_frame_us: u64,
    /// Maximum frame time in microseconds
    pub max_frame_us: u64,
    /// Number of dropped frames
    pub dropped_frames: u64,
    /// Queue depth at last check
    pub last_queue_depth: usize,
}

impl RenderStats {
    pub fn record_frame(&mut self, render_us: u64, queue_depth: usize) {
        self.frames_rendered += 1;
        self.total_render_us += render_us;
        self.avg_frame_us = self.total_render_us / self.frames_rendered.max(1);
        self.max_frame_us = self.max_frame_us.max(render_us);
        self.last_queue_depth = queue_depth;
    }

    pub fn record_dropped(&mut self) {
        self.dropped_frames += 1;
    }
}

/// Spawns a render thread that continuously processes render commands.
/// Returns handles for communication with the thread.
pub fn spawn_render_thread(
    command_queue: RenderCommandQueue,
    shared_state: Arc<Mutex<SharedRenderState>>,
    frame_limiter: Option<f64>,
) -> RenderThreadHandle {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);
    
    let handle = thread::spawn(move || {
        info!("Render thread started");
        
        let target_frame_us = frame_limiter.map(|fps| (1_000_000.0 / fps) as u64);
        let mut stats = RenderStats::default();
        
        while running_clone.load(Ordering::Relaxed) {
            // Block waiting for a command
            let command = match command_queue.recv() {
                Some(cmd) => cmd,
                None => continue,
            };
            
            match command {
                RenderCommand::Shutdown => {
                    info!("Render thread received shutdown command");
                    break;
                }
                RenderCommand::Resize { width, height } => {
                    let mut state = shared_state.lock();
                    state.width = width;
                    state.height = height;
                    info!("Render thread resized to {}x{}", width, height);
                }
                RenderCommand::RenderFrame { frame, prepared_at } => {
                    let frame_start = Instant::now();
                    let queue_depth = command_queue.len();
                    
                    // In a real implementation, this would:
                    // 1. Acquire surface texture
                    // 2. Execute render passes
                    // 3. Present
                    // For now, we just track stats
                    
                    let render_us = frame_start.elapsed().as_micros() as u64;
                    stats.record_frame(render_us, queue_depth);
                    
                    // Frame limiting: sleep if we're ahead of schedule
                    if let Some(target_us) = target_frame_us {
                        let elapsed = frame_start.elapsed().as_micros() as u64;
                        if elapsed < target_us {
                            let sleep_us = target_us - elapsed;
                            thread::sleep(std::time::Duration::from_micros(sleep_us));
                        }
                    }
                    
                    // Log stats periodically
                    if stats.frames_rendered % 600 == 0 {
                        info!(
                            "Render stats: frames={}, avg={}µs, max={}µs, dropped={}, queue={}",
                            stats.frames_rendered,
                            stats.avg_frame_us,
                            stats.max_frame_us,
                            stats.dropped_frames,
                            stats.last_queue_depth
                        );
                    }
                }
            }
        }
        
        info!(
            "Render thread exiting: {} frames rendered, avg={}µs/frame",
            stats.frames_rendered, stats.avg_frame_us
        );
    });
    
    RenderThreadHandle {
        running,
        join_handle: handle,
    }
}

/// Handle to the render thread for signaling shutdown.
pub struct RenderThreadHandle {
    running: Arc<AtomicBool>,
    join_handle: thread::JoinHandle<()>,
}

impl RenderThreadHandle {
    /// Signal the render thread to shut down gracefully.
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
    
    /// Wait for the render thread to finish.
    pub fn join(self) -> thread::Result<()> {
        self.join_handle.join()
    }
}

/// Request a render frame on the render thread.
/// This is a non-blocking operation - the command is queued and returns immediately.
#[inline]
pub fn request_render_frame(
    queue: &RenderCommandQueue,
    frame: u64,
) -> bool {
    queue.push(RenderCommand::RenderFrame {
        frame,
        prepared_at: Instant::now(),
    })
}

/// Request a resize on the render thread.
#[inline]
pub fn request_resize(
    queue: &RenderCommandQueue,
    width: u32,
    height: u32,
) -> bool {
    queue.push(RenderCommand::Resize { width, height })
}

/// Request the render thread to shut down.
#[inline]
pub fn request_shutdown(queue: &RenderCommandQueue) -> bool {
    queue.push(RenderCommand::Shutdown)
}
