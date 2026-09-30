'use strict';

// App-wide state shared by all windows: settings, downloads, open-tab persistence,
// recently closed tabs and "Clear browsing data".

const { app, session } = require('electron');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const omnibox = require('../shared/omnibox');
const registry = require('./registry');
const { SettingsStore } = require('./settings');
const { SessionStore } = require('./session-store');
const { DownloadManager } = require('./downloads');
const { handleProtocol } = require('./protocol');
const security = require('./security');
const { HOME_URL, homeUrl, homeCaption, isHomeUrl } = require('./urls');
const { WindowController } = require('./window-controller');

const MAX_CLOSED_TABS = 25;

class Browser {
  constructor() {
    this.settings = null;
    this.sessionStore = null;
    this.downloads = null;
    this.closedTabs = [];
    this.quitting = false;
    this.sessionFrozen = false; // set once the final state for the next launch was written
    this.saveTimer = null;
  }

  /** Call once, after app 'ready'. */
  init() {
    const userData = app.getPath('userData');
    this.settings = new SettingsStore(path.join(userData, 'settings.json'));
    this.sessionStore = new SessionStore(path.join(userData, 'session.json'));

    const ses = session.defaultSession;
    handleProtocol(ses, { caption: () => homeCaption(this.settings.get()) });
    security.setupPermissions(ses);
    security.setupCertificateHandling();
    this.configureSession(ses);

    this.downloads = new DownloadManager();
    this.downloads.attach(ses);
    this.downloads.on('changed', (list) => this.broadcast('downloads', list));
    this.downloads.on('started', (info) => this.broadcast('download-started', info));

    this.settings.on('changed', (values, changed) => {
      this.broadcast('settings', values);
      if (changed.includes('restoreTabs')) {
        if (values.restoreTabs) this.saveSessionNow();
        else this.sessionStore.clear();
      }
    });
  }

  configureSession(ses) {
    // A plain Chromium user agent (no "Electron/x" or app token): better site compatibility, less fingerprinting.
    const ua = ses.getUserAgent().replace(/\s+opensurf\/\S+/gi, '').replace(/\s+Electron\/\S+/g, '');
    ses.setUserAgent(ua);
    app.userAgentFallback = ua;
    // Never download Hunspell dictionaries from Google's CDN (Linux). Point the downloader at a
    // local folder instead; Chromium loads .bdic files placed in <userData>/Dictionaries.
    try {
      ses.setSpellCheckerDictionaryDownloadURL(`${pathToFileURL(path.join(app.getPath('userData'), 'Dictionaries')).href}/`);
    } catch (_) { /* macOS uses the native spellchecker */ }
  }

  // ---- search / resolution (single source of truth: shared/omnibox.js) --------------------

  homeUrl() {
    return homeUrl(this.settings.get());
  }

  engineName() {
    return omnibox.engineName(this.settings.get());
  }

  searchUrl(text) {
    return omnibox.buildSearchUrl(text, this.settings.searchOptions());
  }

  resolve(text, { allowFile = false } = {}) {
    return omnibox.resolveInput(text, this.settings.searchOptions({ allowFile }));
  }

  /** opensurf://go?q=... -> URL to load (file: never allowed from pages). */
  resolveGoUrl(url) {
    const q = omnibox.parseGoUrl(url);
    return q === null ? null : this.resolve(q, { allowFile: false });
  }

  // ---- windows --------------------------------------------------------------------------------

  createWindow(options = {}) {
    this.sessionFrozen = false;
    return new WindowController(this, options);
  }

  controllers() {
    return [...registry.controllers].filter((c) => !c.destroyed);
  }

  forEachController(fn) {
    for (const c of this.controllers()) fn(c);
  }

  broadcast(type, payload) {
    this.forEachController((c) => c.send(type, payload));
  }

