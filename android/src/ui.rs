//! The egui UI, laid out for a phone as blink's: Android-sized text and
//! touch targets, cards, and a menu that slides out from the header. The
//! pages:
//!
//! - **Results** (`ui/results.rs`, the first page): a card a result, newest
//!   first, the progress chart above, the running-test banner, the sync
//!   button and what the sync is doing.
//! - **A result** (`ui/result.rs`, from tapping a card): the summary, the
//!   heart rate chart with the levels marked, a row per level, Share TCX
//!   and Share CSV, Delete.
//! - **Watch** (`ui/watch.rs`): how the watch is connected, when it was last
//!   heard from and synced, its app's version, Sync now, and its course and
//!   GPS settings (and its speeds, when the watch's app has that setting).
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
//! it last loaded and reloads what it shows from the store when it moved
//! ([`View`]); inputs from other threads also ask for a repaint, through the
//! context given to [`crate::platform::set_repaint`]. On its first frame it
//! dispatches `Input::Opened`, and it dispatches `Input::Tick` about once a
//! second while open (`request_repaint_after`).
//!
//! **The back key** (Android's, which reaches egui as `Key::BrowserBack`;
//! Escape on the desktop) undoes the last step ([`App::back`]): the menu or
//! a pop-up closes first, then a result goes back to Results, then Watch or
//! Settings goes back to Results. On Results Android's back moves the app
//! to the background ([`crate::platform::leave`]): winit reports every key
//! as handled, so Android wouldn't do it itself.
//!
//! [`App::show`] is a frame without eframe, so the tests drive the pages
//! headlessly (`tests/ui_*.rs`, with `egui_kittest`).

use crate::beeplog;
use crate::platform;
use crate::store::{Store, TestResult, WatchState};
use crate::sync::Input;
use egui::emath::GuiRounding as _;
use egui::{Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, TextStyle, Vec2};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

mod chart;
mod chip;
pub mod demo;
mod dialog;
mod result;
mod results;
mod settings;
mod status;
mod toggle;
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

    /// The menu's icon for it.
    fn icon(&self) -> &'static str {
        match self {
            Page::Results | Page::Result(_) => "🏆",
            Page::Watch => "⌚",
            Page::Settings => "⚙",
        }
    }
}

/// What the back key does next ([`App::back_step`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// Shuts the menu.
    Menu,
    /// Closes the pop-up showing (Delete's confirm, the watch picker).
    Dialog,
    /// Goes back to Results.
    ToResults,
}

/// The pop-ups a page can have open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Dialog {
    /// "Delete this result?", for the result whose log is this.
    Delete(String),
    /// Settings' watch picker, looking since then (and whether the scan was
    /// started).
    Picker { since: Instant, scanning: bool },
}

/// What the pages show from the store, read again when
/// [`platform::generation`] moves.
#[derive(Debug, Default)]
pub(crate) struct View {
    /// Newest first.
    pub results: Vec<TestResult>,
    /// The best result on each course and speed progression: (course in
    /// m, `p`) to its log's name.
    pub bests: BTreeMap<(u32, u32), String>,
    pub watch: WatchState,
    /// The open result's log, read whole, while it's complete.
    pub log: Option<(String, beeplog::Log)>,
}

impl View {
    fn load(store: &Store, page: &Page) -> View {
        let log = match page {
            Page::Result(f) => store.log(f).map(|l| (f.clone(), l)),
            _ => None,
        };
        View {
            results: store.results(),
            bests: store.bests(),
            watch: store.watch_state(),
            log,
        }
    }

    pub fn result(&self, f: &str) -> Option<&TestResult> {
        self.results.iter().find(|r| r.entry.f == f)
    }

    /// Whether `r` is the best on its course and speed progression.
    pub fn is_best(&self, r: &TestResult) -> bool {
        self.bests.get(&(r.entry.m, r.entry.p)) == Some(&r.entry.f)
    }
}

/// The accent: buttons, the selected page, switches, the 20 m line (on
/// the standard speeds).
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x7c, 0xff);
/// Green, amber and red: a dot's colours, as blink's Status page.
pub(crate) const GREEN: Color32 = Color32::from_rgb(0x2e, 0xb8, 0x5c);
pub(crate) const AMBER: Color32 = Color32::from_rgb(0xf0, 0xa8, 0x20);
pub(crate) const RED: Color32 = Color32::from_rgb(0xe0, 0x4a, 0x4a);
const MARGIN: f32 = 16.0;
/// The bold face's family name (`fonts/Ubuntu-Medium.ttf`, beside egui's
/// own Ubuntu Light; UFL).
pub(crate) const BOLD: &str = "bold";

