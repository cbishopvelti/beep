//! The Watch page: which way the watch is connected and how it's doing
//! (`watch_ble::status().sentence(..)`: through Gadgetbridge, or the direct
//! link's state), when it was last heard from and last synced
//! (`health::last_seen` with `Link::Watch` and `Link::Synced`,
//! `health::ago`), the watch app's version (`WatchState::version`), Sync
//! now (`Input::SyncNow`) with what the sync is doing, and the watch's
//! settings: Course (20 m / 15 m) and GPS speed, shown as the watch has
//! them (`WatchState`), which send `beeptest-set` (`Input::SetWatch`).

use super::App;

/// Draws the page.
pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {}
