// Privacy check: runs the app with every connection forced through a local logging proxy,
// exercises the UI without opening any page, and fails if the app contacted anything.
//   xvfb-run -a node scripts/privacy-check.mjs [--packaged]
import { _electron as electron } from 'playwright';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const packaged = process.argv.includes('--packaged');
const MOD = process.platform === 'darwin' ? 'Meta' : 'Control';
const seen = [];

// Logs and refuses every request / CONNECT tunnel.
const proxy = http.createServer((req, res) => {
  seen.push(`${req.method} ${req.url}`);
  res.writeHead(403).end();
});
proxy.on('connect', (req, socket) => {
  seen.push(`CONNECT ${req.url}`);
  socket.end('HTTP/1.1 403 Forbidden\r\n\r\n');
});
await new Promise((r) => proxy.listen(0, '127.0.0.1', r));
const proxyArgs = [`--proxy-server=http://127.0.0.1:${proxy.address().port}`, '--proxy-bypass-list=<-loopback>'];

const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'opensurf-privacy-'));
const app = await electron.launch({
  ...(packaged
    ? { executablePath: path.join(root, 'dist', 'linux-unpacked', 'opensurf'), args: ['--no-sandbox', ...proxyArgs] }
    : { args: ['--no-sandbox', ...proxyArgs, '.'] }),
  cwd: root,
  env: { ...process.env, OPENSURF_USER_DATA_DIR: userData, OPENSURF_TEST: '1' },
});
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let ok = true;
let idle = [];
try {
  let chrome;
  for (let i = 0; i < 100 && !chrome; i++) {
    chrome = app.windows().find((p) => p.url().endsWith('/src/renderer/index.html'));
    if (!chrome) await sleep(100);
  }
  await chrome.waitForSelector('#tabs .tab');
  // Text fields (spellcheck), popovers, settings, new tabs and windows: none of this may touch the network.
  await chrome.click('#omnibox');
  await chrome.keyboard.type('some text with a speling mistake');
  await chrome.keyboard.press('Escape');
  const home = app.windows().find((p) => p.url().startsWith('opensurf://home/'));
  await home.click('input[name="q"]');
  await home.keyboard.type('anothr mispeled word');
  await chrome.click('#btn-menu');
  await chrome.click('#menu-popover [data-action="settings"]');
  await chrome.waitForSelector('#settings-dialog:not([hidden])');
  await chrome.selectOption('#set-engine', 'brave');
  await chrome.keyboard.press('Escape');
  await chrome.click('#btn-downloads');
  await chrome.keyboard.press('Escape');
  await chrome.keyboard.press(`${MOD}+t`);
  await chrome.keyboard.press(`${MOD}+n`);
  await sleep(8000);
  idle = seen.splice(0);
  // Positive control: a page the user opens does go through the proxy.
  await chrome.bringToFront();
  await chrome.click('#omnibox');
  await chrome.keyboard.type('example.com');
  await chrome.keyboard.press('Enter');
  for (let i = 0; i < 100 && !seen.some((l) => l.includes('example.com')); i++) await sleep(100);
  if (!seen.some((l) => l.includes('example.com'))) throw new Error('control request to example.com was not seen by the proxy');
} catch (err) {
  ok = false;
  console.error('FAILED:', err.message);
} finally {
  await app.close().catch(() => {});
  proxy.close();
  fs.rmSync(userData, { recursive: true, force: true });
}
if (idle.length) {
  ok = false;
  console.error(`The app made ${idle.length} network request(s) on its own:\n${idle.join('\n')}`);
} else if (ok) {
  console.log('No network requests without user navigation (control navigation was seen by the proxy): OK');
}
process.exit(ok ? 0 : 1);
