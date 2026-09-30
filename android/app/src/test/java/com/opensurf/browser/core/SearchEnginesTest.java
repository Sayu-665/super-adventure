package com.opensurf.browser.core;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/** Every engine in both SafeSearch states, plus custom-template validation (SPEC B). */
public class SearchEnginesTest {
    private static final String QUERY = "a b&c#d+e?";
    private static final String ENCODED = "a%20b%26c%23d%2Be%3F";

    private static void assertEngine(String id, String name, String off, String on) {
        SearchEngine engine = SearchEngines.byId(id);
        assertEquals(name, engine.getName());
        assertEquals(off.replace("{q}", ENCODED), engine.searchUrl(QUERY, false));
        assertEquals(on.replace("{q}", ENCODED), engine.searchUrl(QUERY, true));
        // The same through SearchConfig, which is what the app uses.
        assertEquals(off.replace("{q}", ENCODED), new SearchConfig(id, false, "").searchUrl(QUERY));
        assertEquals(on.replace("{q}", ENCODED), new SearchConfig(id, true, "").searchUrl(QUERY));
    }

    @Test
    public void duckDuckGo() {
        assertEngine("duckduckgo", "DuckDuckGo",
                "https://duckduckgo.com/?q={q}&kp=-2", "https://duckduckgo.com/?q={q}&kp=1");
    }

    @Test
    public void google() {
        assertEngine("google", "Google",
                "https://www.google.com/search?q={q}&safe=off",
                "https://www.google.com/search?q={q}&safe=active");
    }

    @Test
    public void bing() {
        assertEngine("bing", "Bing",
                "https://www.bing.com/search?q={q}&adlt=off",
                "https://www.bing.com/search?q={q}&adlt=strict");
    }

    @Test
    public void brave() {
        assertEngine("brave", "Brave Search",
                "https://search.brave.com/search?q={q}&safesearch=off",
                "https://search.brave.com/search?q={q}&safesearch=strict");
    }

    @Test
    public void startpage() {
        assertEngine("startpage", "Startpage",
                "https://www.startpage.com/sp/search?query={q}&qadf=none",
                "https://www.startpage.com/sp/search?query={q}&qadf=heavy");
    }

    @Test
    public void mojeek() {
        assertEngine("mojeek", "Mojeek",
                "https://www.mojeek.com/search?q={q}&safe=0",
                "https://www.mojeek.com/search?q={q}&safe=1");
    }

    @Test
    public void catalogueOrderAndDefault() {
        assertEquals(6, SearchEngines.builtIn().size());
        assertEquals("duckduckgo", SearchEngines.defaultEngine().getId());
        assertEquals(SearchEngines.DEFAULT_ID, SearchEngines.builtIn().get(0).getId());
        assertNull(SearchEngines.byId("custom"));
        assertNull(SearchEngines.byId("nope"));
    }

    @Test
    public void defaultsAreDuckDuckGoWithSafeSearchOff() {
        SearchConfig config = SearchConfig.defaults();
        assertFalse(config.isSafeSearch());
        assertEquals("DuckDuckGo", config.engineName());
        assertEquals("https://duckduckgo.com/?q=x&kp=-2", config.searchUrl("x"));
    }

    @Test
    public void unknownEngineFallsBackToDefault() {
        assertEquals("https://duckduckgo.com/?q=x&kp=1", new SearchConfig("gone", true, "").searchUrl("x"));
        assertEquals("https://duckduckgo.com/?q=x&kp=-2", new SearchConfig(null, false, null).searchUrl("x"));
    }

    @Test
    public void customTemplateValidation() {
        assertTrue(SearchEngines.isValidCustomTemplate("https://example.com/search?q=%s"));
        assertTrue(SearchEngines.isValidCustomTemplate("http://example.com/?q=%s"));
        assertTrue(SearchEngines.isValidCustomTemplate("HTTPS://Example.com/s/%s"));
        assertTrue(SearchEngines.isValidCustomTemplate("  https://example.com/?q=%s  "));
        assertFalse(SearchEngines.isValidCustomTemplate(null));
        assertFalse(SearchEngines.isValidCustomTemplate(""));
        assertFalse(SearchEngines.isValidCustomTemplate("https://example.com/search?q="));
        assertFalse(SearchEngines.isValidCustomTemplate("example.com/search?q=%s"));
        assertFalse(SearchEngines.isValidCustomTemplate("ftp://example.com/?q=%s"));
        assertFalse(SearchEngines.isValidCustomTemplate("javascript:alert('%s')"));
        assertFalse(SearchEngines.isValidCustomTemplate("file:///%s"));
        assertFalse(SearchEngines.isValidCustomTemplate("https://example.com/?q={q}"));
    }

    @Test
    public void customTemplateReplacesEveryPlaceholder() {
        SearchConfig config = new SearchConfig("custom", false, "https://example.com/%s?q=%s");
        assertTrue(config.isCustom());
        assertEquals("Custom", config.engineName());
        assertEquals("https://example.com/" + ENCODED + "?q=" + ENCODED, config.searchUrl(QUERY));
    }

    @Test
    public void safeSearchNeverModifiesCustomTemplate() {
        String template = "https://example.com/search?q=%s&safe=whatever";
        assertEquals("https://example.com/search?q=x&safe=whatever",
                new SearchConfig("custom", false, template).searchUrl("x"));
        assertEquals("https://example.com/search?q=x&safe=whatever",
                new SearchConfig("custom", true, template).searchUrl("x"));
    }

    @Test
    public void invalidCustomTemplateFallsBackToDefaultEngine() {
        SearchConfig config = new SearchConfig("custom", true, "not a url %s");
        assertFalse(config.isCustom());
        assertEquals("DuckDuckGo", config.engineName());
        assertEquals("https://duckduckgo.com/?q=x&kp=1", config.searchUrl("x"));
    }

    @Test
    public void queryPlaceholderInQueryIsNotExpandedTwice() {
        SearchConfig config = new SearchConfig("custom", false, "https://example.com/?q=%s");
        assertEquals("https://example.com/?q=%25s", config.searchUrl("%s"));
        assertEquals("https://duckduckgo.com/?q=%7Bq%7D&kp=-2", SearchConfig.defaults().searchUrl("{q}"));
    }

    @Test
    public void encodeQuery() {
        assertEquals("hello%20world", SearchEngines.encodeQuery("hello world"));
        assertEquals("2%2B2%3D4", SearchEngines.encodeQuery("2+2=4"));
        assertEquals("%E4%BD%A0%E5%A5%BD", SearchEngines.encodeQuery("你好"));
        assertEquals("", SearchEngines.encodeQuery(null));
    }
}
