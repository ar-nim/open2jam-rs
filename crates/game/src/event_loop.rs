use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;

use crate::app::App;

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.on_resumed(el)
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.on_about_to_wait(el)
    }

    fn window_event(
        &mut self,
        el: &ActiveEventLoop,
        wid: winit::window::WindowId,
        ev: WindowEvent,
    ) {
        self.on_window_event(el, wid, ev)
    }
}
