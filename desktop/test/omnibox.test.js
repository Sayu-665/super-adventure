'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const omnibox = require('../src/shared/omnibox.js');

const { resolveInput, classifyInput, buildSearchUrl, validateCustomTemplate, parseGoUrl, engineName, ENGINES } = omnibox;

const DDG = (encoded) => `https://duckduckgo.com/?q=${encoded}&kp=-2`;

/** Accepts "%20" or "+" for spaces, as allowed by the spec. */
function assertSearch(input, expectedQuery, options) {
  const url = resolveInput(input, options);
  assert.ok(url, `expected a search URL for ${JSON.stringify(input)}`);
  const parsed = new URL(url);
  assert.equal(parsed.origin + parsed.pathname, 'https://duckduckgo.com/');
  assert.equal(parsed.searchParams.get('q'), expectedQuery);
  assert.equal(parsed.searchParams.get('kp'), '-2');
  assert.equal(classifyInput(input, options).type, 'search');
  return url;
}

test('mandatory table: URLs', () => {
  const table = [
    ['example.com', 'https://example.com'],
    ['  Example.COM  ', 'https://Example.COM'],
    ['http://example.com', 'http://example.com'],
    ['HTTPS://example.com/a?b=c', 'HTTPS://example.com/a?b=c'],
    ['sub.example.co.uk/path?x=1', 'https://sub.example.co.uk/path?x=1'],
    ['localhost:8080', 'http://localhost:8080'],
    ['192.168.1.1', 'http://192.168.1.1'],
    ['[::1]:3000', 'http://[::1]:3000'],
  ];
  for (const [input, expected] of table) {
    assert.equal(resolveInput(input), expected, `input ${JSON.stringify(input)}`);
    assert.equal(classifyInput(input).type, 'url');
  }
});

test('mandatory table: searches', () => {
  assert.equal(resolveInput('hello world'), DDG('hello%20world'));
  assertSearch('hello world', 'hello world');

  const math = assertSearch('what is 2+2', 'what is 2+2');
  assert.ok(math.includes('2%2B2'), '"+" must be percent-encoded as %2B');
  assert.ok(!/q=[^&]*\+2/.test(math.replace('2%2B2', '')), 'no raw "+" in the query');

  assertSearch('example', 'example');
  assertSearch('cats site:reddit.com', 'cats site:reddit.com');
  assertSearch('javascript:alert(1)', 'javascript:alert(1)');
});

test('mandatory table: empty input returns null', () => {
  assert.equal(resolveInput(''), null);
  assert.equal(resolveInput('   '), null);
  assert.equal(resolveInput('\t\n'), null);
  assert.equal(resolveInput(undefined), null);
  assert.equal(resolveInput(null), null);
  assert.equal(classifyInput(''), null);
});

test('special characters are percent-encoded', () => {
  const url = resolveInput('a&b #c ?d=e/f +g');
  assert.equal(url, DDG('a%26b%20%23c%20%3Fd%3De%2Ff%20%2Bg'));
  const unicode = resolveInput('héllo 世界');
  assert.equal(unicode, DDG('h%C3%A9llo%20%E4%B8%96%E7%95%8C'));
  assert.equal(new URL(unicode).searchParams.get('q'), 'héllo 世界');
});

test('other explicit schemes are searched, never executed', () => {
  for (const input of ['javascript:alert(1)', 'data:text/html,<b>x</b>', 'ftp://example.com/file', 'mailto:a@b.com', 'file:///etc/passwd', 'vbscript:msgbox', 'opensurf://home/', 'chrome://settings']) {
    const result = classifyInput(input);
    assert.equal(result.type, 'search', input);
    assert.equal(new URL(result.url).searchParams.get('q'), input);
  }
});

test('file:// is loaded as-is only when explicitly allowed (typed by the user on desktop)', () => {
  assert.equal(resolveInput('file:///home/user/a.html', { allowFile: true }), 'file:///home/user/a.html');
  assert.equal(resolveInput('FILE:///C:/x.txt', { allowFile: true }), 'FILE:///C:/x.txt');
  assert.equal(classifyInput('file:///etc/passwd').type, 'search');
});

test('host detection edge cases', () => {
  const urls = [
    ['LOCALHOST', 'http://LOCALHOST'],
    ['localhost', 'http://localhost'],
    ['localhost/path?x=1#y', 'http://localhost/path?x=1#y'],
    ['127.0.0.1:8080/admin', 'http://127.0.0.1:8080/admin'],
    ['[2001:db8::1]', 'http://[2001:db8::1]'],
    ['[::1]/x', 'http://[::1]/x'],
    ['example.com:8443/a', 'https://example.com:8443/a'],
    ['example.com/', 'https://example.com/'],
    ['example.com#frag', 'https://example.com#frag'],
    ['example.com?q=1', 'https://example.com?q=1'],
    ['my-site.io', 'https://my-site.io'],
    ['münchen.de', 'https://münchen.de'],
    ['xn--80ak6aa92e.xn--p1ai', 'https://xn--80ak6aa92e.xn--p1ai'],
  ];
  for (const [input, expected] of urls) assert.equal(resolveInput(input), expected, input);

  const searches = [
    'a.b', '1.5', '999.1.1.1', '1.2.3.4.5', 'example.com:99999', 'example.com:', 'example.com:abc',
    'user@example.com', 'user:pass@example.com', '-bad.com', 'bad-.com', 'foo..com', '.com', 'example.',
    '[::1', '[zz::1]', 'localhost:', 'site:reddit.com', 'example.com/path with space', 'C:\\Windows', '2+2',
  ];
  for (const input of searches) assert.equal(classifyInput(input).type, 'search', input);
});

