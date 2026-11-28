//! The Watch page, as blink's Status page: rows with a dot, a name and a few
//! words. Which way the watch is connected and how it's doing
//! (`watch_ble::status().sentence(..)`: through Gadgetbridge, or the direct
//! link's state), when it was last heard from and last synced
//! (`health::last_seen` with `Link::Watch` and `Link::Synced`,
//! `health::ago`), the watch app's version (`WatchState::version`), Sync
//! now (`Input::SyncNow`) with what the sync is doing, and the watch's
//! settings: Course (20 m / 15 m) and GPS speed, which send `beeptest-set`
//! (`Input::SetWatch`). They show what the watch last said
//! (`WatchState`), so a change shows once the watch answers; until then the
//! page says it's waiting, and after [`ANSWER_WAIT`] that it didn't answer.

use super::status::{self, Dot};
use super::{App, card, chip, section, toggle};
use crate::health::{self, Link};
use crate::platform;
use crate::store::WatchState;
use crate::sync::{Input, Progress};
use crate::watch_ble::{self, Status};
use egui::RichText;
use std::time::{Duration, Instant};

/// How long a setting sent waits for the watch's answer before the page
/// says it didn't come (the engine's own wait for `state`).
pub const ANSWER_WAIT: Duration = Duration::from_millis(crate::sync::ANSWER_WAIT_MS as u64);

/// A setting sent to the watch, not yet answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sent {
    pub m: Option<u32>,
    pub gps: Option<bool>,
    pub at: Instant,
}

impl Sent {
    /// Whether the watch now says what was sent.
    pub fn answered(&self, watch: &WatchState) -> bool {
        self.m.is_none_or(|m| watch.m == Some(m)) && self.gps.is_none_or(|g| watch.gps == Some(g))
    }
}

/// The page's settings sent and how they went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub sent: Option<Sent>,
    /// The last one sent got no answer.
    pub unanswered: bool,
}

impl State {
    /// Sends a change of the watch's settings (unless it's what the watch
    /// already has).
    pub fn send(&mut self, watch: &WatchState, m: Option<u32>, gps: Option<bool>) {
        let m = m.filter(|m| watch.m != Some(*m));
        let gps = gps.filter(|g| watch.gps != Some(*g));
        if m.is_none() && gps.is_none() {
            return;
        }
        platform::dispatch(Input::SetWatch { m, gps });
        // A course and GPS sent one after the other wait together.
        let (m, gps) = match &self.sent {
            Some(s) => (m.or(s.m), gps.or(s.gps)),
            None => (m, gps),
        };
        self.sent = Some(Sent {
            m,
            gps,
            at: Instant::now(),
        });
        self.unanswered = false;
    }

    /// What the watch says now: an answer clears what was sent; no answer
    /// in time says so.
    pub fn settle(&mut self, watch: &WatchState, now: Instant) {
        if let Some(s) = &self.sent {
            if s.answered(watch) {
                self.sent = None;
            } else if now.duration_since(s.at) >= ANSWER_WAIT {
                self.sent = None;
                self.unanswered = true;
            }
        }
    }
}

/// The connection row's dot and words.
pub(crate) fn connection(direct: bool, link: Status, progress: &Progress) -> (Dot, String) {
    if direct {
        let dot = match link {
            Status::Connected => Dot::Green,
            Status::Connecting | Status::SettingUp | Status::NoWatch => Dot::Amber,
            _ => Dot::Red,
        };
        let words = match link {
            Status::Connected => "Connected directly",
            other => other.words(),
        };
        return (dot, words.to_string());
    }
    match progress {
        Progress::NoAnswer => (Dot::Red, "No answer".into()),
        Progress::Asking { .. } => (Dot::Amber, "Asking...".into()),
        Progress::Idle => (Dot::Off, "Through Gadgetbridge".into()),
        _ => (Dot::Green, "Through Gadgetbridge".into()),
    }
}

/// Draws the page.
pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {
    let now = health::now_s();
    let dir = app.store.dir().to_path_buf();
    let progress = platform::progress();
    let watch = app.view.watch.clone();
    app.watch.settle(&watch, Instant::now());
    if app.watch.sent.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
    let direct = watch_ble::direct();
    let link = watch_ble::status();
    let heard = health::last_seen(&dir, Link::Watch);
    let synced = health::last_seen(&dir, Link::Synced);

    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let (dot, summary) = connection(direct, link, &progress);
        status::row(
            ui,
            "Connection",
            Some(dot),
            &summary,
            dot == Dot::Red,
            |ui| {
                if direct {
                    let name = watch_ble::settings()
                        .watch
                        .map_or_else(|| "the watch".to_string(), |w| w.name);
                    ui.label(link.sentence(&name));
                } else {
                    status::fact(ui, "Link", "Through Gadgetbridge");
                }
                if progress == Progress::NoAnswer {
                    ui.label(if direct {
                        "The watch didn't answer. Check Beep Test is installed on it, and that \
                     it's near."
                    } else {
                        "The watch didn't answer. Check it's connected in Gadgetbridge, with \
                     \"Allow Intents\" on in Gadgetbridge's settings for the Bangle.js, and \
                     that Beep Test is installed on it."
                    });
                }
            },
        );
        status::line(ui);
        let (dot, summary) = match heard {
            Some(t) => (Dot::Green, health::ago(t, now)),
            None => (Dot::Off, "Never".into()),
        };
        status::row(ui, "Last heard from", Some(dot), &summary, false, |ui| {
            status::fact(
                ui,
                "Last message",
                &heard.map_or_else(|| "never".into(), |t| status::when(t, now)),
            );
        });
        status::line(ui);
        let (dot, summary) = match (&progress, synced) {
            (p, _) if p.active() => (Dot::Amber, p.words()),
            (_, Some(t)) => (Dot::Green, health::ago(t, now)),
            (_, None) => (Dot::Off, "Never".into()),
        };
        status::row(ui, "Last synced", Some(dot), &summary, false, |ui| {
            status::fact(
                ui,
                "Everything here",
                &synced.map_or_else(|| "never".into(), |t| status::when(t, now)),
            );
            let words = progress.words();
            if !words.is_empty() {
                status::fact(ui, "Now", &words);
            }
        });
        status::line(ui);
        let summary = watch
            .version
            .as_ref()
            .map_or_else(|| "Not known yet".into(), |v| format!("Beep Test {v}"));
        status::row(ui, "Watch app", None, &summary, false, |ui| {
            ui.label(
                RichText::new(
                    "The watch says which version it has each time it syncs. Update it \
                     from the Bangle.js App Loader.",
                )
                .small()
                .weak(),
            );
        });
    });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if progress.active() {
            super::spinner(ui);
        }
        let words = progress.words();
        if !words.is_empty() {
            ui.label(RichText::new(words).weak());
        }
    });
    if super::primary(ui, "Sync now") {
        platform::dispatch(Input::SyncNow);
    }

    ui.add_space(12.0);
    section(ui, "The watch's settings");
    settings_card(app, ui, &watch);
}