/// How often the engine hears the clock while the app is open.
const TICK: Duration = Duration::from_secs(1);

/// The app: the page open and what it shows.
pub struct App {
    pub(crate) store: Arc<Store>,
    pub(crate) page: Page,
    pub(crate) view: View,
    pub(crate) dialog: Option<Dialog>,
    /// The result page's own state (the confidence option, what a share
    /// said).
    pub(crate) result: result::State,
    /// The Watch page's settings sent and not yet answered.
    pub(crate) watch: watch::State,
    pub(crate) themer: settings::Themer,
    /// An error to show at the top of the page (a setting that didn't
    /// save), until the page changes.
    pub(crate) error: Option<String>,
    menu_open: bool,
    /// Where the header's menu button was drawn, so the one over the open
    /// menu sits exactly on it.
    menu_button: Option<egui::Rect>,
    styled: bool,
    opened: bool,
    /// The generation [`View`] was loaded at.
    generation: Option<u64>,
    last_tick: Option<Instant>,
    /// When the phone's font size was last looked at.
    text_checked: Option<Instant>,
    /// Scroll the page back to the top (after opening a page other than
    /// Results, which keeps its place: back from a result is where it was).
    to_top: bool,
}

impl App {
    pub fn new(store: Arc<Store>) -> App {
        let themer = settings::Themer::new(&store);
        App {
            store,
            page: Page::Results,
            view: View::default(),
            dialog: None,
            result: result::State::default(),
            watch: watch::State::default(),
            themer,
            error: None,
            menu_open: false,
            menu_button: None,
            styled: false,
            opened: false,
            generation: None,
            last_tick: None,
            text_checked: None,
            to_top: false,
        }
    }

    /// The page open.
    pub fn page(&self) -> &Page {
        &self.page
    }

    /// Opens `page` (and what it shows read again).
    pub fn open(&mut self, page: Page) {
        if page != self.page {
            self.to_top = page != Page::Results;
            self.page = page;
            self.result = result::State::default();
            self.error = None;
        }
        self.menu_open = false;
        self.generation = None;
    }

    /// Slides the menu out, as its button does.
    pub fn open_menu(&mut self) {
        self.menu_open = true;
    }

    /// Sends the watch a course (20 or 15), GPS setting or speed
    /// progression, as the Watch page's controls do: the page shows it
    /// waiting for the watch's answer (the desktop's screenshots use it).
    pub fn set_watch(&mut self, m: Option<u32>, gps: Option<bool>, p: Option<u32>) {
        self.reload();
        self.watch.send(&self.view.watch, m, gps, p);
    }

    /// Asks "Delete this result?" for the open result (the desktop's
    /// screenshots use it; the page's Delete button does the same).
    pub fn ask_delete(&mut self) {
        if let Page::Result(f) = &self.page {
            self.dialog = Some(Dialog::Delete(f.clone()));
        }
    }

    /// What the back key would do now; `None` on Results with nothing open,
    /// where Android's back moves the app to the background
    /// ([`crate::platform::leave`]) and Escape does nothing.
    pub fn back_step(&self) -> Option<Back> {
        if self.menu_open {
            Some(Back::Menu)
        } else if self.dialog.is_some() {
            Some(Back::Dialog)
        } else if self.page != Page::Results {
            Some(Back::ToResults)
        } else {
            None
        }
    }

    /// Does what the back key does ([`App::back_step`]); false when it was
    /// left alone.
    pub fn back(&mut self) -> bool {
        match self.back_step() {
            Some(Back::Menu) => self.menu_open = false,
            Some(Back::Dialog) => self.close_dialog(),
            Some(Back::ToResults) => self.open(Page::Results),
            None => return false,
        }
        true
    }

    pub(crate) fn close_dialog(&mut self) {
        if let Some(Dialog::Picker { scanning: true, .. }) = self.dialog {
            platform::scan_watches(false);
        }
        self.dialog = None;
    }

