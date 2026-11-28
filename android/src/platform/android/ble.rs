//! The direct Bluetooth link to the watch ([`crate::watch_ble`]) on Android,
//! ported from blink's `platform/android/ble.rs`: a GATT connection to the
//! watch's UART service, held while the app runs, by `WatchService` (a
//! foreground service) when it's closed.
//!
//! Android reports the connection to `WatchGatt`, which hands each callback
//! to `native*` functions here (`Java_com_cbishopvelti_beep_WatchGatt_*`, as
//! blink's: state, MTU, services, descriptor, written, changed). They set
//! the connection up (a bigger MTU, the services, notifications on), write
//! [`crate::watch_ble::CLEAR`], then after
//! [`crate::watch_ble::AFTER_CLEAR`] the clock
//! ([`crate::watch_ble::hello`]), and then dispatch
//! [`crate::sync::Input::LinkConnected`]. They write what's queued a piece
//! at a time ([`crate::watch_ble::Writer`]) and pass each line the watch
//! prints to [`crate::platform::from_console`]. The scan for watches is
//! `WatchScan` (`Java_com_cbishopvelti_beep_WatchScan_nativeFound`), and
//! `WatchService` reports itself as blink's does. Each status change goes
//! to [`crate::watch_ble::set_status`] and [`crate::platform::changed`].
//!
//! The platform engineer fills the bodies and adds the callbacks.

/// Whether the phone lets the app find and connect to the watch ("Nearby
/// devices" from Android 12; before that, location).
pub fn allowed() -> bool {
    todo!()
}

/// Asks for "Nearby devices" (Android 12 and later).
pub fn request_permission() {
    todo!()
}

/// Follows the setting: with the direct link on, keeps `WatchService`
/// running and the connection up (or being made); with it off, stops both.
pub fn keep() {
    todo!()
}

/// Looks for watches (Settings' picker), or stops.
pub fn scan(on: bool) {
    todo!()
}

/// Writes `text` to the watch's console, after whatever is waiting. False
/// if the link isn't up: nothing is kept for later, since a new connection
/// starts with a sync.
pub fn send(text: &str) -> bool {
    todo!()
}
