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
//!  "tombstones":[{"f":"beeptest.1764248400.csv","state":"pending"}],
//!  "hist":"1234:3735928559","listed":["beeptest.1764334800.csv"]}
//! ```
//!
//! with `"nolog":true` on a result whose log the watch said it hasn't got.
//! The tombstones are a list, oldest deletion first, so `beeptest-sync`
//! names them in that order. An entry that doesn't read (a hand edit) is
//! left out; a `results.json` that doesn't read at all reads as empty, and
//! is kept as `results.json.broken` before anything is written over it.
//!
//! Every write is atomic ([`write_atomic`]: a temp file, then a rename), so
//! a reader (the UI, on its own thread) sees the old file or the new one,
//! and the app being killed mid-write loses nothing. The sync engine
//! (`crate::sync`) is the only writer of `results.json`, the logs and
//! [`WatchState`], and runs one input at a time (`crate::platform::dispatch`);
//! the UI writes `app.json` and the direct link's setting. `watch.json` has
//! two writers, so its writes are read-modify-write under one lock
//! ([`update_json`]). A partial log is appended to in place: what's there
//! is always the start of the watch's file (each piece is checked before
//! it's appended), so a piece cut short by the app being killed is only
//! fetched again.
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
//!
//! Every file name that comes from the watch is checked
//! ([`crate::link::is_log_name`]) before it's made into a path: anything
//! else is no log, and nothing is written for it.

