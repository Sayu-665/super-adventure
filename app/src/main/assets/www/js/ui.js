'use strict';
// Views: tracks overview, instrument/drum/audio editors, modals.

const DPR = () => Math.min(window.devicePixelRatio || 1, 2.5);
function fitCanvas(cv) {
  const r = cv.getBoundingClientRect(), d = DPR();
  const w = Math.max(1, Math.round(r.width * d)), h = Math.max(1, Math.round(r.height * d));
  if (cv.width !== w || cv.height !== h) { cv.width = w; cv.height = h; }
  const g = cv.getContext('2d');
  g.setTransform(d, 0, 0, d, 0, 0);
  return { g, w: r.width, h: r.height };
}
function rrect(g, x, y, w, h, r) {
  r = Math.min(r, w / 2, h / 2);
  g.beginPath();
  g.moveTo(x + r, y);
  g.arcTo(x + w, y, x + w, y + h, r);
  g.arcTo(x + w, y + h, x, y + h, r);
  g.arcTo(x, y + h, x, y, r);
  g.arcTo(x, y, x + w, y, r);
  g.closePath();
}

let toastTimer = null;
function toast(msg) {
  const el = $('#toast');
  el.textContent = msg;
  el.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove('show'), 2200);
}

// ---------- top bar ----------
function renderTop() {
  if (!P) return;
  $('#projName').textContent = P.name;
  $('#bpmVal').textContent = P.bpm;
  const pl = $('#btnPlay');
  pl.classList.toggle('on', T.playing);
  pl.innerHTML = T.playing ? '■' : '▶';
  $('#btnRec').classList.toggle('on', UI.recording);
  $('#btnMetro').classList.toggle('on', P.metro);
  $('#btnUndo').disabled = !undoStack.length;
}
function onTransport() {
  renderTop();
  const rb = $('#recAudioBtn');
  if (rb) {
    rb.classList.toggle('on', !!T.capture);
    rb.textContent = T.capture ? '■ Stop' : '● Record';
  }
  if (!T.playing) updatePlayhead(null);
  startRaf();
}

function render() {
  renderTop();
  renderMain();
}
function renderMain() {
  const main = $('#main');
  const tr = currentTrack();
  if (UI.view === 'editor' && tr) {
    main.innerHTML = editorHTML(tr);
    mountEditor(tr);
  } else {
    UI.view = 'tracks';
    main.innerHTML = tracksHTML();
    drawLanes();
  }
  updatePlayhead(curPos());
}

// ---------- tracks view ----------
function trackSub(t) {
  if (t.type === 'drums') return (KITS.find(k => k.id === t.kit) || KITS[0]).name;
  if (t.type === 'audio') return t.audioId ? 'Recording' : 'Empty – tap to record';
  const inst = (INST[t.instrument] || INSTRUMENTS[0]).name;
  return inst !== t.name ? inst : t.notes.length ? t.notes.length + ' notes' : 'Empty – tap to play';
}
function tracksHTML() {
  let h = `<div class="lanes" id="lanes"><div class="ruler"><div class="rhead">${NOTE_NAMES[P.key]} ${P.scale} · ${P.bars} bar${P.bars > 1 ? 's' : ''}</div><div class="rbody">`;
  for (let b = 0; b < P.bars; b++) h += `<span style="left:${b / P.bars * 100}%">${b + 1}</span>`;
  h += '</div></div>';
  for (const t of P.tracks) {
    h += `<div class="lane${UI.sel === t.id ? ' sel' : ''}" data-id="${t.id}" style="--c:${trackColor(t)}">
      <div class="lhead">
        <button class="ticon" data-act="open">${trackIcon(t)}</button>
        <div class="tinfo" data-act="open"><div class="tname">${esc(t.name)}</div><div class="tsub">${trackSub(t)}</div></div>
        <div class="msbox"><button class="ms${t.mute ? ' on' : ''}" data-act="mute">M</button><button class="ms solo${t.solo ? ' on' : ''}" data-act="solo">S</button></div>
        <button class="ms more" data-act="trackMenu">⋯</button>
      </div>
      <div class="lbody" data-act="open"><canvas class="mini"></canvas></div>
    </div>`;
  }
  if (!P.tracks.length) h += '<div class="empty">No tracks yet. Add drums, an instrument or your voice to start.</div>';
  h += '<button class="addtrack" data-act="addTrack">＋ Add Track</button><div class="playhead" id="playhead"></div></div>';
  return h;
}
function drawLanes() {
  for (const lane of $$('.lane')) {
    const t = P.tracks.find(x => x.id === lane.dataset.id);
    const cv = $('canvas', lane);
    if (!t || !cv) continue;
    const { g, w, h } = fitCanvas(cv);
    const tot = totalSteps(), col = trackColor(t);
    g.clearRect(0, 0, w, h);
    g.fillStyle = 'rgba(255,255,255,0.03)';
    g.fillRect(0, 0, w, h);
    g.strokeStyle = 'rgba(255,255,255,0.08)';
    g.lineWidth = 1;
    for (let b = 1; b < P.bars; b++) { const x = Math.round(b / P.bars * w) + 0.5; g.beginPath(); g.moveTo(x, 0); g.lineTo(x, h); g.stroke(); }
    g.fillStyle = col;
    g.globalAlpha = t.mute ? 0.35 : 0.95;
    if (t.type === 'drums') {
      const rh = (h - 6) / DRUMS.length;
      for (const n of t.notes) if (n.s < tot) g.fillRect(n.s / tot * w, 3 + n.n * rh, Math.max(2, w / tot - 1), Math.max(1.5, rh - 1));
    } else if (t.type === 'inst') {
      if (t.notes.length) {
        let lo = Infinity, hi = -Infinity;
        for (const n of t.notes) { lo = Math.min(lo, n.n); hi = Math.max(hi, n.n); }
        lo -= 2; hi += 2;
        const nh = Math.max(2, Math.min(6, (h - 6) / (hi - lo + 1)));
        for (const n of t.notes) {
          if (n.s >= tot) continue;
          const y = 3 + (hi - n.n) / (hi - lo) * (h - 6 - nh);
          g.fillRect(n.s / tot * w, y, Math.max(2, Math.min(n.l, tot - n.s) / tot * w - 1), nh);
        }
      }
    } else if (t.type === 'audio' && t.audioId && RAW.get(t.audioId)) {
      drawWave(g, RAW.get(t.audioId), t, 0, 0, w, h);
    }
    g.globalAlpha = 1;
  }
}
function drawWave(g, raw, t, x0, y0, w, h) {
  const d = raw.ch[0], ld = loopDur();
  const off = (t.nudge || 0) / 1000;
  const mid = y0 + h / 2;
  g.beginPath();
  for (let px = 0; px < w; px++) {
    const t0 = px / w * ld - off, t1 = (px + 1) / w * ld - off;
    const i0 = Math.floor(t0 * raw.sr), i1 = Math.floor(t1 * raw.sr);
    if (i1 <= 0 || i0 >= d.length) continue;
    let pk = 0;
    const step = Math.max(1, Math.floor((i1 - i0) / 64));
    for (let i = Math.max(0, i0); i < Math.min(d.length, i1); i += step) pk = Math.max(pk, Math.abs(d[i]));
    const a = Math.max(0.5, pk * h * 0.48);
    g.rect(x0 + px, mid - a, 1, a * 2);
  }
  g.fill();
}

