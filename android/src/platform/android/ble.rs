//! The direct Bluetooth link to the watch ([`crate::watch_ble`]) on Android,
//! ported from blink's `platform/android/ble.rs`: a GATT connection to the
//! watch's UART service, held while the app runs, by `WatchService` (a
//! foreground service) when it's closed.
//!
//! Android reports the connection to `WatchGatt`, which hands each callback
//! to the `native*` functions here (`Java_com_cbishopvelti_beep_WatchGatt_*`:
//! state, MTU, services, descriptor, written, changed). They set the
//! connection up (a bigger MTU, the services, notifications on), write
//! [`crate::watch_ble::CLEAR`], then after [`crate::watch_ble::AFTER_CLEAR`]
//! the clock ([`crate::watch_ble::hello`]), and then dispatch
//! [`crate::sync::Input::LinkConnected`], which syncs. They write what's
//! queued a piece at a time ([`crate::watch_ble::Writer`]) and pass each
//! line the watch prints to [`crate::platform::from_console`], the way the
//! Gadgetbridge broadcasts arrive in `nativeWatch`. When the watch goes, the
//! connection is made again with `autoConnect`, so Android finds the watch
//! whenever it comes back; a timer also tries again every [`KEEP_EVERY`],
//! for Bluetooth turned back on or the permission granted, and sends the
//! clock again after a time zone change. The scan for watches is
//! `WatchScan` (`Java_com_cbishopvelti_beep_WatchScan_nativeFound`), and
//! `WatchService` reports itself as blink's does. Each status change goes
//! to [`crate::watch_ble::set_status`] and [`crate::platform::changed`].
//!
//! Locks: [`LINK`] is never held while dispatching (the engine's lock is
//! held while it sends lines, which takes [`LINK`] in [`send`]).

use super::{
    Channel, JResult, PKG, activity, app_class, check, granted, init, init_logging, notification,
    sdk_int, system_service, with_env,
};
use crate::platform;
use crate::sync::Input;
use crate::watch_ble::{self, ClockSent, Read, Reader, Status, Writer};
use jni::JNIEnv;
use jni::objects::{GlobalRef, JByteArray, JObject, JString, JValue};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

const SCAN: &str = "android.permission.BLUETOOTH_SCAN";
const CONNECT: &str = "android.permission.BLUETOOTH_CONNECT";
/// Scanning needs it before Android 12 (the manifest asks only there).
const LOCATION: &str = "android.permission.ACCESS_FINE_LOCATION";
/// For `WatchService`'s notification, from Android 13.
const NOTIFICATIONS: &str = "android.permission.POST_NOTIFICATIONS";
// BluetoothProfile.STATE_*
const STATE_CONNECTED: i32 = 2;
const STATE_DISCONNECTED: i32 = 0;
const TRANSPORT_LE: i32 = 2;
const WRITE_TYPE_DEFAULT: i32 = 2;
const GATT_SUCCESS: i32 = 0;
/// The foreground service's notification.
const SERVICE_NOTIFICATION: i32 = 2;
const FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE: i32 = 0x10;
/// How often to check the link is up, or being made.
const KEEP_EVERY: Duration = Duration::from_secs(20);
/// A write the stack won't start is tried again after this, up to
/// [`MAX_RETRIES`] times in a row before the connection is made afresh.
const RETRY_AFTER: Duration = Duration::from_millis(50);
const MAX_RETRIES: u32 = 100;

struct Link {
    gatt: Option<GlobalRef>,
    /// Which watch `gatt` is for.
    address: Option<String>,
    /// The characteristic the console's input is written to.
    input: Option<GlobalRef>,
    /// Set up, and taking writes.
    ready: bool,
    writer: Writer,
    reader: Reader,
    retries: u32,
    /// Whether `gatt` ever connected: one that fails before it does (the
    /// watch not known to Android yet, say) waits for the keeper rather
    /// than trying again at once.
    connected: bool,
    /// The scan's callback, while Settings looks for watches.
    scan: Option<GlobalRef>,
    /// The scan's callback while the link looks for the watch it waits for
    /// ([`watch_ble::Reach::DeviceAndScan`]).
    seek: Option<GlobalRef>,
    /// Whether a scan saw the watch, or the link connected to it, since
    /// Bluetooth last came on (as far as the link can tell): then Android
    /// knows its address type. Not cleared by [`close`].
    seen: bool,
    /// When `gatt` was made, to make it again if it waits for good
    /// ([`watch_ble::WAIT_AT_MOST`]).
    since: Option<Instant>,
    /// The clock last sent to the watch, and when ([`watch_ble::ClockSent`]).
    clock: Option<(ClockSent, Instant)>,
}

