//! Android through JNI, as blink's `platform/android.rs`. Rust makes every
//! call into the platform; Java only appears where Android has to call into
//! the app by class name (see `app/src/main/java`): those stubs hand each
//! event straight to the `native*` functions at the bottom of this file and
//! in `ble.rs`.
//!
//! What it does:
//!
//! - [`init_activity`]: from `android_main`, the JavaVM and the activity
//!   (logging as "beep", the application context and its files directory).
//! - [`send_line`]: the Gadgetbridge broadcast (`com.banglejs.uart.tx`,
//!   extra `line`), or the direct link ([`ble::send`]) when that's on and up;
//!   while it isn't up, Gadgetbridge after all, as blink.
//! - [`share_file`]: `ACTION_SEND` with a `content://` uri from the app's
//!   `FileProvider` (the store's `exports/`) and the chooser.
//! - The theme and the bars ([`system_dark`], [`light_bars`] through
//!   `ThemeStub`), the safe area ([`status_bar_px`], [`nav_bar_px`]) and the
//!   density, as blink's. The back key reaches the UI as egui's
//!   `Key::BrowserBack` (the manifest keeps predictive back off for that).
//! - `WatchReceiver`'s broadcast:
//!   [`Java_com_cbishopvelti_beep_WatchReceiver_nativeWatch`] reads the
//!   extras named in [`crate::link::EXTRAS`] and passes them to
//!   [`crate::platform::from_watch`], after opening the store
//!   ([`crate::platform::open`]) from the context's files directory if the
//!   process has just started for it, inside `catch_unwind`.
//! - `BootReceiver`: after a reboot or an update, the direct link and
//!   `WatchService` again if that's on ([`ble::keep`]).
//!
//! Each stub's `native*` methods and the Rust functions they land in are
//! listed in `android/README.md` ("The Java side").

use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub mod ble;
pub mod ui_loop;

type JResult<T> = jni::errors::Result<T>;

const PKG: &str = crate::link::PACKAGE;
/// The `FileProvider`'s authority (`AndroidManifest.xml`).
const FILES_AUTHORITY: &str = "com.cbishopvelti.beep.files";
/// `Intent.FLAG_GRANT_READ_URI_PERMISSION`.
const FLAG_GRANT_READ_URI_PERMISSION: i32 = 0x0000_0001;
/// `Intent.FLAG_ACTIVITY_NEW_TASK`.
const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;
// PendingIntent flags.
const FLAG_UPDATE_CURRENT: i32 = 0x0800_0000;
const FLAG_IMMUTABLE: i32 = 0x0400_0000;

struct Jvm {
    vm: JavaVM,
    /// The application context: lives as long as the process.
    ctx: GlobalRef,
    files: PathBuf,
}

static JVM: OnceLock<Jvm> = OnceLock::new();
/// The activity showing now: a new one replaces it each time
/// `android_main` runs (the app opened again after back closed it, in the
/// same process).
static ACTIVITY: Mutex<Option<GlobalRef>> = Mutex::new(None);

fn activity() -> Option<GlobalRef> {
    ACTIVITY.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn init_logging() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("beep"),
    );
}

/// Logs and clears a pending Java exception, so later calls still work.
fn check(env: &mut JNIEnv, what: &str, r: JResult<()>) {
    if let Err(e) = r {
        let thrown = take_exception(env);
        log::error!(
            "{what}: {e}{}",
            thrown.map_or(String::new(), |t| format!(": {t}"))
        );
    }
}

/// Clears a pending Java exception, and says what it was.
fn take_exception(env: &mut JNIEnv) -> Option<String> {
    if !env.exception_check().unwrap_or(false) {
        return None;
    }
    let thrown = env.exception_occurred().ok();
    let _ = env.exception_clear();
    let thrown = thrown?;
    let text = env
        .call_method(&thrown, "toString", "()Ljava/lang/String;", &[])
        .and_then(|v| v.l())
        .ok()?;
    let text: String = env.get_string(&JString::from(text)).ok()?.into();
    Some(text)
}

/// Keeps the JavaVM and the application context (and finds the files
/// directory) the first time any entry into Rust gets a context: the
/// activity, a receiver or the service, whichever starts the process.
fn init(env: &mut JNIEnv, context: &JObject) -> JResult<()> {
    if JVM.get().is_some() {
        return Ok(());
    }
    let app = env
        .call_method(
            context,
            "getApplicationContext",
            "()Landroid/content/Context;",
            &[],
        )?
        .l()?;
    let dir = env
        .call_method(&app, "getFilesDir", "()Ljava/io/File;", &[])?
        .l()?;
    let path = env
        .call_method(&dir, "getAbsolutePath", "()Ljava/lang/String;", &[])?
        .l()?;
    let path: String = env.get_string(&JString::from(path))?.into();
    let _ = JVM.set(Jvm {
        vm: env.get_java_vm()?,
        ctx: env.new_global_ref(&app)?,
        files: PathBuf::from(path),
    });
    Ok(())
}

