package com.cbishopvelti.beep;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;

/**
 * Keeps the app running while it's connected to the watch directly (an
 * option in Settings), as Gadgetbridge's own service does for it: a
 * foreground service, with its notification. Rust
 * (src/platform/android/ble.rs) puts it in the foreground and holds the
 * connection; this only tells it when the service starts and stops.
 */
public final class WatchService extends Service {
    static {
        System.loadLibrary("beep_android");
    }

    private native void nativeStarted(Service service);

    private native void nativeStopped();

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        nativeStarted(this);
        return START_STICKY;
    }

    @Override
    public void onDestroy() {
        nativeStopped();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
