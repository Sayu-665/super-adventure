package com.opensurf.browser;

import android.net.Uri;
import android.webkit.WebView;

import com.opensurf.browser.core.HomePage;

/** One browser tab: a WebView plus the page state shown in the browser chrome. */
final class BrowserTab {
    final WebView webView;
    /** The tab that opened this one (target=_blank / window.open), or null. */
    BrowserTab opener;
    String url = "";
    String title = "";
    int progress = 100;
    /** Host whose certificate error the user explicitly chose to bypass in this tab. */
    String certErrorHost;
    /** Last home-page caption refresh attempted (guards against refresh loops). */
    String homeRefreshAttempt;
    boolean destroyed;

    BrowserTab(WebView webView, BrowserTab opener) {
        this.webView = webView;
        this.opener = opener;
    }

    boolean isHome() {
        return HomePage.isHomeUrl(url);
    }

    boolean isLoading() {
        return progress < 100;
    }

    void load(String target) {
        url = target;
        title = "";
        webView.loadUrl(target);
    }

    boolean isHttps() {
        return url.regionMatches(true, 0, "https://", 0, 8);
    }

    boolean hasCertificateError() {
        String host = Uri.parse(url).getHost();
        return certErrorHost != null && certErrorHost.equalsIgnoreCase(host);
    }
}
