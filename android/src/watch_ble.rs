//! The direct Bluetooth link to the watch: an option in Settings, off by
//! default, where the phone connects to the Bangle.js itself instead of
//! going through Gadgetbridge. Ported from blink's `watch_ble.rs`.
//!
//! It speaks what Gadgetbridge speaks, so the watch app is the same either
//! way: the Nordic UART service the watch's console runs on. The phone
//! writes the same lines it gives Gadgetbridge (`\x10GB({...})\n`,
//! `crate::link`) into the console, and reads what the watch prints back, a
//! line at a time ([`Reader`]): the watch's messages to the phone are the
//! JSON intent messages Gadgetbridge would have turned into broadcasts
//! (`crate::link::extras_from_line`). The watch holds one phone connection
//! at a time, so Gadgetbridge has to let go of it first.
//!
//! What Gadgetbridge did besides, the phone does here for itself: it sets
//! the watch's clock as it connects ([`CLEAR`], then [`hello`]), and again
//! after a time zone change or the clock being set ([`ClockSent`]). Then
//! it dispatches `crate::sync::Input::LinkConnected`, which syncs. Unlike
//! blink, no notifications are forwarded: the Beep Test sends the watch
//! none.
//!
//! This file is the part that doesn't depend on Android: the setting, what
//! the Settings and Watch pages show, and the bytes on the wire. The
//! Bluetooth calls are in `platform/android/ble.rs`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The watch's console: Nordic's UART service.
pub const UART_SERVICE: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
/// What the phone writes: the console's input.
pub const UART_WRITE: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";
/// What the watch prints, as notifications.
pub const UART_NOTIFY: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";
/// The descriptor that turns notifications on.
pub const CLIENT_CONFIG: &str = "00002902-0000-1000-8000-00805f9b34fb";
/// The MTU the phone asks for: Bangle.js 2 takes up to 131 bytes, so a
/// write carries 128. Without it, 20.
pub const MTU: i32 = 131;
/// A write's size until the MTU is agreed.
pub const SMALLEST_CHUNK: usize = 20;
/// The console's flow control: the watch prints XOFF when its input buffer
/// is filling, and XON when there's room again.
pub const XON: u8 = 0x11;
pub const XOFF: u8 = 0x13;

/// The setting, kept in `watch.json` in the app's files directory (keys
/// `direct`, `address`, `name`; the rest of the file is the store's
/// `WatchState`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// Connect to the watch directly, not through Gadgetbridge.
    pub direct: bool,
    /// The watch picked, by its Bluetooth address, and its name.
    pub watch: Option<Watch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    pub address: String,
    pub name: String,
}

impl Settings {
    /// Reads the setting's keys from `watch.json`'s object (anything
    /// missing is the default; an empty address is no watch).
    pub fn from_json(v: &serde_json::Value) -> Settings {
        let watch = match (v["address"].as_str(), v["name"].as_str()) {
            (Some(address), name) if !address.is_empty() => Some(Watch {
                address: address.to_string(),
                name: name.unwrap_or(address).to_string(),
            }),
            _ => None,
        };
        Settings {
            direct: v["direct"].as_bool().unwrap_or(false),
            watch,
        }
    }

    /// Writes the setting's keys into `watch.json`'s object, leaving its
    /// other keys alone (`address` and `name` removed when no watch is
    /// picked).
    pub fn write_json(&self, object: &mut serde_json::Map<String, serde_json::Value>) {
        object.insert("direct".into(), self.direct.into());
        match &self.watch {
            Some(w) => {
                object.insert("address".into(), w.address.clone().into());
                object.insert("name".into(), w.name.clone().into());
            }
            None => {
                object.remove("address");
                object.remove("name");
            }
        }
    }
}

static DIR: OnceLock<PathBuf> = OnceLock::new();
static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);

