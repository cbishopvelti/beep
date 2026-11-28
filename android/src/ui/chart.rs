//! The two charts, drawn with egui's painter (no plotting crate): the
//! progress chart on Results (each course's results over time, as shuttles
//! completed in all, a line a course) and the heart rate over a test on a
//! result's page, with its levels marked as bands.
//!
//! What's worked out is apart from the drawing ([`Scale`], [`nice_range`],
//! [`progress_series`], [`hr_segments`]), so the tests check it without
//! pixels.

use crate::beeplog::LevelRow;
use crate::store::TestResult;
use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, TextStyle, Vec2, pos2};

/// The 15 m course's line (the 20 m one is the accent): an orange that
/// reads on both themes.
pub const COURSE_15: Color32 = Color32::from_rgb(0xe8, 0x7a, 0x2e);
/// The heart rate's line.
pub const HEART: Color32 = Color32::from_rgb(0xe0, 0x4a, 0x5a);

/// A course's line's colour.
pub fn course_color(m: u32) -> Color32 {
    if m == 15 { COURSE_15 } else { super::ACCENT }
}

/// What a chart shows, in its own units, and where that goes on the
/// screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

impl Scale {
    /// Where `(x, y)` goes in `rect`: `x0` at the left, `x1` at the right,
    /// `y0` at the bottom, `y1` at the top. A range with no width puts
    /// everything in its middle.
    pub fn at(&self, rect: Rect, x: f64, y: f64) -> Pos2 {
        let f = |v: f64, a: f64, b: f64| {
            if b > a {
                ((v - a) / (b - a)).clamp(0.0, 1.0) as f32
            } else {
                0.5
            }
        };
        pos2(
            rect.left() + rect.width() * f(x, self.x0, self.x1),
            rect.bottom() - rect.height() * f(y, self.y0, self.y1),
        )
    }
}

/// `lo..hi` widened to whole `step`s, with at least a step between them:
/// a chart's y range, so its lines fall on round numbers.
pub fn nice_range(lo: f64, hi: f64, step: f64) -> (f64, f64) {
    let a = (lo / step).floor() * step;
    let mut b = (hi / step).ceil() * step;
    if b <= a {
        b = a + step;
    }
    (a, b)
}

/// A course's results over time, oldest first: (when, Unix s; shuttles
/// completed in all).
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub m: u32,
    pub points: Vec<(i64, u32)>,
}

/// The progress chart's lines: one a course that has results, 20 m first,
/// each oldest first.
pub fn progress_series(results: &[TestResult]) -> Vec<Series> {
    crate::tables::COURSES
        .iter()
        .filter_map(|&m| {
            let mut points: Vec<(i64, u32)> = results
                .iter()
                .filter(|r| r.entry.m == m)
                .map(|r| (r.entry.t.div_euclid(1000), r.entry.total_shuttles()))
                .collect();
            points.sort();
            (!points.is_empty()).then_some(Series { m, points })
        })
        .collect()
}

/// The progress chart's scale: from the first result to the last (a day
/// either side when they're on one day), and the shuttles widened to tens
/// with a little room above and below.
pub fn progress_scale(series: &[Series]) -> Scale {
    let all = || series.iter().flat_map(|s| s.points.iter());
    let t0 = all().map(|p| p.0).min().unwrap_or(0) as f64;
    let t1 = all().map(|p| p.0).max().unwrap_or(0) as f64;
    let (t0, t1) = if t1 - t0 < 86_400.0 {
        (t0 - 86_400.0, t1 + 86_400.0)
    } else {
        (t0, t1)
    };
    let lo = all().map(|p| p.1).min().unwrap_or(0) as f64;
    let hi = all().map(|p| p.1).max().unwrap_or(0) as f64;
    let (y0, y1) = nice_range((lo - 3.0).max(0.0), hi + 3.0, 10.0);
    Scale {
        x0: t0,
        x1: t1,
        y0,
        y1,
    }
}

