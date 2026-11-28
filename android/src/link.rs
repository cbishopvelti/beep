//! The watch link's wire format, from the phone's side (the watch's side is
//! `sync.js`, which `beeptest.boot.js` hands the phone's messages to;
//! `docs/DEVELOPMENT.md`, "The watch link", has both).
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
//! [`parse_event`] has checked it, and the names it lets through can't
//! leave the store's `logs/` ([`is_log_name`]).

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
/// The most names `state`'s `del` has: the watch remembers its last 30
/// deletions (`sync.js` `KEEP`).
pub const MAX_DEL: usize = 30;
/// The extras the watch's events can have: what `WatchReceiver` reads off
/// the broadcast (Android has no way to list a bundle's string extras that
/// is any simpler) into the map for [`parse_event`].
pub const EXTRAS: [&str; 12] = [
    "event", "v", "m", "gps", "run", "hist", "del", "f", "at", "crc", "d", "len",
];
/// The most digits a log's name may have: the watch names them by the
/// test's start in Unix seconds (10 digits until 2286). Any more and the
/// name isn't taken, so a file name stays short.
const MAX_NAME_DIGITS: usize = 20;
/// The longest `v` taken.
const MAX_VERSION: usize = 32;

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
        let (len, crc) = s.split_once(':')?;
        Some(Hist {
            len: unsigned(len)?,
            crc: u32::try_from(unsigned(crc)?).ok()?,
        })
    }
}

impl fmt::Display for Hist {
    /// `"<length>:<crc>"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.len, self.crc)
    }
}

