package com.opensurf.browser;

import android.annotation.SuppressLint;
import android.app.Activity;
import android.app.AlertDialog;
import android.app.Dialog;
import android.app.SearchManager;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.DialogInterface;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.text.Editable;
import android.text.TextUtils;
import android.text.TextWatcher;
import android.util.SparseArray;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.WindowManager;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.webkit.CookieManager;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.ImageButton;
import android.widget.ImageView;
import android.widget.PopupMenu;
import android.widget.ProgressBar;
import android.widget.TextView;
import android.widget.Toast;
import android.window.OnBackInvokedDispatcher;

import com.opensurf.browser.core.HomePage;
import com.opensurf.browser.core.NavigationPolicy;
import com.opensurf.browser.core.SearchConfig;
import com.opensurf.browser.core.UrlResolver;
import com.opensurf.browser.core.UserAgents;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/**
 * The browser window: a list of tabs (one WebView each), the omnibox, the bottom bar and the
 * overflow menu. Web content never gets a JavaScript bridge into the app.
 */
public class MainActivity extends Activity {
    private static final int REQUEST_FILE_CHOOSER = 1;
    private static final int REQUEST_SETTINGS = 2;
    private static final int FIRST_PERMISSION_REQUEST = 100;
    private static final int LAST_PERMISSION_REQUEST = 60000;
    private static final String STATE_TAB_URLS = "opensurf:tab_urls";
    private static final String STATE_CURRENT_TAB = "opensurf:current_tab";
    private static final float DISABLED_ALPHA = 0.38f;

    /** Called when a runtime permission request finishes; re-check with {@link #hasPermission}. */
    interface PermissionResult {
        void onResult();
    }

    private final List<BrowserTab> tabs = new ArrayList<>();
    private BrowserTab currentTab;
    private final List<Dialog> openDialogs = new ArrayList<>();
    private final Set<String> sslPromptsShowing = new HashSet<>();
    private final SparseArray<PermissionResult> permissionCallbacks = new SparseArray<>();
    private int nextPermissionRequest = FIRST_PERMISSION_REQUEST;

    private Prefs prefs;
    private SitePermissions sitePermissions;
    private String desktopUserAgent;
    private boolean appliedJavaScript;
    private boolean appliedDesktopSite;
    private String appliedHomeUrl;

    private View root;
    private EditText omnibox;
    private TextView pageTitle;
    private ImageView securityIcon;
    private ImageButton clearButton;
    private ImageButton reloadButton;
    private ProgressBar progressBar;
    private FrameLayout webContainer;
    private View findBar;
    private EditText findInput;
    private TextView findCount;
    private ImageButton backButton;
    private ImageButton forwardButton;
    private View tabsButton;
    private TextView tabCount;

    private ValueCallback<Uri[]> fileChooserCallback;
    private View customView;
    private WebChromeClient.CustomViewCallback customViewCallback;
    private FrameLayout fullscreenContainer;

    // ---------------------------------------------------------------------------------------
    // Lifecycle
    // ---------------------------------------------------------------------------------------

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
        prefs = new Prefs(this);
        sitePermissions = new SitePermissions(this);
        bindViews();
        SystemBars.enableEdgeToEdge(getWindow());
        SystemBars.applyInsetsAsPadding(root);
        registerBackCallback();

        try {
            WebView.setWebContentsDebuggingEnabled(
                    (getApplicationInfo().flags & ApplicationInfo.FLAG_DEBUGGABLE) != 0);
            CookieManager.getInstance().setAcceptCookie(true);
            desktopUserAgent = UserAgents.desktopFrom(WebSettings.getDefaultUserAgent(this));
        } catch (RuntimeException e) {
            // The WebView provider is missing, disabled or being updated.
            Toast.makeText(this, R.string.webview_missing, Toast.LENGTH_LONG).show();
            finish();
            return;
        }
        rememberAppliedSettings();

        if (savedInstanceState != null) {
            restoreTabs(savedInstanceState);
        } else if (!openFromIntent(getIntent())) {
            openTab(null, null);
        }
        if (tabs.isEmpty()) {
            openTab(null, null);
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        if (tabs.isEmpty()) {
            return; // onCreate failed (no WebView)
        }
        exitFullscreen();
        openFromIntent(intent);
    }

