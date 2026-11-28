//! The files shared with the watch's tests (`tests/fixtures/` at the repo
//! root), as blink's `phone-line.txt`:
//!
//! - `phone-lines.txt` is one of each line the phone sends, exactly as
//!   `link` builds them. This test fails when the file is out of date;
//!   `BEEP_BLESS=1 cargo test --test fixtures` writes it again, and then
//!   the watch's tests (`tests/fixtures.test.js`) check the watch reads
//!   every line.
//! - `watch-lines.txt` is what the watch printed for a scripted exchange
//!   (the watch's tests make it, on the emulator): fed through `link` and
//!   the sync engine, it must leave the store with the results and logs
//!   the exchange carried. A watch whose engine has no speed progressions
//!   (before its Speeds setting) prints its states without `p`, and one
//!   with them with `p`: this test takes either, and checks the phone
//!   kept what the watch said.

use beep_android::link::{self, HISTORY, Hist};
use beep_android::store::{Entry, LogState, Store, Tombstone};
use beep_android::sync::{Engine, Input, Progress};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(name)
}

const A: &str = "beeptest.1781344800.csv";
const B: &str = "beeptest.1781258400.csv";
const C: &str = "beeptest.1781172000.csv";
const D: &str = "beeptest.1781085600.csv";

/// One of each line, with the names the watch's fixture test has on the
/// watch (A, B and C; D deleted there).
fn phone_lines() -> String {
    [
        link::sync_line(&[]),
        link::sync_line(&[C.to_string(), D.to_string()]),
        link::get_line(HISTORY, 0),
        link::get_line(A, 0),
        link::get_line(B, 2000),
        link::get_line(C, 0),
        link::get_line(HISTORY, 100_000),
        link::set_line(Some(15), Some(true), None),
        link::set_line(Some(20), None, None),
        link::set_line(None, Some(false), None),
        link::set_line(None, None, None),
        link::set_line(None, None, Some(2)),
        link::set_line(Some(15), Some(true), Some(1)),
    ]
    .concat()
}

#[test]
fn phone_lines_txt_is_what_the_phone_sends() {
    let path = fixture("phone-lines.txt");
    let lines = phone_lines();
    assert!(
        lines
            .split('\n')
            .all(|l| l.is_empty() || (l.starts_with("\u{10}GB(") && l[1..].is_ascii())),
        "GB() lines, ASCII after the \\x10"
    );
    if std::env::var_os("BEEP_BLESS").is_some() {
        std::fs::write(&path, &lines).unwrap();
        eprintln!("wrote {}", path.display());
        return;
    }
    let on_file = std::fs::read_to_string(&path).unwrap_or_default();
    if on_file != lines {
        let i = lines
            .split('\n')
            .zip(on_file.split('\n').chain(std::iter::repeat("")))
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "tests/fixtures/phone-lines.txt is out of date (line {} differs: now {:?}, file has {:?}). \
             If the change is meant, run BEEP_BLESS=1 cargo test --test fixtures, then the watch's \
             tests (node tests/run.js tests/fixtures.test.js), which read it.",
            i + 1,
            lines.split('\n').nth(i),
            on_file.split('\n').nth(i)
        );
    }
}

/// The watch's lines from the exchange, as the phone would get them: each
/// read off the console ([`link::extras_from_line`]) and checked
/// ([`link::parse_event`]).
fn watch_events() -> Vec<Input> {
    let text = std::fs::read(fixture("watch-lines.txt")).expect("tests/fixtures/watch-lines.txt");
    let text: String = text.iter().map(|&b| char::from(b)).collect();
    text.lines()
        .map(|line| {
            let extras = link::extras_from_line(line)
                .unwrap_or_else(|| panic!("not a message for the phone: {line:.80}"));
            let event = link::parse_event(&extras)
                .unwrap_or_else(|why| panic!("refused ({why}): {line:.80}"));
            Input::Watch(event)
        })
        .collect()
}

/// The pieces of `f` in the watch's lines, in order, joined.
fn pieces_of(events: &[Input], f: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for e in events {
        if let Input::Watch(link::WatchEvent::Log(p)) = e
            && p.f == f
        {
            assert_eq!(p.at as usize, out.len(), "{f}'s pieces follow on");
            assert!(p.crc_ok(), "{f} at {}: the watch's CRC is zlib's", p.at);
            out.extend_from_slice(&p.d);
        }
    }
    out
}

