package com.opensurf.browser.core;

import java.io.UnsupportedEncodingException;
import java.net.URLEncoder;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;

/** Catalogue of search engines (SPEC B). SafeSearch is OFF unless the user turns it on. */
public final class SearchEngines {
    public static final String DEFAULT_ID = "duckduckgo";
    public static final String CUSTOM_ID = "custom";
    public static final String CUSTOM_NAME = "Custom";
    /** Placeholder a custom template must contain; it is replaced by the encoded query. */
    public static final String CUSTOM_PLACEHOLDER = "%s";

    private static final List<SearchEngine> BUILT_IN = Collections.unmodifiableList(Arrays.asList(
            new SearchEngine("duckduckgo", "DuckDuckGo",
                    "https://duckduckgo.com/?q={q}&kp=-2",
                    "https://duckduckgo.com/?q={q}&kp=1"),
            new SearchEngine("google", "Google",
                    "https://www.google.com/search?q={q}&safe=off",
                    "https://www.google.com/search?q={q}&safe=active"),
            new SearchEngine("bing", "Bing",
                    "https://www.bing.com/search?q={q}&adlt=off",
                    "https://www.bing.com/search?q={q}&adlt=strict"),
            new SearchEngine("brave", "Brave Search",
                    "https://search.brave.com/search?q={q}&safesearch=off",
                    "https://search.brave.com/search?q={q}&safesearch=strict"),
            new SearchEngine("startpage", "Startpage",
                    "https://www.startpage.com/sp/search?query={q}&qadf=none",
                    "https://www.startpage.com/sp/search?query={q}&qadf=heavy"),
            new SearchEngine("mojeek", "Mojeek",
                    "https://www.mojeek.com/search?q={q}&safe=0",
                    "https://www.mojeek.com/search?q={q}&safe=1")));

    private SearchEngines() {
    }

    /** Built-in engines in display order; the first one is the default. */
    public static List<SearchEngine> builtIn() {
        return BUILT_IN;
    }

    /** Returns the built-in engine with this id, or {@code null} (also for "custom"). */
    public static SearchEngine byId(String id) {
        for (SearchEngine engine : BUILT_IN) {
            if (engine.getId().equals(id)) {
                return engine;
            }
        }
        return null;
    }

    public static SearchEngine defaultEngine() {
        return BUILT_IN.get(0);
    }

    /** A custom template must start with http:// or https:// and contain "%s". */
    public static boolean isValidCustomTemplate(String template) {
        if (template == null) {
            return false;
        }
        String t = template.trim();
        return (startsWithIgnoreCase(t, "http://") || startsWithIgnoreCase(t, "https://"))
                && t.contains(CUSTOM_PLACEHOLDER);
    }

    /** Replaces every "%s" in a (valid) custom template with the encoded query. */
    public static String applyCustomTemplate(String template, String query) {
        return template.trim().replace(CUSTOM_PLACEHOLDER, encodeQuery(query));
    }

    /**
     * UTF-8 percent-encoding suitable for both query strings and path segments: spaces become
     * "%20" and reserved characters such as "&", "#", "+", "?" and "/" are escaped.
     */
    public static String encodeQuery(String query) {
        try {
            return URLEncoder.encode(query == null ? "" : query, "UTF-8").replace("+", "%20");
        } catch (UnsupportedEncodingException e) {
            throw new IllegalStateException("UTF-8 is always supported", e);
        }
    }

    static boolean startsWithIgnoreCase(String text, String prefix) {
        return text.regionMatches(true, 0, prefix, 0, prefix.length());
    }
}
