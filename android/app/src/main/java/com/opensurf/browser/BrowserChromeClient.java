package com.opensurf.browser;

import android.net.Uri;
import android.os.Message;
import android.view.View;
import android.webkit.GeolocationPermissions;
import android.webkit.PermissionRequest;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebView;

/** Progress, titles, new windows, fullscreen video, uploads and site permissions for a tab. */
final class BrowserChromeClient extends WebChromeClient {
    private final MainActivity activity;
    private final BrowserTab tab;

    BrowserChromeClient(MainActivity activity, BrowserTab tab) {
        this.activity = activity;
        this.tab = tab;
    }

    @Override
    public void onProgressChanged(WebView view, int newProgress) {
        activity.onProgressChanged(tab, newProgress);
    }

    @Override
    public void onReceivedTitle(WebView view, String title) {
        activity.onTitleChanged(tab, title);
    }

    /** target=_blank links and window.open() (with a user gesture) open a new tab. */
    @Override
    public boolean onCreateWindow(WebView view, boolean isDialog, boolean isUserGesture,
            Message resultMsg) {
        if (!isUserGesture || tab.destroyed) {
            return false; // pop-up without a user gesture: blocked
        }
        WebView.WebViewTransport transport = (WebView.WebViewTransport) resultMsg.obj;
        transport.setWebView(activity.openPopupTab(tab));
        resultMsg.sendToTarget();
        return true;
    }

    @Override
    public void onCloseWindow(WebView window) {
        activity.closeTabLater(tab);
    }

    @Override
    public void onShowCustomView(View view, CustomViewCallback callback) {
        activity.enterFullscreen(view, callback);
    }

    @Override
    public void onHideCustomView() {
        activity.exitFullscreen();
    }

    @Override
    public boolean onShowFileChooser(WebView webView, ValueCallback<Uri[]> filePathCallback,
            FileChooserParams fileChooserParams) {
        return activity.showFileChooser(filePathCallback, fileChooserParams);
    }

    @Override
    public void onPermissionRequest(PermissionRequest request) {
        activity.sitePermissions().onPermissionRequest(request);
    }

    @Override
    public void onPermissionRequestCanceled(PermissionRequest request) {
        activity.sitePermissions().onPermissionRequestCanceled(request);
    }

    @Override
    public void onGeolocationPermissionsShowPrompt(String origin,
            GeolocationPermissions.Callback callback) {
        activity.sitePermissions().onGeolocationPrompt(origin, callback);
    }

    @Override
    public void onGeolocationPermissionsHidePrompt() {
        activity.sitePermissions().onGeolocationPromptHidden();
    }
}