/// The heart rate as runs of readings to join with a line: a gap of more
/// than `gap_s` seconds (readings left out as low-confidence, or none
/// read) starts a new one.
pub fn hr_segments(hr: &[(f64, f64)], gap_s: f64) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    for &p in hr {
        match out.last_mut() {
            Some(run) if run.last().is_some_and(|q| p.0 - q.0 <= gap_s) => run.push(p),
            _ => out.push(vec![p]),
        }
    }
    out
}

/// The heart rate chart's scale: the whole test across (to `end_s`, or the
/// last reading), and the readings widened to twenties.
pub fn hr_scale(hr: &[(f64, f64)], end_s: f64) -> Scale {
    let last = hr.last().map_or(0.0, |p| p.0);
    let lo = hr.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let hi = hr.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let (y0, y1) = if lo.is_finite() {
        nice_range(lo - 5.0, hi + 5.0, 20.0)
    } else {
        (60.0, 200.0)
    };
    Scale {
        x0: 0.0,
        x1: end_s.max(last).max(1.0),
        y0,
        y1,
    }
}

/// Room for the axes' labels: left of the plot, under it, and over it (the
/// levels' numbers).
const LEFT: f32 = 30.0;
const BELOW: f32 = 18.0;
const ABOVE: f32 = 16.0;

fn grid_color(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.noninteractive.bg_stroke.color
}

/// The y axis: a line and a number at each `step` from `scale.y0` to
/// `scale.y1`.
fn y_grid(ui: &egui::Ui, plot: Rect, scale: &Scale, step: f64) {
    let painter = ui.painter();
    let font = TextStyle::Small.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    let mut y = scale.y0;
    while y <= scale.y1 + 1e-9 {
        let p = scale.at(plot, scale.x0, y);
        painter.line_segment(
            [pos2(plot.left(), p.y), pos2(plot.right(), p.y)],
            Stroke::new(1.0_f32, grid_color(ui)),
        );
        painter.text(
            pos2(plot.left() - 6.0, p.y),
            Align2::RIGHT_CENTER,
            format!("{y:.0}"),
            font.clone(),
            weak,
        );
        y += step;
    }
}

/// The y axis's step: about four lines whatever the range.
fn y_step(scale: &Scale, base: f64) -> f64 {
    let mut step = base;
    while (scale.y1 - scale.y0) / step > 5.0 {
        step *= 2.0;
    }
    step
}

/// The progress chart, `height` points high, the page's width.
pub fn progress(ui: &mut egui::Ui, series: &[Series], height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let plot = Rect::from_min_max(
        rect.min + Vec2::new(LEFT, 8.0),
        rect.max - Vec2::new(8.0, BELOW),
    );
    let scale = progress_scale(series);
    y_grid(ui, plot, &scale, y_step(&scale, 10.0));
    let painter = ui.painter();
    let font = TextStyle::Small.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    // The first and last results' dates, under their ends.
    let date = |t: f64| {
        crate::health::local(t as i64).map_or_else(String::new, |l| {
            format!(
                "{} {}",
                l.day,
                crate::health::MONTHS[(l.month as usize).clamp(1, 12) - 1]
            )
        })
    };
    let all: Vec<i64> = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.0))
        .collect();
    if let (Some(&first), Some(&last)) = (all.iter().min(), all.iter().max()) {
        let y = plot.bottom() + 3.0;
        let at = |t: i64| scale.at(plot, t as f64, scale.y0).x;
        if first == last {
            painter.text(
                pos2(at(first), y),
                Align2::CENTER_TOP,
                date(first as f64),
                font.clone(),
                weak,
            );
        } else {
            painter.text(
                pos2(at(first), y),
                Align2::LEFT_TOP,
                date(first as f64),
                font.clone(),
                weak,
            );
            painter.text(
                pos2(at(last), y),
                Align2::RIGHT_TOP,
                date(last as f64),
                font.clone(),
                weak,
            );
        }
    }
    for s in series {
        let color = course_color(s.m);
        let points: Vec<Pos2> = s
            .points
            .iter()
            .map(|&(t, n)| scale.at(plot, t as f64, f64::from(n)))
            .collect();
        if points.len() > 1 {
            painter.add(Shape::line(points.clone(), Stroke::new(2.0_f32, color)));
        }
        for p in points {
            painter.circle_filled(p, 3.5, color);
        }
    }
}

