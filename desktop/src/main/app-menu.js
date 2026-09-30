'use strict';

// Application menu. macOS gets the standard app/File/Edit/View/History/Window menus (so
// Cmd+C/V/Q etc. work); Windows and Linux use no menu bar, as the toolbar menu covers
// everything and shortcuts are handled in before-input-event.

const { app, Menu, BrowserWindow } = require('electron');
const registry = require('./registry');
const { exec } = require('./commands');

let browserRef = null;

function run(command, arg) {
  return (_item, win) => {
    const focused = win || BrowserWindow.getFocusedWindow();
    const ctrl = (focused && registry.controllerForChrome(focused.webContents)) || registry.lastController();
    if (ctrl) exec(ctrl, command, arg);
    else if (browserRef && (command === 'tab.new' || command === 'window.new')) browserRef.createWindow();
  };
}

// Accelerators are informational here: before-input-event handles the keys first.
function macTemplate() {
  return [
    {
      label: app.name,
      submenu: [
        { label: 'About OpenSurf', click: run('ui.about') },
        { type: 'separator' },
        { label: 'Settings…', accelerator: 'Cmd+,', click: run('ui.settings') },
        { type: 'separator' },
        { role: 'services' },
        { type: 'separator' },
        { role: 'hide' },
        { role: 'hideOthers' },
        { role: 'unhide' },
        { type: 'separator' },
        { role: 'quit' },
      ],
    },
    {
      label: 'File',
      submenu: [
        { label: 'New Tab', accelerator: 'Cmd+T', click: run('tab.new') },
        { label: 'New Window', accelerator: 'Cmd+N', click: run('window.new') },
        { label: 'Reopen Closed Tab', accelerator: 'Cmd+Shift+T', click: run('tab.reopen') },
        { label: 'Open Location…', accelerator: 'Cmd+L', click: run('omnibox.focus') },
        { type: 'separator' },
        { label: 'Close Tab', accelerator: 'Cmd+W', click: run('tab.close') },
        { label: 'Close Window', accelerator: 'Cmd+Shift+W', click: run('window.close') },
        { type: 'separator' },
        { label: 'Copy Page Link', click: run('page.copyUrl') },
      ],
    },
    {
      label: 'Edit',
      submenu: [
        { role: 'undo' },
        { role: 'redo' },
        { type: 'separator' },
        { role: 'cut' },
        { role: 'copy' },
        { role: 'paste' },
        { role: 'pasteAndMatchStyle' },
        { role: 'delete' },
        { role: 'selectAll' },
        { type: 'separator' },
        { label: 'Find…', accelerator: 'Cmd+F', click: run('find.open') },
        { label: 'Find Next', accelerator: 'Cmd+G', click: run('find.step', true) },
        { label: 'Find Previous', accelerator: 'Cmd+Shift+G', click: run('find.step', false) },
      ],
    },
    {
      label: 'View',
      submenu: [
        { label: 'Reload', accelerator: 'Cmd+R', click: run('nav.reload') },
        { label: 'Hard Reload', accelerator: 'Cmd+Shift+R', click: run('nav.hardReload') },
        { label: 'Stop', click: run('nav.stop') },
        { type: 'separator' },
        { label: 'Zoom In', accelerator: 'Cmd+Plus', click: run('zoom.in') },
        { label: 'Zoom Out', accelerator: 'Cmd+-', click: run('zoom.out') },
        { label: 'Actual Size', accelerator: 'Cmd+0', click: run('zoom.reset') },
        { type: 'separator' },
        { label: 'Toggle Full Screen', accelerator: 'Ctrl+Cmd+F', click: run('window.fullscreen') },
        { label: 'Downloads', accelerator: 'Cmd+J', click: run('ui.downloads') },
        { label: 'Developer Tools', accelerator: 'Alt+Cmd+I', click: run('devtools.toggle') },
      ],
    },
    {
      label: 'History',
      submenu: [
        { label: 'Back', accelerator: 'Cmd+[', click: run('nav.back') },
        { label: 'Forward', accelerator: 'Cmd+]', click: run('nav.forward') },
        { label: 'Home', accelerator: 'Cmd+Shift+H', click: run('nav.home') },
        { type: 'separator' },
        { label: 'Clear Browsing Data…', accelerator: 'Cmd+Shift+Backspace', click: run('ui.clearData') },
      ],
    },
    {
      role: 'windowMenu',
      submenu: [
        { role: 'minimize' },
        { role: 'zoom' },
        { type: 'separator' },
        { label: 'Next Tab', accelerator: 'Ctrl+Tab', click: run('tab.next') },
        { label: 'Previous Tab', accelerator: 'Ctrl+Shift+Tab', click: run('tab.prev') },
        { type: 'separator' },
        { role: 'front' },
      ],
    },
    { role: 'help', submenu: [{ label: 'About OpenSurf', click: run('ui.about') }] },
  ];
}

/** @param {import('./browser').Browser} browser */
function installAppMenu(browser) {
  browserRef = browser;
  if (process.platform === 'darwin') Menu.setApplicationMenu(Menu.buildFromTemplate(macTemplate()));
  else Menu.setApplicationMenu(null);
}

module.exports = { installAppMenu };
