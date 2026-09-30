/*
 * OpenSurf keyboard shortcuts: maps a key event to a command name. Pure; shared by the main
 * process (before-input-event on the chrome UI and every tab) and the chrome UI renderer
 * (keys that reach the DOM). Works as CommonJS and as a classic <script> (OpenSurfShortcuts).
 */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.OpenSurfShortcuts = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  /** Layout-aware key name: Latin letters from `key` (fallback to the physical key), digits from `code`. */
  function keyOf(input) {
    const key = input.key || '';
    const code = input.code || '';
    if (key.length === 1 && /[a-z]/i.test(key)) return key.toLowerCase();
    if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
    if (/^(Digit|Numpad)\d$/.test(code)) return code.slice(-1);
    return key;
  }

  const cmd = (command, arg) => ({ command, arg });

  /**
   * @param {{type: string, key: string, code?: string, shift?: boolean, control?: boolean, alt?: boolean, meta?: boolean}} input
   * @param {{platform?: string, source?: 'chrome'|'tab', findOpen?: boolean, loading?: boolean, htmlFullscreen?: boolean}} ctx
   * @returns {{command: string, arg?: any} | null}
   */
  function matchShortcut(input, ctx = {}) {
    if (!input || (input.type !== 'keyDown' && input.type !== 'rawKeyDown')) return null;
    const mac = ctx.platform === 'darwin';
    const k = keyOf(input);
    const { shift = false, control = false, alt = false, meta = false } = input;
    const mod = mac ? meta : control; // Cmd on macOS, Ctrl elsewhere
    const accel = mod && !alt && (mac ? !control : !meta);
    const noMods = !control && !alt && !meta;

    // Tab cycling
    if (control && !alt && !meta && k === 'Tab') return cmd(shift ? 'tab.prev' : 'tab.next');
    if (!mac && control && !alt && !meta && !shift && k === 'PageDown') return cmd('tab.next');
    if (!mac && control && !alt && !meta && !shift && k === 'PageUp') return cmd('tab.prev');
    if (mac && meta && alt && !control && !shift && k === 'ArrowRight') return cmd('tab.next');
    if (mac && meta && alt && !control && !shift && k === 'ArrowLeft') return cmd('tab.prev');
    if (mac && meta && shift && !alt && !control && (k === ']' || k === '}')) return cmd('tab.next');
    if (mac && meta && shift && !alt && !control && (k === '[' || k === '{')) return cmd('tab.prev');

    if (accel) {
      // Zoom (Shift is allowed so that "Ctrl +" works on layouts where "+" needs Shift)
      if (k === '+' || k === '=' || input.code === 'NumpadAdd') return cmd('zoom.in');
      if (k === '-' || k === '_' || input.code === 'NumpadSubtract') return cmd('zoom.out');
      if (!shift && k === '0') return cmd('zoom.reset');

      if (!shift) {
        if (/^[1-8]$/.test(k)) return cmd('tab.select', Number(k) - 1);
        if (k === '9') return cmd('tab.select', -1);
        switch (k) {
          case 't': return cmd('tab.new');
          case 'w': return cmd('tab.close');
          case 'n': return cmd('window.new');
          case 'l': return cmd('omnibox.focus');
          case 'r': return cmd('nav.reload');
          case 'f': return cmd('find.open');
          case 'g': return cmd('find.step', true);
          case 'j': return cmd('ui.downloads');
          case ',': return cmd('ui.settings');
          case '[': return mac ? cmd('nav.back') : null;
          case ']': return mac ? cmd('nav.forward') : null;
          default: break;
        }
      } else {
        switch (k) {
          case 't': return cmd('tab.reopen');
          case 'w': return cmd('window.close');
          case 'r': return cmd('nav.hardReload');
          case 'i': return cmd('devtools.toggle');
          case 'g': return cmd('find.step', false);
          case 'Delete':
          case 'Backspace': return cmd('ui.clearData');
          default: break;
        }
      }
    }

    if (mac && meta && alt && !control && !shift && k === 'i') return cmd('devtools.toggle');
    if (mac && meta && control && !alt && k === 'f') return cmd('window.fullscreen');
    if (mac && meta && shift && !alt && !control && k === 'h') return cmd('nav.home');

    if (!mac && alt && !control && !meta && !shift) {
      if (k === 'ArrowLeft') return cmd('nav.back');
      if (k === 'ArrowRight') return cmd('nav.forward');
      if (k === 'd') return cmd('omnibox.focus');
      if (k === 'Home') return cmd('nav.home');
    }
    if (!mac && control && !alt && !meta && !shift && k === 'F4') return cmd('tab.close');

    if (k === 'F5' && !alt && !meta) return cmd(shift || control ? 'nav.hardReload' : 'nav.reload');
    if (noMods && !shift) {
      switch (k) {
        case 'F6': return cmd('omnibox.focus');
        case 'F11': return mac ? null : cmd('window.fullscreen');
        case 'F12': return cmd('devtools.toggle');
        case 'BrowserBack': return cmd('nav.back');
        case 'BrowserForward': return cmd('nav.forward');
        case 'BrowserRefresh': return cmd('nav.reload');
        case 'BrowserHome': return cmd('nav.home');
        case 'BrowserStop': return cmd('nav.stop');
        default: break;
      }
    }
    if (noMods && k === 'F3') return cmd('find.step', !shift);

    // Escape inside a page: close the find bar, else stop loading. Otherwise the page gets it.
    if (noMods && !shift && k === 'Escape' && ctx.source === 'tab' && !ctx.htmlFullscreen) {
      if (ctx.findOpen) return cmd('find.close', { focusPage: true });
      if (ctx.loading) return cmd('nav.stop');
    }
    return null;
  }

  return Object.freeze({ matchShortcut, keyOf });
});
