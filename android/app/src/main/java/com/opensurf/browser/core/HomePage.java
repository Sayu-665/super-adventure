package com.opensurf.browser.core;

import java.io.UnsupportedEncodingException;
import java.net.URLDecoder;
import java.util.Locale;

/**
 * Contract between the native layer and the bundled home page (SPEC D).
 *
 * <p>The page's search form navigates to {@code opensurf://go?q=<text>}; the native layer
 * intercepts that navigation and resolves the text with {@link UrlResolver}. Display-only
 * settings are passed to the page in the URL fragment, e.g. {@code #engine=DuckDuckGo&safe=off}.
 * The page receives no bridge or privileged API.
 */
public final class HomePage {
    public static final String URL = "file:///android_asset/home.html";
    private static final String GO_PREFIX = "opensurf://go";

    private HomePage() {
    }

    /** Fragment describing the current settings; "safe" is on, off, or na for custom engines. */
    public static String fragment(SearchConfig config) {
        String safe = config.isCustom() ? "na" : (config.isSafeSearch() ? "on" : "off");
        return "engine=" + SearchEngines.encodeQuery(config.engineName()) + "&safe=" + safe;
    }

    public static String url(SearchConfig config) {
        return URL + "#" + fragment(config);
    }

    public static boolean isHomeUrl(String url) {
        return url != null && (url.equals(URL) || url.startsWith(URL + "#")
                || url.startsWith(URL + "?"));
    }

    /** True for opensurf://go, opensurf://go/, opensurf://go?q=... (scheme/host in any case). */
    public static boolean isGoUrl(String url) {
        if (url == null || !url.toLowerCase(Locale.ROOT).startsWith(GO_PREFIX)) {
            return false;
        }
        if (url.length() == GO_PREFIX.length()) {
            return true;
        }
        char next = url.charAt(GO_PREFIX.length());
        return next == '?' || next == '/' || next == '#';
    }

    /** Extracts and decodes the "q" parameter of an opensurf://go URL ("" when absent). */
    public static String goQuery(String url) {
        if (!isGoUrl(url)) {
            return "";
        }
        int queryStart = url.indexOf('?');
        if (queryStart < 0) {
            return "";
        }
        int fragmentStart = url.indexOf('#', queryStart);
        String query = fragmentStart < 0
                ? url.substring(queryStart + 1) : url.substring(queryStart + 1, fragmentStart);
        for (String pair : query.split("&")) {
            int eq = pair.indexOf('=');
            String key = eq < 0 ? pair : pair.substring(0, eq);
            if (key.equals("q")) {
                return decode(eq < 0 ? "" : pair.substring(eq + 1));
            }
        }
        return "";
    }

    private static String decode(String value) {
        try {
            return URLDecoder.decode(value, "UTF-8");
        } catch (IllegalArgumentException | UnsupportedEncodingException e) {
            return value.replace('+', ' '); // malformed escape: keep the raw text
        }
    }
}
