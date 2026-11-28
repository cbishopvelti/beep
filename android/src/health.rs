//! When the phone last heard from the watch and last synced with it, for
//! the Watch page. The times are kept as files in `health/` in the store's
//! directory, as blink's, so what a broadcast records with no UI open (the
//! watch's messages can start the app's process by themselves) is there
//! when the app next opens.
//!
//! Unlike blink's, every call names the store's directory and the time is
//! passed in, so the sync engine stays pure and each test can use its own
//! directory. Times are Unix seconds.

use std::path::Path;

/// What a time is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The last message from the watch that [`crate::link::parse_event`]
    /// took (`health/watch`).
    Watch,
    /// The last sync round that finished with everything the watch lists on
    /// the phone (`health/synced`).
    Synced,
}

/// Records `link` at `unix_s` (written atomically).
pub fn seen(dir: &Path, link: Link, unix_s: i64) {
    todo!()
}

/// When `link` was last recorded under `dir`, Unix seconds.
pub fn last_seen(dir: &Path, link: Link) -> Option<i64> {
    todo!()
}

/// The phone's clock, Unix seconds.
pub fn now_s() -> i64 {
    todo!()
}

/// A time in the phone's time zone, broken down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    /// 1 to 31.
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 is Sunday.
    pub weekday: u32,
}

/// `unix_s` in the phone's time zone (`localtime_r`).
pub fn local(unix_s: i64) -> Option<Local> {
    todo!()
}

/// "28 Nov 2026, 14:05:12", in the phone's time zone (the seconds as a
/// number when `localtime_r` fails).
pub fn local_time(unix_s: i64) -> String {
    todo!()
}

/// "just now", "5 min ago", "3 h ago", "1 day ago", "2 days ago".
pub fn ago(then_s: i64, now_s: i64) -> String {
    todo!()
}
