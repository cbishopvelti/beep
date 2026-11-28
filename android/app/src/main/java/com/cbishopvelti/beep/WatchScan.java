package com.cbishopvelti.beep;

import android.bluetooth.le.ScanCallback;
import android.bluetooth.le.ScanRecord;
import android.bluetooth.le.ScanResult;

/**
 * What a Bluetooth scan sees while Settings looks for the watch to connect
 * to directly, or while the direct link looks for the watch it waits for
 * (before Android 13, until Android has seen it since Bluetooth came on);
 * Rust (src/platform/android/ble.rs) keeps the watches and reconnects.
 */
public final class WatchScan extends ScanCallback {
    static {
        System.loadLibrary("beep_android");
    }

    private native void nativeFound(String address, String name, int rssi);

    @Override
    public void onScanResult(int callbackType, ScanResult result) {
        ScanRecord record = result.getScanRecord();
        String name = record == null ? null : record.getDeviceName();
        if (name != null) {
            nativeFound(result.getDevice().getAddress(), name, result.getRssi());
        }
    }
}
