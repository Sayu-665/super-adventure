'use strict';

// Turns a page's favicon URL into a data: URL for the chrome UI (whose CSP forbids remote
// images). The icon is fetched with the tab's session, without cookies, size-limited.

const { homeAssetDataUrl } = require('./protocol');

const MAX_BYTES = 256 * 1024;
const TIMEOUT_MS = 8000;
const CACHE_SIZE = 200;
const cache = new Map(); // url -> data URL (insertion-ordered LRU)

function remember(url, value) {
  cache.delete(url);
  cache.set(url, value);
  if (cache.size > CACHE_SIZE) cache.delete(cache.keys().next().value);
  return value;
}

function sniffImageType(buf) {
  if (buf.length >= 4 && buf[0] === 0 && buf[1] === 0 && buf[2] === 1 && buf[3] === 0) return 'image/x-icon';
  if (buf.length >= 8 && buf.toString('hex', 0, 8) === '89504e470d0a1a0a') return 'image/png';
  if (buf.length >= 3 && buf[0] === 0xff && buf[1] === 0xd8 && buf[2] === 0xff) return 'image/jpeg';
  if (buf.length >= 6 && buf.toString('ascii', 0, 3) === 'GIF') return 'image/gif';
  if (buf.length >= 12 && buf.toString('ascii', 0, 4) === 'RIFF' && buf.toString('ascii', 8, 12) === 'WEBP') return 'image/webp';
  const head = buf.toString('utf8', 0, Math.min(buf.length, 512)).trimStart();
  if (head.startsWith('<svg') || (head.startsWith('<?xml') && head.includes('<svg'))) return 'image/svg+xml';
  return null;
}

/** @returns {Promise<string|null>} */
async function faviconDataUrl(url, ses) {
  if (typeof url !== 'string' || url.length > 8192) return null;
  if (cache.has(url)) return remember(url, cache.get(url));
  if (/^opensurf:/i.test(url)) return remember(url, homeAssetDataUrl(url));
  if (/^data:image\//i.test(url)) return url.length <= MAX_BYTES * 1.4 ? url : null;
  if (!/^https?:\/\//i.test(url)) return null;
  try {
    const res = await ses.fetch(url, { signal: AbortSignal.timeout(TIMEOUT_MS), credentials: 'omit', cache: 'force-cache' });
    if (!res.ok) return remember(url, null);
    const length = Number(res.headers.get('content-length') || 0);
    if (length > MAX_BYTES) return remember(url, null);
    const buf = Buffer.from(await res.arrayBuffer());
    if (!buf.length || buf.length > MAX_BYTES) return remember(url, null);
    const type = sniffImageType(buf);
    if (!type) return remember(url, null);
    return remember(url, `data:${type};base64,${buf.toString('base64')}`);
  } catch (_) {
    return null; // not cached: may work next time (e.g. offline)
  }
}

module.exports = { faviconDataUrl, sniffImageType };
