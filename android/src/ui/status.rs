//! Rows as blink's Status page has them (`src/ui/status.rs` there): a dot,
//! a name and a few words, opening for the rest, with a hairline between
//! rows. The Watch page is made of them.

use crate::health;
use egui::{Color32, RichText};

/// Green is fine, amber on its way, red wrong, grey unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Green,
    Amber,
    Red,
    Off,
}

impl Dot {
    pub fn color(self) -> Color32 {
        match self {
            Dot::Green => super::GREEN,
            Dot::Amber => super::AMBER,
            Dot::Red => super::RED,
            Dot::Off => Color32::from_gray(0x90),
        }
    }
}

/// How far a row's detail sits in: under its name, past the dot.
const INDENT: f32 = 22.0;

/// The hairline between two rows.
pub fn line(ui: &mut egui::Ui) {
    ui.add(egui::Separator::default().spacing(6.0));
}

/// One row: its dot, its name and a few words, with `detail` under it when
/// it's open. Tapping the row opens and shuts it; until it's tapped, a row
/// is open when something is `wrong`, so that shows without a tap.
pub fn row(
    ui: &mut egui::Ui,
    name: &str,
    dot: Option<Dot>,
    summary: &str,
    wrong: bool,
    detail: impl FnOnce(&mut egui::Ui),
) {
    let fine = !wrong;
    // A tap is remembered for the state it was made in: a row shut while
    // green opens again when it turns red.
    let id = ui.id().with(("status-row", name, fine));
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(!fine);
    let head = ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(INDENT - 8.0, 10.0), egui::Sense::hover());
        if let Some(dot) = dot {
            ui.painter().circle_filled(
                egui::pos2(rect.min.x + 5.0, rect.center().y),
                5.0,
                dot.color(),
            );
        }
        ui.add(egui::Label::new(RichText::new(name).strong()).selectable(false));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            chevron(ui, open);
            let text = RichText::new(summary).small();
            let text = match dot {
                Some(Dot::Amber) => text.color(amber_text(ui)),
                Some(Dot::Red) => text.color(Dot::Red.color()),
                _ => text.weak(),
            };
            ui.add(egui::Label::new(text).selectable(false).truncate());
        });
    });
    let head = ui.interact(head.response.rect, id.with("head"), egui::Sense::click());
    head.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    if head.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open {
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                detail(ui);
                ui.add_space(2.0);
            });
        });
    }
}

/// Amber as text: as it is on the dark theme, darker on the light one,
/// where the dot's amber is too pale to read.
pub fn amber_text(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        super::AMBER
    } else {
        Color32::from_rgb(0xa8, 0x6a, 0x00)
    }
}

/// The mark at a row's end: pointing right when it's shut, down when open.
fn chevron(ui: &mut egui::Ui, open: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    let c = rect.center();
    let stroke = egui::Stroke::new(1.5_f32, ui.visuals().weak_text_color());
    let points = if open {
        [(-4.0, -2.0), (0.0, 2.0), (4.0, -2.0)]
    } else {
        [(-2.0, -4.0), (2.0, 0.0), (-2.0, 4.0)]
    }
    .map(|(x, y)| c + egui::vec2(x, y));
    ui.painter().line_segment([points[0], points[1]], stroke);
    ui.painter().line_segment([points[1], points[2]], stroke);
}

/// A line of a row's detail: what it is on the left, weak, and its value
/// on the right, or under it when the two don't fit side by side.
pub fn fact(ui: &mut egui::Ui, label: &str, value: &str) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER)
            .size()
            .x
    };
    if width(label) + width(value) + 16.0 > ui.available_width() {
        ui.label(RichText::new(label).weak());
        ui.label(value);
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = 22.0;
        ui.label(RichText::new(label).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(value);
        });
    });
}

/// "11:32", or "23 Nov 11:32" from another day.
pub fn clock(at: i64, now: i64) -> String {
    short(&health::local_time(at), &health::local_time(now))
}

/// [`clock`] from [`health::local_time`]'s "23 Nov 2026, 11:32:05".
fn short(full: &str, today: &str) -> String {
    let Some((day, time)) = full.split_once(", ") else {
        return full.to_string();
    };
    let time = time.rsplit_once(':').map_or(time, |t| t.0);
    if today.split_once(", ").map(|t| t.0) == Some(day) {
        time.to_string()
    } else {
        let day = day.rsplit_once(' ').map_or(day, |(d, _)| d);
        format!("{day} {time}")
    }
}

/// "23:41, 1 min ago".
pub fn when(at: i64, now: i64) -> String {
    format!("{}, {}", clock(at, now), health::ago(at, now))
}

#[cfg(test)]
mod tests {
    use super::short;

    #[test]
    fn times_are_short_today_and_dated_before() {
        let today = "24 Nov 2026, 23:42:00";
        assert_eq!(short("24 Nov 2026, 08:14:30", today), "08:14");
        assert_eq!(short("23 Nov 2026, 08:14:30", today), "23 Nov 08:14");
        assert_eq!(short("1764000000", today), "1764000000");
    }
}
