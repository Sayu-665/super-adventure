'use strict';

// One browser window: a BrowserWindow whose own webContents renders the chrome UI
// (src/renderer), plus one WebContentsView per tab positioned below it.

const { BrowserWindow, nativeTheme, screen } = require('electron');
const path = require('node:path');
const registry = require('./registry');
const keyboard = require('./keyboard');
const contextMenu = require('./context-menu');
const { Tab } = require('./tab');

const CHROME_HTML = path.join(__dirname, '..', 'renderer', 'index.html');
const CHROME_PRELOAD = path.join(__dirname, '..', 'preload', 'chrome-preload.js');
const ICON = path.join(__dirname, '..', 'assets', 'icon.png');
const ZOOM_STEPS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];

function themeBackground() {
  return nativeTheme.shouldUseDarkColors ? '#1b1c20' : '#f3f4f6';
}

class WindowController {
  /**
   * @param {import('./browser').Browser} browser
   * @param {{urls?: string[], activeIndex?: number}} options
   */
  constructor(browser, options = {}) {
    this.browser = browser;
    this.tabs = [];
    this.activeId = null;
    this.chromeHeight = 88; // updated by the chrome UI once rendered
    this.chromeReady = false;
    this.overlay = false; // a popover covers the page area (page shown as a screenshot)
    this.htmlFullscreenTab = null;
    this.findOpen = false;
    this.destroyed = false;
    this.closing = false;
    this.lastFocused = Date.now();
    this.pendingUpdate = false;

    const area = screen.getPrimaryDisplay().workAreaSize;
    this.win = new BrowserWindow({
      width: Math.min(1280, Math.max(800, Math.round(area.width * 0.8))),
      height: Math.min(860, Math.max(600, Math.round(area.height * 0.85))),
      minWidth: 640,
      minHeight: 480,
      title: 'OpenSurf',
      icon: ICON,
      show: false,
      backgroundColor: themeBackground(),
      webPreferences: {
        preload: CHROME_PRELOAD,
        sandbox: true,
        contextIsolation: true,
        nodeIntegration: false,
        webSecurity: true,
        webviewTag: false,
        spellcheck: true,
        navigateOnDragDrop: false,
      },
    });
    registry.controllers.add(this);
    this.bindWindowEvents();
    this.bindChromeEvents();

    const urls = options.urls && options.urls.length ? options.urls : [browser.homeUrl()];
    for (const url of urls) this.addTab(url, { activate: false });
    const idx = Number.isInteger(options.activeIndex) ? Math.min(Math.max(options.activeIndex, 0), this.tabs.length - 1) : 0;
    this.activate(this.tabs[idx].id, { focus: false });

    this.win.loadFile(CHROME_HTML);
  }

  bindWindowEvents() {
    const win = this.win;
    win.once('ready-to-show', () => {
      win.show();
      this.focusDefault();
    });
    for (const ev of ['resize', 'maximize', 'unmaximize', 'restore', 'enter-full-screen', 'leave-full-screen']) {
      win.on(ev, () => {
        this.layout();
        if (ev.endsWith('full-screen')) this.scheduleUpdate();
      });
    }
    win.on('focus', () => {
      this.lastFocused = Date.now();
    });
    win.on('page-title-updated', (event) => event.preventDefault()); // title is "<page> - OpenSurf"
    win.on('close', () => {
      this.closing = true;
      this.browser.onWindowClosing(this);
    });
    win.on('closed', () => {
      this.destroyed = true;
      registry.controllers.delete(this);
      for (const tab of this.tabs) tab.destroy();
      this.tabs = [];
    });
    const onTheme = () => {
      if (!win.isDestroyed()) win.setBackgroundColor(themeBackground());
    };
    nativeTheme.on('updated', onTheme);
    win.once('closed', () => nativeTheme.removeListener('updated', onTheme));
  }

