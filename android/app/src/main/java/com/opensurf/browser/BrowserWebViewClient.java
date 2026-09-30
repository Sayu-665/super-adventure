package com.opensurf.browser;

import android.graphics.Bitmap;
import android.net.http.SslError;
import android.webkit.HttpAuthHandler;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.SslErrorHandler;
import android.webkit.WebResourceRequest;
import android.webkit.WebView;
import android.webkit.WebViewClient;

import com.opensurf.browser.core.HomePage;
import com.opensurf.browser.core.NavigationPolicy;

/** Navigation policy, page lifecycle and TLS/auth handling for one tab. */
final class BrowserWebViewClient extends WebViewClient {
    private final MainActivity activity;
    private final BrowserTab tab;

    BrowserWebViewClient(MainActivity activity, BrowserTab tab) {
        this.activity = activity;
        this.tab = tab;
    }

    @Override
    public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
        String url = request.getUrl().toString();
        switch (NavigationPolicy.classify(url, request.isForMainFrame())) {
            case LOAD:
                return false;
            case GO:
                // The home page's search form: resolved natively with the omnibox function.
                activity.onHomeSearch(tab, HomePage.goQuery(url));
                return true;
            case INTENT:
            case EXTERNAL:
                ExternalApps.open(activity, tab, url, request.hasGesture() || request.isRedirect());
                return true;
            case BLOCK:
            default:
                return true;
        }
    }

    @Override
    public void onPageStarted(WebView view, String url, Bitmap favicon) {
        activity.onPageStarted(tab, url);
    }

    @Override
    public void onPageFinished(WebView view, String url) {
        activity.onPageFinished(tab, url, view.getTitle());
    }

    @Override
    public void doUpdateVisitedHistory(WebView view, String url, boolean isReload) {
        activity.onUrlChanged(tab, url);
    }

    @Override
    public void onReceivedSslError(WebView view, SslErrorHandler handler, SslError error) {
        if (tab.destroyed) {
            handler.cancel();
            return;
        }
        SecurityDialogs.showSslError(activity, tab, handler, error);
    }

    @Override
    public void onReceivedHttpAuthRequest(WebView view, HttpAuthHandler handler, String host,
            String realm) {
        if (tab.destroyed) {
            handler.cancel();
            return;
        }
        SecurityDialogs.showHttpAuth(activity, handler, host);
    }

    /** API 26+: a crashed or killed renderer must not take the whole app down. */
    @Override
    public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
        activity.onRendererGone(tab);
        return true;
    }
}
