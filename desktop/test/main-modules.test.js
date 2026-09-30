'use strict';

// Unit tests for the Electron-independent main-process modules.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { SettingsStore, DEFAULTS, sanitize } = require('../src/main/settings');
const { SessionStore, sanitizeSession } = require('../src/main/session-store');
const { writeFileAtomic, readJson, sanitizeFilename, uniquePath } = require('../src/main/fsutil');
const { classifyUrl, homeUrl, homeCaption, isHomeUrl, originOf } = require('../src/main/urls');
const { resolveHomeFile, mimeTypeFor, HOME_DIR } = require('../src/main/home-files');
const { matchShortcut } = require('../src/shared/shortcuts');

function tmpDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'opensurf-unit-'));
}

// ---------------------------------------------------------------- settings

test('settings: defaults (DuckDuckGo, SafeSearch off, JavaScript on, restore tabs on)', () => {
  assert.deepEqual(DEFAULTS, { engine: 'duckduckgo', safeSearch: false, customTemplate: '', javascript: true, restoreTabs: true });
  const dir = tmpDir();
  const store = new SettingsStore(path.join(dir, 'settings.json'));
  assert.deepEqual(store.get(), DEFAULTS);
});

test('settings: invalid or missing file falls back to defaults field by field', () => {
  assert.deepEqual(sanitize(null), DEFAULTS);
  assert.deepEqual(sanitize([]), DEFAULTS);
  assert.deepEqual(sanitize({ engine: 'evil', safeSearch: 'yes', javascript: 0, restoreTabs: false }), { ...DEFAULTS, restoreTabs: false });
  assert.equal(sanitize({ engine: 'custom', customTemplate: 'not a url' }).engine, 'duckduckgo');
  assert.equal(sanitize({ engine: 'custom', customTemplate: 'https://x.org/?q=%s' }).engine, 'custom');

  const dir = tmpDir();
  const file = path.join(dir, 'settings.json');
  fs.writeFileSync(file, '{ this is not json');
  assert.deepEqual(new SettingsStore(file).get(), DEFAULTS);
  fs.writeFileSync(file, JSON.stringify({ engine: 'bing', safeSearch: true }));
  assert.deepEqual(new SettingsStore(file).get(), { ...DEFAULTS, engine: 'bing', safeSearch: true });
});

test('settings: validated updates are persisted atomically and emitted', () => {
  const dir = tmpDir();
  const file = path.join(dir, 'settings.json');
  const store = new SettingsStore(file);
  const events = [];
  store.on('changed', (values, changed) => events.push(changed));

  assert.equal(store.update({ engine: 'google' }).ok, true);
  assert.equal(readJson(file).engine, 'google');
  assert.deepEqual(events, [['engine']]);

  for (const bad of [{ engine: 'nope' }, { safeSearch: 'true' }, { javascript: 1 }, { unknown: true }, { customTemplate: 'ftp://x/%s' }, null, []]) {
    const r = store.update(bad);
    assert.equal(r.ok, false, JSON.stringify(bad));
  }
  // "custom" needs a valid template (in the same update or already saved)
  assert.equal(store.update({ engine: 'custom' }).ok, false);
  const ok = store.update({ engine: 'custom', customTemplate: ' https://s.example/?q=%s ' });
  assert.equal(ok.ok, true);
  assert.equal(ok.settings.customTemplate, 'https://s.example/?q=%s');
  assert.deepEqual(new SettingsStore(file).get(), store.get());
  assert.deepEqual(store.searchOptions({ allowFile: true }), { engine: 'custom', safeSearch: false, customTemplate: 'https://s.example/?q=%s', allowFile: true });
  // No temp files left behind.
  assert.deepEqual(fs.readdirSync(dir), ['settings.json']);
});

test('fsutil: atomic write replaces content, readJson tolerates bad files', () => {
  const dir = tmpDir();
  const file = path.join(dir, 'nested', 'a.json');
  writeFileAtomic(file, '{"a":1}');
  writeFileAtomic(file, '{"a":2}');
  assert.deepEqual(readJson(file), { a: 2 });
  assert.equal(readJson(path.join(dir, 'missing.json')), null);
  assert.deepEqual(fs.readdirSync(path.dirname(file)), ['a.json']);
});

// ---------------------------------------------------------------- downloads helpers

test('downloads: filenames are sanitised and never clobber existing files', () => {
  assert.equal(sanitizeFilename('report.pdf'), 'report.pdf');
  assert.equal(sanitizeFilename('../../etc/passwd'), 'passwd');
  assert.equal(sanitizeFilename('..\\..\\evil.exe'), 'evil.exe');
  assert.equal(sanitizeFilename('a<b>c:d"e|f?g*.txt'), 'a_b_c_d_e_f_g_.txt');
  assert.equal(sanitizeFilename(''), 'download');
  assert.equal(sanitizeFilename('...'), 'download');
  assert.equal(sanitizeFilename('CON.txt'), '_CON.txt');
  assert.ok(sanitizeFilename('x'.repeat(500) + '.zip').length <= 200);
  assert.ok(sanitizeFilename('x'.repeat(500) + '.zip').endsWith('.zip'));

  const dir = tmpDir();
  assert.equal(uniquePath(dir, 'file.txt'), path.join(dir, 'file.txt'));
  fs.writeFileSync(path.join(dir, 'file.txt'), '');
  assert.equal(uniquePath(dir, 'file.txt'), path.join(dir, 'file (1).txt'));
  const reserved = new Set([path.join(dir, 'file (1).txt')]);
  assert.equal(uniquePath(dir, 'file.txt', reserved), path.join(dir, 'file (2).txt'));
  assert.equal(uniquePath(dir, 'noext'), path.join(dir, 'noext'));
});

