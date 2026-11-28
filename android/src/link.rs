//! The watch link's wire format, from the phone's side (the watch's side is
//! `beeptest.boot.js`; `docs/DEVELOPMENT.md`, "The watch link", has both).
//!
//! **Phone to watch** is a line `\x10GB(<json>)\n` that the watch's console
//! runs: [`sync_line`], [`get_line`] and [`set_line`]. The line is ASCII:
//! Gadgetbridge sends it as ISO-8859-1 bytes, which turns anything past
//! U+00FF into "?", and the watch reads bytes from 0x80 up as the start of
//! UTF-8, so every char outside 0x20..0x7e is written `\uXXXX`
//! ([`json_str`]), which the watch's JavaScript undoes. The keys go in a
//! fixed order, `t` first, then the fields in the order the spec's table
//! gives them, so a line is always the same bytes
//! (`tests/fixtures/phone-lines.txt` at the repo root holds them, and the
//! watch's tests read that file).
//!
//! **Watch to phone** is a Gadgetbridge intent message, which Gadgetbridge
//! turns into a broadcast to `WatchReceiver` with string extras. Over the
//! direct link the phone reads the same message off the console itself
//! ([`extras_from_line`]). Either way it ends up as a map of extras, which
//! [`parse_event`] checks and turns into a [`WatchEvent`].
//!
//! The receiver is exported (Gadgetbridge has to reach it), so any app on
//! the phone can send it anything: nothing from the watch is used before
//! [`parse_event`] has checked it.

use std::collections::BTreeMap;
use std::fmt;

/// The phone app's package, which the watch's intents name.
pub const PACKAGE: &str = "com.cbishopvelti.beep";
/// The action of the watch's intents to the phone.
pub const WATCH_ACTION: &str = "com.cbishopvelti.beep.WATCH";
/// Gadgetbridge's broadcast to pass a line to the watch (extra `line`).
pub const GB_ACTION: &str = "com.banglejs.uart.tx";
/// The watch's history of results, a plain file.
pub const HISTORY: &str = "beeptest.json";
/// The longest log the phone takes, bytes.
pub const MAX_LOG: u64 = 4 * 1024 * 1024;
/// The longest history the phone takes, bytes.
pub const MAX_HISTORY: u64 = 64 * 1024;
/// The most a `log` piece carries, bytes.
pub const MAX_PIECE: usize = 1000;
/// The extras the watch's events can have: what `WatchReceiver` reads off
/// the broadcast (Android has no way to list a bundle's string extras that
/// is any simpler) into the map for [`parse_event`].
pub const EXTRAS: [&str; 12] = [
    "event", "v", "m", "gps", "run", "hist", "del", "f", "at", "crc", "d", "len",
];

/// A file's length and CRC32, as `state`'s `hist` gives the history's:
/// `"<length>:<crc>"`, the CRC unsigned decimal. `"0:0"` is no history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hist {
    pub len: u64,
    pub crc: u32,
}

impl Hist {
    /// The length and CRC32 of `bytes`.
    pub fn of(bytes: &[u8]) -> Hist {
        Hist {
            len: bytes.len() as u64,
            crc: crc32(bytes),
        }
    }

    /// Reads `"<length>:<crc>"`: two unsigned decimals, nothing else.
    pub fn parse(s: &str) -> Option<Hist> {
        todo!()
    }
}

impl fmt::Display for Hist {
    /// `"<length>:<crc>"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.len, self.crc)
    }
}

/// CRC32 as the watch's `E.CRC32` and zlib compute it (IEEE 802.3,
/// reflected, polynomial 0xEDB88320, starting from and finished with
/// 0xFFFFFFFF).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc = CRC_TABLE[((crc ^ u32::from(b)) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

/// Whether `f` names a test's log: `^beeptest\.\d+\.csv$`.
pub fn is_log_name(f: &str) -> bool {
    todo!()
}

/// Whether `f` is a file the phone may ask for: a log, or [`HISTORY`].
pub fn is_file_name(f: &str) -> bool {
    todo!()
}

/// `s` as a JSON string, quotes included, in ASCII: `"`, `\` and the
/// control characters escaped as JSON does (`\n`, `\r`, `\t`, else
/// `\u00XX`), and everything from 0x7f up as `\uXXXX` (UTF-16 units, so a
/// char past U+FFFF is two), as blink's `engine::json_str`.
pub fn json_str(s: &str) -> String {
    todo!()
}