/// Called from `android_main` with the NativeActivity's VM and activity.
pub fn init_activity(vm: *mut std::ffi::c_void, activity: *mut std::ffi::c_void) {
    init_logging();
    // SAFETY: android-activity hands out the process's JavaVM and a live
    // reference to the activity; we only make our own global ref from it.
    let vm = match unsafe { JavaVM::from_raw(vm.cast()) } {
        Ok(vm) => vm,
        Err(e) => return log::error!("no JavaVM: {e}"),
    };
    let Ok(mut env) = vm.attach_current_thread_permanently() else {
        return log::error!("can't attach to the JavaVM");
    };
    // SAFETY: as above, a live reference for as long as this call.
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let r = (|| {
        init(&mut env, &activity)?;
        let activity = env.new_global_ref(&activity)?;
        *ACTIVITY.lock().unwrap_or_else(|e| e.into_inner()) = Some(activity);
        Ok(())
    })();
    check(&mut env, "init", r);
}

/// The app's files directory (`Context.getFilesDir`), once [`init`] ran.
pub fn files_dir() -> Option<PathBuf> {
    JVM.get().map(|j| j.files.clone())
}

/// Runs `f` with this thread's JNIEnv (attached for good, as threads here
/// are few: the UI's, the Bluetooth callbacks', a timer's) and the
/// application context, in a local frame of its own. Errors are logged.
fn with_env(f: impl FnOnce(&mut JNIEnv, &JObject) -> JResult<()>, what: &str) {
    let Some(jvm) = JVM.get() else { return };
    let Ok(mut env) = jvm.vm.attach_current_thread_permanently() else {
        return;
    };
    let r = env.with_local_frame(64, |env| f(env, jvm.ctx.as_obj()));
    check(&mut env, what, r);
}

/// Loads one of the app's own classes (or a library's packaged with it,
/// such as `FileProvider`). A native thread's `FindClass` only sees the
/// platform's classes, so go through the app's class loader.
fn app_class<'l>(env: &mut JNIEnv<'l>, ctx: &JObject, name: &str) -> JResult<JClass<'l>> {
    let loader = env
        .call_method(ctx, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string(name)?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    Ok(JClass::from(class))
}

fn sdk_int(env: &mut JNIEnv) -> JResult<i32> {
    env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
        .i()
}

fn system_service<'l>(env: &mut JNIEnv<'l>, ctx: &JObject, name: &str) -> JResult<JObject<'l>> {
    let name = env.new_string(name)?;
    env.call_method(
        ctx,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[JValue::Object(&name)],
    )?
    .l()
}

fn granted(env: &mut JNIEnv, ctx: &JObject, permission: &str) -> JResult<bool> {
    let p = env.new_string(permission)?;
    let r = env
        .call_method(
            ctx,
            "checkSelfPermission",
            "(Ljava/lang/String;)I",
            &[JValue::Object(&p)],
        )?
        .i()?;
    Ok(r == 0)
}

// --- The watch -------------------------------------------------------------

/// Sends the watch `line`: over the direct link when it's on and up, else
/// through Gadgetbridge. While the direct link isn't up, it goes to
/// Gadgetbridge after all, as blink's: the watch may still be
/// Gadgetbridge's, and the link syncs again once it's up
/// ([`crate::sync::Input::LinkConnected`]).
pub fn send_line(line: &str) {
    if crate::watch_ble::direct() && ble::send(line) {
        return;
    }
    with_env(|env, ctx| gadgetbridge(env, ctx, line), "watch");
}

/// Gadgetbridge passes the `line` extra of a `com.banglejs.uart.tx`
/// broadcast to the watch (needs "Allow Intents" on in Gadgetbridge).
fn gadgetbridge(env: &mut JNIEnv, ctx: &JObject, line: &str) -> JResult<()> {
    let action = env.new_string(crate::link::GB_ACTION)?;
    let intent = env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&action)],
    )?;
    let key = env.new_string("line")?;
    let value = env.new_string(line)?;
    env.call_method(
        &intent,
        "putExtra",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&key), JValue::Object(&value)],
    )?;
    env.call_method(
        ctx,
        "sendBroadcast",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&intent)],
    )?;
    Ok(())
}

