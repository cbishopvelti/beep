//! The Watch page, as blink's Status page: rows with a dot, a name and a few
//! words. Which way the watch is connected and how it's doing
//! (`watch_ble::status().sentence(..)`: through Gadgetbridge, or the direct
//! link's state), when it was last heard from and last synced
//! (`health::last_seen` with `Link::Watch` and `Link::Synced`,
//! `health::ago`), the watch app's version (`WatchState::version`), Sync
//! now (`Input::SyncNow`) with what the sync is doing, and the watch's
//! settings: Course (20 m / 15 m), GPS speed and, when the watch's app has
//! the Speeds setting (its `state` says `p`: `WatchState::p`), Speeds
//! (8, 9 / 8, 8.5 / 8.5, 9.5, as the watch's menu has them), which send
//! `beeptest-set` (`Input::SetWatch`). They show what the watch last said
//! (`WatchState`), so a change shows once the watch answers; until then the
//! page says it's waiting, and after [`ANSWER_WAIT`] that it didn't answer.

use super::status::{self, Dot};
use super::{App, card, chip, section, toggle};
use crate::health::{self, Link};
use crate::platform;
use crate::store::WatchState;
use crate::sync::{Input, Progress};
use crate::tables;
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
    pub p: Option<u32>,
    pub at: Instant,
    /// When the watch was last heard from as it was sent
    /// (`WatchState::heard`), if something sent is what the watch had then
    /// (a change taken back before it answered): what it says then isn't
    /// the answer, only a `state` heard since is.
    pub after: Option<Option<i64>>,
}

impl Sent {
    /// Whether the watch now says what was sent.
    pub fn answered(&self, watch: &WatchState) -> bool {
        self.m.is_none_or(|m| watch.m == Some(m))
            && self.gps.is_none_or(|g| watch.gps == Some(g))
            && self.p.is_none_or(|p| watch.p == Some(p))
            && self.after.is_none_or(|heard| watch.heard > heard)
    }

    /// What's being sent, for "Sending ... to the watch...".
    pub fn words(&self) -> String {
        match (self.m, self.gps, self.p) {
            (Some(m), None, None) => format!("the {m} m course"),
            (None, Some(true), None) => "GPS speed on".into(),
            (None, Some(false), None) => "GPS speed off".into(),
            (None, None, Some(p)) => format!("the speeds {}", speeds_choice(p)),
            _ => "the change".into(),
        }
    }
}

/// A speed progression as the watch's Speeds menu says it: its first two
/// speeds, "8, 8.5" (km/h at levels 1 and 2).
pub(crate) fn speeds_choice(p: u32) -> String {
    let [a, b] = tables::PROGRESSIONS
        .get(p as usize)
        .copied()
        .unwrap_or(tables::PROGRESSIONS[0]);
    format!("{a}, {b}")
}

/// The page's settings sent and how they went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub sent: Option<Sent>,
    /// The last one sent got no answer.
    pub unanswered: bool,
}

