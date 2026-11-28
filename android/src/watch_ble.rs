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
        todo!()
    }

    /// Writes the setting's keys into `watch.json`'s object, leaving its
    /// other keys alone (`address` and `name` removed when no watch is
    /// picked).
    pub fn write_json(&self, object: &mut serde_json::Map<String, serde_json::Value>) {
        todo!()
    }
}

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where the setting lives: the store's directory. Set once, when the
/// process's store opens (`crate::platform::open`).
pub fn init(files_dir: &Path) {
    let _ = DIR.set(files_dir.to_path_buf());
}

/// The setting, read from `watch.json` once and then kept in memory (the
/// default before [`init`]).
pub fn settings() -> Settings {
    todo!()
}

/// Saves the setting ([`crate::store::update_json`], so the watch's state
/// in the same file stays) and keeps it in memory. An error before
/// [`init`].
pub fn save(s: &Settings) -> std::io::Result<()> {
    todo!()
}

/// Whether the watch's lines go over the phone's own link rather than to
/// Gadgetbridge.
pub fn direct() -> bool {
    todo!()
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
    todo!()
}

impl Status {
    /// What the Settings page says about it: "Through Gadgetbridge",
    /// "Connected", "Bluetooth is off", ...
    pub fn words(self) -> &'static str {
        todo!()
    }

    /// What the Watch page says about it, for the watch called `name`:
    /// "Connected directly to Bangle.js 1a2b", ...
    pub fn sentence(self, name: &str) -> String {
        todo!()
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
    todo!()
}

/// A device the scan saw: kept if it's a watch (one entry an address),
/// strongest first, then by name.
pub fn found(address: &str, name: &str, rssi: i32) {
    todo!()
}

/// The watches found since [`forget_found`], strongest first.
pub fn found_watches() -> Vec<Found> {
    todo!()
}

pub fn forget_found() {
    todo!()
}

/// The bytes for `text`, as Gadgetbridge writes them: ISO-8859-1, with
/// anything past U+00FF as "?". The phone's lines are ASCII anyway
/// (`crate::link::json_str`).
pub fn encode(text: &str) -> Vec<u8> {
    todo!()
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
        todo!()
    }

    /// The next write, if one can go now: none while one is under way
    /// ([`Writer::written`] not yet called), while paused, or with nothing
    /// waiting.
    pub fn next_write(&mut self) -> Option<Vec<u8>> {
        todo!()
    }

    /// The write went through (or failed for good: it isn't tried again).
    pub fn written(&mut self) {
        todo!()
    }

    /// The write couldn't start (the stack was busy): it goes again first.
    pub fn retry(&mut self, chunk: Vec<u8>) {
        todo!()
    }

    /// The watch's XOFF (true) or XON (false).
    pub fn pause(&mut self, paused: bool) {
        todo!()
    }

    /// The MTU the watch and phone agreed: 3 bytes of it are the header.
    /// A write is that less 3, from [`SMALLEST_CHUNK`] to 512.
    pub fn set_mtu(&mut self, mtu: i32) {
        todo!()
    }

    /// Disconnected: nothing waiting goes to the next connection, which
    /// starts at [`SMALLEST_CHUNK`] until its MTU is agreed.
    pub fn clear(&mut self) {
        todo!()
    }

    /// Bytes waiting.
    pub fn waiting(&self) -> usize {
        todo!()
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
        todo!()
    }

    pub fn clear(&mut self) {
        todo!()
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
    todo!()
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
        todo!()
    }
}

/// How long a connection waiting for the watch (`autoConnect`) is left
/// before it's made again: after a reboot or Bluetooth turned off and on,
/// one can wait for good without ever connecting.
pub const WAIT_AT_MOST: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// How far the phone's time zone is ahead of UTC at `unix`, in seconds
/// (`localtime_r`'s `tm_gmtoff`; 0 if that fails).
pub fn utc_offset(unix: i64) -> i32 {
    todo!()
}
