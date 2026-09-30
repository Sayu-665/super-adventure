package com.opensurf.browser.core;

import java.util.Locale;
import java.util.regex.Pattern;

/** Decides what happens when web content navigates to a URL (SPEC I). */
public final class NavigationPolicy {
    /** What to do with a navigation request. */
    public enum Action {
        /** Let the WebView load it (http, https, about, blob; data only in sub-frames). */
        LOAD,
        /** The home page's opensurf://go search request: resolve natively. */
        GO,
        /** An intent: URI to hand to another app after sanitising. */
        INTENT,
        /** Another non-web scheme (mailto:, tel:, sms:, geo:, market:, ...) for the OS. */
        EXTERNAL,
        /** Local or script schemes pages must never reach (file:, content:, javascript:, ...). */
        BLOCK
    }

    private static final Pattern SCHEME = Pattern.compile("^[A-Za-z][A-Za-z0-9+.-]*:.*", Pattern.DOTALL);

    private NavigationPolicy() {
    }

    public static Action classify(String url, boolean isMainFrame) {
        String scheme = schemeOf(url);
        if (scheme == null) {
            return Action.BLOCK;
        }
        switch (scheme) {
            case "http":
            case "https":
            case "about":
            case "blob":
                return Action.LOAD;
            case "data":
                // Top-level data: navigations are a phishing vector; sub-frames are fine.
                return isMainFrame ? Action.BLOCK : Action.LOAD;
            case "opensurf":
                return isMainFrame && HomePage.isGoUrl(url) ? Action.GO : Action.BLOCK;
            case "javascript":
            case "file":
            case "content":
            case "jar":
            case "chrome":
            case "chrome-native":
            case "chrome-extension":
            case "view-source":
            case "ws":
            case "wss":
                return Action.BLOCK;
            case "intent":
                return isMainFrame ? Action.INTENT : Action.BLOCK;
            default:
                // Sub-frames may not launch other apps.
                return isMainFrame ? Action.EXTERNAL : Action.BLOCK;
        }
    }

    /** Lower-case scheme of {@code url}, or {@code null} when it has none. */
    public static String schemeOf(String url) {
        if (url == null || !SCHEME.matcher(url).matches()) {
            return null;
        }
        return url.substring(0, url.indexOf(':')).toLowerCase(Locale.ROOT);
    }

    /** True for http(s) URLs, the only ones restored into tabs or loaded from fallbacks. */
    public static boolean isWebUrl(String url) {
        String scheme = schemeOf(url);
        return "http".equals(scheme) || "https".equals(scheme);
    }
}
