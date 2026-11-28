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

    // The option, then a CSV: the desktop writes it to exports/ and says
    // where.
    harness
        .get_by_label("Leave out low-confidence heart rate readings (<50%)")
        .click();
    harness.run_steps(2);
    harness.get_by_label("Share CSV").click();
    harness.run_steps(2);
    harness.get_by_label_contains("Written to ");
    let exported: Vec<_> = std::fs::read_dir(store.dir().join("exports"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    assert!(exported.iter().any(|f| f.ends_with(".csv")), "{exported:?}");

    // The back key: back to Results, where it's left alone.
    harness.key_press(egui::Key::BrowserBack);
    harness.run_steps(2);
    assert_eq!(harness.state().page(), &Page::Results);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert_eq!(harness.state().page(), &Page::Results);

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
