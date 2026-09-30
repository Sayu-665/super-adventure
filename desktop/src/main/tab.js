'use strict';

// A browser tab: one sandboxed WebContentsView (no preload, no Node) plus its UI state.

const { WebContentsView, dialog, nativeTheme } = require('electron');
const registry = require('./registry');
const security = require('./security');
const keyboard = require('./keyboard');
const contextMenu = require('./context-menu');
const { faviconDataUrl } = require('./favicon');
const { classifyUrl, isHomeUrl, protocolOf, hostOf } = require('./urls');

let nextTabId = 1;

const NET_ERROR_TEXT = {
  '-105': "The server's address could not be found.",
  '-106': 'You are offline. Check your internet connection.',
  '-102': 'The server refused the connection.',
  '-101': 'The connection was reset.',
  '-100': 'The connection was closed unexpectedly.',
  '-109': 'The address is unreachable.',
  '-118': 'The connection timed out.',
  '-137': 'The name could not be resolved.',
  '-130': 'The proxy server is not responding.',
  '-111': 'The proxy tunnel could not be established.',
  '-301': 'The address is not valid.',
  '-300': 'The address is not valid.',
  '-302': 'OpenSurf cannot open this kind of address.',
  '-310': 'The page redirected too many times.',
  '-20': 'The page was blocked.',
  '-27': 'The page was blocked by the client.',
  '-10': 'Access to this page is denied.',
  '-6': 'The file could not be found.',
  '-324': 'The server sent no data.',
};

function describeError(code, description) {
  if (code <= -200 && code > -300) return 'The certificate of this site is not trusted, so the connection is not private.';
  return NET_ERROR_TEXT[String(code)] || 'The page could not be loaded.';
}

class Tab {
  /**
   * @param {import('./window-controller').WindowController} ctrl
   * @param {{openerId?: number}} [options]
   */
  constructor(ctrl, options = {}) {
    this.ctrl = ctrl;
    this.browser = ctrl.browser;
    this.id = nextTabId++;
    this.openerId = options.openerId || null;
    this.title = '';
    this.favicon = null;
    this.faviconToken = 0;
    this.loading = false;
    this.progress = 0;
    this.error = null; // { code, title, description, url, kind }
    this.pendingUrl = ''; // browser-initiated URL shown while it loads
    this.lastRequestedUrl = '';
    this.destroyed = false;
    this.createView();
  }

  createView() {
    this.view = new WebContentsView({
      webPreferences: {
        sandbox: true,
        contextIsolation: true,
        nodeIntegration: false,
        nodeIntegrationInSubFrames: false,
        nodeIntegrationInWorker: false,
        webSecurity: true,
        allowRunningInsecureContent: false,
        webviewTag: false,
        javascript: this.browser.settings.get().javascript,
        spellcheck: true,
        safeDialogs: true,
        navigateOnDragDrop: false,
      },
    });
    this.wc = this.view.webContents;
    this.wcId = this.wc.id;
    this.view.setBackgroundColor(nativeTheme.shouldUseDarkColors ? '#1b1c20' : '#ffffff');
    this.view.setVisible(false);
    registry.tabsByWebContentsId.set(this.wcId, this);
    this.bindEvents();
  }

