'use strict';

// The internal "opensurf:" scheme. Only opensurf://home/* is served (bundled files from
// src/home, path-traversal safe, works inside the asar). opensurf://go?q=... is never
// served: tab navigations to it are intercepted and resolved by the omnibox module.

const { protocol } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { HOME_DIR, resolveHomeFile, mimeTypeFor } = require('./home-files');

const SCHEME = 'opensurf';

const HOME_CSP = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "img-src 'self'",
  'form-action opensurf:',
  "base-uri 'none'",
  "frame-ancestors 'none'",
].join('; ');

/** Must run before app 'ready'. */
function registerSchemePrivileges() {
  protocol.registerSchemesAsPrivileged([
    { scheme: SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: false, corsEnabled: false } },
  ]);
}

function textResponse(status, text) {
  return new Response(text, { status, headers: { 'content-type': 'text/plain; charset=utf-8', 'x-content-type-options': 'nosniff' } });
}

/**
 * Registers the handler on a session.
 * @param {Electron.Session} ses
 * @param {{caption: () => string}} hooks - supplies the no-JS caption rendered into index.html
 */
function handleProtocol(ses, hooks) {
  ses.protocol.handle(SCHEME, async (request) => {
    if (request.method !== 'GET' && request.method !== 'HEAD') return textResponse(405, 'Method not allowed');
    const file = resolveHomeFile(request.url);
    if (!file) return textResponse(404, 'Not found');
    const type = mimeTypeFor(file);
    if (!type) return textResponse(404, 'Not found');
    let body;
    try {
      body = await fs.promises.readFile(file);
    } catch (_) {
      return textResponse(404, 'Not found');
    }
    if (path.basename(file) === 'index.html' && hooks && typeof hooks.caption === 'function') {
      body = body.toString('utf8').replace('%CAPTION%', escapeHtml(hooks.caption()));
    }
    return new Response(body, {
      status: 200,
      headers: {
        'content-type': type,
        'content-security-policy': HOME_CSP,
        'x-content-type-options': 'nosniff',
        'referrer-policy': 'no-referrer',
        'cache-control': 'no-store',
      },
    });
  });
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

/** Reads a bundled home asset as a data: URL (used for the home tab favicon). */
function homeAssetDataUrl(requestUrl) {
  const file = resolveHomeFile(requestUrl);
  const type = file && mimeTypeFor(file);
  if (!type || !type.startsWith('image/')) return null;
  try {
    return `data:${type};base64,${fs.readFileSync(file).toString('base64')}`;
  } catch (_) {
    return null;
  }
}

module.exports = { SCHEME, HOME_DIR, registerSchemePrivileges, handleProtocol, resolveHomeFile, homeAssetDataUrl };
