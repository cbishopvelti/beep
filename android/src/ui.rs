//! The egui UI, laid out for a phone as blink's: Android-sized text and
//! touch targets, cards, and a menu from the header. The pages:
//!
//! - **Results** (`ui/results.rs`, the first page): a card a result, newest
//!   first, the progress chart above, the running-test banner, the sync
//!   button and what the sync is doing.
//! - **A result** (`ui/result.rs`, from tapping a card): the summary, the
//!   heart rate chart with the levels marked, a row per level, Share TCX
//!   and Share CSV, Delete.
//! - **Watch** (`ui/watch.rs`): how the watch is connected, when it was last
//!   heard from and synced, its app's version, Sync now, and its course and
//!   GPS settings.
//! - **Settings** (`ui/settings.rs`): the theme, the direct link with its
//!   picker, the app's version.
//!
//! What the pages draw from, without reaching into the engine:
//!
//! - the store ([`crate::store::Store`]): `results()` (each with its
//!   `LogState`), `bests()`, `watch_state()` (the running test, the watch's
//!   version and settings), `log(f)` with [`crate::beeplog::level_rows`] and
//!   [`crate::beeplog::hr_series`], `export(f, kind, min_confidence)` for
//!   [`crate::platform::share_file`], and `theme()` / `set_theme()`;
//! - the sync's progress ([`crate::platform::progress`]:
//!   [`crate::sync::Progress::words`] and `active()`);
//! - [`crate::health::last_seen`] with the store's directory, for when the
//!   watch was last heard from and last synced;
//! - [`crate::watch_ble::settings`], `status()` and `found_watches()` for
//!   the direct link;
//! - and for what the user does, [`crate::platform::dispatch`] with
//!   [`crate::sync::Input`] `SyncNow`, `Delete(f)` and `SetWatch`.
//!
//! Each frame the app compares [`crate::platform::generation`] with the one
//! it last loaded and reloads what it shows from the store when it moved;
//! inputs from other threads also ask for a repaint, through the context
//! given to [`crate::platform::set_repaint`]. On its first frame it
//! dispatches `Input::Opened`, and it dispatches `Input::Tick` about once a
//! second while open (`request_repaint_after`).

use crate::platform;
use crate::store::Store;
use egui::{Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, TextStyle, Vec2};
use std::sync::Arc;

mod result;
mod results;
mod settings;
mod watch;

/// The pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Results,
    /// A result, by its log's file name.
    Result(String),
    Watch,
    Settings,
}

impl Page {
    /// The pages the menu offers, in order (a result opens from Results).
    pub const MENU: [Page; 3] = [Page::Results, Page::Watch, Page::Settings];

    /// What the header and the menu call it.
    pub fn title(&self) -> &'static str {
        match self {
            Page::Results => "Results",
            Page::Result(_) => "Result",
            Page::Watch => "Watch",
            Page::Settings => "Settings",
        }
    }
}

/// The accent: buttons, the selected page, switches.
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x7c, 0xff);
const MARGIN: f32 = 16.0;
/// The bold face's family name (`fonts/Ubuntu-Medium.ttf`, beside egui's
/// own Ubuntu Light; UFL).
pub(crate) const BOLD: &str = "bold";

/// The app: the page open and what it shows.
pub struct App {
    pub(crate) store: Arc<Store>,
    pub(crate) page: Page,
    menu_open: bool,
    styled: bool,
}

impl App {
    pub fn new(store: Arc<Store>) -> App {
        App {
            store,
            page: Page::Results,
            menu_open: false,
            styled: false,
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(RichText::new("\u{2630}").size(22.0)).frame(false))
                .on_hover_text("Menu")
                .clicked()
            {
                self.menu_open = !self.menu_open;
            }
            ui.label(RichText::new(self.page.title()).heading());
        });
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(RichText::new("Beep Test").size(20.0).strong());
        ui.add_space(8.0);
        for page in Page::MENU {
            let on = self.page == page
                || (page == Page::Results && matches!(self.page, Page::Result(_)));
            let text = RichText::new(page.title()).size(18.0);
            let text = if on { text.color(ACCENT) } else { text };
            if ui
                .add_sized(
                    [ui.available_width(), 48.0],
                    egui::Button::new(text).frame(false),
                )
                .clicked()
            {
                self.page = page;
                self.menu_open = false;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.styled {
            apply_theme(ctx);
            platform::set_repaint(ctx);
            self.styled = true;
        }
        let ppp = ctx.pixels_per_point();
        let top = platform::status_bar_px() / ppp;
        let bottom = platform::nav_bar_px() / ppp;
        let bar = ctx.style().visuals.panel_fill;
        egui::TopBottomPanel::top("header")
            .frame(Frame::new().fill(bar).inner_margin(Margin {
                left: MARGIN as i8,
                right: MARGIN as i8,
                top: (top + 8.0) as i8,
                bottom: 8,
            }))
            .show_separator_line(false)
            .show(ctx, |ui| self.header(ui));
        if self.menu_open {
            egui::SidePanel::left("menu")
                .resizable(false)
                .exact_width(240.0)
                .frame(
                    Frame::new()
                        .fill(bar)
                        .inner_margin(Margin::same(MARGIN as i8)),
                )
                .show(ctx, |ui| self.menu(ui));
        }
        egui::CentralPanel::default()
            .frame(Frame::new().fill(bar).inner_margin(Margin {
                left: MARGIN as i8,
                right: MARGIN as i8,
                top: 0,
                bottom: bottom as i8,
            }))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page.clone() {
                        Page::Results => results::show(self, ui),
                        Page::Result(f) => result::show(self, ui, &f),
                        Page::Watch => watch::show(self, ui),
                        Page::Settings => settings::show(self, ui),
                    });
            });
    }
}

/// Android-like sizes, as blink's: 17 sp body text, 48 dp touch targets,
/// and the bold face.
fn apply_theme(ctx: &egui::Context) {
    add_fonts(ctx);
    if let Some(density) = platform::density() {
        ctx.set_pixels_per_point(density);
    }
    ctx.all_styles_mut(|style| {
        use egui::FontFamily::Proportional;
        style.text_styles = [
            (TextStyle::Heading, FontId::new(26.0, Proportional)),
            (TextStyle::Body, FontId::new(17.0, Proportional)),
            (TextStyle::Button, FontId::new(17.0, Proportional)),
            (
                TextStyle::Monospace,
                FontId::new(15.0, egui::FontFamily::Monospace),
            ),
            (TextStyle::Small, FontId::new(14.0, Proportional)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(10.0, 12.0);
        s.button_padding = Vec2::new(18.0, 12.0);
        s.interact_size = Vec2::new(48.0, 48.0);
        s.scroll.bar_width = 4.0;
        let v = &mut style.visuals;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
        v.hyperlink_color = ACCENT;
        let radius = CornerRadius::same(12);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = radius;
        }
        v.widgets.inactive.expansion = 0.0;
    });
}

/// Adds the bold face (`fonts/Ubuntu-Medium.ttf`) to egui's own fonts, as
/// the family [`BOLD`].
fn add_fonts(ctx: &egui::Context) {
    use egui::FontFamily;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Ubuntu-Medium".into(),
        egui::FontData::from_static(include_bytes!("../fonts/Ubuntu-Medium.ttf")).into(),
    );
    let mut bold = vec!["Ubuntu-Medium".to_string()];
    bold.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    ctx.set_fonts(fonts);
}
