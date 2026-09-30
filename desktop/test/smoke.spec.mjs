// Smoke test: launches the real app with Playwright's Electron driver (run: xvfb-run -a npm run test:smoke).
// Network access is blocked for the whole run; assertions are on the URLs the app requests.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { _electron as electron } from 'playwright';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MOD = process.platform === 'darwin' ? 'Meta' : 'Control';

async function waitFor(fn, message, timeout = 20000) {
  const start = Date.now();
  let last;
  while (Date.now() - start < timeout) {
    try {
      last = await fn();
      if (last) return last;
    } catch (_) { /* retry */ }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`Timed out waiting for: ${message}`);
}

test('OpenSurf desktop smoke test', { timeout: 180000 }, async () => {
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'opensurf-smoke-'));
  const app = await electron.launch({
    args: ['--no-sandbox', '.'], // container runs as root: --no-sandbox is for tests only
    cwd: root,
    env: { ...process.env, OPENSURF_USER_DATA_DIR: userData, OPENSURF_TEST: '1' },
  });
  try {
    // No internet dependency: cancel every http(s) request and record what was asked for.
    await app.evaluate(({ session }) => {
      globalThis.__requests = [];
      session.defaultSession.webRequest.onBeforeRequest({ urls: ['http://*/*', 'https://*/*'] }, (details, callback) => {
        globalThis.__requests.push({ url: details.url, type: details.resourceType });
        callback({ cancel: true });
      });
    });
    const state = () => app.evaluate(() => globalThis.__opensurfTest.state());
    const settings = () => app.evaluate(() => globalThis.__opensurfTest.settings());
    const requests = () => app.evaluate(() => globalThis.__requests);

    // 1. Window + chrome UI + home tab
    const chrome = await waitFor(() => app.windows().find((p) => p.url().endsWith('/src/renderer/index.html')), 'chrome UI page');
    await chrome.waitForSelector('#tabs .tab');
    const title = await waitFor(async () => {
      const t = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].getTitle());
      return t.includes('OpenSurf') && t;
    }, 'window title');
    assert.match(title, /OpenSurf/);

    let s = await waitFor(async () => {
      const st = await state();
      return st.length === 1 && st[0].tabs.length === 1 && st[0].tabs[0].committedUrl.startsWith('opensurf://home/') && st;
    }, 'first tab shows opensurf://home/');
    assert.match(s[0].tabs[0].committedUrl, /^opensurf:\/\/home\/#engine=DuckDuckGo&safe=off$/);
    assert.equal(await chrome.inputValue('#omnibox'), '');
    assert.equal(await chrome.getAttribute('#omnibox', 'placeholder'), 'Search or type URL');

    // The home page is served from the bundle, shows the caption and has no privileged API.
    const home = await waitFor(() => app.windows().find((p) => p.url().startsWith('opensurf://home/')), 'home page');
    await home.waitForSelector('#caption');
    await waitFor(async () => (await home.textContent('#caption')).includes('SafeSearch off · DuckDuckGo'), 'home caption');
    const privileges = await home.evaluate(() => [typeof window.require, typeof window.process, typeof window.opensurf]);
    assert.deepEqual(privileges, ['undefined', 'undefined', 'undefined']);

    // 2. Omnibox search with the default engine (DuckDuckGo, SafeSearch off)
    await chrome.click('#omnibox');
    await chrome.keyboard.type('hello world');
    await chrome.keyboard.press('Enter');
    const ddg = ['https://duckduckgo.com/?q=hello%20world&kp=-2', 'https://duckduckgo.com/?q=hello+world&kp=-2'];
    await waitFor(async () => ddg.includes((await state())[0].tabs[0].requestedUrl), 'tab navigates to the DuckDuckGo search URL');
    await waitFor(async () => (await requests()).some((r) => ddg.includes(r.url) && r.type === 'mainFrame'), 'main-frame request to DuckDuckGo');

    // 3. Ctrl+T adds a tab, Ctrl+W removes it
    await chrome.keyboard.press(`${MOD}+t`);
    await waitFor(async () => (await state())[0].tabs.length === 2, 'second tab after Ctrl+T');
    await waitFor(async () => (await chrome.locator('#tabs .tab').count()) === 2, 'two tabs in the tab strip');
    assert.equal(await chrome.textContent('#tab-count-num'), '2');
    await chrome.keyboard.press(`${MOD}+w`);
    await waitFor(async () => (await state())[0].tabs.length === 1, 'one tab after Ctrl+W');
    await waitFor(async () => (await chrome.locator('#tabs .tab').count()) === 1, 'one tab in the tab strip');

    // 4. Switch the engine to Google in Settings; the next search goes to Google with safe=off
    await chrome.click('#btn-menu');
    await chrome.click('#menu-popover [data-action="settings"]');
    await chrome.waitForSelector('#settings-dialog:not([hidden])');
    assert.equal(await chrome.inputValue('#set-engine'), 'duckduckgo');
    assert.equal(await chrome.isChecked('#set-safe'), false, 'SafeSearch is off by default');
    await chrome.selectOption('#set-engine', 'google');
    await waitFor(async () => (await settings()).engine === 'google', 'engine setting saved');
    await chrome.click('#settings-dialog .dialog-foot [data-close]');
    await chrome.waitForSelector('#settings-dialog', { state: 'hidden' });
    const saved = JSON.parse(fs.readFileSync(path.join(userData, 'settings.json'), 'utf8'));
    assert.equal(saved.engine, 'google');
    assert.equal(saved.safeSearch, false);

    await chrome.click('#omnibox');
    await chrome.keyboard.press(`${MOD}+a`);
    await chrome.keyboard.type('opensurf test');
    await chrome.keyboard.press('Enter');
    const google = ['https://www.google.com/search?q=opensurf%20test&safe=off', 'https://www.google.com/search?q=opensurf+test&safe=off'];
    await waitFor(async () => google.includes((await state())[0].tabs[0].requestedUrl), 'tab navigates to the Google search URL');
    await waitFor(async () => (await requests()).some((r) => google.includes(r.url) && r.type === 'mainFrame'), 'main-frame request to Google');

    // 5. A new home page reflects the new settings
    await chrome.keyboard.press(`${MOD}+t`);
    s = await waitFor(async () => {
      const st = await state();
      return st[0].tabs.length === 2 && st[0].tabs[1].committedUrl.startsWith('opensurf://home/') && st;
    }, 'new home tab');
    assert.match(s[0].tabs[1].committedUrl, /#engine=Google&safe=off$/);
    await waitFor(async () => {
      const page = app.windows().find((p) => p.url().includes('#engine=Google'));
      return page && (await page.textContent('#caption')).includes('SafeSearch off · Google');
    }, 'home caption shows Google');
  } finally {
    await app.close();
    fs.rmSync(userData, { recursive: true, force: true });
  }
});