// ---------- playhead / animation ----------
let rafOn = false;
function startRaf() {
  if (rafOn) return;
  rafOn = true;
  requestAnimationFrame(frame);
}
function frame() {
  updatePlayhead(curPos());
  const lv = $('#micLevel');
  if (lv) lv.style.width = (mic ? Math.min(100, mic.level * 140) : 0) + '%';
  if (T.playing || mic || T.capture) requestAnimationFrame(frame);
  else { rafOn = false; updatePlayhead(null); }
}
let lastDrumCol = -1;
function updatePlayhead(pos) {
  const posEl = $('#pos');
  if (posEl) {
    if (pos == null) posEl.textContent = '1.1';
    else if (pos < 0) posEl.textContent = 'Count ' + (Math.floor((pos + 16) / 4) + 1);
    else posEl.textContent = (Math.floor(pos / 16) + 1) + '.' + (Math.floor(pos / 4) % 4 + 1);
  }
  const ph = $('#playhead');
  if (ph) {
    const body = $('.lane .lbody') || $('.ruler .rbody');
    if (pos == null || pos < 0 || !body) ph.style.display = 'none';
    else {
      const lanes = $('#lanes');
      const lr = lanes.getBoundingClientRect(), br = body.getBoundingClientRect();
      ph.style.display = 'block';
      ph.style.left = (br.left - lr.left + (pos % totalSteps()) / totalSteps() * br.width) + 'px';
      ph.style.height = lanes.scrollHeight + 'px';
    }
  }
  if (UI.view === 'editor') {
    const tr = currentTrack();
    if (!tr) return;
    if (tr.type === 'drums' && UI.drumMode === 'grid') {
      const col = pos == null || pos < 0 ? -1 : Math.floor(pos) % totalSteps();
      if (col !== lastDrumCol) {
        $$('.step.now').forEach(b => b.classList.remove('now'));
        if (col >= 0) $$(`.step[data-step="${col}"]`).forEach(b => b.classList.add('now'));
        lastDrumCol = col;
      }
    } else if (tr.type === 'inst' && UI.instMode === 'edit') {
      rollDraw();
    } else if (tr.type === 'audio') {
      audioDraw();
    }
  }
}

// ---------- editor ----------
function seg(act, items, cur) {
  return `<div class="seg">${items.map(([v, l]) => `<button data-act="${act}" data-v="${v}" class="${String(v) === String(cur) ? 'on' : ''}">${l}</button>`).join('')}</div>`;
}
function editorHTML(t) {
  let bar = `<button class="back" data-act="back">‹ Tracks</button>`;
  const short = s => s.replace('Acoustic ', '').replace(' Guitar', '').replace('Electric Piano', 'E-Piano').replace('Grand ', '').replace(' Kit', '');
  bar += `<button class="instbtn" data-act="pickInst" style="--c:${trackColor(t)}">${trackIcon(t)} ${esc(t.type === 'inst' ? short(INST[t.instrument].name) : t.type === 'drums' ? short((KITS.find(k => k.id === t.kit) || KITS[0]).name) : t.name)} ▾</button>`;
  if (t.type === 'inst') {
    bar += seg('instMode', [['play', '🎹 Play'], ['edit', '✏️ Edit']], UI.instMode);
    if (UI.instMode === 'play') {
      bar += seg('kbMode', [['keys', 'Keys'], ['scale', 'Scale'], ['chords', 'Chords']], UI.kbMode);
      bar += `<div class="oct"><button data-act="oct" data-v="-1">−</button><span>Oct ${t.oct}</span><button data-act="oct" data-v="1">+</button></div>`;
      if (UI.kbMode === 'chords') bar += `<button class="tog${UI.strum ? ' on' : ''}" data-act="strum">Strum</button>`;
    } else {
      bar += `<button class="hot" data-act="ideas">✨ Ideas</button>`;
      bar += seg('noteLen', [[1, '1/16'], [2, '1/8'], [4, '1/4'], [8, '1/2'], [16, 'Bar']], UI.noteLen);
      bar += `<div class="oct"><button data-act="zoom" data-v="-1">−</button><span>Zoom</span><button data-act="zoom" data-v="1">+</button></div>`;
      bar += `<button data-act="clearNotes">🗑 Clear</button>`;
    }
  } else if (t.type === 'drums') {
    bar += seg('drumMode', [['grid', '▦ Grid'], ['pads', '🥁 Pads']], UI.drumMode);
    bar += `<button class="hot" data-act="beats">✨ Beats</button>`;
    if (UI.drumMode === 'grid') {
      if (P.bars > 1) {
        bar += `<div class="oct"><button data-act="page" data-v="-1">‹</button><span>Bar ${UI.drumPage + 1}/${P.bars}</span><button data-act="page" data-v="1">›</button></div>`;
        bar += `<button data-act="copyBar">⧉ Copy to all bars</button>`;
      }
    }
    bar += `<button data-act="clearNotes">🗑 Clear</button>`;
  }
  return `<div class="editor"><div class="ebar">${bar}</div><div class="ebody" id="ebody"></div></div>`;
}
function mountEditor(t) {
  const body = $('#ebody');
  if (t.type === 'drums') {
    UI.drumPage = clamp(UI.drumPage, 0, P.bars - 1);
    if (UI.drumMode === 'grid') mountDrumGrid(body, t);
    else mountDrumPads(body, t);
    lastDrumCol = -1;
  } else if (t.type === 'inst') {
    if (UI.instMode === 'edit') mountRoll(body, t);
    else if (UI.kbMode === 'scale') mountScalePads(body, t);
    else if (UI.kbMode === 'chords') mountChords(body, t);
    else mountKeys(body, t);
  } else {
    mountAudio(body, t);
  }
}
function onNotesRecorded(tr) {
  if (UI.view !== 'editor' || currentTrack() !== tr) return;
  if (tr.type === 'drums' && UI.drumMode === 'grid') refreshDrumGrid(tr);
}