/// Follows the direct-link setting: [`ble::keep`].
pub fn keep_watch() {
    ble::keep();
}

// --- Sharing ---------------------------------------------------------------

/// Opens the share sheet for the file at `path` (in the store's
/// `exports/`), of type `mime`: `ACTION_SEND` with the file as a
/// `content://` uri from the app's `FileProvider`, readable by whichever
/// app is picked, in Android's chooser.
pub fn share_file(path: &Path, mime: &str) -> Result<(), String> {
    let Some(jvm) = JVM.get() else {
        return Err("The app isn't ready to share yet.".into());
    };
    let mut env = jvm
        .vm
        .attach_current_thread_permanently()
        .map_err(|e| format!("Couldn't share it ({e})."))?;
    let r = env.with_local_frame(32, |env| share(env, jvm.ctx.as_obj(), path, mime));
    r.map_err(|e| {
        let thrown = take_exception(&mut env);
        log::error!("share {}: {e} {thrown:?}", path.display());
        "The phone couldn't share the file.".to_string()
    })
}

fn share(env: &mut JNIEnv, ctx: &JObject, path: &Path, mime: &str) -> JResult<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = env.new_string(path.to_string_lossy())?;
    let file = env.new_object(
        "java/io/File",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&path)],
    )?;
    let provider = app_class(env, ctx, "androidx.core.content.FileProvider")?;
    let authority = env.new_string(FILES_AUTHORITY)?;
    let uri = env
        .call_static_method(
            &provider,
            "getUriForFile",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;",
            &[
                JValue::Object(ctx),
                JValue::Object(&authority),
                JValue::Object(&file),
            ],
        )?
        .l()?;

    let action = env.new_string("android.intent.action.SEND")?;
    let intent = env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&action)],
    )?;
    let mime = env.new_string(mime)?;
    env.call_method(
        &intent,
        "setType",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&mime)],
    )?;
    let stream = env.new_string("android.intent.extra.STREAM")?;
    env.call_method(
        &intent,
        "putExtra",
        "(Ljava/lang/String;Landroid/os/Parcelable;)Landroid/content/Intent;",
        &[JValue::Object(&stream), JValue::Object(&uri)],
    )?;
    let subject = env.new_string("android.intent.extra.SUBJECT")?;
    let title = env.new_string(&name)?;
    env.call_method(
        &intent,
        "putExtra",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&subject), JValue::Object(&title)],
    )?;
    // The uri as the intent's clip too: that is what the read grant covers,
    // through the chooser to the app picked.
    let clip = env
        .call_static_method(
            "android/content/ClipData",
            "newRawUri",
            "(Ljava/lang/CharSequence;Landroid/net/Uri;)Landroid/content/ClipData;",
            &[JValue::Object(&title), JValue::Object(&uri)],
        )?
        .l()?;
    env.call_method(
        &intent,
        "setClipData",
        "(Landroid/content/ClipData;)V",
        &[JValue::Object(&clip)],
    )?;
    env.call_method(
        &intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[JValue::Int(FLAG_GRANT_READ_URI_PERMISSION)],
    )?;
    let chooser = env
        .call_static_method(
            "android/content/Intent",
            "createChooser",
            "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;",
            &[JValue::Object(&intent), JValue::Object(&title)],
        )?
        .l()?;
    // From the activity, so the chooser opens over the app; from the
    // application context it needs a task of its own.
    let from = match activity() {
        Some(activity) => env.new_local_ref(activity.as_obj())?,
        None => {
            env.call_method(
                &chooser,
                "addFlags",
                "(I)Landroid/content/Intent;",
                &[JValue::Int(FLAG_ACTIVITY_NEW_TASK)],
            )?;
            env.new_local_ref(ctx)?
        }
    };
    env.call_method(
        &from,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&chooser)],
    )?;
    Ok(())
}

// --- The look --------------------------------------------------------------

/// Whether the phone is set to dark mode (`Configuration.uiMode`).
pub fn system_dark() -> Option<bool> {
    const UI_MODE_NIGHT_MASK: i32 = 0x30;
    const UI_MODE_NIGHT_YES: i32 = 0x20;
    let mut out = None;
    with_env(
        |env, ctx| {
            let res = resources(env, ctx)?;
            let config = env
                .call_method(
                    &res,
                    "getConfiguration",
                    "()Landroid/content/res/Configuration;",
                    &[],
                )?
                .l()?;
            let mode = env.get_field(&config, "uiMode", "I")?.i()?;
            out = Some(mode & UI_MODE_NIGHT_MASK == UI_MODE_NIGHT_YES);
            Ok(())
        },
        "dark mode",
    );
    out
}

