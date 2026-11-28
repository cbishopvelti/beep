//! The app's on/off switch: one row with the label on the left and the
//! switch on the right, like Android's own settings. Every setting that's
//! on or off uses [`show`], so they all look the same.

use super::ACCENT;
use egui::{
    Color32, CornerRadius, Response, Sense, Stroke, StrokeKind, TextStyle, TextWrapMode, Ui, Vec2,
    WidgetText, pos2, vec2,
};

/// The switch's size: Material 3's, a little smaller to stay compact.
const SWITCH: Vec2 = vec2(44.0, 26.0);

/// The gap between the label and the switch.
const GAP: f32 = 12.0;
/// The row's least height, so it's still easy to tap.
const ROW: f32 = 40.0;

/// A setting that's on or off. Tapping anywhere on the row flips it; the
/// response is `changed()` when it did.
pub fn show(ui: &mut Ui, on: &mut bool, text: impl Into<WidgetText>) -> Response {
    let switch = SWITCH;
    let width = ui.available_width();
    let galley = text.into().into_galley(
        ui,
        Some(TextWrapMode::Wrap),
        (width - switch.x - GAP).max(0.0),
        TextStyle::Body,
    );
    let height = (galley.size().y + 8.0).max(ROW);
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *on,
            galley.text(),
        )
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let text_color = ui.visuals().text_color();
    let text_pos = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, text_color);

    let track =
        egui::Rect::from_center_size(pos2(rect.right() - switch.x / 2.0, rect.center().y), switch);
    let t = ui
        .ctx()
        .animate_bool_responsive(response.id.with("toggle"), *on);
    paint(ui, track, t);
    response
}

/// Draws the switch, Material 3 style, `t` going from 0 (off) to 1 (on):
/// off is an outlined track with a small grey knob, on a filled accent
/// track with a bigger white knob.
fn paint(ui: &Ui, track: egui::Rect, t: f32) {
    let painter = ui.painter();
    let radius = CornerRadius::same((track.height() / 2.0) as u8);
    // A grey that reads on the card in either theme.
    let off = if ui.visuals().dark_mode {
        Color32::from_gray(0x8a)
    } else {
        Color32::from_gray(0x79)
    };
    let lerp = |a: Color32, b: Color32| a.lerp_to_gamma(b, t);
    painter.rect_filled(track, radius, lerp(ui.visuals().faint_bg_color, ACCENT));
    painter.rect_stroke(
        track,
        radius,
        Stroke::new(2.0_f32, lerp(off, ACCENT)),
        StrokeKind::Inside,
    );
    let inset = track.height() / 2.0;
    let x = egui::lerp((track.left() + inset)..=(track.right() - inset), t);
    let r = egui::lerp(7.0..=10.0, t);
    painter.circle_filled(pos2(x, track.center().y), r, lerp(off, Color32::WHITE));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2, RawInput, Rect};

    /// Runs one frame of a 300-wide page with a toggle row on it, with
    /// `events` as the input: whether it changed, and the row's rect.
    fn frame(
        ctx: &egui::Context,
        on: &mut bool,
        enabled: bool,
        events: Vec<Event>,
    ) -> (bool, Rect) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 200.0))),
            events,
            ..Default::default()
        };
        let mut out = (false, Rect::NOTHING);
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let r = show(ui, on, "Notify me when I arrive somewhere");
                    out = (r.changed(), r.rect);
                });
            });
        });
        out
    }

    fn tap(ctx: &egui::Context, on: &mut bool, enabled: bool, at: Pos2) -> bool {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(ctx, on, enabled, vec![Event::PointerMoved(at)]);
        frame(ctx, on, enabled, vec![button(true)]);
        frame(ctx, on, enabled, vec![button(false)]).0
    }

    #[test]
    fn tapping_the_label_or_the_switch_flips_it() {
        let ctx = egui::Context::default();
        let mut on = false;
        let (_, row) = frame(&ctx, &mut on, true, vec![]);
        assert!(row.height() >= ROW);
        assert!(tap(
            &ctx,
            &mut on,
            true,
            row.left_center() + vec2(10.0, 0.0)
        ));
        assert!(on);
        assert!(tap(
            &ctx,
            &mut on,
            true,
            row.right_center() - vec2(10.0, 0.0)
        ));
        assert!(!on);
    }

    #[test]
    fn a_disabled_one_stays_put() {
        let ctx = egui::Context::default();
        let mut on = true;
        let (_, row) = frame(&ctx, &mut on, false, vec![]);
        assert!(!tap(&ctx, &mut on, false, row.center()));
        assert!(on);
    }
}