/// `watch.json` in the store's directory (none before [`init`]).
fn file() -> Option<PathBuf> {
    DIR.get().map(|d| d.join("watch.json"))
}

/// Where the setting lives: the store's directory. Set once, when the
/// process's store opens (`crate::platform::open`).
pub fn init(files_dir: &Path) {
    let _ = DIR.set(files_dir.to_path_buf());
}

/// The setting, read from `watch.json` once and then kept in memory (the
/// default before [`init`]).
pub fn settings() -> Settings {
    let mut cached = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = cached.as_ref() {
        return s.clone();
    }
    let Some(path) = file() else {
        return Settings::default();
    };
    let s = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .map(|v| Settings::from_json(&v))
        .unwrap_or_default();
    *cached = Some(s.clone());
    s
}

/// Saves the setting ([`crate::store::update_json`], so the watch's state
/// in the same file stays) and keeps it in memory. An error before
/// [`init`].
pub fn save(s: &Settings) -> std::io::Result<()> {
    let path = file().ok_or_else(|| std::io::Error::other("no files directory yet"))?;
    crate::store::update_json(&path, |o| s.write_json(o))?;
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
    Ok(())
}

/// Whether the watch's lines go over the phone's own link rather than to
/// Gadgetbridge.
pub fn direct() -> bool {
    settings().direct
}

/// Where the direct link is, for the Settings and Watch pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// Not in use: the watch goes through Gadgetbridge.
    #[default]
    Off,
    /// No Bluetooth here (the desktop).
    Unsupported,
    /// The phone hasn't allowed the app to use Bluetooth ("Nearby devices").
    NotAllowed,
    BluetoothOff,
    /// Direct, but no watch picked.
    NoWatch,
    /// Waiting for the watch: out of range, off, or held by Gadgetbridge
    /// (Android keeps trying until it shows up).
    Connecting,
    /// Connected, setting up the console.
    SettingUp,
    Connected,
}

static STATUS: Mutex<Status> = Mutex::new(Status::Off);

/// The link's status now.
pub fn status() -> Status {
    *STATUS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sets the link's status (logged when it changes). The caller tells the
/// UI (`crate::platform::changed`).
pub fn set_status(s: Status) {
    let mut status = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    if *status != s {
        log::info!("watch link: {s:?}");
        *status = s;
    }
}

impl Status {
    /// What the Settings page says about it: "Through Gadgetbridge",
    /// "Connected", "Bluetooth is off", ...
    pub fn words(self) -> &'static str {
        match self {
            Status::Off => "Through Gadgetbridge",
            Status::Unsupported => "No Bluetooth on this device",
            Status::NotAllowed => "Needs the Nearby devices permission",
            Status::BluetoothOff => "Bluetooth is off",
            Status::NoWatch => "No watch picked",
            Status::Connecting => "Looking for it...",
            Status::SettingUp => "Connecting...",
            Status::Connected => "Connected",
        }
    }

    /// What the Watch page says about it, for the watch called `name`:
    /// "Connected directly to Bangle.js 1a2b", ...
    pub fn sentence(self, name: &str) -> String {
        match self {
            Status::Off => "Through Gadgetbridge".into(),
            Status::Unsupported => "Set to connect directly, but there's no Bluetooth here".into(),
            Status::NotAllowed => {
                "Set to connect directly, but Beep Test isn't allowed Nearby devices".into()
            }
            Status::BluetoothOff => "Set to connect directly, but Bluetooth is off".into(),
            Status::NoWatch => "Set to connect directly, but no watch is picked in Settings".into(),
            Status::Connecting => format!(
                "Looking for {name}: the phone connects once it's near and no other phone \
                 holds it"
            ),
            Status::SettingUp => format!("Connecting to {name}..."),
            Status::Connected => format!("Connected directly to {name}"),
        }
    }
}

/// A watch the scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub address: String,
    pub name: String,
    /// Signal strength, dBm.
    pub rssi: i32,
}