/// Dark status and navigation bar icons for a light theme, light ones for
/// a dark theme (`ThemeStub.lightBars`, on the UI thread).
pub fn light_bars(light: bool) {
    let Some(activity) = activity() else {
        return;
    };
    with_env(
        |env, ctx| {
            let stub = app_class(env, ctx, &format!("{PKG}.ThemeStub"))?;
            env.call_static_method(
                &stub,
                "lightBars",
                "(Landroid/app/Activity;Z)V",
                &[
                    JValue::Object(activity.as_obj()),
                    JValue::Bool(light.into()),
                ],
            )?;
            Ok(())
        },
        "system bar colours",
    );
}

fn resources<'l>(env: &mut JNIEnv<'l>, ctx: &JObject) -> JResult<JObject<'l>> {
    env.call_method(
        ctx,
        "getResources",
        "()Landroid/content/res/Resources;",
        &[],
    )?
    .l()
}

/// A system bar's height in pixels (`status_bar_height`,
/// `navigation_bar_height`), so the UI can keep clear of it.
fn system_dimen_px(name: &str) -> f32 {
    let mut px = 0.0;
    with_env(
        |env, ctx| {
            let res = resources(env, ctx)?;
            let name = env.new_string(name)?;
            let kind = env.new_string("dimen")?;
            let pkg = env.new_string("android")?;
            let id = env
                .call_method(
                    &res,
                    "getIdentifier",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
                    &[
                        JValue::Object(&name),
                        JValue::Object(&kind),
                        JValue::Object(&pkg),
                    ],
                )?
                .i()?;
            if id != 0 {
                px = env
                    .call_method(&res, "getDimensionPixelSize", "(I)I", &[JValue::Int(id)])?
                    .i()? as f32;
            }
            Ok(())
        },
        "system bar height",
    );
    px
}

/// The status bar's height, pixels.
pub fn status_bar_px() -> f32 {
    static PX: OnceLock<f32> = OnceLock::new();
    *PX.get_or_init(|| system_dimen_px("status_bar_height"))
}

/// The navigation bar's height, pixels.
pub fn nav_bar_px() -> f32 {
    static PX: OnceLock<f32> = OnceLock::new();
    *PX.get_or_init(|| system_dimen_px("navigation_bar_height"))
}

/// The screen's density (pixels per dp), which egui uses as pixels per
/// point, so sizes match other Android apps.
pub fn density() -> Option<f32> {
    let mut out = None;
    with_env(
        |env, ctx| {
            let res = resources(env, ctx)?;
            let metrics = env
                .call_method(
                    &res,
                    "getDisplayMetrics",
                    "()Landroid/util/DisplayMetrics;",
                    &[],
                )?
                .l()?;
            out = Some(env.get_field(&metrics, "density", "F")?.f()?);
            Ok(())
        },
        "density",
    );
    out
}

// --- Notifications ---------------------------------------------------------

/// A notification channel: its id, the name in the phone's settings, and
/// how loudly it alerts (1 is silent, with no icon in the status bar).
struct Channel {
    id: &'static str,
    name: &'static str,
    importance: i32,
}