test('every engine in both SafeSearch states', () => {
  const expected = {
    duckduckgo: ['https://duckduckgo.com/?q=a%20b&kp=-2', 'https://duckduckgo.com/?q=a%20b&kp=1'],
    google: ['https://www.google.com/search?q=a%20b&safe=off', 'https://www.google.com/search?q=a%20b&safe=active'],
    bing: ['https://www.bing.com/search?q=a%20b&adlt=off', 'https://www.bing.com/search?q=a%20b&adlt=strict'],
    brave: ['https://search.brave.com/search?q=a%20b&safesearch=off', 'https://search.brave.com/search?q=a%20b&safesearch=strict'],
    startpage: ['https://www.startpage.com/sp/search?query=a%20b&qadf=none', 'https://www.startpage.com/sp/search?query=a%20b&qadf=heavy'],
    mojeek: ['https://www.mojeek.com/search?q=a%20b&safe=0', 'https://www.mojeek.com/search?q=a%20b&safe=1'],
  };
  assert.deepEqual(ENGINES.map((e) => e.id).sort(), Object.keys(expected).sort());
  for (const [engine, [off, on]] of Object.entries(expected)) {
    assert.equal(buildSearchUrl('a b', { engine }), off, `${engine} default (SafeSearch off)`);
    assert.equal(buildSearchUrl('a b', { engine, safeSearch: false }), off, `${engine} off`);
    assert.equal(buildSearchUrl('a b', { engine, safeSearch: true }), on, `${engine} on`);
    assert.equal(resolveInput('a b', { engine, safeSearch: true }), on);
    assert.equal(resolveInput('a b', { engine }), off);
  }
});

test('default engine is DuckDuckGo with SafeSearch off; unknown engines fall back to it', () => {
  assert.equal(buildSearchUrl('x'), 'https://duckduckgo.com/?q=x&kp=-2');
  assert.equal(buildSearchUrl('x', { engine: 'nope' }), 'https://duckduckgo.com/?q=x&kp=-2');
  assert.equal(buildSearchUrl('x', { engine: 'nope', safeSearch: true }), 'https://duckduckgo.com/?q=x&kp=1');
  assert.equal(engineName({}), 'DuckDuckGo');
  assert.equal(engineName({ engine: 'brave' }), 'Brave Search');
});

test('custom engine template', () => {
  const template = 'https://search.example.org/find?terms=%s&lang=en';
  assert.equal(buildSearchUrl('a+b c', { engine: 'custom', customTemplate: template }), 'https://search.example.org/find?terms=a%2Bb%20c&lang=en');
  // SafeSearch does not modify a custom template.
  assert.equal(buildSearchUrl('q', { engine: 'custom', customTemplate: template, safeSearch: true }), 'https://search.example.org/find?terms=q&lang=en');
  // Every %s is replaced; the template is trimmed.
  assert.equal(buildSearchUrl('z', { engine: 'custom', customTemplate: '  http://x.test/%s?q=%s  ' }), 'http://x.test/z?q=z');
  // An invalid custom template falls back to the default engine.
  assert.equal(buildSearchUrl('q', { engine: 'custom', customTemplate: 'ftp://x/%s' }), 'https://duckduckgo.com/?q=q&kp=-2');
  assert.equal(resolveInput('hello world', { engine: 'custom', customTemplate: template }), 'https://search.example.org/find?terms=hello%20world&lang=en');
  assert.equal(engineName({ engine: 'custom', customTemplate: template }), 'Custom');
  assert.equal(engineName({ engine: 'custom', customTemplate: '' }), 'DuckDuckGo');
});

test('custom template validation', () => {
  const valid = ['https://example.com/?q=%s', 'http://localhost:8080/search/%s', 'HTTPS://Example.com/s?q=%s&x=1'];
  for (const t of valid) {
    const r = validateCustomTemplate(t);
    assert.equal(r.ok, true, t);
    assert.equal(r.template, t);
  }
  assert.equal(validateCustomTemplate('  https://a.com/?q=%s ').template, 'https://a.com/?q=%s');
  const invalid = [
    undefined, null, 42, '', '   ', 'example.com/?q=%s', 'ftp://example.com/?q=%s', 'javascript:alert(%s)',
    'https://example.com/?q=', 'https://example.com/?q={q}', 'https://exa mple.com/?q=%s', 'https://%s', 'https:///?q=%s',
    'https://example.com/?q=%s' + 'x'.repeat(3000),
  ];
  for (const t of invalid) {
    const r = validateCustomTemplate(t);
    assert.equal(r.ok, false, String(t));
    assert.equal(typeof r.error, 'string');
  }
});

test('parseGoUrl extracts the home page query', () => {
  assert.equal(parseGoUrl('opensurf://go?q=hello+world'), 'hello world');
  assert.equal(parseGoUrl('opensurf://go/?q=what%20is%202%2B2'), 'what is 2+2');
  assert.equal(parseGoUrl('opensurf://go?q='), '');
  assert.equal(parseGoUrl('opensurf://go'), '');
  assert.equal(parseGoUrl('opensurf://home/'), null);
  assert.equal(parseGoUrl('https://go?q=x'), null);
  assert.equal(parseGoUrl('opensurf://gopher?q=x'), null);
  assert.equal(parseGoUrl(42), null);
  // The query is resolved like omnibox input.
  assert.equal(resolveInput(parseGoUrl('opensurf://go?q=example.com')), 'https://example.com');
});
