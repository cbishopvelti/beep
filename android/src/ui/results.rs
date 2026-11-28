//! The Results page, the first: a card a result, newest first
//! (`Store::results`), with the date and time in the phone's time zone
//! (`crate::health::local`), the course, the result large as
//! `level.shuttle`, the distance, the time, heart rate avg / max, and the
//! speeds when they aren't the standard ones (`Entry::speeds`); a "best"
//! mark on the best of each course and speed progression (`Store::bests`:
//! a result on one progression isn't compared with another's). Above the list,
//! a banner while a test runs on the watch (`WatchState::run`), the sync
//! button (`Input::SyncNow`) with what the sync is doing
//! (`Progress::words`), and a small progress chart of results over time
//! with a line per course and progression that has results
//! (`Entry::total_shuttles`, `ui/chart.rs`). With no
//! results, how to connect the watch. Tapping a card opens it
//! (`Page::Result`).

use super::{App, Page, card, chart, chip, medium, status};
use crate::health;
use crate::platform;
use crate::store::{LogState, TestResult};
use crate::sync::{Input, Progress};
use egui::{Frame, Margin, RichText};

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// What a result's card says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardText {
    /// "Sat 28 Nov, 14:05" (with the year when it isn't this year's).
    pub when: String,
    /// "20 m".
    pub course: String,
    /// "9.4".
    pub score: String,
    /// "1480 m · 9:03 · ♥ 160 / 187".
    pub facts: String,
    /// "8.0, 8.5, 9.0... km/h", when the test ran other speeds than the
    /// standard ones (as the watch's result screen says them).
    pub speeds: Option<String>,
    /// Said when its log isn't on the phone.
    pub log: Option<&'static str>,
}

/// When a test was, in the phone's time zone: "Sat 28 Nov, 14:05", with
/// the year when it isn't `now_s`'s.
pub(crate) fn when(t_ms: i64, now_s: i64) -> String {
    let s = t_ms.div_euclid(1000);
    let (Some(t), Some(now)) = (health::local(s), health::local(now_s)) else {
        return health::local_time(s);
    };
    let day = WEEKDAYS[(t.weekday as usize).min(6)];
    let month = health::MONTHS[(t.month as usize).clamp(1, 12) - 1];
    let year = if t.year == now.year {
        String::new()
    } else {
        format!(" {}", t.year)
    };
    format!(
        "{day} {} {month}{year}, {:02}:{:02}",
        t.day, t.hour, t.minute
    )
}

/// "♥ 160 / 187" (average / highest), or "No heart rate" when none was
/// read.
pub(crate) fn heart_rate(avg: u32, max: u32) -> String {
    if max == 0 && avg == 0 {
        "No heart rate".into()
    } else {
        format!("♥ {avg} / {max}")
    }
}

/// What the card for `r` says, at `now_s`.
pub(crate) fn card_text(r: &TestResult, now_s: i64) -> CardText {
    let e = &r.entry;
    CardText {
        when: when(e.t, now_s),
        course: format!("{} m", e.m),
        score: e.score_text(),
        facts: format!(
            "{} m · {} · {}",
            e.d,
            super::duration(f64::from(e.dur)),
            heart_rate(e.avg, e.max)
        ),
        speeds: e.speeds(),
        log: match r.log {
            LogState::Complete => None,
            LogState::Missing | LogState::Partial(_) => Some("Heart rate log still on the watch"),
            LogState::NoLog => Some("No heart rate log"),
        },
    }
}

/// Draws the page.
pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {
    let now = health::now_s();
    let running = app.view.watch.run.is_some();
    if running {
        running_banner(ui);
    }
    sync_row(ui, running);
    if app.view.results.is_empty() {
        how_to_connect(app, ui);
        return;
    }
    if app.view.results.len() >= 2 {
        let series = chart::progress_series(&app.view.results);
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            // The units by the heading, so a legend with long speed
            // progressions can take its own lines without running into them.
            ui.scope(|ui| {
                // No taller than the heading (a row is a touch's height
                // at the least).
                ui.spacing_mut().interact_size.y = 0.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Progress").font(medium(16.0)).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("shuttles").small().weak());
                    });
                });
            });
            chart::progress(ui, &series, 130.0);
            let items: Vec<_> = series
                .iter()
                .map(|s| (chart::series_color(s.m, s.p), chart::series_label(s.m, s.p)))
                .collect();
            chart::legend(ui, &items);
        });
        ui.add_space(4.0);
    }
    let mut open = None;
    for r in &app.view.results {
        let best = app.view.is_best(r);
        if result_card(ui, r, best, now) {
            open = Some(r.entry.f.clone());
        }
    }
    if let Some(f) = open {
        app.open(Page::Result(f));
    }
}

