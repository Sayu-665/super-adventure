// End-to-end behaviour checks (run with the smoke test: xvfb-run -a npm run test:smoke).
// Everything is served from local HTTP(S) servers; any other request is cancelled.
// Native dialogs and shell.openExternal are stubbed in the main process.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import https from 'node:https';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { _electron as electron } from 'playwright';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MOD = process.platform === 'darwin' ? 'Meta' : 'Control';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitFor(fn, message, timeout = 20000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (_) { /* retry */ }
    await sleep(100);
  }
  throw new Error(`Timed out waiting for: ${message}`);
}

const PAGE = (body, title = 'Test page') =>
  `<!doctype html><html><head><meta charset="utf-8"><title>${title}</title></head><body>${body}</body></html>`;

function startHttpServer() {
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    if (url.pathname === '/file.bin') {
      res.writeHead(200, { 'content-type': 'application/octet-stream', 'content-disposition': 'attachment; filename="report.pdf"' });
      return res.end(Buffer.alloc(2048, 1));
    }
    if (url.pathname === '/cookie') {
      res.writeHead(200, { 'content-type': 'text/html', 'set-cookie': 'session=abc; Path=/' });
      return res.end(PAGE('cookie set', 'Cookie'));
    }
    if (url.pathname === '/redirect-to-file') {
      res.writeHead(302, { location: 'file:///etc/hostname' });
      return res.end();
    }
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end(PAGE(`<h1 id="h">Hello</h1><p>surf surf surf</p>
      <a id="blank" href="/popup" target="_blank">popup</a>
      <a id="mail" href="mailto:someone@example.com">mail</a>
      <a id="dl" href="/file.bin">download</a>`, url.pathname === '/popup' ? 'Popup' : 'Test page'));
  });
  return new Promise((r) => server.listen(0, '127.0.0.1', () => r(server)));
}

/** Self-signed HTTPS server, or null when openssl is not available. */
function startHttpsServer(dir) {
  try {
    execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', path.join(dir, 'key.pem'), '-out', path.join(dir, 'cert.pem'),
      '-days', '2', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1'], { stdio: 'ignore' });
  } catch (_) {
    return Promise.resolve(null);
  }
  const server = https.createServer({ key: fs.readFileSync(path.join(dir, 'key.pem')), cert: fs.readFileSync(path.join(dir, 'cert.pem')) }, (req, res) => {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end(PAGE('<h1>secure-ish</h1>', 'Self-signed'));
  });
  return new Promise((r) => server.listen(0, '127.0.0.1', () => r(server)));
}

async function launch(userData, downloads) {
  const app = await electron.launch({
    args: ['--no-sandbox', '.'],
    cwd: root,
    env: { ...process.env, OPENSURF_USER_DATA_DIR: userData, OPENSURF_TEST: '1', NO_PROXY: '127.0.0.1,localhost', no_proxy: '127.0.0.1,localhost' },
  });
  await app.evaluate(({ app: a, session, dialog, shell }, dir) => {
    a.setPath('downloads', dir);
    globalThis.__requests = [];
    globalThis.__dialogs = [];
    globalThis.__answers = [];
    globalThis.__external = [];
    session.defaultSession.webRequest.onBeforeRequest({ urls: ['http://*/*', 'https://*/*'] }, (d, cb) => {
      globalThis.__requests.push({ url: d.url, type: d.resourceType });
      const host = new URL(d.url).hostname;
      cb({ cancel: host !== '127.0.0.1' && host !== 'localhost' });
    });
    const answer = (opts) => {
      globalThis.__dialogs.push(opts.message);
      return { response: globalThis.__answers.length ? globalThis.__answers.shift() : 0, checkboxChecked: false };
    };
    dialog.showMessageBox = async (a1, a2) => answer(a2 || a1);
    dialog.showMessageBoxSync = (a1, a2) => answer(a2 || a1).response;
    shell.openExternal = async (url) => { globalThis.__external.push(url); };
  }, downloads);
  const chrome = await waitFor(() => app.windows().find((p) => p.url().endsWith('/src/renderer/index.html')), 'chrome UI');
  await chrome.waitForSelector('#tabs .tab');
  const api = {
    app,
    chrome,
    state: () => app.evaluate(() => globalThis.__opensurfTest.state()[0]),
    main: (fn, arg) => app.evaluate(fn, arg),
    page: (pred) => waitFor(() => app.windows().find((p) => pred(p.url())), 'page').catch(async (err) => {
      console.error('pages:', app.windows().map((p) => p.url()), 'state:', JSON.stringify(await api.state()));
      throw err;
    }),
    /** Types into the omnibox and waits until the active tab requested `expected`. */
    async go(text, expected = text) {
      await chrome.click('#omnibox');
      await chrome.keyboard.press(`${MOD}+a`);
      await chrome.keyboard.type(text);
      await chrome.keyboard.press('Enter');
      await waitFor(async () => (await api.active()).requestedUrl === expected, `omnibox navigation to ${expected}`);
    },
    active: async () => {
      const s = await api.state();
      return s.tabs.find((t) => t.id === s.activeId);
    },
  };
  return api;
}

