'use strict';

// Session hardening: permission prompts, certificate errors, device pickers and
// handing non-web schemes to the OS after the user agrees.

const { app, dialog, shell } = require('electron');
const { classifyUrl, protocolOf, originOf, hostOf } = require('./urls');
const registry = require('./registry');

/** Granted without asking. */
const ALWAYS_ALLOWED = new Set(['fullscreen', 'clipboard-sanitized-write', 'pointerLock']);

/** Asked once per origin + permission for this session. Everything else is denied. */
const PROMPTED = {
  media: 'use your camera and/or microphone',
  geolocation: 'know your location',
  notifications: 'show notifications',
  midi: 'use your MIDI devices',
  midiSysex: 'get full control of your MIDI devices',
  'display-capture': 'see your screen',
  'idle-detection': 'know when you are actively using this device',
  'clipboard-read': 'see text and images copied to the clipboard',
  'window-management': 'manage windows on all your displays',
  'storage-access': 'use cookies and site data in an embedded frame',
  'top-level-storage-access': 'use cookies and site data across sites',
  'speaker-selection': 'choose audio output devices',
};

const permissionDecisions = new Map(); // "origin|permission" -> boolean
const pendingPrompts = new Map(); // key -> Promise<boolean>
const certDecisions = new Map(); // "host|fingerprint" -> boolean
const pendingCertPrompts = new Map();
const externalAllowed = new Set(); // "origin|scheme:"
const externalCooldown = new Map(); // "origin|scheme:" -> timestamp until which prompts are suppressed

function dialogParent(wc) {
  const win = registry.windowForWebContents(wc);
  return win && !win.isDestroyed() ? win : undefined;
}

async function showMessageBox(wc, options) {
  const parent = dialogParent(wc);
  return parent ? dialog.showMessageBox(parent, options) : dialog.showMessageBox(options);
}

/** Deduplicates concurrent prompts with the same key. */
function once(map, key, fn) {
  if (map.has(key)) return map.get(key);
  const p = Promise.resolve().then(fn).finally(() => map.delete(key));
  map.set(key, p);
  return p;
}

function mediaKeys(origin, mediaTypes) {
  const types = Array.isArray(mediaTypes) && mediaTypes.length ? mediaTypes : ['unknown'];
  return types.map((t) => `${origin}|media:${t}`);
}

function describeMedia(mediaTypes) {
  const t = new Set(mediaTypes || []);
  if (t.has('video') && t.has('audio')) return 'use your camera and microphone';
  if (t.has('video')) return 'use your camera';
  if (t.has('audio')) return 'use your microphone';
  return PROMPTED.media;
}

async function askPermission(wc, origin, permission, details) {
  const what = permission === 'media' ? describeMedia(details.mediaTypes) : PROMPTED[permission];
  const { response } = await showMessageBox(wc, {
    type: 'question',
    title: 'Permission request',
    message: `${origin} wants to ${what}.`,
    detail: 'Your choice is remembered for this site until OpenSurf is closed.',
    buttons: ['Block', 'Allow'],
    defaultId: 0,
    cancelId: 0,
    noLink: true,
  });
  return response === 1;
}

function isTabContents(wc) {
  return Boolean(registry.tabForWebContents(wc));
}

function setupPermissions(ses) {
  ses.setPermissionRequestHandler((wc, permission, callback, details) => {
    const d = details || {};
    if (permission === 'openExternal') {
      // Renderer-initiated external protocol launches that got past the navigation guards.
      if (d.externalURL && isTabContents(wc)) openExternalWithConsent(wc, d.externalURL, d.requestingUrl);
      callback(false);
      return;
    }
    if (ALWAYS_ALLOWED.has(permission)) return callback(true);
    if (!PROMPTED[permission] || !isTabContents(wc)) return callback(false);
    const origin = originOf(d.requestingUrl || wc.getURL());
    if (!/^https?:/.test(origin) && origin !== 'file://') return callback(false);

    const keys = permission === 'media' ? mediaKeys(origin, d.mediaTypes) : [`${origin}|${permission}`];
    const known = keys.map((k) => permissionDecisions.get(k));
    if (known.some((v) => v === false)) return callback(false);
    if (known.every((v) => v === true)) return callback(true);

    once(pendingPrompts, keys.join(','), () => askPermission(wc, origin, permission, d))
      .then((allow) => {
        for (const k of keys) permissionDecisions.set(k, allow);
        callback(allow);
      })
      .catch(() => callback(false));
  });

  ses.setPermissionCheckHandler((wc, permission, requestingOrigin, details) => {
    if (ALWAYS_ALLOWED.has(permission)) return true;
    if (!PROMPTED[permission]) return false;
    const origin = originOf(requestingOrigin || (details && details.requestingUrl) || '');
    if (permission === 'media') {
      const type = details && details.mediaType;
      return permissionDecisions.get(`${origin}|media:${type || 'unknown'}`) === true;
    }
    return permissionDecisions.get(`${origin}|${permission}`) === true;
  });

  // No WebHID / WebSerial / WebUSB / Bluetooth device access.
  ses.setDevicePermissionHandler(() => false);
  ses.on('select-hid-device', (event, _details, callback) => { event.preventDefault(); callback(); });
  ses.on('select-serial-port', (event, _ports, _wc, callback) => { event.preventDefault(); callback(''); });
  ses.on('select-usb-device', (event, _details, callback) => { event.preventDefault(); callback(); });
}