/// A card's first row: when, and "★ Best" at the end of it. The date
/// wraps in the room the chip leaves rather than running under it (a
/// narrow phone with large text). The date's response, and the chip's.
pub(super) fn when_row(
    ui: &mut egui::Ui,
    when: RichText,
    best: bool,
) -> (egui::Response, Option<egui::Response>) {
    const BEST: &str = "★ Best";
    ui.horizontal(|ui| {
        let chip_w = if best {
            chip::width(ui, BEST, chip::HEIGHT) + ui.spacing().item_spacing.x
        } else {
            0.0
        };
        let room = (ui.available_width() - chip_w).max(0.0);
        let date = ui
            .scope(|ui| {
                ui.set_max_width(room);
                ui.add(egui::Label::new(when).wrap())
            })
            .inner;
        let chip = ui
            .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                best.then(|| chip::tinted(ui, BEST, chip::Kind::Label, status::green_text(ui)))
            })
            .inner;
        (date, chip)
    })
    .inner
}

/// A result's card; true when it was tapped.
fn result_card(ui: &mut egui::Ui, r: &TestResult, best: bool, now: i64) -> bool {
    let text = card_text(r, now);
    let ((), response) = card(ui, |ui| {
        // Rows as high as their text, not a touch target each: the whole
        // card is the target.
        ui.spacing_mut().interact_size.y = 20.0;
        ui.spacing_mut().item_spacing.y = 2.0;
        when_row(ui, RichText::new(&text.when).small().weak(), best);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&text.score).font(medium(34.0)).strong());
            ui.add_space(2.0);
            ui.label(RichText::new(format!("on {}", text.course)).weak());
        });
        ui.label(&text.facts);
        if let Some(speeds) = &text.speeds {
            ui.label(RichText::new(speeds).small().weak());
        }
        if let Some(log) = text.log {
            ui.label(RichText::new(log).small().weak());
        }
    });
    let id = ui.id().with(("result-card", &r.entry.f));
    let tap = ui.interact(response.rect, id, egui::Sense::click());
    tap.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            match &text.speeds {
                None => format!("Result {} on {}, {}", text.score, text.course, text.when),
                Some(speeds) => format!(
                    "Result {} on {} at {speeds}, {}",
                    text.score, text.course, text.when
                ),
            },
        )
    });
    if tap.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            16.0,
            ui.visuals().widgets.hovered.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    tap.clicked()
}

/// What the sync is doing, and the button that starts one. While the
/// banner says a test is running, it doesn't say it again.
fn sync_row(ui: &mut egui::Ui, running: bool) {
    let progress = platform::progress();
    let words = progress.words();
    let words = if running && progress == Progress::Busy {
        "Syncs when it ends".to_string()
    } else if words.is_empty() {
        "Not synced yet".to_string()
    } else {
        words
    };
    if words_and_sync_now(ui, progress.active(), &words)
        .1
        .clicked()
    {
        platform::dispatch(Input::SyncNow);
    }
    ui.add_space(4.0);
}

/// How the sync is going, and "⟳ Sync now" at the end of the row: the
/// words wrap in the room the button leaves, or, when that's under half
/// the row (a narrow phone with large text), have the row and the button
/// goes under them. The words' response, and the button's.
pub(super) fn words_and_sync_now(
    ui: &mut egui::Ui,
    spinning: bool,
    words: &str,
) -> (egui::Response, egui::Response) {
    const SYNC_NOW: &str = "⟳ Sync now";
    const TALL: f32 = 40.0;
    let button_w = chip::width(ui, SYNC_NOW, TALL) + ui.spacing().item_spacing.x;
    let beside = ui.available_width() - button_w >= ui.available_width() / 2.0;
    let sync_now = |ui: &mut egui::Ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            chip::button(ui, SYNC_NOW, TALL)
        })
        .inner
    };
    let (words, button) = ui
        .horizontal(|ui| {
            ui.set_min_height(TALL);
            if spinning {
                super::spinner(ui);
            }
            let room = if beside {
                ui.available_width() - button_w
            } else {
                ui.available_width()
            };
            let words = ui
                .scope(|ui| {
                    ui.set_max_width(room.max(0.0));
                    ui.add(egui::Label::new(RichText::new(words).weak()).wrap())
                })
                .inner;
            (words, beside.then(|| sync_now(ui)))
        })
        .inner;
    let button = match button {
        Some(button) => button,
        None => {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), TALL),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| chip::button(ui, SYNC_NOW, TALL),
            )
            .inner
        }
    };
    (words, button)
}