use crate::beeplog;
use crate::link::{self, Hist};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::{self, Write as _};
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
        format!("{}.{}", self.l, self.s)
    }

    /// Shuttles completed in all (`crate::tables::total_shuttles`): what
    /// the progress chart plots.
    pub fn total_shuttles(&self) -> u32 {
        crate::tables::total_shuttles(self.m, self.l, self.s)
    }

    fn to_json(&self, nolog: bool) -> Value {
        let mut v = json!({
            "f": self.f, "t": self.t, "m": self.m, "l": self.l, "s": self.s,
            "d": self.d, "dur": self.dur, "max": self.max, "avg": self.avg,
        });
        if nolog {
            v["nolog"] = true.into();
        }
        v
    }

    fn from_json(v: &Value) -> Option<Entry> {
        let small = |key: &str| v[key].as_u64().and_then(|n| u32::try_from(n).ok());
        Some(Entry {
            f: v["f"]
                .as_str()
                .filter(|f| link::is_log_name(f))?
                .to_string(),
            t: v["t"].as_i64()?,
            m: small("m")?,
            l: small("l")?,
            s: small("s")?,
            d: small("d")?,
            dur: small("dur")?,
            max: small("max")?,
            avg: small("avg")?,
        })
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

impl Tombstone {
    fn key(self) -> &'static str {
        match self {
            Tombstone::Pending => "pending",
            Tombstone::Done => "done",
        }
    }
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

/// `results.json`, read.
#[derive(Debug, Default)]
struct Data {
    /// Each result and whether the watch said it has no log, in no order.
    results: Vec<(Entry, bool)>,
    /// Oldest deletion first.
    tombstones: Vec<(String, Tombstone)>,
    hist: Hist,
    listed: Vec<String>,
}

impl Data {
    fn from_json(v: &Value) -> Data {
        let list = |key: &str| v[key].as_array().map(Vec::as_slice).unwrap_or_default();
        let mut data = Data::default();
        for r in list("results") {
            match Entry::from_json(r) {
                Some(e) if data.find(&e.f).is_none() => {
                    let nolog = r["nolog"].as_bool().unwrap_or(false);
                    data.results.push((e, nolog));
                }
                _ => log::warn!("results.json: left out {r}"),
            }
        }
        for t in list("tombstones") {
            let state = match t["state"].as_str() {
                Some("pending") => Tombstone::Pending,
                Some("done") => Tombstone::Done,
                _ => continue,
            };
            if let Some(f) = t["f"].as_str().filter(|f| link::is_log_name(f))
                && data.tombstone(f).is_none()
            {
                data.tombstones.push((f.to_string(), state));
            }
        }
        data.hist = v["hist"].as_str().and_then(Hist::parse).unwrap_or_default();
        data.listed = list("listed")
            .iter()
            .filter_map(Value::as_str)
            .filter(|f| link::is_log_name(f))
            .map(str::to_string)
            .collect();
        data
    }

    fn to_json(&self) -> Value {
        json!({
            "results": self.results.iter().map(|(e, nolog)| e.to_json(*nolog)).collect::<Vec<_>>(),
            "tombstones": self.tombstones.iter()
                .map(|(f, t)| json!({"f": f, "state": t.key()}))
                .collect::<Vec<_>>(),
            "hist": self.hist.to_string(),
            "listed": self.listed,
        })
    }

    fn find(&self, f: &str) -> Option<usize> {
        self.results.iter().position(|(e, _)| e.f == f)
    }

    fn tombstone(&self, f: &str) -> Option<Tombstone> {
        self.tombstones
            .iter()
            .find(|(t, _)| t == f)
            .map(|(_, s)| *s)
    }

    /// Pending tombstones the watch's history doesn't list (`listed`)
    /// become done: the watch has been seen without them. How many.
    fn settle(&mut self, listed: &[String]) -> usize {
        let mut settled = 0;
        for (f, t) in &mut self.tombstones {
            if *t == Tombstone::Pending && !listed.contains(f) {
                *t = Tombstone::Done;
                settled += 1;
            }
        }
        settled
    }

    /// Marks `f` deleted as `state`: a done tombstone stays done.
    fn set_tombstone(&mut self, f: &str, state: Tombstone) {
        match self.tombstones.iter_mut().find(|(t, _)| t == f) {
            Some((_, s)) => {
                if *s == Tombstone::Pending {
                    *s = state;
                }
            }
            None => self.tombstones.push((f.to_string(), state)),
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

    fn results_path(&self) -> PathBuf {
        self.dir.join("results.json")
    }

    fn watch_path(&self) -> PathBuf {
        self.dir.join("watch.json")
    }

    fn app_path(&self) -> PathBuf {
        self.dir.join("app.json")
    }

    /// `results.json` as it is, and the bytes it was read from (`None`
    /// when there's no file; an `Err` when there's one that doesn't read).
    fn read(&self) -> (Data, Result<Option<Vec<u8>>, ()>) {
        match std::fs::read(self.results_path()) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(v) if v.is_object() => (Data::from_json(&v), Ok(Some(bytes))),
                _ => {
                    log::error!("results.json doesn't read: taken as empty");
                    (Data::default(), Err(()))
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => (Data::default(), Ok(None)),
            Err(e) => {
                log::error!("results.json: {e}");
                (Data::default(), Err(()))
            }
        }
    }

    fn data(&self) -> Data {
        self.read().0
    }

    /// Reads `results.json`, lets `change` change it, and writes it back
    /// if it changed, all under the store's lock.
    fn change<R>(&self, change: impl FnOnce(&mut Data) -> R) -> io::Result<R> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let (mut data, was) = self.read();
        let out = change(&mut data);
        let bytes = data.to_json().to_string().into_bytes();
        match was {
            Ok(Some(old)) if old == bytes => return Ok(out),
            Err(()) => {
                // Keep what didn't read, once, rather than lose it.
                let broken = self.dir.join("results.json.broken");
                if !broken.exists() {
                    let _ = std::fs::copy(self.results_path(), broken);
                }
            }
            _ => {}
        }
        write_atomic(&self.results_path(), &bytes)?;
        Ok(out)
    }

    /// What's in `logs/`: each log's state by its result's name (complete
    /// files, and the partials' sizes).
    fn logs_here(&self) -> BTreeMap<String, LogState> {
        let mut out = BTreeMap::new();
        let Ok(dir) = std::fs::read_dir(self.dir.join("logs")) else {
            return out;
        };
        for entry in dir.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if let Some(f) = name.strip_suffix(".part") {
                let len = entry.metadata().map_or(0, |m| m.len());
                // A complete log wins over a partial left beside it.
                out.entry(f.to_string()).or_insert(LogState::Partial(len));
            } else {
                out.insert(name, LogState::Complete);
            }
        }
        out
    }

    fn state_of(logs: &BTreeMap<String, LogState>, f: &str, nolog: bool) -> LogState {
        match logs.get(f) {
            Some(LogState::Complete) => LogState::Complete,
            _ if nolog => LogState::NoLog,
            Some(s) => *s,
            None => LogState::Missing,
        }
    }

    /// Every result, newest first (by `t`, then by `f`), each with its log
    /// state.
    pub fn results(&self) -> Vec<TestResult> {
        let data = self.data();
        let logs = self.logs_here();
        let mut out: Vec<TestResult> = data
            .results
            .into_iter()
            .map(|(entry, nolog)| {
                let log = Self::state_of(&logs, &entry.f, nolog);
                TestResult { entry, log }
            })
            .collect();
        out.sort_by(|a, b| (b.entry.t, &b.entry.f).cmp(&(a.entry.t, &a.entry.f)));
        out
    }

    /// The result whose log is `f`.
    pub fn result(&self, f: &str) -> Option<TestResult> {
        let data = self.data();
        let (entry, nolog) = data.results.into_iter().find(|(e, _)| e.f == f)?;
        let log = self.log_state_with(f, nolog);
        Some(TestResult { entry, log })
    }

    /// The best result on each course, by course (metres) to its `f`: the
    /// furthest distance (`d`), the earlier one on a tie.
    pub fn bests(&self) -> BTreeMap<u32, String> {
        let mut best: BTreeMap<u32, Entry> = BTreeMap::new();
        for (e, _) in self.data().results {
            let better = best.get(&e.m).is_none_or(|b| {
                (e.d, std::cmp::Reverse((e.t, &e.f))) > (b.d, std::cmp::Reverse((b.t, &b.f)))
            });
            if better {
                best.insert(e.m, e);
            }
        }
        best.into_iter().map(|(m, e)| (m, e.f)).collect()
    }

    /// Every tombstone, by log file name.
    pub fn tombstones(&self) -> BTreeMap<String, Tombstone> {
        self.data().tombstones.into_iter().collect()
    }

    /// The pending tombstones' names, for `beeptest-sync`'s `del`, oldest
    /// deletion first.
    pub fn pending_deletes(&self) -> Vec<String> {
        self.data()
            .tombstones
            .into_iter()
            .filter(|(_, t)| *t == Tombstone::Pending)
            .map(|(f, _)| f)
            .collect()
    }

    /// The watch's history as last fetched whole.
    pub fn watch_history(&self) -> WatchHistory {
        let data = self.data();
        WatchHistory {
            hist: data.hist,
            listed: data.listed,
        }
    }

    /// Takes in the watch's history, fetched whole, as `hist`: `entries`
    /// in the watch's order ([`crate::link::parse_history`]). New results
    /// are added and known ones updated, by `f`; results the watch no
    /// longer lists are kept; none with a tombstone is added back; pending
    /// tombstones the watch no longer lists become done; and `hist` with
    /// the names listed becomes [`Store::watch_history`]. One write.
    pub fn merge_history(&self, entries: &[Entry], hist: Hist) -> io::Result<Merged> {
        self.change(|data| {
            let mut merged = Merged::default();
            let mut listed: Vec<String> = Vec::new();
            for e in entries {
                if !link::is_log_name(&e.f) || listed.contains(&e.f) {
                    continue;
                }
                listed.push(e.f.clone());
                if data.tombstone(&e.f).is_some() {
                    continue;
                }
                match data.find(&e.f) {
                    Some(i) if data.results[i].0 != *e => {
                        data.results[i].0 = e.clone();
                        merged.updated += 1;
                    }
                    Some(_) => {}
                    None => {
                        data.results.push((e.clone(), false));
                        merged.added += 1;
                    }
                }
            }
            merged.settled = data.settle(&listed);
            data.hist = hist;
            data.listed = listed;
            merged
        })
    }

    /// The watch's history is still the one last fetched whole (its `hist`
    /// is the same): pending tombstones it doesn't list become done, as
    /// [`Store::merge_history`] would make them. That's a delete on the
    /// phone of a result the watch had already forgotten, which leaves
    /// its history as it was. How many.
    pub fn settle_unlisted(&self) -> io::Result<usize> {
        self.change(|data| {
            let listed = std::mem::take(&mut data.listed);
            let settled = data.settle(&listed);
            data.listed = listed;
            settled
        })
    }

    /// Deletes result `f` on the phone (the user, after confirming): the
    /// result and its log (complete or partial) go, and a pending tombstone
    /// stays (a done one stays done). False if there was no such result.
    pub fn delete(&self, f: &str) -> io::Result<bool> {
        if !link::is_log_name(f) {
            return Ok(false);
        }
        let had = self.change(|data| {
            let Some(i) = data.find(f) else {
                return false;
            };
            data.results.remove(i);
            data.set_tombstone(f, Tombstone::Pending);
            true
        })?;
        if had {
            self.remove_logs(f)?;
        }
        Ok(had)
    }

    /// The watch says `names` were deleted there: each result and its log
    /// go, with a done tombstone. How many results there were to delete.
    pub fn deleted_on_watch(&self, names: &[String]) -> io::Result<usize> {
        let names: Vec<&String> = names.iter().filter(|f| link::is_log_name(f)).collect();
        if names.is_empty() {
            return Ok(0);
        }
        let gone = self.change(|data| {
            let mut gone = Vec::new();
            for f in &names {
                if let Some(i) = data.find(f) {
                    data.results.remove(i);
                    gone.push(f.to_string());
                }
                // Done, so a history that still lists it (one fetched
                // before the delete, say) can't bring it back.
                match data.tombstones.iter_mut().find(|(t, _)| t == *f) {
                    Some((_, s)) => *s = Tombstone::Done,
                    None => data.tombstones.push((f.to_string(), Tombstone::Done)),
                }
            }
            gone
        })?;
        for f in &gone {
            self.remove_logs(f)?;
        }
        Ok(gone.len())
    }

    fn remove_logs(&self, f: &str) -> io::Result<()> {
        remove_if_there(&self.log_path(f))?;
        remove_if_there(&self.part_path(f))
    }

    /// How much of `f`'s log is here.
    pub fn log_state(&self, f: &str) -> LogState {
        let nolog = self
            .data()
            .results
            .iter()
            .any(|(e, nolog)| e.f == f && *nolog);
        self.log_state_with(f, nolog)
    }

    fn log_state_with(&self, f: &str, nolog: bool) -> LogState {
        if !link::is_log_name(f) {
            return LogState::Missing;
        }
        if self.log_path(f).is_file() {
            LogState::Complete
        } else if nolog {
            LogState::NoLog
        } else {
            match std::fs::metadata(self.part_path(f)) {
                Ok(m) => LogState::Partial(m.len()),
                Err(_) => LogState::Missing,
            }
        }
    }

    /// Where `f`'s complete log is (`logs/<f>`), whether it's there or not.
    /// For a name that isn't a log's, `logs/not-a-log`, which no log is.
    pub fn log_path(&self, f: &str) -> PathBuf {
        let name = if link::is_log_name(f) { f } else { "not-a-log" };
        self.dir.join("logs").join(name)
    }

    fn part_path(&self, f: &str) -> PathBuf {
        let mut path = self.log_path(f).into_os_string();
        path.push(".part");
        PathBuf::from(path)
    }

    /// The size of `f`'s partial log (`logs/<f>.part`), 0 with none: where
    /// the next `beeptest-get` starts.
    pub fn part_len(&self, f: &str) -> u64 {
        if !link::is_log_name(f) {
            return 0;
        }
        std::fs::metadata(self.part_path(f)).map_or(0, |m| m.len())
    }

    /// Appends a piece to `f`'s partial log, making it if need be. Returns
    /// the partial's new size. An error for a name that isn't a log's.
    pub fn append_part(&self, f: &str, bytes: &[u8]) -> io::Result<u64> {
        check_name(f)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.part_path(f))?;
        file.write_all(bytes)?;
        Ok(file.metadata()?.len())
    }

    /// Throws `f`'s partial log away (the watch's file is shorter than it:
    /// start again from 0).
    pub fn drop_part(&self, f: &str) -> io::Result<()> {
        check_name(f)?;
        remove_if_there(&self.part_path(f))
    }

    /// `f`'s partial log is all of it: renames `logs/<f>.part` to
    /// `logs/<f>` (an empty log when there's no partial: the watch's file
    /// was empty).
    pub fn finish_log(&self, f: &str) -> io::Result<()> {
        check_name(f)?;
        let part = self.part_path(f);
        if !part.exists() {
            std::fs::File::create(&part)?;
        }
        std::fs::File::open(&part)?.sync_all()?;
        std::fs::rename(part, self.log_path(f))?;
        // Here after all, so not "no log" any more.
        self.change(|data| {
            if let Some(i) = data.find(f) {
                data.results[i].1 = false;
            }
        })
    }

    /// The watch hasn't got `f`'s log: marks the result [`LogState::NoLog`]
    /// so it isn't asked for again, and drops any partial.
    pub fn set_no_log(&self, f: &str) -> io::Result<()> {
        check_name(f)?;
        self.change(|data| {
            if let Some(i) = data.find(f) {
                data.results[i].1 = true;
            }
        })?;
        self.drop_part(f)
    }

    /// `f`'s complete log, read ([`crate::beeplog::parse`]); `None` until
    /// it's all here.
    pub fn log(&self, f: &str) -> Option<beeplog::Log> {
        if !link::is_log_name(f) {
            return None;
        }
        let bytes = std::fs::read(self.log_path(f)).ok()?;
        // Each byte a char, as the watch's strings are (the logs are ASCII).
        let text: String = bytes.iter().map(|&b| char::from(b)).collect();
        Some(beeplog::parse(&text))
    }

    /// Writes `f`'s log as TCX (leaving out heart rate readings below
    /// `min_confidence`) or CSV to `exports/<file base>.<ext>`
    /// ([`crate::beeplog::file_base`]), for the share sheet, and returns
    /// where. An error to show when the log isn't all here yet or has no
    /// samples.
    pub fn export(&self, f: &str, kind: Export, min_confidence: f64) -> Result<PathBuf, String> {
        let log = self
            .log(f)
            .ok_or_else(|| "The log hasn't come from the watch yet".to_string())?;
        let text = match kind {
            Export::Tcx => beeplog::to_tcx(&log, min_confidence)?,
            Export::Csv => beeplog::to_csv(&log)?,
        };
        let path = self.dir.join("exports").join(format!(
            "{}.{}",
            beeplog::file_base(&log)?,
            kind.extension()
        ));
        write_atomic(&path, text.as_bytes())
            .map_err(|e| format!("Couldn't write the file: {e}"))?;
        Ok(path)
    }

    /// What the watch last said.
    pub fn watch_state(&self) -> WatchState {
        let v = read_object(&self.watch_path());
        WatchState {
            version: v.get("version").and_then(Value::as_str).map(str::to_string),
            m: v.get("m")
                .and_then(Value::as_u64)
                .and_then(|m| u32::try_from(m).ok()),
            gps: v.get("gps").and_then(Value::as_bool),
            run: v
                .get("run")
                .and_then(Value::as_str)
                .filter(|f| link::is_log_name(f))
                .map(str::to_string),
            heard: v.get("heard").and_then(Value::as_i64),
        }
    }

    /// Saves what the watch said, leaving `watch.json`'s other keys (the
    /// direct link's) as they are.
    pub fn save_watch_state(&self, state: &WatchState) -> io::Result<()> {
        update_json(&self.watch_path(), |o| {
            let mut put = |key: &str, value: Option<Value>| match value {
                Some(v) => {
                    o.insert(key.into(), v);
                }
                None => {
                    o.remove(key);
                }
            };
            put("version", state.version.clone().map(Value::from));
            put("m", state.m.map(Value::from));
            put("gps", state.gps.map(Value::from));
            put("run", state.run.clone().map(Value::from));
            put("heard", state.heard.map(Value::from));
        })
    }

    /// The theme chosen in Settings.
    pub fn theme(&self) -> Theme {
        read_object(&self.app_path())
            .get("theme")
            .and_then(Value::as_str)
            .and_then(Theme::from_key)
            .unwrap_or_default()
    }

    pub fn set_theme(&self, theme: Theme) -> io::Result<()> {
        update_json(&self.app_path(), |o| {
            o.insert("theme".into(), theme.key().into());
        })
    }
}

