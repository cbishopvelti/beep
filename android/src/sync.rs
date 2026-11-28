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
//! too). Each `state` starts a round:
//!
//! 1. Save the watch's version, settings and running file
//!    ([`crate::store::WatchState`], `heard` now), and delete every result
//!    named in `del` ([`Store::deleted_on_watch`]).
//! 2. If `hist` differs from [`Store::watch_history`]'s, `get`
//!    `beeptest.json` from 0. It is kept in memory as it arrives (at most
//!    [`crate::link::MAX_HISTORY`]) and taken in ([`Store::merge_history`])
//!    only when whole: pieces contiguous from 0, each CRC right, `logend`'s
//!    `len` what arrived, and the whole's [`crate::link::Hist`] equal to
//!    `state`'s. Otherwise it is asked for again (the same at most
//!    [`TRIES`] times as a log).
//! 3. The logs, one at a time: the first name in
//!    [`crate::store::WatchHistory::listed`] that has a result on the phone
//!    without a complete log ([`crate::store::LogState`] `Partial` or
//!    `Missing`), isn't `state`'s `run`, and hasn't been given up on this
//!    round. `get` it from [`Store::part_len`]. A good piece (right `f`,
//!    `at` equal to the partial's size, CRC right, not past
//!    [`crate::link::MAX_LOG`]) is appended; any other is dropped and the
//!    transfer marked broken. On `logend`, `len` equal to the partial's
//!    size finishes it ([`Store::finish_log`]); `None` marks it
//!    ([`Store::set_no_log`]); a `len` shorter than the partial drops the
//!    partial; otherwise (broken, or short) it is asked for again from the
//!    partial's size, at most [`TRIES`] times a file a round, then given
//!    up on for the round. So is a transfer from which nothing has arrived
//!    for [`STALL_MS`] ([`Input::Tick`]). Then the next.
//! 4. Nothing left: the round is done ([`crate::health::Link::Synced`]).
//!
//! `busy` ends the round (a test is running; the watch says `state` when it
//! ends). Everything needed to carry on is on disk (the partials, the
//! tombstones, the history last fetched), so a round cut short by the app
//! being killed carries on at the next `state`. What is only in memory (the
//! history arriving, the tries) starts again.
//!
//! Each event taken counts as hearing from the watch
//! ([`crate::health::Link::Watch`]).

use crate::link::WatchEvent;
use crate::store::Store;
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
    /// ([`ANSWER_WAIT_MS`], [`STALL_MS`]) are checked. Usually nothing.
    Tick,
    /// The user deleted result `f` (after confirming): it goes from the
    /// phone ([`Store::delete`]) and a `beeptest-sync` asks the watch to
    /// delete it too.
    Delete(String),
    /// The user changed the watch's course (20 or 15) or GPS setting on the
    /// Watch page: a `beeptest-set` line. The page shows the watch's own
    /// settings once its `state` answers.
    SetWatch { m: Option<u32>, gps: Option<bool> },
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
    /// in it (empty when everything the watch lists is here).
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
    /// answering", "Fetching the results...", "Fetching 2 of 3 logs...",
    /// "A test is running on the watch", "Up to date", "Couldn't fetch 1
    /// log", or "" for [`Progress::Idle`].
    pub fn words(&self) -> String {
        todo!()
    }
}

/// The engine's state between inputs: only what a round needs in memory
/// (the core engineer may reshape these private fields freely).
#[derive(Debug, Default)]
pub struct Engine {
    progress: Progress,
    /// The file being fetched and how it's going.
    transfer: Option<Transfer>,
    /// The `state` the round started from.
    state: Option<crate::link::State>,
    /// Times each file was asked for again this round.
    tries: BTreeMap<String, u32>,
    /// Logs given up on this round.
    failed: Vec<String>,
    /// Logs finished this round, for "n of".
    fetched: usize,
}

#[derive(Debug)]
struct Transfer {
    f: String,
    /// A bad or out-of-place piece came.
    broken: bool,
    /// When it was asked for, or a piece last came, ms.
    last_ms: i64,
    /// The history so far, while `f` is `beeptest.json`.
    history: Vec<u8>,
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Runs `input` at `now_ms` (Unix ms): updates `store` and says what to
    /// send. Store errors (a full disk) are logged and leave the round
    /// where it was; they never panic.
    pub fn handle(&mut self, store: &Store, input: Input, now_ms: i64) -> Output {
        todo!()
    }

    /// What the sync is doing.
    pub fn progress(&self) -> &Progress {
        &self.progress
    }
}
