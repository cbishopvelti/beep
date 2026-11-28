//! What the UI and the engine call on the phone. The process's one store and
//! sync engine live here: every input goes through [`dispatch`], which runs
//! it through [`crate::sync::Engine::handle`] under one lock and sends the
//! lines it returns ([`send_line`]). On Android the rest is JNI calls into
//! the platform (`android.rs`, and `android/ble.rs` for the direct link); on
//! the desktop it logs, so the same UI runs there.
//!
//! **Where inputs come from.**
//!
//! - The UI: [`crate::sync::Input::Opened`] on its first frame,
//!   [`crate::sync::Input::Tick`] about once a second while it's open, and
//!   Sync now, Delete and the watch's settings from the pages. On Android
//!   `android/ui_loop.rs` also dispatches `Opened` each time the app comes
//!   back to the front (`resumed` after `suspended`).
//! - The watch, through Gadgetbridge: `WatchReceiver` (Java) hands the
//!   broadcast to `Java_com_cbishopvelti_beep_WatchReceiver_nativeWatch` in
//!   `android.rs`, which opens the store if the process has just started
//!   ([`open`], with the context's files directory), reads the extras named
//!   in [`crate::link::EXTRAS`] into a map and calls [`from_watch`]. No UI
//!   is needed: the engine's answer goes out from there.
//! - The watch, over the direct link: `android/ble.rs` splits what the
//!   watch prints into lines ([`crate::watch_ble::Reader`]) and calls
//!   [`from_console`] for each; once the link is set up (Ctrl-C, then the
//!   clock) it dispatches [`crate::sync::Input::LinkConnected`].
//!
//! **How the UI hears of it.** [`generation`] goes up whenever an input
//! changed what the pages show ([`crate::sync::Output::changed`]), or the
//! direct link's status changed ([`changed`]): the UI compares it each
//! frame and reloads from the store when it moved. Every dispatch also asks
//! the UI to draw again ([`set_repaint`] gives this module the egui
//! context), so the sync's progress ([`progress`]) shows as it goes, from
//! whichever thread the input came on.

use crate::link;
use crate::store::Store;
use crate::sync::{Engine, Input, Progress};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(target_os = "android")]
pub mod android;

static STORE: Mutex<Option<Arc<Store>>> = Mutex::new(None);
static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static REPAINT: Mutex<Option<egui::Context>> = Mutex::new(None);

/// The process's one store, opened on `files_dir` the first time (the
/// activity and the broadcast receivers run in the same process and share
/// it; a later call with another directory gets the first). Also tells
/// [`crate::watch_ble`] where its setting is.
pub fn open(files_dir: &Path) -> std::io::Result<Arc<Store>> {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(store) = guard.as_ref() {
        return Ok(store.clone());
    }
    let store = Arc::new(Store::open(files_dir)?);
    crate::watch_ble::init(files_dir);
    *guard = Some(store.clone());
    Ok(store)
}

/// The store, once [`open`] has opened it.
pub fn store() -> Option<Arc<Store>> {
    STORE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Runs `input` through the sync engine and sends the watch the lines it
/// returns, in order. One input at a time, from any thread: the lines are
/// sent with the lock held, so two inputs' lines can't interleave. Then the
/// UI hears of it (see the module's docs). Before [`open`], the input is
/// logged and dropped.
pub fn dispatch(input: Input) {
    let Some(store) = store() else {
        return log::error!("input before the store opened: {input:?}");
    };
    let changed = {
        let mut engine = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        let engine = engine.get_or_insert_with(Engine::new);
        let output = engine.handle(&store, input, now_ms());
        for line in &output.lines {
            send_line(line);
        }
        output.changed
    };
    if changed {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
    repaint();
}

/// A watch event's extras (Gadgetbridge's broadcast, or a line over the
/// direct link): checked ([`crate::link::parse_event`]) and dispatched, or
/// logged and dropped if refused.
pub fn from_watch(extras: &BTreeMap<String, String>) {
    match link::parse_event(extras) {
        Ok(event) => dispatch(Input::Watch(event)),
        Err(why) => log::warn!("watch event refused: {why}"),
    }
}

/// A line the watch printed over the direct link: an event for this app
/// goes to [`from_watch`]; anything else (the prompt, Android Integration's
/// own messages) is ignored.
pub fn from_console(line: &str) {
    if let Some(extras) = link::extras_from_line(line) {
        from_watch(&extras);
    }
}

/// What the sync is doing now.
pub fn progress() -> Progress {
    ENGINE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|e| e.progress().clone())
        .unwrap_or_default()
}

/// Goes up whenever what the pages show changed: a newer one than the UI
/// last loaded means reload.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Something the pages show changed outside the engine (the direct link's
/// status, a watch found by the scan): bumps [`generation`] and redraws.
pub fn changed() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
    repaint();
}

