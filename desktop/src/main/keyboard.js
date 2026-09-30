'use strict';

// Keyboard shortcuts in the main process: 'before-input-event' of the chrome UI and every tab,
// so they work wherever the focus is (see shared/shortcuts.js for the key map).

const { matchShortcut } = require('../shared/shortcuts');

/** Returns true when the key was consumed. */
function handleInput(ctrl, input, source) {
  if (!ctrl || ctrl.destroyed) return false;
  const tab = ctrl.activeTab();
  const match = matchShortcut(input, {
    platform: process.platform,
    source,
    findOpen: ctrl.findOpen,
    loading: Boolean(tab && tab.loading),
    htmlFullscreen: Boolean(ctrl.htmlFullscreenTab),
  });
  if (!match) return false;
  require('./commands').exec(ctrl, match.command, match.arg);
  return true;
}

module.exports = { handleInput };