    /// Reads what the pages show from the store again when it changed.
    fn reload(&mut self) {
        let generation = platform::generation();
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.view = View::load(&self.store, &self.page);
            // A result deleted (here or on the watch) while open: back to
            // the list.
            if let Page::Result(f) = &self.page
                && self.view.result(f).is_none()
            {
                self.open(Page::Results);
                self.generation = Some(generation);
                self.view = View::load(&self.store, &self.page);
            }
            if let Some(Dialog::Delete(f)) = &self.dialog
                && self.view.result(f).is_none()
            {
                self.dialog = None;
            }
        }
    }

    /// One frame of the app, with no eframe around it: what
    /// [`eframe::App::update`] runs, and what the tests drive.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.styled {
            apply_theme(ctx);
            platform::set_repaint(ctx);
            self.styled = true;
            // The fonts are there from the next frame: the bold face
            // isn't yet.
            ctx.request_repaint();
            return;
        }
        self.themer.apply(ctx);
        // The phone's font size, followed when it's changed in its
        // settings (the activity isn't made again for it: `configChanges`).
        if self
            .text_checked
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            self.text_checked = Some(Instant::now());
            let scale = scale_for(platform::font_scale());
            if scale != text_scale() {
                set_text_sizes(ctx, scale);
            }
        }
        if !self.opened {
            self.opened = true;
            platform::dispatch(Input::Opened);
            self.last_tick = Some(Instant::now());
        }
        if self.last_tick.is_none_or(|t| t.elapsed() >= TICK) {
            self.last_tick = Some(Instant::now());
            platform::dispatch(Input::Tick);
        }
        ctx.request_repaint_after(TICK);
        self.reload();

        // Android's back key, or Escape on the desktop (only when there's
        // somewhere to go back to). Back on Results leaves the app.
        if self.back_step().is_some() {
            let pressed = ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::BrowserBack)
                    || (cfg!(not(target_os = "android"))
                        && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            });
            if pressed {
                self.back();
                self.reload();
            }
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::BrowserBack)) {
            platform::leave();
        }

        let ppp = ctx.pixels_per_point();
        let top = platform::status_bar_px() / ppp;
        let bottom = platform::nav_bar_px() / ppp;
        let bar = ctx.style().visuals.panel_fill;
        let mut go_back = false;
        egui::TopBottomPanel::top("header")
            .frame(Frame::new().fill(bar).inner_margin(Margin {
                left: 4,
                right: MARGIN as i8,
                top: (top + 8.0).min(f32::from(i8::MAX)) as i8,
                bottom: 4,
            }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if matches!(self.page, Page::Result(_)) {
                        go_back = back_arrow(ui).on_hover_text("Back").clicked();
                    } else {
                        let r = hamburger(ui).on_hover_text("Menu");
                        if r.clicked() {
                            self.menu_open = !self.menu_open;
                        }
                        self.menu_button = Some(r.rect);
                    }
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(self.page.title()).font(medium(24.0)).strong(),
                        )
                        .truncate()
                        .selectable(false),
                    );
                });
            });
        if go_back {
            self.open(Page::Results);
            self.reload();
        }
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(bar)
                    .inner_margin(Margin::symmetric(MARGIN as i8, 0)),
            )
            .show(ctx, |ui| {
                // Results scrolls on its own, so it keeps its place while
                // a result (or another page) is open.
                let id = if self.page == Page::Results {
                    "results"
                } else {
                    "page"
                };
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(id)
                    .auto_shrink([false, false]);
                if std::mem::take(&mut self.to_top) {
                    scroll = scroll.vertical_scroll_offset(0.0);
                }
                scroll.show(ui, |ui| {
                    ui.add_space(8.0);
                    if let Some(e) = self.error.clone() {
                        card(ui, |ui| {
                            ui.colored_label(ui.visuals().error_fg_color, e);
                        });
                    }
                    match self.page.clone() {
                        Page::Results => results::show(self, ui),
                        Page::Result(f) => result::show(self, ui, &f),
                        Page::Watch => watch::show(self, ui),
                        Page::Settings => settings::show(self, ui),
                    }
                    // Clear of the navigation bar at the end.
                    ui.add_space(24.0 + bottom);
                });
            });
        dialog::set_bottom(ctx, bottom);
        result::delete_dialog(self, ctx);
        settings::picker(self, ctx);
        self.drawer(ctx, top, bottom);
        // Whatever a page dispatched shows from the next frame.
        self.reload();
    }

    /// The slide-out menu, as blink's: a scrim over the page and a drawer
    /// from the left, the full height of the screen with its items clear of
    /// the status bar and the navigation bar (`nav`, in points). A tap on
    /// the scrim shuts it.
    fn drawer(&mut self, ctx: &egui::Context, top: f32, nav: f32) {
        let t = ctx.animate_bool_with_time(egui::Id::new("drawer"), self.menu_open, 0.18);
        if t <= 0.0 {
            return;
        }
        let screen = ctx.content_rect();
        let width = (screen.width() * 0.8).min(320.0);
        let button = self.menu_button.unwrap_or_else(|| {
            egui::Rect::from_min_size(
                egui::pos2(screen.min.x + 4.0, screen.min.y + top + 8.0),
                Vec2::splat(MENU_BUTTON),
            )
        });
        let scrim = egui::Area::new(egui::Id::new("scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_black_alpha((140.0 * t) as u8));
                response
            });
        if scrim.inner.clicked() {
            self.menu_open = false;
        }
        let fill = ctx.style().visuals.window_fill;
        let bottom_margin = (12.0 + nav).round().min(f32::from(i8::MAX)) as i8;
        let mut picked = None;
        egui::Area::new(egui::Id::new("drawer_panel"))
            .order(egui::Order::Tooltip)
            .constrain(false)
            .fixed_pos(egui::pos2(screen.min.x - width * (1.0 - t), screen.min.y))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(fill)
                    .corner_radius(CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: 20,
                        se: 20,
                    })
                    .inner_margin(Margin {
                        left: 12,
                        right: 12,
                        top: 0,
                        bottom: bottom_margin,
                    })
                    .show(ui, |ui| {
                        ui.set_width(width - 24.0);
                        ui.set_min_height(screen.height() - f32::from(bottom_margin));
                        ui.add_space(button.min.y - screen.min.y);
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), button.height()),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add_space(48.0);
                                ui.label(
                                    RichText::new("Beep Test").font(medium(24.0)).color(ACCENT),
                                );
                            },
                        );
                        ui.add_space(16.0);
                        for page in Page::MENU {
                            let on = self.page == page
                                || (page == Page::Results && matches!(self.page, Page::Result(_)));
                            let text = RichText::new(format!("{}   {}", page.icon(), page.title()))
                                .size(18.0 * text_scale());
                            let text = if on {
                                text.color(ACCENT).strong()
                            } else {
                                text
                            };
                            let item = egui::Button::new(text)
                                .fill(if on {
                                    ACCENT.gamma_multiply(0.18)
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(Stroke::NONE)
                                .corner_radius(CornerRadius::same(24))
                                .min_size(Vec2::new(ui.available_width(), 52.0));
                            if ui.add(item).clicked() {
                                picked = Some(page);
                            }
                        }
                    });
            });
        if let Some(page) = picked {
            self.open(page);
        }
        // The menu button stays where it was, over the drawer, to shut it.
        egui::Area::new(egui::Id::new("drawer_toggle"))
            .order(egui::Order::Debug)
            .constrain(false)
            .fixed_pos(button.min)
            .show(ctx, |ui| {
                if hamburger(ui).on_hover_text("Close the menu").clicked() {
                    self.menu_open = false;
                }
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
    }
}

