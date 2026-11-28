//! The Settings page, as blink's: the theme, light, dark or the phone's
//! (`Store::theme`, applied each frame by [`Themer`], with
//! `platform::system_dark` and `platform::light_bars` for the status bar's
//! icons); "Connect to the watch directly, without Gadgetbridge" with the
//! pop-up that picks the watch (`watch_ble::settings` / `save`,
//! `platform::keep_watch`, `platform::scan_watches`,
//! `watch_ble::found_watches`, the Nearby devices permission); and the
//! app's version (`CARGO_PKG_VERSION`).

use super::{App, Dialog, card, chip, dialog, section, toggle};
use crate::platform;
use crate::store::{Store, Theme};
use crate::watch_ble::{self, Status};
use egui::{Color32, RichText, Vec2};
use std::time::{Duration, Instant};

/// What Settings calls a theme.
pub(crate) fn label(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "Same as the phone",
        Theme::Light => "Light",
        Theme::Dark => "Dark",
    }
}

/// Keeps egui, and the phone's status bar, on the chosen theme, as
/// blink's.
#[derive(Debug)]
pub struct Themer {
    pub theme: Theme,
    /// What was last applied: light or not.
    applied: Option<bool>,
    checked: Option<Instant>,
}

impl Themer {
    pub fn new(store: &Store) -> Themer {
        Themer {
            theme: store.theme(),
            applied: None,
            checked: None,
        }
    }

    /// Applies the theme. On "Same as the phone" it asks the phone every
    /// couple of seconds, so switching the phone to dark mode follows.
    pub fn apply(&mut self, ctx: &egui::Context) {
        let due = self
            .checked
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2));
        if !due && self.applied.is_some() {
            return;
        }
        self.checked = Some(Instant::now());
        let preference = match self.theme {
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
            Theme::System => match platform::system_dark() {
                Some(true) => egui::ThemePreference::Dark,
                Some(false) => egui::ThemePreference::Light,
                None => egui::ThemePreference::System,
            },
        };
        ctx.set_theme(preference);
        let light = ctx.theme() == egui::Theme::Light;
        if self.applied != Some(light) {
            self.applied = Some(light);
            platform::light_bars(light);
        }
    }

    pub fn pick(&mut self, store: &Store, theme: Theme) -> std::io::Result<()> {
        store.set_theme(theme)?;
        self.theme = theme;
        // Apply on the next frame.
        self.checked = None;
        Ok(())
    }
}

/// Draws the page.
pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {
    section(ui, "Theme");
    card(ui, |ui| {
        // Close together: three short choices don't need a full touch
        // target's gap between them.
        let s = ui.spacing_mut();
        s.item_spacing.y = 0.0;
        s.interact_size.y = 40.0;
        for theme in Theme::ALL {
            if ui.radio(app.themer.theme == theme, label(theme)).clicked()
                && app.themer.theme != theme
                && let Err(e) = app.themer.pick(&app.store, theme)
            {
                app.error = Some(format!("Couldn't save the theme: {e}"));
            }
        }
    });

    ui.add_space(12.0);
    section(ui, "Watch");
    watch(app, ui);

    ui.add_space(12.0);
    ui.label(RichText::new(format!("Beep Test {}", env!("CARGO_PKG_VERSION"))).weak());
}

/// How the phone reaches the watch: through Gadgetbridge (the default), or
/// connected directly, with the watch it connects to and how that's going.
fn watch(app: &mut App, ui: &mut egui::Ui) {
    let saved = watch_ble::settings();
    let mut settings = saved.clone();
    let mut pick = false;
    card(ui, |ui| {
        toggle::show(
            ui,
            &mut settings.direct,
            "Connect to the watch directly, without Gadgetbridge",
        );
        if settings.direct {
            let status = watch_ble::status();
            ui.add_space(4.0);
            chip::row(ui, |ui| {
                let name = settings
                    .watch
                    .as_ref()
                    .map_or("No watch picked", |w| &w.name);
                ui.label(format!("⌚ {name}"));
                if settings.watch.is_some() {
                    ui.label(
                        RichText::new(status.words())
                            .small()
                            .color(color(ui, status)),
                    );
                }
                let label = if settings.watch.is_some() {
                    "Change"
                } else {
                    "Pick the watch"
                };
                pick |= button(ui, label);
                if status == Status::NotAllowed && button(ui, "Allow") {
                    platform::request_bluetooth();
                }
            });
            ui.add_space(4.0);
        }
        ui.label(
            RichText::new(
                "The watch takes one phone at a time: disconnect it in Gadgetbridge first. \
                 Beep Test then syncs with the watch itself, with a quiet notification while \
                 it's connected.",
            )
            .weak()
            .small(),
        );
    });
    if settings.direct != saved.direct {
        if let Err(e) = watch_ble::save(&settings) {
            app.error = Some(format!("Couldn't save the setting: {e}"));
        } else if settings.direct {
            if !platform::bluetooth_allowed() {
                platform::request_bluetooth();
            }
            pick |= settings.watch.is_none();
        } else if matches!(app.dialog, Some(Dialog::Picker { .. })) {
            app.close_dialog();
        }
        platform::keep_watch();
    }
    if pick {
        app.dialog = Some(Dialog::Picker {
            since: Instant::now(),
            scanning: false,
        });
    }
}