/// The heart rate chart, `height` points high, the page's width: the
/// levels as bands with their numbers over them, and the readings as a
/// line, broken where there are none for a while.
pub fn heart_rate(
    ui: &mut egui::Ui,
    hr: &[(f64, f64)],
    rows: &[LevelRow],
    end_s: f64,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let plot = Rect::from_min_max(
        rect.min + Vec2::new(LEFT, ABOVE),
        rect.max - Vec2::new(8.0, BELOW),
    );
    let scale = hr_scale(hr, end_s);
    let font = TextStyle::Small.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    // Every other level shaded, its number over it where there's room.
    let band = ui.visuals().text_color().gamma_multiply(0.06);
    let mut last_label = f32::NEG_INFINITY;
    for (i, row) in rows.iter().enumerate() {
        let a = scale.at(plot, row.start_s, scale.y1);
        let b = scale.at(plot, row.end_s, scale.y0);
        if i % 2 == 1 {
            ui.painter()
                .rect_filled(Rect::from_min_max(a, b), 0.0, band);
        }
        let mid = (a.x + b.x) / 2.0;
        let text = row.level.to_string();
        let w = 7.0 * text.len() as f32;
        if mid - w / 2.0 >= last_label + 4.0 && mid + w / 2.0 <= rect.right() {
            ui.painter().text(
                pos2(mid, plot.top() - 2.0),
                Align2::CENTER_BOTTOM,
                text,
                font.clone(),
                weak,
            );
            last_label = mid + w / 2.0;
        }
    }
    y_grid(ui, plot, &scale, y_step(&scale, 20.0));
    // Minutes along the bottom.
    let minutes = (scale.x1 / 60.0).floor() as i64;
    let every = match minutes {
        0..=6 => 1,
        7..=14 => 2,
        15..=30 => 5,
        _ => 10,
    };
    for m in (0..=minutes).step_by(every as usize) {
        let x = scale.at(plot, m as f64 * 60.0, scale.y0).x;
        let align = if m == 0 {
            Align2::LEFT_TOP
        } else {
            Align2::CENTER_TOP
        };
        if x + 12.0 <= rect.right() {
            ui.painter().text(
                pos2(x, plot.bottom() + 3.0),
                align,
                format!("{m}:00"),
                font.clone(),
                weak,
            );
        }
    }
    for run in hr_segments(hr, 5.0) {
        let points: Vec<Pos2> = run.iter().map(|&(t, bpm)| scale.at(plot, t, bpm)).collect();
        if points.len() == 1 {
            ui.painter().circle_filled(points[0], 1.5, HEART);
        } else {
            ui.painter()
                .add(Shape::line(points, Stroke::new(2.0_f32, HEART)));
        }
    }
}

