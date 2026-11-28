//! When the phone last heard from the watch and last synced with it, for
//! the Watch page. The times are kept as files in `health/` in the store's
//! directory, as blink's, so what a broadcast records with no UI open (the
//! watch's messages can start the app's process by themselves) is there
//! when the app next opens.
//!
//! Unlike blink's, every call names the store's directory and the time is
//! passed in, so the sync engine stays pure and each test can use its own
//! directory. Times are Unix seconds.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

impl Link {
    fn file(self, dir: &Path) -> PathBuf {
        dir.join("health").join(match self {
            Link::Watch => "watch",
            Link::Synced => "synced",
        })
    }
}

/// Records `link` at `unix_s` (written atomically).
pub fn seen(dir: &Path, link: Link, unix_s: i64) {
    let path = link.file(dir);
    let written = std::fs::create_dir_all(dir.join("health"))
        .and_then(|()| crate::store::write_atomic(&path, unix_s.to_string().as_bytes()));
    if let Err(e) = written {
        log::warn!("health: can't record {link:?}: {e}");
    }
}

/// When `link` was last recorded under `dir`, Unix seconds.
pub fn last_seen(dir: &Path, link: Link) -> Option<i64> {
    std::fs::read_to_string(link.file(dir))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The phone's clock, Unix seconds.
pub fn now_s() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
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
    let t = libc::time_t::try_from(unix_s).ok()?;
    // SAFETY: localtime_r writes only into `tm`, which outlives the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return None;
    }
    let small = |n: libc::c_int| u32::try_from(n).ok();
    Some(Local {
        year: tm.tm_year.checked_add(1900)?,
        month: small(tm.tm_mon)? + 1,
        day: small(tm.tm_mday)?,
        hour: small(tm.tm_hour)?,
        minute: small(tm.tm_min)?,
        second: small(tm.tm_sec)?,
        weekday: small(tm.tm_wday)?,
    })
}

/// The months as the pages write them.
pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// "28 Nov 2026, 14:05:12", in the phone's time zone (the seconds as a
/// number when `localtime_r` fails).
pub fn local_time(unix_s: i64) -> String {
    match local(unix_s) {
        Some(t) => format!(
            "{} {} {}, {:02}:{:02}:{:02}",
            t.day,
            MONTHS[(t.month as usize - 1).min(11)],
            t.year,
            t.hour,
            t.minute,
            t.second
        ),
        None => unix_s.to_string(),
    }
}

/// "just now", "5 min ago", "3 h ago", "1 day ago", "2 days ago".
pub fn ago(then_s: i64, now_s: i64) -> String {
    let s = now_s.saturating_sub(then_s).max(0);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => {
            let d = s / 86_400;
            format!("{d} day{} ago", if d == 1 { "" } else { "s" })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(last_seen(dir.path(), Link::Watch), None);
        seen(dir.path(), Link::Watch, 1_795_000_000);
        seen(dir.path(), Link::Synced, 1_795_000_100);
        assert_eq!(last_seen(dir.path(), Link::Watch), Some(1_795_000_000));
        assert_eq!(last_seen(dir.path(), Link::Synced), Some(1_795_000_100));
        seen(dir.path(), Link::Watch, 1_795_000_200);
        assert_eq!(last_seen(dir.path(), Link::Watch), Some(1_795_000_200));
        std::fs::write(dir.path().join("health/watch"), "garbage").unwrap();
        assert_eq!(last_seen(dir.path(), Link::Watch), None);
        assert!((now_s() - 1_795_000_000).abs() < 100 * 365 * 86_400);
    }

    #[test]
    fn ages() {
        assert_eq!(ago(100, 130), "just now");
        assert_eq!(ago(200, 100), "just now");
        assert_eq!(ago(0, 125), "2 min ago");
        assert_eq!(ago(0, 7200), "2 h ago");
        assert_eq!(ago(0, 86_400), "1 day ago");
        assert_eq!(ago(0, 3 * 86_400), "3 days ago");
        assert_eq!(
            ago(i64::MIN, i64::MAX),
            format!("{} days ago", i64::MAX / 86_400)
        );
    }

    #[test]
    fn local_times() {
        let t = local_time(1_795_000_000);
        assert!(t.contains("2026, ") && t.len() >= 20, "{t}");
        let l = local(1_795_000_000).unwrap();
        assert_eq!(l.year, 2026);
        assert!((1..=12).contains(&l.month) && (1..=31).contains(&l.day) && l.weekday < 7);
        // A day later is the next weekday.
        assert_eq!(local(1_795_086_400).unwrap().weekday, (l.weekday + 1) % 7);
    }
}
