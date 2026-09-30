package com.opensurf.browser;

import android.Manifest;
import android.app.DownloadManager;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.text.TextUtils;
import android.webkit.CookieManager;
import android.webkit.MimeTypeMap;
import android.webkit.URLUtil;
import android.widget.Toast;

import com.opensurf.browser.core.NavigationPolicy;

import java.util.Locale;

/** Saves responses the WebView cannot display to the public Downloads folder (SPEC F). */
final class Downloads {
    private Downloads() {
    }

    static void start(MainActivity activity, String url, String userAgent,
            String contentDisposition, String mimeType) {
        if (!NavigationPolicy.isWebUrl(url)) {
            // blob: and data: downloads would need page cooperation; not supported.
            Toast.makeText(activity, R.string.download_unsupported, Toast.LENGTH_SHORT).show();
            return;
        }
        if (Build.VERSION.SDK_INT <= Build.VERSION_CODES.P
                && !activity.hasPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE)) {
            // Keep the request until the user answers the permission prompt.
            activity.requestRuntimePermissions(
                    new String[] {Manifest.permission.WRITE_EXTERNAL_STORAGE}, () -> {
                        if (activity.hasPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE)) {
                            enqueue(activity, url, userAgent, contentDisposition, mimeType);
                        } else {
                            Toast.makeText(activity, R.string.download_permission_denied,
                                    Toast.LENGTH_LONG).show();
                        }
                    });
            return;
        }
        enqueue(activity, url, userAgent, contentDisposition, mimeType);
    }

    private static void enqueue(MainActivity activity, String url, String userAgent,
            String contentDisposition, String mimeType) {
        String fileName = URLUtil.guessFileName(url, contentDisposition, mimeType);
        try {
            DownloadManager.Request request = new DownloadManager.Request(Uri.parse(url));
            request.setMimeType(resolveMimeType(mimeType, fileName));
            String cookies = CookieManager.getInstance().getCookie(url);
            if (!TextUtils.isEmpty(cookies)) {
                request.addRequestHeader("Cookie", cookies);
            }
            if (!TextUtils.isEmpty(userAgent)) {
                request.addRequestHeader("User-Agent", userAgent);
            }
            request.setTitle(fileName);
            request.setDescription(Uri.parse(url).getHost());
            request.setNotificationVisibility(
                    DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED);
            request.setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, fileName);
            DownloadManager manager = activity.getSystemService(DownloadManager.class);
            if (manager == null) {
                throw new IllegalStateException("DownloadManager unavailable");
            }
            manager.enqueue(request);
            Toast.makeText(activity, activity.getString(R.string.download_started, fileName),
                    Toast.LENGTH_SHORT).show();
        } catch (RuntimeException e) {
            // IllegalArgumentException / IllegalStateException / SecurityException
            Toast.makeText(activity, R.string.download_failed, Toast.LENGTH_LONG).show();
        }
    }

    private static String resolveMimeType(String mimeType, String fileName) {
        if (!TextUtils.isEmpty(mimeType) && !"application/octet-stream".equals(mimeType)) {
            return mimeType;
        }
        int dot = fileName.lastIndexOf('.');
        if (dot >= 0) {
            String guessed = MimeTypeMap.getSingleton().getMimeTypeFromExtension(
                    fileName.substring(dot + 1).toLowerCase(Locale.ROOT));
            if (guessed != null) {
                return guessed;
            }
        }
        return TextUtils.isEmpty(mimeType) ? "application/octet-stream" : mimeType;
    }
}
