use winit::dpi::LogicalSize;
use winit::window::Fullscreen;

use open2jam_rs_core::orchestrator::AppMode;

pub fn compute_inner_size(
    mode: AppMode,
    display_width: u32,
    display_height: u32,
    monitor: Option<&winit::monitor::MonitorHandle>,
) -> LogicalSize<f64> {
    if mode == AppMode::Menu {
        if let Some(monitor) = monitor {
            let size = monitor.size();
            let scale = monitor.scale_factor();
            LogicalSize::new(
                (size.width as f64 * 0.65) / scale as f64,
                (size.height as f64 * 0.65) / scale as f64,
            )
        } else {
            LogicalSize::new(1280.0, 720.0)
        }
    } else {
        LogicalSize::new(display_width as f64, display_height as f64)
    }
}

pub fn build_window_attributes(
    inner_size: LogicalSize<f64>,
    mode: AppMode,
    fullscreen: bool,
) -> winit::window::WindowAttributes {
    let mut attrs = winit::window::WindowAttributes::default()
        .with_title("open2jam-rs")
        .with_visible(true)
        .with_resizable(true)
        .with_inner_size(inner_size);

    if mode == AppMode::Game && fullscreen {
        attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
    }

    attrs
}
