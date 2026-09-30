package com.opensurf.browser;

import android.content.Context;
import android.content.SharedPreferences;

import com.opensurf.browser.core.SearchConfig;
import com.opensurf.browser.core.SearchEngines;

/** Persisted user settings (SPEC E). Stored only on the device. */
final class Prefs {
    private static final String FILE = "settings";
    private static final String KEY_ENGINE = "engine";
    private static final String KEY_CUSTOM_TEMPLATE = "custom_template";
    private static final String KEY_SAFE_SEARCH = "safe_search";
    private static final String KEY_JAVASCRIPT = "javascript";
    private static final String KEY_DESKTOP_SITE = "desktop_site";

    private final SharedPreferences preferences;

    Prefs(Context context) {
        preferences = context.getApplicationContext().getSharedPreferences(FILE, Context.MODE_PRIVATE);
    }

    SearchConfig searchConfig() {
        return new SearchConfig(engineId(), safeSearch(), customTemplate());
    }

    String engineId() {
        return preferences.getString(KEY_ENGINE, SearchEngines.DEFAULT_ID);
    }

    void setEngineId(String id) {
        preferences.edit().putString(KEY_ENGINE, id).apply();
    }

    String customTemplate() {
        return preferences.getString(KEY_CUSTOM_TEMPLATE, "");
    }

    void setCustomTemplate(String template) {
        preferences.edit().putString(KEY_CUSTOM_TEMPLATE, template.trim()).apply();
    }

    /** SafeSearch is OFF unless the user turns it on. */
    boolean safeSearch() {
        return preferences.getBoolean(KEY_SAFE_SEARCH, false);
    }

    void setSafeSearch(boolean enabled) {
        preferences.edit().putBoolean(KEY_SAFE_SEARCH, enabled).apply();
    }

    boolean javaScript() {
        return preferences.getBoolean(KEY_JAVASCRIPT, true);
    }

    void setJavaScript(boolean enabled) {
        preferences.edit().putBoolean(KEY_JAVASCRIPT, enabled).apply();
    }

    boolean desktopSite() {
        return preferences.getBoolean(KEY_DESKTOP_SITE, false);
    }

    void setDesktopSite(boolean enabled) {
        preferences.edit().putBoolean(KEY_DESKTOP_SITE, enabled).apply();
    }
}
