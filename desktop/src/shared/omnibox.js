/*
 * OpenSurf omnibox: pure functions that turn what the user typed into a URL.
 *
 * Shared by the main process (the single source of truth for resolution) and the
 * chrome UI renderer (engine list + instant template validation). Works as a
 * CommonJS module and as a classic <script> (exposes `OpenSurfOmnibox`).
 */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.OpenSurfOmnibox = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const QUERY_TOKEN = '{q}';
  const CUSTOM_TOKEN = '%s';
  const MAX_TEMPLATE_LENGTH = 2048;

  /** Built-in engines. `off` is the default (SafeSearch off = unrestricted). */
  const ENGINES = Object.freeze([
    { id: 'duckduckgo', name: 'DuckDuckGo', off: 'https://duckduckgo.com/?q={q}&kp=-2', on: 'https://duckduckgo.com/?q={q}&kp=1' },
    { id: 'google', name: 'Google', off: 'https://www.google.com/search?q={q}&safe=off', on: 'https://www.google.com/search?q={q}&safe=active' },
    { id: 'bing', name: 'Bing', off: 'https://www.bing.com/search?q={q}&adlt=off', on: 'https://www.bing.com/search?q={q}&adlt=strict' },
    { id: 'brave', name: 'Brave Search', off: 'https://search.brave.com/search?q={q}&safesearch=off', on: 'https://search.brave.com/search?q={q}&safesearch=strict' },
    { id: 'startpage', name: 'Startpage', off: 'https://www.startpage.com/sp/search?query={q}&qadf=none', on: 'https://www.startpage.com/sp/search?query={q}&qadf=heavy' },
    { id: 'mojeek', name: 'Mojeek', off: 'https://www.mojeek.com/search?q={q}&safe=0', on: 'https://www.mojeek.com/search?q={q}&safe=1' },
  ].map(Object.freeze));

  const CUSTOM_ENGINE = Object.freeze({ id: 'custom', name: 'Custom' });
  const DEFAULT_ENGINE = 'duckduckgo';
  const ENGINE_IDS = Object.freeze(ENGINES.map((e) => e.id).concat(CUSTOM_ENGINE.id));

  function getEngine(id) {
    return ENGINES.find((e) => e.id === id) || null;
  }

  /** Display name of the engine that will actually be used for these options. */
  function engineName(options) {
    const opts = options || {};
    if (opts.engine === 'custom' && validateCustomTemplate(opts.customTemplate).ok) return CUSTOM_ENGINE.name;
    return (getEngine(opts.engine) || getEngine(DEFAULT_ENGINE)).name;
  }

  /** UTF-8 percent-encoding; encodes space as %20 and "&", "#", "+", "?", "=", "/" etc. */
  function encodeQuery(query) {
    return encodeURIComponent(String(query));
  }

  /**
   * Validates a custom search template: must be http(s), contain "%s" and be a valid URL.
   * @returns {{ok: true, template: string} | {ok: false, error: string}}
   */
  function validateCustomTemplate(template) {
    if (typeof template !== 'string') return { ok: false, error: 'Template must be text.' };
    const t = template.trim();
    if (!t) return { ok: false, error: 'Enter a search URL template.' };
    if (t.length > MAX_TEMPLATE_LENGTH) return { ok: false, error: 'Template is too long.' };
    if (!/^https?:\/\//i.test(t)) return { ok: false, error: 'Template must start with http:// or https://' };
    if (!t.includes(CUSTOM_TOKEN)) return { ok: false, error: 'Template must contain %s where the search terms go.' };
    if (/\s/.test(t)) return { ok: false, error: 'Template must not contain spaces.' };
    try {
      const marker = 'opensurfquery';
      const url = new URL(t.split(CUSTOM_TOKEN).join(marker));
      if (!url.hostname) return { ok: false, error: 'Template has no host name.' };
      if (url.hostname.includes(marker)) return { ok: false, error: 'Put %s in the path or query, not in the host name.' };
    } catch (_) {
      return { ok: false, error: 'Template is not a valid URL.' };
    }
    return { ok: true, template: t };
  }

  /**
   * Builds the search URL for `query`.
   * options: { engine, safeSearch (default false), customTemplate }
   * An unknown engine, or "custom" with an invalid template, falls back to DuckDuckGo.
   */
  function buildSearchUrl(query, options) {
    const opts = options || {};
    const encoded = encodeQuery(query);
    if (opts.engine === 'custom') {
      const check = validateCustomTemplate(opts.customTemplate);
      if (check.ok) return check.template.split(CUSTOM_TOKEN).join(encoded);
    }
    const engine = getEngine(opts.engine) || getEngine(DEFAULT_ENGINE);
    const template = opts.safeSearch === true ? engine.on : engine.off;
    return template.replace(QUERY_TOKEN, encoded);
  }

  const PORT_RE = /^:(\d{1,5})$/;
  const IPV4_RE = /^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}$/;
  const LABEL_RE = /^[\p{L}\p{N}](?:[\p{L}\p{N}-]{0,61}[\p{L}\p{N}])?$/u;
  const TLD_RE = /^(?:\p{L}{2,63}|xn--[a-z0-9-]{1,59})$/iu;

  function isIPv6Literal(host) {
    if (!/^\[[0-9a-f:.]+\]$/i.test(host) || (host.match(/:/g) || []).length < 2) return false;
    try {
      return new URL('http://' + host + '/').hostname.length > 2;
    } catch (_) {
      return false;
    }
  }

  function isDomain(host) {
    const labels = host.split('.');
    if (labels.length < 2 || host.length > 253) return false;
    if (!labels.every((l) => LABEL_RE.test(l))) return false;
    return TLD_RE.test(labels[labels.length - 1]);
  }

  /**
   * If `text` (no whitespace) looks like host[:port][/path?query#frag], returns the
   * scheme to prepend ("http://" or "https://"), otherwise null.
   */
  function hostSchemeFor(text) {
    if (!text || /\s/.test(text)) return null;
    const cut = text.search(/[/?#]/);
    const authority = cut === -1 ? text : text.slice(0, cut);
    let host = authority;
    let port = '';
    if (authority.startsWith('[')) {
      const end = authority.indexOf(']');
      if (end === -1) return null;
      host = authority.slice(0, end + 1);
      port = authority.slice(end + 1);
    } else {
      const colon = authority.lastIndexOf(':');
      if (colon !== -1) {
        host = authority.slice(0, colon);
        port = authority.slice(colon);
      }
    }
    if (port) {
      const m = PORT_RE.exec(port);
      if (!m || Number(m[1]) > 65535) return null;
    }
    if (!host) return null;
    if (host.toLowerCase() === 'localhost' || IPV4_RE.test(host) || isIPv6Literal(host)) return 'http://';
    if (isDomain(host)) return 'https://';
    return null;
  }

  /**
   * Classifies and resolves omnibox input.
   * options: { engine, safeSearch, customTemplate, allowFile }
   * @returns {{type: 'url'|'search', url: string} | null}
   */
  function classifyInput(text, options) {
    if (typeof text !== 'string') return null;
    const t = text.trim();
    if (!t) return null;
    const opts = options || {};
    if (/^https?:\/\//i.test(t)) return { type: 'url', url: t };
    if (opts.allowFile === true && /^file:\/\//i.test(t)) return { type: 'url', url: t };
    const scheme = hostSchemeFor(t);
    if (scheme) return { type: 'url', url: scheme + t };
    // Everything else, including other explicit schemes (javascript:, data:, ftp:...), is a search.
    return { type: 'search', url: buildSearchUrl(t, opts) };
  }

  /** Resolves omnibox input to the URL to load, or null for empty input. */
  function resolveInput(text, options) {
    const result = classifyInput(text, options);
    return result ? result.url : null;
  }

  /** Extracts the raw query from an internal "opensurf://go?q=..." URL, or null. */
  function parseGoUrl(url) {
    if (typeof url !== 'string' || !/^opensurf:\/\/go(?:[/?#]|$)/i.test(url)) return null;
    try {
      const q = new URL(url).searchParams.get('q');
      return q === null ? '' : q;
    } catch (_) {
      return null;
    }
  }

  return Object.freeze({
    ENGINES,
    ENGINE_IDS,
    CUSTOM_ENGINE,
    DEFAULT_ENGINE,
    getEngine,
    engineName,
    encodeQuery,
    validateCustomTemplate,
    buildSearchUrl,
    classifyInput,
    resolveInput,
    parseGoUrl,
  });
});
