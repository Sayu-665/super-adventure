# OpenSurf for desktop

A simple, fast, private web browser for unrestricted web search, for Windows, macOS and Linux
(Electron). No accounts, no telemetry, no ads, and no content filtering by the app. SafeSearch
is off by default.

## Build and run

```sh
npm install                 # the Electron binary is downloaded on first use
npm start                   # run from source
npm test                    # unit tests (node:test)
xvfb-run -a npm run test:smoke   # Playwright end-to-end tests (Linux, headless)

npm run dist                # installers for the current OS
npm run dist:linux          # dist/OpenSurf-<version>-x86_64.AppImage + dist/opensurf_<version>_amd64.deb
npm run dist:win            # Windows, on Windows: NSIS installer + portable .exe
npm run dist:win:cross      # the same Windows targets, cross-built on Linux without Wine
npm run dist:mac            # dmg + zip (on macOS)
npm run icons               # regenerate build/icon.png, build/icon.ico, src/assets/icon.png from ../assets/logo.svg
```

`OPENSURF_USER_DATA_DIR=/some/dir` runs the app with an isolated profile. The end-to-end tests
pass `--no-sandbox` only because the CI container runs as root; the app never disables the
sandbox on its own.

`dist:win:cross` preloads `scripts/nsis-without-wine.cjs`. Without it, electron-builder runs
Wine on Linux to extract the NSIS uninstaller. The preload switches it to the pure-JavaScript
extractor that electron-builder already uses on macOS. Because `signAndEditExecutable=false`,
the cross-built `OpenSurf.exe` keeps Electron's default icon and version info. Windows builds
made on Windows (as in CI) have the full metadata.

Extra checks, not part of `npm test`:

- `xvfb-run -a node scripts/visual-check.mjs [--packaged]` saves light and dark screenshots to
  `test-output/`. It fails if the app prints unexpected errors.
- `xvfb-run -a node scripts/privacy-check.mjs [--packaged]` sends every connection through a
  logging proxy. It fails if the app contacts anything the user did not open.

## Using it

- **Omnibox.** Type a URL or a search. `example.com` opens `https://example.com`, and
  `localhost:8080`, IPv4 addresses and `[::1]` open over `http://`. Anything else is searched
  with the selected engine. Other schemes such as `javascript:` or `data:` are searched and
  never executed. You can type `file://` URLs.
- **Search engines.** DuckDuckGo (default), Google, Bing, Brave Search, Startpage, Mojeek, or
  a custom `https://…%s…` template. SafeSearch is off by default and can be turned on in
  Settings. It has no effect on custom templates.
- **JavaScript toggle.** It applies to new tabs straight away. Open tabs keep their setting
  until you click **Apply to open tabs** in Settings. That rebuilds each tab, keeping its
  back/forward history, and reloads it.
- **Restore tabs on startup** (on by default). The URLs of open tabs are saved in the profile,
  and nothing else is saved.
- **Clear browsing data** removes cookies, cache, all site storage, back/forward history,
  recently closed tabs, the downloads list and the permission and certificate decisions for
  this session.
- **Downloads.** Files that can't be shown are saved to the OS Downloads folder without a save
  dialog, under a unique name such as `file (1).pdf`. The downloads button shows progress, and
  its list lets you pause, cancel, open a file or show it in its folder.
- **Keyboard shortcuts.** These work in the toolbar and inside pages:

  | Action | Shortcut |
  | --- | --- |
  | New tab / close tab / reopen closed tab | Ctrl/Cmd+T / Ctrl/Cmd+W / Ctrl/Cmd+Shift+T |
  | Next / previous tab, go to tab N | Ctrl+Tab / Ctrl+Shift+Tab, Ctrl/Cmd+1…8 (9 = last) |
  | Focus the address bar | Ctrl/Cmd+L, F6, Alt+D |
  | Reload / hard reload | Ctrl/Cmd+R or F5 / Ctrl/Cmd+Shift+R |
  | Back / forward | Alt+Left/Right (Cmd+[ / Cmd+] on macOS) |
  | Find in page | Ctrl/Cmd+F, F3 or Ctrl/Cmd+G for next match, Esc to close |
  | Zoom in / out / reset | Ctrl/Cmd + / - / 0 |
  | Full screen | F11 (Ctrl+Cmd+F on macOS) |
  | Developer tools | F12, Ctrl/Cmd+Shift+I |
  | New window / downloads / settings | Ctrl/Cmd+N, Ctrl/Cmd+J, Ctrl/Cmd+, |