static FOUND: Mutex<Vec<Found>> = Mutex::new(Vec::new());

/// Whether a device the scan saw is a Bangle.js: they advertise as
/// "Bangle.js 1a2b", the end of their address.
pub fn is_watch(name: &str) -> bool {
    name.starts_with("Bangle.js")
}

/// A device the scan saw: kept if it's a watch (one entry an address),
/// strongest first, then by name.
pub fn found(address: &str, name: &str, rssi: i32) {
    if !is_watch(name) {
        return;
    }
    let mut all = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|f| f.address != address);
    all.push(Found {
        address: address.to_string(),
        name: name.to_string(),
        rssi,
    });
    all.sort_by(|a, b| b.rssi.cmp(&a.rssi).then(a.name.cmp(&b.name)));
}

/// The watches found since [`forget_found`], strongest first.
pub fn found_watches() -> Vec<Found> {
    FOUND.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn forget_found() {
    FOUND.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// The bytes for `text`, as Gadgetbridge writes them: ISO-8859-1, with
/// anything past U+00FF as "?". The phone's lines are ASCII anyway
/// (`crate::link::json_str`).
pub fn encode(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| if (c as u32) <= 0xff { c as u8 } else { b'?' })
        .collect()
}

/// What's waiting to be written, in pieces one write long, one write at a
/// time (each waits for the watch to take it), and none while the watch
/// says to wait (XOFF).
#[derive(Debug)]
pub struct Writer {
    queue: VecDeque<u8>,
    chunk: usize,
    busy: bool,
    paused: bool,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub const fn new() -> Writer {
        Writer {
            queue: VecDeque::new(),
            chunk: SMALLEST_CHUNK,
            busy: false,
            paused: false,
        }
    }

    /// Queues `bytes` after what's waiting.
    pub fn push(&mut self, bytes: &[u8]) {
        self.queue.extend(bytes);
    }

    /// The next write, if one can go now: none while one is under way
    /// ([`Writer::written`] not yet called), while paused, or with nothing
    /// waiting.
    pub fn next_write(&mut self) -> Option<Vec<u8>> {
        if self.busy || self.paused || self.queue.is_empty() {
            return None;
        }
        let n = self.chunk.min(self.queue.len());
        self.busy = true;
        Some(self.queue.drain(..n).collect())
    }

    /// The write went through (or failed for good: it isn't tried again).
    pub fn written(&mut self) {
        self.busy = false;
    }

    /// The write couldn't start (the stack was busy): it goes again first.
    pub fn retry(&mut self, chunk: Vec<u8>) {
        for b in chunk.into_iter().rev() {
            self.queue.push_front(b);
        }
        self.busy = false;
    }

    /// The watch's XOFF (true) or XON (false).
    pub fn pause(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// The MTU the watch and phone agreed: 3 bytes of it are the header.
    /// A write is that less 3, from [`SMALLEST_CHUNK`] to 512.
    pub fn set_mtu(&mut self, mtu: i32) {
        self.chunk = (mtu.max(0) as usize)
            .saturating_sub(3)
            .clamp(SMALLEST_CHUNK, 512);
    }

    /// Disconnected: nothing waiting goes to the next connection, which
    /// starts at [`SMALLEST_CHUNK`] until its MTU is agreed.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.busy = false;
        self.paused = false;
        // The next connection agrees its own MTU, or keeps to the smallest.
        self.chunk = SMALLEST_CHUNK;
    }

    /// Bytes waiting.
    pub fn waiting(&self) -> usize {
        self.queue.len()
    }
}

/// What came from the watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// A line it printed, without the `\n` (a `\r` stays).
    Line(String),
    /// Flow control: false to stop writing (XOFF), true to go on (XON).
    Go(bool),
}

/// Splits what the watch prints into lines.
#[derive(Debug, Default)]
pub struct Reader {
    buf: Vec<u8>,
}

