package com.opensurf.browser.core;

import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** User-agent helpers for the "Desktop site" mode. */
public final class UserAgents {
    private static final Pattern CHROME_VERSION = Pattern.compile("Chrome/([0-9][0-9.]*)");
    private static final String FALLBACK_CHROME_VERSION = "130.0.0.0";

    private UserAgents() {
    }

    /**
     * Derives a desktop Chrome UA from the WebView's default mobile UA: the Android platform
     * token, "; wv", "Version/4.0" and "Mobile" are dropped, keeping the real Chrome version.
     */
    public static String desktopFrom(String mobileUserAgent) {
        String version = FALLBACK_CHROME_VERSION;
        if (mobileUserAgent != null) {
            Matcher m = CHROME_VERSION.matcher(mobileUserAgent);
            if (m.find()) {
                version = m.group(1);
            }
        }
        return "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/"
                + version + " Safari/537.36";
    }
}