  bindChromeEvents() {
    const wc = this.win.webContents;
    wc.setVisualZoomLevelLimits(1, 1).catch(() => {});
    wc.setWindowOpenHandler(() => ({ action: 'deny' }));
    wc.on('will-navigate', (event) => event.preventDefault()); // the chrome UI never navigates
    wc.on('will-frame-navigate', (event) => {
      if (!event.isMainFrame) event.preventDefault();
    });
    wc.on('before-input-event', (event, input) => {
      if (keyboard.handleInput(this, input, 'chrome')) event.preventDefault();
    });
    wc.on('context-menu', (_event, params) => contextMenu.showChromeMenu(this, params));
    wc.on('did-start-loading', () => {
      this.chromeReady = false;
    });
    wc.on('render-process-gone', (_event, details) => {
      this.chromeReady = false;
      this.overlay = false;
      if (details.reason !== 'clean-exit' && !this.win.isDestroyed()) setTimeout(() => !this.win.isDestroyed() && wc.reload(), 500);
    });
  }

  // ---- chrome UI messaging -------------------------------------------------

  send(type, payload) {
    if (this.destroyed || !this.chromeReady) return;
    const wc = this.win.webContents;
    if (!wc.isDestroyed()) wc.send('opensurf:event', type, payload);
  }

  /** Called when the chrome UI script has loaded and subscribed. */
  onChromeReady() {
    this.chromeReady = true;
    this.overlay = false;
    this.send('init', {
      platform: process.platform,
      settings: this.browser.settings.get(),
      downloads: this.browser.downloads.list(),
    });
    this.sendState();
    this.layout();
    if (this.activeTab() && this.activeTab().isHome() && this.win.isFocused()) this.send('focus-omnibox', { select: true });
  }

  scheduleUpdate() {
    if (this.pendingUpdate || this.destroyed) return;
    this.pendingUpdate = true;
    setImmediate(() => {
      this.pendingUpdate = false;
      this.sendState();
    });
  }

  sendState() {
    if (this.destroyed) return;
    this.send('state', {
      tabs: this.tabs.map((t) => t.toJSON()),
      activeId: this.activeId,
      fullscreen: this.win.isFullScreen(),
      htmlFullscreen: Boolean(this.htmlFullscreenTab),
      canReopenClosedTab: this.browser.closedTabs.length > 0,
    });
  }

  updateWindowTitle() {
    const tab = this.activeTab();
    if (!tab || this.win.isDestroyed()) return;
    this.win.setTitle(`${tab.displayTitle()} - OpenSurf`);
  }

  // ---- tabs -----------------------------------------------------------------

  getTab(id) {
    return this.tabs.find((t) => t.id === id) || null;
  }

  activeTab() {
    return this.getTab(this.activeId);
  }

  attachView(tab) {
    this.win.contentView.addChildView(tab.view);
  }

  /** Creates a tab. Inserts after the opener (and its other children) when given, else at the end. */
  addTab(url, { activate = true, openerId = null, background = false } = {}) {
    const tab = new Tab(this, { openerId });
    let index = this.tabs.length;
    const opener = openerId && this.getTab(openerId);
    if (opener) {
      index = this.tabs.indexOf(opener) + 1;
      while (index < this.tabs.length && this.tabs[index].openerId === opener.id) index++;
    }
    this.tabs.splice(index, 0, tab);
    this.attachView(tab);
    tab.load(url || this.browser.homeUrl());
    if (activate && !background) this.activate(tab.id);
    else this.layout();
    this.scheduleUpdate();
    this.browser.scheduleSessionSave();
    return tab;
  }

  newTab() {
    return this.addTab(this.browser.homeUrl());
  }

  openTab(url, options = {}) {
    return this.addTab(url, { ...options, activate: !options.background });
  }

  activate(id, { focus = true } = {}) {
    const tab = this.getTab(id);
    if (!tab) return;
    if (this.activeId !== id) {
      if (this.findOpen) this.closeFind({ notify: true });
      if (this.htmlFullscreenTab && this.htmlFullscreenTab !== tab) this.exitHtmlFullscreen();
      this.activeId = id;
    }
    tab.lastActive = Date.now();
    this.layout();
    this.updateWindowTitle();
    this.scheduleUpdate();
    if (focus) this.focusDefault();
  }

  /** Home tabs focus the omnibox, web pages get keyboard focus. */
  focusDefault() {
    const tab = this.activeTab();
    if (!tab || this.win.isDestroyed()) return;
    if (tab.isHome() || tab.error) this.focusOmnibox(true);
    else if (!this.overlay) tab.wc.focus();
  }

  focusOmnibox(select = true) {
    if (this.win.isDestroyed()) return;
    this.win.webContents.focus();
    this.send('focus-omnibox', { select });
  }