/** Certificate errors are never auto-accepted: main-frame errors ask the user, default Cancel. */
function setupCertificateHandling() {
  app.on('certificate-error', (event, wc, url, error, certificate, callback, isMainFrame) => {
    event.preventDefault();
    const host = hostOf(url);
    const key = `${host}|${certificate && certificate.fingerprint}`;
    if (certDecisions.has(key)) return callback(certDecisions.get(key));
    if (!isMainFrame || !isTabContents(wc)) return callback(false);

    once(pendingCertPrompts, key, async () => {
      const { response } = await showMessageBox(wc, {
        type: 'warning',
        title: 'Your connection is not private',
        message: `Your connection to ${host} is not private.`,
        detail:
          `Attackers might be trying to steal your information from ${host} (for example, passwords, messages or credit cards).\n\n` +
          `Error: ${error}\nIssued to: ${certificate.subjectName}\nIssued by: ${certificate.issuerName}\nFingerprint: ${certificate.fingerprint}\n\n` +
          'Only proceed if you understand the risk.',
        buttons: ['Cancel', 'Proceed anyway (unsafe)'],
        defaultId: 0,
        cancelId: 0,
        noLink: true,
      });
      return response === 1;
    })
      .then((proceed) => {
        certDecisions.set(key, proceed);
        callback(proceed);
      })
      .catch(() => callback(false));
  });

  // Never send a client certificate without asking; OpenSurf has no picker, so none is sent.
  app.on('select-client-certificate', (event, _wc, _url, _list, callback) => {
    event.preventDefault();
    callback();
  });
}

/** Lets the user be asked again about a host whose certificate they declined. */
function forgetCertificateDenials(host) {
  for (const [key, value] of certDecisions) if (!value && key.startsWith(`${host}|`)) certDecisions.delete(key);
}

/**
 * Hands an allow-listed non-web URL (mailto:, tel:, ...) to the OS after confirmation.
 * Everything else is ignored.
 */
function openExternalWithConsent(wc, url, sourceUrl) {
  if (typeof url !== 'string' || url.length > 4096 || classifyUrl(url) !== 'external') return;
  const scheme = protocolOf(url);
  const origin = originOf(sourceUrl || (wc && !wc.isDestroyed() ? wc.getURL() : '')) || 'This page';
  const key = `${origin}|${scheme}`;
  const open = () => shell.openExternal(url).catch(() => {});
  if (externalAllowed.has(key)) return void open();
  if ((externalCooldown.get(key) || 0) > Date.now()) return;

  once(pendingPrompts, `external:${key}`, async () => {
    const { response, checkboxChecked } = await showMessageBox(wc, {
      type: 'question',
      title: 'Open external application?',
      message: `Open this ${scheme.slice(0, -1)} link with another application?`,
      detail: `${origin} wants to open:\n${url.length > 200 ? url.slice(0, 200) + '…' : url}`,
      checkboxLabel: `Always allow ${origin} to open ${scheme} links`,
      buttons: ['Cancel', 'Open'],
      defaultId: 0,
      cancelId: 0,
      noLink: true,
    });
    if (response === 1) {
      if (checkboxChecked) externalAllowed.add(key);
      open();
    } else {
      externalCooldown.set(key, Date.now() + 10_000);
    }
  }).catch(() => {});
}

function clearSessionDecisions() {
  permissionDecisions.clear();
  certDecisions.clear();
  externalAllowed.clear();
  externalCooldown.clear();
}

module.exports = {
  setupPermissions,
  setupCertificateHandling,
  forgetCertificateDenials,
  openExternalWithConsent,
  clearSessionDecisions,
};
