'use strict';

// Maps opensurf://home/* URLs to bundled files in src/home (pure; unit tested).

const path = require('node:path');

const HOME_DIR = path.join(__dirname, '..', 'home');

const MIME_TYPES = Object.freeze({
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.webp': 'image/webp',
  '.woff2': 'font/woff2',
});

/** Absolute file inside HOME_DIR for an opensurf://home/... URL, or null (other host, traversal, bad encoding). */
function resolveHomeFile(requestUrl, homeDir = HOME_DIR) {
  let url;
  try {
    url = new URL(requestUrl);
  } catch (_) {
    return null;
  }
  if (url.protocol !== 'opensurf:' || url.hostname !== 'home') return null;
  let pathname;
  try {
    pathname = decodeURIComponent(url.pathname);
  } catch (_) {
    return null;
  }
  if (pathname.includes('\0')) return null;
  if (pathname === '' || pathname.endsWith('/')) pathname += 'index.html';
  const file = path.resolve(homeDir, '.' + path.posix.normalize('/' + pathname));
  const rel = path.relative(homeDir, file);
  if (!rel || rel.startsWith('..') || path.isAbsolute(rel)) return null;
  return file;
}

/** MIME type for a served file, or null if that kind of file is never served. */
function mimeTypeFor(file) {
  return MIME_TYPES[path.extname(file).toLowerCase()] || null;
}

module.exports = { HOME_DIR, MIME_TYPES, resolveHomeFile, mimeTypeFor };
