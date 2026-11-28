//! A result whose log carries numbers no test has (a sample at 1e12 s, a
//! heart rate of Infinity: a corrupt log, or a broadcast forged by another
//! app, since the receiver is exported) opens, draws in a moment and can be
//! deleted. Before the chart kept to what a test can be, either one hung
//! the frame until the app ran out of memory, every time the result was
//! opened, and Delete is on that page. A file of its own: the process has
//! one store (`platform::open`).

use beep_android::link::Hist;
use beep_android::platform;
use beep_android::store::{Entry, LogState};
use beep_android::ui::{App, Page};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use std::time::{Duration, Instant};

const LATE: &str = "beeptest.1781000000.csv";
const INFINITE: &str = "beeptest.1781100000.csv";

fn entry(f: &str, t_s: i64) -> Entry {
    Entry {
        f: f.into(),
        t: t_s * 1000,
        m: 20,
        p: 0,
        l: 1,
        s: 1,
        d: 20,
        dur: 10,
        max: 120,
        avg: 100,
    }
}

#[test]
fn a_log_with_numbers_no_test_has_opens_draws_and_deletes() {
    // A frame that never finishes fails the test rather than hanging it.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        eprintln!("a frame of the result page took two minutes");
        std::process::exit(1);
    });
    let dir = tempfile::tempdir().unwrap();
    let store = platform::open(dir.path()).unwrap();
    store
        .merge_history(
            &[entry(INFINITE, 1_781_100_000), entry(LATE, 1_781_000_000)],
            Hist { len: 1, crc: 1 },
        )
        .unwrap();
    // No end line, so parse keeps the sample at 1e12 s.
    let logs = [
        (
            LATE,
            "start,1781000000000,0,20\n0.0,90,90,1,0,0.0\n2.0,95,90,1,0,0.0\n\
             1000000000000.0,120,90,1,1,20.0\n",
        ),
        (
            INFINITE,
            "start,1781100000000,0,20\n0.0,90,90,1,0,0.0\n2.0,Infinity,90,1,1,20.0\n\
             4.0,100,90,1,1,20.0\nend,5,1,1,20,Infinity,100\n",
        ),
    ];
    for (f, text) in logs {
        store.append_part(f, text.as_bytes()).unwrap();
        store.finish_log(f).unwrap();
        assert_eq!(store.log_state(f), LogState::Complete);
    }
    let mut harness = Harness::builder()
        .with_size([320.0, 1900.0])
        .build_state(|ctx, app: &mut App| app.show(ctx), App::new(store.clone()));
    harness.run_steps(3);
    for f in [LATE, INFINITE] {
        harness.state_mut().open(Page::Result(f.into()));
        let at = Instant::now();
        harness.run_steps(3);
        assert!(
            at.elapsed() < Duration::from_secs(20),
            "{f}: three frames took {:?}",
            at.elapsed()
        );
        harness.get_by_label("Heart rate");
        harness.get_by_label("Share TCX");
        // And it can be deleted.
        harness.get_by_label("Delete this result").click();
        harness.run_steps(3);
        harness.get_by_label("Delete").click();
        harness.run_steps(3);
        assert_eq!(harness.state().page(), &Page::Results);
        assert!(store.result(f).is_none(), "{f} deleted");
    }
    // None left: how to connect, and which watch app syncs.
    harness.run_steps(2);
    harness.get_by_label("No results yet");
    harness.get_by_label("Beep Test 0.05 or later on the watch");
    harness.get_by_label_contains("cbishopvelti.github.io/beep");
}