  bindEvents() {
    const wc = this.wc;
    const update = () => this.ctrl.scheduleUpdate(this);

    wc.setWindowOpenHandler((details) => this.onWindowOpen(details));
    wc.on('will-navigate', (event, url) => this.onWillNavigate(event, url));
    wc.on('will-frame-navigate', (event) => this.onWillFrameNavigate(event));
    wc.on('will-redirect', (event, url, _inPlace, isMainFrame) => this.onWillRedirect(event, url, isMainFrame));

    wc.on('did-start-loading', () => {
      this.loading = true;
      this.progress = Math.max(this.progress, 0.1);
      update();
    });
    wc.on('did-start-navigation', (details) => {
      if (!details.isMainFrame || details.isSameDocument) return;
      this.progress = Math.max(this.progress, 0.25);
      update();
    });
    wc.on('did-navigate', (_event, url) => {
      this.pendingUrl = '';
      this.error = null;
      this.progress = Math.max(this.progress, 0.6);
      this.title = '';
      this.setFavicon(null);
      this.view.setBackgroundColor(isHomeUrl(url) && nativeTheme.shouldUseDarkColors ? '#1b1c20' : '#ffffff');
      this.ctrl.onTabNavigated(this);
    });
    wc.on('did-navigate-in-page', (_event, _url, isMainFrame) => {
      if (isMainFrame) this.ctrl.onTabNavigated(this);
    });
    wc.on('dom-ready', () => {
      this.progress = Math.max(this.progress, 0.8);
      update();
    });
    wc.on('did-stop-loading', () => {
      this.loading = false;
      this.progress = 0;
      this.pendingUrl = '';
      update();
    });
    wc.on('did-fail-load', (_event, code, description, url, isMainFrame) => {
      if (!isMainFrame || code === -3) return; // -3: aborted (superseded / download)
      this.pendingUrl = '';
      this.error = {
        kind: code <= -200 && code > -300 ? 'certificate' : 'network',
        code,
        name: description || String(code),
        title: code <= -200 && code > -300 ? 'Your connection is not private' : "This page isn't available",
        description: describeError(code, description),
        url: String(url || '').slice(0, 4096),
      };
      this.ctrl.onTabErrorChanged(this);
    });
    wc.on('render-process-gone', (_event, details) => {
      if (this.destroyed || details.reason === 'clean-exit') return;
      this.loading = false;
      this.error = {
        kind: 'crashed',
        code: 0,
        name: details.reason,
        title: 'This page crashed',
        description: 'Something went wrong while displaying this page.',
        url: this.url(),
      };
      this.ctrl.onTabErrorChanged(this);
    });
    wc.on('page-title-updated', (_event, title) => {
      this.title = title;
      this.ctrl.onTabTitleChanged(this);
    });
    wc.on('page-favicon-updated', (_event, favicons) => this.loadFavicon(favicons));
    wc.on('audio-state-changed', update);
    wc.on('enter-html-full-screen', () => this.ctrl.setHtmlFullscreen(this, true));
    wc.on('leave-html-full-screen', () => this.ctrl.setHtmlFullscreen(this, false));
    wc.on('found-in-page', (_event, result) => this.ctrl.onFindResult(this, result));
    wc.on('zoom-changed', (_event, direction) => this.ctrl.zoom(this, direction === 'in' ? 'in' : 'out'));
    wc.on('context-menu', (_event, params) => contextMenu.showPageMenu(this, params));
    wc.on('before-input-event', (event, input) => {
      if (keyboard.handleInput(this.ctrl, input, 'tab')) event.preventDefault();
    });
    wc.on('select-bluetooth-device', (event, _devices, callback) => {
      event.preventDefault();
      callback('');
    });
    wc.on('will-prevent-unload', (event) => {
      // A page asked to confirm leaving (beforeunload). Default is to stay.
      const win = this.ctrl.win;
      if (!win || win.isDestroyed()) return;
      const choice = dialog.showMessageBoxSync(win, {
        type: 'question',
        buttons: ['Leave', 'Stay'],
        defaultId: 1,
        cancelId: 1,
        message: 'Leave site?',
        detail: 'Changes that you made may not be saved.',
        noLink: true,
      });
      if (choice === 0) event.preventDefault();
    });
  }

  // ---- navigation guards -------------------------------------------------

  /** Renderer-initiated main-frame navigations. */
  onWillNavigate(event, url) {
    const kind = classifyUrl(url);
    switch (kind) {
      case 'go':
        event.preventDefault();
        this.ctrl.navigateFromGoUrl(this, url);
        return;
      case 'home':
        // Always show the canonical home page (current settings in the fragment).
        if (url !== this.browser.homeUrl()) {
          event.preventDefault();
          this.load(this.browser.homeUrl());
        }
        return;
      case 'external':
        event.preventDefault();
        security.openExternalWithConsent(this.wc, url, this.wc.getURL());
        return;
      case 'file':
        // Web pages may never navigate to file:, only local pages the user opened may.
        if (classifyUrl(this.wc.getURL()) !== 'file') event.preventDefault();
        return;
      case 'web':
      case 'blob':
      case 'about':
      case 'data': // Chromium itself blocks renderer-initiated top-level data: navigations
        return;
      default:
        event.preventDefault();
    }
  }

  onWillFrameNavigate(event) {
    if (event.isMainFrame) return; // handled by will-navigate
    const kind = classifyUrl(event.url);
    if (kind === 'go' || kind === 'external' || kind === 'file' || kind === 'blocked') event.preventDefault();
  }

  onWillRedirect(event, url, isMainFrame) {
    const kind = classifyUrl(url);
    if (kind === 'web' || (!isMainFrame && (kind === 'blob' || kind === 'about' || kind === 'data'))) return;
    event.preventDefault();
  }

  onWindowOpen({ url, disposition }) {
    const kind = classifyUrl(url);
    const background = disposition === 'background-tab';
    if (kind === 'web') {
      this.ctrl.openTab(url, { openerId: this.id, background });
    } else if (kind === 'home') {
      this.ctrl.openTab(this.browser.homeUrl(), { openerId: this.id, background });
    } else if (kind === 'go') {
      const target = this.browser.resolveGoUrl(url);
      if (target) this.ctrl.openTab(target, { openerId: this.id, background });
    } else if (kind === 'external') {
      security.openExternalWithConsent(this.wc, url, this.wc.getURL());
    }
    return { action: 'deny' };
  }

