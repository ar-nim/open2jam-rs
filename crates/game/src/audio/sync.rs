//! Thread-safe audio synchronization primitives.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// AtomicF64 wrapper using AtomicU64 bits for lock-free f64 storage.
pub struct AtomicF64 {
    bits: AtomicU64,
}

impl Clone for AtomicF64 {
    fn clone(&self) -> Self {
        Self {
            bits: AtomicU64::new(self.bits.load(Ordering::Relaxed)),
        }
    }
}

impl AtomicF64 {
    #[inline]
    pub fn new(val: f64) -> Self {
        Self { bits: AtomicU64::new(val.to_bits()) }
    }
    
    #[inline]
    pub fn load(&self, order: Ordering) -> f64 {
        f64::from_bits(self.bits.load(order))
    }
    
    #[inline]
    pub fn store(&self, val: f64, order: Ordering) {
        self.bits.store(val.to_bits(), order);
    }
}

/// Thread-safe audio time source. Clone via Arc for sharing.
#[derive(Clone)]
pub struct AudioTimeSource {
    audio_time_ms: Arc<AtomicF64>,
    callback_os_time_ns: Arc<AtomicU64>,
    callback_count: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU32>,
    stream_start: Instant,
}

impl AudioTimeSource {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            audio_time_ms: Arc::new(AtomicF64::new(0.0)),
            callback_os_time_ns: Arc::new(AtomicU64::new(0)),
            callback_count: Arc::new(AtomicU64::new(0)),
            sample_rate: Arc::new(AtomicU32::new(sample_rate)),
            stream_start: Instant::now(),
        }
    }

    #[inline]
    pub fn record_callback(&self, samples_played: u64) {
        let rate = self.sample_rate.load(Ordering::Relaxed) as f64;
        let audio_ms = samples_played as f64 / rate * 1000.0;
        self.audio_time_ms.store(audio_ms, Ordering::Relaxed);
        let os_ns = self.stream_start.elapsed().as_nanos() as u64;
        self.callback_os_time_ns.store(os_ns, Ordering::Relaxed);
        self.callback_count.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_sample_rate(&self, sample_rate: u32) {
        self.sample_rate.store(sample_rate, Ordering::Relaxed);
    }

    pub fn reader(&self) -> AudioTimeReader {
        AudioTimeReader {
            audio_time_ms: Arc::clone(&self.audio_time_ms),
            callback_os_time_ns: Arc::clone(&self.callback_os_time_ns),
            callback_count: Arc::clone(&self.callback_count),
            sample_rate: Arc::clone(&self.sample_rate),
            stream_start: self.stream_start,
        }
    }
}

/// Lock-free reader for audio time.
#[derive(Clone)]
pub struct AudioTimeReader {
    audio_time_ms: Arc<AtomicF64>,
    callback_os_time_ns: Arc<AtomicU64>,
    callback_count: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU32>,
    stream_start: Instant,
}

impl AudioTimeReader {
    #[inline]
    pub fn audio_time_ms(&self) -> f64 {
        self.audio_time_ms.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn hybrid_time_ms(&self) -> f64 {
        let audio_ms = self.audio_time_ms.load(Ordering::Relaxed);
        let callback_ns = self.callback_os_time_ns.load(Ordering::Relaxed);
        let now_ns = self.stream_start.elapsed().as_nanos() as u64;
        let wall_offset_ms = (now_ns.saturating_sub(callback_ns)) as f64 / 1_000_000.0;
        audio_ms + wall_offset_ms
    }

    #[inline]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn ms_to_samples(&self, ms: f64) -> u32 {
        let rate = self.sample_rate.load(Ordering::Relaxed) as f64;
        (ms * rate / 1000.0).max(0.0) as u32
    }

    #[inline]
    pub fn samples_to_ms(&self, samples: u32) -> f64 {
        let rate = self.sample_rate.load(Ordering::Relaxed) as f64;
        samples as f64 / rate * 1000.0
    }
}

// ============================================================================
// Platform-specific thread priority elevation
// ============================================================================

pub fn elevate_audio_thread() {
    #[cfg(target_os = "linux")]
    {
        elevate_linux_audio_thread();
    }
    #[cfg(target_os = "macos")]
    {
        elevate_macos_audio_thread();
    }
    #[cfg(target_os = "windows")]
    {
        elevate_windows_audio_thread();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        log::warn!("Thread priority elevation not supported on this platform");
    }
}

#[cfg(target_os = "linux")]
fn elevate_linux_audio_thread() {
    use std::ffi::c_int;
    
    const SCHED_FIFO: c_int = 1;
    const SCHED_RR: c_int = 2;
    
    let param = libc::sched_param { sched_priority: 50 };
    
    unsafe {
        if libc::sched_setscheduler(0, SCHED_FIFO, &param) == 0 {
            log::info!("Audio thread: SCHED_FIFO priority 50");
        } else if libc::sched_setscheduler(0, SCHED_RR, &param) == 0 {
            log::info!("Audio thread: SCHED_RR priority 50");
        } else {
            log::warn!("Audio thread: Failed to set real-time scheduling (need root)");
        }
    }
}

#[cfg(target_os = "macos")]
fn elevate_macos_audio_thread() {
    use std::ffi::c_int;
    const PRIO_DARWIN_THREAD: c_int = 3;
    unsafe {
        if libc::setpriority(PRIO_DARWIN_THREAD, 0, -10) == 0 {
            log::info!("Audio thread: macOS priority -10");
        }
    }
}

#[cfg(target_os = "windows")]
fn elevate_windows_audio_thread() {
    const THREAD_PRIORITY_TIME_CRITICAL: i32 = 15;
    unsafe {
        if libc::SetThreadPriority(libc::GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) != 0 {
            log::info!("Audio thread: Windows THREAD_PRIORITY_TIME_CRITICAL");
        }
    }
}

#[cfg(target_os = "windows")]
mod libc {
    pub use std::os::raw::c_int;
    extern "system" {
        pub fn GetCurrentThread() -> *mut std::ffi::c_void;
        pub fn SetThreadPriority(hThread: *mut std::ffi::c_void, nPriority: c_int) -> c_int;
    }
}

#[cfg(target_os = "linux")]
mod libc {
    pub use std::os::raw::c_int;
    #[repr(C)]
    pub struct sched_param { pub sched_priority: c_int }
    extern "C" {
        pub fn sched_setscheduler(pid: c_int, policy: c_int, param: *const sched_param) -> c_int;
        pub fn setpriority(which: c_int, who: c_int, prio: c_int) -> c_int;
    }
}

#[cfg(target_os = "macos")]
mod libc {
    pub use std::os::raw::c_int;
    extern "C" {
        pub fn setpriority(which: c_int, who: c_int, prio: c_int) -> c_int;
    }
}
