//! A result's page (from a card on Results): the summary as on its card
//! (with the speeds it ran, when they aren't the standard ones);
//! the heart rate over the test (`beeplog::hr_series`) with the levels
//! marked; a row per level (`beeplog::level_rows`: time, average and max
//! heart rate, GPS speed when recorded); Share TCX and Share CSV
//! (`Store::export`, then `platform::share_file`), with the option to leave
//! out low-confidence heart rate readings (below 50%) as `interface.html`
//! has; and Delete, which asks first, then dispatches `Input::Delete` (it
//! goes from the watch too at the next sync) and goes back to Results. A
//! result whose log hasn't arrived, or that has none (`LogState`), says so
//! and offers no chart or shares. The back key goes back to Results.

use super::{App, Dialog, Page, card, chart, dialog, medium, section, status, toggle};
use crate::beeplog::{self, LevelRow, Log};
use crate::health;
use crate::platform;
use crate::store::{Export, LogState, TestResult};
use crate::sync::{Input, Progress};
use egui::{Color32, RichText};

/// The confidence below which a reading is low (`interface.html`'s 50%).
pub const LOW_CONFIDENCE: f64 = 50.0;

/// The page's own state, while it's open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct State {
    /// "Leave out low-confidence heart rate readings": the chart, the table
    /// and the TCX.
    pub leave_out_low: bool,
    /// What the last share said.
    pub said: Option<Said>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Said {
    /// Where the file was written (the desktop, which has no share sheet).
    Written(String),
    /// Why it couldn't be shared.
    Failed(String),
}

impl State {
    fn min_confidence(&self) -> f64 {
        if self.leave_out_low {
            LOW_CONFIDENCE
        } else {
            0.0
        }
    }
}

/// What the page says about a log that isn't here, or `None` when it is.
pub(crate) fn missing_words(log: LogState, fetching: bool) -> Option<(&'static str, String)> {
    match log {
        LogState::Complete => None,
        LogState::NoLog => Some((
            "No heart rate log",
            "The watch has no log for this test (it was deleted there, or never written), \
             so there's no chart and nothing to share."
                .into(),
        )),
        LogState::Missing | LogState::Partial(_) => {
            let so_far = match log {
                LogState::Partial(n) if n > 0 => {
                    format!(" {} KB of it is here so far.", n.div_ceil(1024))
                }
                _ => String::new(),
            };
            let when = if fetching {
                "It's coming over now."
            } else {
                "It comes over at the next sync, while the watch is connected (and not \
                 running a test)."
            };
            Some(("The log is still on the watch", format!("{when}{so_far}")))
        }
    }
}

/// What a level's row says: the level, when it started and ended, its
/// average and highest heart rate, and the GPS speed (km/h, one place)
/// when recorded; "-" for what wasn't read.
pub(crate) fn row_cells(row: &LevelRow) -> [String; 5] {
    let hr = |v: Option<u32>| v.map_or_else(|| "-".to_string(), |v| v.to_string());
    [
        row.level.to_string(),
        format!(
            "{}–{}",
            super::duration(row.start_s),
            super::duration(row.end_s)
        ),
        hr(row.avg_hr),
        hr(row.max_hr),
        row.gps_kmh
            .map_or_else(|| "-".to_string(), |v| format!("{v:.1}")),
    ]
}

/// How long the log runs: to its end line, or its last sample, and never
/// past what a test can be ([`beeplog::LONGEST_TEST_S`]).
fn end_s(log: &Log) -> f64 {
    log.end
        .map(|e| e.t)
        .filter(|t| t.is_finite())
        .or_else(|| log.samples.last().map(|s| s.t))
        .filter(|t| t.is_finite())
        .map_or(0.0, |t| t.clamp(0.0, beeplog::LONGEST_TEST_S))
}

/// The title of a log that isn't here, after a spinner while it's coming
/// over: it wraps within the card (a small phone with large text).
fn missing_title(ui: &mut egui::Ui, title: &str, fetching: bool) -> egui::Response {
    ui.horizontal(|ui| {
        if fetching {
            super::spinner(ui);
        }
        ui.add(egui::Label::new(RichText::new(title).font(medium(16.0)).strong()).wrap())
    })
    .inner
}

