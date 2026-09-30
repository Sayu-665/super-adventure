'use strict';

// Native context menus: web pages, the chrome UI's text fields, tabs and the tab list.

const { Menu, clipboard } = require('electron');
const { classifyUrl } = require('./urls');

function truncate(text, max) {
  const t = String(text).replace(/\s+/g, ' ').trim();
  return t.length > max ? `${t.slice(0, max - 1)}…` : t;
}

function compact(items) {
  // Drop leading/trailing/duplicate separators.
  const out = [];
  for (const item of items) {
    if (!item) continue;
    if (item.type === 'separator' && (!out.length || out[out.length - 1].type === 'separator')) continue;
    out.push(item);
  }
  while (out.length && out[out.length - 1].type === 'separator') out.pop();
  return out;
}

function popup(ctrl, template) {
  const items = compact(template);
  if (!items.length || ctrl.win.isDestroyed()) return;
  Menu.buildFromTemplate(items).popup({ window: ctrl.win });
}

function editItems(params, wc) {
  const f = params.editFlags || {};
  const items = [];
  if (params.isEditable && params.misspelledWord) {
    const suggestions = (params.dictionarySuggestions || []).slice(0, 5);
    for (const s of suggestions) items.push({ label: s, click: () => wc.replaceMisspelling(s) });
    if (!suggestions.length) items.push({ label: 'No spelling suggestions', enabled: false });
    items.push({ label: 'Add to dictionary', click: () => wc.session.addWordToSpellCheckerDictionary(params.misspelledWord) });
    items.push({ type: 'separator' });
  }
  if (params.isEditable) {
    items.push({ label: 'Undo', role: 'undo', enabled: f.canUndo });
    items.push({ label: 'Redo', role: 'redo', enabled: f.canRedo });
    items.push({ type: 'separator' });
    items.push({ label: 'Cut', role: 'cut', enabled: f.canCut });
  }
  if (params.isEditable || params.selectionText) items.push({ label: 'Copy', role: 'copy', enabled: f.canCopy });
  if (params.isEditable) items.push({ label: 'Paste', role: 'paste', enabled: f.canPaste });
  if (params.isEditable || !params.linkURL) items.push({ label: 'Select all', role: 'selectAll', enabled: f.canSelectAll !== false });
  return items;
}

/** Right-click inside a web page. */
function showPageMenu(tab, params) {
  const ctrl = tab.ctrl;
  const wc = tab.wc;
  const browser = ctrl.browser;
  const link = params.linkURL && classifyUrl(params.linkURL) === 'web' ? params.linkURL : '';
  const image = params.mediaType === 'image' && params.srcURL ? params.srcURL : '';
  const selection = (params.selectionText || '').trim();
  const template = [];

  if (link) {
    template.push(
      { label: 'Open link in new tab', click: () => ctrl.openTab(link, { openerId: tab.id, background: true }) },
      { label: 'Open link in new window', click: () => browser.createWindow({ urls: [link] }) },
      { label: 'Copy link address', click: () => clipboard.writeText(link) },
      { type: 'separator' },
    );
  } else if (params.linkURL && classifyUrl(params.linkURL) === 'external') {
    template.push({ label: 'Copy link address', click: () => clipboard.writeText(params.linkURL) }, { type: 'separator' });
  }

  if (image) {
    const openable = classifyUrl(image) === 'web';
    template.push(
      openable && { label: 'Open image in new tab', click: () => ctrl.openTab(image, { openerId: tab.id, background: true }) },
      { label: 'Save image', click: () => wc.downloadURL(image) },
      { label: 'Copy image', click: () => wc.copyImageAt(params.x, params.y) },
      openable && { label: 'Copy image address', click: () => clipboard.writeText(image) },
      { type: 'separator' },
    );
  }

  if (!link && !image && !selection && !params.isEditable) {
    template.push(
      { label: 'Back', enabled: wc.navigationHistory.canGoBack(), click: () => tab.goBack() },
      { label: 'Forward', enabled: wc.navigationHistory.canGoForward(), click: () => tab.goForward() },
      { label: 'Reload', click: () => tab.reload(false) },
      { type: 'separator' },
    );
  }

  template.push(...editItems(params, wc), { type: 'separator' });

  if (selection) {
    const engine = browser.engineName();
    template.push(
      { label: `Search ${engine} for “${truncate(selection, 32)}”`, click: () => ctrl.openTab(browser.searchUrl(selection.slice(0, 2000)), { openerId: tab.id }) },
      { type: 'separator' },
    );
  }

  if (!link && !image && !selection && !params.isEditable && !tab.isHome()) {
    template.push({ label: 'Copy page address', click: () => clipboard.writeText(tab.url()) }, { type: 'separator' });
  }

  template.push({
    label: 'Inspect element',
    click: () => {
      if (!wc.isDevToolsOpened()) wc.openDevTools({ mode: 'detach' });
      wc.inspectElement(params.x, params.y);
    },
  });
  popup(ctrl, template);
}

/** Right-click in the chrome UI: only text fields get a menu (plus "Paste and go" in the omnibox). */
function showChromeMenu(ctrl, params) {
  if (!params.isEditable) return;
  const template = editItems(params, ctrl.win.webContents);
  // The omnibox is the only type=search field in the chrome UI.
  const text = clipboard.readText().trim();
  if (text && params.formControlType === 'input-search') {
    template.push({ type: 'separator' }, { label: 'Paste and go', click: () => ctrl.navigate(text.slice(0, 8192)) });
  }
  popup(ctrl, template);
}

/** Right-click on a tab in the tab strip. */
function showTabMenu(ctrl, tab) {
  if (!tab) return;
  const muted = tab.wc.isAudioMuted();
  popup(ctrl, [
    { label: 'New tab', accelerator: 'CmdOrCtrl+T', click: () => ctrl.newTab() },
    { type: 'separator' },
    { label: 'Reload', click: () => tab.reload(false) },
    { label: 'Duplicate', click: () => ctrl.duplicateTab(tab.id) },
    { label: muted ? 'Unmute site' : 'Mute site', click: () => { tab.wc.setAudioMuted(!muted); ctrl.scheduleUpdate(); } },
    !tab.isHome() && { label: 'Copy link', click: () => clipboard.writeText(tab.url()) },
    { type: 'separator' },
    { label: 'Close tab', accelerator: 'CmdOrCtrl+W', click: () => ctrl.closeTab(tab.id) },
    { label: 'Close other tabs', enabled: ctrl.tabs.length > 1, click: () => ctrl.closeOtherTabs(tab.id) },
    { type: 'separator' },
    { label: 'Reopen closed tab', accelerator: 'CmdOrCtrl+Shift+T', enabled: ctrl.browser.closedTabs.length > 0, click: () => ctrl.browser.reopenClosedTab(ctrl) },
  ]);
}

/** The tab-count button: lists every open tab. */
function showTabListMenu(ctrl) {
  popup(ctrl, [
    ...ctrl.tabs.map((tab) => ({
      label: truncate(tab.displayTitle(), 60),
      type: 'radio',
      checked: tab.id === ctrl.activeId,
      click: () => ctrl.activate(tab.id),
    })),
    { type: 'separator' },
    { label: 'New tab', accelerator: 'CmdOrCtrl+T', click: () => ctrl.newTab() },
  ]);
}

module.exports = { showPageMenu, showChromeMenu, showTabMenu, showTabListMenu };
