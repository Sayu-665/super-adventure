'use strict';

// OpenSurf chrome UI: renders tabs/toolbar/popovers from state pushed by the main process
// and forwards user actions through the preload API (window.opensurf).
(function () {
  const api = window.opensurf;
  const Icons = window.OpenSurfIcons;
  const Omnibox = window.OpenSurfOmnibox;
  const Shortcuts = window.OpenSurfShortcuts;
  const $ = (id) => document.getElementById(id);

  const els = {
    top: $('top'),
    tabs: $('tabs'),
    newTab: $('new-tab'),
    tabCount: $('tab-count'),
    tabCountNum: $('tab-count-num'),
    back: $('btn-back'),
    forward: $('btn-forward'),
    reload: $('btn-reload'),
    home: $('btn-home'),
    form: $('omnibox-form'),
    omnibox: $('omnibox'),
    siteIcon: $('site-icon'),
    chip: $('omnibox-chip'),
    zoomBadge: $('zoom-badge'),
    copy: $('btn-copy'),
    downloadsBtn: $('btn-downloads'),
    downloadsBadge: $('downloads-badge'),
    ring: $('ring-value'),
    menuBtn: $('btn-menu'),
    progress: $('progress-bar'),
    findbar: $('findbar'),
    findInput: $('find-input'),
    findCount: $('find-count'),
    backdrop: $('backdrop'),
    scrim: $('scrim'),
    errorPanel: $('error-panel'),
    menuZoom: $('menu-zoom'),
    downloadsList: $('downloads-list'),
    downloadsEmpty: $('downloads-empty'),
    engine: $('set-engine'),
    customField: $('custom-field'),
    template: $('set-template'),
    templateMsg: $('template-msg'),
    safe: $('set-safe'),
    safeHelp: $('safe-help'),
    js: $('set-js'),
    jsApplyRow: $('js-apply-row'),
    restore: $('set-restore'),
    settingsStatus: $('settings-status'),
  };

  const POPOVERS = {
    menu: { el: $('menu-popover'), anchor: els.menuBtn, modal: false },
    downloads: { el: $('downloads-popover'), anchor: els.downloadsBtn, modal: false },
    settings: { el: $('settings-dialog'), modal: true },
    'clear-data': { el: $('clear-dialog'), modal: true },
    about: { el: $('about-dialog'), modal: true },
  };

  const state = {
    platform: 'linux',
    tabs: [],
    activeId: null,
    settings: null,
    downloads: [],
    canReopenClosedTab: false,
    popover: null,
    overlay: false,
    omniboxDirty: false,
    unseenDownload: false,
  };
  let popoverSeq = 0;

  const activeTab = () => state.tabs.find((t) => t.id === state.activeId) || null;
  const isMac = () => state.platform === 'darwin';

  // ---------------------------------------------------------------- helpers

  function formatBytes(n) {
    if (!Number.isFinite(n) || n <= 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.min(units.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
    const v = n / 1024 ** i;
    return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
  }

  function kbdLabel(spec) {
    if (!isMac()) return spec;
    const special = { F12: '⌥⌘I', 'Ctrl+Shift+Del': '⇧⌘⌫' };
    if (special[spec]) return special[spec];
    return spec.replace('Ctrl+Shift+', '⇧⌘').replace('Ctrl+', '⌘');
  }

  let chipTimer = 0;
  function showChip(text, ms = 1800) {
    els.chip.textContent = text;
    els.chip.hidden = false;
    clearTimeout(chipTimer);
    chipTimer = setTimeout(() => {
      els.chip.hidden = true;
    }, ms);
  }

  // ---------------------------------------------------------------- tabs

  function createTabEl(id) {
    const el = document.createElement('div');
    el.className = 'tab';
    el.dataset.id = String(id);
    el.setAttribute('role', 'tab');
    el.draggable = true;
    el.innerHTML =
      '<span class="tab-icon"></span><span class="tab-title"></span>' +
      `<button class="tab-audio" tabindex="-1" hidden></button>` +
      `<button class="tab-close" tabindex="-1" aria-label="Close tab" title="Close tab">${Icons.svg('close')}</button>`;
    return el;
  }

  function updateTabEl(el, tab, narrow) {
    el.classList.toggle('active', tab.id === state.activeId);
    el.classList.toggle('narrow', narrow);
    el.setAttribute('aria-selected', String(tab.id === state.activeId));
    el.title = tab.displayUrl ? `${tab.title}\n${tab.displayUrl}` : tab.title;

    const title = el.querySelector('.tab-title');
    if (title.textContent !== tab.title) title.textContent = tab.title;

    const icon = el.querySelector('.tab-icon');
    const iconKey = tab.loading ? 'loading' : tab.error ? 'error' : tab.favicon || 'globe';
    if (icon.dataset.key !== iconKey) {
      icon.dataset.key = iconKey;
      icon.textContent = '';
      if (tab.loading) {
        icon.innerHTML = '<span class="spinner"></span>';
      } else if (tab.error) {
        icon.innerHTML = Icons.svg(tab.error.kind === 'certificate' ? 'warning' : 'info');
      } else if (tab.favicon) {
        const img = document.createElement('img');
        img.alt = '';
        img.src = tab.favicon;
        img.onerror = () => {
          icon.dataset.key = 'globe';
          icon.innerHTML = Icons.svg('globe');
        };
        icon.appendChild(img);
      } else {
        icon.innerHTML = Icons.svg('globe');
      }
    }

    const audio = el.querySelector('.tab-audio');
    const showAudio = tab.audible || tab.muted;
    audio.hidden = !showAudio;
    if (showAudio) {
      const key = tab.muted ? 'muted' : 'speaker';
      if (audio.dataset.key !== key) {
        audio.dataset.key = key;
        audio.innerHTML = Icons.svg(key);
        audio.title = tab.muted ? 'Unmute tab' : 'Mute tab';
        audio.setAttribute('aria-label', audio.title);
      }
    }
  }

  function renderTabs() {
    const container = els.tabs;
    const existing = new Map([...container.children].map((el) => [Number(el.dataset.id), el]));
    const available = container.parentElement.clientWidth - 140;
    const narrow = state.tabs.length > 0 && available / state.tabs.length < 96;
    state.tabs.forEach((tab, i) => {
      let el = existing.get(tab.id);
      if (el) existing.delete(tab.id);
      else el = createTabEl(tab.id);
      updateTabEl(el, tab, narrow);
      if (container.children[i] !== el) container.insertBefore(el, container.children[i] || null);
    });
    for (const el of existing.values()) el.remove();
    els.tabCountNum.textContent = String(state.tabs.length);
    els.tabCount.title = `${state.tabs.length} open ${state.tabs.length === 1 ? 'tab' : 'tabs'}`;
    els.tabCount.setAttribute('aria-label', els.tabCount.title);
    const activeEl = container.querySelector('.tab.active');
    if (activeEl) activeEl.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  function tabIdFromEvent(e) {
    const el = e.target.closest('.tab');
    return el ? Number(el.dataset.id) : null;
  }

  els.tabs.addEventListener('mousedown', (e) => {
    const id = tabIdFromEvent(e);
    if (id === null) return;
    if (e.button === 1) e.preventDefault(); // no autoscroll on middle click
    if (e.button === 0 && !e.target.closest('button')) api.activateTab(id);
  });
  els.tabs.addEventListener('auxclick', (e) => {
    const id = tabIdFromEvent(e);
    if (id !== null && e.button === 1) api.closeTab(id);
  });
  els.tabs.addEventListener('click', (e) => {
    const id = tabIdFromEvent(e);
    if (id === null) return;
    if (e.target.closest('.tab-close')) api.closeTab(id);
    else if (e.target.closest('.tab-audio')) api.toggleMute(id);
  });
  els.tabs.addEventListener('contextmenu', (e) => {
    const id = tabIdFromEvent(e);
    if (id === null) return;
    e.preventDefault();
    api.showTabMenu(id);
  });
  els.tabs.addEventListener('wheel', (e) => {
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      els.tabs.scrollLeft += e.deltaY;
      e.preventDefault();
    }
  }, { passive: false });
  $('tabstrip').addEventListener('dblclick', (e) => {
    if (e.target === $('tabstrip') || e.target.classList.contains('grow') || e.target === els.tabs) api.newTab();
  });

  // Drag to reorder
  let dragId = null;
  els.tabs.addEventListener('dragstart', (e) => {
    const id = tabIdFromEvent(e);
    if (id === null) return;
    dragId = id;
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/x-opensurf-tab', String(id));
    e.target.closest('.tab').classList.add('dragging');
  });
  els.tabs.addEventListener('dragover', (e) => {
    if (dragId !== null) e.preventDefault();
  });
  els.tabs.addEventListener('drop', (e) => {
    if (dragId === null) return;
    e.preventDefault();
    const others = [...els.tabs.children].filter((el) => Number(el.dataset.id) !== dragId);
    let index = others.length;
    for (let i = 0; i < others.length; i++) {
      const r = others[i].getBoundingClientRect();
      if (e.clientX < r.left + r.width / 2) {
        index = i;
        break;
      }
    }
    api.moveTab(dragId, index);
  });
  els.tabs.addEventListener('dragend', () => {
    dragId = null;
    for (const el of els.tabs.querySelectorAll('.dragging')) el.classList.remove('dragging');
  });

  els.newTab.addEventListener('click', () => api.newTab());
  els.tabCount.addEventListener('click', () => api.showTabList());

  // ---------------------------------------------------------------- toolbar

  let reloadMode = '';
  let trickle = 0;
  let trickleTimer = 0;

  function renderToolbar() {
    const tab = activeTab();
    if (!tab) return;
    els.back.disabled = !tab.canGoBack;
    els.forward.disabled = !tab.canGoForward;

    const mode = tab.loading ? 'stop' : 'reload';
    if (mode !== reloadMode) {
      reloadMode = mode;
      els.reload.innerHTML = Icons.svg(mode === 'stop' ? 'close' : 'reload');
      els.reload.title = mode === 'stop' ? 'Stop loading (Esc)' : `Reload (${kbdLabel('Ctrl+R')})`;
      els.reload.setAttribute('aria-label', mode === 'stop' ? 'Stop' : 'Reload');
    }

    // Site / security indicator
    let icon = 'search';
    let cls = '';
    let title = 'Search or type a URL';
    if (!tab.isHome) {
      if (tab.error && tab.error.kind === 'certificate') {
        icon = 'warning'; cls = 'insecure'; title = 'Connection is not private';
      } else if (tab.scheme === 'https') {
        icon = 'lock'; cls = 'secure'; title = 'Connection is secure (HTTPS)';
      } else if (tab.scheme === 'http') {
        icon = 'info'; cls = 'insecure'; title = 'Not secure: this page does not use HTTPS';
      } else if (tab.scheme === 'file') {
        icon = 'file'; title = 'Local file';
      } else {
        icon = 'globe'; title = '';
      }
    }
    const key = `${icon}|${cls}`;
    if (els.siteIcon.dataset.key !== key) {
      els.siteIcon.dataset.key = key;
      els.siteIcon.innerHTML = Icons.svg(icon);
      els.siteIcon.className = `site-icon ${cls}`;
    }
    els.siteIcon.title = title;

    syncOmnibox();
    els.copy.hidden = tab.isHome;
    els.zoomBadge.hidden = tab.zoom === 100;
    els.zoomBadge.textContent = `${tab.zoom}%`;
    els.menuZoom.textContent = `${tab.zoom}%`;
    renderProgress(tab);
    renderError(tab);
    for (const btn of document.querySelectorAll('[data-action="reopen-tab"]')) btn.disabled = !state.canReopenClosedTab;
    for (const btn of document.querySelectorAll('[data-action="copy-link"]')) btn.disabled = tab.isHome;
  }

  function renderProgress(tab) {
    const bar = els.progress;
    if (tab.loading) {
      if (!bar.classList.contains('active')) {
        bar.classList.remove('done');
        bar.style.transition = 'none';
        bar.style.width = '0';
        void bar.offsetWidth; // restart the transition
        bar.style.transition = '';
        bar.classList.add('active');
        trickle = 0;
      }
      const target = Math.max(tab.progress || 0.1, trickle);
      bar.style.width = `${Math.min(0.95, target) * 100}%`;
      if (!trickleTimer) {
        trickleTimer = setInterval(() => {
          trickle = Math.max(trickle, activeTab() ? activeTab().progress : 0);
          trickle += (0.9 - trickle) * 0.06;
          if (bar.classList.contains('active')) bar.style.width = `${Math.min(0.95, trickle) * 100}%`;
        }, 250);
      }
    } else {
      clearInterval(trickleTimer);
      trickleTimer = 0;
      if (bar.classList.contains('active')) {
        bar.classList.remove('active');
        bar.classList.add('done');
      }
    }
  }

  function renderError(tab) {
    const err = tab.error;
    els.errorPanel.hidden = !err;
    if (!err) return;
    const icon = $('error-icon');
    icon.className = `error-icon ${err.kind}`;
    icon.innerHTML = Icons.svg(err.kind === 'certificate' ? 'warning' : err.kind === 'crashed' ? 'info' : 'globe');
    $('error-title').textContent = err.title;
    $('error-desc').textContent = err.description;
    $('error-url').textContent = err.url;
    $('error-code').textContent = err.kind === 'crashed' ? `Reason: ${err.name}` : err.name;
    $('error-retry').textContent = err.kind === 'crashed' ? 'Reload' : 'Try again';
    $('error-proceed').hidden = err.kind !== 'certificate';
  }

  $('error-retry').addEventListener('click', () => state.activeId !== null && api.retry(state.activeId));
  $('error-proceed').addEventListener('click', () => state.activeId !== null && api.retryCertificate(state.activeId));

  els.back.addEventListener('click', () => api.back());
  els.forward.addEventListener('click', () => api.forward());
  els.reload.addEventListener('click', (e) => (reloadMode === 'stop' ? api.stop() : api.reload(e.shiftKey)));
  els.home.addEventListener('click', () => api.home());
  els.copy.addEventListener('click', () => api.copyUrl());
  els.zoomBadge.addEventListener('click', () => api.zoomReset());

  // ---------------------------------------------------------------- omnibox

  let mouseFocus = false;

  function syncOmnibox() {
    const tab = activeTab();
    if (!tab || state.omniboxDirty) return;
    if (els.omnibox.value !== tab.displayUrl) els.omnibox.value = tab.displayUrl;
  }

  function focusOmnibox(select) {
    els.omnibox.focus();
    if (select) els.omnibox.select();
  }

  els.omnibox.addEventListener('mousedown', () => {
    if (document.activeElement !== els.omnibox) mouseFocus = true;
  });
  els.omnibox.addEventListener('mouseup', (e) => {
    if (!mouseFocus) return;
    mouseFocus = false;
    if (els.omnibox.selectionStart === els.omnibox.selectionEnd) {
      e.preventDefault();
      els.omnibox.select();
    }
  });
  els.omnibox.addEventListener('focus', () => {
    if (!mouseFocus) els.omnibox.select();
  });
  els.omnibox.addEventListener('input', () => {
    state.omniboxDirty = true;
  });
  els.omnibox.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    const tab = activeTab();
    if (state.omniboxDirty) {
      state.omniboxDirty = false;
      syncOmnibox();
      els.omnibox.select();
    } else if (tab && tab.loading) {
      api.stop();
    } else if (tab && !tab.isHome) {
      api.focusPage();
    }
  });
  els.form.addEventListener('submit', (e) => {
    e.preventDefault();
    const text = els.omnibox.value;
    if (!text.trim()) return;
    state.omniboxDirty = false;
    api.navigate(text);
  });

  // ---------------------------------------------------------------- find bar

  function openFind() {
    els.findbar.hidden = false;
    els.findInput.focus();
    els.findInput.select();
    if (els.findInput.value) api.find(els.findInput.value, { findNext: true });
  }

  function hideFind() {
    els.findbar.hidden = true;
    els.findCount.textContent = '';
    els.findCount.classList.remove('none');
  }

  function closeFind(focusPage) {
    if (els.findbar.hidden) return;
    hideFind();
    api.closeFind(focusPage);
  }

  els.findInput.addEventListener('input', () => {
    const text = els.findInput.value;
    if (!text) {
      els.findCount.textContent = '';
      els.findCount.classList.remove('none');
    }
    api.find(text, { findNext: true });
  });
  els.findInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (els.findInput.value) api.find(els.findInput.value, { forward: !e.shiftKey, findNext: false });
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeFind(true);
    }
  });
  $('find-next').addEventListener('click', () => els.findInput.value && api.find(els.findInput.value, { forward: true }));
  $('find-prev').addEventListener('click', () => els.findInput.value && api.find(els.findInput.value, { forward: false }));
  $('find-close').addEventListener('click', () => closeFind(true));

  function onFindResult(r) {
    if (r.tabId !== state.activeId || els.findbar.hidden) return;
    const hasText = Boolean(els.findInput.value);
    els.findCount.textContent = hasText ? `${r.matches ? r.active : 0}/${r.matches}` : '';
    els.findCount.classList.toggle('none', hasText && r.matches === 0);
  }

  // ---------------------------------------------------------------- popovers & dialogs

  function positionPopover(p) {
    if (!p.anchor) return;
    const r = p.anchor.getBoundingClientRect();
    p.el.style.top = `${Math.round(r.bottom + 6)}px`;
    p.el.style.right = `${Math.max(8, Math.round(window.innerWidth - r.right))}px`;
  }

  function hideAllPopovers() {
    for (const p of Object.values(POPOVERS)) p.el.hidden = true;
    els.scrim.hidden = true;
    for (const b of [els.menuBtn, els.downloadsBtn]) b.classList.remove('active');
  }

  async function refreshBackdrop(seq) {
    let shot = null;
    try {
      shot = await api.capturePage();
    } catch (_) { /* ignore */ }
    if (seq !== popoverSeq || !shot) return false;
    els.backdrop.src = shot;
    try {
      await els.backdrop.decode();
    } catch (_) { /* ignore */ }
    if (seq !== popoverSeq) return false;
    els.backdrop.hidden = false;
    return true;
  }

  async function openPopover(name) {
    const p = POPOVERS[name];
    if (!p) return;
    if (state.popover === name) {
      closePopover();
      return;
    }
    const seq = ++popoverSeq;
    hideAllPopovers();
    state.popover = name;
    if (!state.overlay) {
      // The page view is a separate native layer above this document: show a still
      // image of it and hide the real view while the popover is open.
      state.overlay = true;
      await refreshBackdrop(seq);
      if (seq !== popoverSeq) return;
      api.setOverlay(true);
    }
    if (p.modal) els.scrim.hidden = false;
    positionPopover(p);
    p.el.hidden = false;
    if (p.anchor) p.anchor.classList.add('active');
    onPopoverShown(name);
  }

  function closePopover() {
    if (!state.popover) return;
    popoverSeq++;
    const wasModal = POPOVERS[state.popover].modal;
    hideAllPopovers();
    state.popover = null;
    if (state.overlay) {
      state.overlay = false;
      api.setOverlay(false);
      setTimeout(() => {
        if (!state.overlay) {
          els.backdrop.hidden = true;
          els.backdrop.removeAttribute('src');
        }
      }, 120);
    }
    const tab = activeTab();
    if (tab && (tab.isHome || tab.error) && (wasModal || document.activeElement === document.body)) focusOmnibox(false);
  }

  function onPopoverShown(name) {
    if (name === 'menu') {
      const first = POPOVERS.menu.el.querySelector('.menu-item');
      if (first) first.focus();
    } else if (name === 'downloads') {
      state.unseenDownload = false;
      renderDownloads();
      $('dl-clear').focus();
    } else if (name === 'settings') {
      renderSettings();
      els.settingsStatus.textContent = '';
      els.engine.focus();
    } else if (name === 'clear-data') {
      $('clear-status').textContent = '';
      $('clear-confirm').disabled = false;
      $('clear-confirm').focus();
    } else if (name === 'about') {
      api.appInfo().then((info) => {
        $('about-version').textContent = `Version ${info.version} · Electron ${info.electron} · Chromium ${info.chrome}`;
      }).catch(() => {});
      POPOVERS.about.el.querySelector('[data-close]').focus();
    }
  }

  document.addEventListener('mousedown', (e) => {
    if (!state.popover) return;
    const p = POPOVERS[state.popover];
    if (p.el.contains(e.target) || (p.anchor && p.anchor.contains(e.target))) return;
    closePopover();
  }, true);

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      if (state.popover) {
        e.preventDefault();
        closePopover();
      } else if (!els.findbar.hidden) {
        e.preventDefault();
        closeFind(true);
      } else if (activeTab() && activeTab().loading) {
        api.stop();
      }
    } else if (state.popover === 'menu' && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      e.preventDefault();
      const items = [...POPOVERS.menu.el.querySelectorAll('.menu-item:not(:disabled)')];
      const i = items.indexOf(document.activeElement);
      const next = e.key === 'ArrowDown' ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
      items[next].focus();
    }
  });

  for (const p of Object.values(POPOVERS)) {
    for (const btn of p.el.querySelectorAll('[data-close]')) btn.addEventListener('click', () => closePopover());
  }

  window.addEventListener('resize', () => {
    if (state.popover) positionPopover(POPOVERS[state.popover]);
    renderTabs();
  });

  els.menuBtn.addEventListener('click', () => openPopover('menu'));
  els.downloadsBtn.addEventListener('click', () => openPopover('downloads'));

  // ---------------------------------------------------------------- menu

  function afterZoom() {
    // The page is hidden behind the popover: refresh its picture so the zoom is visible.
    const seq = popoverSeq;
    setTimeout(() => {
      if (state.overlay && seq === popoverSeq) refreshBackdrop(seq);
    }, 150);
  }

  const MENU_ACTIONS = {
    'new-tab': () => api.newTab(),
    'new-window': () => api.newWindow(),
    'reopen-tab': () => api.reopenClosedTab(),
    'zoom-in': () => { api.zoomIn(); afterZoom(); return true; },
    'zoom-out': () => { api.zoomOut(); afterZoom(); return true; },
    'zoom-reset': () => { api.zoomReset(); afterZoom(); return true; },
    fullscreen: () => api.toggleFullscreen(),
    find: () => openFind(),
    'copy-link': () => api.copyUrl(),
    downloads: () => { openPopover('downloads'); return true; },
    settings: () => { openPopover('settings'); return true; },
    'clear-data': () => { openPopover('clear-data'); return true; },
    devtools: () => api.toggleDevTools(),
    about: () => { openPopover('about'); return true; },
  };

  POPOVERS.menu.el.addEventListener('click', (e) => {
    const btn = e.target.closest('[data-action]');
    if (!btn || btn.disabled) return;
    const action = MENU_ACTIONS[btn.dataset.action];
    if (!action) return;
    const keepOpen = btn.dataset.action.startsWith('zoom') || ['downloads', 'settings', 'clear-data', 'about'].includes(btn.dataset.action);
    if (!keepOpen) closePopover();
    action();
  });

  // ---------------------------------------------------------------- downloads

  const speedSamples = new Map(); // id -> {bytes, time, speed}
  const lastStates = new Map();

  function downloadMeta(d) {
    switch (d.state) {
      case 'completed':
        return `Completed · ${formatBytes(d.totalBytes || d.receivedBytes)}`;
      case 'cancelled':
        return 'Cancelled';
      case 'interrupted':
        return 'Failed';
      default: {
        const sample = speedSamples.get(d.id);
        const now = Date.now();
        let speed = sample ? sample.speed : 0;
        if (sample && now - sample.time >= 500) {
          speed = ((d.receivedBytes - sample.bytes) / (now - sample.time)) * 1000;
          speedSamples.set(d.id, { bytes: d.receivedBytes, time: now, speed });
        } else if (!sample) {
          speedSamples.set(d.id, { bytes: d.receivedBytes, time: now, speed: 0 });
        }
        const amount = d.totalBytes > 0 ? `${formatBytes(d.receivedBytes)} of ${formatBytes(d.totalBytes)}` : formatBytes(d.receivedBytes);
        if (d.paused) return `Paused · ${amount}`;
        return speed > 0 ? `${amount} · ${formatBytes(speed)}/s` : amount;
      }
    }
  }

  function renderDownloads() {
    const list = state.downloads;
    // Badge + ring on the toolbar button
    const active = list.filter((d) => d.state === 'progressing');
    els.downloadsBtn.classList.toggle('progressing', active.length > 0);
    const total = active.reduce((s, d) => s + (d.totalBytes || 0), 0);
    const received = active.reduce((s, d) => s + (d.totalBytes ? d.receivedBytes : 0), 0);
    const fraction = total > 0 ? received / total : 0.15;
    els.ring.style.strokeDashoffset = String(97.4 * (1 - Math.min(1, fraction)));
    for (const d of list) {
      if (lastStates.get(d.id) === 'progressing' && d.state === 'completed' && state.popover !== 'downloads') state.unseenDownload = true;
      lastStates.set(d.id, d.state);
    }
    if (active.length) {
      els.downloadsBadge.hidden = false;
      els.downloadsBadge.className = 'badge';
      els.downloadsBadge.textContent = String(active.length);
    } else if (state.unseenDownload) {
      els.downloadsBadge.hidden = false;
      els.downloadsBadge.className = 'badge dot';
      els.downloadsBadge.textContent = '';
    } else {
      els.downloadsBadge.hidden = true;
    }
    els.downloadsBtn.title = active.length ? `Downloads (${active.length} in progress)` : 'Downloads';

    if (state.popover !== 'downloads') return;
    els.downloadsEmpty.hidden = list.length > 0;
    $('dl-clear').disabled = !list.some((d) => d.state !== 'progressing');
    const ul = els.downloadsList;
    ul.textContent = '';
    for (const d of list.slice(0, 50)) {
      const li = document.createElement('li');
      li.className = `dl-item ${d.state === 'interrupted' || d.state === 'cancelled' ? 'failed' : ''}`;
      const file = document.createElement('div');
      file.className = 'dl-file';
      file.innerHTML = Icons.svg(d.state === 'completed' ? 'file' : d.state === 'progressing' ? 'download' : 'close');
      const main = document.createElement('div');
      main.className = 'dl-main';
      const name = document.createElement('button');
      name.className = `dl-name ${d.state === 'completed' ? 'link' : ''}`;
      name.textContent = d.filename;
      name.title = d.savePath;
      if (d.state === 'completed') name.addEventListener('click', () => api.openDownload(d.id));
      main.appendChild(name);
      if (d.state === 'progressing') {
        const bar = document.createElement('div');
        bar.className = `dl-bar ${d.totalBytes > 0 ? '' : 'indeterminate'}`;
        const fill = document.createElement('div');
        if (d.totalBytes > 0) fill.style.width = `${Math.min(100, (d.receivedBytes / d.totalBytes) * 100)}%`;
        bar.appendChild(fill);
        main.appendChild(bar);
      }
      const meta = document.createElement('div');
      meta.className = 'dl-meta';
      meta.textContent = downloadMeta(d);
      main.appendChild(meta);
      const actions = document.createElement('div');
      actions.className = 'dl-actions';
      const action = (icon, title, fn) => {
        const b = document.createElement('button');
        b.className = 'icon-btn small';
        b.title = title;
        b.setAttribute('aria-label', title);
        b.innerHTML = Icons.svg(icon);
        b.addEventListener('click', fn);
        actions.appendChild(b);
      };
      if (d.state === 'progressing') {
        action(d.paused ? 'play' : 'pause', d.paused ? 'Resume' : 'Pause', () => api.pauseDownload(d.id));
        action('close', 'Cancel', () => api.cancelDownload(d.id));
      } else if (d.state === 'completed') {
        action('open', 'Open file', () => api.openDownload(d.id));
        action('folder', 'Show in folder', () => api.showDownload(d.id));
      } else {
        action('folder', 'Open Downloads folder', () => api.showDownload(d.id));
      }
      li.append(file, main, actions);
      ul.appendChild(li);
    }
  }

  $('dl-clear').addEventListener('click', () => api.clearDownloads());
  $('dl-open-folder').addEventListener('click', () => api.openDownloadsFolder());

  // ---------------------------------------------------------------- settings

  for (const e of Omnibox.ENGINES.concat([Omnibox.CUSTOM_ENGINE])) {
    const opt = document.createElement('option');
    opt.value = e.id;
    opt.textContent = e.id === Omnibox.DEFAULT_ENGINE ? `${e.name} (default)` : e.name;
    els.engine.appendChild(opt);
  }

  let pendingCustom = false; // "Custom" chosen but no valid template saved yet

  function renderSettings() {
    const s = state.settings;
    if (!s) return;
    const engine = pendingCustom ? 'custom' : s.engine;
    els.engine.value = engine;
    if (document.activeElement !== els.template) els.template.value = s.customTemplate;
    els.customField.hidden = engine !== 'custom';
    els.safe.checked = s.safeSearch;
    els.safe.disabled = engine === 'custom';
    els.safeHelp.textContent = engine === 'custom'
      ? 'Not applied to custom search engines: your template is used exactly as entered.'
      : 'Off by default: results are not filtered. Turn on to ask the search engine to hide explicit results.';
    els.js.checked = s.javascript;
    els.restore.checked = s.restoreTabs;
    els.jsApplyRow.hidden = !state.tabs.some((t) => t.javascript !== s.javascript);
    validateTemplateField();
  }

  function validateTemplateField() {
    const value = els.template.value.trim();
    if (!value) {
      els.template.classList.remove('invalid');
      els.templateMsg.className = 'field-help';
      els.templateMsg.textContent = 'Use %s where the search terms go. Must start with http:// or https://';
      return false;
    }
    const check = Omnibox.validateCustomTemplate(value);
    els.template.classList.toggle('invalid', !check.ok);
    els.templateMsg.className = `field-help ${check.ok ? 'ok' : 'error'}`;
    els.templateMsg.textContent = check.ok ? `Example: ${Omnibox.buildSearchUrl('open surf', { engine: 'custom', customTemplate: value })}` : check.error;
    return check.ok;
  }

  async function saveSettings(patch) {
    try {
      const res = await api.updateSettings(patch);
      if (res && res.ok) {
        state.settings = res.settings;
        els.settingsStatus.className = 'field-help ok';
        els.settingsStatus.textContent = 'Saved';
      } else {
        els.settingsStatus.className = 'field-help error';
        els.settingsStatus.textContent = (res && res.error) || 'Could not save';
      }
    } catch (_) {
      els.settingsStatus.className = 'field-help error';
      els.settingsStatus.textContent = 'Could not save';
    }
    renderSettings();
  }

  els.engine.addEventListener('change', () => {
    const value = els.engine.value;
    if (value === 'custom') {
      if (validateTemplateField()) {
        pendingCustom = false;
        saveSettings({ engine: 'custom', customTemplate: els.template.value.trim() });
      } else {
        pendingCustom = true;
        renderSettings();
        els.template.focus();
      }
      return;
    }
    pendingCustom = false;
    saveSettings({ engine: value });
  });
  els.template.addEventListener('input', validateTemplateField);
  const commitTemplate = () => {
    const value = els.template.value.trim();
    if (value && !validateTemplateField()) return;
    if (!value && (pendingCustom || state.settings.engine === 'custom')) return;
    const patch = { customTemplate: value };
    if (pendingCustom && value) patch.engine = 'custom';
    pendingCustom = false;
    saveSettings(patch);
  };
  els.template.addEventListener('change', commitTemplate);
  els.template.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitTemplate();
    }
  });
  els.safe.addEventListener('change', () => saveSettings({ safeSearch: els.safe.checked }));
  els.js.addEventListener('change', () => saveSettings({ javascript: els.js.checked }));
  els.restore.addEventListener('change', () => saveSettings({ restoreTabs: els.restore.checked }));
  $('js-apply').addEventListener('click', () => {
    api.applyJavaScriptToOpenTabs();
    els.jsApplyRow.hidden = true;
  });
  $('set-clear').addEventListener('click', () => openPopover('clear-data'));

  // ---------------------------------------------------------------- clear data

  $('clear-confirm').addEventListener('click', async () => {
    const btn = $('clear-confirm');
    const status = $('clear-status');
    btn.disabled = true;
    status.className = 'field-help';
    status.textContent = 'Clearing…';
    try {
      await api.clearBrowsingData();
      status.className = 'field-help ok';
      status.textContent = 'Browsing data cleared';
      setTimeout(() => {
        if (state.popover === 'clear-data') closePopover();
        showChip('Browsing data cleared');
      }, 600);
    } catch (_) {
      status.className = 'field-help error';
      status.textContent = 'Could not clear all data';
      btn.disabled = false;
    }
  });

  // ---------------------------------------------------------------- state from main

  function applyState(s) {
    const switched = s.activeId !== state.activeId;
    state.tabs = s.tabs;
    state.activeId = s.activeId;
    state.canReopenClosedTab = s.canReopenClosedTab;
    if (switched) {
      state.omniboxDirty = false;
      if (state.popover) closePopover();
    }
    renderTabs();
    renderToolbar();
    if (state.popover === 'settings') els.jsApplyRow.hidden = !state.tabs.some((t) => t.javascript !== state.settings.javascript);
  }

  function onEvent(type, payload) {
    switch (type) {
      case 'init':
        state.platform = payload.platform;
        state.settings = payload.settings;
        state.downloads = payload.downloads;
        document.documentElement.dataset.platform = payload.platform;
        for (const k of document.querySelectorAll('kbd[data-kbd]')) k.textContent = kbdLabel(k.dataset.kbd);
        els.newTab.title = `New tab (${kbdLabel('Ctrl+T')})`;
        els.back.title = `Back (${isMac() ? '⌘[' : 'Alt+Left'})`;
        els.forward.title = `Forward (${isMac() ? '⌘]' : 'Alt+Right'})`;
        els.home.title = 'Home';
        renderDownloads();
        break;
      case 'state':
        applyState(payload);
        break;
      case 'settings':
        state.settings = payload;
        if (state.popover === 'settings') renderSettings();
        break;
      case 'downloads':
        state.downloads = payload;
        renderDownloads();
        break;
      case 'download-started':
        els.downloadsBtn.classList.remove('pulse');
        void els.downloadsBtn.offsetWidth;
        els.downloadsBtn.classList.add('pulse');
        showChip(`Downloading ${payload.filename.length > 28 ? `${payload.filename.slice(0, 27)}…` : payload.filename}`, 2500);
        break;
      case 'find-result':
        onFindResult(payload);
        break;
      case 'open-find':
        if (state.popover) closePopover();
        openFind();
        break;
      case 'close-find':
        hideFind();
        break;
      case 'focus-omnibox':
        if (state.popover) closePopover();
        focusOmnibox(payload && payload.select);
        break;
      case 'open-popover':
        openPopover(payload.name);
        break;
      case 'toast':
        showChip(payload.message);
        break;
      default:
        break;
    }
  }

  // ---------------------------------------------------------------- shortcuts

  // Real key presses are handled in the main process before they reach this page; keys that
  // do arrive here (e.g. synthetic events) are forwarded when they are shortcuts.
  window.addEventListener('keydown', (e) => {
    const input = { type: 'keyDown', key: e.key, code: e.code, shift: e.shiftKey, control: e.ctrlKey, alt: e.altKey, meta: e.metaKey };
    if (!Shortcuts.matchShortcut(input, { platform: state.platform, source: 'chrome' })) return;
    e.preventDefault();
    e.stopPropagation();
    api.shortcutKey(input);
  }, true);

  // ---------------------------------------------------------------- boot

  Icons.hydrate(document);
  document.addEventListener('contextmenu', (e) => {
    if (!e.target.closest('input, textarea')) e.preventDefault();
  });
  document.addEventListener('dragover', (e) => {
    if (dragId === null) e.preventDefault();
  });
  document.addEventListener('drop', (e) => {
    if (dragId !== null) return;
    e.preventDefault();
    const text = e.dataTransfer.getData('text/uri-list') || e.dataTransfer.getData('text/plain');
    if (text && text.trim()) api.navigate(text.trim().split('\n')[0]);
  });

  let lastHeight = -1;
  const reportHeight = () => {
    const h = Math.ceil(els.top.getBoundingClientRect().height);
    if (h === lastHeight) return;
    lastHeight = h;
    document.documentElement.style.setProperty('--chrome-h', `${h}px`);
    api.setChromeHeight(h);
  };
  new ResizeObserver(reportHeight).observe(els.top);
  reportHeight();
  api.connect(onEvent);
})();
