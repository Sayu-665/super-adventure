'use strict';

// Small file-system helpers (no Electron imports, unit tested).

const fs = require('node:fs');
const path = require('node:path');

/** Writes a file atomically: temp file in the same directory, fsync, then rename over the target. */
function writeFileAtomic(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.${process.pid}.${Date.now()}.${Math.random().toString(36).slice(2)}.tmp`;
  try {
    const fd = fs.openSync(tmp, 'w', 0o600);
    try {
      fs.writeFileSync(fd, data);
      fs.fsyncSync(fd);
    } finally {
      fs.closeSync(fd);
    }
    fs.renameSync(tmp, file);
  } catch (err) {
    try { fs.unlinkSync(tmp); } catch (_) { /* ignore */ }
    throw err;
  }
}

/** Parsed JSON content of a file, or null if it is missing or invalid. */
function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (_) {
    return null;
  }
}

/** Makes a server-supplied filename safe for every desktop file system. */
function sanitizeFilename(name) {
  let base = path.basename(String(name || '').replace(/\\/g, '/'));
  base = base.replace(/[\u0000-\u001f\u007f<>:"/\\|?*]/g, '_').replace(/^[\s.]+|[\s.]+$/g, '');
  if (/^(con|prn|aux|nul|com\d|lpt\d)(\..*)?$/i.test(base)) base = `_${base}`;
  if (base.length > 200) {
    const ext = path.extname(base).slice(0, 20);
    base = base.slice(0, 200 - ext.length) + ext;
  }
  return base || 'download';
}

/** First "name.ext", "name (1).ext", "name (2).ext"... that exists neither on disk nor in `reserved`. */
function uniquePath(dir, filename, reserved = new Set()) {
  const ext = path.extname(filename);
  const stem = filename.slice(0, filename.length - ext.length);
  let candidate = path.join(dir, filename);
  for (let i = 1; fs.existsSync(candidate) || reserved.has(candidate); i++) {
    candidate = path.join(dir, `${stem} (${i})${ext}`);
  }
  return candidate;
}

module.exports = { writeFileAtomic, readJson, sanitizeFilename, uniquePath };