/// Whether `WatchService` is running.
static SERVICE: AtomicBool = AtomicBool::new(false);

static LINK: Mutex<Link> = Mutex::new(Link {
    gatt: None,
    address: None,
    input: None,
    ready: false,
    writer: Writer::new(),
    reader: Reader::new(),
    retries: 0,
    connected: false,
    scan: None,
    seek: None,
    seen: false,
    since: None,
    clock: None,
});

fn link() -> MutexGuard<'static, Link> {
    LINK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sets the link's status and tells the UI.
fn status(s: Status) {
    watch_ble::set_status(s);
    platform::changed();
}

fn later(after: Duration, f: fn()) {
    std::thread::spawn(move || {
        std::thread::sleep(after);
        f();
    });
}

fn uuid<'l>(env: &mut JNIEnv<'l>, s: &str) -> JResult<JObject<'l>> {
    let s = env.new_string(s)?;
    env.call_static_method(
        "java/util/UUID",
        "fromString",
        "(Ljava/lang/String;)Ljava/util/UUID;",
        &[JValue::Object(&s)],
    )?
    .l()
}

fn adapter<'l>(env: &mut JNIEnv<'l>, ctx: &JObject) -> JResult<JObject<'l>> {
    let manager = system_service(env, ctx, "bluetooth")?;
    if manager.is_null() {
        return Ok(manager);
    }
    env.call_method(
        &manager,
        "getAdapter",
        "()Landroid/bluetooth/BluetoothAdapter;",
        &[],
    )?
    .l()
}

/// Whether the phone lets the app find and connect to the watch: "Nearby
/// devices" from Android 12; before that, location, which scanning needs
/// there.
pub fn allowed() -> bool {
    let mut out = false;
    with_env(
        |env, ctx| {
            out = if sdk_int(env)? < 31 {
                granted(env, ctx, LOCATION)?
            } else {
                granted(env, ctx, SCAN)? && granted(env, ctx, CONNECT)?
            };
            Ok(())
        },
        "bluetooth permission",
    );
    out
}

/// Whether the phone finds no Bluetooth watches because its Location is
/// off: Android 11 and earlier give a scan nothing then (and say nothing
/// about why). Android 12 and later scan with "Nearby devices" instead.
pub fn location_off() -> bool {
    let mut out = false;
    with_env(
        |env, ctx| {
            let sdk = sdk_int(env)?;
            if sdk >= 31 {
                return Ok(());
            }
            let manager = system_service(env, ctx, "location")?;
            if manager.is_null() {
                return Ok(());
            }
            let on = if sdk >= 28 {
                env.call_method(&manager, "isLocationEnabled", "()Z", &[])?
                    .z()?
            } else {
                let mut any = false;
                for provider in ["gps", "network"] {
                    let p = env.new_string(provider)?;
                    any |= env
                        .call_method(
                            &manager,
                            "isProviderEnabled",
                            "(Ljava/lang/String;)Z",
                            &[JValue::Object(&p)],
                        )?
                        .z()?;
                }
                any
            };
            out = !on;
            Ok(())
        },
        "location on",
    );
    out
}

/// Asks for "Nearby devices" (Android 12 and later; location before), and
/// from Android 13 for notifications too, so `WatchService`'s shows.
pub fn request_permission() {
    let Some(activity) = activity() else {
        return;
    };
    with_env(
        |env, _ctx| {
            let sdk = sdk_int(env)?;
            let wanted: &[&str] = match sdk {
                ..31 => &[LOCATION],
                31..33 => &[SCAN, CONNECT],
                _ => &[SCAN, CONNECT, NOTIFICATIONS],
            };
            let array =
                env.new_object_array(wanted.len() as i32, "java/lang/String", JObject::null())?;
            for (i, p) in wanted.iter().enumerate() {
                let s = env.new_string(p)?;
                env.set_object_array_element(&array, i as i32, s)?;
            }
            env.call_method(
                activity.as_obj(),
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(&array), JValue::Int(2)],
            )?;
            Ok(())
        },
        "ask for bluetooth",
    );
}