/// Draws the page for the result whose log is `f`.
pub(super) fn show(app: &mut App, ui: &mut egui::Ui, f: &str) {
    let Some(r) = app.view.result(f).cloned() else {
        super::empty(ui, "Not here any more", "This result has been deleted.");
        return;
    };
    let best = app.view.is_best(&r);
    summary(ui, &r, best);
    ui.add_space(8.0);
    let fetching =
        matches!(platform::progress(), Progress::Logs { f: ref now, .. } if *now == r.entry.f);
    match (missing_words(r.log, fetching), &app.view.log) {
        (Some((title, words)), _) => {
            card(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.spacing_mut().interact_size.y = 22.0;
                missing_title(ui, title, fetching);
                ui.label(words);
            });
        }
        (None, Some((lf, log))) if lf == f => {
            let log = log.clone();
            with_log(app, ui, &log, f);
        }
        (None, _) => {
            card(ui, |ui| {
                ui.label(RichText::new("The log couldn't be read.").weak());
            });
        }
    }
    ui.add_space(16.0);
    if super::secondary(ui, "Delete this result", Some(super::RED)) {
        app.dialog = Some(Dialog::Delete(f.to_string()));
    }
}

/// The summary: when, the result large, the course, and the rest.
fn summary(ui: &mut egui::Ui, r: &TestResult, best: bool) {
    let text = super::results::card_text(r, health::now_s());
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.spacing_mut().interact_size.y = 22.0;
        super::results::when_row(ui, RichText::new(&text.when).weak(), best);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&text.score).font(medium(44.0)).strong());
            ui.label(RichText::new(format!("on {}", text.course)).weak());
        });
        let e = &r.entry;
        if let Some(speeds) = &text.speeds {
            status::fact(ui, "Speeds", speeds);
        }
        status::fact(ui, "Distance", &format!("{} m", e.d));
        status::fact(ui, "Time", &super::duration(f64::from(e.dur)));
        status::fact(
            ui,
            "Heart rate (avg / max)",
            &if e.max == 0 && e.avg == 0 {
                "none read".to_string()
            } else {
                format!("{} / {} bpm", e.avg, e.max)
            },
        );
    });
}

/// The chart, the option, the table and the shares, for a log that's here.
fn with_log(app: &mut App, ui: &mut egui::Ui, log: &Log, f: &str) {
    let min_conf = app.result.min_confidence();
    let hr = beeplog::hr_series(log, min_conf);
    let rows = beeplog::level_rows(log, min_conf);
    section(ui, "Heart rate");
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        if hr.is_empty() {
            let any = !beeplog::hr_series(log, 0.0).is_empty();
            ui.add_space(4.0);
            ui.label(
                RichText::new(if any {
                    "Every reading was below 50% confidence: turn the option below off to \
                     see them."
                } else {
                    "No heart rate was read in this test (was the watch worn snugly?)."
                })
                .weak(),
            );
            ui.add_space(4.0);
        } else {
            chart::heart_rate(ui, &hr, &rows, end_s(log), 180.0);
            ui.label(
                RichText::new("bpm, with each level's number over it")
                    .small()
                    .weak(),
            );
        }
        ui.add_space(4.0);
        toggle::show(
            ui,
            &mut app.result.leave_out_low,
            "Leave out low-confidence heart rate readings (<50%)",
        );
    });

    ui.add_space(8.0);
    section(ui, "Levels");
    card(ui, |ui| levels_table(ui, &rows));

    ui.add_space(8.0);
    section(ui, "Share");
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.add_space(2.0);
        let mut shared = None;
        if super::primary(ui, "Share TCX") {
            shared = Some(Export::Tcx);
        }
        if super::secondary(ui, "Share CSV", None) {
            shared = Some(Export::Csv);
        }
        ui.label(
            RichText::new(
                "TCX for Strava, Garmin Connect and the like (leaving out low-confidence \
                 readings if that's on); CSV has every reading.",
            )
            .small()
            .weak(),
        );
        if let Some(kind) = shared {
            app.result.said = share(app, f, kind, min_conf);
        }
        match &app.result.said {
            Some(Said::Written(path)) => {
                ui.label(RichText::new(format!("Written to {path}")).small());
            }
            Some(Said::Failed(why)) => {
                ui.colored_label(ui.visuals().error_fg_color, why);
            }
            None => {}
        }
    });
}

