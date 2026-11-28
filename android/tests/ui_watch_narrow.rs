//! The Watch page's settings rows on small phones with large text
//! (`egui_kittest`, no pixels, the demo data, whose watch has the Speeds
//! setting): each row's chips keep clear of its label, stacking under it
//! when they don't fit beside it, and stay inside the card, a change on
//! its way (" …") included. A file of its own: the text size comes from
//! `BEEP_FONT_SCALE`, which is the process's.

use beep_android::ui::{App, Page};
use beep_android::{health, platform, ui};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

const COURSE: [&str; 2] = ["20 m", "15 m"];
const SPEEDS: [&str; 3] = ["8, 9", "8, 8.5", "8.5, 9.5"];

fn watch_page(width: f32) -> Harness<'static, App> {
    let dir = tempfile::tempdir().unwrap();
    ui::demo::fill(dir.path(), health::now_s(), false).unwrap();
    let store = platform::open(dir.path()).unwrap();
    // The store keeps the directory open; the test's files go with the
    // process.
    std::mem::forget(dir);
    let mut harness = Harness::builder()
        .with_size([width, 1600.0])
        .build_state(|ctx, app: &mut App| app.show(ctx), App::new(store));
    harness.run_steps(3);
    harness.state_mut().open(Page::Watch);
    harness.run_steps(5);
    harness
}

/// `label`'s row: its chips (`chips`, waiting ones marked " …") neither
/// overlap the label nor each other, and none sticks out of the card.
fn row_fits(harness: &Harness<'_, App>, label: &str, chips: &[&str], case: &str) {
    let l = harness.get_by_label(label).rect();
    // The card's content: from the label's left to the GPS switch's right
    // (it takes the card's width).
    let right = harness.get_by_label("Record GPS speed").rect().right();
    let rects: Vec<_> = chips
        .iter()
        .map(|c| {
            let marked = format!("{c} …");
            let rect = harness
                .query_by_label(c)
                .or_else(|| harness.query_by_label(&marked))
                .unwrap_or_else(|| panic!("{case}: no {c}"))
                .rect();
            (c, rect)
        })
        .collect();
    for (i, (c, r)) in rects.iter().enumerate() {
        assert!(!r.intersects(l), "{case}: {c} {r:?} is over {label} {l:?}");
        assert!(
            r.left() >= l.left() - 0.5 && r.right() <= right + 0.5,
            "{case}: {c} {r:?} is outside the card ({} to {right})",
            l.left()
        );
        for (d, s) in &rects[i + 1..] {
            assert!(!r.intersects(*s), "{case}: {c} {r:?} is over {d} {s:?}");
        }
    }
}

#[test]
fn the_settings_rows_fit_on_small_phones_with_large_text() {
    for (width, scale) in [
        (411.0, 1.0),
        (411.0, 1.3),
        (360.0, 1.0),
        (360.0, 1.15),
        (360.0, 1.3),
        (320.0, 1.0),
        (320.0, 1.3),
    ] {
        // SAFETY: the only test in this process, and nothing else reads
        // the environment while it's set.
        unsafe { std::env::set_var("BEEP_FONT_SCALE", format!("{scale}")) };
        let mut harness = watch_page(width);
        let case = format!("{width} wide, text at {scale}");
        row_fits(&harness, "Course", &COURSE, &case);
        row_fits(&harness, "Speeds", &SPEEDS, &case);
        // A change on its way marks its chip " …".
        harness.get_by_label("8.5, 9.5").click();
        harness.get_by_label("15 m").click();
        harness.run_steps(3);
        harness.get_by_label("8.5, 9.5 …");
        harness.get_by_label("15 m …");
        let case = format!("{case}, sending");
        row_fits(&harness, "Course", &COURSE, &case);
        row_fits(&harness, "Speeds", &SPEEDS, &case);
    }
}