impl Reader {
    pub const fn new() -> Reader {
        Reader { buf: Vec::new() }
    }

    /// What `bytes` complete, in order. XON and XOFF are taken out wherever
    /// they come; a line of over 16 KB with no newline is thrown away.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Read> {
        let mut out = Vec::new();
        for &b in bytes {
            match b {
                XON => out.push(Read::Go(true)),
                XOFF => out.push(Read::Go(false)),
                b'\n' => {
                    let line = String::from_utf8_lossy(&self.buf).into_owned();
                    self.buf.clear();
                    out.push(Read::Line(line));
                }
                b => self.buf.push(b),
            }
        }
        // A runaway line (something printing without newlines) isn't kept.
        if self.buf.len() > 16 * 1024 {
            self.buf.clear();
        }
        out
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

/// What the phone writes first as the watch connects, as Gadgetbridge does:
/// a Ctrl-C, which clears anything half typed at the console (a line cut
/// off when the last connection dropped). It goes on its own, with a pause
/// before [`hello`]: the console drops what arrives with it.
pub const CLEAR: &str = " \u{3}";
/// How long after [`CLEAR`] the rest goes.
pub const AFTER_CLEAR: std::time::Duration = std::time::Duration::from_millis(300);

/// Then, as Gadgetbridge does: the time and the time zone (`offset_s` east
/// of UTC), also saved in the watch's settings. Byte for byte blink's
/// `watch_ble::hello`.
pub fn hello(unix: i64, offset_s: i32) -> String {
    let tz = f64::from(offset_s) / 3600.0;
    format!(
        "\u{10}setTime({unix});E.setTimeZone({tz});\
         (s=>s&&(s.timezone={tz},require('Storage').write('setting.json',s)))\
         (require('Storage').readJSON('setting.json',1))\n"
    )
}

/// The clock and time zone the watch was last sent ([`hello`]), to tell
/// when to send them again: Gadgetbridge does after a time zone change (a
/// DST night, or travel) or the clock being set, and the link otherwise
/// sends them only as it connects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockSent {
    /// The phone's clock then (Unix seconds).
    pub unix: i64,
    /// Its time zone then ([`utc_offset`]).
    pub offset_s: i32,
}

/// How far the clock may move from where `elapsed_s` says it should be
/// before it counts as set: more than any drift between two checks.
pub const CLOCK_SET_S: i64 = 60;

impl ClockSent {
    /// Whether the watch needs the clock again: `elapsed_s` seconds (by a
    /// clock that's never set, `Instant`) after it was sent, the phone's
    /// clock reads `unix` in a time zone `offset_s` ahead of UTC. True when
    /// the time zone changed or the clock moved more than [`CLOCK_SET_S`].
    pub fn stale(self, elapsed_s: i64, unix: i64, offset_s: i32) -> bool {
        offset_s != self.offset_s || (unix - self.unix - elapsed_s).abs() > CLOCK_SET_S
    }
}

