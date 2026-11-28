//! The small outlined chips, as blink's (`src/ui/chip.rs`): "Best" on a
//! card, "Change" and "Allow" beside the direct link's watch, "20 m" and
//! "15 m" for the watch's course. Painted to one fixed height, text
//! centred, so a row of them lines up whatever's inside.

use egui::{Color32, Pos2, Response, Sense, Stroke, StrokeKind, TextStyle, Vec2};

/// Every chip is this tall.
pub const HEIGHT: f32 = 22.0;
/// Space either side of the text.
const PAD: f32 = 8.0;

/// What a chip is for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Shows something; not tappable.
    Label,
    /// Tap it.
    Button,
}

pub fn show(ui: &mut egui::Ui, text: &str, kind: Kind) -> Response {
    paint(ui, text, kind, None, HEIGHT)
}

/// A chip in `color`: its edge, its text and a faint fill, so it stands
/// out from the plain ones ("Best").
pub fn tinted(ui: &mut egui::Ui, text: &str, kind: Kind, color: Color32) -> Response {
    paint(ui, text, kind, Some(color), HEIGHT)
}

/// A chip a touch target high (`height`), for a choice made with it (the
/// watch's course), filled with the accent when `on`.
pub fn choice(ui: &mut egui::Ui, text: &str, on: bool, height: f32) -> Response {
    let tint = on.then_some(super::ACCENT);
    paint(ui, text, Kind::Button, tint, height)
}

fn paint(
    ui: &mut egui::Ui,
    text: &str,
    kind: Kind,
    tint: Option<Color32>,
    height: f32,
) -> Response {
    let font = if height > HEIGHT {
        TextStyle::Body.resolve(ui.style())
    } else {
        TextStyle::Small.resolve(ui.style())
    };
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        font,
        tint.unwrap_or(ui.visuals().text_color()),
    );
    let pad = if height > HEIGHT { 2.0 * PAD } else { PAD };
    let size = Vec2::new(pad + galley.size().x + pad, height);
    let sense = if kind == Kind::Label {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, body) = ui.allocate_exact_size(size, sense);
    body.widget_info(|| {
        egui::WidgetInfo::labeled(
            if kind == Kind::Label {
                egui::WidgetType::Label
            } else {
                egui::WidgetType::Button
            },
            ui.is_enabled(),
            text,
        )
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let hot = kind != Kind::Label && body.hovered();
        let stroke = if hot {
            visuals.widgets.hovered.bg_stroke
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };
        let painter = ui.painter();
        if let Some(c) = tint {
            painter.rect_filled(rect, height / 2.0, c.gamma_multiply(0.15));
        }
        painter.rect_stroke(
            rect,
            height / 2.0,
            Stroke::new(1.0_f32, tint.unwrap_or(stroke.color)),
            StrokeKind::Inside,
        );
        let at = Pos2::new(rect.min.x + pad, rect.center().y - galley.size().y / 2.0);
        painter.galley(at, galley, tint.unwrap_or(visuals.text_color()));
    }
    body
}

/// The space above and under a card's contents.
pub const GAP: f32 = 10.0;

/// A row of chips that wraps, spaced evenly.
pub fn row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        // Rows take their height from this before the closure runs, so
        // it's set out here, or each row is a touch target high.
        ui.spacing_mut().interact_size.y = HEIGHT;
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        ui.horizontal_wrapped(add).inner
    })
    .inner
}