/// Writes the file and opens the share sheet: what to say after.
fn share(app: &App, f: &str, kind: Export, min_conf: f64) -> Option<Said> {
    let min_conf = if kind == Export::Tcx { min_conf } else { 0.0 };
    let path = match app.store.export(f, kind, min_conf) {
        Ok(path) => path,
        Err(why) => return Some(Said::Failed(why)),
    };
    match platform::share_file(&path, kind.mime()) {
        Err(why) => Some(Said::Failed(why)),
        // The desktop has no share sheet: the file stays where it is.
        Ok(()) if cfg!(not(target_os = "android")) => {
            Some(Said::Written(path.display().to_string()))
        }
        Ok(()) => None,
    }
}

/// A row per level: the level, from when to when, average and highest
/// heart rate, and GPS speed when it was recorded.
fn levels_table(ui: &mut egui::Ui, rows: &[LevelRow]) {
    if rows.is_empty() {
        ui.label(RichText::new("The log has no samples.").weak());
        return;
    }
    let width = ui.available_width();
    let table = Table::lay_out(ui, rows, width);
    let small = egui::TextStyle::Small.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    let ink = ui.visuals().text_color();
    let stripe = ui.visuals().faint_bg_color.lerp_to_gamma(ink, 0.05);
    // Painted, a line a level, each column as wide as its widest cell
    // ([`Table::lay_out`]), so the table never runs past the card and no
    // cell runs into the next.
    let line = |ui: &mut egui::Ui,
                height: f32,
                cells: &[(String, egui::FontId, Color32)],
                striped: bool| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        if striped {
            ui.painter()
                .rect_filled(rect.expand2(egui::vec2(6.0, 0.0)), 6.0, stripe);
        }
        let mut x = rect.left();
        for ((text, font, color), w) in cells.iter().zip(&table.widths) {
            ui.painter().text(
                egui::pos2(x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                font.clone(),
                *color,
            );
            x += w;
        }
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    let head: Vec<_> = table
        .heads()
        .iter()
        .take(table.widths.len())
        .map(|h| (h.to_string(), small.clone(), weak))
        .collect();
    line(ui, 24.0, &head, false);
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<_> = table
            .cells(row)
            .into_iter()
            .take(table.widths.len())
            .enumerate()
            .map(|(c, text)| (text, table.fonts[c].clone(), ink))
            .collect();
        line(ui, 28.0, &cells, i % 2 == 0);
    }
}

/// The table's headings.
const HEADS: [&str; 5] = ["Level", "Time", "Avg", "Max", "km/h"];
/// The least room between one column's text and the next's.
const COLUMN_GAP: f32 = 6.0;

/// How the levels table is laid out: each column's width (points) and the
/// font of its cells (the heads are Small).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Table {
    pub widths: Vec<f32>,
    pub fonts: Vec<egui::FontId>,
    /// The Time column says only when each level ended ("Ended"), for
    /// when even that's too wide from when to when.
    pub ends_only: bool,
}

impl Table {
    /// A row's cells as this table shows them.
    pub fn cells(&self, row: &LevelRow) -> [String; 5] {
        let mut cells = row_cells(row);
        if self.ends_only {
            cells[1] = super::duration(row.end_s);
        }
        cells
    }

    /// The headings.
    pub fn heads(&self) -> [&'static str; 5] {
        let mut heads = HEADS;
        if self.ends_only {
            heads[1] = "Ended";
        }
        heads
    }
}