/// A small chip to tap.
fn button(ui: &mut egui::Ui, text: &str) -> bool {
    chip::show(ui, text, chip::Kind::Button).clicked()
}

fn color(ui: &egui::Ui, status: Status) -> Color32 {
    match status {
        Status::Connected => super::status::green_text(ui),
        Status::Connecting | Status::SettingUp | Status::NoWatch | Status::Off => {
            super::status::amber_text(ui)
        }
        _ => super::RED,
    }
}

/// What the picker says under the watches it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hint {
    /// Android 11 and earlier with Location off: a scan finds nothing.
    LocationOff(&'static str),
    /// Nothing found for a while.
    NoneFound(&'static str),
}

/// The picker's hint, with the phone's Location off (where scanning needs
/// it), nothing found yet, and `looking` this long: Location first, since
/// nothing shows without it, and the other hint wouldn't help.
pub(crate) fn picker_hint(location_off: bool, none_found: bool, looking: Duration) -> Option<Hint> {
    if location_off {
        Some(Hint::LocationOff(
            "Turn on Location: Android 11 and earlier only find Bluetooth watches with it on.",
        ))
    } else if none_found && looking > Duration::from_secs(6) {
        Some(Hint::NoneFound(
            "A watch only shows here while no phone is connected to it: disconnect it in \
             Gadgetbridge first, and keep it near.",
        ))
    } else {
        None
    }
}

/// The pop-up that finds the watch: the Bangle.js watches about, nearest
/// first, to tap one. Picking one saves it and connects.
pub(super) fn picker(app: &mut App, ctx: &egui::Context) {
    let Some(Dialog::Picker { since, scanning }) = app.dialog.clone() else {
        return;
    };
    let allowed = platform::bluetooth_allowed();
    if platform::HAS_BLUETOOTH && allowed && !scanning {
        platform::scan_watches(true);
        app.dialog = Some(Dialog::Picker {
            since: Instant::now(),
            scanning: true,
        });
    }
    let mut picked = None;
    let mut cancel = false;
    let modal = dialog::modal(ctx, "watch_picker_modal").show(ctx, |ui| {
        dialog::begin(ui);
        dialog::title(ui, "Pick your watch");
        ui.add_space(8.0);
        if !platform::HAS_BLUETOOTH {
            ui.label(RichText::new("Only the phone connects to the watch.").weak());
        } else if !allowed {
            ui.label("Beep Test needs the Nearby devices permission to find the watch.");
            if chip::row(ui, |ui| button(ui, "Allow")) {
                platform::request_bluetooth();
            }
        } else {
            let found = watch_ble::found_watches();
            for w in &found {
                let signal = match w.rssi {
                    r if r >= -60 => "near",
                    r if r >= -80 => "in range",
                    _ => "far",
                };
                let row = egui::Button::new(RichText::new(format!("⌚ {}  ·  {signal}", w.name)))
                    .frame(false)
                    .min_size(Vec2::new(ui.available_width(), 40.0));
                if ui.add(row).clicked() {
                    picked = Some(w.clone());
                }
            }
            ui.horizontal(|ui| {
                super::spinner(ui);
                ui.label(RichText::new("Looking for watches...").weak());
            });
            match picker_hint(platform::location_off(), found.is_empty(), since.elapsed()) {
                Some(Hint::LocationOff(words)) => {
                    ui.label(
                        RichText::new(words)
                            .small()
                            .color(super::status::amber_text(ui)),
                    );
                }
                Some(Hint::NoneFound(words)) => {
                    ui.label(RichText::new(words).weak().small());
                }
                None => {}
            }
        }
        dialog::actions(ui, |ui| {
            cancel = dialog::button(ui, "Cancel", None);
        });
    });
    if let Some(found) = picked {
        let mut settings = watch_ble::settings();
        settings.watch = Some(watch_ble::Watch {
            address: found.address,
            name: found.name,
        });
        match watch_ble::save(&settings) {
            Ok(()) => platform::keep_watch(),
            Err(e) => app.error = Some(format!("Couldn't save the watch: {e}")),
        }
        app.close_dialog();
    } else if cancel || modal.should_close() {
        app.close_dialog();
    } else {
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picker_says_to_turn_location_on_before_blaming_gadgetbridge() {
        let s = Duration::from_secs;
        assert_eq!(picker_hint(false, true, s(2)), None);
        assert!(matches!(
            picker_hint(false, true, s(7)),
            Some(Hint::NoneFound(w)) if w.contains("Gadgetbridge")
        ));
        assert_eq!(picker_hint(false, false, s(60)), None);
        for (none, looking) in [(true, 0), (true, 60), (false, 60)] {
            assert!(matches!(
                picker_hint(true, none, s(looking)),
                Some(Hint::LocationOff(w)) if w.starts_with("Turn on Location")
            ));
        }
    }

    #[test]
    fn the_theme_is_kept_and_applied() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut themer = Themer::new(&store);
        assert_eq!(themer.theme, Theme::System);
        themer.pick(&store, Theme::Dark).unwrap();
        assert_eq!(store.theme(), Theme::Dark);
        let ctx = egui::Context::default();
        themer.apply(&ctx);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        themer.pick(&store, Theme::Light).unwrap();
        themer.apply(&ctx);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(Themer::new(&store).theme, Theme::Light);
        assert_eq!(label(Theme::System), "Same as the phone");
    }
}