  /** URLs from the command line, a second launch or the OS (open-url). */
  openUrls(urls) {
    const ctrl = registry.lastController();
    if (!ctrl) {
      this.createWindow({ urls });
      return;
    }
    for (const url of urls) ctrl.openTab(url);
    this.focusWindow(ctrl);
  }

  focusWindow(ctrl = registry.lastController()) {
    if (!ctrl) {
      this.createWindow();
      return;
    }
    if (ctrl.win.isMinimized()) ctrl.win.restore();
    ctrl.win.show();
    ctrl.win.focus();
  }

  /** First window(s) at startup: restored tabs (if enabled) plus URLs from the command line. */
  start(urls) {
    const saved = this.settings.get().restoreTabs ? this.sessionStore.load() : [];
    if (!saved.length) {
      this.createWindow({ urls });
      return;
    }
    for (const w of saved) {
      this.createWindow({ urls: w.tabs.map((t) => (isHomeUrl(t.url) ? this.homeUrl() : t.url)), activeIndex: w.activeIndex });
    }
    if (urls.length) this.openUrls(urls);
  }

  // ---- closed tabs ------------------------------------------------------------------------------

  recordClosedTab(entry) {
    this.closedTabs.push(entry);
    if (this.closedTabs.length > MAX_CLOSED_TABS) this.closedTabs.shift();
  }

  reopenClosedTab(ctrl) {
    const entry = this.closedTabs.pop();
    if (entry) ctrl.openTab(entry.url);
    this.forEachController((c) => c.scheduleUpdate());
  }

  /** Applies the JavaScript setting to open tabs by rebuilding them (history is kept, pages reload). */
  recreateAllTabs() {
    this.forEachController((c) => c.tabs.forEach((t) => t.recreate()));
  }

  // ---- clear browsing data ----------------------------------------------------------------------

  async clearBrowsingData() {
    const ses = session.defaultSession;
    await ses.clearStorageData(); // cookies, local/session storage, IndexedDB, service workers, cache storage...
    await ses.clearCache();
    await ses.clearAuthCache();
    await ses.clearHostResolverCache();
    await ses.clearCodeCaches({}).catch(() => {});
    this.forEachController((c) => {
      for (const t of c.tabs) {
        try {
          t.wc.navigationHistory.clear();
        } catch (_) { /* tab closing */ }
      }
    });
    this.closedTabs.length = 0;
    security.clearSessionDecisions();
    this.downloads.clearFinished();
    this.forEachController((c) => c.scheduleUpdate());
    return { ok: true };
  }

  // ---- open-tab persistence -------------------------------------------------------------------

  snapshot(ctrls = this.controllers().filter((c) => !c.closing)) {
    return ctrls.map((c) => ({
      tabs: c.tabs.map((t) => ({ url: t.isHome() ? HOME_URL : t.url(), title: t.displayTitle() })),
      activeIndex: Math.max(0, c.tabs.findIndex((t) => t.id === c.activeId)),
    }));
  }

  scheduleSessionSave() {
    if (this.sessionFrozen || !this.settings.get().restoreTabs) return;
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.saveSessionNow(), 1000);
  }

  saveSessionNow(windows = this.snapshot()) {
    clearTimeout(this.saveTimer);
    if (!this.settings.get().restoreTabs) return;
    this.sessionStore.save(windows);
  }

  /** A window is about to close. The last one is kept for the next launch. */
  onWindowClosing(ctrl) {
    if (this.quitting || this.sessionFrozen) return;
    const others = this.controllers().filter((c) => c !== ctrl && !c.closing);
    if (others.length) {
      this.saveSessionNow();
    } else {
      this.saveSessionNow(this.snapshot([ctrl]));
      this.sessionFrozen = true;
    }
  }

  onBeforeQuit() {
    if (this.quitting) return;
    if (!this.sessionFrozen) this.saveSessionNow();
    this.quitting = true;
    this.sessionFrozen = true;
  }
}

module.exports = { Browser };
