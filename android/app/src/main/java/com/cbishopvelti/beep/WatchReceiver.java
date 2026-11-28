package com.cbishopvelti.beep;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * The watch's events (state, log, logend, busy), as Gadgetbridge's broadcast
 * of the watch's intent message. Rust (src/platform/android.rs) reads the
 * extras, checks them and runs them through the sync, with or without the
 * app open.
 */
public final class WatchReceiver extends BroadcastReceiver {
    static {
        System.loadLibrary("beep_android");
    }

    private native void nativeWatch(Context context, Intent intent);

    @Override
    public void onReceive(Context context, Intent intent) {
        nativeWatch(context, intent);
    }
}