  // ---- actions ------------------------------------------------------------

  load(url) {
    if (this.destroyed || typeof url !== 'string' || !url) return;
    this.pendingUrl = url;
    this.lastRequestedUrl = url;
    this.error = null;
    this.progress = 0.05;
    this.wc.loadURL(url).catch(() => { /* reported through did-fail-load */ });
    this.ctrl.onTabErrorChanged(this);
  }

  reload(hard = false) {
    if (this.error && this.error.url) {
      this.load(this.error.url);
      return;
    }
    if (hard) this.wc.reloadIgnoringCache();
    else this.wc.reload();
  }

  goBack() {
    if (this.wc.navigationHistory.canGoBack()) this.wc.navigationHistory.goBack();
  }

  goForward() {
    if (this.wc.navigationHistory.canGoForward()) this.wc.navigationHistory.goForward();
  }

  stop() {
    this.wc.stop();
  }

  url() {
    if (this.destroyed) return '';
    return this.pendingUrl || this.wc.getURL() || (this.error && this.error.url) || '';
  }

  isHome() {
    return isHomeUrl(this.url());
  }

  displayTitle() {
    const url = this.url();
    if (isHomeUrl(url)) return this.title && this.title !== url ? this.title : 'New Tab';
    if (this.error) return this.error.kind === 'crashed' ? 'Page crashed' : hostOf(this.error.url) || this.error.url;
    const title = this.title || this.wc.getTitle();
    if (title && title !== url) return title;
    return hostOf(url) || url || 'New Tab';
  }

  // ---- favicon --------------------------------------------------------------

  setFavicon(dataUrl) {
    this.faviconToken++;
    this.favicon = dataUrl;
  }

  async loadFavicon(favicons) {
    const candidate = (favicons || []).find((u) => typeof u === 'string' && /^(https?:|data:image\/|opensurf:)/i.test(u));
    if (!candidate) return;
    const token = ++this.faviconToken;
    const dataUrl = await faviconDataUrl(candidate, this.wc.session);
    if (this.destroyed || token !== this.faviconToken) return;
    this.favicon = dataUrl;
    this.ctrl.scheduleUpdate(this);
  }

  // ---- state ---------------------------------------------------------------

  toJSON() {
    const url = this.url();
    const home = isHomeUrl(url);
    const history = this.wc.navigationHistory;
    return {
      id: this.id,
      title: this.displayTitle(),
      url,
      displayUrl: home ? '' : url,
      isHome: home,
      scheme: protocolOf(url).replace(/:$/, ''),
      loading: this.loading,
      progress: this.progress,
      canGoBack: history.canGoBack(),
      canGoForward: history.canGoForward(),
      favicon: this.favicon,
      zoom: Math.round(this.wc.getZoomFactor() * 100),
      audible: this.wc.isCurrentlyAudible(),
      muted: this.wc.isAudioMuted(),
      error: this.error,
      javascript: this.jsEnabled(),
    };
  }

  jsEnabled() {
    try {
      return this.wc.getLastWebPreferences().javascript !== false;
    } catch (_) {
      return true;
    }
  }

  /** Rebuilds the web contents (e.g. to apply the JavaScript setting), keeping back/forward history. */
  recreate() {
    if (this.destroyed) return;
    const history = this.wc.navigationHistory;
    const entries = history.getAllEntries().map(({ url, title }) => ({ url, title }));
    const index = history.getActiveIndex();
    const fallbackUrl = this.url() || this.browser.homeUrl();
    const wasVisible = this.ctrl.activeId === this.id;
    this.disposeView();
    this.createView();
    this.ctrl.attachView(this);
    this.error = null;
    this.loading = false;
    this.setFavicon(null);
    const valid = entries.length && index >= 0 && index < entries.length;
    if (valid) this.wc.navigationHistory.restore({ entries, index }).catch(() => this.load(fallbackUrl));
    else this.load(fallbackUrl);
    if (wasVisible) this.ctrl.layout();
    this.ctrl.scheduleUpdate(this);
  }

  disposeView() {
    registry.tabsByWebContentsId.delete(this.wcId);
    const view = this.view;
    const wc = this.wc;
    try {
      if (!this.ctrl.win.isDestroyed()) this.ctrl.win.contentView.removeChildView(view);
    } catch (_) { /* window gone */ }
    if (!wc.isDestroyed()) wc.close();
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.disposeView();
  }
}

module.exports = { Tab };
