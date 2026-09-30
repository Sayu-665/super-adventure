'use strict';

// URL classification helpers used by the navigation guards. Pure (no Electron imports).

const omnibox = require('../shared/omnibox');

const HOME_URL = 'opensurf://home/';

/** Non-web schemes that may be handed to the OS (after the user confirms). */
const EXTERNAL_SCHEMES = new Set([
  'mailto:', 'tel:', 'sms:', 'callto:', 'facetime:', 'facetime-audio:', 'sip:', 'xmpp:',
  'magnet:', 'webcal:', 'geo:', 'irc:', 'ircs:', 'news:',
]);

function protocolOf(url) {
  if (typeof url !== 'string') return '';
  const m = /^([a-z][a-z0-9+.-]*):/i.exec(url);
  return m ? m[1].toLowerCase() + ':' : '';
}

function isHomeUrl(url) {
  return typeof url === 'string' && /^opensurf:\/\/home(?:[/?#]|$)/i.test(url);
}

/**
 * Kinds: 'web' | 'home' | 'go' | 'file' | 'blob' | 'about' | 'data' | 'external' | 'blocked'
 */
function classifyUrl(url) {
  const protocol = protocolOf(url);
  switch (protocol) {
    case 'http:':
    case 'https:':
      return 'web';
    case 'opensurf:':
      if (isHomeUrl(url)) return 'home';
      if (omnibox.parseGoUrl(url) !== null) return 'go';
      return 'blocked';
    case 'file:':
      return 'file';
    case 'blob:':
      return 'blob';
    case 'about:':
      return /^about:blank(?:[?#]|$)/i.test(url) ? 'about' : 'blocked';
    case 'data:':
      return 'data';
    default:
      return EXTERNAL_SCHEMES.has(protocol) ? 'external' : 'blocked';
  }
}

/** The home page URL; display info for the page is passed in the fragment. */
function homeUrl(settings) {
  const engine = omnibox.engineName(settings);
  const safe = settings && settings.safeSearch === true && engine !== omnibox.CUSTOM_ENGINE.name ? 'on' : 'off';
  return `${HOME_URL}#engine=${encodeURIComponent(engine)}&safe=${safe}`;
}

/** "Unrestricted search · SafeSearch off · DuckDuckGo" style caption (no-JS fallback for the home page). */
function homeCaption(settings) {
  const engine = omnibox.engineName(settings);
  const safeOn = settings && settings.safeSearch === true && engine !== omnibox.CUSTOM_ENGINE.name;
  return safeOn ? `Filtered search · SafeSearch on · ${engine}` : `Unrestricted search · SafeSearch off · ${engine}`;
}

/** Scheme + host (+ port) of a URL, for display in permission prompts. */
function originOf(url) {
  try {
    const u = new URL(url);
    if (u.protocol === 'file:') return 'file://';
    return u.origin && u.origin !== 'null' ? u.origin : `${u.protocol}//${u.host}`;
  } catch (_) {
    return String(url || '').slice(0, 100);
  }
}

function hostOf(url) {
  try {
    return new URL(url).host;
  } catch (_) {
    return '';
  }
}

module.exports = { HOME_URL, EXTERNAL_SCHEMES, protocolOf, isHomeUrl, classifyUrl, homeUrl, homeCaption, originOf, hostOf };
