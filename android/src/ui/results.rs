//! The Results page, the first: a card a result, newest first
//! (`Store::results`), with the date and time in the phone's time zone
//! (`crate::health::local`), the course, the result large as
//! `level.shuttle`, the distance, the time, and heart rate avg / max; a
//! "best" mark on the best of each course (`Store::bests`). Above the list,
//! a banner while a test runs on the watch (`WatchState::run`), a small
//! progress chart of results over time with a line per course
//! (`Entry::total_shuttles`), the sync button (`Input::SyncNow`) and what
//! the sync is doing (`Progress::words`). With no results, how to connect
//! the watch. Tapping a card opens it (`Page::Result`).

use super::App;

/// Draws the page.
pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {}
