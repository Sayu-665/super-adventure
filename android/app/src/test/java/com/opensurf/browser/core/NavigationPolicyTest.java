package com.opensurf.browser.core;

import static com.opensurf.browser.core.NavigationPolicy.Action.BLOCK;
import static com.opensurf.browser.core.NavigationPolicy.Action.EXTERNAL;
import static com.opensurf.browser.core.NavigationPolicy.Action.GO;
import static com.opensurf.browser.core.NavigationPolicy.Action.INTENT;
import static com.opensurf.browser.core.NavigationPolicy.Action.LOAD;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class NavigationPolicyTest {
    @Test
    public void webSchemesLoad() {
        assertEquals(LOAD, NavigationPolicy.classify("https://example.com", true));
        assertEquals(LOAD, NavigationPolicy.classify("HTTP://example.com", false));
        assertEquals(LOAD, NavigationPolicy.classify("about:blank", true));
        assertEquals(LOAD, NavigationPolicy.classify("blob:https://example.com/uuid", true));
    }

    @Test
    public void dataOnlyInSubFrames() {
        assertEquals(BLOCK, NavigationPolicy.classify("data:text/html,hi", true));
        assertEquals(LOAD, NavigationPolicy.classify("data:text/html,hi", false));
    }

    @Test
    public void homeSearchIsHandledNatively() {
        assertEquals(GO, NavigationPolicy.classify("opensurf://go?q=x", true));
        assertEquals(BLOCK, NavigationPolicy.classify("opensurf://go?q=x", false));
        assertEquals(BLOCK, NavigationPolicy.classify("opensurf://other", true));
    }

    @Test
    public void localAndScriptSchemesAreBlocked() {
        assertEquals(BLOCK, NavigationPolicy.classify("file:///sdcard/x.html", true));
        assertEquals(BLOCK, NavigationPolicy.classify("file:///android_asset/home.html", true));
        assertEquals(BLOCK, NavigationPolicy.classify("content://com.example/x", true));
        assertEquals(BLOCK, NavigationPolicy.classify("javascript:alert(1)", true));
        assertEquals(BLOCK, NavigationPolicy.classify("JavaScript:alert(1)", false));
        assertEquals(BLOCK, NavigationPolicy.classify("chrome://settings", true));
        assertEquals(BLOCK, NavigationPolicy.classify("not a url", true));
        assertEquals(BLOCK, NavigationPolicy.classify("", true));
        assertEquals(BLOCK, NavigationPolicy.classify(null, true));
    }

    @Test
    public void otherSchemesGoToTheOsFromTheMainFrameOnly() {
        assertEquals(INTENT, NavigationPolicy.classify("intent://scan/#Intent;scheme=zxing;end", true));
        assertEquals(BLOCK, NavigationPolicy.classify("intent://scan/#Intent;scheme=zxing;end", false));
        for (String url : new String[] {"mailto:a@b.c", "tel:+123", "sms:123", "geo:0,0",
                "market://details?id=x", "whatsapp://send"}) {
            assertEquals(url, EXTERNAL, NavigationPolicy.classify(url, true));
            assertEquals(url, BLOCK, NavigationPolicy.classify(url, false));
        }
    }

    @Test
    public void schemeHelpers() {
        assertEquals("https", NavigationPolicy.schemeOf("HTTPS://x"));
        assertNull(NavigationPolicy.schemeOf("/relative"));
        assertTrue(NavigationPolicy.isWebUrl("http://x"));
        assertFalse(NavigationPolicy.isWebUrl("file:///x"));
        assertFalse(NavigationPolicy.isWebUrl(null));
    }
}