// Multi-touch helper: tracks which element each finger is on, supports sliding.
function touchSurface(el, selector, onDown, onUp) {
  const fingers = new Map(); // pointerId -> {target, data}
  const hit = e => { const x = document.elementFromPoint(e.clientX, e.clientY); return x && x.closest(selector); };
  el.addEventListener('pointerdown', e => {
    const target = e.target.closest(selector);
    if (!target) return;
    e.preventDefault();
    try { el.setPointerCapture(e.pointerId); } catch (err) {}
    fingers.set(e.pointerId, { target, data: onDown(target, e) });
  });
  el.addEventListener('pointermove', e => {
    const f = fingers.get(e.pointerId);
    if (!f) return;
    const target = hit(e);
    if (target && target !== f.target) {
      onUp(f.target, f.data);
      fingers.set(e.pointerId, { target, data: onDown(target, e) });
    }
  });
  const end = e => {
    const f = fingers.get(e.pointerId);
    if (!f) return;
    fingers.delete(e.pointerId);
    onUp(f.target, f.data);
  };
  el.addEventListener('pointerup', end);
  el.addEventListener('pointercancel', end);
  el.addEventListener('lostpointercapture', end);
}

// --- piano keys ---
function mountKeys(body, t) {
  const w = body.clientWidth || 600;
  const nWhite = clamp(Math.floor(w / 44), 8, 36);
  const whites = [];
  let m = 12 * (t.oct + 1);
  while (whites.length < nWhite) { if (!isBlack(m)) whites.push(m); m++; }
  let h = '<div class="keys">';
  whites.forEach(n => {
    const root = pc(n) === P.key;
    h += `<div class="wk${inScale(n, P.key, P.scale) ? ' insc' : ''}${root ? ' root' : ''}" data-midi="${n}"><span>${pc(n) === 0 ? noteName(n) : ''}</span></div>`;
  });
  whites.forEach((n, i) => {
    if (i < whites.length - 1 && isBlack(n + 1)) {
      h += `<div class="bk${inScale(n + 1, P.key, P.scale) ? ' insc' : ''}" data-midi="${n + 1}" style="left:calc(${(i + 1) / nWhite * 100}% - ${50 / nWhite * 0.62}%);width:${100 / nWhite * 0.62}%"></div>`;
    }
  });
  h += '</div>';
  body.innerHTML = h;
  keyTouch($('.keys', body), t, el => [+el.dataset.midi], el => {
    const r = el.getBoundingClientRect();
    return (y) => clamp(0.45 + 0.55 * (y - r.top) / r.height, 0.3, 1);
  });
}
// --- scale pads: two rows, only notes in key ---
function mountScalePads(body, t) {
  const deg = SCALES[P.scale];
  const base = 12 * (t.oct + 1) + P.key;
  const rowNotes = o => [...deg.map(i => base + o + i), base + o + 12];
  let h = '<div class="pads scalepads">';
  for (const o of [12, 0]) {
    h += '<div class="prow">';
    rowNotes(o).forEach((n, i) => { h += `<div class="pad deg${i % 7}" data-midi="${n}"><b>${NOTE_NAMES[pc(n)]}</b><small>${noteName(n)}</small></div>`; });
    h += '</div>';
  }
  body.innerHTML = h + '</div>';
  keyTouch($('.pads', body), t, el => [+el.dataset.midi]);
}
// --- chord strips ---
function mountChords(body, t) {
  const chords = chordList(P.key, P.scale);
  const bassy = ['bass', 'sub808'].includes(t.instrument);
  let h = '<div class="chords">';
  chords.forEach((c, i) => {
    h += `<div class="strip"><div class="chord" data-chord="${i}"><b>${c.name}</b><small>${c.roman}</small></div><div class="chord bassnote" data-bass="${i}">${bassy ? 'Low' : 'Bass'} ${c.rootName}</div></div>`;
  });
  body.innerHTML = h + '</div>';
  keyTouch($('.chords', body), t, el => {
    const c = chords[+(el.dataset.chord ?? el.dataset.bass)];
    const v = voiceChord(c, t.oct);
    if (el.dataset.bass != null) return [v.bass];
    return bassy ? [v.notes[0]] : [v.bass, ...v.notes];
  }, null, true);
}
function keyTouch(el, t, notesOf, velOf, strum) {
  touchSurface(el, '[data-midi],[data-chord],[data-bass]', (target, e) => {
    target.classList.add('down');
    const notes = notesOf(target);
    const vel = velOf ? velOf(target)(e.clientY) : 0.82;
    const keys = notes.map((n, i) => {
      const k = e.pointerId + ':' + n + ':' + i;
      liveOn(k, t, n, vel, strum && UI.strum ? i * 0.022 : 0);
      return k;
    });
    return keys;
  }, (target, keys) => {
    target.classList.remove('down');
    keys.forEach(liveOff);
  });
}

// --- drum grid ---
function mountDrumGrid(body, t) {
  const page = UI.drumPage;
  let h = '<div class="dgrid">';
  DRUMS.forEach((d, r) => {
    h += `<div class="drow"><div class="dlabel" data-pad="${r}">${d.name}</div><div class="steps">`;
    for (let i = 0; i < 16; i++) {
      const s = page * 16 + i;
      h += `<div class="step${i % 4 === 0 ? ' beat' : ''}${Math.floor(i / 4) % 2 ? ' alt' : ''}" data-step="${s}" data-row="${r}"></div>`;
    }
    h += '</div></div>';
  });
  body.innerHTML = h + '</div>';
  refreshDrumGrid(t);
  const grid = $('.dgrid', body);
  let paint = null;
  touchSurface(grid, '.step,.dlabel', (el, e) => {
    if (el.classList.contains('dlabel')) { el.classList.add('down'); previewNote(t, +el.dataset.pad, 0.9); return null; }
    const s = +el.dataset.step, r = +el.dataset.row;
    if (paint == null) { pushUndo(); paint = !t.notes.some(n => n.s === s && n.n === r); }
    setDrumStep(t, s, r, paint);
    el.classList.toggle('on', paint);
    if (paint) previewNote(t, r, 0.85);
    return null;
  }, el => el.classList.remove('down'));
  grid.addEventListener('pointerup', () => { paint = null; changed('none'); });
  grid.addEventListener('pointercancel', () => { paint = null; changed('none'); });
}
function setDrumStep(t, s, r, on) {
  const i = t.notes.findIndex(n => n.s === s && n.n === r);
  if (on && i < 0) t.notes.push({ s, n: r, l: 1, v: s % 4 === 0 ? 0.95 : 0.8 });
  if (!on && i >= 0) t.notes.splice(i, 1);
}
function refreshDrumGrid(t) {
  const on = new Set(t.notes.map(n => n.s + ':' + n.n));
  for (const el of $$('.step')) el.classList.toggle('on', on.has(el.dataset.step + ':' + el.dataset.row));
}
// --- drum pads ---
function mountDrumPads(body, t) {
  const order = [[7, 6, 5, 4], [3, 2, 1, 0]];
  let h = '<div class="pads drumpads">';
  for (const row of order) {
    h += '<div class="prow">';
    for (const r of row) h += `<div class="pad dp${r}" data-pad="${r}"><b>${DRUMS[r].name}</b></div>`;
    h += '</div>';
  }
  body.innerHTML = h + '</div>';
  touchSurface($('.pads', body), '[data-pad]', (el, e) => {
    el.classList.add('down');
    const r = el.getBoundingClientRect();
    liveOn('d' + e.pointerId, t, +el.dataset.pad, clamp(0.55 + 0.45 * (e.clientY - r.top) / r.height, 0.4, 1));
  }, el => el.classList.remove('down'));
}

