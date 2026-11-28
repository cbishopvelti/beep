//! Clicks through Results and a result headlessly (`egui_kittest`, no
//! pixels), on the demo data: opening a result, the confidence option,
//! sharing on the desktop, the back key, and deleting with the confirm.
//! A file of its own: the process has one store (`platform::open`).

use beep_android::store::{LogState, Store};
use beep_android::ui::{App, Page};
use beep_android::{health, platform, ui};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use std::sync::{Arc, OnceLock};

/// The process's store, on the demo data: filled once, shared by the
/// tests here (which each leave alone what the other looks at).
fn store() -> Arc<Store> {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        ui::demo::fill(dir.path(), health::now_s(), false).unwrap();
        dir
    });
    platform::open(dir.path()).unwrap()
}

fn harness() -> (Arc<Store>, Harness<'static, App>) {
    let store = store();
    let app = App::new(store.clone());
    let mut harness = Harness::builder()
        .with_size([411.0, 2400.0])
        .build_state(|ctx, app: &mut App| app.show(ctx), app);
    harness.run_steps(3);
    (store, harness)
}

#[test]
fn open_a_result_share_it_go_back_and_delete_one() {
    let (store, mut harness) = harness();
    // The first frame asked the watch.
    assert!(platform::progress().active() || !platform::progress().words().is_empty());
    harness.get_by_label("Progress");

    // The newest result with its log: open its card.
    let newest = store
        .results()
        .into_iter()
        .find(|r| r.log == LogState::Complete)
        .unwrap();
    let card = format!(
        "Result {} on {} m",
        newest.entry.score_text(),
        newest.entry.m
    );
    harness.get_by_label_contains(&card).click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().page(),
        &Page::Result(newest.entry.f.clone())
    );
    harness.get_by_label("Share TCX");
    harness.get_by_label("Levels");

    // The option, then a TCX: the desktop writes it to exports/ and says
    // where. It leaves out the readings below 50% (the demo's logs have
    // some), as beeplog.js's toTCX(log, {minConfidence: 50}) does.
    harness
        .get_by_label("Leave out low-confidence heart rate readings (<50%)")
        .click();
    harness.run_steps(2);
    harness.get_by_label("Share TCX").click();
    harness.run_steps(2);
    harness.get_by_label_contains("Written to ");
    let log = store.log(&newest.entry.f).unwrap();
    let want = beep_android::beeplog::to_tcx(&log, 50.0).unwrap();
    assert_ne!(
        want,
        beep_android::beeplog::to_tcx(&log, 0.0).unwrap(),
        "the demo log has low-confidence readings"
    );
    let tcx: Vec<_> = std::fs::read_dir(store.dir().join("exports"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tcx"))
        .collect();
    assert_eq!(tcx.len(), 1);
    assert_eq!(std::fs::read_to_string(tcx[0].path()).unwrap(), want);
    // A CSV has every reading, whatever the option.
    harness.get_by_label("Share CSV").click();
    harness.run_steps(2);
    harness.get_by_label_contains("Written to ");
    let exported: Vec<_> = std::fs::read_dir(store.dir().join("exports"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    assert!(exported.iter().any(|f| f.ends_with(".csv")), "{exported:?}");

    // The back key: back to Results. There it leaves the app (Android
    // moves it to the background), and Escape does nothing.
    let left = platform::left();
    harness.key_press(egui::Key::BrowserBack);
    harness.run_steps(2);
    assert_eq!(harness.state().page(), &Page::Results);
    assert_eq!(platform::left(), left, "back to Results doesn't leave");
    harness.key_press(egui::Key::BrowserBack);
    harness.run_steps(2);
    assert_eq!(harness.state().page(), &Page::Results);
    assert_eq!(platform::left(), left + 1, "back on Results leaves");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert_eq!(harness.state().page(), &Page::Results);
    assert_eq!(platform::left(), left + 1, "Escape doesn't");

    // Delete, asked first: Cancel keeps it.
    harness.get_by_label_contains(&card).click();
    harness.run_steps(3);
    harness.get_by_label("Delete this result").click();
    harness.run_steps(3);
    harness.get_by_label("Delete this result?");
    harness.get_by_label("Cancel").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("Delete this result?").is_none());
    assert_eq!(store.results().len(), 12);

    // The back key closes the confirm before leaving the page.
    harness.get_by_label("Delete this result").click();
    harness.run_steps(3);
    harness.key_press(egui::Key::BrowserBack);
    harness.run_steps(3);
    assert!(harness.query_by_label("Delete this result?").is_none());
    assert_eq!(
        harness.state().page(),
        &Page::Result(newest.entry.f.clone())
    );

    // Delete for good: back on Results, gone here, and the watch is asked
    // to delete it at the next sync.
    harness.get_by_label("Delete this result").click();
    harness.run_steps(3);
    harness.get_by_label("Delete").click();
    harness.run_steps(3);
    assert_eq!(harness.state().page(), &Page::Results);
    assert_eq!(store.results().len(), 11);
    assert!(store.result(&newest.entry.f).is_none());
    assert_eq!(store.pending_deletes(), vec![newest.entry.f.clone()]);
    assert!(harness.query_by_label_contains(&card).is_none());
}

#[test]
fn a_result_without_its_log_says_so_and_offers_no_shares() {
    let (store, mut harness) = harness();
    for want in [LogState::NoLog, LogState::Partial(1)] {
        let r = store
            .results()
            .into_iter()
            .find(|r| std::mem::discriminant(&r.log) == std::mem::discriminant(&want))
            .unwrap();
        harness.state_mut().open(Page::Result(r.entry.f.clone()));
        harness.run_steps(3);
        let said = match want {
            LogState::NoLog => "No heart rate log",
            _ => "The log is still on the watch",
        };
        harness.get_by_label(said);
        assert!(harness.query_by_label("Share TCX").is_none());
        assert!(harness.query_by_label("Share CSV").is_none());
        harness.get_by_label("Delete this result");
    }
}

#[test]
fn a_result_on_other_speeds_says_them_and_has_its_own_best() {
    let (store, mut harness) = harness();
    // The demo's two on 8.5, 9.5, 10.0... km/h (20 m): the further is the
    // best on those speeds, whatever the standard speeds' results are.
    let fast: Vec<_> = store
        .results()
        .into_iter()
        .filter(|r| r.entry.p == 2)
        .collect();
    assert_eq!(fast.len(), 2);
    let best = fast.iter().max_by_key(|r| r.entry.d).unwrap();
    assert_eq!(
        store.bests().get(&(20, 2)),
        Some(&best.entry.f),
        "{:?}",
        store.bests()
    );
    let card = format!(
        "Result {} on 20 m at 8.5, 9.5, 10.0... km/h",
        best.entry.score_text()
    );
    harness.get_by_label_contains(&card).click();
    harness.run_steps(3);
    assert_eq!(harness.state().page(), &Page::Result(best.entry.f.clone()));
    harness.get_by_label("Speeds");
    harness.get_by_label("8.5, 9.5, 10.0... km/h");
    harness.get_by_label("★ Best");
}

#[test]
fn back_from_a_result_is_where_the_list_was() {
    let store = store();
    let app = App::new(store.clone());
    let mut harness = Harness::builder()
        .with_size([411.0, 800.0])
        .build_state(|ctx, app: &mut App| app.show(ctx), app);
    harness.run_steps(3);
    // The oldest result with its log, far down the list: scrolled to.
    let oldest = store
        .results()
        .into_iter()
        .rfind(|r| r.log == LogState::Complete)
        .unwrap();
    let card = format!(
        "Result {} on {} m",
        oldest.entry.score_text(),
        oldest.entry.m
    );
    harness.get_by_label_contains(&card).scroll_to_me();
    harness.run_steps(5);
    let at = harness.get_by_label_contains(&card).rect();
    assert!(at.top() > 0.0 && at.bottom() < 800.0, "{at:?}");
    let top = harness.get_by_label("Progress").rect();
    assert!(top.bottom() < 0.0, "scrolled past the chart: {top:?}");
    // Open it: the page starts at its top.
    harness.get_by_label_contains(&card).click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().page(),
        &Page::Result(oldest.entry.f.clone())
    );
    let distance = harness.get_by_label("Distance").rect();
    assert!(
        distance.top() > 0.0 && distance.bottom() < 800.0,
        "{distance:?}"
    );
    // Back (the key, then the arrow): the list is where it was.
    for back in [true, false] {
        if back {
            harness.key_press(egui::Key::BrowserBack);
        } else {
            harness.get_by_label_contains(&card).click();
            harness.run_steps(3);
            harness.get_by_label("Back").click();
        }
        harness.run_steps(3);
        assert_eq!(harness.state().page(), &Page::Results);
        let again = harness.get_by_label_contains(&card).rect();
        assert!((again.top() - at.top()).abs() < 1.0, "{again:?} not {at:?}");
    }
}

#[test]
fn the_progress_legend_keeps_clear_of_its_units_on_a_narrow_screen() {
    let store = store();
    // Each course and speed progression the chart has a line for.
    let mut labels: Vec<String> = Vec::new();
    for r in store.results() {
        let e = &r.entry;
        let label = if e.p == 0 {
            format!("{} m", e.m)
        } else {
            format!("{} m, {}", e.m, beep_android::tables::speeds_text(e.p))
        };
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    assert!(labels.len() >= 3, "{labels:?}");
    for width in [411.0, 320.0] {
        let app = App::new(store.clone());
        let mut harness = Harness::builder()
            .with_size([width, 1200.0])
            .build_state(|ctx, app: &mut App| app.show(ctx), app);
        harness.run_steps(3);
        let units = harness.get_by_label("shuttles").rect();
        assert!(units.right() <= width, "{width}: {units:?}");
        // By the heading, which keeps its height.
        let heading = harness.get_by_label("Progress").rect();
        assert!(
            units.top() < heading.bottom() && heading.top() < units.bottom(),
            "{width}: {units:?} beside {heading:?}"
        );
        let mut tops = Vec::new();
        for label in &labels {
            let item = harness.get_by_label(label).rect();
            tops.push(item.top());
            assert!(
                item.left() >= 0.0 && item.right() <= width,
                "{width}: {label} at {item:?}"
            );
            // One line: an item's words aren't split from its colour.
            assert!(item.height() < 24.0, "{width}: {label} at {item:?}");
            assert!(
                !item.intersects(units),
                "{width}: {label} at {item:?} runs into shuttles at {units:?}"
            );
        }
        // Lines of the legend a line apart, not a touch's height.
        tops.sort_by(f32::total_cmp);
        for pair in tops.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(gap < 26.0, "{width}: lines {gap} apart");
        }
    }
}
