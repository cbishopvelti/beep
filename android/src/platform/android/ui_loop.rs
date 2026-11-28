//! The UI's event loop, which sleeps while the app is in the background,
//! copied from blink's (`src/platform/android/ui_loop.rs`).
//!
//! Leaving the app takes its window away (winit's `suspended`), but eframe
//! 0.33 doesn't notice: a repaint that was due (the open app asks for one
//! every second) finds no window, and eframe has already switched the loop
//! to `ControlFlow::Poll` for it, which nothing switches back. The loop then
//! polls without waiting, a whole CPU core, until Android freezes or kills
//! the app: on 27 Nov that was blink's 11 hours of CPU and a third of the
//! phone's battery in a day. Later eframes (0.34 to 0.37) leave a
//! `WaitUntil` in the past instead, which spins the same way.
//!
//! [`Sleepy`] passes every event to eframe and, while there's no window, has
//! the loop wait for the next event instead. Coming back (`resumed`) gives
//! eframe its window and the loop to run as before, and syncs
//! ([`crate::sync::Input::Opened`]): the app came back to the front. Each
//! `resumed` and `suspended` also tells the direct link
//! ([`super::ble::in_front`], [`super::ble::to_back`]), which starts a
//! scan for the watch again when one started in the background.

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

pub struct Sleepy<A> {
    app: A,
    /// Between `suspended` and `resumed`: there's no window to draw.
    away: bool,
}

impl<A> Sleepy<A> {
    pub fn new(app: A) -> Self {
        Self { app, away: false }
    }
}

impl<T: 'static, A: ApplicationHandler<T>> ApplicationHandler<T> for Sleepy<A> {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.app.new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // The first `resumed` is the app starting, where the UI's first
        // frame syncs; later ones are coming back to the front.
        let back = std::mem::replace(&mut self.away, false);
        // Before the app's first frame, so a scan the link starts then
        // counts as started in front.
        super::ble::in_front();
        self.app.resumed(event_loop);
        if back {
            crate::platform::dispatch(crate::sync::Input::Opened);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: T) {
        self.app.user_event(event_loop, event);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(event_loop, id, event);
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.app.device_event(event_loop, id, event);
    }

    /// The last call before the loop waits: whatever eframe asked for,
    /// with no window it waits for the next event.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.app.about_to_wait(event_loop);
        if self.away {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.away = true;
        super::ble::to_back();
        self.app.suspended(event_loop);
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.app.exiting(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.app.memory_warning(event_loop);
    }
}