  closeTab(id) {
    const index = this.tabs.findIndex((t) => t.id === id);
    if (index === -1) return;
    const tab = this.tabs[index];
    const url = tab.url();
    if (url && !tab.isHome()) this.browser.recordClosedTab({ url, title: tab.displayTitle() });
    this.tabs.splice(index, 1);
    if (this.htmlFullscreenTab === tab) this.htmlFullscreenTab = null;
    if (!this.tabs.length) {
      tab.destroy();
      this.win.close();
      return;
    }
    if (this.activeId === id) {
      // Prefer the opener when closing a child tab, otherwise the tab that takes its place.
      const opener = tab.openerId && this.getTab(tab.openerId);
      const next = opener || this.tabs[Math.min(index, this.tabs.length - 1)];
      this.activeId = null;
      this.activate(next.id);
    }
    tab.destroy();
    this.layout();
    this.scheduleUpdate();
    this.browser.scheduleSessionSave();
  }

  closeOtherTabs(id) {
    for (const tab of this.tabs.slice()) if (tab.id !== id) this.closeTab(tab.id);
  }

  duplicateTab(id) {
    const tab = this.getTab(id);
    if (tab) this.openTab(tab.url() || this.browser.homeUrl(), { openerId: tab.id });
  }

  selectRelativeTab(delta) {
    if (this.tabs.length < 2) return;
    const index = this.tabs.findIndex((t) => t.id === this.activeId);
    const next = (index + delta + this.tabs.length) % this.tabs.length;
    this.activate(this.tabs[next].id);
  }

  selectTabIndex(n) {
    const tab = n === -1 ? this.tabs[this.tabs.length - 1] : this.tabs[n];
    if (tab) this.activate(tab.id);
  }

  moveTab(id, toIndex) {
    const from = this.tabs.findIndex((t) => t.id === id);
    if (from === -1) return;
    const [tab] = this.tabs.splice(from, 1);
    this.tabs.splice(Math.max(0, Math.min(toIndex, this.tabs.length)), 0, tab);
    this.scheduleUpdate();
    this.browser.scheduleSessionSave();
  }

  // ---- tab callbacks ----------------------------------------------------------

  onTabNavigated(tab) {
    this.scheduleUpdate();
    if (tab.id === this.activeId) {
      this.updateWindowTitle();
      this.layout();
    }
    this.browser.scheduleSessionSave();
  }

  onTabTitleChanged(tab) {
    this.scheduleUpdate();
    if (tab.id === this.activeId) this.updateWindowTitle();
  }

  onTabErrorChanged(tab) {
    this.scheduleUpdate();
    if (tab.id === this.activeId) {
      this.layout();
      this.updateWindowTitle();
    }
  }

  // ---- navigation --------------------------------------------------------------

  /** Omnibox submit: resolves typed text with the shared omnibox module. */
  navigate(text) {
    const url = this.browser.resolve(text, { allowFile: true });
    const tab = this.activeTab();
    if (!url || !tab) return;
    tab.load(url);
    if (!this.overlay) tab.wc.focus();
  }

  /** opensurf://go?q=... from the home page form (never allows file:). */
  navigateFromGoUrl(tab, goUrl) {
    const url = this.browser.resolveGoUrl(goUrl);
    if (url) tab.load(url);
    if (tab.id === this.activeId) tab.wc.focus();
  }

  goHome() {
    const tab = this.activeTab();
    if (tab) tab.load(this.browser.homeUrl());
  }

  // ---- layout ------------------------------------------------------------------

  setChromeHeight(height) {
    if (height === this.chromeHeight) return;
    this.chromeHeight = height;
    this.layout();
  }

  layout() {
    if (this.destroyed || this.win.isDestroyed()) return;
    const [width, height] = this.win.getContentSize();
    const fullscreenTab = this.htmlFullscreenTab;
    const zoom = this.win.webContents.getZoomFactor() || 1;
    const top = fullscreenTab ? 0 : Math.min(height, Math.round(this.chromeHeight * zoom));
    for (const tab of this.tabs) {
      const visible = fullscreenTab ? tab === fullscreenTab : tab.id === this.activeId && !tab.error && !this.overlay;
      if (visible) tab.view.setBounds({ x: 0, y: top, width, height: Math.max(0, height - top) });
      tab.view.setVisible(visible);
    }
  }