/// Gives this module the UI's egui context, so inputs from other threads
/// can have it draw again. The UI calls it on its first frame.
pub fn set_repaint(ctx: &egui::Context) {
    *REPAINT.lock().unwrap_or_else(|e| e.into_inner()) = Some(ctx.clone());
}

fn repaint() {
    if let Some(ctx) = REPAINT.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        ctx.request_repaint();
    }
}

/// The phone's clock, Unix ms: the time [`dispatch`] gives the engine.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Whether this device can connect to the watch itself
/// ([`crate::watch_ble`]): the phone can, the desktop has no Bluetooth link.
pub const HAS_BLUETOOTH: bool = cfg!(target_os = "android");

#[cfg(target_os = "android")]
pub use android::{
    density, keep_watch, light_bars, nav_bar_px, send_line, share_file, status_bar_px, system_dark,
};

#[cfg(target_os = "android")]
pub use android::ble::{
    allowed as bluetooth_allowed, request_permission as request_bluetooth, scan as scan_watches,
};

/// Sends the watch a line (Android: through Gadgetbridge, or the direct
/// link when it's on and up). The desktop has no watch: it logs the line.
#[cfg(not(target_os = "android"))]
pub fn send_line(line: &str) {
    log::info!("watch: {}", line.trim_end());
}

/// Offers the file at `path` (in the store's `exports/`) to other apps
/// with the share sheet (Android, through the `FileProvider`). On the
/// desktop the file stays where it is, which is logged.
#[cfg(not(target_os = "android"))]
pub fn share_file(path: &Path, mime: &str) -> Result<(), String> {
    log::info!("share: {} ({mime}) is left in exports/", path.display());
    Ok(())
}

/// Follows the direct-link setting (Android: keeps the link and
/// `WatchService` going, or stops them). The desktop says it has no
/// Bluetooth.
#[cfg(not(target_os = "android"))]
pub fn keep_watch() {
    use crate::watch_ble::{self, Status};
    watch_ble::set_status(if watch_ble::direct() {
        Status::Unsupported
    } else {
        Status::Off
    });
    changed();
}

/// Looks for watches to connect to, or stops looking (Android only): each
/// one found goes to [`crate::watch_ble::found`], then [`changed`].
#[cfg(not(target_os = "android"))]
pub fn scan_watches(_on: bool) {}

/// Whether the app may use Bluetooth (Android's "Nearby devices"); the
/// desktop has nothing to ask. `BEEP_BLUETOOTH=off` pretends it wasn't
/// allowed, for the Settings page.
#[cfg(not(target_os = "android"))]
pub fn bluetooth_allowed() -> bool {
    std::env::var("BEEP_BLUETOOTH").as_deref() != Ok("off")
}

/// Asks for "Nearby devices" (Android only).
#[cfg(not(target_os = "android"))]
pub fn request_bluetooth() {}

/// Whether the system is in dark mode, where the app can ask: on the
/// desktop egui follows the system itself.
#[cfg(not(target_os = "android"))]
pub fn system_dark() -> Option<bool> {
    None
}

/// The status and navigation bars' icons, dark on a light theme (Android
/// only).
#[cfg(not(target_os = "android"))]
pub fn light_bars(_light: bool) {}

/// How much of the top of the window the status bar covers, pixels (the
/// safe area; Android only).
#[cfg(not(target_os = "android"))]
pub fn status_bar_px() -> f32 {
    0.0
}

/// How much of the bottom the navigation bar covers, pixels (Android only).
#[cfg(not(target_os = "android"))]
pub fn nav_bar_px() -> f32 {
    0.0
}

/// The screen's pixels per point, where the platform says (Android's
/// display density); egui's own guess on the desktop.
#[cfg(not(target_os = "android"))]
pub fn density() -> Option<f32> {
    None
}