// --- piano roll ---
const ROLL = { KEYW: 44, RULER: 20, ROWH: 22, LO: 24, HI: 96 };
let roll = null;
function mountRoll(body, t) {
  body.innerHTML = '<canvas class="roll"></canvas>';
  const cv = $('canvas', body);
  let st = UI.roll[t.id];
  if (!st) {
    const center = t.notes.length ? t.notes.reduce((a, n) => a + n.n, 0) / t.notes.length : 12 * (t.oct + 1) + 7;
    const visH = (body.clientHeight || 300) - ROLL.RULER;
    st = UI.roll[t.id] = { x: 0, y: (ROLL.HI - center + 0.5) * ROLL.ROWH - visH / 2 };
  }
  roll = { cv, t, st, ptr: null };
  rollScroll(st.x, st.y);
  rollDraw();
  cv.addEventListener('pointerdown', e => {
    if (roll.ptr) return;
    cv.setPointerCapture(e.pointerId);
    const hitn = rollHit(e);
    roll.ptr = { id: e.pointerId, x0: e.clientX, y0: e.clientY, sx: st.x, sy: st.y, moved: false, hit: hitn, undo: false };
    if (hitn.key != null) previewNote(t, hitn.key);
  });
  cv.addEventListener('pointermove', e => {
    const p = roll.ptr;
    if (!p || p.id !== e.pointerId) return;
    const dx = e.clientX - p.x0, dy = e.clientY - p.y0;
    if (!p.moved && Math.hypot(dx, dy) > 8) p.moved = true;
    if (!p.moved) return;
    if (p.hit.note) {
      if (!p.undo) { pushUndo(); p.undo = true; }
      const h2 = rollHit(e);
      p.hit.note.l = clamp(h2.step - p.hit.note.s + 1, 1, totalSteps() - p.hit.note.s);
    } else {
      rollScroll(p.sx - dx, p.sy - dy);
    }
    rollDraw();
  });
  const up = e => {
    const p = roll.ptr;
    if (!p || p.id !== e.pointerId) return;
    roll.ptr = null;
    if (e.type === 'pointerup' && !p.moved && p.hit.key == null && p.hit.step != null) {
      pushUndo();
      if (p.hit.note) {
        t.notes.splice(t.notes.indexOf(p.hit.note), 1);
      } else {
        const snap = Math.min(UI.noteLen, 4);
        const s = Math.floor(p.hit.step / snap) * snap;
        const l = Math.min(UI.noteLen, totalSteps() - s);
        if (l > 0) {
          t.notes.push({ s, n: p.hit.midi, l, v: 0.8 });
          previewNote(t, p.hit.midi);
        }
      }
      changed('none');
    } else if (p.undo) changed('none');
    rollDraw();
  };
  cv.addEventListener('pointerup', up);
  cv.addEventListener('pointercancel', up);
  cv.addEventListener('wheel', e => { rollScroll(st.x + e.deltaX, st.y + e.deltaY); rollDraw(); e.preventDefault(); }, { passive: false });
}
function rollCellW() { return 26 * UI.zoom; }
function rollScroll(x, y) {
  const r = roll.cv.getBoundingClientRect();
  const maxX = Math.max(0, totalSteps() * rollCellW() - (r.width - ROLL.KEYW) + 20);
  const maxY = Math.max(0, (ROLL.HI - ROLL.LO + 1) * ROLL.ROWH - (r.height - ROLL.RULER));
  roll.st.x = clamp(x, 0, maxX);
  roll.st.y = clamp(y, 0, maxY);
}
function rollHit(e) {
  const r = roll.cv.getBoundingClientRect();
  const x = e.clientX - r.left, y = e.clientY - r.top;
  if (y < ROLL.RULER) return {};
  const midi = ROLL.HI - Math.floor((y - ROLL.RULER + roll.st.y) / ROLL.ROWH);
  if (x < ROLL.KEYW) return { key: midi };
  const step = Math.floor((x - ROLL.KEYW + roll.st.x) / rollCellW());
  if (step < 0 || step >= totalSteps() || midi < ROLL.LO || midi > ROLL.HI) return {};
  const note = roll.t.notes.find(n => n.n === midi && step >= n.s && step < n.s + n.l);
  return { step, midi, note };
}
function rollDraw() {
  if (!roll || !roll.cv.isConnected) return;
  const { g, w, h } = fitCanvas(roll.cv);
  const { KEYW, RULER, ROWH, LO, HI } = ROLL;
  const cw = rollCellW(), sx = roll.st.x, sy = roll.st.y, tot = totalSteps(), t = roll.t;
  g.fillStyle = '#1a1b20';
  g.fillRect(0, 0, w, h);
  const r0 = Math.max(0, Math.floor(sy / ROWH)), r1 = Math.min(HI - LO, Math.ceil((sy + h) / ROWH));
  for (let r = r0; r <= r1; r++) {
    const m = HI - r, y = RULER + r * ROWH - sy;
    g.fillStyle = pc(m) === P.key ? '#2e2b3a' : inScale(m, P.key, P.scale) ? '#26272e' : '#1d1e23';
    g.fillRect(KEYW, y, w - KEYW, ROWH - 1);
  }
  const s0 = Math.max(0, Math.floor(sx / cw)), s1 = Math.min(tot, Math.ceil((sx + w) / cw));
  for (let s = s0; s <= s1; s++) {
    const x = Math.round(KEYW + s * cw - sx) + 0.5;
    g.strokeStyle = s % 16 === 0 ? '#6a6c78' : s % 4 === 0 ? '#3c3e48' : '#2a2b32';
    g.beginPath(); g.moveTo(x, RULER); g.lineTo(x, h); g.stroke();
  }
  const endX = KEYW + tot * cw - sx;
  if (endX < w) { g.fillStyle = 'rgba(0,0,0,0.5)'; g.fillRect(endX, RULER, w - endX, h); }
  const col = trackColor(t);
  for (const n of t.notes) {
    if (n.s >= tot || n.n < LO || n.n > HI) continue;
    const x = KEYW + n.s * cw - sx, y = RULER + (HI - n.n) * ROWH - sy;
    const nw = Math.min(n.l, tot - n.s) * cw;
    if (x + nw < KEYW || x > w || y + ROWH < RULER || y > h) continue;
    g.globalAlpha = 0.55 + 0.45 * n.v;
    g.fillStyle = col;
    rrect(g, x + 1, y + 1, nw - 2, ROWH - 3, 4);
    g.fill();
    g.globalAlpha = 1;
    g.fillStyle = 'rgba(255,255,255,0.35)';
    g.fillRect(x + nw - 5, y + 5, 2, ROWH - 11);
  }
  const pos = curPos();
  if (pos != null && pos >= 0) {
    const x = KEYW + (pos % tot) * cw - sx;
    if (x >= KEYW) { g.fillStyle = '#fff'; g.fillRect(x, RULER, 2, h); }
  }
  // key column
  for (let r = r0; r <= r1; r++) {
    const m = HI - r, y = RULER + r * ROWH - sy;
    g.fillStyle = isBlack(m) ? '#111' : '#e8e8ec';
    g.fillRect(0, y, KEYW - 2, ROWH - 1);
    if (!isBlack(m) || pc(m) === P.key) {
      g.fillStyle = isBlack(m) ? '#ccc' : '#333';
      g.font = '10px sans-serif';
      g.fillText(noteName(m), 4, y + ROWH - 7);
    }
  }
  // ruler
  g.fillStyle = '#23242b';
  g.fillRect(0, 0, w, RULER);
  g.fillStyle = '#aaa';
  g.font = '11px sans-serif';
  for (let b = 0; b < P.bars; b++) {
    const x = KEYW + b * 16 * cw - sx;
    if (x > KEYW - 20 && x < w) g.fillText(String(b + 1), x + 4, 14);
  }
}