test('OpenSurf behaviour: navigation guards, popups, downloads, certificates, permissions, settings, restore', { timeout: 240000 }, async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'opensurf-behaviour-'));
  const userData = path.join(tmp, 'profile');
  const downloads = path.join(tmp, 'downloads');
  fs.mkdirSync(downloads);
  const server = await startHttpServer();
  const secure = await startHttpsServer(tmp);
  const base = `http://127.0.0.1:${server.address().port}`;
  let b = await launch(userData, downloads);
  try {
    // Load a local page from the omnibox.
    await b.go(`${base}/`);
    await waitFor(async () => (await b.active()).committedUrl === `${base}/`, 'local page');
    const page = await b.page((u) => u === `${base}/`);
    await page.waitForSelector('#h');
    assert.equal(await page.evaluate(() => typeof window.require + typeof window.process + typeof window.opensurf), 'undefinedundefinedundefined');

    // target=_blank opens a new foreground tab.
    await page.click('#blank');
    await waitFor(async () => (await b.state()).tabs.length === 2, 'popup tab');
    let s = await b.state();
    assert.equal(s.tabs[1].requestedUrl, `${base}/popup`);
    assert.equal(s.activeId, s.tabs[1].id);
    await b.chrome.keyboard.press(`${MOD}+1`);
    await waitFor(async () => { const st = await b.state(); return st.activeId === st.tabs[0].id; }, 'Ctrl+1 selects the first tab');
    await b.chrome.keyboard.press('Control+Tab');
    await waitFor(async () => { const st = await b.state(); return st.activeId === st.tabs[1].id; }, 'Ctrl+Tab selects the next tab');
    await b.chrome.keyboard.press(`${MOD}+w`);
    await waitFor(async () => (await b.state()).tabs.length === 1, 'popup closed');

    // window.open from script also becomes a tab; javascript: URLs never run.
    await page.evaluate((u) => window.open(u), `${base}/popup2`);
    await waitFor(async () => (await b.state()).tabs.length === 2, 'window.open tab');
    await b.chrome.keyboard.press(`${MOD}+w`);
    await waitFor(async () => (await b.state()).tabs.length === 1, 'window.open tab closed');

    // Web pages cannot navigate to file:, even through a redirect.
    await page.evaluate(() => { location.href = 'file:///etc/hostname'; });
    await sleep(800);
    assert.equal((await b.active()).committedUrl, `${base}/`);
    await page.evaluate((u) => { location.href = u; }, `${base}/redirect-to-file`);
    await sleep(800);
    assert.ok(!(await b.active()).committedUrl.startsWith('file:'), 'redirect to file: is blocked');

    // Pages can use opensurf://go (same resolution as the omnibox, never file:).
    await b.go(`${base}/`);
    await waitFor(async () => (await b.active()).committedUrl === `${base}/`, 'back on the local page');
    const page2 = await b.page((u) => u === `${base}/`);
    await page2.evaluate(() => { location.href = 'opensurf://go?q=file%3A%2F%2F%2Fetc%2Fpasswd'; });
    await waitFor(async () => (await b.active()).requestedUrl === 'https://duckduckgo.com/?q=file%3A%2F%2F%2Fetc%2Fpasswd&kp=-2', 'go URL resolved as a search');

    // The home page form goes through opensurf://go and the same resolver.
    await b.chrome.click('#btn-home');
    const home = await b.page((u) => u.startsWith('opensurf://home/'));
    await home.waitForSelector('input[name="q"]');
    await home.fill('input[name="q"]', 'example.com');
    await home.press('input[name="q"]', 'Enter');
    await waitFor(async () => (await b.active()).requestedUrl === 'https://example.com', 'home form -> https://example.com');

    // mailto: asks first, then goes to the OS.
    await b.go(`${base}/`);
    const page3 = await b.page((u) => u === `${base}/`);
    await page3.waitForSelector('#mail');
    await b.main(() => { globalThis.__answers.push(1); });
    await page3.evaluate(() => document.getElementById('mail').click()); // no navigation to wait for
    await waitFor(async () => (await b.main(() => globalThis.__external)).includes('mailto:someone@example.com'), 'mailto handed to the OS');

    // Permission prompt (Block by default), remembered per origin.
    await b.main(() => { globalThis.__answers.push(1); });
    assert.equal(await page3.evaluate(() => Notification.requestPermission()), 'granted');
    const promptsBefore = (await b.main(() => globalThis.__dialogs)).length;
    assert.equal(await page3.evaluate(() => Notification.requestPermission()), 'granted');
    assert.equal((await b.main(() => globalThis.__dialogs)).length, promptsBefore, 'decision remembered');

    // Downloads go to the Downloads folder, without clobbering.
    const clickDownload = () => page3.evaluate(() => document.getElementById('dl').click());
    await clickDownload();
    await waitFor(() => fs.readdirSync(downloads).includes('report.pdf'), 'first download');
    await clickDownload();
    await waitFor(() => fs.readdirSync(downloads).includes('report (1).pdf'), 'second download gets a unique name');
    await b.chrome.click('#btn-downloads');
    await b.chrome.waitForSelector('#downloads-popover:not([hidden]) .dl-item');
    assert.equal(await b.chrome.locator('.dl-item').count(), 2);
    await b.chrome.keyboard.press('Escape');

    // Find in page (Ctrl+F) with match count.
    await b.chrome.keyboard.press(`${MOD}+f`);
    await b.chrome.waitForSelector('#findbar:not([hidden])');
    await b.chrome.keyboard.type('surf');
    await waitFor(async () => (await b.chrome.textContent('#find-count')) === '1/3', 'find count 1/3');
    await b.chrome.keyboard.press('Enter');
    await waitFor(async () => (await b.chrome.textContent('#find-count')) === '2/3', 'find next');
    await b.chrome.keyboard.press('Escape');
    await b.chrome.waitForSelector('#findbar', { state: 'hidden' });

    // Certificate errors: Cancel is the default; "Proceed anyway" asks again.
    if (secure) {
      const secureUrl = `https://127.0.0.1:${secure.address().port}/`;
      await b.go(secureUrl);
      await b.chrome.waitForSelector('#error-panel:not([hidden])');
      assert.match(await b.chrome.textContent('#error-title'), /not private/);
      assert.ok(await b.chrome.isVisible('#error-proceed'));
      await b.main(() => { globalThis.__answers.push(1); });
      await b.chrome.click('#error-proceed');
      await waitFor(async () => (await b.active()).committedUrl === secureUrl, 'proceeded to the self-signed site');
      await b.chrome.waitForSelector('#error-panel', { state: 'hidden' });
    }

    // Cookies + Clear browsing data.
    await b.go(`${base}/cookie`);
    const cookiePage = await b.page((u) => u === `${base}/cookie`);
    await waitFor(async () => (await cookiePage.evaluate(() => document.cookie)).includes('session=abc'), 'cookie set');
    await b.chrome.click('#btn-menu');
    await b.chrome.click('#menu-popover [data-action="clear-data"]');
    await b.chrome.click('#clear-confirm');
    await waitFor(async () => (await b.chrome.textContent('#clear-status')).includes('cleared'), 'data cleared');
    const cookies = await b.main(({ session }) => session.defaultSession.cookies.get({}));
    assert.equal(cookies.length, 0);
    await b.chrome.waitForSelector('#clear-dialog', { state: 'hidden' });

    // JavaScript off: new tabs get it immediately, open tabs after "Apply".
    await b.main(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].webContents.focus());
    await b.chrome.keyboard.press(`${MOD}+,`);
    await b.chrome.waitForSelector('#settings-dialog:not([hidden])');
    await b.chrome.click('#set-js');
    await waitFor(async () => (await b.main(() => globalThis.__opensurfTest.settings())).javascript === false, 'JS setting saved');
    await b.chrome.waitForSelector('#js-apply-row:not([hidden])');
    await b.chrome.click('#js-apply');
    await waitFor(async () => (await b.state()).tabs.every((t) => t.javascript === false), 'open tabs rebuilt without JS');
    await b.chrome.click('#set-js');
    await waitFor(async () => (await b.main(() => globalThis.__opensurfTest.settings())).javascript === true, 'JS back on');
    await b.chrome.keyboard.press('Escape');
    await b.chrome.keyboard.press(`${MOD}+t`);
    await waitFor(async () => { const st = await b.state(); return st.tabs.length === 2 && st.tabs[1].javascript === true; }, 'new tab with JS on');

    // Restore tabs on the next launch.
    s = await b.state();
    const openUrls = s.tabs.map((t) => t.url.replace(/#.*$/, ''));
    await b.app.close();
    b = await launch(userData, downloads);
    await waitFor(async () => (await b.state()).tabs.length === openUrls.length, 'tabs restored');
    s = await b.state();
    assert.deepEqual(s.tabs.map((t) => t.requestedUrl.replace(/#.*$/, '')), openUrls);
  } finally {
    await b.app.close().catch(() => {});
    server.close();
    if (secure) secure.close();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
