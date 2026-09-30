package com.opensurf.browser;

import android.graphics.Insets;
import android.graphics.Rect;
import android.os.Build;
import android.view.DisplayCutout;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;

/**
 * Edge-to-edge support. Android 15 (targetSdk 35) always draws apps behind the system bars, so
 * the app draws edge-to-edge on every API level and pads its root view by the system bars,
 * display cutout and keyboard insets instead of relying on the platform.
 */
final class SystemBars {
    private SystemBars() {
    }

    @SuppressWarnings("deprecation")
    static void enableEdgeToEdge(Window window) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false);
        } else {
            View decor = window.getDecorView();
            decor.setSystemUiVisibility(decor.getSystemUiVisibility()
                    | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
        }
    }

    /** Pads {@code view} so nothing is hidden by the bars, a cutout or the soft keyboard. */
    static void applyInsetsAsPadding(View view) {
        view.setOnApplyWindowInsetsListener((v, insets) -> {
            Rect safe = safeInsets(insets);
            v.setPadding(safe.left, safe.top, safe.right, safe.bottom);
            return consume(insets);
        });
        view.requestApplyInsets();
    }

    @SuppressWarnings("deprecation")
    private static Rect safeInsets(WindowInsets insets) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Insets i = insets.getInsets(WindowInsets.Type.systemBars()
                    | WindowInsets.Type.displayCutout() | WindowInsets.Type.ime());
            return new Rect(i.left, i.top, i.right, i.bottom);
        }
        // Before API 30 the system window insets include the keyboard (adjustResize).
        Rect safe = new Rect(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(),
                insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            DisplayCutout cutout = insets.getDisplayCutout();
            if (cutout != null) {
                safe.left = Math.max(safe.left, cutout.getSafeInsetLeft());
                safe.top = Math.max(safe.top, cutout.getSafeInsetTop());
                safe.right = Math.max(safe.right, cutout.getSafeInsetRight());
                safe.bottom = Math.max(safe.bottom, cutout.getSafeInsetBottom());
            }
        }
        return safe;
    }

    @SuppressWarnings("deprecation")
    private static WindowInsets consume(WindowInsets insets) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return WindowInsets.CONSUMED;
        }
        return insets.consumeSystemWindowInsets();
    }

    /** Hides (immersive, swipe to reveal) or shows the system bars, e.g. for fullscreen video. */
    @SuppressWarnings("deprecation")
    static void setImmersive(Window window, boolean immersive) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = window.getInsetsController();
            if (controller == null) {
                return;
            }
            if (immersive) {
                controller.setSystemBarsBehavior(
                        WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                controller.hide(WindowInsets.Type.systemBars());
            } else {
                controller.show(WindowInsets.Type.systemBars());
            }
        } else {
            View decor = window.getDecorView();
            int flags = View.SYSTEM_UI_FLAG_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY;
            int current = decor.getSystemUiVisibility();
            decor.setSystemUiVisibility(immersive ? current | flags : current & ~flags);
        }
    }
}