/// `\x10GB({"t":"beeptest-sync","del":[...]})\n`: asks the watch to delete
/// the results named in `del` (log file names) and answer `state`. `del` is
/// left out when there are none.
pub fn sync_line(del: &[String]) -> String {
    todo!()
}

/// `\x10GB({"t":"beeptest-get","f":"...","at":N})\n`: asks the watch to
/// stream file `f` from byte `at` as `log` events, then `logend`.
pub fn get_line(f: &str, at: u64) -> String {
    todo!()
}

/// `\x10GB({"t":"beeptest-set","m":20,"gps":true})\n`: changes the watch's
/// course and GPS setting; either is left out when `None`.
pub fn set_line(m: Option<u32>, gps: Option<bool>) -> String {
    todo!()
}

/// What the watch said in a `state` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// The watch app's version (`v`, from `beeptest.info`).
    pub version: String,
    /// The course, 20 or 15.
    pub m: u32,
    /// Whether the watch records GPS speed.
    pub gps: bool,
    /// The running test's log file name; `None` for `""`.
    pub run: Option<String>,
    /// The history's length and CRC32 as stored on the watch.
    pub hist: Hist,
    /// Log file names of results deleted on the watch, newest last, at most
    /// 30 (`""` is none).
    pub del: Vec<String>,
}

/// A `log` event: a piece of a file asked for with [`get_line`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub f: String,
    /// The piece's offset in the file.
    pub at: u64,
    /// The watch's `E.CRC32` of the piece.
    pub crc: u32,
    /// The piece's bytes: each char of the `d` extra as one byte (the
    /// watch's strings are bytes, and the logs and history are ASCII).
    pub d: Vec<u8>,
}

impl Piece {
    /// Whether the piece's CRC is right: [`crc32`] of `d` is `crc`.
    pub fn crc_ok(&self) -> bool {
        crc32(&self.d) == self.crc
    }
}

/// An event from the watch, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    State(State),
    Log(Piece),
    /// After the last piece: the file's total length, or `None` when the
    /// watch hasn't got it (`len` `-1`).
    LogEnd {
        f: String,
        len: Option<u64>,
    },
    /// A `get` while a test is running: nothing was sent.
    Busy {
        f: String,
    },
}

/// Why an event was refused, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// Checks the extras of a watch event (all strings, as Gadgetbridge gives
/// them) and reads them. Refused, with why:
///
/// - an `event` that isn't `state`, `log`, `logend` or `busy`;
/// - a missing extra the event needs (`state`: `v`, `m`, `gps`, `run`,
///   `hist`, `del`; `log`: `f`, `at`, `crc`, `d`; `logend`: `f`, `len`;
///   `busy`: `f`); extras it doesn't need are ignored;
/// - `f`, `run` (unless `""`) or a name in `del` that isn't a log name
///   ([`is_log_name`]; `f` may also be [`HISTORY`]);
/// - `m` not `"20"` or `"15"`, `gps` not `"true"` or `"false"`, `hist` not
///   `"<length>:<crc>"`, `at` or `crc` not an unsigned decimal (`crc` at
///   most 0xFFFFFFFF), `len` not one or `"-1"`;
/// - `d` longer than [`MAX_PIECE`] bytes, or with a char past U+00FF;
/// - `v` longer than 32 chars or not printable ASCII.
pub fn parse_event(extras: &BTreeMap<String, String>) -> Result<WatchEvent, Refused> {
    todo!()
}

/// Reads a line the watch printed over the direct link: an intent message
/// for this app (`t` `intent`, `target` `broadcastreceiver`, `action`
/// [`WATCH_ACTION`]), as Gadgetbridge reads them, into the extras it would
/// have put on its broadcast. A non-string extra becomes its JSON text, as
/// blink's `watch_ble::parse`. The console's prompt (`>`) and a trailing
/// `\r` are trimmed first. Anything else (Android Integration's own
/// messages, errors, the prompt alone) is `None`.
pub fn extras_from_line(line: &str) -> Option<BTreeMap<String, String>> {
    todo!()
}

/// Reads the watch's history (`beeptest.json`, as `lib.js` `stop` writes
/// it: a JSON array of entries, newest first) into its entries, in the
/// watch's order. An entry whose `f` isn't a log name or whose numbers are
/// missing or don't parse is skipped (numbers are rounded to whole ones);
/// text that isn't a JSON array is refused.
pub fn parse_history(bytes: &[u8]) -> Result<Vec<crate::store::Entry>, Refused> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_is_zlibs() {
        // The standard check value, and what zlib.crc32 gives.
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"a"), 0xe8b7_be43);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414f_a339
        );
    }
}