fn check_name(f: &str) -> io::Result<()> {
    if link::is_log_name(f) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a log's name: {f:?}"),
        ))
    }
}

fn remove_if_there(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The JSON object in `path`; empty if it's missing or isn't one.
fn read_object(path: &Path) -> Map<String, Value> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| match v {
            Value::Object(o) => Some(o),
            _ => None,
        })
        .unwrap_or_default()
}

/// Writes `bytes` to `path` atomically: to `<path>.tmp` first (flushed to
/// the disk), then renamed over it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

static JSON_LOCK: Mutex<()> = Mutex::new(());

/// Changes some keys of the JSON object in `path`: reads it (an empty
/// object if it's missing or isn't one), lets `change` edit it, and writes
/// it back atomically. One process-wide lock is held throughout, so two
/// writers of the same file (`watch.json`) can't lose each other's keys.
pub fn update_json(path: &Path, change: impl FnOnce(&mut Map<String, Value>)) -> io::Result<()> {
    let _guard = JSON_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut object = read_object(path);
    change(&mut object);
    write_atomic(path, Value::Object(object).to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(f: &str, t: i64, m: u32, d: u32) -> Entry {
        Entry {
            f: f.into(),
            t,
            m,
            l: 3,
            s: 2,
            d,
            dur: 200,
            max: 170,
            avg: 150,
        }
    }

    const A: &str = "beeptest.1781344800.csv";
    const B: &str = "beeptest.1781258400.csv";
    const C: &str = "beeptest.1781172000.csv";

    fn names(store: &Store) -> Vec<String> {
        store.results().into_iter().map(|r| r.entry.f).collect()
    }

    #[test]
    fn merges_the_watchs_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.results(), vec![]);
        assert_eq!(store.watch_history(), WatchHistory::default());
        let hist = Hist { len: 10, crc: 99 };
        let m = store
            .merge_history(
                &[
                    entry(A, 3000, 20, 500),
                    entry(B, 2000, 15, 300),
                    entry(C, 1000, 20, 900),
                ],
                hist,
            )
            .unwrap();
        assert_eq!(
            m,
            Merged {
                added: 3,
                updated: 0,
                settled: 0
            }
        );
        assert!(m.changed());
        assert_eq!(names(&store), vec![A, B, C]);
        assert_eq!(store.watch_history().listed, vec![A, B, C]);
        assert_eq!(store.watch_history().hist, hist);
        assert_eq!(store.result(A).unwrap().log, LogState::Missing);

        // The watch dropped C (it keeps 30) and its clock moved A's t: C is
        // kept, A updated.
        let m = store
            .merge_history(&[entry(A, 3500, 20, 500), entry(B, 2000, 15, 300)], hist)
            .unwrap();
        assert_eq!((m.added, m.updated), (0, 1));
        assert_eq!(names(&store), vec![A, B, C]);
        assert_eq!(store.result(A).unwrap().entry.t, 3500);
        assert_eq!(store.watch_history().listed, vec![A, B]);
        // The same again changes nothing.
        assert!(
            !store
                .merge_history(&[entry(A, 3500, 20, 500)], hist)
                .unwrap()
                .changed()
        );

        // Bests: the furthest on each course, the earlier on a tie.
        store
            .merge_history(&[entry("beeptest.5.csv", 5000, 20, 900)], hist)
            .unwrap();
        assert_eq!(
            store.bests(),
            BTreeMap::from([(15, B.to_string()), (20, C.to_string())])
        );
        // A new store on the same directory reads the same.
        let again = Store::open(dir.path()).unwrap();
        assert_eq!(again.results(), store.results());
        assert_eq!(again.watch_history(), store.watch_history());
    }

    #[test]
    fn tombstones_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let h = Hist::default();
        let all = [entry(A, 3, 20, 1), entry(B, 2, 20, 1), entry(C, 1, 20, 1)];
        store.merge_history(&all, h).unwrap();
        store.append_part(A, b"start").unwrap();
        store.append_part(B, b"whole").unwrap();
        store.finish_log(B).unwrap();

        // Deleted on the phone: gone with its log, pending until the watch
        // is seen without it, and a history listing it doesn't bring it back.
        assert!(store.delete(A).unwrap());
        assert!(!store.delete(A).unwrap());
        assert!(!store.delete("../../etc/passwd").unwrap());
        assert_eq!(store.part_len(A), 0);
        assert_eq!(store.pending_deletes(), vec![A]);
        store.merge_history(&all, h).unwrap();
        assert_eq!(names(&store), vec![B, C]);
        assert_eq!(store.pending_deletes(), vec![A]);
        assert!(store.delete(C).unwrap());
        assert_eq!(store.pending_deletes(), vec![A, C]);
        // The watch no longer lists A: done. C still listed: pending.
        let m = store.merge_history(&all[1..], h).unwrap();
        assert_eq!(m.settled, 1);
        assert_eq!(store.pending_deletes(), vec![C]);
        assert_eq!(store.tombstones()[A], Tombstone::Done);
        // A watch restored from a backup lists A again: it stays deleted.
        store.merge_history(&all, h).unwrap();
        assert_eq!(names(&store), vec![B]);
        assert_eq!(store.tombstones()[A], Tombstone::Done);

        // Deleted on the watch: gone here with its log, done.
        assert_eq!(store.log_state(B), LogState::Complete);
        assert_eq!(
            store
                .deleted_on_watch(&[B.into(), "beeptest.77.csv".into(), "../x".into()])
                .unwrap(),
            1
        );
        assert_eq!(names(&store), Vec::<String>::new());
        assert!(!store.log_path(B).exists());
        assert_eq!(store.tombstones()[B], Tombstone::Done);
        assert_eq!(store.tombstones()["beeptest.77.csv"], Tombstone::Done);
        assert!(!store.tombstones().contains_key("../x"));
        // The watch deleting one the phone deleted first: done.
        store.deleted_on_watch(&[C.into()]).unwrap();
        assert_eq!(store.pending_deletes(), Vec::<String>::new());
        // Nothing comes back.
        store.merge_history(&all, h).unwrap();
        assert!(store.results().is_empty());
    }

    #[test]
    fn logs_arrive_in_pieces() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .merge_history(&[entry(A, 1, 20, 1), entry(B, 2, 20, 1)], Hist::default())
            .unwrap();
        assert_eq!(store.part_len(A), 0);
        assert_eq!(
            store
                .append_part(A, b"start,1781344800000,-60,20\n")
                .unwrap(),
            27
        );
        assert_eq!(store.append_part(A, b"0.0,100,95,1,0,0.0\n").unwrap(), 46);
        assert_eq!(store.log_state(A), LogState::Partial(46));
        assert_eq!(store.result(A).unwrap().log, LogState::Partial(46));
        assert_eq!(store.log(A), None);
        assert!(store.export(A, Export::Csv, 0.0).is_err());
        store.finish_log(A).unwrap();
        assert_eq!(store.log_state(A), LogState::Complete);
        assert_eq!(store.part_len(A), 0);
        assert_eq!(store.log(A).unwrap().samples.len(), 1);

        // Exports, with beeplog's name.
        let csv = store.export(A, Export::Csv, 0.0).unwrap();
        assert_eq!(
            csv,
            dir.path().join("exports/beeptest-20m-2026-06-13-11-00.csv")
        );
        assert!(std::fs::read_to_string(&csv).unwrap().starts_with("time,"));
        let tcx = store.export(A, Export::Tcx, 50.0).unwrap();
        assert!(tcx.ends_with("beeptest-20m-2026-06-13-11-00.tcx"));

        // No log on the watch: marked, and the partial dropped.
        store.append_part(B, b"x").unwrap();
        store.set_no_log(B).unwrap();
        assert_eq!(store.log_state(B), LogState::NoLog);
        assert_eq!(store.part_len(B), 0);
        assert_eq!(
            store.results().iter().map(|r| r.log).collect::<Vec<_>>(),
            vec![LogState::NoLog, LogState::Complete]
        );
        // A partial dropped starts again from 0.
        store.append_part(C, b"abc").unwrap();
        store.drop_part(C).unwrap();
        assert_eq!(store.part_len(C), 0);
        // An empty log is a log.
        store.finish_log(C).unwrap();
        assert_eq!(store.log_state(C), LogState::Complete);
    }

    #[test]
    fn names_never_leave_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("data")).unwrap();
        for bad in ["../x", "../../x.csv", "/tmp/x", "beeptest.json", "", ".."] {
            assert!(store.append_part(bad, b"x").is_err(), "{bad}");
            assert!(store.finish_log(bad).is_err(), "{bad}");
            assert!(store.set_no_log(bad).is_err(), "{bad}");
            assert!(store.drop_part(bad).is_err(), "{bad}");
            assert_eq!(store.part_len(bad), 0);
            assert_eq!(store.log_state(bad), LogState::Missing);
            assert!(
                store
                    .log_path(bad)
                    .starts_with(dir.path().join("data/logs"))
            );
            assert_eq!(store.log(bad), None);
            assert!(store.export(bad, Export::Tcx, 0.0).is_err());
        }
        // A history with a bad name is left alone.
        store
            .merge_history(&[entry("../x", 1, 20, 1)], Hist::default())
            .unwrap();
        assert!(store.results().is_empty());
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "only data/"
        );
    }

    #[test]
    fn a_broken_results_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        std::fs::write(dir.path().join("results.json"), "{not json").unwrap();
        assert!(store.results().is_empty());
        store
            .merge_history(&[entry(A, 1, 20, 1)], Hist::default())
            .unwrap();
        assert_eq!(names(&store), vec![A]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("results.json.broken")).unwrap(),
            "{not json"
        );
        // An entry that doesn't read is left out; the rest stay.
        std::fs::write(
            dir.path().join("results.json"),
            r#"{"results":[{"f":"x"},{"f":"beeptest.1.csv","t":1,"m":20,"l":1,"s":1,"d":20,"dur":9,"max":0,"avg":0,"nolog":true}],"tombstones":[{"f":"beeptest.2.csv","state":"pending"},{"f":"y","state":"done"},{"f":"beeptest.3.csv","state":"maybe"}],"hist":"5:6","listed":["beeptest.1.csv",7,"../z"]}"#,
        )
        .unwrap();
        assert_eq!(names(&store), vec!["beeptest.1.csv"]);
        assert_eq!(store.log_state("beeptest.1.csv"), LogState::NoLog);
        assert_eq!(store.pending_deletes(), vec!["beeptest.2.csv"]);
        assert_eq!(store.tombstones().len(), 1);
        assert_eq!(
            store.watch_history(),
            WatchHistory {
                hist: Hist { len: 5, crc: 6 },
                listed: vec!["beeptest.1.csv".into()]
            }
        );
    }

    #[test]
    fn watch_state_and_theme_keep_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.watch_state(), WatchState::default());
        assert_eq!(store.theme(), Theme::System);
        update_json(&dir.path().join("watch.json"), |o| {
            o.insert("direct".into(), true.into());
        })
        .unwrap();
        let s = WatchState {
            version: Some("0.05".into()),
            m: Some(15),
            gps: Some(true),
            run: Some(A.into()),
            heard: Some(1_781_344_800),
        };
        store.save_watch_state(&s).unwrap();
        assert_eq!(store.watch_state(), s);
        let none = WatchState {
            run: None,
            ..s.clone()
        };
        store.save_watch_state(&none).unwrap();
        assert_eq!(store.watch_state(), none);
        let file = read_object(&dir.path().join("watch.json"));
        assert_eq!(file["direct"], true);
        assert!(!file.contains_key("run"));

        store.set_theme(Theme::Dark).unwrap();
        assert_eq!(store.theme(), Theme::Dark);
        std::fs::write(dir.path().join("app.json"), r#"{"theme":"purple"}"#).unwrap();
        assert_eq!(store.theme(), Theme::System);
        assert_eq!(Theme::from_key("light"), Some(Theme::Light));
        assert_eq!(Export::Tcx.mime(), "application/vnd.garmin.tcx+xml");
    }

    #[test]
    fn writes_are_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!dir.path().join("f.json.tmp").exists());
        // A write that can't happen leaves the file as it was.
        assert!(write_atomic(&dir.path().join("no/such/dir"), b"x").is_err());
        // update_json over something that isn't an object starts afresh.
        std::fs::write(&path, "[1]").unwrap();
        update_json(&path, |o| {
            o.insert("a".into(), 1.into());
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"a":1}"#);
        // Two writers of one file keep each other's keys.
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for n in 0..20 {
                        update_json(&path, |o| {
                            o.insert(format!("k{i}"), n.into());
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(read_object(&path).len(), 9);
    }

    #[test]
    fn scores() {
        let e = Entry {
            l: 9,
            s: 4,
            ..entry(A, 1, 20, 1)
        };
        assert_eq!(e.score_text(), "9.4");
        assert_eq!(e.total_shuttles(), crate::tables::total_shuttles(20, 9, 4));
    }
}
