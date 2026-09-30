'use strict';

// Every user-facing action, keyed by name. Used by IPC (chrome UI), keyboard shortcuts
// and menus. Each entry validates its argument before running.

const { app, clipboard } = require('electron');
const security = require('./security');
const { hostOf } = require('./urls');

const INVALID = Symbol('invalid');

// ---- argument validators: return the normalised value or INVALID ------------------------

const none = (a) => (a === undefined || a === null ? undefined : INVALID);
const tabId = (a, ctrl) => (Number.isInteger(a) && ctrl.getTab(a) ? a : INVALID);
const optTabId = (a, ctrl) => (a === undefined || a === null ? null : tabId(a, ctrl));
const bool = (a) => (typeof a === 'boolean' ? a : INVALID);
const int = (min, max) => (a) => (Number.isInteger(a) && a >= min && a <= max ? a : INVALID);
const str = (min, max) => (a) => (typeof a === 'string' && a.length >= min && a.length <= max ? a : INVALID);
const downloadId = int(1, Number.MAX_SAFE_INTEGER);

function plainObject(a) {
  return a !== null && typeof a === 'object' && !Array.isArray(a) && Object.getPrototypeOf(a) === Object.prototype;
}

const findQuery = (a) => {
  if (!plainObject(a) || typeof a.text !== 'string' || a.text.length > 1000) return INVALID;
  return { text: a.text, forward: a.forward !== false, findNext: a.findNext === true };
};

const findClose = (a) => {
  if (a === undefined || a === null) return { focusPage: false };
  return plainObject(a) ? { focusPage: a.focusPage === true } : INVALID;
};

const tabMove = (a, ctrl) => {
  if (!plainObject(a) || tabId(a.id, ctrl) === INVALID || !Number.isInteger(a.index) || a.index < 0 || a.index > 10000) return INVALID;
  return { id: a.id, index: a.index };
};

const keyInput = (a) => {
  if (!plainObject(a) || typeof a.key !== 'string' || a.key.length > 32 || typeof a.code !== 'string' || a.code.length > 32) return INVALID;
  return { type: 'keyDown', key: a.key, code: a.code, shift: a.shift === true, control: a.control === true, alt: a.alt === true, meta: a.meta === true };
};

const settingsPatch = (a) => {
  if (!plainObject(a)) return INVALID;
  const keys = Object.keys(a);
  return keys.length > 0 && keys.length <= 10 ? { ...a } : INVALID;
};

const activeTab = (ctrl) => ctrl.activeTab();

function openPopover(ctrl, name) {
  ctrl.win.webContents.focus();
  ctrl.send('open-popover', { name });
}

// ---- the table ------------------------------------------------------------------------------