/// A key under a chart: a short line in each colour and what it is.
pub fn legend(ui: &mut egui::Ui, items: &[(Color32, String)]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (color, text) in items {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 14.0), Sense::hover());
            ui.painter().line_segment(
                [rect.left_center(), rect.right_center()],
                Stroke::new(3.0_f32, *color),
            );
            ui.label(egui::RichText::new(text).small());
            ui.add_space(8.0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Entry, LogState};

    fn result(t_s: i64, m: u32, l: u32, s: u32) -> TestResult {
        TestResult {
            entry: Entry {
                f: format!("beeptest.{t_s}.csv"),
                t: t_s * 1000,
                m,
                l,
                s,
                d: 0,
                dur: 0,
                max: 0,
                avg: 0,
            },
            log: LogState::Complete,
        }
    }

    #[test]
    fn the_scale_puts_the_range_on_the_rect_bottom_up() {
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(110.0, 220.0));
        let s = Scale {
            x0: 0.0,
            x1: 10.0,
            y0: 100.0,
            y1: 200.0,
        };
        assert_eq!(s.at(rect, 0.0, 100.0), pos2(10.0, 220.0));
        assert_eq!(s.at(rect, 10.0, 200.0), pos2(110.0, 20.0));
        assert_eq!(s.at(rect, 5.0, 150.0), pos2(60.0, 120.0));
        // Outside the range stays on the chart.
        assert_eq!(s.at(rect, -5.0, 900.0), pos2(10.0, 20.0));
        // A range with no width: the middle.
        let flat = Scale { x1: 0.0, ..s };
        assert_eq!(flat.at(rect, 3.0, 100.0).x, 60.0);
    }

    #[test]
    fn ranges_widen_to_round_numbers() {
        assert_eq!(nice_range(37.0, 81.0, 10.0), (30.0, 90.0));
        assert_eq!(nice_range(40.0, 40.0, 10.0), (40.0, 50.0));
        assert_eq!(nice_range(92.0, 187.0, 20.0), (80.0, 200.0));
    }

    #[test]
    fn progress_is_a_line_a_course_oldest_first() {
        let results = vec![
            result(3_000_000, 20, 9, 4),
            result(2_000_000, 15, 6, 2),
            result(1_000_000, 20, 8, 1),
        ];
        let series = progress_series(&results);
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].m, 20);
        assert_eq!(
            series[0].points,
            vec![
                (1_000_000, crate::tables::total_shuttles(20, 8, 1)),
                (3_000_000, crate::tables::total_shuttles(20, 9, 4)),
            ]
        );
        assert_eq!(series[1].m, 15);
        assert_eq!(series[1].points.len(), 1);
        assert!(progress_series(&results[1..2]).iter().all(|s| s.m == 15));

        let scale = progress_scale(&series);
        assert_eq!((scale.x0, scale.x1), (1_000_000.0, 3_000_000.0));
        // Every point is on the chart, on round lines.
        for s in &series {
            for &(_, n) in &s.points {
                assert!(scale.y0 < f64::from(n) && f64::from(n) < scale.y1);
            }
        }
        assert_eq!(scale.y0 % 10.0, 0.0);
        assert_eq!(scale.y1 % 10.0, 0.0);
    }

    #[test]
    fn one_day_of_results_still_spreads_across() {
        let series = progress_series(&[result(5_000_000, 20, 5, 1)]);
        let scale = progress_scale(&series);
        assert_eq!(scale.x1 - scale.x0, 2.0 * 86_400.0);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0));
        assert_eq!(scale.at(rect, 5_000_000.0, scale.y0).x, 50.0);
    }

    #[test]
    fn heart_rate_breaks_where_readings_stop() {
        let hr = [
            (0.0, 90.0),
            (1.0, 92.0),
            (2.0, 95.0),
            (12.0, 120.0),
            (13.0, 121.0),
        ];
        let runs = hr_segments(&hr, 5.0);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 3);
        assert_eq!(runs[1], vec![(12.0, 120.0), (13.0, 121.0)]);
        assert!(hr_segments(&[], 5.0).is_empty());

        let s = hr_scale(&hr, 200.0);
        assert_eq!((s.x0, s.x1), (0.0, 200.0));
        assert_eq!((s.y0, s.y1), (80.0, 140.0));
        // No end: as far as the readings go. No readings: a resting range.
        assert_eq!(hr_scale(&hr, 0.0).x1, 13.0);
        let none = hr_scale(&[], 0.0);
        assert_eq!((none.y0, none.y1, none.x1), (60.0, 200.0, 1.0));
    }
}