/// The banner while a test runs on the watch.
fn running_banner(ui: &mut egui::Ui) {
    let amber = super::AMBER;
    Frame::new()
        .fill(amber.gamma_multiply(0.15))
        .stroke(egui::Stroke::new(1.0_f32, amber))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new("A test is running on the watch")
                    .font(medium(16.0))
                    .color(status::amber_text(ui)),
            );
            ui.label(RichText::new("Its result comes over when it ends.").small());
        });
    ui.add_space(8.0);
}

/// No results yet: how to connect the watch.
fn how_to_connect(app: &mut App, ui: &mut egui::Ui) {
    super::empty(
        ui,
        "No results yet",
        "Results come over from the watch by themselves once it's connected.",
    );
    ui.add_space(16.0);
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(
            RichText::new(format!(
                "Beep Test {} or later on the watch",
                super::watch::NEEDS
            ))
            .font(medium(16.0))
            .strong(),
        );
        ui.label(super::watch::OLDER);
    });
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(
            RichText::new("Through Gadgetbridge")
                .font(medium(16.0))
                .strong(),
        );
        ui.label(RichText::new(
            "Connect the watch in Gadgetbridge, and turn on \"Allow Intents\" in \
                 Gadgetbridge's settings for the Bangle.js.",
        ));
    });
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new("Or directly").font(medium(16.0)).strong());
        ui.label(RichText::new(
            "Without Gadgetbridge: turn on \"Connect to the watch directly\" in \
                 Settings and pick the watch.",
        ));
        ui.add_space(4.0);
        if super::secondary(ui, "Open Settings", None) {
            app.open(Page::Settings);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Entry;

    fn result(log: LogState) -> TestResult {
        TestResult {
            entry: Entry {
                f: "beeptest.1795000000.csv".into(),
                t: 1_795_000_000_123,
                m: 20,
                p: 0,
                l: 9,
                s: 4,
                d: 1480,
                dur: 543,
                max: 187,
                avg: 160,
            },
            log,
        }
    }

    #[test]
    fn a_card_says_the_result_large_and_the_rest_under_it() {
        let now = 1_795_000_000 + 3600;
        let text = card_text(&result(LogState::Complete), now);
        assert_eq!(text.score, "9.4");
        assert_eq!(text.course, "20 m");
        assert_eq!(text.facts, "1480 m · 9:03 · ♥ 160 / 187");
        assert_eq!(text.speeds, None, "the standard speeds aren't said");
        assert_eq!(text.log, None);
        // The phone's time zone, as health::local has it, without the
        // year this year.
        let l = health::local(1_795_000_000).unwrap();
        assert_eq!(
            text.when,
            format!(
                "{} {} {}, {:02}:{:02}",
                WEEKDAYS[l.weekday as usize],
                l.day,
                health::MONTHS[l.month as usize - 1],
                l.hour,
                l.minute
            )
        );
        // A year later, with it.
        let later = card_text(&result(LogState::Complete), now + 400 * 86_400);
        assert!(
            later.when.contains(&format!(" {},", l.year)),
            "{}",
            later.when
        );
    }

    #[test]
    fn a_card_says_the_speeds_when_they_arent_the_standard_ones() {
        let mut r = result(LogState::Complete);
        for (p, words) in [(1, "8.0, 8.5, 9.0... km/h"), (2, "8.5, 9.5, 10.0... km/h")] {
            r.entry.p = p;
            assert_eq!(card_text(&r, 0).speeds.as_deref(), Some(words));
        }
    }

    #[test]
    fn a_card_says_when_its_log_isnt_here() {
        let now = 1_795_000_000;
        assert!(card_text(&result(LogState::Missing), now).log.is_some());
        assert_eq!(
            card_text(&result(LogState::Partial(300)), now).log,
            card_text(&result(LogState::Missing), now).log
        );
        assert_eq!(
            card_text(&result(LogState::NoLog), now).log,
            Some("No heart rate log")
        );
    }

    #[test]
    fn no_heart_rate_says_so() {
        assert_eq!(heart_rate(0, 0), "No heart rate");
        assert_eq!(heart_rate(150, 172), "♥ 150 / 172");
        let mut r = result(LogState::Complete);
        r.entry.avg = 0;
        r.entry.max = 0;
        r.entry.dur = 3723;
        assert_eq!(card_text(&r, 0).facts, "1480 m · 1:02:03 · No heart rate");
    }
}
