'use strict';

// OpenSurf main process entry point: app lifecycle, single instance, URL hand-off.

const { app, nativeTheme } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

// Isolated profile (used by the tests). Must happen before 'ready'.
if (process.env.OPENSURF_USER_DATA_DIR) {
  app.setPath('userData', path.resolve(process.env.OPENSURF_USER_DATA_DIR));
}

const { registerSchemePrivileges } = require('./protocol');
const { registerIpc } = require('./ipc');
const { installAppMenu } = require('./app-menu');
const { Browser } = require('./browser');
const registry = require('./registry');

// Sandbox every renderer. The only exception is an explicit --no-sandbox switch (used solely by
// the test harness, which runs as root in a container): combined with enableSandbox() every
// child process would refuse to start. Renderers still use sandbox: true / no Node.js then.
if (!app.commandLine.hasSwitch('no-sandbox')) app.enableSandbox();
registerSchemePrivileges();

const browser = new Browser();
let ready = false;
const queuedUrls = [];

/** http(s)/file URLs and existing files from a command line. Switches and directories are ignored. */
function urlsFromArgv(argv, cwd = process.cwd()) {
  const urls = [];
  for (const arg of argv.slice(1)) {
    if (typeof arg !== 'string' || !arg || arg.startsWith('-')) continue;
    if (/^https?:\/\//i.test(arg) || /^file:\/\//i.test(arg)) {
      urls.push(arg);
      continue;
    }
    try {
      const file = path.resolve(cwd, arg);
      if (fs.statSync(file).isFile()) urls.push(pathToFileURL(file).href);
    } catch (_) { /* not a file: ignore */ }
  }
  return urls.slice(0, 20);
}

function handleUrls(urls) {
  if (!urls.length) return;
  if (ready) browser.openUrls(urls);
  else queuedUrls.push(...urls);
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', (_event, argv, workingDirectory) => {
    if (!ready) return;
    const urls = urlsFromArgv(argv, workingDirectory);
    if (urls.length) browser.openUrls(urls);
    else browser.focusWindow();
  });

  // macOS: links and files handed to the app (default browser, Finder "Open With").
  app.on('will-finish-launching', () => {
    app.on('open-url', (event, url) => {
      event.preventDefault();
      if (/^https?:\/\//i.test(url)) handleUrls([url]);
    });
    app.on('open-file', (event, file) => {
      event.preventDefault();
      handleUrls([pathToFileURL(file).href]);
    });
  });

  // No <webview> anywhere.
  app.on('web-contents-created', (_event, contents) => {
    contents.on('will-attach-webview', (e) => e.preventDefault());
  });

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit();
  });

  app.on('activate', () => {
    if (ready && registry.lastController() === null) browser.createWindow();
  });

  app.on('before-quit', () => browser.onBeforeQuit());

  app.whenReady().then(() => {
    nativeTheme.themeSource = 'system';
    if (process.platform === 'win32') app.setAppUserModelId('com.opensurf.browser');
    app.setAboutPanelOptions({
      applicationName: 'OpenSurf',
      applicationVersion: app.getVersion(),
      copyright: 'MIT License',
      credits: 'A simple, fast, private browser for unrestricted search. No telemetry.',
    });
    browser.init();
    registerIpc();
    installAppMenu(browser);
    ready = true;
    const urls = [...urlsFromArgv(process.argv), ...queuedUrls.splice(0)];
    browser.start(urls);

    if (process.env.OPENSURF_TEST === '1') {
      // Read-only hook for the smoke test (main process only; not reachable from any page).
      globalThis.__opensurfTest = {
        state: () =>
          browser.controllers().map((c) => ({
            activeId: c.activeId,
            chromeReady: c.chromeReady,
            title: c.win.getTitle(),
            tabs: c.tabs.map((t) => ({
              id: t.id,
              url: t.url(),
              committedUrl: t.wc.getURL(),
              requestedUrl: t.lastRequestedUrl,
              title: t.displayTitle(),
              javascript: t.jsEnabled(),
            })),
          })),
        settings: () => browser.settings.get(),
        /** PNG data URL of the active tab of the first window (for visual checks). */
        captureActiveTab: async () => {
          const ctrl = browser.controllers()[0];
          const tab = ctrl && ctrl.activeTab();
          if (!tab || tab.error || ctrl.overlay) return null;
          const image = await tab.wc.capturePage();
          return image.isEmpty() ? null : image.toDataURL();
        },
      };
    }
  });
}

module.exports = { urlsFromArgv };