    @Override
    protected void onStart() {
        super.onStart();
        if (currentTab != null) {
            currentTab.webView.onResume();
            currentTab.webView.resumeTimers();
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (currentTab != null) {
            applySettingsIfChanged();
            updateUi();
        }
    }

    @Override
    protected void onStop() {
        super.onStop();
        if (currentTab != null) {
            currentTab.webView.onPause();
            currentTab.webView.pauseTimers();
        }
        CookieManager.getInstance().flush();
    }

    @Override
    protected void onSaveInstanceState(Bundle outState) {
        super.onSaveInstanceState(outState);
        // Only URLs are saved (full WebView state can exceed the Binder transaction limit).
        ArrayList<String> urls = new ArrayList<>();
        for (BrowserTab tab : tabs) {
            String url = tab.webView.getUrl();
            if (!NavigationPolicy.isWebUrl(url)) {
                url = tab.url;
            }
            urls.add(NavigationPolicy.isWebUrl(url) ? url : "");
        }
        outState.putStringArrayList(STATE_TAB_URLS, urls);
        outState.putInt(STATE_CURRENT_TAB, tabs.indexOf(currentTab));
    }

    private void restoreTabs(Bundle state) {
        ArrayList<String> urls = state.getStringArrayList(STATE_TAB_URLS);
        if (urls == null) {
            return;
        }
        for (String url : urls) {
            BrowserTab tab = createTab(null);
            tabs.add(tab);
            tab.load(NavigationPolicy.isWebUrl(url) ? url : homeUrl());
        }
        if (!tabs.isEmpty()) {
            int index = state.getInt(STATE_CURRENT_TAB, 0);
            switchTo(tabs.get(Math.max(0, Math.min(index, tabs.size() - 1))));
        }
    }

    @Override
    protected void onDestroy() {
        for (Dialog dialog : new ArrayList<>(openDialogs)) {
            dialog.dismiss(); // resolves pending SSL/permission prompts as "cancel"
        }
        openDialogs.clear();
        if (fileChooserCallback != null) {
            fileChooserCallback.onReceiveValue(null);
            fileChooserCallback = null;
        }
        exitFullscreen();
        for (BrowserTab tab : new ArrayList<>(tabs)) {
            destroyTab(tab);
        }
        tabs.clear();
        currentTab = null;
        super.onDestroy();
    }

    // ---------------------------------------------------------------------------------------
    // Views
    // ---------------------------------------------------------------------------------------

    private void bindViews() {
        root = findViewById(R.id.root);
        omnibox = findViewById(R.id.omnibox);
        pageTitle = findViewById(R.id.page_title);
        securityIcon = findViewById(R.id.security_icon);
        clearButton = findViewById(R.id.clear_button);
        reloadButton = findViewById(R.id.reload_button);
        progressBar = findViewById(R.id.progress);
        webContainer = findViewById(R.id.web_container);
        findBar = findViewById(R.id.find_bar);
        findInput = findViewById(R.id.find_input);
        findCount = findViewById(R.id.find_count);
        backButton = findViewById(R.id.back_button);
        forwardButton = findViewById(R.id.forward_button);
        tabsButton = findViewById(R.id.tabs_button);
        tabCount = findViewById(R.id.tab_count);

        omnibox.setOnEditorActionListener((v, actionId, event) -> {
            boolean enterKey = event != null && event.getKeyCode() == KeyEvent.KEYCODE_ENTER;
            if (actionId == EditorInfo.IME_ACTION_GO || enterKey) {
                if (!enterKey || event.getAction() == KeyEvent.ACTION_DOWN) {
                    submitOmnibox();
                }
                return true;
            }
            return false;
        });
        omnibox.setOnFocusChangeListener((v, hasFocus) -> {
            if (hasFocus) {
                String text = currentTab == null || currentTab.isHome() ? "" : currentTab.url;
                if (!omnibox.getText().toString().equals(text)) {
                    omnibox.setText(text);
                }
                omnibox.selectAll();
            } else {
                hideKeyboard(omnibox);
            }
            updateUi();
        });
        omnibox.addTextChangedListener(new SimpleTextWatcher() {
            @Override
            public void afterTextChanged(Editable s) {
                updateClearButton();
            }
        });
        clearButton.setOnClickListener(v -> {
            omnibox.setText("");
            showKeyboard(omnibox);
        });
        reloadButton.setOnClickListener(v -> reloadOrStop());

        backButton.setOnClickListener(v -> goBackInTab());
        forwardButton.setOnClickListener(v -> {
            if (currentTab != null && currentTab.webView.canGoForward()) {
                currentTab.webView.goForward();
            }
        });
        findViewById(R.id.home_button).setOnClickListener(v -> {
            dismissOmnibox();
            if (currentTab != null) {
                currentTab.load(homeUrl());
                updateUi();
            }
        });
        tabsButton.setOnClickListener(v -> {
            dismissOmnibox();
            TabSwitcher.show(this);
        });
        findViewById(R.id.menu_button).setOnClickListener(this::showMenu);

        findInput.addTextChangedListener(new SimpleTextWatcher() {
            @Override
            public void afterTextChanged(Editable s) {
                if (currentTab == null || findBar.getVisibility() != View.VISIBLE) {
                    return;
                }
                if (s.length() == 0) {
                    currentTab.webView.clearMatches();
                    findCount.setText("");
                } else {
                    currentTab.webView.findAllAsync(s.toString());
                }
            }
        });
        findInput.setOnEditorActionListener((v, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_SEARCH
                    || (event != null && event.getKeyCode() == KeyEvent.KEYCODE_ENTER)) {
                if (currentTab != null && (event == null || event.getAction() == KeyEvent.ACTION_DOWN)) {
                    currentTab.webView.findNext(true);
                }
                return true;
            }
            return false;
        });
        findViewById(R.id.find_previous).setOnClickListener(v -> {
            if (currentTab != null) {
                currentTab.webView.findNext(false);
            }
        });
        findViewById(R.id.find_next).setOnClickListener(v -> {
            if (currentTab != null) {
                currentTab.webView.findNext(true);
            }
        });
        findViewById(R.id.find_close).setOnClickListener(v -> closeFindBar());
    }

