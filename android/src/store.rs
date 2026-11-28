//! The phone's store: plain files in the app's files directory (on the
//! desktop `./beep-data`, or `$BEEP_DATA`). The phone is the long-term
//! store: the watch only remembers its last 30 results.
//!
//! | File | What |
//! | --- | --- |
//! | `results.json` | Every result (the watch's history entries, by log file name `f`), the tombstones of deleted ones, and the watch's history as last fetched whole (its `len:crc` and the names it listed, in its order). |
//! | `logs/<f>` | A result's log, complete. |
//! | `logs/<f>.part` | A log still arriving: the bytes so far, appended a piece at a time. |
//! | `watch.json` | What the watch last said ([`WatchState`]: version, course, GPS, running test, when), and the direct link's setting (`crate::watch_ble::Settings`), each owner writing only its own keys ([`update_json`]). |
//! | `app.json` | The theme ([`Theme`]). |
//! | `health/` | When the watch was last heard from and last synced (`crate::health`). |
//! | `exports/` | TCX and CSV files made to share ([`Store::export`]); Android's share sheet reads them through the `FileProvider`. |
//!
//! `results.json` looks like
//!
//! ```json
//! {"results":[{"f":"beeptest.1764334800.csv","t":1764334800123,"m":20,"l":9,"s":4,
//!   "d":1480,"dur":543,"max":187,"avg":160}],
//!  "tombstones":{"beeptest.1764248400.csv":"pending"},
//!  "hist":"1234:3735928559","listed":["beeptest.1764334800.csv"]}
//! ```
//!
//! with `"nolog":true` on a result whose log the watch said it hasn't got.
//!
//! Every write is atomic ([`write_atomic`]: a temp file, then a rename), so
//! a reader (the UI, on its own thread) sees the old file or the new one,
//! and the app being killed mid-write loses nothing. The sync engine
//! (`crate::sync`) is the only writer of `results.json`, the logs and
//! [`WatchState`], and runs one input at a time (`crate::platform::dispatch`);
//! the UI writes `app.json` and the direct link's setting. `watch.json` has
//! two writers, so its writes are read-modify-write under one lock
//! ([`update_json`]).
//!
//! **Deleting.** A result deleted on the phone ([`Store::delete`]) goes at
//! once, with its log, and leaves a *pending* tombstone: the watch hasn't
//! been seen without it yet, so the next `beeptest-sync` asks the watch to
//! delete it too. Once a history fetched whole no longer lists it, the
//! tombstone is *done* ([`Store::merge_history`]). A result deleted on the
//! watch (`state`'s `del`) is deleted here with a done tombstone
//! ([`Store::deleted_on_watch`]). Tombstones are kept for good: no history
//! ever brings back a result with one, pending or done (a watch restored
//! from a backup, say). There is no undo on the phone: the page asks first.

use crate::beeplog;
use crate::link::Hist;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A history entry, as the watch's `lib.js` `stop` writes it to
/// `beeptest.json`. The key is `f`: the result's `t` can move when the
/// watch's clock is set mid-test, the log's file name doesn't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The log's file name, `beeptest.<start s>.csv`.
    pub f: String,
    /// When the test started, ms since 1970.
    pub t: i64,
    /// The course, metres (20 or 15).
    pub m: u32,
    /// The result, `l.s`: the last fully completed shuttle.
    pub l: u32,
    pub s: u32,
    /// Distance run, metres.
    pub d: u32,
    /// How long the test lasted, seconds.
    pub dur: u32,
    /// Highest and average heart rate, bpm (0 when none was read).
    pub max: u32,
    pub avg: u32,
}

impl Entry {
    /// The result as `level.shuttle`: "9.4".
    pub fn score_text(&self) -> String {
        todo!()
    }

    /// Shuttles completed in all (`crate::tables::total_shuttles`): what
    /// the progress chart plots.
    pub fn total_shuttles(&self) -> u32 {
        todo!()
    }
}

/// How much of a result's log the phone has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogState {
    /// `logs/<f>`: all of it.
    Complete,
    /// `logs/<f>.part`: this many bytes so far.
    Partial(u64),
    /// None of it yet.
    Missing,
    /// The watch said it hasn't got it (`logend` with `len` `-1`): it was
    /// deleted there, or never written. Not asked for again.
    NoLog,
}