/// Android-like sizes, as blink's: 17 sp body text (times the phone's font
/// size setting, [`set_text_sizes`]), 48 dp touch targets, and the bold
/// face.
pub(crate) fn apply_theme(ctx: &egui::Context) {
    add_fonts(ctx);
    if let Some(density) = platform::density() {
        ctx.set_pixels_per_point(density);
    }
    set_text_sizes(ctx, scale_for(platform::font_scale()));
    ctx.all_styles_mut(|style| {
        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(10.0, 12.0);
        s.button_padding = Vec2::new(18.0, 12.0);
        s.interact_size = Vec2::new(48.0, 48.0);
        s.icon_width = 26.0;
        s.icon_width_inner = 14.0;
        s.icon_spacing = 12.0;
        s.scroll.bar_width = 4.0;
        // A phone's text isn't picked up by a press: a press on a card
        // opens it.
        style.interaction.selectable_labels = false;
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

/// The medium (bold) face at `size`, times the phone's text size
/// ([`text_scale`]).
pub(crate) fn medium(size: f32) -> FontId {
    FontId::new(size * text_scale(), egui::FontFamily::Name(BOLD.into()))
}

/// The most and least the phone's font size setting scales the text by:
/// past 1.3 the pages' rows (a result card's score beside its course, the
/// levels table on a small phone) no longer fit.
pub const TEXT_SCALE: std::ops::RangeInclusive<f32> = 0.85..=1.3;

thread_local! {
    /// The text scale applied ([`set_text_sizes`]). The UI draws on one
    /// thread, and it's that thread's: each test (a thread of its own)
    /// has its own, so tests at different sizes don't change each other's.
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// What the text sizes are multiplied by: Android's font size setting
/// (`Configuration.fontScale`), kept within [`TEXT_SCALE`]; 1 until known.
pub(crate) fn text_scale() -> f32 {
    SCALE.get()
}

/// The scale for the phone's font size setting `asked` (1 when it can't
/// say), within [`TEXT_SCALE`].
pub fn scale_for(asked: Option<f32>) -> f32 {
    asked
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(1.0)
        .clamp(*TEXT_SCALE.start(), *TEXT_SCALE.end())
}

/// The text styles at `scale` times the app's own sizes (17 sp body text,
/// as blink's), and [`medium`]'s too from now on.
pub fn set_text_sizes(ctx: &egui::Context, scale: f32) {
    SCALE.set(scale);
    ctx.all_styles_mut(|style| {
        use egui::FontFamily::Proportional;
        style.text_styles = [
            (TextStyle::Heading, FontId::new(26.0 * scale, Proportional)),
            (TextStyle::Body, FontId::new(17.0 * scale, Proportional)),
            (TextStyle::Button, FontId::new(17.0 * scale, Proportional)),
            (
                TextStyle::Monospace,
                FontId::new(15.0 * scale, egui::FontFamily::Monospace),
            ),
            (TextStyle::Small, FontId::new(14.0 * scale, Proportional)),
        ]
        .into();
    });
}

/// A card, as blink's: a faint fill, a hairline edge, round corners.
pub(crate) fn card<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, egui::Response) {
    let shown = Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::symmetric(16, chip::GAP as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    ui.add_space(4.0);
    (shown.inner, shown.response)
}

/// A full-width filled button.
pub(crate) fn primary(ui: &mut egui::Ui, text: &str) -> bool {
    let button = egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(ACCENT);
    ui.add_sized([ui.available_width(), 48.0], button).clicked()
}

/// A full-width outlined button, in `color` (the accent when `None`).
pub(crate) fn secondary(ui: &mut egui::Ui, text: &str, color: Option<Color32>) -> bool {
    let color = color.unwrap_or(ACCENT);
    let button = egui::Button::new(RichText::new(text).color(color))
        .stroke(Stroke::new(1.5_f32, color))
        .fill(Color32::TRANSPARENT);
    ui.add_sized([ui.available_width(), 48.0], button).clicked()
}

/// A page with nothing on it yet: a title and a few words.
pub(crate) fn empty(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.add_space(32.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).font(medium(20.0)).strong());
        ui.add_space(4.0);
        ui.label(RichText::new(detail).weak());
    });
}

/// A small heading over a card.
pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).weak());
}