    /** Refreshes the browser chrome from the current tab. */
    private void updateUi() {
        BrowserTab tab = currentTab;
        if (tab == null) {
            return;
        }
        boolean editing = omnibox.hasFocus();
        boolean home = tab.isHome();
        if (!editing) {
            // The home page shows an empty omnibox with the hint, never its internal URL.
            String text = home ? "" : tab.url;
            if (!omnibox.getText().toString().equals(text)) {
                omnibox.setText(text);
                omnibox.setSelection(0);
            }
        }

        boolean showTitle = !editing && !home && !tab.title.isEmpty() && !tab.title.equals(tab.url);
        pageTitle.setText(showTitle ? tab.title : "");
        pageTitle.setVisibility(showTitle ? View.VISIBLE : View.GONE);

        if (editing || home || !tab.isHttps()) {
            securityIcon.setVisibility(View.GONE);
        } else {
            boolean certError = tab.hasCertificateError();
            securityIcon.setImageResource(certError ? R.drawable.ic_warning : R.drawable.ic_lock);
            securityIcon.setContentDescription(
                    getString(certError ? R.string.cd_not_secure : R.string.cd_secure));
            securityIcon.setVisibility(View.VISIBLE);
        }

        boolean loading = tab.isLoading();
        progressBar.setProgress(tab.progress);
        progressBar.setVisibility(loading ? View.VISIBLE : View.INVISIBLE);
        reloadButton.setImageResource(loading ? R.drawable.ic_close : R.drawable.ic_refresh);
        reloadButton.setContentDescription(getString(loading ? R.string.cd_stop : R.string.cd_reload));

        setButtonEnabled(backButton, tab.webView.canGoBack() || isOpen(tab.opener));
        setButtonEnabled(forwardButton, tab.webView.canGoForward());

        int count = tabs.size();
        tabCount.setText(String.valueOf(Math.min(count, 99)));
        tabsButton.setContentDescription(getString(R.string.tabs_title, count));
        updateClearButton();
    }

    private void updateClearButton() {
        boolean show = omnibox.hasFocus() && omnibox.length() > 0;
        clearButton.setVisibility(show ? View.VISIBLE : View.GONE);
    }

    private static void setButtonEnabled(View button, boolean enabled) {
        button.setEnabled(enabled);
        button.setAlpha(enabled ? 1f : DISABLED_ALPHA);
    }

    // ---------------------------------------------------------------------------------------
    // Omnibox
    // ---------------------------------------------------------------------------------------

    private void submitOmnibox() {
        String target = UrlResolver.resolve(omnibox.getText().toString(), searchConfig());
        if (target == null || currentTab == null) {
            return; // empty input: do nothing
        }
        currentTab.load(target);
        dismissOmnibox();
        updateUi();
    }

    private void dismissOmnibox() {
        if (omnibox.hasFocus()) {
            hideKeyboard(omnibox);
            root.requestFocus();
        }
    }

    private void focusOmnibox() {
        omnibox.requestFocus();
        showKeyboard(omnibox);
    }

    private void showKeyboard(View view) {
        view.post(() -> {
            InputMethodManager imm = getSystemService(InputMethodManager.class);
            if (imm != null) {
                imm.showSoftInput(view, InputMethodManager.SHOW_IMPLICIT);
            }
        });
    }

