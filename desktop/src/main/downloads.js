'use strict';

// Downloads: everything the web contents cannot display is saved straight into the OS
// Downloads folder under a unique, non-clobbering name. Progress is broadcast to every
// window's chrome UI (downloads popover + badge).

const { app, shell } = require('electron');
const { EventEmitter } = require('node:events');
const fs = require('node:fs');
const path = require('node:path');
const { sanitizeFilename, uniquePath } = require('./fsutil');

const MAX_ENTRIES = 100;
const PROGRESS_INTERVAL_MS = 250;

class DownloadManager extends EventEmitter {
  constructor() {
    super();
    this.entries = new Map(); // id -> { id, item, info }
    this.reserved = new Set();
    this.nextId = 1;
  }

  attach(ses) {
    ses.on('will-download', (_event, item) => this.onWillDownload(item));
  }

  onWillDownload(item) {
    const dir = app.getPath('downloads');
    try {
      fs.mkdirSync(dir, { recursive: true });
    } catch (_) { /* reported by the item as interrupted */ }
    const savePath = uniquePath(dir, sanitizeFilename(item.getFilename()), this.reserved);
    this.reserved.add(savePath);
    item.setSavePath(savePath); // no save dialog

    const id = this.nextId++;
    const entry = {
      id,
      item,
      lastEmit: 0,
      info: {
        id,
        filename: path.basename(savePath),
        savePath,
        url: item.getURL().slice(0, 2048),
        receivedBytes: 0,
        totalBytes: item.getTotalBytes(),
        state: 'progressing',
        paused: false,
        startTime: Date.now(),
      },
    };
    this.entries.set(id, entry);
    this.trim();

    item.on('updated', (_e, state) => {
      Object.assign(entry.info, {
        state: state === 'interrupted' ? 'interrupted' : 'progressing',
        paused: item.isPaused(),
        receivedBytes: item.getReceivedBytes(),
        totalBytes: item.getTotalBytes(),
      });
      const now = Date.now();
      if (now - entry.lastEmit >= PROGRESS_INTERVAL_MS) {
        entry.lastEmit = now;
        this.changed();
      }
    });
    item.once('done', (_e, state) => {
      this.reserved.delete(savePath);
      Object.assign(entry.info, {
        state, // completed | cancelled | interrupted
        paused: false,
        receivedBytes: item.getReceivedBytes(),
        totalBytes: item.getTotalBytes() || item.getReceivedBytes(),
      });
      entry.item = null;
      this.changed();
      this.emit('done', { ...entry.info });
    });
    this.emit('started', { ...entry.info });
    this.changed();
  }

  trim() {
    if (this.entries.size <= MAX_ENTRIES) return;
    for (const [id, entry] of this.entries) {
      if (this.entries.size <= MAX_ENTRIES) break;
      if (!entry.item) this.entries.delete(id);
    }
  }

  changed() {
    this.emit('changed', this.list());
  }

  list() {
    return [...this.entries.values()].map((e) => ({ ...e.info })).reverse();
  }

  get(id) {
    return this.entries.get(id) || null;
  }

  cancel(id) {
    const entry = this.get(id);
    if (entry && entry.item) entry.item.cancel();
  }

  togglePause(id) {
    const entry = this.get(id);
    if (!entry || !entry.item) return;
    if (entry.item.isPaused()) {
      if (entry.item.canResume()) entry.item.resume();
    } else {
      entry.item.pause();
    }
    entry.info.paused = entry.item.isPaused();
    this.changed();
  }

  open(id) {
    const entry = this.get(id);
    if (entry && entry.info.state === 'completed' && fs.existsSync(entry.info.savePath)) {
      shell.openPath(entry.info.savePath).catch(() => {});
    }
  }

  showInFolder(id) {
    const entry = this.get(id);
    if (!entry) return;
    if (fs.existsSync(entry.info.savePath)) shell.showItemInFolder(entry.info.savePath);
    else shell.openPath(app.getPath('downloads')).catch(() => {});
  }

  openFolder() {
    shell.openPath(app.getPath('downloads')).catch(() => {});
  }

  /** Removes finished entries from the list (files stay on disk). */
  clearFinished() {
    for (const [id, entry] of this.entries) if (!entry.item) this.entries.delete(id);
    this.changed();
  }
}

module.exports = { DownloadManager };