/// A small spinner, the size of a line of text: something's under way.
pub(crate) fn spinner(ui: &mut egui::Ui) {
    ui.add(egui::Spinner::new().size(18.0));
}

/// The menu button's size, square.
const MENU_BUTTON: f32 = 44.0;

/// Three lines; opens the menu. It starts on a whole pixel, so the same
/// button drawn over the open menu lines up with it to the pixel.
fn hamburger(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(MENU_BUTTON), egui::Sense::click());
    let rect =
        egui::Rect::from_min_size(rect.min.round_to_pixels(ui.pixels_per_point()), rect.size());
    let color = ui.visuals().text_color();
    let stroke = Stroke::new(2.5_f32, color);
    let c = rect.center();
    for dy in [-7.0, 0.0, 7.0] {
        ui.painter().line_segment(
            [
                egui::pos2(c.x - 11.0, c.y + dy),
                egui::pos2(c.x + 11.0, c.y + dy),
            ],
            stroke,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Menu"));
    response.with_new_rect(rect)
}

/// An arrow pointing left: back to Results.
fn back_arrow(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(MENU_BUTTON), egui::Sense::click());
    let stroke = Stroke::new(2.5_f32, ui.visuals().text_color());
    let c = rect.center();
    let painter = ui.painter();
    let tip = c + Vec2::new(-10.0, 0.0);
    painter.line_segment([tip, c + Vec2::new(10.0, 0.0)], stroke);
    painter.line_segment([tip, c + Vec2::new(-3.0, -7.0)], stroke);
    painter.line_segment([tip, c + Vec2::new(-3.0, 7.0)], stroke);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Back"));
    response
}