/// A result on the phone: its history entry and how much of its log is
/// here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    pub entry: Entry,
    pub log: LogState,
}

/// A deleted result's tombstone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tombstone {
    /// Deleted on the phone; the watch hasn't been seen without it yet.
    Pending,
    /// Gone from the watch too.
    Done,
}

/// The watch's history as last fetched whole: what `state`'s `hist` is
/// compared with, and which logs to fetch (in the watch's order, newest
/// first). `Hist` `0:0` and nothing listed before the first fetch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchHistory {
    pub hist: Hist,
    pub listed: Vec<String>,
}

/// What [`Store::merge_history`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Merged {
    /// Results new to the phone.
    pub added: usize,
    /// Known results whose entry changed.
    pub updated: usize,
    /// Pending tombstones the watch no longer lists, now done.
    pub settled: usize,
}

impl Merged {
    /// Whether anything the UI shows changed.
    pub fn changed(&self) -> bool {
        self.added + self.updated > 0
    }
}

/// What the watch last said in a `state` event, kept in `watch.json`
/// (keys `version`, `m`, `gps`, `run`, `heard`). All `None` until it has
/// said anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchState {
    /// The watch app's version.
    pub version: Option<String>,
    /// The watch's course, metres.
    pub m: Option<u32>,
    /// Whether the watch records GPS speed.
    pub gps: Option<bool>,
    /// The running test's log file name, while one runs.
    pub run: Option<String>,
    /// When the phone got that `state`, Unix seconds. (`crate::health`'s
    /// watch time is the last message of any kind.)
    pub heard: Option<i64>,
}

/// Which theme to show (`app.json`, key `theme`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Whatever the phone is set to.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    /// What `app.json` says: "system", "light", "dark".
    pub fn key(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// The theme `key` names; `None` for anything else.
    pub fn from_key(key: &str) -> Option<Theme> {
        Theme::ALL.into_iter().find(|t| t.key() == key)
    }
}

/// A file to share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    Tcx,
    Csv,
}

impl Export {
    /// "tcx", "csv".
    pub fn extension(self) -> &'static str {
        match self {
            Export::Tcx => "tcx",
            Export::Csv => "csv",
        }
    }

    /// The file's type for the share sheet, as `interface.html` saves it.
    pub fn mime(self) -> &'static str {
        match self {
            Export::Tcx => "application/vnd.garmin.tcx+xml",
            Export::Csv => "text/csv",
        }
    }
}

/// The store, opened on a directory. One per process on the phone
/// (`crate::platform::open`); tests open their own on temp directories.
/// `Sync`: the UI reads it while the engine writes.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    /// Held across each read-modify-write of `results.json`.
    lock: Mutex<()>,
}

