'use strict';

// Preload for the chrome UI only (tabs have no preload). Exposes a small, typed API;
// ipcRenderer itself is never exposed. The main process validates everything again.

const { contextBridge, ipcRenderer } = require('electron');

const send = (name, arg) => ipcRenderer.send('opensurf:command', name, arg);
const invoke = (name, arg) => ipcRenderer.invoke('opensurf:invoke', name, arg);

const EVENTS = new Set([
  'init', 'state', 'settings', 'downloads', 'download-started', 'find-result', 'open-find',
  'close-find', 'focus-omnibox', 'open-popover', 'toast',
]);
let listener = null;

ipcRenderer.on('opensurf:event', (_event, type, payload) => {
  if (listener && EVENTS.has(type)) listener(type, payload);
});

const id = (v) => (Number.isInteger(v) ? v : -1);
const text = (v, max) => String(v == null ? '' : v).slice(0, max);

contextBridge.exposeInMainWorld('opensurf', {
  /** Registers the single event callback, then tells the main process the UI is ready. */
  connect(callback) {
    if (typeof callback !== 'function' || listener) return;
    listener = callback;
    send('chrome.ready');
  },

  // layout & popovers
  setChromeHeight: (h) => send('chrome.height', Math.max(0, Math.round(Number(h) || 0))),
  capturePage: () => invoke('overlay.capture'),
  setOverlay: (on) => send('overlay.set', on === true),

  /** A shortcut key pressed in the chrome UI (the main process decides what it does). */
  shortcutKey: (e) => {
    const o = e && typeof e === 'object' ? e : {};
    send('chrome.key', {
      key: text(o.key, 32), code: text(o.code, 32),
      shift: o.shift === true, control: o.control === true, alt: o.alt === true, meta: o.meta === true,
    });
  },

  // tabs
  newTab: () => send('tab.new'),
  closeTab: (tabId) => send('tab.close', tabId === undefined ? null : id(tabId)),
  activateTab: (tabId) => send('tab.activate', id(tabId)),
  moveTab: (tabId, index) => send('tab.move', { id: id(tabId), index: id(index) }),
  toggleMute: (tabId) => send('tab.mute', id(tabId)),
  showTabMenu: (tabId) => send('tab.menu', id(tabId)),
  showTabList: () => send('tab.list'),
  reopenClosedTab: () => send('tab.reopen'),

  // navigation
  navigate: (input) => send('nav.go', text(input, 8192)),
  back: () => send('nav.back'),
  forward: () => send('nav.forward'),
  reload: (hard) => send(hard === true ? 'nav.hardReload' : 'nav.reload'),
  stop: () => send('nav.stop'),
  home: () => send('nav.home'),
  retry: (tabId) => send('nav.retry', id(tabId)),
  retryCertificate: (tabId) => send('nav.certRetry', id(tabId)),
  focusPage: () => send('page.focus'),
  copyUrl: () => send('page.copyUrl'),

  // find in page
  find: (query, options) => {
    const o = options && typeof options === 'object' ? options : {};
    send('find.query', { text: text(query, 1000), forward: o.forward !== false, findNext: o.findNext === true });
  },
  closeFind: (focusPage) => send('find.close', { focusPage: focusPage === true }),

  // zoom, windows, tools
  zoomIn: () => send('zoom.in'),
  zoomOut: () => send('zoom.out'),
  zoomReset: () => send('zoom.reset'),
  newWindow: () => send('window.new'),
  toggleFullscreen: () => send('window.fullscreen'),
  toggleDevTools: () => send('devtools.toggle'),

  // downloads
  cancelDownload: (dlId) => send('downloads.cancel', id(dlId)),
  pauseDownload: (dlId) => send('downloads.pause', id(dlId)),
  openDownload: (dlId) => send('downloads.open', id(dlId)),
  showDownload: (dlId) => send('downloads.show', id(dlId)),
  clearDownloads: () => send('downloads.clear'),
  openDownloadsFolder: () => send('downloads.openFolder'),

  // settings & data
  updateSettings: (patch) => {
    const p = patch && typeof patch === 'object' ? patch : {};
    const clean = {};
    for (const key of ['engine', 'customTemplate']) if (key in p) clean[key] = text(p[key], 2048);
    for (const key of ['safeSearch', 'javascript', 'restoreTabs']) if (key in p) clean[key] = p[key] === true;
    return invoke('settings.update', clean);
  },
  applyJavaScriptToOpenTabs: () => send('tabs.applyJavaScript'),
  clearBrowsingData: () => invoke('data.clear'),
  appInfo: () => invoke('app.info'),
});