/// Course and GPS speed, as the watch has them, and what was sent.
fn settings_card(app: &mut App, ui: &mut egui::Ui, watch: &WatchState) {
    let sent = app.watch.sent.clone();
    let mut m = None;
    let mut gps = None;
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.horizontal(|ui| {
            ui.set_min_height(44.0);
            ui.label("Course");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // Right to left: 15 m first.
                for course in [15, 20] {
                    let waiting = sent.as_ref().is_some_and(|s| s.m == Some(course));
                    let text = if waiting {
                        format!("{course} m …")
                    } else {
                        format!("{course} m")
                    };
                    if chip::choice(ui, &text, watch.m == Some(course), 40.0).clicked() {
                        m = Some(course);
                    }
                }
            });
        });
        let mut on = watch.gps.unwrap_or(false);
        if toggle::show(ui, &mut on, "Record GPS speed").changed() {
            gps = Some(on);
        }
        ui.horizontal_wrapped(|ui| {
            if let Some(s) = &sent {
                super::spinner(ui);
                let what = match (s.m, s.gps) {
                    (Some(m), None) => format!("the {m} m course"),
                    (None, Some(true)) => "GPS speed on".into(),
                    (None, Some(false)) => "GPS speed off".into(),
                    _ => "the change".into(),
                };
                ui.label(RichText::new(format!("Sending {what} to the watch...")).small());
            } else if app.watch.unanswered {
                ui.label(
                    RichText::new(
                        "The watch didn't answer, so it still has what's shown here. Is it \
                         connected?",
                    )
                    .small()
                    .color(status::amber_text(ui)),
                );
            } else if watch.m.is_none() {
                ui.label(
                    RichText::new("Not heard from the watch yet: its settings show once it syncs.")
                        .small()
                        .weak(),
                );
            }
        });
        ui.label(
            RichText::new(
                "These are the watch's own, used for the next test it runs. A change here \
                 goes to the watch and shows once it answers.",
            )
            .small()
            .weak(),
        );
    });
    if m.is_some() || gps.is_some() {
        app.watch.send(watch, m, gps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(m: u32, gps: bool) -> WatchState {
        WatchState {
            m: Some(m),
            gps: Some(gps),
            ..WatchState::default()
        }
    }

    #[test]
    fn a_setting_waits_for_the_watch_then_shows() {
        let at = Instant::now();
        let mut s = State {
            sent: Some(Sent {
                m: Some(15),
                gps: None,
                at,
            }),
            unanswered: false,
        };
        // The watch still says 20 m: waiting.
        s.settle(&watch(20, false), at + Duration::from_secs(1));
        assert!(s.sent.is_some());
        // It answers 15 m: done.
        s.settle(&watch(15, false), at + Duration::from_secs(2));
        assert_eq!(s.sent, None);
        assert!(!s.unanswered);
    }

    #[test]
    fn no_answer_in_time_says_so() {
        let at = Instant::now();
        let mut s = State {
            sent: Some(Sent {
                m: None,
                gps: Some(true),
                at,
            }),
            unanswered: false,
        };
        s.settle(&watch(20, false), at + ANSWER_WAIT);
        assert_eq!(s.sent, None);
        assert!(s.unanswered);
    }

    #[test]
    fn what_the_watch_has_already_isnt_sent() {
        let mut s = State::default();
        s.send(&watch(20, true), Some(20), Some(true));
        assert_eq!(s.sent, None);
    }

    #[test]
    fn the_connection_row_says_how_its_going() {
        let gb = |p: Progress| connection(false, Status::Off, &p);
        assert_eq!(gb(Progress::NoAnswer).0, Dot::Red);
        assert_eq!(gb(Progress::Asking { since_ms: 0 }).0, Dot::Amber);
        assert_eq!(
            gb(Progress::Idle),
            (Dot::Off, "Through Gadgetbridge".into())
        );
        assert_eq!(
            gb(Progress::Done {
                at_ms: 0,
                failed: vec![]
            })
            .0,
            Dot::Green
        );
        let direct = |s: Status| connection(true, s, &Progress::Idle);
        assert_eq!(
            direct(Status::Connected),
            (Dot::Green, "Connected directly".into())
        );
        assert_eq!(direct(Status::Connecting).0, Dot::Amber);
        assert_eq!(
            direct(Status::BluetoothOff),
            (Dot::Red, "Bluetooth is off".into())
        );
    }
}