  setHtmlFullscreen(tab, on) {
    if (on) {
      if (tab.id !== this.activeId) this.activate(tab.id, { focus: false });
      this.htmlFullscreenTab = tab;
      if (!this.win.isFullScreen()) {
        this.fullscreenForHtml = true;
        this.win.setFullScreen(true);
      }
    } else if (this.htmlFullscreenTab === tab) {
      this.htmlFullscreenTab = null;
      if (this.fullscreenForHtml && this.win.isFullScreen()) this.win.setFullScreen(false);
      this.fullscreenForHtml = false;
    }
    this.layout();
    this.scheduleUpdate();
  }

  exitHtmlFullscreen() {
    const tab = this.htmlFullscreenTab;
    if (!tab || tab.destroyed) return;
    tab.wc.executeJavaScript('document.fullscreenElement && document.exitFullscreen()', true).catch(() => {});
  }

  toggleFullscreen() {
    if (this.htmlFullscreenTab) return this.exitHtmlFullscreen();
    this.win.setFullScreen(!this.win.isFullScreen());
  }

  /** Popovers cover the page area: hide the page view while one is open. */
  setOverlay(on) {
    if (this.overlay === on) return;
    this.overlay = on;
    this.layout();
    if (!on) {
      const tab = this.activeTab();
      if (tab && !tab.isHome() && !tab.error && this.win.isFocused()) tab.wc.focus();
    }
  }

  /** Screenshot of the visible page, shown behind popovers. */
  async captureActivePage() {
    const tab = this.activeTab();
    if (!tab || tab.error || this.overlay) return null;
    try {
      const image = await tab.wc.capturePage();
      if (image.isEmpty()) return null;
      return `data:image/jpeg;base64,${image.toJPEG(85).toString('base64')}`;
    } catch (_) {
      return null;
    }
  }

  // ---- find in page ------------------------------------------------------------------

  openFind() {
    this.findOpen = true;
    this.win.webContents.focus();
    this.send('open-find', {});
  }

  find(text, { forward = true, findNext = false } = {}) {
    const tab = this.activeTab();
    if (!tab) return;
    this.findOpen = true;
    this.findText = text;
    if (!text) {
      tab.wc.stopFindInPage('clearSelection');
      this.send('find-result', { tabId: tab.id, matches: 0, active: 0 });
      return;
    }
    tab.wc.findInPage(text, { forward, findNext });
  }

  /** F3 / Ctrl+G: next or previous match, or open the find bar. */
  findStep(forward) {
    const tab = this.activeTab();
    if (!this.findOpen || !this.findText || !tab) return this.openFind();
    tab.wc.findInPage(this.findText, { forward, findNext: false });
  }

  onFindResult(tab, result) {
    if (tab.id !== this.activeId) return;
    this.send('find-result', { tabId: tab.id, matches: result.matches, active: result.activeMatchOrdinal, final: result.finalUpdate });
  }

  closeFind({ notify = false, focusPage = false } = {}) {
    const tab = this.activeTab();
    if (tab && !tab.destroyed) tab.wc.stopFindInPage('keepSelection');
    this.findOpen = false;
    this.findText = '';
    if (notify) this.send('close-find', {});
    if (focusPage && tab && !tab.isHome()) tab.wc.focus();
  }

  // ---- zoom -------------------------------------------------------------------------

  zoom(tab, direction) {
    tab = tab || this.activeTab();
    if (!tab) return;
    const current = tab.wc.getZoomFactor();
    let next = 1;
    if (direction === 'in') next = ZOOM_STEPS.find((z) => z > current + 0.001) || ZOOM_STEPS[ZOOM_STEPS.length - 1];
    else if (direction === 'out') next = [...ZOOM_STEPS].reverse().find((z) => z < current - 0.001) || ZOOM_STEPS[0];
    tab.wc.setZoomFactor(next);
    // Zoom is per site in Chromium: refresh every window's state.
    this.browser.forEachController((c) => c.scheduleUpdate());
  }

  toggleDevTools(tab) {
    tab = tab || this.activeTab();
    if (!tab) return;
    if (tab.wc.isDevToolsOpened()) tab.wc.closeDevTools();
    else tab.wc.openDevTools({ mode: 'detach' });
  }
}

module.exports = { WindowController, themeBackground };