// ---------------------------------------------------------------- restore tabs

test('session store: round trip and validation', () => {
  const dir = tmpDir();
  const store = new SessionStore(path.join(dir, 'session.json'));
  assert.deepEqual(store.load(), []);
  store.save([
    { tabs: [{ url: 'https://example.com/', title: 'Example' }, { url: 'opensurf://home/', title: 'New Tab' }], activeIndex: 1 },
    { tabs: [{ url: 'javascript:alert(1)' }, { url: 'chrome://settings' }], activeIndex: 0 },
  ]);
  assert.deepEqual(store.load(), [
    { tabs: [{ url: 'https://example.com/', title: 'Example' }, { url: 'opensurf://home/', title: 'New Tab' }], activeIndex: 1 },
  ]);
  assert.deepEqual(sanitizeSession({ windows: [{ tabs: [{ url: 'file:///tmp/a.html' }], activeIndex: 9 }] }), [
    { tabs: [{ url: 'file:///tmp/a.html', title: '' }], activeIndex: 0 },
  ]);
  store.clear();
  assert.deepEqual(store.load(), []);
});

// ---------------------------------------------------------------- URLs & home page

test('urls: classification used by the navigation guards', () => {
  const cases = {
    'https://example.com/': 'web',
    'HTTP://example.com': 'web',
    'opensurf://home/': 'home',
    'opensurf://home/#engine=Google&safe=off': 'home',
    'opensurf://go?q=test': 'go',
    'opensurf://settings': 'blocked',
    'file:///etc/passwd': 'file',
    'blob:https://example.com/uuid': 'blob',
    'about:blank': 'about',
    'about:config': 'blocked',
    'data:text/html,hi': 'data',
    'mailto:someone@example.com': 'external',
    'tel:+123': 'external',
    'sms:+123': 'external',
    'magnet:?xt=urn:btih:abc': 'external',
    'javascript:alert(1)': 'blocked',
    'chrome://settings': 'blocked',
    'devtools://devtools': 'blocked',
    'intent://x#Intent;end': 'blocked',
    'ms-settings:': 'blocked',
    'vbscript:x': 'blocked',
    '': 'blocked',
  };
  for (const [url, kind] of Object.entries(cases)) assert.equal(classifyUrl(url), kind, url);
  assert.equal(isHomeUrl('opensurf://home'), true);
  assert.equal(isHomeUrl('opensurf://homepage/'), false);
  assert.equal(originOf('https://a.example:8443/x?y'), 'https://a.example:8443');
});

test('urls: home URL fragment and caption reflect the settings', () => {
  assert.equal(homeUrl(DEFAULTS), 'opensurf://home/#engine=DuckDuckGo&safe=off');
  assert.equal(homeUrl({ ...DEFAULTS, engine: 'brave', safeSearch: true }), 'opensurf://home/#engine=Brave%20Search&safe=on');
  assert.equal(homeUrl({ engine: 'custom', customTemplate: 'https://x.org/?q=%s', safeSearch: true }), 'opensurf://home/#engine=Custom&safe=off');
  assert.equal(homeCaption(DEFAULTS), 'Unrestricted search · SafeSearch off · DuckDuckGo');
  assert.equal(homeCaption({ ...DEFAULTS, engine: 'google', safeSearch: true }), 'Filtered search · SafeSearch on · Google');
});

test('home files: served from src/home only, path traversal is rejected', () => {
  assert.equal(resolveHomeFile('opensurf://home/'), path.join(HOME_DIR, 'index.html'));
  assert.equal(resolveHomeFile('opensurf://home/#engine=x'), path.join(HOME_DIR, 'index.html'));
  assert.equal(resolveHomeFile('opensurf://home/logo.svg'), path.join(HOME_DIR, 'logo.svg'));
  for (const bad of [
    'opensurf://home/../main/main.js',
    'opensurf://home/%2e%2e/main/main.js',
    'opensurf://home/..%2fmain%2fmain.js',
    'opensurf://home/..%5cmain%5cmain.js',
    'opensurf://home/%00',
    'opensurf://home/%E0%A4%A',
    'opensurf://go?q=x',
    'opensurf://other/index.html',
    'https://home/index.html',
    'not a url',
  ]) {
    const file = resolveHomeFile(bad);
    assert.ok(file === null || file.startsWith(HOME_DIR + path.sep), `${bad} -> ${file}`);
  }
  assert.equal(resolveHomeFile('opensurf://home/../main/main.js') === path.join(HOME_DIR, '..', 'main', 'main.js'), false);
  assert.equal(mimeTypeFor('a.html'), 'text/html; charset=utf-8');
  assert.equal(mimeTypeFor('a.svg'), 'image/svg+xml');
  assert.equal(mimeTypeFor('a.exe'), null);
  for (const f of ['index.html', 'home.css', 'home.js', 'logo.svg']) assert.ok(fs.existsSync(path.join(HOME_DIR, f)), f);
});

