'use strict';

// Persisted user settings: validated JSON in userData, written atomically (temp file + rename).

const { EventEmitter } = require('node:events');
const omnibox = require('../shared/omnibox');
const { writeFileAtomic, readJson } = require('./fsutil');

const DEFAULTS = Object.freeze({
  engine: omnibox.DEFAULT_ENGINE,
  safeSearch: false,
  customTemplate: '',
  javascript: true,
  restoreTabs: true,
});

const MAX_TEMPLATE_LENGTH = 2048;

/** Returns a complete, valid settings object; invalid or missing fields take their default. */
function sanitize(raw) {
  const src = raw && typeof raw === 'object' && !Array.isArray(raw) ? raw : {};
  const out = { ...DEFAULTS };
  if (omnibox.ENGINE_IDS.includes(src.engine)) out.engine = src.engine;
  if (typeof src.safeSearch === 'boolean') out.safeSearch = src.safeSearch;
  if (typeof src.customTemplate === 'string' && src.customTemplate.length <= MAX_TEMPLATE_LENGTH) {
    out.customTemplate = src.customTemplate.trim();
  }
  if (typeof src.javascript === 'boolean') out.javascript = src.javascript;
  if (typeof src.restoreTabs === 'boolean') out.restoreTabs = src.restoreTabs;
  // "custom" is only usable with a valid template.
  if (out.engine === 'custom' && !omnibox.validateCustomTemplate(out.customTemplate).ok) out.engine = DEFAULTS.engine;
  return out;
}

class SettingsStore extends EventEmitter {
  constructor(file) {
    super();
    this.file = file;
    this.values = sanitize(readJson(file));
  }

  get() {
    return { ...this.values };
  }

  /** Options for the omnibox module. */
  searchOptions(extra) {
    const { engine, safeSearch, customTemplate } = this.values;
    return { engine, safeSearch, customTemplate, ...extra };
  }

  /**
   * Applies a partial update after validation.
   * @returns {{ok: true, settings: object} | {ok: false, error: string, field: string}}
   */
  update(patch) {
    if (!patch || typeof patch !== 'object' || Array.isArray(patch)) return { ok: false, error: 'Invalid settings.', field: '' };
    const next = { ...this.values };
    for (const [key, value] of Object.entries(patch)) {
      switch (key) {
        case 'engine':
          if (!omnibox.ENGINE_IDS.includes(value)) return { ok: false, error: 'Unknown search engine.', field: key };
          next.engine = value;
          break;
        case 'customTemplate': {
          if (typeof value !== 'string' || value.length > MAX_TEMPLATE_LENGTH) return { ok: false, error: 'Invalid template.', field: key };
          const trimmed = value.trim();
          if (trimmed) {
            const check = omnibox.validateCustomTemplate(trimmed);
            if (!check.ok) return { ok: false, error: check.error, field: key };
          }
          next.customTemplate = trimmed;
          break;
        }
        case 'safeSearch':
        case 'javascript':
        case 'restoreTabs':
          if (typeof value !== 'boolean') return { ok: false, error: `Invalid value for ${key}.`, field: key };
          next[key] = value;
          break;
        default:
          return { ok: false, error: `Unknown setting "${key}".`, field: key };
      }
    }
    if (next.engine === 'custom') {
      const check = omnibox.validateCustomTemplate(next.customTemplate);
      if (!check.ok) return { ok: false, error: check.error, field: 'customTemplate' };
    }
    const changed = Object.keys(next).filter((k) => next[k] !== this.values[k]);
    this.values = next;
    if (changed.length) {
      this.save();
      this.emit('changed', this.get(), changed);
    }
    return { ok: true, settings: this.get() };
  }

  save() {
    try {
      writeFileAtomic(this.file, JSON.stringify(this.values, null, 2) + '\n');
    } catch (err) {
      console.error('OpenSurf: could not save settings:', err.message);
    }
  }
}

module.exports = { SettingsStore, DEFAULTS, sanitize };
