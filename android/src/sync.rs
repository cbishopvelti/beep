//! The sync engine: keeps the phone and the watch in step (the spec's "What
//! the phone does with it", and `docs/DEVELOPMENT.md`, "The watch link").
//!
//! It is pure: [`Engine::handle`] takes the store, an [`Input`] and the time,
//! updates the store, and returns the lines to send the watch
//! ([`Output::lines`]) and whether what the pages show changed
//! ([`Output::changed`]); [`Engine::progress`] says what it's doing. No JNI,
//! no threads, no clock of its own, so the tests drive it with made-up times
//! and the watch's own lines (`tests/fixtures/watch-lines.txt`).
//!
//! **How inputs get here.** `crate::platform::dispatch` holds the process's
//! one [`Engine`] and store, runs each input through [`Engine::handle`]
//! under one lock (inputs come from the UI thread, Android's main thread for
//! broadcasts, and the Bluetooth callbacks), sends the lines it returns
//! (Gadgetbridge or the direct link), and tells the UI. A watch event
//! needs no UI: Gadgetbridge's broadcast starts the app's process for
//! `WatchReceiver`, which opens the store (`crate::platform::open`), checks
//! the extras (`crate::link::parse_event`) and dispatches
//! [`Input::Watch`]; the answer (the next `get`, say) goes out at once.
//! Only the 10 s waits need the app open: [`Input::Tick`] comes from the UI,
//! about once a second.
//!
//! **A round.** Sync starts with a `beeptest-sync` line ([`Input::Opened`],
//! [`Input::SyncNow`], [`Input::LinkConnected`], or after a delete) carrying
//! the pending tombstones; the watch answers `state` (and says it unasked
//! too). Each `state` starts a round, dropping whatever the last one was
//! fetching (the watch's next `get` replaces its transfer anyway):
//!
//! 1. Save the watch's version, settings and running file
//!    ([`crate::store::WatchState`], `heard` now), and delete every result
//!    named in `del` ([`Store::deleted_on_watch`]). While a test runs
//!    (`run` isn't `""`) that's all: the watch would answer any `get` with
//!    `busy`, and it says `state` again when the test ends.
//! 2. If `hist` is [`Store::watch_history`]'s, the history fetched last is
//!    still the watch's: pending tombstones it doesn't list are done
//!    ([`Store::settle_unlisted`]). If it differs, `get`
//!    `beeptest.json` from 0 (unless `hist` says it's past
//!    [`crate::link::MAX_HISTORY`]: then it isn't fetched). It is kept in
//!    memory as it arrives and taken in ([`Store::merge_history`]) only
//!    when whole: pieces contiguous from 0, each CRC right, `logend`'s
//!    `len` what arrived, and the whole's [`crate::link::Hist`] equal to
//!    `state`'s (`len` `-1` is whole only when `hist` is `0:0`).
//!    Otherwise it is asked for again (the same at most [`TRIES`] times as
//!    a log), then given up on for the round, and the round goes on with
//!    the history fetched before.
//! 3. The logs, one at a time: the first name in
//!    [`crate::store::WatchHistory::listed`] that has a result on the phone
//!    without a complete log ([`crate::store::LogState`] `Partial` or
//!    `Missing`), and hasn't been given up on this round. `get` it from
//!    [`Store::part_len`]. A good piece (right `f`, `at` equal to the
//!    partial's size, CRC right) is appended; any other is dropped and the
//!    transfer marked broken. One that would take the log past
//!    [`crate::link::MAX_LOG`] refuses the log: it's marked as having none
//!    ([`Store::set_no_log`]), so it isn't fetched again. On `logend`,
//!    `len` equal to the partial's size finishes it ([`Store::finish_log`]),
//!    whatever came before (everything appended was checked, so the
//!    partial is the start of the watch's file); `None` marks it
//!    ([`Store::set_no_log`], or gives it up for the round if that can't be
//!    written); a `len` past the cap gives it up for the round; a `len`
//!    shorter than the partial drops the partial; otherwise (broken, or
//!    short) it is asked for again from the partial's size, at most
//!    [`TRIES`] times a file a round, then given up on for the round. So is a transfer from which nothing good has
//!    arrived for [`STALL_MS`] ([`Input::Tick`]). Then the next.
//! 4. Nothing left: the round is done, and if nothing was given up on, the
//!    phone has synced ([`crate::health::Link::Synced`]).
//!
//! `busy` ends the round (a test is running; the watch says `state` when it
//! ends). Everything needed to carry on is on disk (the partials, the
//! tombstones, the history last fetched), so a round cut short by the app
//! being killed carries on at the next `state`. What is only in memory (the
//! history arriving, the tries) starts again.
//!
//! Each event taken counts as hearing from the watch
//! ([`crate::health::Link::Watch`], written at most once a second).

use crate::health;
use crate::link::{self, HISTORY, Hist, MAX_HISTORY, MAX_LOG, Piece, State, WatchEvent};
use crate::store::{LogState, Store, WatchState};
use std::collections::BTreeMap;

/// How long to wait for the watch's `state` after a `beeptest-sync` before
/// saying it isn't answering, ms.
pub const ANSWER_WAIT_MS: i64 = 10_000;
/// How long a transfer may go without a piece (while the app is open)
/// before it is asked for again, ms.
pub const STALL_MS: i64 = 10_000;
/// How many times a file is asked for again in a round before it is given
/// up on until the next.
pub const TRIES: u32 = 3;

/// What happened, for the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// The app opened or came back to the front: sync.
    Opened,
    /// The user pressed Sync now.
    SyncNow,
    /// The direct link is up (after its Ctrl-C and clock lines): sync.
    LinkConnected,
    /// An event from the watch, checked.
    Watch(WatchEvent),
    /// About a second has gone by with the app open: the waits
    /// ([`ANSWER_WAIT_MS`], [`STALL_MS`]) are checked. Usually nothing. A
    /// wait that started after `now` (the clock was set back) starts now.
    Tick,
    /// The user deleted result `f` (after confirming): it goes from the
    /// phone ([`Store::delete`]) and a `beeptest-sync` asks the watch to
    /// delete it too. If its log was being fetched, the round ends there
    /// (the watch's answer starts the next).
    Delete(String),
    /// The user changed the watch's course (20 or 15), GPS setting or speed
    /// progression (0, 1 or 2) on the Watch page: a `beeptest-set` line.
    /// The page shows the watch's own settings once its `state` answers. A
    /// course other than 20 or 15, or a progression the phone doesn't know,
    /// is left out.
    SetWatch {
        m: Option<u32>,
        gps: Option<bool>,
        p: Option<u32>,
    },
}

