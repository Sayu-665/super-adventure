'use strict';

// "Restore tabs on startup": open tab URLs per window, persisted as validated JSON.

const fs = require('node:fs');
const { writeFileAtomic, readJson } = require('./fsutil');
const { classifyUrl } = require('./urls');

const MAX_WINDOWS = 20;
const MAX_TABS = 200;
const MAX_URL = 8192;

function isRestorableUrl(url) {
  return typeof url === 'string' && url.length <= MAX_URL && ['web', 'home', 'file'].includes(classifyUrl(url));
}

/** Returns [{ tabs: [{url, title}], activeIndex }] with invalid entries dropped. */
function sanitizeSession(raw) {
  const windows = raw && Array.isArray(raw.windows) ? raw.windows.slice(0, MAX_WINDOWS) : [];
  const out = [];
  for (const w of windows) {
    if (!w || !Array.isArray(w.tabs)) continue;
    const tabs = w.tabs
      .filter((t) => t && isRestorableUrl(t.url))
      .slice(0, MAX_TABS)
      .map((t) => ({ url: t.url, title: typeof t.title === 'string' ? t.title.slice(0, 300) : '' }));
    if (!tabs.length) continue;
    const idx = Number.isInteger(w.activeIndex) && w.activeIndex >= 0 && w.activeIndex < tabs.length ? w.activeIndex : 0;
    out.push({ tabs, activeIndex: idx });
  }
  return out;
}

class SessionStore {
  constructor(file) {
    this.file = file;
  }

  load() {
    return sanitizeSession(readJson(this.file));
  }

  save(windows) {
    try {
      writeFileAtomic(this.file, JSON.stringify({ version: 1, windows: sanitizeSession({ windows }) }) + '\n');
    } catch (err) {
      console.error('OpenSurf: could not save open tabs:', err.message);
    }
  }

  clear() {
    try {
      fs.unlinkSync(this.file);
    } catch (_) { /* not there */ }
  }
}

module.exports = { SessionStore, sanitizeSession, isRestorableUrl };
