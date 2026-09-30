package com.opensurf.browser;

import android.content.Context;
import android.webkit.CookieManager;
import android.webkit.GeolocationPermissions;
import android.webkit.WebStorage;
import android.webkit.WebView;
import android.webkit.WebViewDatabase;

import java.util.List;

/** "Clear browsing data": cookies, cache, history, web storage and saved site credentials. */
final class BrowsingData {
    private BrowsingData() {
    }

    /** Clears the data shared by every WebView in the app. */
    @SuppressWarnings("deprecation")
    static void clearShared(Context context) {
        CookieManager cookies = CookieManager.getInstance();
        cookies.removeAllCookies(null);
        cookies.flush();
        WebStorage.getInstance().deleteAllData();
        GeolocationPermissions.getInstance().clearAll();
        WebViewDatabase database = WebViewDatabase.getInstance(context);
        database.clearHttpAuthUsernamePassword();
        database.clearFormData();
    }

    /** Clears the HTTP cache (app-wide) and each tab's history, form data and SSL decisions. */
    static void clearTabs(List<BrowserTab> tabs) {
        for (BrowserTab tab : tabs) {
            WebView webView = tab.webView;
            webView.clearCache(true);
            webView.clearHistory();
            webView.clearFormData();
            webView.clearSslPreferences();
            tab.certErrorHost = null;
        }
    }
}