// ---------------------------------------------------------------- shortcuts

const key = (k, mods = {}, code) => ({ type: 'keyDown', key: k, code: code || (k.length === 1 ? `Key${k.toUpperCase()}` : k), ...mods });

test('shortcuts: Windows/Linux key map', () => {
  const ctx = { platform: 'linux', source: 'tab' };
  const expect = (input, command, arg) => {
    const m = matchShortcut(input, ctx);
    assert.ok(m, `${JSON.stringify(input)} should match ${command}`);
    assert.equal(m.command, command);
    if (arg !== undefined) assert.deepEqual(m.arg, arg);
  };
  expect(key('t', { control: true }), 'tab.new');
  expect(key('T', { control: true, shift: true }), 'tab.reopen');
  expect(key('w', { control: true }), 'tab.close');
  expect(key('F4', { control: true }), 'tab.close');
  expect(key('n', { control: true }), 'window.new');
  expect(key('l', { control: true }), 'omnibox.focus');
  expect(key('F6'), 'omnibox.focus');
  expect(key('d', { alt: true }), 'omnibox.focus');
  expect(key('r', { control: true }), 'nav.reload');
  expect(key('F5'), 'nav.reload');
  expect(key('R', { control: true, shift: true }), 'nav.hardReload');
  expect(key('ArrowLeft', { alt: true }), 'nav.back');
  expect(key('ArrowRight', { alt: true }), 'nav.forward');
  expect(key('Tab', { control: true }), 'tab.next');
  expect(key('Tab', { control: true, shift: true }), 'tab.prev');
  expect(key('1', { control: true }, 'Digit1'), 'tab.select', 0);
  expect(key('8', { control: true }, 'Digit8'), 'tab.select', 7);
  expect(key('9', { control: true }, 'Digit9'), 'tab.select', -1);
  expect(key('f', { control: true }), 'find.open');
  expect(key('F3'), 'find.step', true);
  expect(key('F3', { shift: true }), 'find.step', false);
  expect(key('=', { control: true }, 'Equal'), 'zoom.in');
  expect(key('+', { control: true, shift: true }, 'Equal'), 'zoom.in');
  expect(key('-', { control: true }, 'Minus'), 'zoom.out');
  expect(key('0', { control: true }, 'Digit0'), 'zoom.reset');
  expect(key('F11'), 'window.fullscreen');
  expect(key('F12'), 'devtools.toggle');
  expect(key('I', { control: true, shift: true }), 'devtools.toggle');
  // Layout independence: Cyrillic layout, physical T key
  expect({ type: 'keyDown', key: 'е', code: 'KeyT', control: true }, 'tab.new');
  // Escape only acts when there is something to stop
  assert.equal(matchShortcut(key('Escape'), ctx), null);
  ctx.loading = true;
  expect(key('Escape'), 'nav.stop');
  ctx.findOpen = true;
  expect(key('Escape'), 'find.close', { focusPage: true });
  ctx.htmlFullscreen = true;
  assert.equal(matchShortcut(key('Escape'), ctx), null, 'Escape leaves HTML fullscreen to the page');
});

test('shortcuts: macOS key map and non-shortcuts', () => {
  const mac = { platform: 'darwin', source: 'chrome' };
  assert.equal(matchShortcut(key('t', { meta: true }), mac).command, 'tab.new');
  assert.equal(matchShortcut(key('t', { control: true }), mac), null, 'Ctrl+T is not a shortcut on macOS');
  assert.equal(matchShortcut(key('[', { meta: true }, 'BracketLeft'), mac).command, 'nav.back');
  assert.equal(matchShortcut(key(']', { meta: true }, 'BracketRight'), mac).command, 'nav.forward');
  assert.equal(matchShortcut(key('f', { meta: true, control: true }), mac).command, 'window.fullscreen');
  assert.equal(matchShortcut(key('i', { meta: true, alt: true }), mac).command, 'devtools.toggle');
  assert.equal(matchShortcut(key('F11'), mac), null);
  assert.equal(matchShortcut(key('Tab', { control: true }), mac).command, 'tab.next');

  const linux = { platform: 'linux', source: 'chrome' };
  for (const input of [key('a', { control: true }), key('c', { control: true }), key('v', { control: true }), key('a'), key('Enter'), key('Escape'), key('ArrowLeft')]) {
    assert.equal(matchShortcut(input, linux), null, JSON.stringify(input));
  }
  assert.equal(matchShortcut({ ...key('t', { control: true }), type: 'keyUp' }, linux), null);
});
