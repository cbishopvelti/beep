package com.cbishopvelti.beep;

import android.bluetooth.BluetoothGatt;
import android.bluetooth.BluetoothGattCallback;
import android.bluetooth.BluetoothGattCharacteristic;
import android.bluetooth.BluetoothGattDescriptor;

/**
 * The direct Bluetooth link to the watch (an option in Settings): Android
 * reports the connection here, and each callback goes straight to Rust
 * (src/platform/android/ble.rs), which does everything else.
 */
public final class WatchGatt extends BluetoothGattCallback {
    static {
        System.loadLibrary("beep_android");
    }

    private native void nativeState(BluetoothGatt gatt, int status, int state);

    private native void nativeMtu(BluetoothGatt gatt, int mtu, int status);

    private native void nativeServices(BluetoothGatt gatt, int status);

    private native void nativeDescriptor(BluetoothGatt gatt, int status);

    private native void nativeWritten(BluetoothGatt gatt, int status);

    private native void nativeChanged(BluetoothGatt gatt, byte[] value);

    @Override
    public void onConnectionStateChange(BluetoothGatt gatt, int status, int newState) {
        nativeState(gatt, status, newState);
    }

    @Override
    public void onMtuChanged(BluetoothGatt gatt, int mtu, int status) {
        nativeMtu(gatt, mtu, status);
    }

    @Override
    public void onServicesDiscovered(BluetoothGatt gatt, int status) {
        nativeServices(gatt, status);
    }

    @Override
    public void onDescriptorWrite(BluetoothGatt gatt, BluetoothGattDescriptor descriptor, int status) {
        nativeDescriptor(gatt, status);
    }

    @Override
    public void onCharacteristicWrite(
            BluetoothGatt gatt, BluetoothGattCharacteristic characteristic, int status) {
        nativeWritten(gatt, status);
    }

    /** Android 13 and later. */
    @Override
    public void onCharacteristicChanged(
            BluetoothGatt gatt, BluetoothGattCharacteristic characteristic, byte[] value) {
        nativeChanged(gatt, value);
    }

    /** Before Android 13 (from 13 on, the one above is called instead). */
    @Override
    @SuppressWarnings("deprecation")
    public void onCharacteristicChanged(
            BluetoothGatt gatt, BluetoothGattCharacteristic characteristic) {
        byte[] value = characteristic.getValue();
        if (value != null) {
            nativeChanged(gatt, value);
        }
    }
}
