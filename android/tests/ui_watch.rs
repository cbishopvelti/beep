//! Clicks through the menu, Watch and Settings headlessly (`egui_kittest`,
//! no pixels), on the demo data: a course change waits for the watch and
//! shows once it answers, and so do the speeds, offered only while the
//! watch says it has them; a test starting shows the banner on Results;
//! the theme is kept. A file of its own: the process has one store
//! (`platform::open`).

use beep_android::store::Theme;
use beep_android::ui::{App, Page};
use beep_android::{health, platform, ui};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use std::collections::BTreeMap;

/// The watch says `state`, as Gadgetbridge's broadcast would bring it
/// (with the speeds `p` from a watch app that has them).
fn watch_says(m: &str, gps: &str, p: Option<&str>, run: &str) {
    let store = platform::store().unwrap();
    let mut extras: BTreeMap<String, String> = [
        ("event", "state"),
        ("v", "0.05"),
        ("m", m),
        ("gps", gps),
        ("run", run),
        ("hist", &store.watch_history().hist.to_string()),
        ("del", ""),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if let Some(p) = p {
        extras.insert("p".into(), p.into());
    }
    platform::from_watch(&extras);
}

/// Whether the chip called `label` is drawn as the one chosen.
fn chosen(harness: &Harness<'_, App>, label: &str) -> bool {
    let toggled = harness.get_by_label(label).accesskit_node().toggled();
    assert!(toggled.is_some(), "{label} is a choice");
    toggled == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_menu_the_watch_settings_and_the_theme() {
    let dir = tempfile::tempdir().unwrap();
    ui::demo::fill(dir.path(), health::now_s(), false).unwrap();
    let store = platform::open(dir.path()).unwrap();
    let mut harness = Harness::builder()
        .with_size([411.0, 1600.0])
        .build_state(|ctx, app: &mut App| app.show(ctx), App::new(store.clone()));
    harness.run_steps(3);

    // The menu to Watch.
    harness.get_by_label("Menu").click();
    harness.run_steps(15);
    harness.get_by_label_contains("Watch").click();
    harness.run_steps(15);
    assert_eq!(harness.state().page(), &Page::Watch);
    harness.get_by_label("Beep Test 0.05");

    // 15 m: sent, and waiting for the watch, which still says 20 m.
    assert!(chosen(&harness, "20 m") && !chosen(&harness, "15 m"));
    harness.get_by_label("15 m").click();
    harness.run_steps(3);
    harness.get_by_label("Sending the 15 m course to the watch...");
    harness.get_by_label("15 m …");
    assert!(chosen(&harness, "20 m"), "the watch's own until it answers");
    // Changed back before it answers: 20 m goes too, and is waited for.
    harness.get_by_label("20 m").click();
    harness.run_steps(3);
    harness.get_by_label("Sending the 20 m course to the watch...");
    harness.get_by_label("20 m …");
    // And to 15 m again. The watch answers the 20 m first: still waiting.
    harness.get_by_label("15 m").click();
    harness.run_steps(3);
    harness.get_by_label("15 m …");
    watch_says("20", "false", Some("0"), "");
    harness.run_steps(3);
    harness.get_by_label("Sending the 15 m course to the watch...");
    // The watch answers: shown as the watch has it, nothing waiting.
    watch_says("15", "false", Some("0"), "");
    harness.run_steps(3);
    assert_eq!(store.watch_state().m, Some(15));
    assert!(
        harness
            .query_by_label("Sending the 15 m course to the watch...")
            .is_none()
    );
    assert!(chosen(&harness, "15 m") && !chosen(&harness, "20 m"));

    // GPS speed, the same way, and the switch shows what's on its way,
    // so a second tap takes it back.
    harness.get_by_label("Record GPS speed").click();
    harness.run_steps(3);
    harness.get_by_label("Sending GPS speed on to the watch...");
    harness.get_by_label("Record GPS speed").click();
    harness.run_steps(3);
    harness.get_by_label("Sending GPS speed off to the watch...");
    harness.get_by_label("Record GPS speed").click();
    harness.run_steps(3);
    harness.get_by_label("Sending GPS speed on to the watch...");
    watch_says("15", "true", Some("0"), "");
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Sending GPS speed on to the watch...")
            .is_none()
    );
    assert_eq!(store.watch_state().gps, Some(true));

    // The speeds, the same way (the demo's watch has them).
    harness.get_by_label("Speeds");
    harness.get_by_label("8.5, 9.5").click();
    harness.run_steps(3);
    harness.get_by_label("Sending the speeds 8.5, 9.5 to the watch...");
    harness.get_by_label("8.5, 9.5 …");
    watch_says("15", "true", Some("2"), "");
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Sending the speeds 8.5, 9.5 to the watch...")
            .is_none()
    );
    assert_eq!(store.watch_state().p, Some(2));
    assert!(chosen(&harness, "8.5, 9.5") && !chosen(&harness, "8, 9"));
    harness.get_by_label_contains("8.5, 9.5, 10.0... km/h now");
    // A watch app without the Speeds setting: no Speeds to choose.
    watch_says("15", "true", None, "");
    harness.run_steps(3);
    assert_eq!(store.watch_state().p, None);
    assert!(harness.query_by_label("Speeds").is_none());
    assert!(harness.query_by_label("8.5, 9.5").is_none());
    harness.get_by_label("Record GPS speed");

    // Back goes to Results; a test starting on the watch shows there.
    harness.key_press(egui::Key::BrowserBack);
    harness.run_steps(3);
    assert_eq!(harness.state().page(), &Page::Results);
    assert!(
        harness
            .query_by_label("A test is running on the watch")
            .is_none()
    );
    watch_says(
        "15",
        "true",
        None,
        &format!("beeptest.{}.csv", health::now_s()),
    );
    harness.run_steps(3);
    harness.get_by_label("A test is running on the watch");
    watch_says("15", "true", None, "");
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("A test is running on the watch")
            .is_none()
    );

    // Settings: the theme, kept.
    harness.state_mut().open(Page::Settings);
    harness.run_steps(3);
    harness.get_by_label("Dark").click();
    harness.run_steps(3);
    assert_eq!(store.theme(), Theme::Dark);
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);
    harness.get_by_label("Light").click();
    harness.run_steps(3);
    assert_eq!(harness.ctx.theme(), egui::Theme::Light);
    harness.get_by_label(&format!("Beep Test {}", env!("CARGO_PKG_VERSION")));
}