/// What [`Engine::handle`] says to do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    /// Lines for the watch, in order (`crate::link`'s, each ending `\n`).
    pub lines: Vec<String>,
    /// Whether the store changed in a way the pages show (results, logs,
    /// the watch's state): the UI reloads. [`Engine::progress`] may change
    /// without it.
    pub changed: bool,
}

/// What the sync is doing, for the Results and Watch pages.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Progress {
    /// Nothing asked since the process started.
    #[default]
    Idle,
    /// A `beeptest-sync` went at `since_ms`; waiting for `state`.
    Asking { since_ms: i64 },
    /// No `state` came within [`ANSWER_WAIT_MS`] of asking.
    NoAnswer,
    /// Fetching the watch's history: `got` bytes so far.
    History { got: u64 },
    /// Fetching log `n` of the `of` the round needs (counting from 1): `f`,
    /// with `got` bytes of it here.
    Logs {
        n: usize,
        of: usize,
        f: String,
        got: u64,
    },
    /// A test is running on the watch: its logs wait until it ends.
    Busy,
    /// The round finished at `at_ms`; `failed` names the logs given up on
    /// in it (empty when everything the watch lists is here), and
    /// `beeptest.json` if the history was.
    Done { at_ms: i64, failed: Vec<String> },
}

impl Progress {
    /// Whether a sync is under way (asking, or fetching).
    pub fn active(&self) -> bool {
        matches!(
            self,
            Progress::Asking { .. } | Progress::History { .. } | Progress::Logs { .. }
        )
    }

    /// What the pages say: "Asking the watch...", "The watch isn't
    /// answering", "Fetching the results...", "Fetching 2 of 3 logs..."
    /// ("Fetching the log..." when there's one), "A test is running on the
    /// watch", "Up to date", "Couldn't fetch 1 log" (or "Couldn't fetch the
    /// results" when the history was given up on), or "" for
    /// [`Progress::Idle`].
    pub fn words(&self) -> String {
        match self {
            Progress::Idle => String::new(),
            Progress::Asking { .. } => "Asking the watch...".into(),
            Progress::NoAnswer => "The watch isn't answering".into(),
            Progress::History { .. } => "Fetching the results...".into(),
            Progress::Logs { of: 1, .. } => "Fetching the log...".into(),
            Progress::Logs { n, of, .. } => format!("Fetching {n} of {of} logs..."),
            Progress::Busy => "A test is running on the watch".into(),
            Progress::Done { failed, .. } if failed.is_empty() => "Up to date".into(),
            Progress::Done { failed, .. } if failed.iter().any(|f| f == HISTORY) => {
                "Couldn't fetch the results".into()
            }
            Progress::Done { failed, .. } => format!(
                "Couldn't fetch {} log{}",
                failed.len(),
                if failed.len() == 1 { "" } else { "s" }
            ),
        }
    }
}

/// The engine's state between inputs: only what a round needs in memory.
#[derive(Debug, Default)]
pub struct Engine {
    progress: Progress,
    /// The file being fetched and how it's going.
    transfer: Option<Transfer>,
    /// The `state` the round started from; `None` between rounds.
    state: Option<State>,
    /// Times each file was asked for again this round.
    tries: BTreeMap<String, u32>,
    /// Files given up on this round.
    failed: Vec<String>,
    /// Logs finished this round, for "n of".
    fetched: usize,
    /// When the last `beeptest-sync` went, until a `state` answers it.
    asked_ms: Option<i64>,
    /// The second last written as hearing from the watch.
    heard_s: Option<i64>,
}

#[derive(Debug)]
struct Transfer {
    f: String,
    /// A bad or out-of-place piece came.
    broken: bool,
    /// When it was asked for, or a good piece last came, ms.
    last_ms: i64,
    /// The history so far, while `f` is `beeptest.json`.
    history: Vec<u8>,
}

impl Transfer {
    fn new(f: &str, now_ms: i64) -> Transfer {
        Transfer {
            f: f.to_string(),
            broken: false,
            last_ms: now_ms,
            history: Vec::new(),
        }
    }
}