// --- audio track ---
function mountAudio(body, t) {
  body.innerHTML = `<div class="audiopanel">
    <canvas class="wave"></canvas>
    <div class="arow">
      <button class="bigrec${T.capture ? ' on' : ''}" id="recAudioBtn" data-act="recAudio">${T.capture ? '■ Stop' : '● Record'}</button>
      <label class="btn">📂 Import audio<input type="file" accept="audio/*" id="importFile" hidden></label>
      <button data-act="clearAudio"${t.audioId ? '' : ' disabled'}>🗑 Clear</button>
      <div class="meter"><div id="micLevel"></div></div>
    </div>
    <div class="arow">
      <span class="lbl">Timing</span>
      <input type="range" id="nudge" min="-300" max="300" step="5" value="${t.nudge || 0}">
      <span class="lbl" id="nudgeVal">${t.nudge || 0} ms</span>
    </div>
    <p class="hint">Tap <b>Record</b>: you get a 1-bar count-in, then it records one loop (${P.bars} bar${P.bars > 1 ? 's' : ''}). Headphones help. If it sounds late or early, slide <b>Timing</b>.</p>
  </div>`;
  $('#importFile').addEventListener('change', e => { const f = e.target.files[0]; if (f) importAudio(t, f); });
  const nz = $('#nudge');
  nz.addEventListener('input', () => { t.nudge = +nz.value; $('#nudgeVal').textContent = t.nudge + ' ms'; audioDraw(); });
  nz.addEventListener('change', () => changed('none'));
  audioDraw();
}
function audioDraw() {
  const cv = $('canvas.wave');
  const t = currentTrack();
  if (!cv || !t) return;
  const { g, w, h } = fitCanvas(cv);
  g.fillStyle = '#1d1e23';
  g.fillRect(0, 0, w, h);
  g.strokeStyle = '#3c3e48';
  for (let b = 0; b <= P.bars * 4; b++) {
    const x = Math.round(b / (P.bars * 4) * w) + 0.5;
    g.strokeStyle = b % 4 === 0 ? '#5a5c68' : '#2e3038';
    g.beginPath(); g.moveTo(x, 0); g.lineTo(x, h); g.stroke();
  }
  const raw = t.audioId && RAW.get(t.audioId);
  g.fillStyle = trackColor(t);
  if (raw) drawWave(g, raw, t, 0, 8, w, h - 16);
  else { g.fillStyle = '#777'; g.font = '14px sans-serif'; g.textAlign = 'center'; g.fillText(T.capture ? 'Recording…' : 'No recording yet', w / 2, h / 2); g.textAlign = 'start'; }
  const pos = curPos();
  if (pos != null && pos >= 0) { g.fillStyle = '#fff'; g.fillRect((pos % totalSteps()) / totalSteps() * w, 0, 2, h); }
}

