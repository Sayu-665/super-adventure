package com.opensurf.browser.core;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class HomePageTest {
    @Test
    public void fragmentReflectsSettings() {
        assertEquals("engine=DuckDuckGo&safe=off", HomePage.fragment(SearchConfig.defaults()));
        assertEquals("engine=Brave%20Search&safe=on",
                HomePage.fragment(new SearchConfig("brave", true, "")));
        assertEquals("engine=Custom&safe=na",
                HomePage.fragment(new SearchConfig("custom", true, "https://x.example/?q=%s")));
        assertEquals("file:///android_asset/home.html#engine=Google&safe=off",
                HomePage.url(new SearchConfig("google", false, "")));
    }

    @Test
    public void recognisesHomeUrls() {
        assertTrue(HomePage.isHomeUrl("file:///android_asset/home.html"));
        assertTrue(HomePage.isHomeUrl("file:///android_asset/home.html#engine=Bing&safe=on"));
        assertFalse(HomePage.isHomeUrl("file:///android_asset/home.html.evil"));
        assertFalse(HomePage.isHomeUrl("https://example.com/home.html"));
        assertFalse(HomePage.isHomeUrl(null));
    }

    @Test
    public void recognisesGoUrls() {
        assertTrue(HomePage.isGoUrl("opensurf://go?q=x"));
        assertTrue(HomePage.isGoUrl("OpenSurf://GO/?q=x"));
        assertTrue(HomePage.isGoUrl("opensurf://go"));
        assertFalse(HomePage.isGoUrl("opensurf://gone?q=x"));
        assertFalse(HomePage.isGoUrl("opensurf://settings"));
        assertFalse(HomePage.isGoUrl("https://go?q=x"));
        assertFalse(HomePage.isGoUrl(null));
    }

    @Test
    public void decodesGoQueryFromScriptAndPlainForms() {
        // encodeURIComponent style (script path)
        assertEquals("what is 2+2", HomePage.goQuery("opensurf://go?q=what%20is%202%2B2"));
        // application/x-www-form-urlencoded style (no-JavaScript form path)
        assertEquals("what is 2+2", HomePage.goQuery("opensurf://go?q=what+is+2%2B2"));
        assertEquals("a&b", HomePage.goQuery("opensurf://go/?x=1&q=a%26b#frag"));
        assertEquals("café", HomePage.goQuery("opensurf://go?q=caf%C3%A9"));
        assertEquals("100%", HomePage.goQuery("opensurf://go?q=100%"));
        assertEquals("", HomePage.goQuery("opensurf://go"));
        assertEquals("", HomePage.goQuery("opensurf://go?q="));
        assertEquals("", HomePage.goQuery("https://example.com/?q=x"));
    }

    @Test
    public void goQueryResolvesThroughTheOmniboxFunction() {
        SearchConfig config = SearchConfig.defaults();
        assertEquals("https://example.com",
                UrlResolver.resolve(HomePage.goQuery("opensurf://go?q=example.com"), config));
        assertEquals("https://duckduckgo.com/?q=javascript%3Aalert%281%29&kp=-2",
                UrlResolver.resolve(HomePage.goQuery("opensurf://go?q=javascript%3Aalert(1)"), config));
    }
}