/// Logs a store error; the round goes on as the caller decides.
fn warn(what: &str, f: &str, e: &std::io::Error) {
    log::warn!("sync: can't {what} {f}: {e}");
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Runs `input` at `now_ms` (Unix ms): updates `store` and says what to
    /// send. Store errors (a full disk) are logged and leave the round
    /// where it was; they never panic.
    pub fn handle(&mut self, store: &Store, input: Input, now_ms: i64) -> Output {
        let mut out = Output::default();
        match input {
            Input::Opened | Input::SyncNow | Input::LinkConnected => {
                self.ask(store, now_ms, &mut out)
            }
            Input::Delete(f) => {
                match store.delete(&f) {
                    Ok(true) => out.changed = true,
                    Ok(false) => {}
                    Err(e) => warn("delete", &f, &e),
                }
                if self.transfer.as_ref().is_some_and(|t| t.f == f) {
                    // The log being fetched: the round ends here, and the
                    // watch's answer to the sync starts the next.
                    self.transfer = None;
                    self.state = None;
                }
                self.ask(store, now_ms, &mut out);
            }
            Input::SetWatch { m, gps, p } => {
                let m = m.filter(|m| *m == 20 || *m == 15);
                let p = p.filter(|p| crate::tables::is_progression(*p));
                if m.is_some() || gps.is_some() || p.is_some() {
                    out.lines.push(link::set_line(m, gps, p));
                }
            }
            Input::Tick => self.tick(store, now_ms, &mut out),
            Input::Watch(event) => {
                self.heard(store, now_ms);
                match event {
                    WatchEvent::State(s) => self.on_state(store, s, now_ms, &mut out),
                    WatchEvent::Log(p) => self.on_piece(store, p, now_ms, &mut out),
                    WatchEvent::LogEnd { f, len } => self.on_end(store, &f, len, now_ms, &mut out),
                    WatchEvent::Busy { f } => {
                        if self.transfer.as_ref().is_some_and(|t| t.f == f) {
                            self.transfer = None;
                            self.state = None;
                            self.progress = Progress::Busy;
                        }
                    }
                }
            }
        }
        out
    }

    /// What the sync is doing.
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    /// `beeptest-sync`, with the pending tombstones. A fetch under way
    /// goes on showing; otherwise it's asking.
    fn ask(&mut self, store: &Store, now_ms: i64, out: &mut Output) {
        out.lines.push(link::sync_line(&store.pending_deletes()));
        self.asked_ms = Some(now_ms);
        if self.transfer.is_none()
            || !matches!(
                self.progress,
                Progress::History { .. } | Progress::Logs { .. }
            )
        {
            self.progress = Progress::Asking { since_ms: now_ms };
        }
    }

    /// Records hearing from the watch, at most once a second.
    fn heard(&mut self, store: &Store, now_ms: i64) {
        let s = now_ms.div_euclid(1000);
        if self.heard_s != Some(s) {
            self.heard_s = Some(s);
            health::seen(store.dir(), health::Link::Watch, s);
        }
    }

    fn tick(&mut self, store: &Store, now_ms: i64, out: &mut Output) {
        // The phone's clock set back: the waits count from now.
        if let Some(since) = self.asked_ms.as_mut() {
            *since = (*since).min(now_ms);
        }
        if let Some(t) = self.transfer.as_mut() {
            t.last_ms = t.last_ms.min(now_ms);
        }
        if let Some(since) = self.asked_ms
            && now_ms - since >= ANSWER_WAIT_MS
        {
            self.asked_ms = None;
            if matches!(self.progress, Progress::Asking { .. }) {
                self.progress = Progress::NoAnswer;
            }
        }
        if let Some(t) = &self.transfer
            && now_ms - t.last_ms >= STALL_MS
        {
            let f = t.f.clone();
            log::info!("sync: nothing of {f} for {} ms", now_ms - t.last_ms);
            self.again(store, &f, now_ms, out);
        }
    }

    fn on_state(&mut self, store: &Store, s: State, now_ms: i64, out: &mut Output) {
        let saved = WatchState {
            version: Some(s.version.clone()),
            m: Some(s.m),
            gps: Some(s.gps),
            p: s.p,
            run: s.run.clone(),
            heard: Some(now_ms.div_euclid(1000)),
        };
        if let Err(e) = store.save_watch_state(&saved) {
            warn("save", "watch.json", &e);
        }
        match store.deleted_on_watch(&s.del) {
            Ok(_) => {}
            Err(e) => warn("delete", "what the watch deleted", &e),
        }
        out.changed = true;
        self.asked_ms = None;
        // A new round.
        self.tries.clear();
        self.failed.clear();
        self.fetched = 0;
        self.transfer = None;
        let busy = s.run.is_some();
        let hist = s.hist;
        self.state = Some(s);
        if busy {
            self.state = None;
            self.progress = Progress::Busy;
        } else if hist != store.watch_history().hist {
            if hist.len > MAX_HISTORY {
                log::warn!(
                    "sync: the watch's history is {} bytes: not fetched",
                    hist.len
                );
                self.failed.push(HISTORY.into());
                self.next_log(store, now_ms, out);
            } else {
                self.fetch(store, HISTORY, now_ms, out);
            }
        } else {
            // The history last fetched is still the watch's: a phone delete
            // it doesn't list has been done (one the watch had forgotten
            // leaves its history, and so `hist`, as it was).
            if let Err(e) = store.settle_unlisted() {
                warn("settle", "the deletes", &e);
            }
            self.next_log(store, now_ms, out);
        }
    }

    /// Asks for `f` from where the phone has got to.
    fn fetch(&mut self, store: &Store, f: &str, now_ms: i64, out: &mut Output) {
        let at = if f == HISTORY { 0 } else { store.part_len(f) };
        self.transfer = Some(Transfer::new(f, now_ms));
        out.lines.push(link::get_line(f, at));
        self.show(f, at);
    }

    /// The progress for fetching `f`, `got` bytes so far.
    fn show(&mut self, f: &str, got: u64) {
        self.progress = if f == HISTORY {
            Progress::History { got }
        } else {
            match &mut self.progress {
                Progress::Logs { f: now, got: g, .. } if now == f => {
                    *g = got;
                    return;
                }
                _ => Progress::Logs {
                    n: 1,
                    of: 1,
                    f: f.to_string(),
                    got,
                },
            }
        };
    }

    /// Asks for `f` again, or gives up on it for the round after
    /// [`TRIES`] times, and goes on to the next.
    fn again(&mut self, store: &Store, f: &str, now_ms: i64, out: &mut Output) {
        let tries = self.tries.entry(f.to_string()).or_insert(0);
        if *tries >= TRIES {
            log::warn!("sync: gave up on {f} for now");
            self.give_up(f);
            self.next_log(store, now_ms, out);
        } else {
            *tries += 1;
            let progress = self.progress.clone();
            self.fetch(store, f, now_ms, out);
            if let (Progress::Logs { n, of, .. }, Progress::Logs { n: n2, of: of2, .. }) =
                (progress, &mut self.progress)
            {
                (*n2, *of2) = (n, of);
            }
        }
    }

    fn give_up(&mut self, f: &str) {
        if !self.failed.iter().any(|g| g == f) {
            self.failed.push(f.to_string());
        }
        self.transfer = None;
    }

    /// The next log the round needs, or the round is done.
    fn next_log(&mut self, store: &Store, now_ms: i64, out: &mut Output) {
        self.transfer = None;
        if self.state.is_none() {
            return;
        }
        let here: BTreeMap<String, LogState> = store
            .results()
            .into_iter()
            .map(|r| (r.entry.f, r.log))
            .collect();
        let todo: Vec<String> = store
            .watch_history()
            .listed
            .into_iter()
            .filter(|f| {
                matches!(here.get(f), Some(LogState::Partial(_) | LogState::Missing))
                    && !self.failed.contains(f)
            })
            .collect();
        let Some(f) = todo.first() else {
            self.state = None;
            self.progress = Progress::Done {
                at_ms: now_ms,
                failed: self.failed.clone(),
            };
            if self.failed.is_empty() {
                health::seen(store.dir(), health::Link::Synced, now_ms.div_euclid(1000));
            }
            return;
        };
        let f = f.clone();
        let before = self.fetched + self.failed.iter().filter(|f| *f != HISTORY).count();
        self.fetch(store, &f, now_ms, out);
        if let Progress::Logs { n, of, .. } = &mut self.progress {
            (*n, *of) = (before + 1, before + todo.len());
        }
    }

    fn on_piece(&mut self, store: &Store, p: Piece, now_ms: i64, out: &mut Output) {
        let Some(t) = self.transfer.as_mut() else {
            return;
        };
        if p.f != t.f {
            t.broken = true;
            return;
        }
        if t.f == HISTORY {
            let len = t.history.len() as u64;
            if p.at == len && p.crc_ok() && len + p.d.len() as u64 <= MAX_HISTORY {
                t.history.extend_from_slice(&p.d);
                t.last_ms = now_ms;
                let got = t.history.len() as u64;
                self.show(HISTORY, got);
            } else {
                t.broken = true;
            }
            return;
        }
        let part = store.part_len(&p.f);
        if p.at != part || !p.crc_ok() {
            t.broken = true;
            return;
        }
        if part + p.d.len() as u64 > MAX_LOG {
            log::warn!("sync: {} is past {MAX_LOG} bytes: refused", p.f);
            if let Err(e) = store.set_no_log(&p.f) {
                // Not marked: not asked for again this round either.
                warn("mark", &p.f, &e);
                self.give_up(&p.f);
            }
            out.changed = true;
            self.next_log(store, now_ms, out);
            return;
        }
        match store.append_part(&p.f, &p.d) {
            Ok(got) => {
                t.last_ms = now_ms;
                self.show(&p.f, got);
            }
            Err(e) => {
                warn("append to", &p.f, &e);
                t.broken = true;
            }
        }
    }

    fn on_end(&mut self, store: &Store, f: &str, len: Option<u64>, now_ms: i64, out: &mut Output) {
        let Some(t) = self.transfer.as_mut() else {
            return;
        };
        if t.f != f {
            return;
        }
        if f == HISTORY {
            let want = self.state.as_ref().map_or(Hist::default(), |s| s.hist);
            let history = std::mem::take(&mut t.history);
            let whole = match len {
                Some(len) => len == history.len() as u64 && Hist::of(&history) == want,
                None => want == Hist::default(),
            };
            if !whole {
                log::info!(
                    "sync: the history came {} of {len:?} bytes, {} not {want}{}",
                    history.len(),
                    Hist::of(&history),
                    if t.broken { ", broken" } else { "" }
                );
                return self.again(store, f, now_ms, out);
            }
            match link::parse_history(&history) {
                Ok(entries) => match store.merge_history(&entries, want) {
                    Ok(merged) => out.changed |= merged.changed(),
                    Err(e) => {
                        warn("take in", HISTORY, &e);
                        self.give_up(HISTORY);
                    }
                },
                Err(_) if len.is_none() => {
                    // No history on the watch at all: nothing listed.
                    if let Err(e) = store.merge_history(&[], want) {
                        warn("take in", HISTORY, &e);
                    }
                }
                Err(why) => {
                    log::warn!("sync: {why}");
                    self.give_up(HISTORY);
                }
            }
            return self.next_log(store, now_ms, out);
        }
        let part = store.part_len(f);
        match len {
            None => {
                if let Err(e) = store.set_no_log(f) {
                    // Not marked: not asked for again this round either.
                    warn("mark", f, &e);
                    self.give_up(f);
                }
                out.changed = true;
                self.next_log(store, now_ms, out);
            }
            Some(len) if len == part => {
                match store.finish_log(f) {
                    Ok(()) => {
                        self.fetched += 1;
                        out.changed = true;
                    }
                    Err(e) => {
                        warn("finish", f, &e);
                        self.give_up(f);
                    }
                }
                self.next_log(store, now_ms, out);
            }
            Some(len) if len > MAX_LOG => {
                // Not fetched this round. Only a piece past the cap marks
                // the log for good: anyone can send a logend.
                log::warn!("sync: {f} says it's {len} bytes: not fetched");
                self.give_up(f);
                self.next_log(store, now_ms, out);
            }
            Some(len) => {
                if len < part {
                    // The watch's file is shorter than what's here: start again.
                    if let Err(e) = store.drop_part(f) {
                        warn("drop", f, &e);
                    }
                }
                self.again(store, f, now_ms, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::sync_line;
    use crate::store::{Entry, Tombstone};

    const A: &str = "beeptest.1781344800.csv";
    const B: &str = "beeptest.1781258400.csv";

    fn entry(f: &str, t: i64) -> Entry {
        Entry {
            f: f.into(),
            t,
            m: 20,
            p: 0,
            l: 2,
            s: 1,
            d: 160,
            dur: 71,
            max: 150,
            avg: 130,
        }
    }

    /// The history as the watch sends it (`JSON.stringify` of its file).
    fn history(entries: &[Entry]) -> Vec<u8> {
        let items: Vec<String> = entries
            .iter()
            .map(|e| {
                format!(
                    "{{\"t\":{},\"m\":{},\"l\":{},\"s\":{},\"d\":{},\"dur\":{},\"max\":{},\"avg\":{},\"f\":\"{}\"}}",
                    e.t, e.m, e.l, e.s, e.d, e.dur, e.max, e.avg, e.f
                )
            })
            .collect();
        format!("[{}]", items.join(",")).into_bytes()
    }

    fn state(hist: Hist, run: Option<&str>, del: &[&str]) -> Input {
        Input::Watch(WatchEvent::State(State {
            version: "0.05".into(),
            m: 20,
            gps: false,
            p: None,
            run: run.map(str::to_string),
            hist,
            del: del.iter().map(|s| s.to_string()).collect(),
        }))
    }

    fn piece(f: &str, at: u64, d: &[u8]) -> Input {
        Input::Watch(WatchEvent::Log(Piece {
            f: f.into(),
            at,
            crc: link::crc32(d),
            d: d.to_vec(),
        }))
    }

    fn end(f: &str, len: i64) -> Input {
        Input::Watch(WatchEvent::LogEnd {
            f: f.into(),
            len: u64::try_from(len).ok(),
        })
    }

    struct T {
        _dir: tempfile::TempDir,
        store: Store,
        engine: Engine,
        now: i64,
    }

    impl T {
        fn new() -> T {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(dir.path()).unwrap();
            T {
                _dir: dir,
                store,
                engine: Engine::new(),
                now: 1_781_400_000_000,
            }
        }

        fn go(&mut self, input: Input) -> Vec<String> {
            self.now += 50;
            self.engine.handle(&self.store, input, self.now).lines
        }

        /// The watch has `entries` and sends its history whole in one piece.
        fn history(&mut self, entries: &[Entry]) -> Hist {
            let h = history(entries);
            let hist = Hist::of(&h);
            assert_eq!(
                self.go(state(hist, None, &[])),
                vec![link::get_line(HISTORY, 0)]
            );
            self.go(piece(HISTORY, 0, &h));
            hist
        }
    }

    #[test]
    fn a_round_fetches_the_history_then_each_log() {
        let mut t = T::new();
        assert_eq!(t.go(Input::Opened), vec![sync_line(&[])]);
        assert_eq!(t.engine.progress().words(), "Asking the watch...");
        let h = history(&[entry(A, 2), entry(B, 1)]);
        assert!(t.engine.progress().active());
        let hist = t.history(&[entry(A, 2), entry(B, 1)]);
        assert_eq!(
            t.engine.progress(),
            &Progress::History {
                got: h.len() as u64
            }
        );
        assert_eq!(
            t.go(end(HISTORY, h.len() as i64)),
            vec![link::get_line(A, 0)]
        );
        assert_eq!(t.engine.progress().words(), "Fetching 1 of 2 logs...");
        assert_eq!(t.store.watch_history().hist, hist);
        assert_eq!(t.store.results().len(), 2);
        t.go(piece(A, 0, b"start,1,0,20\n"));
        t.go(piece(A, 13, b"0,1,2,3,4,5\n"));
        assert_eq!(t.go(end(A, 25)), vec![link::get_line(B, 0)]);
        assert_eq!(t.store.log_state(A), LogState::Complete);
        assert_eq!(t.engine.progress().words(), "Fetching 2 of 2 logs...");
        assert_eq!(t.go(end(B, -1)), Vec::<String>::new());
        assert_eq!(t.store.log_state(B), LogState::NoLog);
        assert!(matches!(t.engine.progress(), Progress::Done { failed, .. } if failed.is_empty()));
        assert_eq!(t.engine.progress().words(), "Up to date");
        let dir = t.store.dir().to_path_buf();
        assert_eq!(
            health::last_seen(&dir, health::Link::Synced),
            Some(t.now.div_euclid(1000))
        );
        assert!(health::last_seen(&dir, health::Link::Watch).is_some());
        // The same state again: nothing to fetch.
        assert_eq!(t.go(state(hist, None, &[])), Vec::<String>::new());
        assert_eq!(t.store.watch_state().version.as_deref(), Some("0.05"));
    }

    #[test]
    fn a_broken_transfer_is_asked_for_again_from_the_partial() {
        let mut t = T::new();
        let h = history(&[entry(A, 2)]);
        t.history(&[entry(A, 2)]);
        t.go(end(HISTORY, h.len() as i64));
        let log = b"0123456789abcdefghij";
        t.go(piece(A, 0, &log[..5]));
        // A piece lost: the next is out of place, and dropped.
        t.go(piece(A, 10, &log[10..15]));
        // A bad CRC is dropped too.
        t.go(Input::Watch(WatchEvent::Log(Piece {
            f: A.into(),
            at: 5,
            crc: 1,
            d: log[5..10].to_vec(),
        })));
        // A piece of another file.
        t.go(piece(B, 5, &log[5..10]));
        assert_eq!(t.store.part_len(A), 5);
        assert_eq!(t.go(end(A, 20)), vec![link::get_line(A, 5)]);
        t.go(piece(A, 5, &log[5..15]));
        t.go(piece(A, 5, &log[5..15]));
        t.go(piece(A, 15, &log[15..]));
        assert_eq!(t.go(end(A, 20)), Vec::<String>::new());
        assert_eq!(std::fs::read(t.store.log_path(A)).unwrap(), log);
    }

    #[test]
    fn three_tries_then_the_next_round() {
        let mut t = T::new();
        let h = history(&[entry(A, 2), entry(B, 1)]);
        let hist = t.history(&[entry(A, 2), entry(B, 1)]);
        t.go(end(HISTORY, h.len() as i64));
        for i in 0..TRIES {
            t.go(piece(A, i as u64, b"x"));
            assert_eq!(
                t.go(end(A, 100)),
                vec![link::get_line(A, i as u64 + 1)],
                "try {i}"
            );
        }
        t.go(piece(A, 3, b"x"));
        // Given up on: the next.
        assert_eq!(t.go(end(A, 100)), vec![link::get_line(B, 0)]);
        assert_eq!(t.go(end(B, -1)), Vec::<String>::new());
        assert_eq!(t.engine.progress().words(), "Couldn't fetch 1 log");
        assert!(matches!(t.engine.progress(), Progress::Done { failed, .. } if failed == &[A]));
        assert_eq!(health::last_seen(t.store.dir(), health::Link::Synced), None);
        // The next round tries again from where it got to.
        assert_eq!(t.go(state(hist, None, &[])), vec![link::get_line(A, 4)]);
    }

    #[test]
    fn a_stalled_transfer_is_asked_for_again_while_the_app_is_open() {
        let mut t = T::new();
        let h = history(&[entry(A, 2)]);
        t.history(&[entry(A, 2)]);
        t.go(end(HISTORY, h.len() as i64));
        t.go(piece(A, 0, b"abc"));
        t.now += STALL_MS - 100;
        assert_eq!(t.go(Input::Tick), Vec::<String>::new());
        t.now += 100;
        assert_eq!(t.go(Input::Tick), vec![link::get_line(A, 3)]);
        for _ in 1..TRIES {
            t.now += STALL_MS;
            assert_eq!(t.go(Input::Tick), vec![link::get_line(A, 3)]);
        }
        t.now += STALL_MS;
        assert_eq!(t.go(Input::Tick), Vec::<String>::new());
        assert_eq!(t.engine.progress().words(), "Couldn't fetch 1 log");
        // The history stalls the same way.
        let mut t = T::new();
        t.go(state(Hist { len: 9, crc: 9 }, None, &[]));
        t.now += STALL_MS;
        assert_eq!(t.go(Input::Tick), vec![link::get_line(HISTORY, 0)]);
    }

    #[test]
    fn no_answer() {
        let mut t = T::new();
        t.go(Input::SyncNow);
        t.now += ANSWER_WAIT_MS;
        t.go(Input::Tick);
        assert_eq!(t.engine.progress(), &Progress::NoAnswer);
        assert_eq!(t.engine.progress().words(), "The watch isn't answering");
        assert!(!t.engine.progress().active());
        // It answers later after all.
        t.go(state(Hist::default(), None, &[]));
        assert_eq!(t.engine.progress().words(), "Up to date");
    }

    #[test]
    fn a_history_is_taken_only_whole() {
        let mut t = T::new();
        let h = history(&[entry(A, 2), entry(B, 1)]);
        let hist = Hist::of(&h);
        t.go(state(hist, None, &[]));
        t.go(piece(HISTORY, 0, &h[..10]));
        t.go(piece(HISTORY, 20, &h[20..]));
        // Short: asked for again from 0.
        assert_eq!(
            t.go(end(HISTORY, h.len() as i64)),
            vec![link::get_line(HISTORY, 0)]
        );
        assert!(t.store.results().is_empty());
        // Whole, but not what state said (it changed since): again.
        let other = history(&[entry(A, 2)]);
        t.go(piece(HISTORY, 0, &other));
        assert_eq!(
            t.go(end(HISTORY, other.len() as i64)),
            vec![link::get_line(HISTORY, 0)]
        );
        // A length that doesn't match.
        t.go(piece(HISTORY, 0, &h));
        assert_eq!(t.go(end(HISTORY, 5)), vec![link::get_line(HISTORY, 0)]);
        t.go(piece(HISTORY, 0, &h));
        assert_eq!(
            t.go(end(HISTORY, h.len() as i64)),
            vec![link::get_line(A, 0)]
        );
        assert_eq!(t.store.results().len(), 2);
    }

    #[test]
    fn a_history_given_up_on_leaves_the_last_one() {
        let mut t = T::new();
        let h = history(&[entry(A, 2)]);
        t.history(&[entry(A, 2)]);
        t.go(end(HISTORY, h.len() as i64));
        t.go(end(A, -1));
        // A new history that never arrives right.
        t.go(state(Hist { len: 5, crc: 5 }, None, &[]));
        for _ in 0..TRIES {
            assert_eq!(t.go(end(HISTORY, 0)), vec![link::get_line(HISTORY, 0)]);
        }
        assert_eq!(t.go(end(HISTORY, 0)), Vec::<String>::new());
        assert_eq!(t.engine.progress().words(), "Couldn't fetch the results");
        assert_eq!(t.store.watch_history().listed, vec![A]);
        // Not a JSON list: given up on at once.
        let mut t = T::new();
        let junk = b"{\"a\":1}";
        t.go(state(Hist::of(junk), None, &[]));
        t.go(piece(HISTORY, 0, junk));
        assert_eq!(t.go(end(HISTORY, junk.len() as i64)), Vec::<String>::new());
        assert_eq!(t.engine.progress().words(), "Couldn't fetch the results");
        // Past the cap: not even asked for.
        let mut t = T::new();
        let lines = t.go(state(
            Hist {
                len: MAX_HISTORY + 1,
                crc: 0,
            },
            None,
            &[],
        ));
        assert_eq!(lines, Vec::<String>::new());
        assert_eq!(t.engine.progress().words(), "Couldn't fetch the results");
    }

    #[test]
    fn no_history_on_the_watch() {
        let mut t = T::new();
        let h = history(&[entry(A, 2)]);
        t.history(&[entry(A, 2)]);
        t.go(end(HISTORY, h.len() as i64));
        t.go(end(A, -1));
        t.go(Input::Delete(A.into()));
        // The watch's history went (erased): 0:0, and get answers len -1.
        assert_eq!(
            t.go(state(Hist::default(), None, &[])),
            vec![link::get_line(HISTORY, 0)]
        );
        assert_eq!(t.go(end(HISTORY, -1)), Vec::<String>::new());
        assert_eq!(t.store.watch_history(), Default::default());
        // A's tombstone is done: the watch no longer has it.
        assert_eq!(t.store.tombstones()[A], Tombstone::Done);
        assert_eq!(t.store.pending_deletes(), Vec::<String>::new());
    }

    #[test]
    fn busy_and_running_tests() {
        let mut t = T::new();
        let h = history(&[entry(A, 2)]);
        let hist = t.history(&[entry(A, 2)]);
        t.go(end(HISTORY, h.len() as i64));
        assert_eq!(
            t.go(Input::Watch(WatchEvent::Busy { f: A.into() })),
            Vec::<String>::new()
        );
        assert_eq!(t.engine.progress(), &Progress::Busy);
        assert_eq!(
            t.engine.progress().words(),
            "A test is running on the watch"
        );
        // Pieces after it are ignored.
        t.go(piece(A, 0, b"x"));
        assert_eq!(t.store.part_len(A), 0);
        // A state while a test runs fetches nothing, and says so.
        let run = "beeptest.1781500000.csv";
        assert_eq!(t.go(state(hist, Some(run), &[])), Vec::<String>::new());
        assert_eq!(t.engine.progress(), &Progress::Busy);
        assert_eq!(t.store.watch_state().run.as_deref(), Some(run));
        // It ends: the round goes on.
        assert_eq!(t.go(state(hist, None, &[])), vec![link::get_line(A, 0)]);
        assert_eq!(t.store.watch_state().run, None);
    }

    #[test]
    fn deletes_both_ways() {
        let mut t = T::new();
        let h = history(&[entry(A, 2), entry(B, 1)]);
        let hist = t.history(&[entry(A, 2), entry(B, 1)]);
        t.go(end(HISTORY, h.len() as i64));
        t.go(piece(A, 0, b"partial"));
        // Deleted on the phone mid-transfer: gone, and the sync names it.
        let out = t.engine.handle(&t.store, Input::Delete(A.into()), t.now);
        assert!(out.changed);
        assert_eq!(out.lines, vec![sync_line(&[A.into()])]);
        assert_eq!(t.store.part_len(A), 0);
        assert_eq!(t.engine.progress().words(), "Asking the watch...");
        t.go(piece(A, 7, b"more"));
        assert!(!t.store.log_path(A).exists() && t.store.part_len(A) == 0);
        // The watch answers before deleting it (a state already on its way):
        // A isn't fetched, and isn't back.
        assert_eq!(t.go(state(hist, None, &[])), vec![link::get_line(B, 0)]);
        assert!(t.store.result(A).is_none());
        // Deleted on the watch: B goes here.
        t.go(piece(B, 0, b"b"));
        let out = t.engine.handle(&t.store, state(hist, None, &[B]), t.now);
        assert!(out.changed);
        assert!(t.store.results().is_empty());
        assert_eq!(t.store.part_len(B), 0);
        assert_eq!(t.store.tombstones()[B], Tombstone::Done);
        // The next sync still names A until the watch is seen without it.
        assert_eq!(t.go(Input::SyncNow), vec![sync_line(&[A.into()])]);
        let h2 = history(&[]);
        t.go(state(Hist::of(&h2), None, &[B]));
        t.go(piece(HISTORY, 0, &h2));
        t.go(end(HISTORY, h2.len() as i64));
        assert_eq!(t.go(Input::SyncNow), vec![sync_line(&[])]);
    }

    #[test]
    fn a_phone_delete_the_watch_had_forgotten_is_settled_by_an_equal_hist() {
        let mut t = T::new();
        // The watch lists only B; the phone still has A from before.
        t.store
            .merge_history(&[entry(A, 2)], Hist::default())
            .unwrap();
        let h = history(&[entry(B, 1)]);
        let hist = t.history(&[entry(B, 1)]);
        t.go(end(HISTORY, h.len() as i64));
        t.go(end(B, -1));
        assert_eq!(t.store.watch_history().listed, vec![B]);
        assert!(t.store.result(A).is_some());
        assert_eq!(t.go(Input::Delete(A.into())), vec![sync_line(&[A.into()])]);
        // The watch had nothing of A to take out of its history: the same
        // hist. A isn't listed, so the delete is done.
        assert_eq!(t.go(state(hist, None, &[])), Vec::<String>::new());
        assert_eq!(t.store.tombstones()[A], Tombstone::Done);
        assert_eq!(t.go(Input::SyncNow), vec![sync_line(&[])]);
        // One the watch still lists stays pending until it's seen without it.
        t.go(state(hist, None, &[]));
        assert_eq!(t.go(Input::Delete(B.into())), vec![sync_line(&[B.into()])]);
        t.go(state(hist, None, &[]));
        assert_eq!(t.store.tombstones()[B], Tombstone::Pending);
    }

    #[test]
    fn deleting_the_log_being_fetched_ends_the_round() {
        let mut t = T::new();
        let h = history(&[entry(A, 2), entry(B, 1)]);
        t.history(&[entry(A, 2), entry(B, 1)]);
        t.go(end(HISTORY, h.len() as i64));
        t.go(piece(A, 0, b"abc"));
        assert_eq!(t.engine.progress().words(), "Fetching 1 of 2 logs...");
        // The watch went out of range (which is why A is still arriving),
        // and A is deleted here.
        assert_eq!(t.go(Input::Delete(A.into())), vec![sync_line(&[A.into()])]);
        assert_eq!(t.engine.progress().words(), "Asking the watch...");
        for _ in 0..(ANSWER_WAIT_MS / 1000) {
            t.now += 1000;
            assert_eq!(t.go(Input::Tick), Vec::<String>::new());
        }
        assert_eq!(t.engine.progress(), &Progress::NoAnswer);
        assert!(!t.engine.progress().active());
        // Sync now asks again, and says so the same way.
        t.go(Input::SyncNow);
        t.now += ANSWER_WAIT_MS;
        t.go(Input::Tick);
        assert_eq!(t.engine.progress(), &Progress::NoAnswer);
        // A delete of another result leaves a fetch under way showing.
        let hist = t.store.watch_history().hist;
        assert_eq!(t.go(state(hist, None, &[])), vec![link::get_line(B, 0)]);
        t.go(Input::Delete("beeptest.1.csv".into()));
        assert_eq!(t.engine.progress().words(), "Fetching the log...");
    }

    #[test]
    fn a_log_that_cant_be_marked_is_given_up_on_not_asked_for_forever() {
        let mut t = T::new();
        let h = history(&[entry(A, 2), entry(B, 1)]);
        t.history(&[entry(A, 2), entry(B, 1)]);
        t.go(end(HISTORY, h.len() as i64));
        // The phone's storage can't take a write of results.json.
        let tmp = t.store.dir().join("results.json.tmp");
        std::fs::create_dir(&tmp).unwrap();
        std::fs::write(tmp.join("x"), b"x").unwrap();
        assert!(t.store.set_no_log(A).is_err(), "the write fails");
        // The watch hasn't got A's log: on to B, and A isn't asked for
        // again this round.
        assert_eq!(t.go(end(A, -1)), vec![link::get_line(B, 0)]);
        assert_eq!(t.go(end(A, -1)), Vec::<String>::new());
        assert_eq!(t.go(end(B, -1)), Vec::<String>::new());
        assert!(
            matches!(t.engine.progress(), Progress::Done { failed, .. } if failed == &[A, B]),
            "{:?}",
            t.engine.progress()
        );
        // A piece past the cap, the same.
        let hist = t.store.watch_history().hist;
        t.store
            .append_part(A, &vec![b'x'; MAX_LOG as usize - 10])
            .unwrap();
        t.go(state(hist, None, &[]));
        assert_eq!(
            t.go(piece(A, MAX_LOG - 10, &[b'y'; 11])),
            vec![link::get_line(B, 0)]
        );
        assert_eq!(t.go(end(A, -1)), Vec::<String>::new());
    }

    #[test]
    fn the_waits_count_from_now_when_the_clock_is_set_back() {
        let mut t = T::new();
        let h = history(&[entry(A, 2)]);
        t.history(&[entry(A, 2)]);
        t.go(end(HISTORY, h.len() as i64));
        t.go(piece(A, 0, b"abc"));
        // Set back an hour, then the watch goes out of range.
        t.now -= 3_600_000;
        let mut asked = 0;
        for _ in 0..(STALL_MS / 1000 + 1) {
            t.now += 1000;
            asked += t.go(Input::Tick).len();
        }
        assert_eq!(asked, 1, "asked for again after 10 s, not an hour");
        // The answer's wait too.
        let mut t = T::new();
        t.go(Input::SyncNow);
        t.now -= 3_600_000;
        for _ in 0..(ANSWER_WAIT_MS / 1000 + 1) {
            t.now += 1000;
            t.go(Input::Tick);
        }
        assert_eq!(t.engine.progress(), &Progress::NoAnswer);
    }

    #[test]
    fn a_shorter_file_starts_again_and_a_huge_one_is_refused() {
        let mut t = T::new();
        let h = history(&[entry(A, 2), entry(B, 1)]);
        t.history(&[entry(A, 2), entry(B, 1)]);
        t.go(end(HISTORY, h.len() as i64));
        t.store.append_part(A, b"0123456789").unwrap();
        t.go(piece(A, 0, b"zz"));
        // The watch's file is 4 bytes: what's here is wrong, from 0.
        assert_eq!(t.go(end(A, 4)), vec![link::get_line(A, 0)]);
        assert_eq!(t.store.part_len(A), 0);
        t.go(piece(A, 0, b"abcd"));
        assert_eq!(t.go(end(A, 4)), vec![link::get_line(B, 0)]);
        // B says it's past the cap: not fetched this round, but not marked
        // (anyone can send a logend).
        assert_eq!(t.go(end(B, MAX_LOG as i64 + 1)), Vec::<String>::new());
        assert_eq!(t.store.log_state(B), LogState::Missing);
        assert_eq!(t.engine.progress().words(), "Couldn't fetch 1 log");
    }

    #[test]
    fn a_piece_past_the_cap_refuses_the_log() {
        let mut t = T::new();
        let h = history(&[entry(A, 2)]);
        t.history(&[entry(A, 2)]);
        t.go(end(HISTORY, h.len() as i64));
        let big = vec![b'x'; MAX_LOG as usize - 10];
        t.store.append_part(A, &big).unwrap();
        t.go(state(t.store.watch_history().hist, None, &[]));
        t.go(piece(A, MAX_LOG - 10, &[b'y'; 11]));
        assert_eq!(t.store.log_state(A), LogState::NoLog);
        assert_eq!(t.store.part_len(A), 0);
        assert!(matches!(t.engine.progress(), Progress::Done { .. }));
    }

    #[test]
    fn the_watchs_speeds_are_kept_when_it_has_them() {
        let mut t = T::new();
        let Input::Watch(WatchEvent::State(s)) = state(Hist::default(), None, &[]) else {
            unreachable!()
        };
        t.go(Input::Watch(WatchEvent::State(State {
            p: Some(2),
            ..s.clone()
        })));
        assert_eq!(t.store.watch_state().p, Some(2));
        // A watch app without the Speeds setting (installed over, say).
        t.go(Input::Watch(WatchEvent::State(s)));
        assert_eq!(t.store.watch_state().p, None);
        assert_eq!(t.store.watch_state().m, Some(20));
    }

    #[test]
    fn set_watch_lines() {
        let mut t = T::new();
        assert_eq!(
            t.go(Input::SetWatch {
                m: Some(15),
                gps: None,
                p: None
            }),
            vec![link::set_line(Some(15), None, None)]
        );
        assert_eq!(
            t.go(Input::SetWatch {
                m: Some(17),
                gps: Some(true),
                p: None
            }),
            vec![link::set_line(None, Some(true), None)]
        );
        assert_eq!(
            t.go(Input::SetWatch {
                m: None,
                gps: None,
                p: Some(2)
            }),
            vec![link::set_line(None, None, Some(2))]
        );
        assert_eq!(
            t.go(Input::SetWatch {
                m: Some(20),
                gps: None,
                p: Some(3)
            }),
            vec![link::set_line(Some(20), None, None)]
        );
        assert_eq!(
            t.go(Input::SetWatch {
                m: Some(17),
                gps: None,
                p: Some(9)
            }),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_restart_carries_on() {
        let mut t = T::new();
        let h = history(&[entry(A, 2)]);
        let hist = t.history(&[entry(A, 2)]);
        t.go(end(HISTORY, h.len() as i64));
        t.go(piece(A, 0, b"start,"));
        // The app is killed: a new engine, the store opened again.
        let dir = t.store.dir().to_path_buf();
        t.engine = Engine::new();
        t.store = Store::open(&dir).unwrap();
        assert_eq!(t.engine.progress(), &Progress::Idle);
        assert_eq!(t.engine.progress().words(), "");
        // A piece of the old transfer arriving now is ignored.
        t.go(piece(A, 6, b"1,0,20\n"));
        assert_eq!(t.store.part_len(A), 6);
        // The next state carries on from the partial.
        assert_eq!(t.go(state(hist, None, &[])), vec![link::get_line(A, 6)]);
        t.go(piece(A, 6, b"1,0,20\n"));
        t.go(end(A, 13));
        assert_eq!(
            std::fs::read(t.store.log_path(A)).unwrap(),
            b"start,1,0,20\n"
        );
    }

    #[test]
    fn progress_words() {
        let logs = |n, of| Progress::Logs {
            n,
            of,
            f: A.into(),
            got: 0,
        };
        assert_eq!(logs(1, 1).words(), "Fetching the log...");
        assert_eq!(logs(2, 3).words(), "Fetching 2 of 3 logs...");
        assert_eq!(
            Progress::Done {
                at_ms: 0,
                failed: vec![A.into(), B.into()]
            }
            .words(),
            "Couldn't fetch 2 logs"
        );
        assert!(logs(1, 1).active() && Progress::History { got: 0 }.active());
        assert!(!Progress::Busy.active() && !Progress::Idle.active());
    }
}
