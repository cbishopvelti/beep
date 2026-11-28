//! Believable demo data, for looking at the pages without a phone or a
//! watch: the desktop app's `--demo` (see `src/bin/desktop.rs`) and the UI
//! tests. Never called on the phone.
//!
//! [`fill`] writes, through the store's own calls, a dozen results over the
//! last five months on both courses (two of them on the faster speeds,
//! 8.5, 9.5, 10.0... km/h), getting better, each with a heart rate log a
//! line a second as the watch's `lib.js` writes it (heart rate rising
//! through the levels, a few low-confidence readings, one test with GPS
//! speed), the newest one's log still arriving, and an old one the watch
//! had no log for; what the watch last said (version, 20 m, GPS off, the
//! standard speeds, optionally a test running) and when it was heard from
//! and synced.

use crate::health::{self, Link};
use crate::link::Hist;
use crate::store::{Entry, Store, WatchState};
use crate::tables;
use std::io;
use std::path::Path;

/// What's in the demo: (days ago, course, speed progression, shuttles
/// completed in all, with GPS). Oldest first.
const TESTS: [(i64, u32, u32, u32, bool); 12] = [
    (150, 20, 0, 52, false),
    (136, 20, 0, 55, false),
    (121, 15, 0, 64, false),
    (107, 20, 0, 58, false),
    (93, 20, 0, 57, false),
    (80, 15, 0, 70, false),
    (66, 20, 0, 63, true),
    (52, 20, 2, 58, false),
    (38, 15, 0, 75, false),
    (24, 20, 2, 62, false),
    (10, 20, 0, 73, false),
    (1, 15, 0, 79, false),
];

/// Which of [`TESTS`] the watch had no log for (`logend` -1).
const NO_LOG: usize = 1;
/// Which one's log is still arriving (half of it here).
const ARRIVING: usize = 11;

/// A small deterministic noise, so the demo looks the same each time.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as f64) / f64::from(1u32 << 31)
    }
}

/// One test's log, as `lib.js` writes it (with the Speeds setting: the
/// start line ends with the progression `p`), and its history entry.
pub fn test_log(
    start_s: i64,
    m: u32,
    p: u32,
    target: u32,
    gps: bool,
    seed: u64,
) -> (String, Entry) {
    let mut noise = Noise(seed);
    // When the target shuttle ends, and a little into the next.
    let mut t_end = 0.0;
    while tables::position(m, t_end, p).total < target {
        t_end += 0.5;
    }
    t_end += 2.0 + 3.0 * noise.next();
    let tz = -watch_offset_min(start_s);
    let mut text = format!("start,{},{tz},{m},{p}\n", start_s * 1000);
    let (mut hr_sum, mut hr_n, mut hr_max) = (0.0, 0u32, 0u32);
    let mut bpm: f64 = 88.0 + 6.0 * noise.next();
    let peak = 186.0 + 8.0 * noise.next();
    let mut low_left = 0;
    let mut t = 0.0;
    while t <= t_end {
        let at = tables::position(m, t, p);
        let into = ((t - at.shuttle_start) / (at.next_at - at.shuttle_start)).clamp(0.0, 1.0);
        let dist = f64::from(at.total * m) + into * f64::from(m);
        let want = (110.0 + 6.5 * f64::from(at.level) + t / 40.0).min(peak);
        bpm += (want - bpm) * 0.06 + (noise.next() - 0.5) * 3.0;
        // Now and then the sensor loses the wrist for a few seconds.
        if low_left == 0 && noise.next() < 0.015 {
            low_left = 3 + (noise.next() * 6.0) as u32;
        }
        let conf = if low_left > 0 {
            low_left -= 1;
            (10.0 + 30.0 * noise.next()).round()
        } else {
            (70.0 + 30.0 * noise.next()).round()
        };
        let read = bpm.round().max(0.0) as u32;
        hr_sum += f64::from(read);
        hr_n += 1;
        hr_max = hr_max.max(read);
        text += &format!("{t:.1},{read},{conf},{},{},{dist:.1}", at.level, at.shuttle);
        if gps {
            // Running up and down a 20 m line, slowing at each turn.
            let speed =
                tables::speed(at.level, p) * (0.75 + 0.35 * (into * std::f64::consts::PI).sin());
            let along = if at.total.is_multiple_of(2) {
                into
            } else {
                1.0 - into
            };
            let lat = 51.500_500 + along * 0.000_18;
            let lon = -0.120_900 + (noise.next() - 0.5) * 0.000_01;
            text += &format!(",{speed:.1},{lat:.6},{lon:.6}");
        }
        text.push('\n');
        t += 1.0;
    }
    let end = tables::position(m, t_end, p);
    let r = tables::result(m, &end, p);
    let avg = if hr_n > 0 {
        (hr_sum / f64::from(hr_n)).round() as u32
    } else {
        0
    };
    let d = end.total * m;
    text += &format!(
        "end,{t_end:.1},{},{},{d},{hr_max},{avg}\n",
        r.level, r.shuttle
    );
    let entry = Entry {
        f: format!("beeptest.{start_s}.csv"),
        t: start_s * 1000,
        m,
        p,
        l: r.level,
        s: r.shuttle,
        d,
        dur: t_end.round() as u32,
        max: hr_max,
        avg,
    };
    (text, entry)
}

