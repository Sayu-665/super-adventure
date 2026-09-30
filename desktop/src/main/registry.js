'use strict';

// Lookup tables shared by the main-process modules (avoids circular requires).

const { BrowserWindow } = require('electron');

const controllers = new Set(); // WindowController instances
const tabsByWebContentsId = new Map(); // webContents.id -> Tab

function tabForWebContents(wc) {
  return wc ? tabsByWebContentsId.get(wc.id) || null : null;
}

function controllerForChrome(wc) {
  if (!wc) return null;
  for (const ctrl of controllers) if (!ctrl.destroyed && ctrl.win.webContents === wc) return ctrl;
  return null;
}

/** The BrowserWindow hosting a webContents (chrome UI or tab), if any. */
function windowForWebContents(wc) {
  const tab = tabForWebContents(wc);
  if (tab && tab.ctrl && !tab.ctrl.destroyed) return tab.ctrl.win;
  const ctrl = controllerForChrome(wc);
  if (ctrl) return ctrl.win;
  try {
    const win = wc && BrowserWindow.fromWebContents(wc);
    if (win && !win.isDestroyed()) return win;
  } catch (_) { /* ignore */ }
  return null;
}

/** Most recently focused live controller. */
function lastController() {
  let best = null;
  for (const ctrl of controllers) {
    if (ctrl.destroyed) continue;
    if (!best || ctrl.lastFocused > best.lastFocused) best = ctrl;
  }
  return best;
}

module.exports = { controllers, tabsByWebContentsId, tabForWebContents, controllerForChrome, windowForWebContents, lastController };