// ---------- modals ----------
let modalHandler = null, modalGen = 0;
function openModal(title, html, handler) {
  modalGen++;
  const m = $('#modal');
  m.innerHTML = `<div class="sheet"><div class="mhead"><h2>${title}</h2><button class="x" data-mact="close">✕</button></div><div class="mbody">${html}</div></div>`;
  m.classList.remove('hidden');
  modalHandler = handler || null;
  return m;
}
function closeModal() {
  const m = $('#modal');
  if (m.classList.contains('hidden')) return false;
  m.classList.add('hidden');
  m.innerHTML = '';
  modalHandler = null;
  return true;
}
// ✕ / backdrop / Android back: let the dialog react, then close it unless it opened another one
function dismissModal() {
  if ($('#modal').classList.contains('hidden')) return false;
  const g = modalGen;
  if (modalHandler) modalHandler('close');
  if (modalGen === g) closeModal();
  return true;
}
function ask(msg, ok = 'OK') {
  return new Promise(res => {
    openModal('Are you sure?', `<p>${msg}</p><div class="mrow right"><button data-mact="no">Cancel</button><button class="primary" data-mact="yes">${ok}</button></div>`, a => {
      if (a === 'yes' || a === 'no' || a === 'close') { closeModal(); res(a === 'yes'); }
    });
  });
}
function cards(items) {
  return `<div class="cards">${items.map(i => `<button class="card" data-mact="${i.act}" data-v="${i.v}" style="--c:${i.color || '#555'}"><span class="ci">${i.icon}</span><span>${i.name}</span></button>`).join('')}</div>`;
}
function openAddTrack() {
  const items = [
    { act: 'new', v: 'drums', icon: '🥁', name: 'Drums', color: '#ffb020' },
    { act: 'new', v: 'audio', icon: '🎤', name: 'Voice / Mic', color: '#ef476f' },
    ...INSTRUMENTS.map(i => ({ act: 'new', v: i.id, icon: i.icon, name: i.name, color: i.color })),
  ];
  openModal('Add a track', cards(items), (a, v) => {
    if (a !== 'new') return;
    pushUndo();
    const t = v === 'drums' ? newTrack('drums', 'acoustic') : v === 'audio' ? newTrack('audio') : newTrack('inst', v);
    if (t.type === 'drums' && !P.tracks.some(x => x.type === 'drums')) t.notes = beatNotes(BEATS[0], P.bars);
    P.tracks.push(t);
    UI.sel = t.id;
    UI.view = 'editor';
    if (t.type === 'inst') UI.instMode = 'play';
    syncBuses();
    closeModal();
    changed();
  });
}
function openInstPicker(t) {
  if (t.type === 'audio') return openTrackMenu(t);
  const items = t.type === 'drums'
    ? KITS.map(k => ({ act: 'pick', v: k.id, icon: '🥁', name: k.name, color: '#ffb020' }))
    : INSTRUMENTS.map(i => ({ act: 'pick', v: i.id, icon: i.icon, name: i.name, color: i.color }));
  openModal(t.type === 'drums' ? 'Choose a drum kit' : 'Choose a sound', cards(items), (a, v) => {
    if (a !== 'pick') return;
    pushUndo();
    if (t.type === 'drums') t.kit = v;
    else {
      const wasDefault = t.name === INST[t.instrument].name;
      t.instrument = v;
      if (wasDefault) t.name = INST[v].name;
    }
    closeModal();
    changed();
    if (t.type === 'drums') previewNote(t, 0); else previewNote(t, 12 * (t.oct + 1) + P.key);
  });
}
function slider(id, label, min, max, step, val, fmt) {
  return `<div class="mrow"><label for="${id}">${label}</label><input type="range" id="${id}" min="${min}" max="${max}" step="${step}" value="${val}"><span class="val" id="${id}V">${fmt(val)}</span></div>`;
}
function openTrackMenu(t) {
  const pct = v => Math.round(v * 100) + '%';
  const panf = v => +v === 0 ? 'Center' : (+v < 0 ? 'L ' : 'R ') + Math.round(Math.abs(v) * 100);
  const html = `
    <div class="mrow"><label>Name</label><input type="text" id="tName" value="${esc(t.name)}" maxlength="24"></div>
    ${slider('tVol', 'Volume', 0, 1, 0.01, t.vol, pct)}
    ${slider('tPan', 'Pan', -1, 1, 0.05, t.pan, panf)}
    ${slider('tRev', 'Reverb', 0, 1, 0.01, t.reverb, pct)}
    <div class="mrow btns">
      <button data-mact="up">▲ Move up</button><button data-mact="down">▼ Move down</button>
      <button data-mact="dup">⧉ Duplicate</button><button class="danger" data-mact="del">🗑 Delete</button>
    </div>`;
  const m = openModal(`${trackIcon(t)} Track settings`, html, async a => {
    const i = P.tracks.indexOf(t);
    if (a === 'close') { changed(); return closeModal(); }
    if (a === 'up' || a === 'down') {
      const j = a === 'up' ? i - 1 : i + 1;
      if (j < 0 || j >= P.tracks.length) return;
      pushUndo();
      P.tracks.splice(i, 1); P.tracks.splice(j, 0, t);
      changed();
    } else if (a === 'dup') {
      pushUndo();
      const c = JSON.parse(JSON.stringify(t));
      c.id = uid(); c.name = t.name + ' 2'; c.solo = false;
      P.tracks.splice(i + 1, 0, c);
      syncBuses();
      closeModal();
      changed();
    } else if (a === 'del') {
      closeModal();
      if (!(await ask(`Delete track “${esc(t.name)}”?`, 'Delete'))) return;
      pushUndo();
      P.tracks.splice(P.tracks.indexOf(t), 1);
      if (eng) eng.removeBus(t.id);
      if (UI.sel === t.id) { UI.sel = P.tracks[0] ? P.tracks[0].id : null; UI.view = 'tracks'; }
      syncBuses();
      changed();
    }
  });
  let undoPushed = false;
  const once = () => { if (!undoPushed) { pushUndo(); undoPushed = true; } };
  $('#tName', m).addEventListener('input', e => { once(); t.name = e.target.value || 'Track'; });
  for (const [id, key, fmt] of [['tVol', 'vol', pct], ['tPan', 'pan', panf], ['tRev', 'reverb', pct]]) {
    $('#' + id, m).addEventListener('input', e => {
      once();
      t[key] = +e.target.value;
      $('#' + id + 'V', m).textContent = fmt(t[key]);
      syncBuses();
    });
  }
}
let taps = [];
function openSong() {
  const html = `
    <div class="mrow"><label>Tempo</label><input type="range" id="sBpm" min="50" max="200" step="1" value="${P.bpm}"><span class="val" id="sBpmV">${P.bpm} BPM</span><button data-mact="tap">Tap</button></div>
    <div class="mrow"><label>Key</label><div class="keysel">${NOTE_NAMES.map((n, i) => `<button data-mact="key" data-v="${i}" class="${P.key === i ? 'on' : ''}">${n}</button>`).join('')}</div></div>
    <div class="mrow"><label>Scale</label>${seg('scale', [['major', '😊 Major (happy)'], ['minor', '😢 Minor (moody)']], P.scale).replace(/data-act=/g, 'data-mact=')}</div>
    <div class="mrow"><label>Length</label>${seg('bars', [[1, '1 bar'], [2, '2'], [4, '4'], [8, '8'], [16, '16 bars']], P.bars).replace(/data-act=/g, 'data-mact=')}<button data-mact="double">⧉ Double it</button></div>
    ${slider('sSwing', 'Swing', 0, 0.8, 0.05, P.swing, v => Math.round(v * 100) + '%')}
    <div class="mrow"><label>Options</label><button class="tog${P.metro ? ' on' : ''}" data-mact="metro">♩ Metronome</button><button class="tog${P.countIn ? ' on' : ''}" data-mact="countIn">1-bar count-in</button></div>`;
  let undoPushed = false;
  const once = () => { if (!undoPushed) { pushUndo(); undoPushed = true; } };
  const m = openModal('⚙ Song settings', html, (a, v) => {
    if (a === 'close') { closeModal(); return changed(); }
    once();
    if (a === 'tap') {
      const now = performance.now();
      taps = taps.filter(x => now - x < 2500);
      taps.push(now);
      if (taps.length >= 2) {
        const avg = (taps[taps.length - 1] - taps[0]) / (taps.length - 1);
        setBpm(Math.round(60000 / avg));
        $('#sBpm', m).value = P.bpm;
        $('#sBpmV', m).textContent = P.bpm + ' BPM';
      }
      return;
    }
    if (a === 'key') P.key = +v;
    else if (a === 'scale') P.scale = v;
    else if (a === 'bars') P.bars = +v;
    else if (a === 'metro') P.metro = !P.metro;
    else if (a === 'countIn') P.countIn = !P.countIn;
    else if (a === 'double') {
      if (P.bars >= 16) return toast('Already at 16 bars');
      const len = totalSteps();
      for (const t of P.tracks) {
        t.notes = t.notes.filter(n => n.s < len);
        t.notes.push(...t.notes.map(n => ({ ...n, s: n.s + len })));
      }
      P.bars *= 2;
      toast(`Now ${P.bars} bars`);
    }
    changed();
    openSong();
  });
  $('#sBpm', m).addEventListener('input', e => { once(); setBpm(+e.target.value); $('#sBpmV', m).textContent = P.bpm + ' BPM'; });
  $('#sSwing', m).addEventListener('input', e => { once(); P.swing = +e.target.value; $('#sSwingV', m).textContent = Math.round(P.swing * 100) + '%'; changed('none'); });
}
function setBpm(v) {
  P.bpm = clamp(Math.round(v), 50, 200);
  changed('top');
}
function openIdeas(t) {
  const progs = PROGRESSIONS[P.scale];
  let sel = Settings.get('ideaProg', 0) % progs.length;
  let style = Settings.get('ideaStyle', ['bass', 'sub808'].includes(t.instrument) ? 'bass' : 'held');
  const draw = () => {
    const html = `<p class="muted">Pick a chord progression and a style — it fills this track in ${NOTE_NAMES[P.key]} ${P.scale}. (Undo brings back what was there.)</p>
      <h3>Progression</h3><div class="list">${progs.map((p, i) => `<button data-mact="prog" data-v="${i}" class="${i === sel ? 'on' : ''}"><b>${p.name}</b><small>${p.desc}</small></button>`).join('')}</div>
      <h3>Style</h3><div class="list">${IDEA_STYLES.map(s => `<button data-mact="style" data-v="${s.id}" class="${s.id === style ? 'on' : ''}"><b>${s.name}</b><small>${s.desc}</small></button>`).join('')}</div>
      <div class="mrow right sticky"><button class="primary" data-mact="go">✨ Fill track</button></div>`;
    openModal('✨ Ideas', html, (a, v) => {
      if (a === 'close') return closeModal();
      if (a === 'prog') { sel = +v; draw(); }
      if (a === 'style') { style = v; draw(); }
      if (a === 'go') {
        Settings.set('ideaProg', sel); Settings.set('ideaStyle', style);
        pushUndo();
        t.notes = generateIdea(progs[sel], style, P, t.oct);
        closeModal();
        changed();
        if (!T.playing) play(false);
      }
    });
  };
  draw();
}
function openBeats(t) {
  const html = `<p class="muted">Tap a beat to load it (replaces this drum track; Undo brings it back).</p><div class="list grid">${BEATS.map((b, i) => `<button data-mact="beat" data-v="${i}"><b>${b.name}</b><small>${(KITS.find(k => k.id === b.kit) || KITS[0]).name}${b.swing ? ' · swing' : ''}</small></button>`).join('')}</div>`;
  openModal('✨ Beat presets', html, (a, v) => {
    if (a === 'close') return closeModal();
    if (a !== 'beat') return;
    const b = BEATS[+v];
    pushUndo();
    t.notes = beatNotes(b, P.bars);
    t.kit = b.kit;
    if (b.swing != null) P.swing = b.swing;
    closeModal();
    changed();
    if (!T.playing) play(false);
  });
}
function openExport() {
  let loops = 2;
  const draw = (status = '') => {
    openModal('⤓ Export song', `
      <p class="muted">Saves your song as a WAV file you can play, share or send anywhere.</p>
      <div class="mrow"><label>Loop it</label>${seg('loops', [[1, '1×'], [2, '2×'], [4, '4×'], [8, '8×']], loops).replace(/data-act=/g, 'data-mact=')}<span class="val">${Math.round(loopDur() * loops)} sec</span></div>
      <div class="mrow"><label>File name</label><input type="text" id="eName" value="${esc(P.name)}" maxlength="40"></div>
      <div class="mrow right"><button class="primary" data-mact="go">⤓ Export WAV</button></div>
      <div class="status">${status}</div>`, async (a, v) => {
      if (a === 'close') return closeModal();
      if (a === 'loops') { loops = +v; draw(); }
      if (a === 'share' && window.AndroidBridge) AndroidBridge.shareLast();
      if (a === 'go') {
        const name = ($('#eName').value.trim() || P.name || 'song').replace(/\.wav$/i, '') + '.wav';
        $('.status').innerHTML = '⏳ Rendering…';
        await new Promise(r => setTimeout(r, 30));
        try {
          stop();
          const buf = await renderSong(loops);
          $('.status').innerHTML = '⏳ Saving…';
          await new Promise(r => setTimeout(r, 30));
          const res = await saveFile(name, encodeWav(buf), 'audio/wav');
          if (!res) throw new Error('could not save file');
          draw(`✅ Saved to <b>${esc(res.where)}</b>${res.share ? ' <button data-mact="share">📤 Share</button>' : ''}`);
        } catch (e) {
          draw('❌ Export failed: ' + esc(e.message || e));
        }
      }
    });
  };
  draw();
}
async function openProjects() {
  await saveNow();
  const list = (await DB.listProjects()).sort((a, b) => b.updated - a.updated);
  const html = `
    <div class="mrow btns"><button class="primary" data-mact="newStarter">✨ New song (with starter band)</button><button data-mact="newEmpty">＋ New empty song</button><button data-mact="help">❔ How to use</button></div>
    <div class="plist">${list.map(p => `<div class="pitem${p.id === P.id ? ' cur' : ''}">
      <button class="pname" data-mact="open" data-v="${p.id}"><b>${esc(p.name)}</b><small>${p.tracks.length} tracks · ${p.bpm} BPM · ${new Date(p.updated).toLocaleDateString()}</small></button>
      <button data-mact="rename" data-v="${p.id}">✏️</button><button data-mact="copy" data-v="${p.id}">⧉</button><button data-mact="delete" data-v="${p.id}">🗑</button></div>`).join('')}</div>`;
  openModal('🎵 My songs', html, async (a, v) => {
    if (a === 'close') return closeModal();
    const p = list.find(x => x.id === v);
    if (a === 'newStarter' || a === 'newEmpty') {
      const np = newProject('Song ' + (list.length + 1), a === 'newStarter');
      await DB.putProject(np);
      closeModal();
      await openProject(np);
      if (a === 'newEmpty') openAddTrack();
    } else if (a === 'help') {
      openHelp();
    } else if (a === 'open') {
      closeModal();
      if (p.id !== P.id) await openProject(await DB.getProject(p.id));
    } else if (a === 'rename') {
      renameProject(p);
    } else if (a === 'copy') {
      const c = JSON.parse(JSON.stringify(p.id === P.id ? P : p));
      c.id = uid(); c.name = p.name + ' copy'; c.updated = Date.now();
      await DB.putProject(c);
      openProjects();
    } else if (a === 'delete') {
      if (!(await ask(`Delete “${esc(p.name)}” forever?`, 'Delete'))) return openProjects();
      for (const t of p.tracks) if (t.audioId) DB.deleteAudio(t.audioId).catch(() => {});
      await DB.deleteProject(p.id);
      if (p.id === P.id) {
        const rest = (await DB.listProjects()).sort((a2, b2) => b2.updated - a2.updated);
        let np = rest[0];
        if (!np) { np = newProject('My Song', true); await DB.putProject(np); }
        P = null;
        await openProject(np);
      }
      openProjects();
    }
  });
}
function renameProject(p) {
  openModal('✏️ Rename song', `<div class="mrow"><input type="text" id="rName" value="${esc(p.name)}" maxlength="40"></div><div class="mrow right"><button class="primary" data-mact="ok">Save</button></div>`, async a => {
    if (a === 'close') return openProjects();
    if (a !== 'ok') return;
    const name = $('#rName').value.trim() || 'Untitled';
    if (p.id === P.id) { P.name = name; await saveNow(); renderTop(); }
    else { const full = await DB.getProject(p.id); full.name = name; await DB.putProject(full); }
    openProjects();
  });
  const inp = $('#rName');
  inp.focus();
  inp.select();
}
function openHelp() {
  openModal('❔ How to use Pocket Studio', `<div class="help">
    <p><b>Tracks screen</b> — every row is an instrument. Tap a row to open it. <b>M</b> mutes, <b>S</b> solos, <b>⋯</b> has volume, pan, reverb, duplicate and delete.</p>
    <p><b>▶ Play / ■ Stop</b> loops your song. <b>●</b> records what you play (a 1-bar count-in plays first). Tap the tempo number or <b>⚙</b> to change tempo, key, length and swing.</p>
    <p><b>Instruments</b> — <i>Keys</i> is a real keyboard, <i>Scale</i> only shows notes that sound good together, <i>Chords</i> gives you one-tap chords (like GarageBand's Smart Instruments). Switch to <b>✏️ Edit</b> to draw notes: tap to add, tap a note to delete, drag a note sideways to make it longer, drag empty space to scroll. <b>✨ Ideas</b> writes chords, bass lines or melodies for you.</p>
    <p><b>Drums</b> — tap squares in the <i>Beat Grid</i> (drag to paint several) or finger-drum on the <i>Pads</i>. <b>✨ Beats</b> loads ready-made grooves.</p>
    <p><b>Voice / Mic</b> — add a Voice track to sing or record a real instrument. You can also import an audio file.</p>
    <p><b>↶</b> undoes mistakes. Songs save automatically. <b>⤓</b> exports a WAV to your Download/PocketStudio folder.</p>
  </div>`, a => { if (a === 'close') closeModal(); });
}