/// Follows the setting: with the direct link on, keeps `WatchService`
/// running and the connection up (or being made); with it off, stops both.
pub fn keep() {
    if watch_ble::direct() {
        start_service();
    } else {
        stop_service();
    }
    ensure();
}

/// Makes the connection the setting asks for, if it isn't there.
fn ensure() {
    let settings = watch_ble::settings();
    let mut l = link();
    if !settings.direct {
        close(&mut l);
        status(Status::Off);
        return;
    }
    keeper();
    let Some(watch) = settings.watch else {
        close(&mut l);
        status(Status::NoWatch);
        return;
    };
    if l.gatt.is_some() && l.address.as_deref() == Some(watch.address.as_str()) {
        let waited = l.since.map_or(Duration::ZERO, |t| t.elapsed());
        if l.connected || waited < watch_ble::WAIT_AT_MOST {
            return;
        }
        log::info!(
            "watch link: still waiting after {}s; trying again",
            waited.as_secs()
        );
        // Android may have forgotten the watch's address type (Bluetooth
        // went off and on in between): look for it too this time.
        l.seen = false;
    }
    close(&mut l);
    if !allowed() {
        status(Status::NotAllowed);
        return;
    }
    with_env(
        |env, ctx| {
            let adapter = adapter(env, ctx)?;
            if adapter.is_null() {
                status(Status::Unsupported);
                return Ok(());
            }
            if !env.call_method(&adapter, "isEnabled", "()Z", &[])?.z()? {
                // When it comes back on, Android has forgotten the watch.
                l.seen = false;
                status(Status::BluetoothOff);
                return Ok(());
            }
            let address = env.new_string(&watch.address)?;
            let reach = watch_ble::reach(sdk_int(env)?, &watch.address, l.seen);
            let device = match reach {
                watch_ble::Reach::LeDevice { address_type } => env.call_method(
                    &adapter,
                    "getRemoteLeDevice",
                    "(Ljava/lang/String;I)Landroid/bluetooth/BluetoothDevice;",
                    &[JValue::Object(&address), JValue::Int(address_type)],
                ),
                watch_ble::Reach::Device | watch_ble::Reach::DeviceAndScan => env.call_method(
                    &adapter,
                    "getRemoteDevice",
                    "(Ljava/lang/String;)Landroid/bluetooth/BluetoothDevice;",
                    &[JValue::Object(&address)],
                ),
            }?
            .l()?;
            let class = app_class(env, ctx, &format!("{PKG}.WatchGatt"))?;
            let callback = env.new_object(&class, "()V", &[])?;
            // autoConnect: Android connects whenever the watch is about,
            // with no timeout, and the watch isn't asked to be paired.
            let gatt = env
                .call_method(
                    &device,
                    "connectGatt",
                    "(Landroid/content/Context;ZLandroid/bluetooth/BluetoothGattCallback;I)Landroid/bluetooth/BluetoothGatt;",
                    &[
                        JValue::Object(ctx),
                        JValue::Bool(1),
                        JValue::Object(&callback),
                        JValue::Int(TRANSPORT_LE),
                    ],
                )?
                .l()?;
            if gatt.is_null() {
                log::warn!("watch link: connectGatt gave nothing");
                return Ok(());
            }
            l.gatt = Some(env.new_global_ref(&gatt)?);
            l.address = Some(watch.address.clone());
            l.since = Some(Instant::now());
            status(Status::Connecting);
            log::info!("watch link: waiting for {} ({reach:?})", watch.name);
            if reach == watch_ble::Reach::DeviceAndScan {
                seek(env, ctx, &adapter, &watch.name, &mut l)?;
            }
            Ok(())
        },
        "watch connect",
    );
}

/// Checks on the link every [`KEEP_EVERY`] while it's wanted.
fn keeper() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        std::thread::Builder::new()
            .name("beep-watch-link".into())
            .spawn(|| {
                loop {
                    std::thread::sleep(KEEP_EVERY);
                    if watch_ble::direct() {
                        // Allowed since, say: Android may still refuse
                        // while the app is in the background.
                        if !SERVICE.load(Ordering::Relaxed) {
                            start_service();
                        }
                        ensure();
                        resend_clock();
                    }
                }
            })
            .expect("spawn the watch link keeper");
    });
}

