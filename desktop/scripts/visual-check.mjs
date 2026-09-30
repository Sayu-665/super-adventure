// Visual check: drives the app (dev tree, or the packaged Linux build with --packaged), takes
// full-window screenshots in light and dark mode into test-output/, and fails on errors printed
// to stdout/stderr. Uses a local HTTP server only (no internet).
//   xvfb-run -a node scripts/visual-check.mjs [--packaged]
import { _electron as electron } from 'playwright';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const packaged = process.argv.includes('--packaged');
const outDir = path.join(root, 'test-output');
fs.mkdirSync(outDir, { recursive: true });
const prefix = packaged ? 'packaged' : 'dev';
const MOD = process.platform === 'darwin' ? 'Meta' : 'Control';

const PAGE = `<!doctype html><html><head><meta charset="utf-8"><title>Sample page</title>
<link rel="icon" href="/favicon.svg"><style>body{font:16px/1.6 system-ui,sans-serif;margin:0;padding:40px;max-width:760px}
h1{margin-top:0}a{color:#4f46e5}.card{padding:16px;border:1px solid #ddd;border-radius:12px;margin:16px 0}</style></head>
<body><h1>A sample web page</h1><p>OpenSurf renders this page in a sandboxed tab. The word <b>surf</b> appears a few times so that
find-in-page has something to match: surf, surfing, surfer.</p><div class="card"><a href="/file.bin">Download a file</a> ·
<a href="/" target="_blank">Open in new tab</a></div><p>${'Lorem ipsum dolor sit amet, consectetur adipiscing elit. '.repeat(12)}</p></body></html>`;
const FAVICON = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7" fill="#f59e0b"/></svg>';

const server = http.createServer((req, res) => {
  if (req.url === '/favicon.svg') return res.writeHead(200, { 'content-type': 'image/svg+xml' }).end(FAVICON);
  if (req.url === '/file.bin') {
    return res.writeHead(200, { 'content-type': 'application/octet-stream', 'content-disposition': 'attachment; filename="report.pdf"', 'content-length': 300000 }).end(Buffer.alloc(300000, 7));
  }
  res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(PAGE);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}`;
// A port with nothing listening, for the error page.
const closed = http.createServer();
await new Promise((r) => closed.listen(0, '127.0.0.1', r));
const refusedUrl = `http://127.0.0.1:${closed.address().port}/`;
await new Promise((r) => closed.close(r));

const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'opensurf-visual-'));
const downloads = fs.mkdtempSync(path.join(os.tmpdir(), 'opensurf-dl-'));
const app = await electron.launch({
  ...(packaged ? { executablePath: path.join(root, 'dist', 'linux-unpacked', 'opensurf'), args: ['--no-sandbox'] } : { args: ['--no-sandbox', '.'] }),
  cwd: root,
  colorScheme: 'no-override', // follow nativeTheme instead of Playwright's default "light" emulation
  env: { ...process.env, OPENSURF_USER_DATA_DIR: userData, OPENSURF_TEST: '1', NO_PROXY: '127.0.0.1,localhost', no_proxy: '127.0.0.1,localhost' },
});
const logs = [];
app.process().stdout.on('data', (d) => logs.push(`[stdout] ${d}`));
app.process().stderr.on('data', (d) => logs.push(`[stderr] ${d}`));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function waitFor(fn, what, timeout = 15000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (_) { /* retry */ }
    await sleep(100);
  }
  throw new Error(`Timed out: ${what}`);
}
const state = () => app.evaluate(() => globalThis.__opensurfTest.state()[0]);

async function shot(chrome, name) {
  await sleep(400);
  const overlay = await chrome.evaluate(() => !document.getElementById('backdrop').hidden);
  if (!overlay) {
    const tabImage = await app.evaluate(() => globalThis.__opensurfTest.captureActiveTab());
    if (tabImage) {
      await chrome.evaluate(async (src) => {
        const b = document.getElementById('backdrop');
        b.src = src;
        b.hidden = false;
        await b.decode();
      }, tabImage);
    }
  }
  const file = path.join(outDir, `${prefix}-${name}.png`);
  await chrome.screenshot({ path: file });
  if (!overlay) await chrome.evaluate(() => { document.getElementById('backdrop').hidden = true; });
  console.log('screenshot', path.relative(root, file));
}

