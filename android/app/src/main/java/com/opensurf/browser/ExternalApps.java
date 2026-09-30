package com.opensurf.browser;

import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.net.Uri;
import android.widget.Toast;

import com.opensurf.browser.core.NavigationPolicy;

import java.net.URISyntaxException;

/** Hands non-web links (intent:, mailto:, tel:, market:, ...) to other apps safely (SPEC I). */
final class ExternalApps {
    private static final int MAX_URL_CHARS_IN_DIALOG = 200;

    private ExternalApps() {
    }

    /**
     * Opens {@code url} in another app. Navigations without a user gesture (e.g. a script
     * redirect) need explicit confirmation so pages cannot spam app launches.
     */
    static void open(MainActivity activity, BrowserTab tab, String url, boolean userInitiated) {
        if (userInitiated) {
            launch(activity, tab, url);
            return;
        }
        String shown = url.length() > MAX_URL_CHARS_IN_DIALOG
                ? url.substring(0, MAX_URL_CHARS_IN_DIALOG) + "…" : url;
        AlertDialog.Builder builder = new AlertDialog.Builder(activity)
                .setTitle(R.string.external_title)
                .setMessage(activity.getString(R.string.external_message, shown))
                .setPositiveButton(R.string.external_open, (d, w) -> launch(activity, tab, url))
                .setNegativeButton(R.string.cancel, null);
        activity.showDialog(builder, null);
    }

    private static void launch(MainActivity activity, BrowserTab tab, String url) {
        Intent intent;
        String fallbackUrl = null;
        if ("intent".equals(NavigationPolicy.schemeOf(url))) {
            try {
                intent = Intent.parseUri(url, Intent.URI_INTENT_SCHEME);
            } catch (URISyntaxException | RuntimeException e) {
                Toast.makeText(activity, R.string.no_app_found, Toast.LENGTH_SHORT).show();
                return;
            }
            fallbackUrl = intent.getStringExtra("browser_fallback_url");
        } else {
            intent = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
        }
        sanitize(intent);
        try {
            activity.startActivity(intent);
            activity.closeTabIfBlankPopup(tab);
        } catch (ActivityNotFoundException | SecurityException e) {
            if (NavigationPolicy.isWebUrl(fallbackUrl) && !tab.destroyed) {
                tab.load(fallbackUrl);
            } else {
                Toast.makeText(activity, R.string.no_app_found, Toast.LENGTH_SHORT).show();
                activity.closeTabIfBlankPopup(tab);
            }
        }
    }

    /**
     * Restricts a page-supplied intent to what a web link may do: only BROWSABLE activities, no
     * explicit component or selector, and no URI permission grants.
     */
    static void sanitize(Intent intent) {
        intent.addCategory(Intent.CATEGORY_BROWSABLE);
        intent.setComponent(null);
        intent.setSelector(null);
        intent.setClipData(null);
        intent.setFlags(intent.getFlags() & ~(Intent.FLAG_GRANT_READ_URI_PERMISSION
                | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
                | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION));
    }
}