/// How long a connection waiting for the watch (`autoConnect`) is left
/// before it's made again: after a reboot or Bluetooth turned off and on,
/// one can wait for good without ever connecting.
pub const WAIT_AT_MOST: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// How far the phone's time zone is ahead of UTC at `unix`, in seconds
/// (`localtime_r`'s `tm_gmtoff`; 0 if that fails).
pub fn utc_offset(unix: i64) -> i32 {
    let Some(t) = libc::time_t::try_from(unix).ok() else {
        return 0;
    };
    // SAFETY: localtime_r writes only into `tm`, which outlives the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_reads_back() {
        let s = Settings {
            direct: true,
            watch: Some(Watch {
                address: "C1:2B:3C:4D:5E:6F".into(),
                name: "Bangle.js 5e6f".into(),
            }),
        };
        let mut object = serde_json::Map::new();
        object.insert("version".into(), "0.05".into());
        s.write_json(&mut object);
        let v = serde_json::Value::Object(object.clone());
        assert_eq!(Settings::from_json(&v), s);
        // The watch's state in the same file is left alone.
        assert_eq!(v["version"], "0.05");
        Settings::default().write_json(&mut object);
        assert!(!object.contains_key("address") && !object.contains_key("name"));
        assert_eq!(object["version"], "0.05");
        assert_eq!(
            Settings::from_json(&serde_json::json!({})),
            Settings::default()
        );
        assert_eq!(
            Settings::from_json(&serde_json::json!({"direct": true, "address": ""})),
            Settings {
                direct: true,
                watch: None
            }
        );
        let dir = tempfile::tempdir().unwrap();
        init(dir.path());
        // Only where this test set the directory (another may have first).
        if file().is_some_and(|f| f.starts_with(dir.path())) {
            crate::store::update_json(&dir.path().join("watch.json"), |o| {
                o.insert("m".into(), 15.into());
            })
            .unwrap();
            save(&s).unwrap();
            *SETTINGS.lock().unwrap() = None;
            assert_eq!(settings(), s);
            assert!(direct());
            let file: serde_json::Value =
                serde_json::from_slice(&std::fs::read(dir.path().join("watch.json")).unwrap())
                    .unwrap();
            assert_eq!(file["m"], 15);
        }
    }

    #[test]
    fn writes_go_one_at_a_time_in_pieces() {
        let mut w = Writer::new();
        w.push(&[b'a'; 50]);
        let first = w.next_write().unwrap();
        assert_eq!(first.len(), 20);
        // The next waits for the first to go through.
        assert_eq!(w.next_write(), None);
        w.written();
        assert_eq!(w.next_write().unwrap().len(), 20);
        // Couldn't start: it goes again first.
        let again = vec![b'x'; 20];
        w.retry(again.clone());
        assert_eq!(w.next_write().unwrap(), again);
        w.written();
        assert_eq!(w.next_write().unwrap().len(), 10);
        w.written();
        assert_eq!(w.next_write(), None);

        // A bigger MTU, and the watch saying wait.
        w.set_mtu(MTU);
        w.push(&[b'b'; 300]);
        w.pause(true);
        assert_eq!(w.next_write(), None);
        w.pause(false);
        assert_eq!(w.next_write().unwrap().len(), 128);
        w.set_mtu(10);
        w.written();
        assert_eq!(w.next_write().unwrap().len(), 20);
        w.clear();
        assert_eq!(w.waiting(), 0);
        assert_eq!(w.next_write(), None);
        // The next connection starts at the smallest, until its MTU is agreed.
        w.set_mtu(MTU);
        w.clear();
        w.push(&[b'c'; 300]);
        assert_eq!(w.next_write().unwrap().len(), 20);
        assert_eq!(w.waiting(), 280);
        w.written();
        w.set_mtu(10_000);
        assert_eq!(w.next_write().unwrap().len(), 280);
    }

    #[test]
    fn reads_lines_and_flow_control() {
        let mut r = Reader::new();
        assert_eq!(r.push(b"{\"t\":\"int"), vec![]);
        assert_eq!(
            r.push(b"ent\"}\r\n\x13>"),
            vec![Read::Line("{\"t\":\"intent\"}\r".into()), Read::Go(false)]
        );
        assert_eq!(
            r.push(b"\x11\n"),
            vec![Read::Go(true), Read::Line(">".into())]
        );
        // A runaway line is thrown away.
        r.push(&[b'x'; 17 * 1024]);
        assert_eq!(r.push(b"ok\n"), vec![Read::Line("ok".into())]);
        r.push(b"half");
        r.clear();
        assert_eq!(r.push(b"\n"), vec![Read::Line(String::new())]);
    }

    /// What the watch prints over the link reads as Gadgetbridge's
    /// broadcast would (`crate::link::extras_from_line`), after the Reader.
    #[test]
    fn the_watchs_messages_come_through_the_reader() {
        let mut r = Reader::new();
        let msg = br#"{"t":"intent","target":"broadcastreceiver","action":"com.cbishopvelti.beep.WATCH","extra":{"event":"busy","f":"beeptest.1.csv"},"package":"com.cbishopvelti.beep"}"#;
        let mut bytes = b"\r\n>".to_vec();
        bytes.extend_from_slice(msg);
        bytes.extend_from_slice(b"\r\n");
        let lines: Vec<_> = r
            .push(&bytes)
            .into_iter()
            .filter_map(|read| match read {
                Read::Line(l) => crate::link::extras_from_line(&l),
                Read::Go(_) => None,
            })
            .collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["event"], "busy");
    }

    #[test]
    fn the_clock_goes_again_after_a_time_zone_change_or_the_clock_being_set() {
        let sent = ClockSent {
            unix: 1_795_000_000,
            offset_s: 0,
        };
        // An hour on, as the phone's clock says too: nothing to send.
        assert!(!sent.stale(3600, 1_795_003_600, 0));
        // A little drift between checks is not a clock set.
        assert!(!sent.stale(3600, 1_795_003_630, 0));
        // The clocks went forward an hour (a DST night), or a flight.
        assert!(sent.stale(3600, 1_795_003_600, 3600));
        // The clock set by hand, or by the network after a reboot.
        assert!(sent.stale(3600, 1_795_000_000, 0));
        assert!(sent.stale(3600, 1_795_007_200, 0));
    }

    #[test]
    fn hello_sets_the_clock_like_gadgetbridge() {
        assert_eq!(
            hello(1_795_000_000, 3600),
            "\u{10}setTime(1795000000);E.setTimeZone(1);\
             (s=>s&&(s.timezone=1,require('Storage').write('setting.json',s)))\
             (require('Storage').readJSON('setting.json',1))\n"
        );
        assert!(hello(0, -12_600).contains("E.setTimeZone(-3.5);"));
        assert!(hello(0, 0).contains("E.setTimeZone(0);"));
        assert_eq!(CLEAR, " \u{3}");
        // The offset is the phone's own, within a day either way.
        assert!(utc_offset(1_795_000_000).abs() <= 14 * 3600);
    }

    #[test]
    fn encodes_as_gadgetbridge_does() {
        assert_eq!(
            encode("a\u{10}\u{e9}\u{1f600}"),
            vec![b'a', 0x10, 0xe9, b'?']
        );
        // The phone's lines are ASCII: the bytes are the line's.
        let line = crate::link::get_line("beeptest.1.csv", 0);
        assert_eq!(encode(&line), line.as_bytes());
    }

    #[test]
    fn keeps_only_watches_strongest_first() {
        forget_found();
        found("AA", "Bangle.js 1a2b", -80);
        found("BB", "Pixel Buds", -40);
        found("CC", "Bangle.js 9f00", -60);
        found("AA", "Bangle.js 1a2b", -50);
        let names: Vec<_> = found_watches().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["Bangle.js 1a2b", "Bangle.js 9f00"]);
        forget_found();
        assert!(found_watches().is_empty());
    }

    #[test]
    fn words_for_each_status() {
        set_status(Status::Connected);
        assert_eq!(status(), Status::Connected);
        set_status(Status::Off);
        assert_eq!(status(), Status::Off);
        for s in [
            Status::Off,
            Status::Unsupported,
            Status::NotAllowed,
            Status::BluetoothOff,
            Status::NoWatch,
            Status::Connecting,
            Status::SettingUp,
            Status::Connected,
        ] {
            assert!(!s.words().is_empty());
            assert!(s.sentence("Bangle.js 1a2b").is_ascii());
        }
        assert_eq!(
            Status::Connected.sentence("Bangle.js 1a2b"),
            "Connected directly to Bangle.js 1a2b"
        );
    }
}
