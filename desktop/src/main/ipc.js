'use strict';

// IPC from the chrome UI. Every message is checked: it must come from the main frame of a
// window's own chrome webContents showing the bundled chrome page, name a known command
// and carry a valid argument (see commands.js). Tabs have no preload and cannot reach this.

const { ipcMain } = require('electron');
const { fileURLToPath } = require('node:url');
const path = require('node:path');
const registry = require('./registry');
const { runCommand } = require('./commands');

const CHROME_FILE = path.join(__dirname, '..', 'renderer', 'index.html');
const caseInsensitive = process.platform === 'win32' || process.platform === 'darwin';

function isChromePage(url) {
  if (typeof url !== 'string' || !url.startsWith('file:')) return false;
  try {
    const file = path.normalize(fileURLToPath(url.split(/[?#]/)[0]));
    return caseInsensitive ? file.toLowerCase() === CHROME_FILE.toLowerCase() : file === CHROME_FILE;
  } catch (_) {
    return false;
  }
}

function controllerFor(event) {
  const ctrl = registry.controllerForChrome(event.sender);
  if (!ctrl) return null;
  const frame = event.senderFrame;
  if (!frame || frame !== event.sender.mainFrame) return null;
  if (!isChromePage(frame.url)) return null;
  return ctrl;
}

function registerIpc() {
  ipcMain.on('opensurf:command', (event, name, arg) => {
    const ctrl = controllerFor(event);
    if (!ctrl) return;
    try {
      const result = runCommand(ctrl, name, arg);
      if (result.ok && result.value && typeof result.value.catch === 'function') result.value.catch(() => {});
    } catch (err) {
      console.error(`OpenSurf: command ${String(name).slice(0, 40)} failed:`, err);
    }
  });

  ipcMain.handle('opensurf:invoke', async (event, name, arg) => {
    const ctrl = controllerFor(event);
    if (!ctrl) throw new Error('Rejected');
    const result = runCommand(ctrl, name, arg, { invoke: true });
    if (!result.ok) throw new Error('Invalid request');
    return result.value;
  });
}

module.exports = { registerIpc };
