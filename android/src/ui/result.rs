//! A result's page (from a card on Results): the summary as on its card;
//! the heart rate over the test (`beeplog::hr_series`) with the levels
//! marked; a row per level (`beeplog::level_rows`: time, average and max
//! heart rate, GPS speed when recorded); Share TCX and Share CSV
//! (`Store::export`, then `platform::share_file`), with the option to leave
//! out low-confidence heart rate readings (below 50%) as `interface.html`
//! has; and Delete, which asks first, then dispatches `Input::Delete` (it
//! goes from the watch too at the next sync) and goes back to Results. A
//! result whose log hasn't arrived (`LogState`) says so. The back key goes
//! back to Results.

use super::App;

/// Draws the page for the result whose log is `f`.
pub(super) fn show(app: &mut App, ui: &mut egui::Ui, f: &str) {}