/// "9:03", or "1:02:03" past the hour, for `s` seconds.
pub(crate) fn duration(s: f64) -> String {
    let s = if s.is_finite() {
        s.max(0.0).round() as u64
    } else {
        0
    };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path()).unwrap());
        (dir, App::new(store))
    }

    #[test]
    fn back_closes_what_is_open_first_then_goes_back_to_results() {
        let (_dir, mut app) = app();
        // On Results with nothing open it's Android's (which closes the
        // app).
        assert_eq!(app.back_step(), None);
        assert!(!app.back());

        app.open(Page::Result("beeptest.1764334800.csv".into()));
        app.ask_delete();
        app.menu_open = true;
        assert_eq!(app.back_step(), Some(Back::Menu));
        assert!(app.back());
        assert_eq!(app.back_step(), Some(Back::Dialog));
        assert!(app.back());
        assert_eq!(app.dialog, None);
        assert_eq!(app.page, Page::Result("beeptest.1764334800.csv".into()));
        assert_eq!(app.back_step(), Some(Back::ToResults));
        assert!(app.back());
        assert_eq!(app.page, Page::Results);
        assert_eq!(app.back_step(), None);

        for page in [Page::Watch, Page::Settings] {
            app.open(page);
            assert!(app.back());
            assert_eq!(app.page, Page::Results);
        }
    }

    #[test]
    fn delete_is_only_asked_on_a_result() {
        let (_dir, mut app) = app();
        app.ask_delete();
        assert_eq!(app.dialog, None);
        app.open(Page::Result("beeptest.1.csv".into()));
        app.ask_delete();
        assert_eq!(app.dialog, Some(Dialog::Delete("beeptest.1.csv".into())));
    }

    #[test]
    fn the_text_follows_the_phones_font_size_within_reason() {
        assert_eq!(scale_for(None), 1.0);
        assert_eq!(scale_for(Some(1.15)), 1.15);
        assert_eq!(scale_for(Some(2.0)), 1.3);
        assert_eq!(scale_for(Some(0.5)), 0.85);
        assert_eq!(scale_for(Some(f32::NAN)), 1.0);
        let ctx = egui::Context::default();
        set_text_sizes(&ctx, 1.3);
        let body = ctx.style().text_styles[&TextStyle::Body].size;
        let bold = medium(16.0).size;
        set_text_sizes(&ctx, 1.0);
        assert!((body - 22.1).abs() < 1e-4, "{body}");
        assert!((bold - 20.8).abs() < 1e-4, "{bold}");
        assert_eq!(ctx.style().text_styles[&TextStyle::Body].size, 17.0);
        assert_eq!(medium(16.0).size, 16.0);
    }

    #[test]
    fn the_text_scale_is_the_drawing_threads_own() {
        // Another test (or anything else) at another size, at the same
        // time, on its thread: this one's sizes stay as they are.
        set_text_sizes(&egui::Context::default(), 1.0);
        std::thread::spawn(|| {
            set_text_sizes(&egui::Context::default(), 1.3);
            assert!((medium(16.0).size - 20.8).abs() < 1e-4);
        })
        .join()
        .unwrap();
        assert_eq!(medium(16.0).size, 16.0);
        assert_eq!(text_scale(), 1.0);
    }

    #[test]
    fn durations_read_as_a_stopwatch() {
        assert_eq!(duration(0.0), "0:00");
        assert_eq!(duration(59.6), "1:00");
        assert_eq!(duration(543.0), "9:03");
        assert_eq!(duration(3723.0), "1:02:03");
        assert_eq!(duration(f64::NAN), "0:00");
        assert_eq!(duration(-5.0), "0:00");
    }
}
