package com.cbishopvelti.beep;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * After a reboot or an app update: Rust (src/platform/android.rs) starts
 * WatchService again if the direct link to the watch is on.
 */
public final class BootReceiver extends BroadcastReceiver {
    static {
        System.loadLibrary("beep_android");
    }

    private native void nativeBoot(Context context);

    @Override
    public void onReceive(Context context, Intent intent) {
        nativeBoot(context);
    }
}