impl Table {
    /// The columns for `rows` in a card `width` wide: the parts of it
    /// [`column_widths`] gives, each widened to its widest cell (and a gap)
    /// with the room taken from the others' spare ([`fit_columns`]). When
    /// even that doesn't fit (a narrow screen, GPS on, a long test, large
    /// text), the times go down to Small, then every cell, then the Time
    /// column says only when each level ended; and if it still doesn't,
    /// the columns are shared as they come.
    pub fn lay_out(ui: &egui::Ui, rows: &[LevelRow], width: f32) -> Table {
        let gps = rows.iter().any(|r| r.gps_kmh.is_some());
        let parts = column_widths(gps);
        let body = egui::TextStyle::Body.resolve(ui.style());
        let small = egui::TextStyle::Small.resolve(ui.style());
        let measure = |text: &str, font: &egui::FontId| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER)
                .size()
                .x
        };
        let first: Vec<egui::FontId> = (0..parts.len())
            .map(|c| if c == 0 { medium(16.0) } else { body.clone() })
            .collect();
        let mut times_small = first.clone();
        times_small[1] = small.clone();
        let all_small = vec![small.clone(); parts.len()];
        // Each try in turn, the first that fits.
        let tries = [
            (first, false),
            (times_small, false),
            (all_small.clone(), false),
            (all_small.clone(), true),
        ];
        for (fonts, ends_only) in tries {
            let t = Table {
                widths: Vec::new(),
                fonts,
                ends_only,
            };
            let need: Vec<f32> = (0..parts.len())
                .map(|c| {
                    let head = measure(t.heads()[c], &small);
                    rows.iter()
                        .map(|r| measure(&t.cells(r)[c], &t.fonts[c]))
                        .fold(head, f32::max)
                })
                .collect();
            if let Some(widths) = fit_columns(width, &parts, &need) {
                return Table { widths, ..t };
            }
        }
        Table {
            widths: parts.iter().map(|p| p * width).collect(),
            fonts: all_small,
            ends_only: true,
        }
    }
}

/// The columns' widths in points for a table `width` wide: each its part
/// of the width (`parts`, summing to 1), except that a column narrower
/// than its widest cell (`need`) and a gap is widened to that, the room
/// coming from the other columns in proportion to their parts, none of
/// them below its own need. `None` when the needs alone are past `width`.
pub(crate) fn fit_columns(width: f32, parts: &[f32], need: &[f32]) -> Option<Vec<f32>> {
    let last = parts.len().saturating_sub(1);
    let want: Vec<f32> = need
        .iter()
        .enumerate()
        .map(|(i, n)| if i == last { *n } else { n + COLUMN_GAP })
        .collect();
    if want.iter().sum::<f32>() > width {
        return None;
    }
    // Columns held at their need; the rest share what's left.
    let mut held = vec![false; parts.len()];
    loop {
        let rest = width
            - (0..parts.len())
                .filter(|&i| held[i])
                .map(|i| want[i])
                .sum::<f32>();
        let share: f32 = (0..parts.len())
            .filter(|&i| !held[i])
            .map(|i| parts[i])
            .sum();
        let widths: Vec<f32> = (0..parts.len())
            .map(|i| {
                if held[i] {
                    want[i]
                } else {
                    parts[i] / share * rest
                }
            })
            .collect();
        let short: Vec<usize> = (0..parts.len())
            .filter(|&i| !held[i] && widths[i] < want[i])
            .collect();
        if short.is_empty() {
            return Some(widths);
        }
        for i in short {
            held[i] = true;
        }
    }
}

/// The table's columns, as parts of its width: the level, its time, the
/// heart rates, and GPS speed when there is one.
pub(crate) fn column_widths(gps: bool) -> Vec<f32> {
    if gps {
        vec![0.14, 0.34, 0.17, 0.17, 0.18]
    } else {
        vec![0.16, 0.40, 0.22, 0.22]
    }
}

