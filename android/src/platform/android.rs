//! Android through JNI, as blink's `platform/android.rs`. Rust makes every
//! call into the platform; Java only appears where Android has to call into
//! the app by class name (see `app/src/main/java`): those stubs hand each
//! event straight to the `native*` functions at the bottom of this file and
//! in `ble.rs`.
//!
//! What it does:
//!
//! - [`init_activity`]: from `android_main`, the JavaVM and the activity
//!   (logging as "beep", the application context and its files directory).
//! - [`send_line`]: the Gadgetbridge broadcast (`com.banglejs.uart.tx`,
//!   extra `line`), or the direct link ([`ble::send`]) when that's on and up;
//!   while it isn't up, Gadgetbridge after all, as blink.
//! - [`share_file`]: `ACTION_SEND` with a `content://` uri from the app's
//!   `FileProvider` (the store's `exports/`) and the chooser.
//! - The theme and the bars ([`system_dark`], [`light_bars`] through
//!   `ThemeStub`), the safe area ([`status_bar_px`], [`nav_bar_px`]) and the
//!   density, as blink's.
//! - `WatchReceiver`'s broadcast:
//!   [`Java_com_cbishopvelti_beep_WatchReceiver_nativeWatch`] reads the
//!   extras named in [`crate::link::EXTRAS`] and passes them to
//!   [`crate::platform::from_watch`], after opening the store
//!   ([`crate::platform::open`]) from the context's files directory if the
//!   process has just started for it, inside `catch_unwind`.
//!
//! The platform engineer fills the bodies.

use jni::JNIEnv;
use jni::objects::JObject;
use std::path::Path;

pub mod ble;
pub mod ui_loop;

/// Called from `android_main` with the NativeActivity's VM and activity.
pub fn init_activity(vm: *mut std::ffi::c_void, activity: *mut std::ffi::c_void) {
    todo!()
}

/// Sends the watch `line`: over the direct link when it's on and up, else
/// through Gadgetbridge.
pub fn send_line(line: &str) {
    todo!()
}

/// Opens the share sheet for the file at `path` (in the store's
/// `exports/`), of type `mime`.
pub fn share_file(path: &Path, mime: &str) -> Result<(), String> {
    todo!()
}

/// Follows the direct-link setting: [`ble::keep`].
pub fn keep_watch() {
    todo!()
}

/// Whether the phone is in dark mode.
pub fn system_dark() -> Option<bool> {
    todo!()
}

/// Dark icons on the status and navigation bars for a light theme.
pub fn light_bars(light: bool) {
    todo!()
}

/// The status bar's height, pixels.
pub fn status_bar_px() -> f32 {
    todo!()
}

/// The navigation bar's height, pixels.
pub fn nav_bar_px() -> f32 {
    todo!()
}

/// The display's density (pixels per point).
pub fn density() -> Option<f32> {
    todo!()
}

/// Gadgetbridge turned a watch intent message into this broadcast (the
/// watch link's "Watch to phone").
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchReceiver_nativeWatch<'l>(
    env: JNIEnv<'l>,
    _this: JObject<'l>,
    context: JObject<'l>,
    intent: JObject<'l>,
) {
    todo!()
}
