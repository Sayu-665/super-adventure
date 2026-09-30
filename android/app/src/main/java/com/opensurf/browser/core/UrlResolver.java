package com.opensurf.browser.core;

import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Turns omnibox text into the URL to load (SPEC A). This is the single source of truth used by
 * the address bar, the home page (via opensurf://go) and incoming search/share intents.
 */
public final class UrlResolver {
    /** Host (plain or bracketed IPv6), optional :port, optional /path, ?query or #fragment. */
    private static final Pattern HOST_LIKE = Pattern.compile(
            "^(\\[[0-9A-Fa-f:.]+\\]|[^/?#:\\[\\]@]+)(?::([0-9]{1,5}))?([/?#].*)?$");
    private static final Pattern IPV4 = Pattern.compile(
            "^([0-9]{1,3})\\.([0-9]{1,3})\\.([0-9]{1,3})\\.([0-9]{1,3})$");
    private static final Pattern DOMAIN_LABEL = Pattern.compile(
            "^[\\p{L}\\p{N}](?:[\\p{L}\\p{N}-]*[\\p{L}\\p{N}])?$");
    private static final Pattern TOP_LEVEL_LABEL = Pattern.compile("^\\p{L}{2,}$");
    private static final Pattern WEB_URL_IN_TEXT = Pattern.compile(
            "https?://[^\\s<>\"]+", Pattern.CASE_INSENSITIVE);

    private UrlResolver() {
    }

    /**
     * Resolves omnibox input.
     *
     * @return the URL to load, or {@code null} when the input is empty (do nothing)
     */
    public static String resolve(String input, SearchConfig config) {
        if (input == null) {
            return null;
        }
        String text = input.trim();
        if (text.isEmpty()) {
            return null;
        }
        if (hasWebScheme(text)) {
            return text;
        }
        if (!containsWhitespace(text)) {
            Matcher m = HOST_LIKE.matcher(text);
            if (m.matches() && isValidPort(m.group(2))) {
                String host = m.group(1);
                if (isLocalHostOrIp(host)) {
                    return "http://" + text;
                }
                if (isDomain(host)) {
                    return "https://" + text;
                }
            }
        }
        // Everything else, including "javascript:", "data:", "file:" or "ftp:" input, is a search.
        return config.searchUrl(text);
    }

    /**
     * Resolves text shared from another app: the first http(s) URL inside it wins, otherwise the
     * whole text is resolved like omnibox input.
     */
    public static String resolveSharedText(CharSequence shared, SearchConfig config) {
        if (shared == null) {
            return null;
        }
        String text = shared.toString();
        Matcher m = WEB_URL_IN_TEXT.matcher(text);
        if (m.find()) {
            String url = trimTrailingPunctuation(m.group());
            if (url.indexOf("://") + 3 < url.length()) {
                return url;
            }
        }
        return resolve(text, config);
    }

    /** True for text starting with "http://" or "https://" (any case). */
    public static boolean hasWebScheme(String text) {
        return text != null && (SearchEngines.startsWithIgnoreCase(text, "http://")
                || SearchEngines.startsWithIgnoreCase(text, "https://"));
    }

    private static boolean containsWhitespace(String text) {
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (Character.isWhitespace(c) || Character.isSpaceChar(c)) {
                return true;
            }
        }
        return false;
    }

    private static boolean isValidPort(String port) {
        return port == null || Integer.parseInt(port) <= 65535;
    }

    private static boolean isLocalHostOrIp(String host) {
        if (host.toLowerCase(Locale.ROOT).equals("localhost")) {
            return true;
        }
        if (host.startsWith("[")) {
            return host.indexOf(':') > 0; // bracketed IPv6 literal such as [::1]
        }
        Matcher m = IPV4.matcher(host);
        if (!m.matches()) {
            return false;
        }
        for (int i = 1; i <= 4; i++) {
            if (Integer.parseInt(m.group(i)) > 255) {
                return false;
            }
        }
        return true;
    }

    /** At least two dot-separated labels, the last one made of two or more letters. */
    private static boolean isDomain(String host) {
        String[] labels = host.split("\\.", -1);
        if (labels.length < 2) {
            return false;
        }
        for (String label : labels) {
            if (label.length() > 63 || !DOMAIN_LABEL.matcher(label).matches()) {
                return false;
            }
        }
        return TOP_LEVEL_LABEL.matcher(labels[labels.length - 1]).matches();
    }

    private static String trimTrailingPunctuation(String url) {
        int end = url.length();
        while (end > 0 && ".,;:!?)]}'".indexOf(url.charAt(end - 1)) >= 0) {
            end--;
        }
        return url.substring(0, end);
    }
}