// ---------- event wiring ----------
document.addEventListener('click', async e => {
  const mb = e.target.closest('[data-mact]');
  if (mb && $('#modal').contains(mb)) {
    const a = mb.dataset.mact;
    if (a === 'close') return dismissModal();
    if (modalHandler) modalHandler(a, mb.dataset.v, mb);
    return;
  }
  const b = e.target.closest('[data-act]');
  if (!b) return;
  const a = b.dataset.act, v = b.dataset.v;
  const lane = b.closest('[data-id]');
  const t = lane ? P.tracks.find(x => x.id === lane.dataset.id) : currentTrack();
  switch (a) {
    case 'open': UI.sel = t.id; UI.view = 'editor'; render(); break;
    case 'mute': t.mute = !t.mute; syncBuses(); changed(); break;
    case 'solo': t.solo = !t.solo; syncBuses(); changed(); break;
    case 'trackMenu': UI.sel = t.id; openTrackMenu(t); break;
    case 'addTrack': openAddTrack(); break;
    case 'back': UI.view = 'tracks'; render(); break;
    case 'pickInst': openInstPicker(t); break;
    case 'instMode': UI.instMode = v; renderMain(); break;
    case 'kbMode': UI.kbMode = v; renderMain(); break;
    case 'strum': UI.strum = !UI.strum; renderMain(); break;
    case 'oct': t.oct = clamp(t.oct + +v, 0, 7); changed(); break;
    case 'noteLen': UI.noteLen = +v; renderMain(); break;
    case 'zoom': UI.zoom = clamp(UI.zoom * (+v > 0 ? 1.4 : 1 / 1.4), 0.35, 3); renderMain(); break;
    case 'ideas': openIdeas(t); break;
    case 'beats': openBeats(t); break;
    case 'clearNotes':
      if (t.notes.length && (await ask('Clear all notes on this track?', 'Clear'))) { pushUndo(); t.notes = []; changed(); }
      break;
    case 'drumMode': UI.drumMode = v; renderMain(); break;
    case 'page': UI.drumPage = (UI.drumPage + +v + P.bars) % P.bars; renderMain(); break;
    case 'copyBar': {
      pushUndo();
      const src = t.notes.filter(n => Math.floor(n.s / 16) === UI.drumPage).map(n => ({ ...n, s: n.s % 16 }));
      t.notes = [];
      for (let bar = 0; bar < P.bars; bar++) t.notes.push(...src.map(n => ({ ...n, s: bar * 16 + n.s })));
      toast(`Bar ${UI.drumPage + 1} copied to all bars`);
      changed();
      break;
    }
    case 'recAudio': recordAudio(t); break;
    case 'clearAudio':
      if (t.audioId && (await ask('Remove this recording?', 'Remove'))) { pushUndo(); t.audioId = null; changed(); }
      break;
  }
});