let failed = false;
try {
  await app.evaluate(({ app: a }, dir) => a.setPath('downloads', dir), downloads);
  const chrome = await waitFor(() => app.windows().find((p) => p.url().endsWith('/src/renderer/index.html')), 'chrome UI');
  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].setSize(1280, 800));
  await waitFor(async () => (await state()).tabs[0].committedUrl.startsWith('opensurf://home/'), 'home page');
  await waitFor(() => app.windows().find((p) => p.url().startsWith('opensurf://home/')), 'home page target');
  const home = app.windows().find((p) => p.url().startsWith('opensurf://home/'));
  await home.waitForSelector('#caption');
  const homeInfo = await home.evaluate(() => ({ caption: document.getElementById('caption').textContent, logo: document.querySelector('.logo').naturalWidth }));
  if (!homeInfo.caption.includes('DuckDuckGo') || !homeInfo.logo) throw new Error(`home page not rendered correctly: ${JSON.stringify(homeInfo)}`);
  console.log('home page served from', packaged ? 'app.asar' : 'source tree', homeInfo);

  for (const theme of ['light', 'dark']) {
    await app.evaluate(({ nativeTheme }, t) => { nativeTheme.themeSource = t; }, theme);
    await sleep(300);
    await shot(chrome, `home-${theme}`);
  }
  await app.evaluate(({ nativeTheme }) => { nativeTheme.themeSource = 'light'; });

  // A web page in a second tab
  await chrome.keyboard.press(`${MOD}+t`);
  await waitFor(async () => (await state()).tabs.length === 2, 'second tab');
  await chrome.click('#omnibox');
  await chrome.keyboard.type(`${base}/`);
  await chrome.keyboard.press('Enter');
  await waitFor(async () => (await state()).tabs[1].committedUrl === `${base}/`, 'sample page');
  await waitFor(async () => (await chrome.locator('.tab.active .tab-icon img').count()) === 1, 'favicon');
  await shot(chrome, 'page-light');

  // Find in page
  await chrome.keyboard.press(`${MOD}+f`);
  await chrome.waitForSelector('#findbar:not([hidden])');
  await chrome.keyboard.type('surf');
  await waitFor(async () => /\d+\/\d+/.test(await chrome.textContent('#find-count')), 'find results');
  console.log('find count:', await chrome.textContent('#find-count'));
  await shot(chrome, 'find-light');
  await chrome.keyboard.press('Escape');
  await chrome.waitForSelector('#findbar', { state: 'hidden' });

  // Download
  const page = app.windows().find((p) => p.url() === `${base}/`);
  await page.click('a[href="/file.bin"]');
  await waitFor(() => fs.readdirSync(downloads).includes('report.pdf'), 'download saved to Downloads');

  // Dark mode: menu, downloads, settings
  await app.evaluate(({ nativeTheme }) => { nativeTheme.themeSource = 'dark'; });
  await sleep(300);
  await chrome.click('#btn-menu');
  await chrome.waitForSelector('#menu-popover:not([hidden])');
  await shot(chrome, 'menu-dark');
  await chrome.click('#menu-popover [data-action="downloads"]');
  await chrome.waitForSelector('#downloads-popover:not([hidden]) .dl-item');
  await shot(chrome, 'downloads-dark');
  await chrome.keyboard.press('Escape');
  await chrome.click('#btn-menu');
  await chrome.click('#menu-popover [data-action="settings"]');
  await chrome.waitForSelector('#settings-dialog:not([hidden])');
  await shot(chrome, 'settings-dark');
  await chrome.keyboard.press('Escape');

  // Error panel (connection refused)
  await chrome.click('#omnibox');
  await chrome.keyboard.type(refusedUrl);
  await chrome.keyboard.press('Enter');
  await waitFor(async () => !(await chrome.locator('#error-panel').isHidden()), 'error panel');
  await shot(chrome, 'error-dark');

  // Narrow window
  await app.evaluate(({ nativeTheme, BrowserWindow }) => { nativeTheme.themeSource = 'light'; BrowserWindow.getAllWindows()[0].setSize(660, 520); });
  await chrome.click('.tab');
  await sleep(500);
  await shot(chrome, 'narrow-light');
} catch (err) {
  failed = true;
  console.error('FAILED:', err.message);
} finally {
  await app.close().catch(() => {});
  server.close();
  fs.rmSync(userData, { recursive: true, force: true });
  fs.rmSync(downloads, { recursive: true, force: true });
}

// Errors in the app's output fail the check (Chromium's harmless D-Bus warnings in containers are ignored).
const lines = logs.join('').split('\n').map((l) => l.trim()).filter(Boolean);
const expected = (l) =>
  /dbus|bus\.cc|Failed to connect to the bus/i.test(l) || // no D-Bus in the container
  l.includes(`Failed to load URL: ${refusedUrl}`) || // the error-page scene, on purpose
  /^\(Use `electron --trace-warnings/.test(l) || /Debugger (listening|ending)|nodejs\.org\/.*debugging/.test(l); // Playwright's inspector
const problems = lines.filter((l) => /error|exception|uncaught|fatal|failed|warn/i.test(l) && !expected(l));
if (problems.length) {
  console.error(`--- ${problems.length} unexpected problem line(s) in app output ---\n${problems.join('\n')}`);
  failed = true;
} else {
  console.log(`app output: ${lines.length} line(s), no unexpected errors`);
}
process.exit(failed ? 1 : 0);