    private void hideKeyboard(View view) {
        InputMethodManager imm = getSystemService(InputMethodManager.class);
        if (imm != null) {
            imm.hideSoftInputFromWindow(view.getWindowToken(), 0);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Tabs
    // ---------------------------------------------------------------------------------------

    List<BrowserTab> tabs() {
        return tabs;
    }

    BrowserTab currentTab() {
        return currentTab;
    }

    private boolean isOpen(BrowserTab tab) {
        return tab != null && !tab.destroyed && tabs.contains(tab);
    }

    /** Creates a fully configured tab that is not yet in the tab list and has loaded nothing. */
    @SuppressLint("SetJavaScriptEnabled") // JavaScript is a user setting (default on)
    @SuppressWarnings("deprecation")
    private BrowserTab createTab(BrowserTab opener) {
        WebView webView = new WebView(this);
        BrowserTab tab = new BrowserTab(webView, opener);

        WebSettings settings = webView.getSettings();
        settings.setDomStorageEnabled(true);
        settings.setLoadWithOverviewMode(true);
        settings.setUseWideViewPort(true);
        settings.setSupportZoom(true);
        settings.setBuiltInZoomControls(true);
        settings.setDisplayZoomControls(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE);
        // No access to local files or content providers (assets stay reachable for the home page).
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setAllowFileAccessFromFileURLs(false);
        settings.setAllowUniversalAccessFromFileURLs(false);
        settings.setGeolocationEnabled(true); // still gated by a per-site prompt
        settings.setSupportMultipleWindows(true);
        settings.setJavaScriptCanOpenWindowsAutomatically(false);
        applyUserSettings(settings, prefs.javaScript(), prefs.desktopSite());
        CookieManager.getInstance().setAcceptThirdPartyCookies(webView, true);

        webView.setWebViewClient(new BrowserWebViewClient(this, tab));
        webView.setWebChromeClient(new BrowserChromeClient(this, tab));
        webView.setDownloadListener((url, userAgent, contentDisposition, mimeType, length) -> {
            Downloads.start(this, url, userAgent, contentDisposition, mimeType);
            closeTabIfBlankPopup(tab);
        });
        webView.setFindListener((activeMatch, matches, doneCounting) -> {
            if (tab == currentTab && findBar.getVisibility() == View.VISIBLE) {
                findCount.setText(matches == 0 && findInput.length() == 0 ? ""
                        : getString(R.string.find_count, matches == 0 ? 0 : activeMatch + 1, matches));
            }
        });
        return tab;
    }

    @SuppressLint("SetJavaScriptEnabled")
    private void applyUserSettings(WebSettings settings, boolean javaScript, boolean desktopSite) {
        settings.setJavaScriptEnabled(javaScript);
        settings.setUserAgentString(desktopSite ? desktopUserAgent : null); // null = default UA
    }

    /** Opens a foreground tab loading {@code url}, or the home page when it is null. */
    BrowserTab openTab(String url, BrowserTab opener) {
        BrowserTab tab = createTab(opener);
        tabs.add(tab);
        tab.load(url != null ? url : homeUrl());
        switchTo(tab);
        return tab;
    }

    void openNewTabFromUi() {
        openTab(null, null);
        focusOmnibox();
    }

    /** New tab for target=_blank / window.open; the WebView navigates it itself. */
    WebView openPopupTab(BrowserTab opener) {
        BrowserTab tab = createTab(opener);
        tabs.add(tab);
        switchTo(tab);
        return tab.webView;
    }

    void switchTo(BrowserTab tab) {
        if (tab == currentTab || !isOpen(tab)) {
            updateUi();
            return;
        }
        dismissOmnibox();
        closeFindBar();
        exitFullscreen();
        if (currentTab != null) {
            webContainer.removeView(currentTab.webView);
            currentTab.webView.onPause();
        }
        currentTab = tab;
        detachFromParent(tab.webView);
        webContainer.addView(tab.webView, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        tab.webView.onResume();
        updateUi();
    }

    /** Closes a tab; closing the last one leaves a fresh home tab so there is always one. */
    void closeTab(BrowserTab tab) {
        int index = tabs.indexOf(tab);
        if (index < 0) {
            return;
        }
        tabs.remove(index);
        for (BrowserTab other : tabs) {
            if (other.opener == tab) {
                other.opener = null;
            }
        }
        if (tab == currentTab) {
            closeFindBar();
            exitFullscreen();
            webContainer.removeView(tab.webView);
            currentTab = null;
            BrowserTab next;
            if (isOpen(tab.opener)) {
                next = tab.opener;
            } else if (!tabs.isEmpty()) {
                next = tabs.get(Math.max(0, index - 1));
            } else {
                next = createTab(null);
                tabs.add(next);
                next.load(homeUrl());
            }
            switchTo(next);
        }
        destroyTab(tab);
        updateUi();
    }

    /** Closes a tab after the current WebView callback returns (never destroy mid-callback). */
    void closeTabLater(BrowserTab tab) {
        root.post(() -> closeTab(tab));
    }

    /** A pop-up tab that only led to a download or another app is closed again. */
    void closeTabIfBlankPopup(BrowserTab tab) {
        root.post(() -> {
            if (isOpen(tab) && tab.opener != null
                    && tab.webView.copyBackForwardList().getSize() == 0) {
                closeTab(tab);
            }
        });
    }

    private void destroyTab(BrowserTab tab) {
        if (tab.destroyed) {
            return;
        }
        tab.destroyed = true;
        tab.opener = null;
        detachFromParent(tab.webView);
        tab.webView.stopLoading();
        tab.webView.destroy();
    }

    private static void detachFromParent(View view) {
        ViewParent parent = view.getParent();
        if (parent instanceof ViewGroup) {
            ((ViewGroup) parent).removeView(view);
        }
    }

    /** The renderer crashed or was killed: replace the tab's WebView with a fresh home page. */
    void onRendererGone(BrowserTab crashed) {
        int index = tabs.indexOf(crashed);
        if (index < 0) {
            destroyTab(crashed);
            return;
        }
        boolean wasCurrent = crashed == currentTab;
        if (wasCurrent) {
            closeFindBar();
            exitFullscreen();
            webContainer.removeView(crashed.webView);
            currentTab = null;
        }
        BrowserTab replacement = createTab(isOpen(crashed.opener) ? crashed.opener : null);
        tabs.set(index, replacement);
        for (BrowserTab other : tabs) {
            if (other.opener == crashed) {
                other.opener = replacement;
            }
        }
        destroyTab(crashed);
        replacement.load(homeUrl());
        if (wasCurrent) {
            switchTo(replacement);
            Toast.makeText(this, R.string.page_crashed, Toast.LENGTH_SHORT).show();
        }
        updateUi();
    }

    // ---------------------------------------------------------------------------------------
    // Page events (from the WebView clients)
    // ---------------------------------------------------------------------------------------

    void onPageStarted(BrowserTab tab, String url) {
        if (tab.destroyed) {
            return;
        }
        tab.url = url;
        tab.title = "";
        if (tab.progress >= 100) {
            tab.progress = 5;
        }
        if (tab == currentTab) {
            closeFindBar();
            updateUi();
        }
    }

    void onPageFinished(BrowserTab tab, String url, String title) {
        if (tab.destroyed) {
            return;
        }
        tab.url = url;
        tab.progress = 100;
        if (tab.title.isEmpty() && title != null) {
            tab.title = title;
        }
        refreshHomeCaption(tab);
        if (tab == currentTab) {
            updateUi();
        }
    }

    void onUrlChanged(BrowserTab tab, String url) {
        if (tab.destroyed || url == null) {
            return;
        }
        tab.url = url;
        refreshHomeCaption(tab);
        if (tab == currentTab) {
            updateUi();
        }
    }

    void onProgressChanged(BrowserTab tab, int progress) {
        if (tab.destroyed) {
            return;
        }
        tab.progress = progress;
        if (tab == currentTab) {
            updateUi();
        }
    }

    void onTitleChanged(BrowserTab tab, String title) {
        if (tab.destroyed) {
            return;
        }
        tab.title = title == null ? "" : title;
        if (tab == currentTab) {
            updateUi();
        }
    }

    /** opensurf://go?q=... from the home page, resolved with the omnibox function. */
    void onHomeSearch(BrowserTab tab, String query) {
        String target = UrlResolver.resolve(query, searchConfig());
        if (target != null && !tab.destroyed) {
            tab.load(target);
            if (tab == currentTab) {
                updateUi();
            }
        }
    }

    void onCertificateErrorBypassed(BrowserTab tab, String host) {
        tab.certErrorHost = host;
        if (tab == currentTab) {
            updateUi();
        }
    }

    // ---------------------------------------------------------------------------------------
    // Settings and the home page
    // ---------------------------------------------------------------------------------------

    private SearchConfig searchConfig() {
        return prefs.searchConfig();
    }

    private String homeUrl() {
        return HomePage.url(searchConfig());
    }

    private void rememberAppliedSettings() {
        appliedJavaScript = prefs.javaScript();
        appliedDesktopSite = prefs.desktopSite();
        appliedHomeUrl = homeUrl();
    }

    /** Applies settings changed in {@link SettingsActivity} or the menu to every tab. */
    private void applySettingsIfChanged() {
        boolean javaScript = prefs.javaScript();
        boolean desktopSite = prefs.desktopSite();
        boolean webSettingsChanged =
                javaScript != appliedJavaScript || desktopSite != appliedDesktopSite;
        if (webSettingsChanged) {
            for (BrowserTab tab : tabs) {
                applyUserSettings(tab.webView.getSettings(), javaScript, desktopSite);
            }
        }
        if (!homeUrl().equals(appliedHomeUrl)) {
            for (BrowserTab tab : tabs) {
                refreshHomeCaption(tab);
            }
        }
        rememberAppliedSettings();
        if (webSettingsChanged && currentTab != null) {
            currentTab.webView.reload();
        }
    }

    /**
     * Keeps the home page caption in sync with the settings by replacing its fragment (no new
     * history entry). This is one-way: the page itself has no way to call into the app.
     */
    private void refreshHomeCaption(BrowserTab tab) {
        if (!tab.isHome() || !prefs.javaScript()) {
            return;
        }
        String expected = homeUrl();
        String attempt = tab.url + "\n" + expected;
        if (!expected.equals(tab.url) && !attempt.equals(tab.homeRefreshAttempt)) {
            tab.homeRefreshAttempt = attempt; // at most one attempt per (current, expected) pair
            // The fragment is built from percent-encoded values, so it is a safe JS literal.
            String fragment = "#" + HomePage.fragment(searchConfig());
            tab.webView.evaluateJavascript("location.replace('" + fragment + "')", null);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Menu
    // ---------------------------------------------------------------------------------------

    private void showMenu(View anchor) {
        dismissOmnibox();
        PopupMenu popup = new PopupMenu(this, anchor);
        popup.inflate(R.menu.browser_menu);
        Menu menu = popup.getMenu();
        BrowserTab tab = currentTab;
        boolean webPage = tab != null && NavigationPolicy.isWebUrl(tab.url);
        menu.findItem(R.id.action_reload).setTitle(
                tab != null && tab.isLoading() ? R.string.menu_stop : R.string.menu_reload);
        menu.findItem(R.id.action_share).setEnabled(webPage);
        menu.findItem(R.id.action_copy_link).setEnabled(webPage);
        menu.findItem(R.id.action_find).setEnabled(tab != null && !tab.isHome());
        menu.findItem(R.id.action_desktop_site).setChecked(prefs.desktopSite());
        popup.setOnMenuItemClickListener(this::onMenuItemSelected);
        popup.show();
    }

    private boolean onMenuItemSelected(MenuItem item) {
        int id = item.getItemId();
        if (id == R.id.action_new_tab) {
            openNewTabFromUi();
        } else if (id == R.id.action_reload) {
            reloadOrStop();
        } else if (id == R.id.action_share) {
            shareCurrentPage();
        } else if (id == R.id.action_copy_link) {
            copyCurrentUrl();
        } else if (id == R.id.action_find) {
            showFindBar();
        } else if (id == R.id.action_desktop_site) {
            prefs.setDesktopSite(!prefs.desktopSite());
            applySettingsIfChanged();
        } else if (id == R.id.action_settings) {
            startActivityForResult(new Intent(this, SettingsActivity.class), REQUEST_SETTINGS);
        } else if (id == R.id.action_clear_data) {
            confirmClearBrowsingData();
        } else if (id == R.id.action_exit) {
            finishAndRemoveTask();
        } else {
            return false;
        }
        return true;
    }

    private void reloadOrStop() {
        BrowserTab tab = currentTab;
        if (tab == null) {
            return;
        }
        if (tab.isLoading()) {
            tab.webView.stopLoading();
            tab.progress = 100;
        } else {
            tab.webView.reload();
        }
        updateUi();
    }

    private void shareCurrentPage() {
        BrowserTab tab = currentTab;
        if (tab == null || !NavigationPolicy.isWebUrl(tab.url)) {
            return;
        }
        Intent send = new Intent(Intent.ACTION_SEND)
                .setType("text/plain")
                .putExtra(Intent.EXTRA_TEXT, tab.url);
        if (!TextUtils.isEmpty(tab.title)) {
            send.putExtra(Intent.EXTRA_SUBJECT, tab.title);
        }
        try {
            startActivity(Intent.createChooser(send, getString(R.string.share_link)));
        } catch (ActivityNotFoundException e) {
            Toast.makeText(this, R.string.no_app_found, Toast.LENGTH_SHORT).show();
        }
    }

    private void copyCurrentUrl() {
        BrowserTab tab = currentTab;
        ClipboardManager clipboard = getSystemService(ClipboardManager.class);
        if (tab == null || clipboard == null || !NavigationPolicy.isWebUrl(tab.url)) {
            return;
        }
        clipboard.setPrimaryClip(ClipData.newPlainText(tab.title, tab.url));
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            // Android 13+ shows its own clipboard confirmation.
            Toast.makeText(this, R.string.link_copied, Toast.LENGTH_SHORT).show();
        }
    }

    private void confirmClearBrowsingData() {
        AlertDialog.Builder builder = new AlertDialog.Builder(this)
                .setTitle(R.string.clear_data_title)
                .setMessage(R.string.clear_data_message)
                .setPositiveButton(R.string.clear_data_confirm, (d, w) -> {
                    BrowsingData.clearShared(this);
                    BrowsingData.clearTabs(tabs);
                    updateUi();
                    Toast.makeText(this, R.string.data_cleared, Toast.LENGTH_SHORT).show();
                })
                .setNegativeButton(R.string.cancel, null);
        showDialog(builder, null);
    }

    // ---------------------------------------------------------------------------------------
    // Find in page
    // ---------------------------------------------------------------------------------------

    private void showFindBar() {
        if (currentTab == null || currentTab.isHome()) {
            return;
        }
        findBar.setVisibility(View.VISIBLE);
        findInput.requestFocus();
        showKeyboard(findInput);
        if (findInput.length() > 0) {
            currentTab.webView.findAllAsync(findInput.getText().toString());
        }
    }

    private void closeFindBar() {
        if (findBar.getVisibility() != View.VISIBLE) {
            return;
        }
        hideKeyboard(findInput);
        findBar.setVisibility(View.GONE);
        findInput.setText("");
        findCount.setText("");
        if (currentTab != null) {
            currentTab.webView.clearMatches();
        }
        root.requestFocus();
    }

    // ---------------------------------------------------------------------------------------
    // Back navigation
    // ---------------------------------------------------------------------------------------

    private void registerBackCallback() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT, this::onBackRequested);
        }
    }

    /** API 24-32 path; API 33+ uses the OnBackInvokedCallback registered in onCreate. */
    @SuppressLint("GestureBackNavigation")
    @SuppressWarnings("deprecation")
    @Override
    public void onBackPressed() {
        onBackRequested();
    }

    private void onBackRequested() {
        if (!handleBack()) {
            // Nothing left to go back to: keep the tabs and return to the previous app.
            if (!moveTaskToBack(true)) {
                finish();
            }
        }
    }

    /** @return true when back was consumed inside the browser */
    private boolean handleBack() {
        if (customView != null) {
            exitFullscreen();
            return true;
        }
        if (findBar.getVisibility() == View.VISIBLE) {
            closeFindBar();
            return true;
        }
        if (omnibox.hasFocus()) {
            dismissOmnibox();
            return true;
        }
        return goBackInTab();
    }

    /** Back in the tab's history, or close a pop-up tab and return to the tab that opened it. */
    private boolean goBackInTab() {
        BrowserTab tab = currentTab;
        if (tab == null) {
            return false;
        }
        if (tab.webView.canGoBack()) {
            tab.webView.goBack();
            return true;
        }
        if (isOpen(tab.opener)) {
            closeTab(tab); // switches back to the opener
            return true;
        }
        return false;
    }

    // ---------------------------------------------------------------------------------------
    // Fullscreen video
    // ---------------------------------------------------------------------------------------

    void enterFullscreen(View view, WebChromeClient.CustomViewCallback callback) {
        if (customView != null) {
            callback.onCustomViewHidden();
            return;
        }
        dismissOmnibox();
        customView = view;
        customViewCallback = callback;
        fullscreenContainer = new FrameLayout(this);
        fullscreenContainer.setBackgroundColor(Color.BLACK);
        fullscreenContainer.addView(view, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        ((ViewGroup) getWindow().getDecorView()).addView(fullscreenContainer,
                new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.MATCH_PARENT));
        SystemBars.setImmersive(getWindow(), true);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
    }

    void exitFullscreen() {
        if (customView == null) {
            return;
        }
        ((ViewGroup) getWindow().getDecorView()).removeView(fullscreenContainer);
        fullscreenContainer.removeAllViews();
        fullscreenContainer = null;
        customView = null;
        SystemBars.setImmersive(getWindow(), false);
        getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        WebChromeClient.CustomViewCallback callback = customViewCallback;
        customViewCallback = null;
        if (callback != null) {
            callback.onCustomViewHidden();
        }
    }

    // ---------------------------------------------------------------------------------------
    // File uploads
    // ---------------------------------------------------------------------------------------

    /** Always answers the callback exactly once (files, or null on cancel/error). */
    boolean showFileChooser(ValueCallback<Uri[]> callback,
            WebChromeClient.FileChooserParams params) {
        if (fileChooserCallback != null) {
            fileChooserCallback.onReceiveValue(null);
            fileChooserCallback = null;
        }
        try {
            Intent intent = params.createIntent();
            if (params.getMode() == WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE) {
                intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
            }
            fileChooserCallback = callback;
            startActivityForResult(intent, REQUEST_FILE_CHOOSER);
        } catch (ActivityNotFoundException | SecurityException e) {
            fileChooserCallback = null;
            callback.onReceiveValue(null);
            Toast.makeText(this, R.string.file_chooser_failed, Toast.LENGTH_SHORT).show();
        }
        return true;
    }

    private void deliverFileChooserResult(int resultCode, Intent data) {
        ValueCallback<Uri[]> callback = fileChooserCallback;
        fileChooserCallback = null;
        if (callback == null) {
            return;
        }
        Uri[] result = null;
        if (resultCode == RESULT_OK && data != null) {
            ClipData clip = data.getClipData();
            if (clip != null && clip.getItemCount() > 0) {
                List<Uri> uris = new ArrayList<>();
                for (int i = 0; i < clip.getItemCount(); i++) {
                    Uri uri = clip.getItemAt(i).getUri();
                    if (uri != null) {
                        uris.add(uri);
                    }
                }
                result = uris.isEmpty() ? null : uris.toArray(new Uri[0]);
            } else if (data.getData() != null) {
                result = new Uri[] {data.getData()};
            } else {
                result = WebChromeClient.FileChooserParams.parseResult(resultCode, data);
            }
        }
        callback.onReceiveValue(result);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (requestCode == REQUEST_FILE_CHOOSER) {
            deliverFileChooserResult(resultCode, data);
        } else if (requestCode == REQUEST_SETTINGS) {
            if (data != null && data.getBooleanExtra(SettingsActivity.EXTRA_DATA_CLEARED, false)) {
                BrowsingData.clearTabs(tabs);
            }
        } else {
            super.onActivityResult(requestCode, resultCode, data);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Runtime permissions, dialogs
    // ---------------------------------------------------------------------------------------

    SitePermissions sitePermissions() {
        return sitePermissions;
    }

    /** Keys of certificate-error prompts currently on screen (see {@link SecurityDialogs}). */
    Set<String> sslPromptsShowing() {
        return sslPromptsShowing;
    }

    boolean hasPermission(String permission) {
        return checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED;
    }

    /** Requests the missing permissions, then calls {@code callback} (also when denied). */
    void requestRuntimePermissions(String[] permissions, PermissionResult callback) {
        List<String> missing = new ArrayList<>();
        for (String permission : permissions) {
            if (!hasPermission(permission)) {
                missing.add(permission);
            }
        }
        if (missing.isEmpty()) {
            callback.onResult();
            return;
        }
        int requestCode = nextPermissionRequest;
        nextPermissionRequest = requestCode >= LAST_PERMISSION_REQUEST
                ? FIRST_PERMISSION_REQUEST : requestCode + 1;
        permissionCallbacks.put(requestCode, callback);
        requestPermissions(missing.toArray(new String[0]), requestCode);
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, String[] permissions,
            int[] grantResults) {
        PermissionResult callback = permissionCallbacks.get(requestCode);
        if (callback != null) {
            permissionCallbacks.remove(requestCode);
            callback.onResult();
        } else {
            super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        }
    }

    /**
     * Shows a dialog that is dismissed when the activity is destroyed; {@code onDismiss} runs
     * on every dismissal so callers can resolve pending requests as "cancel".
     */
    AlertDialog showDialog(AlertDialog.Builder builder, DialogInterface.OnDismissListener onDismiss) {
        AlertDialog dialog = builder.create();
        dialog.setOnDismissListener(d -> {
            openDialogs.remove(dialog);
            if (onDismiss != null) {
                onDismiss.onDismiss(d);
            }
        });
        if (isFinishing() || isDestroyed()) {
            if (onDismiss != null) {
                onDismiss.onDismiss(dialog);
            }
            return dialog;
        }
        openDialogs.add(dialog);
        dialog.show();
        return dialog;
    }

    // ---------------------------------------------------------------------------------------
    // Incoming intents
    // ---------------------------------------------------------------------------------------

    /** VIEW (http/https), WEB_SEARCH and SEND text/plain each open a new tab. */
    private boolean openFromIntent(Intent intent) {
        if (intent == null
                || (intent.getFlags() & Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0) {
            return false;
        }
        String target = null;
        try {
            String action = intent.getAction();
            if (Intent.ACTION_VIEW.equals(action)) {
                Uri data = intent.getData();
                String url = data == null ? null : data.toString();
                target = NavigationPolicy.isWebUrl(url) ? url : null;
            } else if (Intent.ACTION_WEB_SEARCH.equals(action)) {
                target = UrlResolver.resolve(intent.getStringExtra(SearchManager.QUERY), searchConfig());
            } else if (Intent.ACTION_SEND.equals(action)
                    && "text/plain".equals(intent.getType())) {
                target = UrlResolver.resolveSharedText(
                        intent.getCharSequenceExtra(Intent.EXTRA_TEXT), searchConfig());
            }
        } catch (RuntimeException e) {
            return false; // malformed extras from another app
        }
        if (target == null) {
            return false;
        }
        dismissOmnibox();
        openTab(target, null);
        return true;
    }

    /** No-op defaults for {@link TextWatcher}. */
    private abstract static class SimpleTextWatcher implements TextWatcher {
        @Override
        public void beforeTextChanged(CharSequence s, int start, int count, int after) {
        }

        @Override
        public void onTextChanged(CharSequence s, int start, int before, int count) {
        }
    }
}
