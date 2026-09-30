'use strict';

// Preload for `npm run dist:win:cross` (Windows installers built on Linux, no Wine).
//
// To embed the uninstaller in the NSIS installer, electron-builder first compiles an
// "uninstaller writer" and, on Linux, runs it under Wine. On macOS it instead extracts the
// uninstaller with its own pure-JavaScript reader (UninstallerReader). The only switch between
// the two paths is isMacOsCatalina(), which is used nowhere else, so this makes electron-builder
// take the Wine-free path on Linux too. If electron-builder's internals change, the build simply
// behaves as before (and asks for Wine).

if (process.platform !== 'win32') {
  try {
    const macosVersion = require('app-builder-lib/out/util/macosVersion');
    if (typeof macosVersion.isMacOsCatalina === 'function') macosVersion.isMacOsCatalina = () => true;
  } catch (err) {
    console.warn(`nsis-without-wine: not applied (${err.message}); the NSIS target may need Wine.`);
  }
}
