//! The two charts, drawn with egui's painter (no plotting crate): the
//! progress chart on Results (each course's results over time, as shuttles
//! completed in all, a line a course and speed progression, since their
//! shuttles aren't the same) and the heart rate over a test on a result's
//! page, with its levels marked as bands.
//!
//! What's worked out is apart from the drawing ([`Scale`], [`nice_range`],
//! [`progress_series`], [`hr_segments`]), so the tests check it without
//! pixels.

use crate::beeplog::{self, LevelRow};
use crate::store::TestResult;
use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, TextStyle, Vec2, pos2};

/// The 15 m course's line on the standard speeds (the 20 m one is the
/// accent): an orange that reads on both themes.
pub const COURSE_15: Color32 = Color32::from_rgb(0xe8, 0x7a, 0x2e);
/// The lines on the other speed progressions, by progression (1, then 2):
/// on 20 m a violet and a teal, on 15 m a magenta and an olive, all
/// reading on both themes and apart from the accent, the orange and the
/// heart rate's red.
const OTHER_20: [Color32; 2] = [
    Color32::from_rgb(0x8e, 0x5c, 0xe0),
    Color32::from_rgb(0x1f, 0xa5, 0x9a),
];
const OTHER_15: [Color32; 2] = [
    Color32::from_rgb(0xc8, 0x4c, 0xb4),
    Color32::from_rgb(0x9a, 0x8a, 0x1c),
];
/// The heart rate's line.
pub const HEART: Color32 = Color32::from_rgb(0xe0, 0x4a, 0x5a);

/// The colour of the line for course `m` and speed progression `p`.
pub fn series_color(m: u32, p: u32) -> Color32 {
    match (m == 15, p) {
        (false, 1 | 2) => OTHER_20[p as usize - 1],
        (true, 1 | 2) => OTHER_15[p as usize - 1],
        (true, _) => COURSE_15,
        (false, _) => super::ACCENT,
    }
}

/// What the key calls a line: "20 m", and on other speeds than the
/// standard ones "20 m, 8.0, 8.5, 9.0... km/h".
pub fn series_label(m: u32, p: u32) -> String {
    if p == 0 {
        format!("{m} m")
    } else {
        format!("{m} m, {}", crate::tables::speeds_text(p))
    }
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

/// A course's results on one speed progression over time, oldest first:
/// (when, Unix s; shuttles completed in all).
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub m: u32,
    pub p: u32,
    pub points: Vec<(i64, u32)>,
}