function bindTop() {
  $('#btnPlay').addEventListener('click', togglePlay);
  $('#btnRec').addEventListener('click', toggleRecord);
  $('#btnRewind').addEventListener('click', rewind);
  $('#btnUndo').addEventListener('click', undo);
  $('#btnMetro').addEventListener('click', () => { P.metro = !P.metro; changed('top'); });
  $('#bpmDown').addEventListener('click', () => setBpm(P.bpm - 1));
  $('#bpmUp').addEventListener('click', () => setBpm(P.bpm + 1));
  $('#bpmVal').addEventListener('click', openSong);
  $('#btnSong').addEventListener('click', openSong);
  $('#btnExport').addEventListener('click', openExport);
  $('#btnProjects').addEventListener('click', openProjects);
  $('#projName').addEventListener('click', openProjects);
  $('#modal').addEventListener('pointerdown', e => { if (e.target.id === 'modal') dismissModal(); });
  document.addEventListener('pointerdown', () => { if (P) audio(); }, { capture: true });
  document.addEventListener('contextmenu', e => e.preventDefault());
  document.addEventListener('keydown', e => {
    if (e.target.tagName === 'INPUT') return;
    if (e.code === 'Space') { e.preventDefault(); togglePlay(); }
    if (e.key === 'r') toggleRecord();
    if ((e.ctrlKey || e.metaKey) && e.key === 'z') undo();
  });
  let rt = null;
  window.addEventListener('resize', () => { clearTimeout(rt); rt = setTimeout(() => { if (P) renderMain(); }, 150); });
  document.addEventListener('visibilitychange', () => { if (document.hidden && P) window.PS_pause(); });
}

async function boot() {
  bindTop();
  let p = null;
  try {
    const last = Settings.get('last', null);
    if (last) p = await DB.getProject(last);
    if (!p) p = (await DB.listProjects()).sort((a, b) => b.updated - a.updated)[0];
  } catch (e) { /* fresh start */ }
  const first = !p;
  if (!p) {
    p = newProject('My First Song', true);
    try { await DB.putProject(p); } catch (e) {}
  }
  await openProject(p);
  if (first) openHelp();
}
boot();