/// Sends the watch its clock and time zone ([`watch_ble::hello`]), and
/// keeps what was sent.
fn send_clock() -> bool {
    let now = crate::health::now_s();
    let offset_s = watch_ble::utc_offset(now);
    if !send(&watch_ble::hello(now, offset_s)) {
        return false;
    }
    link().clock = Some((
        ClockSent {
            unix: now,
            offset_s,
        },
        Instant::now(),
    ));
    true
}

/// Sends the clock again if the time zone changed or the clock was set
/// since it was last sent, as Gadgetbridge does.
fn resend_clock() {
    let Some((sent, at)) = link().clock else {
        return;
    };
    let now = crate::health::now_s();
    let elapsed = at.elapsed().as_secs() as i64;
    if sent.stale(elapsed, now, watch_ble::utc_offset(now)) {
        log::info!("watch link: the time zone or clock changed; sending it again");
        send_clock();
    }
}

/// Looks for the watch called `name` while its connection waits
/// ([`watch_ble::Reach::DeviceAndScan`]): a scan filtered by the name, so
/// it goes on with the screen off. Its results come to `nativeFound`, which
/// makes the connection again once the watch's address shows up.
fn seek(
    env: &mut JNIEnv,
    ctx: &JObject,
    adapter: &JObject,
    name: &str,
    l: &mut Link,
) -> JResult<()> {
    let scanner = env
        .call_method(
            adapter,
            "getBluetoothLeScanner",
            "()Landroid/bluetooth/le/BluetoothLeScanner;",
            &[],
        )?
        .l()?;
    if scanner.is_null() || l.seek.is_some() {
        return Ok(());
    }
    let name = env.new_string(name)?;
    let builder = env.new_object("android/bluetooth/le/ScanFilter$Builder", "()V", &[])?;
    env.call_method(
        &builder,
        "setDeviceName",
        "(Ljava/lang/String;)Landroid/bluetooth/le/ScanFilter$Builder;",
        &[JValue::Object(&name)],
    )?;
    let filter = env
        .call_method(
            &builder,
            "build",
            "()Landroid/bluetooth/le/ScanFilter;",
            &[],
        )?
        .l()?;
    let filters = env.new_object("java/util/ArrayList", "()V", &[])?;
    env.call_method(
        &filters,
        "add",
        "(Ljava/lang/Object;)Z",
        &[JValue::Object(&filter)],
    )?;
    let builder = env.new_object("android/bluetooth/le/ScanSettings$Builder", "()V", &[])?;
    env.call_method(
        &builder,
        "setScanMode",
        "(I)Landroid/bluetooth/le/ScanSettings$Builder;",
        // SCAN_MODE_LOW_POWER: it may take a while, which is fine.
        &[JValue::Int(0)],
    )?;
    let settings = env
        .call_method(
            &builder,
            "build",
            "()Landroid/bluetooth/le/ScanSettings;",
            &[],
        )?
        .l()?;
    let class = app_class(env, ctx, &format!("{PKG}.WatchScan"))?;
    let callback = env.new_object(&class, "()V", &[])?;
    env.call_method(
        &scanner,
        "startScan",
        "(Ljava/util/List;Landroid/bluetooth/le/ScanSettings;Landroid/bluetooth/le/ScanCallback;)V",
        &[
            JValue::Object(&filters),
            JValue::Object(&settings),
            JValue::Object(&callback),
        ],
    )?;
    l.seek = Some(env.new_global_ref(&callback)?);
    log::info!("watch link: looking for the watch too");
    Ok(())
}

/// Stops [`seek`]'s scan, if it's on.
fn stop_seek(l: &mut Link) {
    let Some(callback) = l.seek.take() else {
        return;
    };
    with_env(
        |env, ctx| {
            let adapter = adapter(env, ctx)?;
            if adapter.is_null() {
                return Ok(());
            }
            let scanner = env
                .call_method(
                    &adapter,
                    "getBluetoothLeScanner",
                    "()Landroid/bluetooth/le/BluetoothLeScanner;",
                    &[],
                )?
                .l()?;
            // Null while Bluetooth is off, when the scan has stopped anyway.
            if !scanner.is_null() {
                env.call_method(
                    &scanner,
                    "stopScan",
                    "(Landroid/bluetooth/le/ScanCallback;)V",
                    &[JValue::Object(callback.as_obj())],
                )?;
            }
            Ok(())
        },
        "stop looking for the watch",
    );
}

