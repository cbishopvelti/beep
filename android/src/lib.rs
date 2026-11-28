//! Beep Test on Android: an egui app that keeps the phone and the watch in
//! sync. Results and heart rate logs come off the watch by themselves, a
//! delete on either side deletes on both, and the watch's settings can be
//! changed from the phone. Everything is kept in plain files in the app's
//! files directory ([`store`]).
//!
//! How the parts fit:
//!
//! - [`link`] is the watch link's wire format: the lines the phone sends and
//!   the events the watch sends back.
//! - [`sync`] is the engine: given the store and an input (the app opened, a
//!   watch event, a tick), it updates the store and says which lines to send.
//!   It is pure, so it is all tested on the desktop.
//! - [`platform`] runs every input through the engine under one lock
//!   ([`platform::dispatch`]) and sends the lines, through Gadgetbridge or
//!   the direct link ([`watch_ble`]). On Android that is JNI
//!   (`platform/android.rs`); on the desktop it logs them.
//! - [`ui`] draws the pages from the store and the engine's progress, and
//!   redraws when [`platform::generation`] goes up.
//!
//! The same code runs on the desktop (`cargo run`) for development; Android
//! enters through `android_main`, which the NativeActivity calls.

pub mod beeplog;
pub mod health;
pub mod link;
pub mod platform;
pub mod store;
pub mod sync;
pub mod tables;
pub mod ui;
pub mod watch_ble;

use std::path::Path;

/// Opens the store under `data_dir` (shared with the broadcast receivers,
/// [`platform::open`]) and builds the app.
pub fn app(data_dir: &Path) -> std::io::Result<ui::App> {
    Ok(ui::App::new(platform::open(data_dir)?))
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(android: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid as _;
    platform::android::init_activity(android.vm_as_ptr(), android.activity_as_ptr());
    // Context.getFilesDir(): private to the app, kept across updates.
    let dir = android
        .internal_data_path()
        .expect("the app has an internal data path");
    // eframe on the app's own event loop, so the loop can sleep while the
    // app is in the background (`ui_loop`).
    let event_loop = match winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event()
        .with_android_app(android)
        .build()
    {
        Ok(event_loop) => event_loop,
        Err(e) => return log::error!("no event loop: {e}"),
    };
    let app = eframe::create_native(
        "Beep Test",
        eframe::NativeOptions::default(),
        Box::new(move |_cc| Ok(Box::new(app(&dir)?))),
        &event_loop,
    );
    let mut app = platform::android::ui_loop::Sleepy::new(app);
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("Beep Test exited: {e}");
    }
}
