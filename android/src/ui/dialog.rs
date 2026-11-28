//! The look every pop-up shares: a card in the middle with round corners,
//! no edge, a soft shadow and room inside, a title in the medium font, a
//! few weak words under it, and small text buttons along the bottom right,
//! as Android's own dialogs have them. The card sits in the middle of what
//! the keyboard (or the navigation bar) leaves showing.

use egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, Margin, RichText,
    Shadow, Stroke, Vec2,
};

/// The card's corners.
const ROUND: u8 = 24;
/// Room inside, either side.
const PAD: i8 = 24;
/// The widest a card gets.
const WIDEST: f32 = 360.0;
/// A title's size.
const TITLE: f32 = 20.0;

/// The pop-up's own colour, a step up from the page.
pub fn fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x2b, 0x2c, 0x31)
    } else {
        Color32::WHITE
    }
}

/// Keeps what covers the bottom of the screen this frame (the navigation
/// bar, or the keyboard when it's up): pop-ups sit in the middle of what's
/// above it.
pub fn set_bottom(ctx: &egui::Context, bottom: f32) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(BOTTOM), bottom));
}

const BOTTOM: &str = "dialog_bottom";

/// A pop-up named `id`, its frame and where it sits set.
pub fn modal(ctx: &egui::Context, id: &str) -> egui::Modal {
    let bottom: f32 = ctx
        .data(|d| d.get_temp(egui::Id::new(BOTTOM)))
        .unwrap_or_default();
    let frame = frame(ctx);
    let area = egui::Modal::default_area(egui::Id::new(id))
        .anchor(Align2::CENTER_CENTER, Vec2::new(0.0, -bottom / 2.0));
    egui::Modal::new(egui::Id::new(id))
        .area(area)
        .backdrop_color(Color32::from_black_alpha(90))
        .frame(frame)
}

/// A pop-up's card: its colour, corners and shadow, and room inside.
pub fn frame(ctx: &egui::Context) -> Frame {
    let dark = ctx.style().visuals.dark_mode;
    let shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 50 }),
    };
    Frame::new()
        .fill(fill(dark))
        .shadow(shadow)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(ROUND))
        .inner_margin(Margin {
            left: PAD,
            right: PAD,
            top: 22,
            bottom: 14,
        })
}

/// How wide a pop-up's insides are on this screen.
pub fn width(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().width() - 48.0).min(WIDEST) - 2.0 * PAD as f32
}

/// Sets the insides' width and spacing: the first thing in a pop-up.
pub fn begin(ui: &mut egui::Ui) {
    ui.set_width(width(ui.ctx()));
    ui.spacing_mut().item_spacing.y = 6.0;
}

/// The medium font, at `size`.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(super::BOLD.into()))
}

/// A pop-up's title.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .font(medium(TITLE))
                .color(ui.visuals().strong_text_color()),
        )
        .wrap()
        .selectable(false),
    );
}

/// The few words under a title.
pub fn text(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(RichText::new(text).size(15.0).weak())
            .wrap()
            .selectable(false),
    );
}

/// The row of buttons along the bottom, from the right: `add` puts the
/// rightmost first.
pub fn actions(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(10.0);
    // A row of its own height, so the buttons stay at the bottom whatever
    // height the pop-up had last frame.
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), 40.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            add(ui);
        },
    );
}

/// A text button in `color` (the page's text colour when `None`): true
/// when tapped.
pub fn button(ui: &mut egui::Ui, label: &str, color: Option<Color32>) -> bool {
    let color = color.unwrap_or(ui.visuals().text_color());
    let b = egui::Button::new(RichText::new(label).font(medium(16.0)).color(color))
        .frame(false)
        .min_size(Vec2::new(64.0, 40.0));
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(14.0, 8.0);
        let hover = color.gamma_multiply(0.12);
        let v = ui.visuals_mut();
        v.widgets.hovered.weak_bg_fill = hover;
        v.widgets.active.weak_bg_fill = color.gamma_multiply(0.2);
        v.widgets.hovered.corner_radius = CornerRadius::same(20);
        v.widgets.active.corner_radius = CornerRadius::same(20);
        ui.add(b).clicked()
    })
    .inner
}