## Architecture

```
src/
  main/            Electron main process (CommonJS)
    main.js            lifecycle, single instance, command-line / open-url hand-off, sandbox
    browser.js         app-wide state: settings, downloads, restore tabs, clear data, windows
    window-controller.js  one BrowserWindow: chrome UI webContents + tab layout, find, zoom, overlays
    tab.js             one tab = one sandboxed WebContentsView (no preload); navigation guards,
                       window.open handling, load state, favicon, error/crash state
    commands.js        every action, with argument validation (used by IPC, shortcuts, menus)
    ipc.js             validates the sender (the window's chrome main frame) and runs commands
    keyboard.js        before-input-event -> shared/shortcuts.js -> command
    protocol.js        opensurf:// scheme: serves src/home (home-files.js) from the asar
    security.js        permission prompts, certificate errors, device pickers, external schemes
    downloads.js       will-download -> Downloads folder, progress broadcast
    settings.js        validated settings.json with atomic writes (fsutil.js)
    session-store.js   restore-tabs file
    context-menu.js    page / tab / tab-list / text-field context menus
    app-menu.js        macOS application menu (no menu bar on Windows/Linux)
    urls.js, favicon.js, registry.js, home-files.js, fsutil.js   small helpers
  shared/
    omnibox.js         pure URL/search resolution and engine table (unit tested)
    shortcuts.js       pure key -> command map (unit tested)
  preload/chrome-preload.js   minimal typed contextBridge API for the chrome UI only
  renderer/          chrome UI: tab strip, toolbar, find bar, menu/downloads/settings/about popovers
  home/              the home page, served at opensurf://home/
  assets/            window icon and logo
```

- The chrome UI is the window's own webContents. Each tab is a `WebContentsView` placed below
  it, and only the active tab is visible. A tab view is native and draws above the chrome UI,
  so while a popover is open the page is shown as a still screenshot and the view is hidden.
- The home page form submits to `opensurf://go?q=…`. The main process intercepts that and
  resolves it with the same omnibox function, so `file:` is never allowed from pages. The home
  page gets display info only through its URL fragment (`#engine=DuckDuckGo&safe=off`), and it
  has no bridge or API.
- Web content has no Node.js. `sandbox` and `contextIsolation` are on, `nodeIntegration` is off
  and `app.enableSandbox()` is called. Pages can't navigate to `file:`, `chrome:` or other
  internal schemes. `mailto:`, `tel:`, `sms:`, `magnet:` and similar links go to the OS only
  after the user confirms. Certificate errors default to Cancel.
- Privacy: OpenSurf makes no telemetry, crash-report, update or remote-config requests. The
  user agent is plain Chromium, without the Electron or app tokens. Spellcheck dictionaries are
  never downloaded.

## Known limitations

- Where Chromium uses its Hunspell spellchecker (Linux), no dictionaries are downloaded, because
  that would contact Google's CDN. You can place `.bdic` files in `<profile>/Dictionaries`.
  macOS uses the system spellchecker.
- Pages that use `window.open` get a normal new tab without an `opener` reference, so some
  pop-up sign-in flows may not complete. HTTP authentication prompts and client certificates
  are not supported: the request is cancelled and no certificate is sent.
- PDFs download instead of opening in a built-in viewer.
- Geolocation, which is asked for per site, uses Chromium's network location provider. It only
  runs after the user allows a site.