/// The watch's `getTimezoneOffset()` for the phone's zone then: minutes
/// behind UTC.
fn watch_offset_min(unix: i64) -> i64 {
    i64::from(crate::watch_ble::utc_offset(unix)) / 60
}

/// The history the watch would send for `entries` (newest first), as
/// `JSON.stringify` writes it.
fn history_json(entries: &[Entry]) -> String {
    let list: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "t": e.t, "m": e.m, "p": e.p, "l": e.l, "s": e.s, "d": e.d,
                "dur": e.dur, "max": e.max, "avg": e.avg, "f": e.f,
            })
        })
        .collect();
    serde_json::Value::Array(list).to_string()
}

/// Fills the store in `dir` with the demo, as of `now_s`; with `running`, a
/// test is running on the watch. What was there is added to.
pub fn fill(dir: &Path, now_s: i64, running: bool) -> io::Result<()> {
    let store = Store::open(dir)?;
    let mut entries = Vec::new();
    let mut logs = Vec::new();
    for (i, &(days, m, p, target, gps)) in TESTS.iter().enumerate() {
        // Early evening, on the day.
        let day = now_s - days * 86_400;
        let start = day - day.rem_euclid(86_400) + 17 * 3600 + 1800 + (i as i64 * 7 * 60) % 3600
            - i64::from(crate::watch_ble::utc_offset(day));
        let start = start.min(now_s - 3600);
        let (text, entry) = test_log(start, m, p, target, gps, 7 + i as u64);
        logs.push((entry.f.clone(), text));
        entries.push(entry);
    }
    entries.reverse();
    logs.reverse();
    let json = history_json(&entries);
    store.merge_history(&entries, Hist::of(json.as_bytes()))?;
    let last = TESTS.len() - 1;
    for (i, (f, text)) in logs.iter().enumerate() {
        let i = last - i;
        if i == NO_LOG {
            store.set_no_log(f)?;
        } else if i == ARRIVING {
            store.drop_part(f)?;
            store.append_part(f, &text.as_bytes()[..text.len() / 2])?;
        } else {
            store.drop_part(f)?;
            store.append_part(f, text.as_bytes())?;
            store.finish_log(f)?;
        }
    }
    store.save_watch_state(&WatchState {
        version: Some("0.05".into()),
        m: Some(20),
        gps: Some(false),
        p: Some(0),
        run: running.then(|| format!("beeptest.{}.csv", now_s - 95)),
        heard: Some(now_s - 240),
    })?;
    health::seen(dir, Link::Watch, now_s - 240);
    health::seen(dir, Link::Synced, now_s - 26 * 3600);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beeplog;
    use crate::store::LogState;

    #[test]
    fn a_demo_log_reads_as_the_watch_s_would() {
        let (text, entry) = test_log(1_790_000_000, 20, 0, 63, true, 3);
        let log = beeplog::parse(&text);
        let end = log.end.expect("an end line");
        assert_eq!(
            (end.level, end.shuttle),
            (f64::from(entry.l), f64::from(entry.s))
        );
        assert_eq!(entry.total_shuttles(), 63);
        assert_eq!(end.dist, f64::from(entry.d));
        assert!(log.samples.len() as u32 >= entry.dur - 1);
        assert!(log.samples.iter().all(|s| !s.gps_speed.is_nan()));
        assert!(beeplog::to_tcx(&log, 50.0).is_ok());
        let rows = beeplog::level_rows(&log, 0.0);
        assert!(rows.last().unwrap().level >= entry.l);
        assert!(entry.max > entry.avg && entry.avg > 100, "{entry:?}");
        // On other speeds: its own tables, and the start line says which.
        let (text, entry) = test_log(1_790_000_000, 15, 1, 63, false, 3);
        assert!(text.starts_with("start,1790000000000,"));
        assert!(text.lines().next().unwrap().ends_with(",15,1"));
        assert_eq!((entry.p, entry.total_shuttles()), (1, 63));
        assert_eq!(beeplog::parse(&text).progression, 1);
    }

    #[test]
    fn the_demo_fills_a_store() {
        let dir = tempfile::tempdir().unwrap();
        fill(dir.path(), 1_795_000_000, true).unwrap();
        let store = Store::open(dir.path()).unwrap();
        let results = store.results();
        assert_eq!(results.len(), 12);
        assert!(results.windows(2).all(|w| w[0].entry.t > w[1].entry.t));
        assert!(matches!(results[0].log, LogState::Partial(n) if n > 0));
        assert_eq!(results[10].log, LogState::NoLog);
        assert_eq!(
            results
                .iter()
                .filter(|r| r.log == LogState::Complete)
                .count(),
            10
        );
        // 20 m and 15 m on the standard speeds, and 20 m on the faster.
        assert_eq!(store.bests().len(), 3);
        assert_eq!(results.iter().filter(|r| r.entry.p == 2).count(), 2);
        assert!(store.watch_state().run.is_some());
        // Filling again leaves it as it was.
        fill(dir.path(), 1_795_000_000, false).unwrap();
        assert_eq!(store.results().len(), 12);
        assert_eq!(store.watch_state().run, None);
    }
}
