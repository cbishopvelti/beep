package com.cbishopvelti.beep;

import android.app.NativeActivity;
import android.os.Process;

/**
 * The app's activity: Android's NativeActivity, which runs the Rust UI
 * (android_main in src/lib.rs), with one change. When the activity is
 * destroyed while the process lives on (swiped away in Recents while
 * WatchService keeps the process, or a configuration change the manifest
 * doesn't list), NativeActivity's onDestroy waits on the main thread until
 * android_main returns, and it never does (winit drops onDestroy, and it
 * allows one event loop per process, so it couldn't start again either).
 * The main thread would hang, and every broadcast after it with it. So the
 * process ends here instead: WatchService is START_STICKY, so Android
 * starts it again in a new process, and the next launch gets a new one too.
 * Everything the app keeps is on disk, written atomically.
 */
public class MainActivity extends NativeActivity {
    @Override
    protected void onDestroy() {
        Process.killProcess(Process.myPid());
        super.onDestroy();
    }
}