impl Store {
    /// Opens the store in `dir`, making `dir`, `logs/`, `health/` and
    /// `exports/` if they aren't there. Reads nothing yet: a missing or
    /// unreadable `results.json` reads as empty.
    pub fn open(dir: &Path) -> io::Result<Store> {
        for sub in ["logs", "health", "exports"] {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        Ok(Store {
            dir: dir.to_path_buf(),
            lock: Mutex::new(()),
        })
    }

    /// The directory the store is in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every result, newest first (by `t`, then by `f`), each with its log
    /// state.
    pub fn results(&self) -> Vec<TestResult> {
        todo!()
    }

    /// The result whose log is `f`.
    pub fn result(&self, f: &str) -> Option<TestResult> {
        todo!()
    }

    /// The best result on each course, by course (metres) to its `f`: the
    /// furthest distance (`d`), the earlier one on a tie.
    pub fn bests(&self) -> BTreeMap<u32, String> {
        todo!()
    }

    /// Every tombstone, by log file name.
    pub fn tombstones(&self) -> BTreeMap<String, Tombstone> {
        todo!()
    }

    /// The pending tombstones' names, for `beeptest-sync`'s `del`, oldest
    /// deletion first.
    pub fn pending_deletes(&self) -> Vec<String> {
        todo!()
    }

    /// The watch's history as last fetched whole.
    pub fn watch_history(&self) -> WatchHistory {
        todo!()
    }

    /// Takes in the watch's history, fetched whole, as `hist`: `entries`
    /// in the watch's order ([`crate::link::parse_history`]). New results
    /// are added and known ones updated, by `f`; results the watch no
    /// longer lists are kept; none with a tombstone is added back; pending
    /// tombstones the watch no longer lists become done; and `hist` with
    /// the names listed becomes [`Store::watch_history`]. One write.
    pub fn merge_history(&self, entries: &[Entry], hist: Hist) -> io::Result<Merged> {
        todo!()
    }

    /// Deletes result `f` on the phone (the user, after confirming): the
    /// result and its log (complete or partial) go, and a pending tombstone
    /// stays (a done one stays done). False if there was no such result.
    pub fn delete(&self, f: &str) -> io::Result<bool> {
        todo!()
    }

    /// The watch says `names` were deleted there: each result and its log
    /// go, with a done tombstone. How many results there were to delete.
    pub fn deleted_on_watch(&self, names: &[String]) -> io::Result<usize> {
        todo!()
    }

    /// How much of `f`'s log is here.
    pub fn log_state(&self, f: &str) -> LogState {
        todo!()
    }

    /// Where `f`'s complete log is (`logs/<f>`), whether it's there or not.
    pub fn log_path(&self, f: &str) -> PathBuf {
        todo!()
    }

    /// The size of `f`'s partial log (`logs/<f>.part`), 0 with none: where
    /// the next `beeptest-get` starts.
    pub fn part_len(&self, f: &str) -> u64 {
        todo!()
    }

    /// Appends a piece to `f`'s partial log, making it if need be. Returns
    /// the partial's new size.
    pub fn append_part(&self, f: &str, bytes: &[u8]) -> io::Result<u64> {
        todo!()
    }

    /// Throws `f`'s partial log away (the watch's file is shorter than it:
    /// start again from 0).
    pub fn drop_part(&self, f: &str) -> io::Result<()> {
        todo!()
    }

    /// `f`'s partial log is all of it: renames `logs/<f>.part` to
    /// `logs/<f>`.
    pub fn finish_log(&self, f: &str) -> io::Result<()> {
        todo!()
    }

    /// The watch hasn't got `f`'s log: marks the result [`LogState::NoLog`]
    /// so it isn't asked for again, and drops any partial.
    pub fn set_no_log(&self, f: &str) -> io::Result<()> {
        todo!()
    }

    /// `f`'s complete log, read ([`crate::beeplog::parse`]); `None` until
    /// it's all here.
    pub fn log(&self, f: &str) -> Option<beeplog::Log> {
        todo!()
    }

    /// Writes `f`'s log as TCX (leaving out heart rate readings below
    /// `min_confidence`) or CSV to `exports/<file base>.<ext>`
    /// ([`crate::beeplog::file_base`]), for the share sheet, and returns
    /// where. An error to show when the log isn't all here yet or has no
    /// samples.
    pub fn export(&self, f: &str, kind: Export, min_confidence: f64) -> Result<PathBuf, String> {
        todo!()
    }

    /// What the watch last said.
    pub fn watch_state(&self) -> WatchState {
        todo!()
    }

    /// Saves what the watch said, leaving `watch.json`'s other keys (the
    /// direct link's) as they are.
    pub fn save_watch_state(&self, state: &WatchState) -> io::Result<()> {
        todo!()
    }

    /// The theme chosen in Settings.
    pub fn theme(&self) -> Theme {
        todo!()
    }

    pub fn set_theme(&self, theme: Theme) -> io::Result<()> {
        todo!()
    }
}

/// Writes `bytes` to `path` atomically: to `<path>.tmp` first, then renamed
/// over it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    todo!()
}

/// Changes some keys of the JSON object in `path`: reads it (an empty
/// object if it's missing or isn't one), lets `change` edit it, and writes
/// it back atomically. One process-wide lock is held throughout, so two
/// writers of the same file (`watch.json`) can't lose each other's keys.
pub fn update_json(
    path: &Path,
    change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) -> io::Result<()> {
    todo!()
}
