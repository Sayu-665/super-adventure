package com.opensurf.browser.core;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;

import org.junit.Test;

public class UserAgentsTest {
    @Test
    public void derivesDesktopChromeUserAgent() {
        String mobile = "Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/UQ1A.240205.004; wv) "
                + "AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/126.0.6478.71 "
                + "Mobile Safari/537.36";
        String desktop = UserAgents.desktopFrom(mobile);
        assertEquals("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) "
                + "Chrome/126.0.6478.71 Safari/537.36", desktop);
        assertFalse(desktop.contains("Mobile"));
        assertFalse(desktop.contains("Android"));
        assertFalse(desktop.contains("; wv"));
    }

    @Test
    public void fallsBackWhenNoChromeVersion() {
        assertEquals("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) "
                + "Chrome/130.0.0.0 Safari/537.36", UserAgents.desktopFrom(null));
    }
}