/// An unsigned decimal: ASCII digits only (no sign, no space), at most
/// `u64::MAX`.
fn unsigned(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
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

/// Whether `f` names a test's log: `^beeptest\.\d+\.csv$` (ASCII digits,
/// as JavaScript's `\d`), with at most 20 digits. Such a name has no `/`
/// and isn't `..`, so `logs/<f>` stays in `logs/`.
pub fn is_log_name(f: &str) -> bool {
    f.strip_prefix("beeptest.")
        .and_then(|rest| rest.strip_suffix(".csv"))
        .is_some_and(|digits| {
            !digits.is_empty()
                && digits.len() <= MAX_NAME_DIGITS
                && digits.bytes().all(|b| b.is_ascii_digit())
        })
}

/// Whether `f` is a file the phone may ask for: a log, or [`HISTORY`].
pub fn is_file_name(f: &str) -> bool {
    f == HISTORY || is_log_name(f)
}

/// `s` as a JSON string, quotes included, in ASCII: `"`, `\` and the
/// control characters escaped as JSON does (`\n`, `\r`, `\t`, else
/// `\u00XX`), and everything from 0x7f up as `\uXXXX` (UTF-16 units, so a
/// char past U+FFFF is two), as blink's `engine::json_str`.
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Gadgetbridge sends the line as ISO-8859-1 bytes, which turns
            // anything past U+00FF into "?", and the watch reads bytes from
            // 0x80 up as the start of UTF-8. So the line stays ASCII: the
            // watch evaluates GB(...) as JavaScript, which undoes \u escapes.
            c if (c as u32) < 0x20 || (c as u32) >= 0x7f => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `\x10GB({"t":"beeptest-sync","del":[...]})\n`: asks the watch to delete
/// the results named in `del` (log file names) and answer `state`. `del` is
/// left out when there are none.
pub fn sync_line(del: &[String]) -> String {
    let mut json = String::from("{\"t\":\"beeptest-sync\"");
    if !del.is_empty() {
        let names: Vec<String> = del.iter().map(|f| json_str(f)).collect();
        json.push_str(&format!(",\"del\":[{}]", names.join(",")));
    }
    json.push('}');
    gb(&json)
}

/// `\x10GB({"t":"beeptest-get","f":"...","at":N})\n`: asks the watch to
/// stream file `f` from byte `at` as `log` events, then `logend`.
pub fn get_line(f: &str, at: u64) -> String {
    gb(&format!(
        "{{\"t\":\"beeptest-get\",\"f\":{},\"at\":{at}}}",
        json_str(f)
    ))
}

/// `\x10GB({"t":"beeptest-set","m":20,"gps":true})\n`: changes the watch's
/// course and GPS setting; either is left out when `None`.
pub fn set_line(m: Option<u32>, gps: Option<bool>) -> String {
    let mut json = String::from("{\"t\":\"beeptest-set\"");
    if let Some(m) = m {
        json.push_str(&format!(",\"m\":{m}"));
    }
    if let Some(gps) = gps {
        json.push_str(&format!(",\"gps\":{gps}"));
    }
    json.push('}');
    gb(&json)
}

/// The console line that hands `json` to the watch's `GB()`: Ctrl-P first,
/// which keeps the console from echoing it.
fn gb(json: &str) -> String {
    format!("\u{10}GB({json})\n")
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
    /// The length and CRC32 of the history as the watch sends it
    /// (`JSON.stringify` of its `beeptest.json`).
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

/// A value for a refusal's message: quoted, and cut short (it can be
/// anything another app sent).
fn shown(s: &str) -> String {
    let cut: String = s.chars().take(40).collect();
    if cut.len() < s.len() {
        format!("{cut:?}...")
    } else {
        format!("{cut:?}")
    }
}

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
/// - `del` with more than [`MAX_DEL`] names;
/// - `d` longer than [`MAX_PIECE`] bytes, or with a char past U+00FF;
/// - `v` longer than 32 chars or not printable ASCII.
pub fn parse_event(extras: &BTreeMap<String, String>) -> Result<WatchEvent, Refused> {
    let event = extras
        .get("event")
        .ok_or_else(|| Refused("no event".into()))?;
    let get = |key: &str| {
        extras
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| Refused(format!("{event}: no {key}")))
    };
    let bad = |key: &str, value: &str| Refused(format!("{event}: {key} is {}", shown(value)));
    let file = |key: &str| {
        let f = get(key)?;
        if is_file_name(f) {
            Ok(f.to_string())
        } else {
            Err(bad(key, f))
        }
    };
    match event.as_str() {
        "state" => {
            let version = get("v")?;
            if version.len() > MAX_VERSION || !version.bytes().all(|b| (0x20..0x7f).contains(&b)) {
                return Err(bad("v", version));
            }
            let m = match get("m")? {
                "20" => 20,
                "15" => 15,
                other => return Err(bad("m", other)),
            };
            let gps = match get("gps")? {
                "true" => true,
                "false" => false,
                other => return Err(bad("gps", other)),
            };
            let run = match get("run")? {
                "" => None,
                f if is_log_name(f) => Some(f.to_string()),
                other => return Err(bad("run", other)),
            };
            let hist = get("hist")?;
            let hist = Hist::parse(hist).ok_or_else(|| bad("hist", hist))?;
            let del = match get("del")? {
                "" => Vec::new(),
                list if list.split(',').count() > MAX_DEL => {
                    return Err(Refused(format!(
                        "state: del has {} names, more than {MAX_DEL}",
                        list.split(',').count()
                    )));
                }
                list => list
                    .split(',')
                    .map(|f| {
                        if is_log_name(f) {
                            Ok(f.to_string())
                        } else {
                            Err(bad("del", f))
                        }
                    })
                    .collect::<Result<_, _>>()?,
            };
            Ok(WatchEvent::State(State {
                version: version.to_string(),
                m,
                gps,
                run,
                hist,
                del,
            }))
        }
        "log" => {
            let f = file("f")?;
            let at = get("at")?;
            let at = unsigned(at).ok_or_else(|| bad("at", at))?;
            let crc = get("crc")?;
            let crc = unsigned(crc)
                .and_then(|c| u32::try_from(c).ok())
                .ok_or_else(|| bad("crc", crc))?;
            let text = get("d")?;
            if text.chars().count() > MAX_PIECE {
                return Err(Refused(format!(
                    "log: d is {} chars, more than {MAX_PIECE}",
                    text.chars().count()
                )));
            }
            let d = text
                .chars()
                .map(|c| u8::try_from(u32::from(c)).ok())
                .collect::<Option<Vec<u8>>>()
                .ok_or_else(|| Refused("log: d has a char past U+00FF".into()))?;
            Ok(WatchEvent::Log(Piece { f, at, crc, d }))
        }
        "logend" => {
            let f = file("f")?;
            let len = match get("len")? {
                "-1" => None,
                len => Some(unsigned(len).ok_or_else(|| bad("len", len))?),
            };
            Ok(WatchEvent::LogEnd { f, len })
        }
        "busy" => Ok(WatchEvent::Busy { f: file("f")? }),
        other => Err(Refused(format!("event {}", shown(other)))),
    }
}

/// Reads a line the watch printed over the direct link: an intent message
/// for this app (`t` `intent`, `target` `broadcastreceiver`, `action`
/// [`WATCH_ACTION`]), as Gadgetbridge reads them, into the extras it would
/// have put on its broadcast. A non-string extra becomes its JSON text, as
/// blink's `watch_ble::parse`. The console's prompt (`>`) and a trailing
/// `\r` are trimmed first. Anything else (Android Integration's own
/// messages, errors, the prompt alone) is `None`.
pub fn extras_from_line(line: &str) -> Option<BTreeMap<String, String>> {
    let line = line.trim_start_matches(['>', ' ']).trim_end_matches('\r');
    if !line.starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v["t"] != "intent" || v["target"] != "broadcastreceiver" || v["action"] != WATCH_ACTION {
        return None;
    }
    let extra = v["extra"].as_object()?;
    Some(
        extra
            .iter()
            .map(|(k, v)| {
                let s = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                (k.clone(), s)
            })
            .collect(),
    )
}

/// Reads the watch's history (`beeptest.json`, as `lib.js` `stop` writes
/// it: a JSON array of entries, newest first) into its entries, in the
/// watch's order. An entry whose `f` isn't a log name or whose numbers are
/// missing or don't parse is skipped (numbers are rounded to whole ones,
/// and must fit: `t` within a `Date`'s range, the rest 0 to 2^32 - 1), as
/// is a second entry with the same `f`; text that isn't a JSON array, or is
/// longer than [`MAX_HISTORY`], is refused.
pub fn parse_history(bytes: &[u8]) -> Result<Vec<crate::store::Entry>, Refused> {
    if bytes.len() as u64 > MAX_HISTORY {
        return Err(Refused(format!(
            "the history is {} bytes, more than {MAX_HISTORY}",
            bytes.len()
        )));
    }
    let v: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| Refused(format!("the history isn't JSON: {e}")))?;
    let list = v
        .as_array()
        .ok_or_else(|| Refused("the history isn't a list".into()))?;
    let mut out: Vec<crate::store::Entry> = Vec::new();
    for item in list {
        let Some(entry) = history_entry(item) else {
            log::warn!("history: skipped {}", shown(&item.to_string()));
            continue;
        };
        if out.iter().any(|e| e.f == entry.f) {
            log::warn!("history: {} twice", entry.f);
            continue;
        }
        out.push(entry);
    }
    Ok(out)
}