/// Builds a notification in `channel` (made first: a service can start
/// before the activity ever did) that opens the app when tapped.
/// `WatchService`'s is the only one.
fn notification<'l>(
    env: &mut JNIEnv<'l>,
    ctx: &JObject,
    channel: &Channel,
    title: &str,
    body: &str,
) -> JResult<JObject<'l>> {
    let nm = system_service(env, ctx, "notification")?;
    let id = env.new_string(channel.id)?;
    let name = env.new_string(channel.name)?;
    let made = env.new_object(
        "android/app/NotificationChannel",
        "(Ljava/lang/String;Ljava/lang/CharSequence;I)V",
        &[
            JValue::Object(&id),
            JValue::Object(&name),
            JValue::Int(channel.importance),
        ],
    )?;
    env.call_method(
        &nm,
        "createNotificationChannel",
        "(Landroid/app/NotificationChannel;)V",
        &[JValue::Object(&made)],
    )?;

    // The app's mark (res/drawable/ic_notification.xml), else Bluetooth's.
    let icon = {
        let res = resources(env, ctx)?;
        let name = env.new_string("ic_notification")?;
        let kind = env.new_string("drawable")?;
        let pkg = env.new_string(PKG)?;
        let id = env
            .call_method(
                &res,
                "getIdentifier",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
                &[
                    JValue::Object(&name),
                    JValue::Object(&kind),
                    JValue::Object(&pkg),
                ],
            )?
            .i()?;
        if id != 0 {
            id
        } else {
            env.get_static_field("android/R$drawable", "stat_sys_data_bluetooth", "I")?
                .i()?
        }
    };
    let pm = env
        .call_method(
            ctx,
            "getPackageManager",
            "()Landroid/content/pm/PackageManager;",
            &[],
        )?
        .l()?;
    let pkg = env
        .call_method(ctx, "getPackageName", "()Ljava/lang/String;", &[])?
        .l()?;
    let launch = env
        .call_method(
            &pm,
            "getLaunchIntentForPackage",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&pkg)],
        )?
        .l()?;
    let open = env
        .call_static_method(
            "android/app/PendingIntent",
            "getActivity",
            "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;",
            &[
                JValue::Object(ctx),
                JValue::Int(0),
                JValue::Object(&launch),
                JValue::Int(FLAG_UPDATE_CURRENT | FLAG_IMMUTABLE),
            ],
        )?
        .l()?;

    let title = env.new_string(title)?;
    let text = env.new_string(body)?;
    let b = env.new_object(
        "android/app/Notification$Builder",
        "(Landroid/content/Context;Ljava/lang/String;)V",
        &[JValue::Object(ctx), JValue::Object(&id)],
    )?;
    const B: &str = "Landroid/app/Notification$Builder;";
    env.call_method(&b, "setSmallIcon", format!("(I){B}"), &[JValue::Int(icon)])?;
    env.call_method(
        &b,
        "setContentTitle",
        format!("(Ljava/lang/CharSequence;){B}"),
        &[JValue::Object(&title)],
    )?;
    env.call_method(
        &b,
        "setContentText",
        format!("(Ljava/lang/CharSequence;){B}"),
        &[JValue::Object(&text)],
    )?;
    env.call_method(
        &b,
        "setContentIntent",
        format!("(Landroid/app/PendingIntent;){B}"),
        &[JValue::Object(&open)],
    )?;
    env.call_method(&b, "setOngoing", format!("(Z){B}"), &[JValue::Bool(1)])?;
    env.call_method(&b, "build", "()Landroid/app/Notification;", &[])?
        .l()
}

// --- Events from the Java stubs --------------------------------------------

/// Runs what a receiver or service passed in: keeps the JVM and context
/// ([`init`]), opens the process's store from the files directory if
/// nothing has yet (the process may have started just for this), then runs
/// `f`. A panic is caught and logged: it mustn't unwind into Java.
fn receive(
    env: &mut JNIEnv,
    context: &JObject,
    what: &str,
    f: impl FnOnce(&mut JNIEnv) -> JResult<()>,
) {
    init_logging();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let r = init(env, context);
        check(env, "init", r);
        let Some(dir) = files_dir() else {
            return log::error!("{what}: no files directory");
        };
        if let Err(e) = super::open(&dir) {
            return log::error!("{what}: can't open the store: {e}");
        }
        let r = f(env);
        check(env, what, r);
    }));
    if r.is_err() {
        log::error!("{what} panicked");
    }
}

/// Gadgetbridge turned a watch intent message into this broadcast (the
/// watch link's "Watch to phone"). Only the extras named in
/// [`crate::link::EXTRAS`] are read; [`crate::platform::from_watch`] checks
/// them and runs the event through the sync, whose answer goes to the watch
/// from here, with or without the UI.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_WatchReceiver_nativeWatch<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    context: JObject<'l>,
    intent: JObject<'l>,
) {
    receive(&mut env, &context, "watch event", |env| {
        let mut extras = std::collections::BTreeMap::new();
        for name in crate::link::EXTRAS {
            let key = env.new_string(name)?;
            let v = env
                .call_method(
                    &intent,
                    "getStringExtra",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&key)],
                )?
                .l()?;
            env.delete_local_ref(key)?;
            if v.is_null() {
                continue;
            }
            let v = JString::from(v);
            let s: String = env.get_string(&v)?.into();
            env.delete_local_ref(v)?;
            extras.insert(name.to_string(), s);
        }
        super::from_watch(&extras);
        Ok(())
    });
}

/// After a reboot or an app update (`BootReceiver`): the direct link to the
/// watch, and `WatchService`, if that's on.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cbishopvelti_beep_BootReceiver_nativeBoot<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    context: JObject<'l>,
) {
    receive(&mut env, &context, "boot", |_| {
        if crate::watch_ble::direct() {
            ble::keep();
        }
        Ok(())
    });
}
