package com.opensurf.browser;

import android.annotation.SuppressLint;
import android.app.AlertDialog;
import android.net.Uri;
import android.net.http.SslError;
import android.view.LayoutInflater;
import android.view.View;
import android.webkit.HttpAuthHandler;
import android.webkit.SslErrorHandler;
import android.widget.EditText;
import android.widget.TextView;

/** Certificate-error interstitial and HTTP authentication prompt. */
final class SecurityDialogs {
    private SecurityDialogs() {
    }

    /**
     * Certificate errors are never accepted automatically: "Go back" (cancel) is the default and
     * dismissing the dialog also cancels. Only an explicit "Proceed anyway" continues.
     */
    static void showSslError(MainActivity activity, BrowserTab tab, SslErrorHandler handler,
            SslError error) {
        String url = error.getUrl();
        String host = url == null ? null : Uri.parse(url).getHost();
        String shownHost = host != null ? host : String.valueOf(url);
        // One prompt per tab and host: further errors (e.g. sub-resources) are cancelled.
        String promptKey = System.identityHashCode(tab) + "|" + shownHost;
        if (!activity.sslPromptsShowing().add(promptKey)) {
            handler.cancel();
            return;
        }
        boolean[] decided = {false};
        AlertDialog.Builder builder = new AlertDialog.Builder(activity)
                .setTitle(R.string.ssl_title)
                .setMessage(activity.getString(R.string.ssl_message, shownHost,
                        activity.getString(reason(error))))
                .setPositiveButton(R.string.ssl_go_back, (d, w) -> {
                    decided[0] = true;
                    handler.cancel();
                })
                .setNegativeButton(R.string.ssl_proceed, (d, w) -> {
                    decided[0] = true;
                    activity.onCertificateErrorBypassed(tab, host);
                    handler.proceed();
                });
        activity.showDialog(builder, d -> {
            activity.sslPromptsShowing().remove(promptKey);
            if (!decided[0]) {
                decided[0] = true;
                handler.cancel();
            }
        });
    }

    private static int reason(SslError error) {
        if (error.hasError(SslError.SSL_UNTRUSTED)) {
            return R.string.ssl_untrusted;
        }
        if (error.hasError(SslError.SSL_IDMISMATCH)) {
            return R.string.ssl_mismatch;
        }
        if (error.hasError(SslError.SSL_EXPIRED)) {
            return R.string.ssl_expired;
        }
        if (error.hasError(SslError.SSL_NOTYETVALID)) {
            return R.string.ssl_not_yet_valid;
        }
        if (error.hasError(SslError.SSL_DATE_INVALID)) {
            return R.string.ssl_date_invalid;
        }
        return R.string.ssl_invalid;
    }

    /** HTTP Basic/Digest authentication. Credentials are not stored. */
    @SuppressLint("InflateParams") // dialog content has no parent
    static void showHttpAuth(MainActivity activity, HttpAuthHandler handler, String host) {
        View view = LayoutInflater.from(activity).inflate(R.layout.dialog_http_auth, null);
        TextView message = view.findViewById(R.id.auth_message);
        EditText username = view.findViewById(R.id.auth_username);
        EditText password = view.findViewById(R.id.auth_password);
        message.setText(activity.getString(R.string.auth_message, host));
        boolean[] decided = {false};
        AlertDialog.Builder builder = new AlertDialog.Builder(activity)
                .setTitle(R.string.auth_title)
                .setView(view)
                .setPositiveButton(R.string.auth_sign_in, (d, w) -> {
                    decided[0] = true;
                    handler.proceed(username.getText().toString(), password.getText().toString());
                })
                .setNegativeButton(R.string.cancel, (d, w) -> {
                    decided[0] = true;
                    handler.cancel();
                });
        activity.showDialog(builder, d -> {
            if (!decided[0]) {
                decided[0] = true;
                handler.cancel();
            }
        });
    }
}