fn history_entry(v: &serde_json::Value) -> Option<crate::store::Entry> {
    let f = v["f"].as_str().filter(|f| is_log_name(f))?;
    let num = |key: &str| v[key].as_f64().filter(|n| n.is_finite()).map(|n| n.round());
    let small = |key: &str| {
        num(key)
            .filter(|n| (0.0..=f64::from(u32::MAX)).contains(n))
            .map(|n| n as u32)
    };
    let t = num("t").filter(|t| t.abs() <= 8.64e15)? as i64;
    Some(crate::store::Entry {
        f: f.to_string(),
        t,
        m: small("m")?,
        l: small("l")?,
        s: small("s")?,
        d: small("d")?,
        dur: small("dur")?,
        max: small("max")?,
        avg: small("avg")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

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

    #[test]
    fn the_lines_are_exactly_these() {
        assert_eq!(sync_line(&[]), "\u{10}GB({\"t\":\"beeptest-sync\"})\n");
        assert_eq!(
            sync_line(&["beeptest.1.csv".into(), "beeptest.22.csv".into()]),
            "\u{10}GB({\"t\":\"beeptest-sync\",\"del\":[\"beeptest.1.csv\",\"beeptest.22.csv\"]})\n"
        );
        assert_eq!(
            get_line(HISTORY, 0),
            "\u{10}GB({\"t\":\"beeptest-get\",\"f\":\"beeptest.json\",\"at\":0})\n"
        );
        assert_eq!(
            get_line("beeptest.1781344800.csv", 2000),
            "\u{10}GB({\"t\":\"beeptest-get\",\"f\":\"beeptest.1781344800.csv\",\"at\":2000})\n"
        );
        assert_eq!(
            set_line(Some(15), Some(true)),
            "\u{10}GB({\"t\":\"beeptest-set\",\"m\":15,\"gps\":true})\n"
        );
        assert_eq!(
            set_line(None, Some(false)),
            "\u{10}GB({\"t\":\"beeptest-set\",\"gps\":false})\n"
        );
        assert_eq!(
            set_line(Some(20), None),
            "\u{10}GB({\"t\":\"beeptest-set\",\"m\":20})\n"
        );
        assert_eq!(set_line(None, None), "\u{10}GB({\"t\":\"beeptest-set\"})\n");
    }

    #[test]
    fn strings_are_escaped_into_ascii() {
        let s = json_str("a\"b\\c\nd\re\tf\u{1}g\u{7f}\u{e9}\u{2028}\u{1f600}");
        assert_eq!(
            s,
            "\"a\\\"b\\\\c\\nd\\re\\tf\\u0001g\\u007f\\u00e9\\u2028\\ud83d\\ude00\""
        );
        assert!(s.bytes().all(|b| (0x20..0x7f).contains(&b)));
        // It reads back as the same string, as the watch's JavaScript reads it.
        let back: String = serde_json::from_str(&s).unwrap();
        assert_eq!(back, "a\"b\\c\nd\re\tf\u{1}g\u{7f}\u{e9}\u{2028}\u{1f600}");
        // A line with any name stays ASCII.
        let line = get_line("caf\u{e9}", 1);
        assert!(line[1..].is_ascii() && !line[1..].contains('\u{10}'));
    }

    #[test]
    fn names_and_hists() {
        for f in ["beeptest.1781344800.csv", "beeptest.0.csv"] {
            assert!(is_log_name(f) && is_file_name(f), "{f}");
        }
        for f in [
            "beeptest..csv",
            "beeptest.12a.csv",
            "beeptest.12.csv\n",
            "../beeptest.1.csv",
            "beeptest.1.csv/..",
            "beeptest.json",
            "beeptest.\u{661}.csv",
            "beeptest.123456789012345678901.csv",
            "",
            "..",
            "/etc/passwd",
        ] {
            assert!(!is_log_name(f), "{f:?}");
        }
        assert!(is_file_name(HISTORY));
        assert!(!is_file_name("beeptest.settings.json"));

        assert_eq!(Hist::parse("0:0"), Some(Hist::default()));
        let h = Hist::parse("216:21098452").unwrap();
        assert_eq!((h.len, h.crc), (216, 21_098_452));
        assert_eq!(h.to_string(), "216:21098452");
        assert_eq!(Hist::parse("1:4294967295").unwrap().crc, u32::MAX);
        for s in [
            "1:4294967296",
            "216",
            ":1",
            "1:",
            "-1:0",
            "1:2:3",
            " 1:2",
            "+1:2",
            "18446744073709551616:0",
        ] {
            assert_eq!(Hist::parse(s), None, "{s}");
        }
        assert_eq!(Hist::of(b"[]"), Hist::parse("2:223132457").unwrap());
    }

    fn state() -> BTreeMap<String, String> {
        map(&[
            ("event", "state"),
            ("v", "0.05"),
            ("m", "20"),
            ("gps", "false"),
            ("run", ""),
            ("hist", "216:21098452"),
            ("del", "beeptest.1781085600.csv,beeptest.2.csv"),
        ])
    }

    #[test]
    fn reads_events() {
        assert_eq!(
            parse_event(&state()),
            Ok(WatchEvent::State(State {
                version: "0.05".into(),
                m: 20,
                gps: false,
                run: None,
                hist: Hist {
                    len: 216,
                    crc: 21_098_452
                },
                del: vec!["beeptest.1781085600.csv".into(), "beeptest.2.csv".into()],
            }))
        );
        let mut s = state();
        s.insert("run".into(), "beeptest.9.csv".into());
        s.insert("del".into(), "".into());
        s.insert("m".into(), "15".into());
        s.insert("gps".into(), "true".into());
        s.insert("extra".into(), "ignored".into());
        let WatchEvent::State(st) = parse_event(&s).unwrap() else {
            panic!()
        };
        assert_eq!(
            (st.run.as_deref(), st.del.len(), st.m, st.gps),
            (Some("beeptest.9.csv"), 0, 15, true)
        );

        // As many deletions as the watch remembers, and no more.
        let names = |n: u64| -> String {
            (0..n)
                .map(|i| format!("beeptest.{}.csv", 1_781_000_000 + i))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut s = state();
        s.insert("del".into(), names(MAX_DEL as u64));
        let WatchEvent::State(st) = parse_event(&s).unwrap() else {
            panic!()
        };
        assert_eq!(st.del.len(), MAX_DEL);
        s.insert("del".into(), names(MAX_DEL as u64 + 1));
        assert_eq!(
            parse_event(&s),
            Err(Refused("state: del has 31 names, more than 30".into()))
        );
        s.insert("del".into(), names(20_000));
        assert!(parse_event(&s).is_err());

        let piece = map(&[
            ("event", "log"),
            ("f", "beeptest.1.csv"),
            ("at", "1000"),
            ("crc", "3641885845"),
            ("d", "123456789\u{e9}"),
        ]);
        let WatchEvent::Log(p) = parse_event(&piece).unwrap() else {
            panic!()
        };
        assert_eq!((p.at, p.d.len(), p.d[9]), (1000, 10, 0xe9));
        // The CRC is of the bytes (each char one), as E.CRC32 of the string.
        assert!(p.crc_ok());
        let ok = Piece {
            f: "x".into(),
            at: 0,
            crc: 0xcbf4_3926,
            d: b"123456789".to_vec(),
        };
        assert!(ok.crc_ok());

        assert_eq!(
            parse_event(&map(&[
                ("event", "logend"),
                ("f", "beeptest.json"),
                ("len", "216")
            ])),
            Ok(WatchEvent::LogEnd {
                f: HISTORY.into(),
                len: Some(216)
            })
        );
        assert_eq!(
            parse_event(&map(&[
                ("event", "logend"),
                ("f", "beeptest.1.csv"),
                ("len", "-1")
            ])),
            Ok(WatchEvent::LogEnd {
                f: "beeptest.1.csv".into(),
                len: None
            })
        );
        assert_eq!(
            parse_event(&map(&[("event", "busy"), ("f", "beeptest.1.csv")])),
            Ok(WatchEvent::Busy {
                f: "beeptest.1.csv".into()
            })
        );
    }

    #[test]
    fn refuses_what_isnt_right() {
        let with = |key: &str, value: &str| {
            let mut s = state();
            s.insert(key.into(), value.into());
            s
        };
        let without = |key: &str| {
            let mut s = state();
            s.remove(key);
            s
        };
        for bad in [
            with("event", "pong"),
            with("m", "17"),
            with("m", "20 "),
            with("gps", "1"),
            with("run", "../x"),
            with("run", "beeptest.json"),
            with("hist", "x:1"),
            with("hist", "1:99999999999"),
            with("del", "beeptest.1.csv,"),
            with("del", "beeptest.1.csv,../../x"),
            with("v", &"9".repeat(33)),
            with("v", "0.05\n"),
            with("v", "\u{e9}"),
            without("event"),
            without("v"),
            without("m"),
            without("gps"),
            without("run"),
            without("hist"),
            without("del"),
        ] {
            assert!(parse_event(&bad).is_err(), "{bad:?}");
        }
        let piece = |f: &str, at: &str, crc: &str, d: &str| {
            map(&[
                ("event", "log"),
                ("f", f),
                ("at", at),
                ("crc", crc),
                ("d", d),
            ])
        };
        let f = "beeptest.1.csv";
        for bad in [
            piece("../x", "0", "0", "a"),
            piece("beeptest.settings.json", "0", "0", "a"),
            piece(f, "-1", "0", "a"),
            piece(f, "1e3", "0", "a"),
            piece(f, "99999999999999999999999", "0", "a"),
            piece(f, "0", "4294967296", "a"),
            piece(f, "0", "-5", "a"),
            piece(f, "0", "0", &"x".repeat(MAX_PIECE + 1)),
            piece(f, "0", "0", "\u{100}"),
            piece(f, "0", "0", "\u{1f600}"),
            map(&[("event", "log"), ("f", f), ("at", "0"), ("crc", "0")]),
            map(&[("event", "logend"), ("f", f), ("len", "-2")]),
            map(&[("event", "logend"), ("f", f), ("len", "")]),
            map(&[("event", "logend"), ("f", f)]),
            map(&[("event", "logend"), ("len", "1")]),
            map(&[("event", "busy"), ("f", "x")]),
            map(&[("event", "busy")]),
            map(&[]),
        ] {
            assert!(parse_event(&bad).is_err(), "{bad:?}");
        }
        assert!(parse_event(&piece(f, "0", "0", &"\u{ff}".repeat(MAX_PIECE))).is_ok());
        // What was refused, and not more of it than that.
        let why = parse_event(&with("m", &"x".repeat(5000))).unwrap_err();
        assert!(why.0.len() < 100, "{why}");
    }

    #[test]
    fn reads_the_watchs_messages() {
        // sync.js, send(): Android Integration prints a blank line, then this.
        let line = r#"{"t":"intent","target":"broadcastreceiver","action":"com.cbishopvelti.beep.WATCH","extra":{"event":"busy","f":"beeptest.1.csv"},"package":"com.cbishopvelti.beep"}"#;
        let extras = extras_from_line(line).unwrap();
        assert_eq!(extras["event"], "busy");
        assert_eq!(extras["f"], "beeptest.1.csv");
        // After the console's prompt, with a number for a string.
        let extras = extras_from_line(&format!(
            ">{}\r",
            r#"{"t":"intent","target":"broadcastreceiver","action":"com.cbishopvelti.beep.WATCH","extra":{"event":"logend","f":"beeptest.json","len":216}}"#
        ))
        .unwrap();
        assert_eq!(extras["len"], "216");
        for line in [
            "",
            ">",
            r#"{"t":"status","bat":87,"chg":0}"#,
            r#"{"t":"intent","target":"broadcastreceiver","action":"other.app","extra":{"event":"x"}}"#,
            r#"{"t":"intent","target":"activity","action":"com.cbishopvelti.beep.WATCH","extra":{"event":"x"}}"#,
            r#"{"t":"intent","target":"broadcastreceiver","action":"com.cbishopvelti.beep.WATCH"}"#,
            "Uncaught SyntaxError: Got ')' expected EOF",
            "{not json",
        ] {
            assert_eq!(extras_from_line(line), None, "{line}");
        }
    }

    #[test]
    fn reads_the_history() {
        let h = br#"[{"t":1781344800000,"m":20,"l":7,"s":3,"d":1080,"dur":400,"max":139,"avg":120,"f":"beeptest.1781344800.csv"},
            {"t":1781258400000.4,"m":15,"l":3,"s":5,"d":360,"dur":150.6,"max":139,"avg":120,"f":"beeptest.1781258400.csv"},
            {"t":1,"m":20,"l":1,"s":1,"d":20,"dur":9,"max":0,"avg":0,"f":"../x"},
            {"t":1,"m":20,"l":1,"s":1,"d":-20,"dur":9,"max":0,"avg":0,"f":"beeptest.3.csv"},
            {"t":1,"m":20,"l":1,"s":1,"d":"20","dur":9,"max":0,"avg":0,"f":"beeptest.4.csv"},
            {"t":1,"m":20,"l":1,"s":1,"dur":9,"max":0,"avg":0,"f":"beeptest.5.csv"},
            {"t":1e300,"m":20,"l":1,"s":1,"d":20,"dur":9,"max":0,"avg":0,"f":"beeptest.6.csv"},
            {"t":2,"m":20,"l":1,"s":1,"d":20,"dur":9,"max":0,"avg":0,"f":"beeptest.1781344800.csv"},
            null, 5, "x"]"#;
        let entries = parse_history(h).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            crate::store::Entry {
                f: "beeptest.1781344800.csv".into(),
                t: 1_781_344_800_000,
                m: 20,
                l: 7,
                s: 3,
                d: 1080,
                dur: 400,
                max: 139,
                avg: 120,
            }
        );
        assert_eq!((entries[1].t, entries[1].dur), (1_781_258_400_000, 151));
        assert_eq!(parse_history(b"[]"), Ok(vec![]));
        for bad in [&b"{}"[..], b"", b"[", b"\xff", b"null"] {
            assert!(parse_history(bad).is_err(), "{bad:?}");
        }
        let big = format!("[{}]", " ".repeat(MAX_HISTORY as usize));
        assert!(parse_history(big.as_bytes()).is_err());
        let deep = format!("{}{}", "[".repeat(30_000), "]".repeat(30_000));
        assert!(parse_history(deep.as_bytes()).is_err());
    }
}