/// Drops the connection, and with it whatever was waiting to be written.
fn close(l: &mut Link) {
    stop_seek(l);
    if let Some(gatt) = l.gatt.take() {
        with_env(
            |env, _| {
                env.call_method(gatt.as_obj(), "disconnect", "()V", &[])?;
                env.call_method(gatt.as_obj(), "close", "()V", &[])?;
                Ok(())
            },
            "watch disconnect",
        );
    }
    l.address = None;
    l.input = None;
    l.ready = false;
    l.writer.clear();
    l.reader.clear();
    l.retries = 0;
    l.connected = false;
    l.since = None;
    l.clock = None;
}

/// Writes `text` to the watch's console, after whatever is waiting. False
/// if the link isn't up: nothing is kept for later, since a new connection
/// starts with a sync.
pub fn send(text: &str) -> bool {
    let mut l = link();
    if !l.ready {
        return false;
    }
    l.writer.push(&watch_ble::encode(text));
    with_env(
        |env, _| {
            pump(env, &mut l);
            Ok(())
        },
        "watch write",
    );
    true
}

/// Starts the next write, if one can go.
fn pump(env: &mut JNIEnv, l: &mut Link) {
    if !l.ready {
        return;
    }
    let (Some(gatt), Some(input)) = (l.gatt.clone(), l.input.clone()) else {
        return;
    };
    let Some(chunk) = l.writer.next_write() else {
        return;
    };
    let r = write(env, gatt.as_obj(), input.as_obj(), &chunk);
    let started = matches!(r, Ok(true));
    check(env, "watch write", r.map(|_| ()));
    if started {
        l.retries = 0;
        return;
    }
    l.writer.retry(chunk);
    l.retries += 1;
    if l.retries > MAX_RETRIES {
        log::warn!("watch link: writes keep failing; connecting again");
        close(l);
        status(Status::Connecting);
        later(Duration::from_secs(1), ensure);
        return;
    }
    later(RETRY_AFTER, || {
        let mut l = link();
        with_env(
            |env, _| {
                pump(env, &mut l);
                Ok(())
            },
            "watch write",
        );
    });
}

fn write(env: &mut JNIEnv, gatt: &JObject, input: &JObject, chunk: &[u8]) -> JResult<bool> {
    let value = env.byte_array_from_slice(chunk)?;
    if sdk_int(env)? >= 33 {
        let status = env
            .call_method(
                gatt,
                "writeCharacteristic",
                "(Landroid/bluetooth/BluetoothGattCharacteristic;[BI)I",
                &[
                    JValue::Object(input),
                    JValue::Object(&value),
                    JValue::Int(WRITE_TYPE_DEFAULT),
                ],
            )?
            .i()?;
        return Ok(status == GATT_SUCCESS);
    }
    env.call_method(
        input,
        "setWriteType",
        "(I)V",
        &[JValue::Int(WRITE_TYPE_DEFAULT)],
    )?;
    env.call_method(input, "setValue", "([B)Z", &[JValue::Object(&value)])?;
    env.call_method(
        gatt,
        "writeCharacteristic",
        "(Landroid/bluetooth/BluetoothGattCharacteristic;)Z",
        &[JValue::Object(input)],
    )?
    .z()
}

/// Whether a callback is about the connection in use (not one closed since).
fn ours(env: &mut JNIEnv, l: &Link, gatt: &JObject) -> bool {
    l.gatt
        .as_ref()
        .is_some_and(|g| env.is_same_object(g.as_obj(), gatt).unwrap_or(false))
}

/// Runs a callback's body, catching a panic: it mustn't unwind into Java.
fn callback(what: &str, f: impl FnOnce()) {
    init_logging();
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
        log::error!("{what} panicked");
    }
}

