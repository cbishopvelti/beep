package com.cbishopvelti.beep;

import android.app.Activity;
import android.os.Build;
import android.view.View;
import android.view.WindowInsetsController;

/**
 * Sets the status and navigation bar icons dark (on a light theme) or light,
 * for src/platform/android.rs. The window has to be changed on the UI thread.
 */
public final class ThemeStub {
    private ThemeStub() {}

    public static void lightBars(final Activity activity, final boolean light) {
        activity.runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT >= 30) {
                int mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                        | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                WindowInsetsController c = activity.getWindow().getInsetsController();
                if (c != null) {
                    c.setSystemBarsAppearance(light ? mask : 0, mask);
                }
            } else {
                View decor = activity.getWindow().getDecorView();
                int mask = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
                        | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
                int flags = decor.getSystemUiVisibility() & ~mask;
                decor.setSystemUiVisibility(light ? flags | mask : flags);
            }
        });
    }
}