impl State {
    /// Sends a change of the watch's settings, unless it's what the watch
    /// will have already: what was sent and not yet answered, or else what
    /// the watch has. So a tap back to the watch's own value while another
    /// is on its way goes too, and replaces it.
    pub fn send(&mut self, watch: &WatchState, m: Option<u32>, gps: Option<bool>, p: Option<u32>) {
        let sent = self.sent.as_ref();
        let m = m.filter(|m| sent.and_then(|s| s.m).or(watch.m) != Some(*m));
        let gps = gps.filter(|g| sent.and_then(|s| s.gps).or(watch.gps) != Some(*g));
        let p = p.filter(|p| sent.and_then(|s| s.p).or(watch.p) != Some(*p));
        if m.is_none() && gps.is_none() && p.is_none() {
            return;
        }
        platform::dispatch(Input::SetWatch { m, gps, p });
        // Changes sent one after the other wait together.
        let (m, gps, p) = match &self.sent {
            Some(s) => (m.or(s.m), gps.or(s.gps), p.or(s.p)),
            None => (m, gps, p),
        };
        let back = (m.is_some() && m == watch.m)
            || (gps.is_some() && gps == watch.gps)
            || (p.is_some() && p == watch.p);
        self.sent = Some(Sent {
            m,
            gps,
            p,
            at: Instant::now(),
            after: back.then_some(watch.heard),
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

/// What the page says when the watch didn't answer: what to check. A
/// watch app older than 0.05 has nothing that answers (it only logs the
/// phone's line), so that's said too, with where to get a newer one.
pub(crate) fn no_answer_words(direct: bool) -> String {
    let check = if direct {
        "Check it's near"
    } else {
        "Check it's connected in Gadgetbridge, with \"Allow Intents\" on in Gadgetbridge's \
         settings for the Bangle.js"
    };
    format!("The watch didn't answer. {check}, and that it has Beep Test {NEEDS} or later. {OLDER}")
}

/// The first watch app version that syncs.
pub(crate) const NEEDS: &str = "0.05";
/// What an older watch app does, and where a newer one is.
pub(crate) const OLDER: &str = "Older versions don't sync: install a newer one from \
                                cbishopvelti.github.io/beep (the Bangle.js App Loader).";

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
                    ui.label(no_answer_words(direct));
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
        // When, with an amber dot while a sync is under way (what it's
        // doing is under the card, by Sync now).
        let summary = synced.map_or_else(|| "Never".into(), |t| health::ago(t, now));
        let dot = match synced {
            _ if progress.active() => Dot::Amber,
            Some(_) => Dot::Green,
            None => Dot::Off,
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
                     from cbishopvelti.github.io/beep (the Bangle.js App Loader).",
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

/// A chip of a setting's row ([`choices`]).
struct Choice {
    text: String,
    /// The watch's own.
    on: bool,
    /// Sent, and waiting for the watch: marked " …".
    waiting: bool,
}

/// A setting's row: `label`, then a chip for each choice, beside the label
/// on the right when they fit there (with room for one of them marked " …",
/// so the row doesn't move when one is sent), else on their own line under
/// it, wrapping if even that is too narrow (a small phone with large
/// text). Returns which was tapped.
fn choices(ui: &mut egui::Ui, label: &str, chips: &[Choice]) -> Option<usize> {
    const TALL: f32 = 40.0;
    const GAP: f32 = 6.0;
    let text = |c: &Choice| {
        if c.waiting {
            format!("{} …", c.text)
        } else {
            c.text.clone()
        }
    };
    let label_w = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            egui::TextStyle::Body.resolve(ui.style()),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x;
    let marked = chips
        .iter()
        .map(|c| chip::width(ui, &format!("{} …", c.text), TALL) - chip::width(ui, &c.text, TALL))
        .fold(0.0, f32::max);
    let chips_w: f32 = chips
        .iter()
        .map(|c| chip::width(ui, &c.text, TALL) + GAP)
        .sum::<f32>()
        - GAP
        + marked;
    let beside = label_w + ui.spacing().item_spacing.x + chips_w <= ui.available_width();
    let mut tapped = None;
    let mut add = |ui: &mut egui::Ui, i: usize, c: &Choice| {
        if chip::choice(ui, &text(c), c.on, TALL).clicked() {
            tapped = Some(i);
        }
    };
    if beside {
        ui.horizontal(|ui| {
            ui.set_min_height(44.0);
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                // Right to left: the last first.
                for (i, c) in chips.iter().enumerate().rev() {
                    add(ui, i, c);
                }
            });
        });
    } else {
        ui.label(label);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
            for (i, c) in chips.iter().enumerate() {
                add(ui, i, c);
            }
        });
    }
    tapped
}

/// Course, GPS speed and (when the watch has the setting) Speeds, as the
/// watch has them, and what was sent.
fn settings_card(app: &mut App, ui: &mut egui::Ui, watch: &WatchState) {
    let sent = app.watch.sent.clone();
    let mut m = None;
    let mut gps = None;
    let mut p = None;
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        let courses: Vec<Choice> = tables::COURSES
            .iter()
            .map(|&course| Choice {
                text: format!("{course} m"),
                on: watch.m == Some(course),
                waiting: sent.as_ref().is_some_and(|s| s.m == Some(course)),
            })
            .collect();
        if let Some(i) = choices(ui, "Course", &courses) {
            m = Some(tables::COURSES[i]);
        }
        // Only a watch app with the Speeds setting says which it has.
        if let Some(now) = watch.p {
            let speeds: Vec<Choice> = (0..tables::PROGRESSIONS.len() as u32)
                .map(|choice| Choice {
                    text: speeds_choice(choice),
                    on: now == choice,
                    waiting: sent.as_ref().is_some_and(|s| s.p == Some(choice)),
                })
                .collect();
            if let Some(i) = choices(ui, "Speeds", &speeds) {
                p = Some(i as u32);
            }
            ui.label(
                RichText::new(format!(
                    "km/h at levels 1 and 2, then 0.5 more a level: {} now. 8, 9 is the \
                     standard beep test.",
                    tables::speeds_text(now)
                ))
                .small()
                .weak(),
            );
        }
        // The switch shows a change on its way (a switch that sprang back
        // would send the same again when tapped); the course and speeds
        // show the watch's, with the one on its way marked.
        let mut on = sent
            .as_ref()
            .and_then(|s| s.gps)
            .or(watch.gps)
            .unwrap_or(false);
        if toggle::show(ui, &mut on, "Record GPS speed").changed() {
            gps = Some(on);
        }
        // How the last change went, when there's something to say (an
        // empty row would leave a gap).
        let say = sent.is_some() || app.watch.unanswered || watch.m.is_none();
        if say {
            ui.horizontal_wrapped(|ui| {
                if let Some(s) = &sent {
                    super::spinner(ui);
                    let what = s.words();
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
                        RichText::new(
                            "Not heard from the watch yet: its settings show once it syncs.",
                        )
                        .small()
                        .weak(),
                    );
                }
            });
        }
        ui.label(
            RichText::new(
                "These are the watch's own, used for the next test it runs. A change here \
                 goes to the watch and shows once it answers.",
            )
            .small()
            .weak(),
        );
    });
    if m.is_some() || gps.is_some() || p.is_some() {
        app.watch.send(watch, m, gps, p);
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
                p: None,
                at,
                after: None,
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
                p: None,
                at,
                after: None,
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
        s.send(&watch(20, true), Some(20), Some(true), None);
        assert_eq!(s.sent, None);
        let w = WatchState {
            p: Some(1),
            ..watch(20, true)
        };
        s.send(&w, None, None, Some(1));
        assert_eq!(s.sent, None);
    }

    #[test]
    fn a_change_on_its_way_can_be_changed_back() {
        // The watch is on 20 m; 15 m is sent, then 20 m before it answers:
        // 20 m goes too, and is what the page waits for.
        let w = watch(20, false);
        let mut s = State::default();
        s.send(&w, Some(15), None, None);
        assert_eq!(s.sent.as_ref().unwrap().m, Some(15));
        s.send(&w, Some(20), None, None);
        assert_eq!(s.sent.as_ref().unwrap().m, Some(20));
        // What the watch said before isn't the answer, though it says 20.
        s.settle(&w, Instant::now());
        assert!(s.sent.is_some());
        // The watch answers the first (15): still waiting for 20.
        let heard = |m, s| WatchState {
            heard: Some(s),
            ..watch(m, false)
        };
        s.settle(&heard(15, 100), Instant::now());
        assert!(s.sent.is_some());
        s.settle(&heard(20, 101), Instant::now());
        assert_eq!(s.sent, None);
        // GPS on, then off again before the answer: off goes.
        s.send(&w, None, Some(true), None);
        s.send(&w, None, Some(false), None);
        let sent = s.sent.clone().unwrap();
        assert_eq!(sent.gps, Some(false));
        assert_eq!(sent.words(), "GPS speed off");
        // The same again as what's on its way: nothing more.
        let at = sent.at;
        s.send(&w, None, Some(false), None);
        assert_eq!(s.sent.as_ref().unwrap().at, at);
        // The speeds, the same way.
        let w = WatchState { p: Some(0), ..w };
        let mut s = State::default();
        s.send(&w, None, None, Some(2));
        s.send(&w, None, None, Some(0));
        assert_eq!(s.sent.as_ref().unwrap().p, Some(0));
    }

    #[test]
    fn speeds_wait_for_the_watch_too() {
        let at = Instant::now();
        let sent = Sent {
            m: None,
            gps: None,
            p: Some(2),
            at,
            after: None,
        };
        assert_eq!(sent.words(), "the speeds 8.5, 9.5");
        let mut s = State {
            sent: Some(sent),
            unanswered: false,
        };
        let mut w = WatchState {
            p: Some(0),
            ..watch(20, false)
        };
        s.settle(&w, at + Duration::from_secs(1));
        assert!(s.sent.is_some());
        w.p = Some(2);
        s.settle(&w, at + Duration::from_secs(2));
        assert_eq!(s.sent, None);
        assert_eq!(
            (0..3).map(speeds_choice).collect::<Vec<_>>(),
            ["8, 9", "8, 8.5", "8.5, 9.5"]
        );
    }

    #[test]
    fn no_answer_says_which_watch_app_is_needed_and_where_it_is() {
        for direct in [false, true] {
            let words = no_answer_words(direct);
            assert!(words.contains("Beep Test 0.05 or later"), "{words}");
            assert!(words.contains("cbishopvelti.github.io/beep"), "{words}");
            assert!(words.contains("Gadgetbridge") != direct, "{words}");
        }
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
        // Location off: something to do on the phone, as Bluetooth off.
        assert_eq!(direct(Status::LocationOff).0, Dot::Red);
        assert_eq!(direct(Status::LocationOff).1, "Turn on Location to find it");
        assert_eq!(
            direct(Status::BluetoothOff),
            (Dot::Red, "Bluetooth is off".into())
        );
    }
}