#[test]
fn watch_lines_txt_leaves_the_exchanges_results_stored() {
    let events = watch_events();
    assert_eq!(events.len(), 21);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    // Before the exchange: the phone had C, and the user deleted it there.
    let c = Entry {
        f: C.into(),
        t: 1_781_172_000_000,
        m: 20,
        p: 0,
        l: 1,
        s: 3,
        d: 60,
        dur: 30,
        max: 139,
        avg: 120,
    };
    store
        .merge_history(std::slice::from_ref(&c), Hist { len: 1, crc: 1 })
        .unwrap();
    assert!(store.delete(C).unwrap());

    let mut engine = Engine::new();
    let mut now = 1_781_400_000_000;
    let mut sent = Vec::new();
    // The app opens: the sync deleting C, as in the exchange.
    sent.extend(engine.handle(&store, Input::Opened, now).lines);
    assert_eq!(sent, vec![link::sync_line(&[C.into()])]);
    for event in events.iter().cloned() {
        now += 50;
        sent.extend(engine.handle(&store, event, now).lines);
    }
    // What the phone asked for along the way: the history (after each of
    // the first two states, the second replacing the first), then A, then B.
    assert_eq!(
        sent[1..],
        [
            link::get_line(HISTORY, 0),
            link::get_line(HISTORY, 0),
            link::get_line(A, 0),
            link::get_line(B, 0),
        ]
    );
    assert!(
        matches!(engine.progress(), Progress::Done { failed, .. } if failed.is_empty()),
        "{:?}",
        engine.progress()
    );

    let results = store.results();
    let names: Vec<&str> = results.iter().map(|r| r.entry.f.as_str()).collect();
    assert_eq!(names, vec![A, B]);
    assert_eq!(
        results[0].entry,
        Entry {
            f: A.into(),
            t: 1_781_344_800_000,
            m: 20,
            p: 0,
            l: 7,
            s: 3,
            d: 1080,
            dur: 400,
            max: 139,
            avg: 120,
        }
    );
    assert_eq!(
        (
            results[1].entry.m,
            results[1].entry.l,
            results[1].entry.s,
            results[1].entry.d
        ),
        (15, 3, 5, 360)
    );
    // The logs, byte for byte what the watch sent.
    for (f, len) in [(A, 9140), (B, 3361)] {
        assert_eq!(store.log_state(f), LogState::Complete, "{f}");
        let bytes = std::fs::read(store.log_path(f)).unwrap();
        assert_eq!(bytes.len(), len, "{f}");
        assert_eq!(bytes, pieces_of(&events, f), "{f}");
        assert_eq!(store.part_len(f), 0);
    }
    let a = store.log(A).unwrap();
    assert_eq!(
        (a.course, a.end.unwrap().level, a.end.unwrap().shuttle),
        (20.0, 7.0, 3.0)
    );
    assert_eq!(store.log(B).unwrap().course, 15.0);
    // C (deleted here, then gone from the watch's history) and D (deleted
    // on the watch) are tombstones, done.
    let tombstones = store.tombstones();
    assert_eq!(tombstones.get(C), Some(&Tombstone::Done));
    assert_eq!(tombstones.get(D), Some(&Tombstone::Done));
    assert_eq!(store.pending_deletes(), Vec::<String>::new());
    // The history as fetched, and the watch's settings as it last said.
    assert_eq!(
        store.watch_history().hist,
        Hist::parse("216:21098452").unwrap()
    );
    assert_eq!(store.watch_history().listed, vec![A, B]);
    let w = store.watch_state();
    assert_eq!(
        (w.version.as_deref(), w.m, w.gps, w.run),
        (Some("0.05"), Some(15), Some(true), None)
    );
    // The speeds: the exchange sets p 1, which a watch without the Speeds
    // setting ignores (its states have no p), and one with it says.
    let ps: Vec<Option<u32>> = events
        .iter()
        .filter_map(|e| match e {
            Input::Watch(link::WatchEvent::State(s)) => Some(s.p),
            _ => None,
        })
        .collect();
    if ps.iter().any(Option::is_some) {
        assert_eq!(ps, [Some(0), Some(0), Some(1)]);
        assert_eq!(w.p, Some(1));
    } else {
        assert_eq!(ps, [None, None, None]);
        assert_eq!(w.p, None);
    }
}