/// The progress chart's lines: one for each course and speed progression
/// that has results, 20 m first and the standard speeds first on each,
/// each oldest first.
pub fn progress_series(results: &[TestResult]) -> Vec<Series> {
    let progressions = 0..crate::tables::PROGRESSIONS.len() as u32;
    crate::tables::COURSES
        .iter()
        .flat_map(|&m| progressions.clone().map(move |p| (m, p)))
        .filter_map(|(m, p)| {
            let mut points: Vec<(i64, u32)> = results
                .iter()
                .filter(|r| r.entry.m == m && r.entry.p == p)
                .map(|r| (r.entry.t.div_euclid(1000), r.entry.total_shuttles()))
                .collect();
            points.sort();
            (!points.is_empty()).then_some(Series { m, p, points })
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
/// last reading), and the readings widened to twenties. Whatever it's
/// given, it stays finite and inside what a test can be
/// ([`beeplog::LONGEST_TEST_S`], [`beeplog::HIGHEST_BPM`]), so drawing it
/// is a few lines and labels, never millions.
pub fn hr_scale(hr: &[(f64, f64)], end_s: f64) -> Scale {
    let sane = |t: f64| {
        if t.is_finite() {
            t.clamp(0.0, beeplog::LONGEST_TEST_S)
        } else {
            0.0
        }
    };
    let last = hr.iter().map(|p| sane(p.0)).fold(0.0, f64::max);
    let bpm = || {
        hr.iter()
            .map(|p| p.1)
            .filter(|b| b.is_finite())
            .map(|b| b.clamp(0.0, beeplog::HIGHEST_BPM))
    };
    let lo = bpm().fold(f64::INFINITY, f64::min);
    let hi = bpm().fold(f64::NEG_INFINITY, f64::max);
    let (y0, y1) = if lo.is_finite() {
        nice_range(lo - 5.0, hi + 5.0, 20.0)
    } else {
        (60.0, 200.0)
    };
    Scale {
        x0: 0.0,
        x1: sane(end_s).max(last).max(1.0),
        y0,
        y1,
    }
}

/// Room for the axes' labels (at the phone's text size): left of the plot,
/// under it, and over it.
fn margins() -> (f32, f32, f32) {
    let s = super::text_scale();
    (30.0 * s, 18.0 * s, 16.0 * s)
}

fn grid_color(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.noninteractive.bg_stroke.color
}

/// The most lines [`grid_lines`] gives.
pub const MOST_LINES: usize = 10;

/// Where the y axis's lines go: at each `step` from `scale.y0` to
/// `scale.y1`, at most [`MOST_LINES`], and none for a scale or step that
/// isn't a finite number.
pub fn grid_lines(scale: &Scale, step: f64) -> Vec<f64> {
    let (y0, y1) = (scale.y0, scale.y1);
    if !(y0.is_finite() && y1.is_finite() && step.is_finite() && step > 0.0) || y1 < y0 {
        return Vec::new();
    }
    let n = ((y1 - y0) / step + 1e-9)
        .floor()
        .min(MOST_LINES as f64 - 1.0) as usize
        + 1;
    (0..n).map(|i| y0 + i as f64 * step).collect()
}

/// The y axis: a line and a number at each of [`grid_lines`].
fn y_grid(ui: &egui::Ui, plot: Rect, scale: &Scale, step: f64) {
    let painter = ui.painter();
    let font = TextStyle::Small.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    for y in grid_lines(scale, step) {
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
    }
}

/// The y axis's step: about four lines whatever the range (`base` for a
/// range that isn't a finite number).
pub fn y_step(scale: &Scale, base: f64) -> f64 {
    let range = scale.y1 - scale.y0;
    let mut step = base;
    if !(range.is_finite() && base.is_finite() && base > 0.0) {
        return base;
    }
    while range / step > 5.0 {
        step *= 2.0;
    }
    step
}

/// The minutes the heart rate chart labels along its bottom, for a test
/// `end_s` long drawn `px_per_s` points a second, with labels as wide as
/// `width` says ("14:00", Small): every 1, 2, 5, 10, 15, 30 or 60 minutes,
/// the first of those that leaves room between labels (the first, "0:00",
/// sits right of its mark, the rest centred on theirs). At most
/// [`MOST_LINES`] + 1 of them, whatever it's given.
pub fn minute_marks(end_s: f64, px_per_s: f64, width: impl Fn(i64) -> f32) -> Vec<i64> {
    if !(end_s.is_finite() && px_per_s.is_finite() && px_per_s > 0.0) {
        return vec![0];
    }
    let minutes = (end_s.clamp(0.0, beeplog::LONGEST_TEST_S) / 60.0).floor() as i64;
    let widest = (0..=minutes.min(9))
        .chain([minutes])
        .map(&width)
        .fold(0.0_f32, f32::max) as f64;
    let first = width(0) as f64;
    let room = (widest + 8.0).max(first + widest / 2.0 + 8.0);
    let every = [1, 2, 5, 10, 15, 30, 60]
        .into_iter()
        .find(|e| *e as f64 * 60.0 * px_per_s >= room)
        .unwrap_or(60)
        .max((minutes + MOST_LINES as i64 - 1) / MOST_LINES as i64);
    (0..=minutes).step_by(every as usize).collect()
}

/// What the progress chart says under its ends: the first result's date
/// at the left (from `first.1`, `first.2` wide) and the last's at the right
/// (to `last.1`). One date, centred between them, when they say the same;
/// and one line, "27 Nov – 28 Nov", when the two would run into each
/// other. (text, x, how it's aligned on x.)
pub fn date_labels(
    first: (String, f32, f32),
    last: (String, f32, f32),
) -> Vec<(String, f32, Align2)> {
    let (a, xa, wa) = first;
    let (b, xb, wb) = last;
    let mid = (xa + xb) / 2.0;
    if a == b {
        vec![(a, mid, Align2::CENTER_TOP)]
    } else if xa + wa + 8.0 > xb - wb {
        vec![(format!("{a} – {b}"), mid, Align2::CENTER_TOP)]
    } else {
        vec![(a, xa, Align2::LEFT_TOP), (b, xb, Align2::RIGHT_TOP)]
    }
}

/// The progress chart, `height` points high, the page's width.
pub fn progress(ui: &mut egui::Ui, series: &[Series], height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let plot = Rect::from_min_max(
        rect.min + Vec2::new(margins().0, 8.0),
        rect.max - Vec2::new(8.0, margins().1),
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
        let width = |text: &str| {
            painter
                .layout_no_wrap(text.to_owned(), font.clone(), weak)
                .size()
                .x
        };
        let (a, b) = (date(first as f64), date(last as f64));
        let (wa, wb) = (width(&a), width(&b));
        let labels = if first == last {
            vec![(a, at(first), Align2::CENTER_TOP)]
        } else {
            date_labels((a, at(first), wa), (b, at(last), wb))
        };
        for (text, x, align) in labels {
            painter.text(pos2(x, y), align, text, font.clone(), weak);
        }
    }
    for s in series {
        let color = series_color(s.m, s.p);
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
        rect.min + Vec2::new(margins().0, margins().2),
        rect.max - Vec2::new(8.0, margins().1),
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
        let w = 7.0 * super::text_scale() * text.len() as f32;
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
    // Minutes along the bottom, as many as fit.
    let width = |m: i64| {
        ui.painter()
            .layout_no_wrap(format!("{m}:00"), font.clone(), weak)
            .size()
            .x
    };
    let px_per_s = f64::from(plot.width()) / (scale.x1 - scale.x0);
    let mut last_right = f32::NEG_INFINITY;
    for m in minute_marks(scale.x1, px_per_s, width) {
        let x = scale.at(plot, m as f64 * 60.0, scale.y0).x;
        let w = width(m);
        let (align, left) = if m == 0 {
            (Align2::LEFT_TOP, x)
        } else {
            (Align2::CENTER_TOP, x - w / 2.0)
        };
        if left >= last_right + 4.0 && left + w <= rect.right() {
            ui.painter().text(
                pos2(x, plot.bottom() + 3.0),
                align,
                format!("{m}:00"),
                font.clone(),
                weak,
            );
            last_right = left + w;
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
    ui.scope(|ui| legend_lines(ui, items));
}

fn legend_lines(ui: &mut egui::Ui, items: &[(Color32, String)]) {
    const SWATCH: f32 = 18.0;
    const GAP: f32 = 6.0;
    // Its lines a line apart (a row is a touch's height at the least).
    ui.spacing_mut().interact_size.y = 0.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(14.0, 4.0);
        let row = ui.max_rect().width();
        for (color, text) in items {
            // Each item is one widget, its colour and its words, so a
            // line that runs out moves the whole of it to the next one
            // (cut short only if it's wider than a line by itself).
            let galley = egui::WidgetText::from(egui::RichText::new(text).small()).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                (row - SWATCH - GAP).max(0.0),
                TextStyle::Small,
            );
            let size = Vec2::new(SWATCH + GAP + galley.size().x, galley.size().y.max(14.0));
            let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text.as_str())
            });
            let y = rect.center().y;
            ui.painter().line_segment(
                [pos2(rect.left(), y), pos2(rect.left() + SWATCH, y)],
                Stroke::new(3.0_f32, *color),
            );
            let at = pos2(rect.left() + SWATCH + GAP, y - galley.size().y / 2.0);
            ui.painter().galley(at, galley, ui.visuals().text_color());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Entry, LogState};

    fn result(t_s: i64, m: u32, l: u32, s: u32) -> TestResult {
        on(t_s, m, 0, l, s)
    }

    /// A result on speed progression `p`.
    fn on(t_s: i64, m: u32, p: u32, l: u32, s: u32) -> TestResult {
        TestResult {
            entry: Entry {
                f: format!("beeptest.{t_s}.csv"),
                t: t_s * 1000,
                m,
                p,
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
                (1_000_000, crate::tables::total_shuttles(20, 8, 1, 0)),
                (3_000_000, crate::tables::total_shuttles(20, 9, 4, 0)),
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
    fn progress_is_a_line_a_course_and_speed_progression() {
        let results = vec![
            on(6_000_000, 20, 2, 9, 4),
            on(5_000_000, 15, 1, 7, 1),
            on(4_000_000, 20, 0, 9, 4),
            on(3_000_000, 20, 2, 8, 1),
            on(2_000_000, 15, 0, 6, 2),
        ];
        let series = progress_series(&results);
        let lines: Vec<(u32, u32, usize)> =
            series.iter().map(|s| (s.m, s.p, s.points.len())).collect();
        assert_eq!(lines, vec![(20, 0, 1), (20, 2, 2), (15, 0, 1), (15, 1, 1)]);
        // Each counted on its own tables: 9.4 is 76 shuttles on the
        // standard speeds and 80 on 8.5, 9.5, 10.0... (node, lib.js).
        assert_eq!(series[0].points, vec![(4_000_000, 76)]);
        assert_eq!(series[1].points[1], (6_000_000, 80));
        assert_eq!(
            series[3].points,
            vec![(5_000_000, 9 + 10 + 11 + 11 + 12 + 12 + 1)]
        );
        // Each line its own colour, and its own words in the key.
        let colors: Vec<Color32> = series.iter().map(|s| series_color(s.m, s.p)).collect();
        for (i, c) in colors.iter().enumerate() {
            assert!(!colors[..i].contains(c), "{colors:?}");
        }
        let all: Vec<Color32> = [20, 15]
            .into_iter()
            .flat_map(|m| (0..3).map(move |p| series_color(m, p)))
            .collect();
        for (i, c) in all.iter().enumerate() {
            assert!(!all[..i].contains(c) && *c != HEART, "{all:?}");
        }
        assert_eq!(series_label(20, 0), "20 m");
        assert_eq!(series_label(15, 2), "15 m, 8.5, 9.5, 10.0... km/h");
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

    #[test]
    fn a_log_with_numbers_no_test_has_still_draws_a_few_lines() {
        // A reading at 1e12 s and one at Infinity bpm (a forged broadcast,
        // or a corrupt log): the axes stay what a test can be.
        let s = hr_scale(&[(2.0, 90.0), (1e12, f64::INFINITY)], f64::INFINITY);
        assert!(s.x1.is_finite() && s.x1 <= beeplog::LONGEST_TEST_S, "{s:?}");
        assert!(
            s.y1.is_finite() && s.y1 <= beeplog::HIGHEST_BPM + 20.0,
            "{s:?}"
        );
        assert_eq!(hr_scale(&[(f64::NAN, f64::NAN)], f64::NAN).x1, 1.0);
        // However wide the scale, the grid is a few lines, and none for
        // one that isn't finite.
        let wide = Scale {
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1e300,
        };
        assert!(grid_lines(&wide, y_step(&wide, 20.0)).len() <= MOST_LINES);
        assert!(grid_lines(&wide, 1.0).len() <= MOST_LINES);
        let inf = Scale {
            y1: f64::INFINITY,
            ..wide
        };
        assert_eq!(y_step(&inf, 20.0), 20.0);
        assert!(grid_lines(&inf, 20.0).is_empty());
        assert!(grid_lines(&wide, f64::INFINITY).is_empty());
        assert!(grid_lines(&wide, 0.0).is_empty());
        assert_eq!(
            grid_lines(
                &Scale {
                    y0: 80.0,
                    y1: 200.0,
                    ..wide
                },
                40.0
            ),
            vec![80.0, 120.0, 160.0, 200.0]
        );
        // The minutes along the bottom: a few, for any length.
        let w = |m: i64| if m >= 10 { 35.0 } else { 27.0 };
        for end in [1e12, f64::INFINITY, beeplog::LONGEST_TEST_S] {
            assert!(minute_marks(end, 218.0 / 600.0, w).len() <= MOST_LINES + 1);
        }
        assert_eq!(minute_marks(f64::NAN, 1.0, w), vec![0]);
        assert_eq!(minute_marks(100.0, 0.0, w), vec![0]);
    }

    #[test]
    fn minute_labels_leave_room_between_them() {
        // "10:00" is about 35 pt in Small, "0:00" about 27.
        let w = |m: i64| if m >= 10 { 35.0 } else { 27.0 };
        // A 14-minute test at 320 dp (a plot 218 pt wide, 870 s across):
        // every 2 minutes would be 30 pt apart, so every 5.
        let marks = minute_marks(870.0, 218.0 / 870.0, w);
        assert_eq!(marks, vec![0, 5, 10]);
        // Each label clear of the next: the first sits right of its mark,
        // the rest centred.
        let px = 218.0 / 870.0 * 60.0;
        for pair in marks.windows(2) {
            let right = if pair[0] == 0 {
                w(0)
            } else {
                pair[0] as f32 * px + w(pair[0]) / 2.0
            };
            let left = pair[1] as f32 * px - w(pair[1]) / 2.0;
            assert!(left - right >= 8.0, "{pair:?}");
        }
        // A short test on a wide screen: every minute.
        assert_eq!(minute_marks(300.0, 1.0, w), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn the_progress_charts_dates_never_run_together() {
        // All on one day: the same date at both ends, once, centred.
        assert_eq!(
            date_labels(
                ("27 Nov".into(), 150.0, 40.0),
                ("27 Nov".into(), 152.0, 40.0)
            ),
            vec![("27 Nov".to_string(), 151.0, Align2::CENTER_TOP)]
        );
        // Two days less than a day apart, near the middle: one line.
        assert_eq!(
            date_labels(
                ("27 Nov".into(), 140.0, 40.0),
                ("28 Nov".into(), 160.0, 40.0)
            ),
            vec![("27 Nov – 28 Nov".to_string(), 150.0, Align2::CENTER_TOP)]
        );
        // Far apart: each under its end.
        assert_eq!(
            date_labels(("1 Oct".into(), 30.0, 30.0), ("27 Nov".into(), 300.0, 40.0)),
            vec![
                ("1 Oct".to_string(), 30.0, Align2::LEFT_TOP),
                ("27 Nov".to_string(), 300.0, Align2::RIGHT_TOP)
            ]
        );
    }
}