// --- Callbacks from WatchGatt ----------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchGatt_nativeState<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    gatt: JObject<'l>,
    gatt_status: i32,
    state: i32,
) {
    callback("watch state", || {
        let mut l = link();
        if !ours(&mut env, &l, &gatt) {
            return;
        }
        if state == STATE_CONNECTED {
            log::info!("watch link: connected");
            l.connected = true;
            l.seen = true;
            stop_seek(&mut l);
            status(Status::SettingUp);
            let r = (|| {
                let asked = env
                    .call_method(&gatt, "requestMtu", "(I)Z", &[JValue::Int(watch_ble::MTU)])?
                    .z()?;
                if !asked {
                    env.call_method(&gatt, "discoverServices", "()Z", &[])?;
                }
                Ok(())
            })();
            check(&mut env, "watch MTU", r);
        } else if state == STATE_DISCONNECTED {
            log::info!("watch link: disconnected ({gatt_status})");
            // Only a link that was up (set up, and taking writes) is made
            // again at once.
            let was_up = l.ready;
            close(&mut l);
            status(Status::Connecting);
            drop(l);
            // A fresh autoConnect, which waits for the watch to come back.
            // One that never got there, or dropped while being set up,
            // waits for the keeper, so a watch that keeps failing set-up
            // isn't tried every 2 s.
            if was_up {
                later(Duration::from_secs(2), ensure);
            }
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchGatt_nativeMtu<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    gatt: JObject<'l>,
    mtu: i32,
    gatt_status: i32,
) {
    callback("watch MTU", || {
        let mut l = link();
        if !ours(&mut env, &l, &gatt) {
            return;
        }
        if gatt_status == GATT_SUCCESS {
            l.writer.set_mtu(mtu);
        }
        let r = env
            .call_method(&gatt, "discoverServices", "()Z", &[])
            .map(|_| ());
        check(&mut env, "watch services", r);
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchGatt_nativeServices<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    gatt: JObject<'l>,
    gatt_status: i32,
) {
    callback("watch services", || {
        let mut l = link();
        if !ours(&mut env, &l, &gatt) {
            return;
        }
        let r = set_up(&mut env, &gatt, gatt_status);
        let input = match r {
            Ok(input) => input,
            Err(e) => {
                check(&mut env, "watch services", Err(e));
                None
            }
        };
        match input {
            Some(input) => l.input = Some(input),
            None => {
                // Dropping it reports a disconnect, which tries again.
                let r = env.call_method(&gatt, "disconnect", "()V", &[]).map(|_| ());
                check(&mut env, "watch disconnect", r);
            }
        }
    });
}

/// Finds the UART service and turns its notifications on: the console's
/// input characteristic, or `None` if this isn't a watch's console.
fn set_up<'l>(
    env: &mut JNIEnv<'l>,
    gatt: &JObject,
    gatt_status: i32,
) -> JResult<Option<GlobalRef>> {
    if gatt_status != GATT_SUCCESS {
        return Ok(None);
    }
    let id = uuid(env, watch_ble::UART_SERVICE)?;
    let service = env
        .call_method(
            gatt,
            "getService",
            "(Ljava/util/UUID;)Landroid/bluetooth/BluetoothGattService;",
            &[JValue::Object(&id)],
        )?
        .l()?;
    if service.is_null() {
        log::warn!("watch link: no UART service: is it a Bangle.js?");
        return Ok(None);
    }
    let characteristic = |env: &mut JNIEnv<'l>, which: &str| -> JResult<JObject<'l>> {
        let id = uuid(env, which)?;
        env.call_method(
            &service,
            "getCharacteristic",
            "(Ljava/util/UUID;)Landroid/bluetooth/BluetoothGattCharacteristic;",
            &[JValue::Object(&id)],
        )?
        .l()
    };
    let input = characteristic(env, watch_ble::UART_WRITE)?;
    let output = characteristic(env, watch_ble::UART_NOTIFY)?;
    if input.is_null() || output.is_null() {
        return Ok(None);
    }
    env.call_method(
        gatt,
        "setCharacteristicNotification",
        "(Landroid/bluetooth/BluetoothGattCharacteristic;Z)Z",
        &[JValue::Object(&output), JValue::Bool(1)],
    )?;
    let id = uuid(env, watch_ble::CLIENT_CONFIG)?;
    let descriptor = env
        .call_method(
            &output,
            "getDescriptor",
            "(Ljava/util/UUID;)Landroid/bluetooth/BluetoothGattDescriptor;",
            &[JValue::Object(&id)],
        )?
        .l()?;
    if descriptor.is_null() {
        return Ok(None);
    }
    let on = env
        .get_static_field(
            "android/bluetooth/BluetoothGattDescriptor",
            "ENABLE_NOTIFICATION_VALUE",
            "[B",
        )?
        .l()?;
    if sdk_int(env)? >= 33 {
        env.call_method(
            gatt,
            "writeDescriptor",
            "(Landroid/bluetooth/BluetoothGattDescriptor;[B)I",
            &[JValue::Object(&descriptor), JValue::Object(&on)],
        )?;
    } else {
        env.call_method(&descriptor, "setValue", "([B)Z", &[JValue::Object(&on)])?;
        env.call_method(
            gatt,
            "writeDescriptor",
            "(Landroid/bluetooth/BluetoothGattDescriptor;)Z",
            &[JValue::Object(&descriptor)],
        )?;
    }
    Ok(Some(env.new_global_ref(&input)?))
}

/// Notifications are on: the link is up. A Ctrl-C on its own, then the
/// clock, then a sync.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchGatt_nativeDescriptor<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    gatt: JObject<'l>,
    gatt_status: i32,
) {
    callback("watch notifications", || {
        let mut l = link();
        if !ours(&mut env, &l, &gatt) {
            return;
        }
        if gatt_status != GATT_SUCCESS {
            log::warn!("watch link: notifications not turned on ({gatt_status})");
            let r = env.call_method(&gatt, "disconnect", "()V", &[]).map(|_| ());
            check(&mut env, "watch disconnect", r);
            return;
        }
        l.ready = true;
        status(Status::Connected);
        l.writer.push(&watch_ble::encode(watch_ble::CLEAR));
        pump(&mut env, &mut l);
        drop(l);
        later(watch_ble::AFTER_CLEAR, || {
            callback("watch link up", || {
                if send_clock() {
                    platform::dispatch(Input::LinkConnected);
                }
            })
        });
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchGatt_nativeWritten<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    gatt: JObject<'l>,
    gatt_status: i32,
) {
    callback("watch written", || {
        let mut l = link();
        if !ours(&mut env, &l, &gatt) {
            return;
        }
        if gatt_status != GATT_SUCCESS {
            log::warn!("watch link: a write failed ({gatt_status})");
        }
        l.writer.written();
        pump(&mut env, &mut l);
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchGatt_nativeChanged<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    gatt: JObject<'l>,
    value: JByteArray<'l>,
) {
    callback("watch read", || {
        let mut l = link();
        if !ours(&mut env, &l, &gatt) {
            return;
        }
        let bytes = match env.convert_byte_array(&value) {
            Ok(b) => b,
            Err(e) => return check(&mut env, "watch read", Err(e)),
        };
        let mut lines = Vec::new();
        for read in l.reader.push(&bytes) {
            match read {
                Read::Go(go) => l.writer.pause(!go),
                Read::Line(line) => lines.push(line),
            }
        }
        pump(&mut env, &mut l);
        drop(l);
        // Our events go through the same code as WatchReceiver's
        // broadcasts; anything else the console prints is ignored.
        for line in lines {
            platform::from_console(&line);
        }
    });
}

// --- Finding the watch -----------------------------------------------------

/// Looks for watches (Settings' picker), into
/// [`watch_ble::found_watches`], or stops.
pub fn scan(on: bool) {
    let mut l = link();
    with_env(
        |env, ctx| {
            let adapter = adapter(env, ctx)?;
            if adapter.is_null() {
                return Ok(());
            }
            let scanner = env
                .call_method(
                    &adapter,
                    "getBluetoothLeScanner",
                    "()Landroid/bluetooth/le/BluetoothLeScanner;",
                    &[],
                )?
                .l()?;
            // Null while Bluetooth is off.
            if scanner.is_null() {
                return Ok(());
            }
            if let Some(callback) = l.scan.take() {
                env.call_method(
                    &scanner,
                    "stopScan",
                    "(Landroid/bluetooth/le/ScanCallback;)V",
                    &[JValue::Object(callback.as_obj())],
                )?;
            }
            if on {
                watch_ble::forget_found();
                let class = app_class(env, ctx, &format!("{PKG}.WatchScan"))?;
                let callback = env.new_object(&class, "()V", &[])?;
                env.call_method(
                    &scanner,
                    "startScan",
                    "(Landroid/bluetooth/le/ScanCallback;)V",
                    &[JValue::Object(&callback)],
                )?;
                l.scan = Some(env.new_global_ref(&callback)?);
            }
            Ok(())
        },
        "watch scan",
    );
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchScan_nativeFound<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    address: JString<'l>,
    name: JString<'l>,
    rssi: i32,
) {
    callback("watch found", || {
        let (Ok(address), Ok(name)) = (env.get_string(&address), env.get_string(&name)) else {
            return;
        };
        let (address, name): (String, String) = (address.into(), name.into());
        let before = watch_ble::found_watches();
        watch_ble::found(&address, &name, rssi);
        if watch_ble::found_watches() != before {
            platform::changed();
        }
        // The watch the link waits for: Android knows its address type now,
        // so a connection made before it did (which would never find it) is
        // made again.
        let ours = watch_ble::settings()
            .watch
            .is_some_and(|w| w.address.eq_ignore_ascii_case(&address));
        if ours {
            let mut l = link();
            l.seen = true;
            if l.seek.is_some() && !l.connected {
                log::info!("watch link: found the watch; connecting again");
                close(&mut l);
                drop(l);
                later(Duration::ZERO, ensure);
            } else {
                stop_seek(&mut l);
            }
        }
    });
}

// --- WatchService ----------------------------------------------------------

fn service_intent<'l>(env: &mut JNIEnv<'l>, ctx: &JObject) -> JResult<JObject<'l>> {
    let class = app_class(env, ctx, &format!("{PKG}.WatchService"))?;
    env.new_object(
        "android/content/Intent",
        "(Landroid/content/Context;Ljava/lang/Class;)V",
        &[JValue::Object(ctx), JValue::Object(&class)],
    )
}

fn start_service() {
    if !allowed() {
        // Android 14 refuses a connected-device service without it.
        return;
    }
    with_env(
        |env, ctx| {
            let intent = service_intent(env, ctx)?;
            env.call_method(
                ctx,
                "startForegroundService",
                "(Landroid/content/Intent;)Landroid/content/ComponentName;",
                &[JValue::Object(&intent)],
            )?;
            Ok(())
        },
        "start the watch service",
    );
}

fn stop_service() {
    with_env(
        |env, ctx| {
            let intent = service_intent(env, ctx)?;
            env.call_method(
                ctx,
                "stopService",
                "(Landroid/content/Intent;)Z",
                &[JValue::Object(&intent)],
            )?;
            Ok(())
        },
        "stop the watch service",
    );
}

/// `WatchService` started (by the app, after a reboot, or by Android again
/// after it stopped it, in a process of its own): into the foreground, with
/// its notification, the store open for the sync, and connect.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchService_nativeStarted<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    service: JObject<'l>,
) {
    callback("watch service", || {
        let r = init(&mut env, &service);
        check(&mut env, "init", r);
        // The store (and with it the setting) from the files directory
        // init found.
        match super::files_dir().map(|dir| platform::open(&dir)) {
            Some(Ok(_)) => {}
            Some(Err(e)) => log::error!("watch service: can't open the store: {e}"),
            None => log::error!("watch service: no files directory"),
        }
        let r = (|| {
            let channel = Channel {
                id: "watch",
                name: "Watch connection",
                // IMPORTANCE_MIN: no icon in the status bar, no sound.
                importance: 1,
            };
            let n = notification(
                &mut env,
                &service,
                &channel,
                "Watch link on",
                "Beep Test connects to your watch, and syncs with it, while it's closed.",
            )?;
            if sdk_int(&mut env)? >= 29 {
                env.call_method(
                    &service,
                    "startForeground",
                    "(ILandroid/app/Notification;I)V",
                    &[
                        JValue::Int(SERVICE_NOTIFICATION),
                        JValue::Object(&n),
                        JValue::Int(FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE),
                    ],
                )?;
            } else {
                env.call_method(
                    &service,
                    "startForeground",
                    "(ILandroid/app/Notification;)V",
                    &[JValue::Int(SERVICE_NOTIFICATION), JValue::Object(&n)],
                )?;
            }
            Ok(())
        })();
        let started = r.is_ok();
        check(&mut env, "watch service", r);
        // Turned off since, or Android refused the foreground (Nearby
        // devices taken back): stop.
        if !started || !watch_ble::direct() {
            let r = env
                .call_method(&service, "stopSelf", "()V", &[])
                .map(|_| ());
            check(&mut env, "stop the watch service", r);
        } else {
            SERVICE.store(true, Ordering::Relaxed);
        }
        ensure();
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchService_nativeStopped<'l>(
    _env: JNIEnv<'l>,
    _this: JObject<'l>,
) {
    SERVICE.store(false, Ordering::Relaxed);
    callback("watch service stopped", || {
        log::info!("watch link: service stopped")
    });
}