/** @type {Record<string, {arg?: Function, invoke?: boolean, run: Function}>} */
const COMMANDS = Object.assign(Object.create(null), {
  // chrome UI plumbing
  'chrome.ready': { run: (c) => c.onChromeReady() },
  'chrome.height': { arg: int(0, 2000), run: (c, h) => c.setChromeHeight(h) },
  'overlay.capture': { invoke: true, run: (c) => c.captureActivePage() },
  'overlay.set': { arg: bool, run: (c, on) => c.setOverlay(on) },
  // Shortcut keys that reached the chrome UI's DOM (real key presses are normally consumed
  // earlier by before-input-event; synthetic ones, e.g. from automation, arrive here).
  'chrome.key': { arg: keyInput, run: (c, input) => require('./keyboard').handleInput(c, input, 'chrome') },

  // tabs
  'tab.new': { run: (c) => c.newTab() },
  'tab.close': { arg: optTabId, run: (c, id) => c.closeTab(id === null ? c.activeId : id) },
  'tab.activate': { arg: tabId, run: (c, id) => c.activate(id) },
  'tab.next': { run: (c) => c.selectRelativeTab(1) },
  'tab.prev': { run: (c) => c.selectRelativeTab(-1) },
  'tab.select': { arg: int(-1, 1000), run: (c, n) => c.selectTabIndex(n) },
  'tab.reopen': { run: (c) => c.browser.reopenClosedTab(c) },
  'tab.duplicate': { arg: optTabId, run: (c, id) => c.duplicateTab(id === null ? c.activeId : id) },
  'tab.move': { arg: tabMove, run: (c, { id, index }) => c.moveTab(id, index) },
  'tab.mute': {
    arg: tabId,
    run: (c, id) => {
      const tab = c.getTab(id);
      tab.wc.setAudioMuted(!tab.wc.isAudioMuted());
      c.scheduleUpdate();
    },
  },
  'tab.menu': { arg: tabId, run: (c, id) => require('./context-menu').showTabMenu(c, c.getTab(id)) },
  'tab.list': { run: (c) => require('./context-menu').showTabListMenu(c) },

  // navigation
  'nav.go': { arg: str(1, 8192), run: (c, text) => c.navigate(text) },
  'nav.back': { run: (c) => activeTab(c) && activeTab(c).goBack() },
  'nav.forward': { run: (c) => activeTab(c) && activeTab(c).goForward() },
  'nav.reload': { run: (c) => activeTab(c) && activeTab(c).reload(false) },
  'nav.hardReload': { run: (c) => activeTab(c) && activeTab(c).reload(true) },
  'nav.stop': { run: (c) => activeTab(c) && activeTab(c).stop() },
  'nav.home': { run: (c) => c.goHome() },
  'nav.retry': { arg: tabId, run: (c, id) => c.getTab(id).reload(false) },
  'nav.certRetry': {
    arg: tabId,
    run: (c, id) => {
      const tab = c.getTab(id);
      if (!tab.error || !tab.error.url) return;
      security.forgetCertificateDenials(hostOf(tab.error.url));
      tab.load(tab.error.url);
    },
  },

  // find in page
  'find.open': { run: (c) => c.openFind() },
  'find.query': { arg: findQuery, run: (c, q) => c.find(q.text, q) },
  'find.close': { arg: findClose, run: (c, o) => c.closeFind(o) },
  'find.step': { arg: bool, run: (c, forward) => c.findStep(forward) },

  // zoom, window, devtools
  'zoom.in': { run: (c) => c.zoom(null, 'in') },
  'zoom.out': { run: (c) => c.zoom(null, 'out') },
  'zoom.reset': { run: (c) => c.zoom(null, 'reset') },
  'window.new': { run: (c) => c.browser.createWindow() },
  'window.close': { run: (c) => c.win.close() },
  'window.fullscreen': { run: (c) => c.toggleFullscreen() },
  'devtools.toggle': { run: (c) => c.toggleDevTools() },

  // page / focus
  'page.copyUrl': {
    run: (c) => {
      const tab = activeTab(c);
      const url = tab && !tab.isHome() ? tab.url() : '';
      if (!url) return;
      clipboard.writeText(url);
      c.send('toast', { message: 'Link copied' });
    },
  },
  'page.focus': {
    run: (c) => {
      const tab = activeTab(c);
      if (tab && !c.overlay) tab.wc.focus();
    },
  },
  'omnibox.focus': { run: (c) => c.focusOmnibox(true) },

  // popovers in the chrome UI (triggered by shortcuts / menus)
  'ui.settings': { run: (c) => openPopover(c, 'settings') },
  'ui.downloads': { run: (c) => openPopover(c, 'downloads') },
  'ui.about': { run: (c) => openPopover(c, 'about') },
  'ui.clearData': { run: (c) => openPopover(c, 'clear-data') },
  'ui.menu': { run: (c) => openPopover(c, 'menu') },

  // downloads
  'downloads.cancel': { arg: downloadId, run: (c, id) => c.browser.downloads.cancel(id) },
  'downloads.pause': { arg: downloadId, run: (c, id) => c.browser.downloads.togglePause(id) },
  'downloads.open': { arg: downloadId, run: (c, id) => c.browser.downloads.open(id) },
  'downloads.show': { arg: downloadId, run: (c, id) => c.browser.downloads.showInFolder(id) },
  'downloads.clear': { run: (c) => c.browser.downloads.clearFinished() },
  'downloads.openFolder': { run: (c) => c.browser.downloads.openFolder() },

  // settings & data
  'settings.update': { invoke: true, arg: settingsPatch, run: (c, patch) => c.browser.settings.update(patch) },
  'tabs.applyJavaScript': { run: (c) => c.browser.recreateAllTabs() },
  'data.clear': { invoke: true, run: (c) => c.browser.clearBrowsingData() },
  'app.info': {
    invoke: true,
    run: () => ({
      name: 'OpenSurf',
      version: app.getVersion(),
      electron: process.versions.electron,
      chrome: process.versions.chrome,
      platform: process.platform,
    }),
  },
});

/**
 * Validates and runs a command.
 * @returns {{ok: true, value: any} | {ok: false}}
 */
function runCommand(ctrl, name, arg, { invoke = false } = {}) {
  if (typeof name !== 'string' || !ctrl || ctrl.destroyed) return { ok: false };
  const cmd = COMMANDS[name];
  if (!cmd || Boolean(cmd.invoke) !== invoke) return { ok: false };
  const value = (cmd.arg || none)(arg, ctrl);
  if (value === INVALID) return { ok: false };
  return { ok: true, value: cmd.run(ctrl, value) };
}

/** Runs a command from a trusted main-process source (shortcut or menu). */
function exec(ctrl, name, arg) {
  if (!ctrl || ctrl.destroyed) return;
  const cmd = COMMANDS[name];
  if (!cmd) return;
  const value = (cmd.arg || none)(arg, ctrl);
  if (value === INVALID) return;
  Promise.resolve()
    .then(() => cmd.run(ctrl, value))
    .catch((err) => console.error(`OpenSurf: command ${name} failed:`, err));
}

module.exports = { COMMANDS, runCommand, exec, INVALID };