/// The confirm before Delete: there's no undo.
pub(super) fn delete_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(Dialog::Delete(f)) = app.dialog.clone() else {
        return;
    };
    let mut delete = false;
    let mut cancel = false;
    let modal = dialog::modal(ctx, "delete_result").show(ctx, |ui| {
        dialog::begin(ui);
        dialog::title(ui, "Delete this result?");
        ui.add_space(4.0);
        dialog::text(
            ui,
            "It goes from this phone now, with its heart rate log, and from the watch at \
             the next sync. There's no undo.",
        );
        dialog::actions(ui, |ui| {
            delete = dialog::button(ui, "Delete", Some(super::RED));
            cancel = dialog::button(ui, "Cancel", None);
        });
    });
    if delete {
        platform::dispatch(Input::Delete(f));
        app.dialog = None;
        app.open(Page::Results);
    } else if cancel || modal.should_close() {
        app.dialog = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_not_here_says_why() {
        assert_eq!(missing_words(LogState::Complete, false), None);
        let (title, words) = missing_words(LogState::Missing, false).unwrap();
        assert_eq!(title, "The log is still on the watch");
        assert!(words.contains("next sync"), "{words}");
        let (_, words) = missing_words(LogState::Partial(3000), true).unwrap();
        assert!(
            words.starts_with("It's coming over now.") && words.contains("3 KB"),
            "{words}"
        );
        let (title, _) = missing_words(LogState::NoLog, false).unwrap();
        assert_eq!(title, "No heart rate log");
    }

    #[test]
    fn a_level_row_reads_as_a_line_of_the_table() {
        let row = LevelRow {
            level: 3,
            start_s: 63.0,
            end_s: 125.4,
            avg_hr: Some(142),
            max_hr: Some(151),
            gps_kmh: Some(9.04),
        };
        assert_eq!(
            row_cells(&row),
            ["3", "1:03–2:05", "142", "151", "9.0"].map(String::from)
        );
        let none = LevelRow {
            avg_hr: None,
            max_hr: None,
            gps_kmh: None,
            ..row
        };
        assert_eq!(row_cells(&none)[2..], ["-", "-", "-"].map(String::from));
    }

    #[test]
    fn the_tables_columns_fill_the_card_and_no_more() {
        for gps in [false, true] {
            let sum: f32 = column_widths(gps).iter().sum();
            assert!((sum - 1.0).abs() < 1e-6, "{sum}");
        }
        assert_eq!(column_widths(true).len(), 5);
        assert_eq!(column_widths(false).len(), 4);
    }

    /// A frame with the app's fonts and sizes (the text at `scale`),
    /// `width` points wide.
    fn in_a_frame(width: f32, scale: f32, f: impl FnOnce(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        super::super::apply_theme(&ctx);
        super::super::set_text_sizes(&ctx, scale);
        let mut f = Some(f);
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    // The fonts are there from the second frame.
                    if ctx.cumulative_frame_nr() > 0
                        && let Some(f) = f.take()
                    {
                        f(ui);
                    }
                });
            });
        }
        super::super::set_text_sizes(&ctx, 1.0);
        assert!(f.is_none(), "it ran");
    }

    #[test]
    fn the_date_keeps_clear_of_best_on_a_narrow_screen_with_large_text() {
        for (width, scale) in [(411.0, 1.0), (256.0, 1.0), (256.0, 1.3), (200.0, 1.3)] {
            in_a_frame(width, scale, |ui| {
                // As the cards have it.
                ui.spacing_mut().interact_size.y = super::super::chip::HEIGHT;
                for small in [false, true] {
                    let mut when = RichText::new("Wed 18 Nov, 17:40").weak();
                    if small {
                        when = when.small();
                    }
                    let top = ui.cursor().top();
                    let (date, chip) = super::super::results::when_row(ui, when, true);
                    let chip = chip.unwrap();
                    // A row (two lines at most), not the rest of the card.
                    let tall = ui.cursor().top() - top;
                    assert!(tall < 90.0, "{width} at {scale}: {tall} high");
                    let at = format!("{width} at {scale}: {:?} and {:?}", date.rect, chip.rect);
                    assert!(!date.rect.intersects(chip.rect), "{at}");
                    assert!(chip.rect.right() <= width, "{at}");
                    let (alone, none) =
                        super::super::results::when_row(ui, RichText::new("x"), false);
                    assert!(none.is_none() && alone.rect.left() <= date.rect.left() + 0.5);
                }
            });
        }
    }

    #[test]
    fn a_missing_logs_title_wraps_within_its_card_on_a_narrow_screen_with_large_text() {
        for (width, scale) in [(411.0, 1.0), (320.0, 1.3), (256.0, 1.3)] {
            in_a_frame(width, scale, |ui| {
                // The column, before anything in it can widen it.
                let right = ui.max_rect().right();
                for fetching in [false, true] {
                    let (title, frame) = super::super::card(ui, |ui| {
                        missing_title(ui, "The log is still on the watch", fetching)
                    });
                    let at = format!(
                        "{width} at {scale}, fetching {fetching}: {:?} in {:?}",
                        title.rect, frame.rect
                    );
                    // The card is the column's width, the title inside it.
                    assert!(frame.rect.right() <= right + 0.5, "{at}");
                    assert!(title.rect.right() <= right - 16.0 + 0.5, "{at}");
                }
            });
        }
    }

    #[test]
    fn the_sync_words_keep_clear_of_sync_now_on_a_narrow_screen_with_large_text() {
        for (width, scale) in [(411.0, 1.0), (256.0, 1.0), (256.0, 1.3), (200.0, 1.3)] {
            in_a_frame(width, scale, |ui| {
                for spinning in [false, true] {
                    let top = ui.cursor().top();
                    let (words, button) = super::super::results::words_and_sync_now(
                        ui,
                        spinning,
                        "A test is running on the watch",
                    );
                    let at = format!("{width} at {scale}: {:?} and {:?}", words.rect, button.rect);
                    assert!(!words.rect.intersects(button.rect), "{at}");
                    assert!(button.rect.right() <= width, "{at}");
                    let tall = ui.cursor().top() - top;
                    assert!(tall < 150.0, "{width} at {scale}: {tall} high");
                }
            });
        }
    }

    #[test]
    fn the_levels_table_fits_long_times_on_a_narrow_screen() {
        // Twelve levels with GPS, to 12:39: "10:33–11:37" in the body face
        // is about 94 pt, past the GPS layout's 34% of a 320 dp phone's
        // 256 pt card.
        let rows: Vec<LevelRow> = (1..=12)
            .map(|l| LevelRow {
                level: l,
                start_s: f64::from(l - 1) * 63.3,
                end_s: f64::from(l) * 63.3,
                avg_hr: Some(188),
                max_hr: Some(199),
                gps_kmh: Some(12.5),
            })
            .collect();
        // And with the phone's font size at its largest the app follows.
        for (width, scale) in [
            (256.0_f32, 1.0),
            (300.0, 1.0),
            (379.0, 1.0),
            (300.0, 1.3),
            (256.0, 1.3),
        ] {
            in_a_frame(width + 40.0, scale, |ui| {
                let t = Table::lay_out(ui, &rows, width);
                let sum: f32 = t.widths.iter().sum();
                assert!(sum <= width + 0.01, "{width} at {scale}: {t:?}");
                // No cell runs into the next column.
                assert_eq!(
                    t.ends_only,
                    width == 256.0 && scale > 1.0,
                    "{width} at {scale}"
                );
                for row in &rows {
                    let cells = t.cells(row);
                    for (c, w) in t.widths.iter().enumerate().take(4) {
                        let text = ui
                            .painter()
                            .layout_no_wrap(cells[c].clone(), t.fonts[c].clone(), Color32::WHITE)
                            .size()
                            .x;
                        assert!(
                            text + COLUMN_GAP <= w + 0.01,
                            "{width}: {:?} in {w}",
                            cells[c]
                        );
                    }
                }
            });
        }
        // Columns only widen as far as they need, from the others' spare.
        let w = fit_columns(256.0, &column_widths(true), &[20.0, 94.0, 30.0, 30.0, 32.0]).unwrap();
        assert!(
            w[1] >= 100.0 && (w.iter().sum::<f32>() - 256.0).abs() < 0.01,
            "{w:?}"
        );
        assert_eq!(
            fit_columns(100.0, &column_widths(true), &[20.0, 94.0, 30.0, 30.0, 32.0]),
            None
        );
        let roomy = fit_columns(400.0, &column_widths(false), &[20.0, 60.0, 30.0, 30.0]).unwrap();
        for (w, p) in roomy.iter().zip(column_widths(false)) {
            assert!((w - p * 400.0).abs() < 0.01, "{roomy:?}");
        }
    }

    #[test]
    fn the_option_is_interface_htmls_50_percent() {
        let mut s = State::default();
        assert_eq!(s.min_confidence(), 0.0);
        s.leave_out_low = true;
        assert_eq!(s.min_confidence(), 50.0);
    }

    #[test]
    fn a_log_runs_to_its_end_or_its_last_sample() {
        let log = beeplog::parse("start,0,0,20\n0.0,90,80,1,0,0.0\n7.0,95,80,1,1,20.0\n");
        assert_eq!(end_s(&log), 7.0);
        let log = beeplog::parse("start,0,0,20\n0.0,90,80,1,0,0.0\nend,9.5,1,1,20,95,92\n");
        assert_eq!(end_s(&log), 9.5);
        assert_eq!(end_s(&beeplog::parse("")), 0.0);
        let log = beeplog::parse("start,0,0,20\n1000000000000.0,120,90,1,1,20.0\n");
        assert_eq!(end_s(&log), beeplog::LONGEST_TEST_S);
        let log = beeplog::parse("start,0,0,20\nInfinity,120,90,1,1,20.0\n");
        assert_eq!(end_s(&log), 0.0);
    }
}
