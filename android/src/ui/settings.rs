//! The Settings page: the theme, light, dark or the phone's
//! (`Store::theme`, applied each frame as blink's `Themer`, with
//! `platform::system_dark` and `platform::light_bars`); "Connect to the
//! watch directly, without Gadgetbridge" with the pop-up that picks the
//! watch (`watch_ble::settings` / `save`, `platform::keep_watch`,
//! `platform::scan_watches`, `watch_ble::found_watches`, the Nearby
//! devices permission), as blink's; and the app's version
//! (`CARGO_PKG_VERSION`).

use super::App;

/// Draws the page.
pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {}
