'use strict';
// Core: project state, transport/scheduler, live input + recording, mic capture, export.

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const uid = () => Math.random().toString(36).slice(2, 9) + Date.now().toString(36).slice(-5);
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const esc = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

let P = null;                 // current project (plain JSON)
let ctx = null, eng = null;   // live AudioContext + Engine
const RAW = new Map();        // audioId -> {sr, ch:[Float32Array]}
const UI = {
  view: 'tracks', sel: null,
  instMode: 'play', kbMode: 'keys', strum: true,
  noteLen: 2, zoom: 1, roll: {},
  drumMode: 'grid', drumPage: 0,
  recording: false,
};
const T = { playing: false, step: 0, nextTime: 0, timer: null, queue: [], endAt: null, capture: null };
const undoStack = [];

// ---------- audio context ----------
function audio() {
  if (!ctx) {
    const AC = window.AudioContext || window.webkitAudioContext;
    ctx = new AC({ latencyHint: 'interactive' });
    eng = new Engine(ctx);
    if (P) syncBuses(true);
  }
  if (ctx.state === 'suspended') ctx.resume();
  return eng;
}
const anySolo = () => P.tracks.some(t => t.solo);
const audible = t => !t.mute && (!anySolo() || t.solo);
function syncBuses(immediate) {
  if (!eng || !P) return;
  for (const t of P.tracks) eng.updateBus(t, audible(t), immediate);
}
const stepDur = () => 60 / P.bpm / 4;
const totalSteps = () => P.bars * 16;
const loopDur = () => totalSteps() * stepDur();
const currentTrack = () => P && P.tracks.find(t => t.id === UI.sel);

// ---------- project model ----------
function trackColor(t) {
  if (t.type === 'drums') return '#ffb020';
  if (t.type === 'audio') return '#ef476f';
  return (INST[t.instrument] || INSTRUMENTS[0]).color;
}
function trackIcon(t) {
  if (t.type === 'drums') return '🥁';
  if (t.type === 'audio') return '🎤';
  return (INST[t.instrument] || INSTRUMENTS[0]).icon;
}
function newTrack(type, instrument) {
  const t = { id: uid(), type, instrument: null, kit: null, name: '', vol: 0.8, pan: 0, reverb: 0.12, mute: false, solo: false, notes: [], audioId: null, nudge: 0, oct: 4 };
  if (type === 'drums') { t.kit = instrument || 'acoustic'; t.name = 'Drums'; t.reverb = 0.06; }
  else if (type === 'audio') { t.name = 'Voice'; t.reverb = 0.15; }
  else {
    const i = INST[instrument] || INSTRUMENTS[0];
    t.instrument = i.id; t.name = i.name; t.oct = i.oct;
    if (['pad', 'strings', 'bells'].includes(i.id)) t.reverb = 0.3;
    if (['bass', 'sub808'].includes(i.id)) t.reverb = 0.02;
  }
  return t;
}
function newProject(name, starter) {
  const p = { id: uid(), name, bpm: 100, bars: 4, key: 0, scale: 'major', swing: 0, metro: false, countIn: true, created: Date.now(), updated: Date.now(), tracks: [] };
  if (starter) {
    const d = newTrack('drums', 'acoustic');
    d.notes = beatNotes(BEATS[0], p.bars);
    const k = newTrack('inst', 'piano');
    k.notes = generateIdea(PROGRESSIONS.major[0], 'rhythm', p, k.oct);
    const b = newTrack('inst', 'bass');
    b.notes = generateIdea(PROGRESSIONS.major[0], 'bass', p, b.oct);
    b.vol = 0.75;
    p.tracks.push(d, k, b);
  }
  return p;
}

// ---------- persistence + undo ----------
let saveTimer = null;
function saveNow() {
  clearTimeout(saveTimer);
  saveTimer = null;
  if (!P) return Promise.resolve();
  Settings.set('last', P.id);
  return DB.putProject(JSON.parse(JSON.stringify(P))).catch(e => toast('Save failed: ' + e.message));
}
// what: 'all' re-renders the whole view, 'top' just the top bar, 'none' only saves
function changed(what = 'all') {
  P.updated = Date.now();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(saveNow, 700);
  if (what === 'all') render();
  else if (what === 'top') renderTop();
}
function pushUndo() {
  undoStack.push(JSON.stringify(P));
  if (undoStack.length > 60) undoStack.shift();
  const b = document.getElementById('btnUndo');
  if (b) b.disabled = false;
}
function undo() {
  if (!undoStack.length) return toast('Nothing to undo');
  const id = P.id;
  P = JSON.parse(undoStack.pop());
  P.id = id;
  if (!currentTrack()) UI.view = 'tracks';
  syncBuses();
  changed();
  toast('Undone');
}
async function openProject(p) {
  stop();
  P = p;
  undoStack.length = 0;
  UI.view = 'tracks';
  UI.sel = p.tracks[0] ? p.tracks[0].id : null;
  for (const t of p.tracks) {
    if (t.audioId && !RAW.has(t.audioId)) {
      const raw = await DB.getAudio(t.audioId).catch(() => null);
      if (raw) RAW.set(t.audioId, raw);
    }
  }
  if (eng) { for (const id of [...eng.buses.keys()]) eng.removeBus(id); syncBuses(true); }
  Settings.set('last', p.id);
  render();
}

// ---------- transport ----------
function play(countIn = false) {
  const e = audio();
  if (!e || T.playing) return;
  syncBuses();
  T.playing = true;
  T.step = countIn ? -16 : 0;
  T.nextTime = ctx.currentTime + 0.08;
  T.queue = [];
  T.endAt = null;
  T.timer = setInterval(tick, 25);
  tick();
  onTransport();
}
function stop() {
  if (!T.playing) return;
  T.playing = false;
  clearInterval(T.timer);
  T.timer = null;
  eng.killAll(ctx.currentTime);
  finishHeld();
  UI.recording = !!T.capture; // audio capture keeps running briefly to catch the tail
  if (T.capture && T.capture.stopAt == null) T.capture.stopAt = ctx.currentTime;
  onTransport();
}
function togglePlay() { if (T.playing) stop(); else play(false); }
function rewind() { if (T.playing) { stop(); play(false); } }

function tick() {
  if (!T.playing) return;
  const now = ctx.currentTime;
  if (T.endAt != null && now >= T.endAt) { stop(); return; }
  if (T.nextTime < now - 0.25) T.nextTime = now + 0.02; // we fell behind (app was throttled)
  while (T.nextTime < now + 0.12 && (T.endAt == null || T.nextTime < T.endAt - 1e-4)) {
    scheduleStep(T.step, T.nextTime);
    T.queue.push({ s: T.step, t: T.nextTime });
    if (T.queue.length > 48) T.queue.shift();
    T.nextTime += stepDur();
    T.step++;
    if (T.step >= totalSteps()) {
      T.step = 0;
      if (T.capture && T.capture.loopStart != null) T.endAt = T.nextTime; // one loop recorded
    }
  }
}

function scheduleStep(s, time) {
  if (s < 0) { // count-in bar
    if (s % 4 === 0) eng.click(time, s === -16);
    return;
  }
  if (s === 0 && T.capture && T.capture.loopStart == null) T.capture.loopStart = time;
  if (P.metro && s % 4 === 0) eng.click(time, s % 16 === 0);
  scheduleNotes(eng, s, time, T.capture ? T.capture.track : null);
}

// shared by live playback and offline export
function scheduleNotes(e, s, time, skipTrack) {
  const sd = stepDur();
  const t = time + (s % 2 === 1 ? P.swing * sd * 0.5 : 0);
  for (const tr of P.tracks) {
    if (!audible(tr) || tr === skipTrack) continue;
    if (tr.type === 'audio') {
      if (s === 0 && tr.audioId) playClip(e, tr, time);
      continue;
    }
    for (const n of tr.notes) {
      if (n.s !== s) continue;
      if (tr.type === 'drums') e.drum(tr, DRUMS[n.n].id, t, n.v);
      else e.note(tr, n.n, t, n.l * sd * 0.97, n.v);
    }
  }
}
function playClip(e, tr, time) {
  const buf = e.buffer(tr.audioId, RAW.get(tr.audioId));
  if (!buf) return;
  const off = (tr.nudge || 0) / 1000, ld = loopDur();
  if (off >= 0) e.clip(tr, buf, time + off, 0, Math.min(buf.duration, ld - off));
  else e.clip(tr, buf, time, -off, Math.min(buf.duration + off, ld));
}

// position (in steps) of what the listener hears right now; null when stopped
function curPos() {
  if (!T.playing || !T.queue.length || !ctx) return null;
  const now = ctx.currentTime - (ctx.outputLatency || ctx.baseLatency || 0);
  let e = null;
  for (const q of T.queue) if (q.t <= now) e = q;
  if (!e) return T.queue[0].s - 1;
  return e.s + clamp((now - e.t) / stepDur(), 0, 1);
}

// ---------- live playing + recording ----------
const held = new Map(); // key -> {v, rec, tr, n, vel}
function liveOn(key, tr, n, vel = 0.85, delay = 0) {
  const e = audio();
  const t = ctx.currentTime + delay;
  if (held.has(key)) liveOff(key);
  let rec = null;
  if (UI.recording && T.playing && !T.capture) {
    const p = curPos();
    if (p !== null && p >= -0.5) rec = { start: p };
  }
  if (tr.type === 'drums') {
    e.drum(tr, DRUMS[n].id, t, vel);
    if (rec) addRecNote(tr, n, rec.start, 1, vel);
    return;
  }
  held.set(key, { v: e.note(tr, n, t, null, vel), rec, tr, n, vel });
}
function liveOff(key) {
  const h = held.get(key);
  if (!h) return;
  held.delete(key);
  h.v.release(ctx.currentTime);
  if (h.rec) {
    const p = curPos();
    let len = (p == null ? h.rec.start + 1 : p) - h.rec.start;
    if (len < 0) len += totalSteps();
    addRecNote(h.tr, h.n, h.rec.start, len, h.vel);
  }
}
function finishHeld() { for (const k of [...held.keys()]) liveOff(k); }
function addRecNote(tr, n, start, len, v) {
  const tot = totalSteps();
  const s = ((Math.round(start) % tot) + tot) % tot;
  const l = clamp(Math.round(len), 1, tot);
  if (tr.notes.some(x => x.s === s && x.n === n)) return;
  tr.notes.push({ s, n, l, v: Math.round(v * 100) / 100 });
  changed('none');
  if (typeof onNotesRecorded === 'function') onNotesRecorded(tr);
}
function previewNote(tr, n, vel = 0.8) {
  const e = audio();
  if (tr.type === 'drums') e.drum(tr, DRUMS[n].id, ctx.currentTime, vel);
  else e.note(tr, n, ctx.currentTime, 0.35, vel);
}

function toggleRecord() {
  const tr = currentTrack();
  if (UI.view === 'editor' && tr && tr.type === 'audio') return recordAudio(tr);
  if (T.capture) return stopCapture();
  if (UI.recording) {
    UI.recording = false;
    finishHeld();
    onTransport();
    return;
  }
  if (UI.view !== 'editor' || !tr) toast('Open a track to record into it');
  pushUndo();
  UI.recording = true;
  if (!T.playing) play(P.countIn);
  onTransport();
}

// ---------- microphone capture ----------
let mic = null;
async function openMic() {
  audio();
  if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) throw new Error('not supported here');
  const stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false } });
  const src = ctx.createMediaStreamSource(stream);
  const proc = ctx.createScriptProcessor(2048, 1, 1);
  const sink = ctx.createGain();
  sink.gain.value = 0;
  src.connect(proc); proc.connect(sink); sink.connect(ctx.destination);
  mic = { stream, src, proc, sink, level: 0 };
  proc.onaudioprocess = onMicData;
  startRaf();
}
function closeMic() {
  if (!mic) return;
  mic.proc.onaudioprocess = null;
  try { mic.src.disconnect(); mic.proc.disconnect(); mic.sink.disconnect(); } catch (e) {}
  mic.stream.getTracks().forEach(t => t.stop());
  mic = null;
}
function onMicData(ev) {
  const d = ev.inputBuffer.getChannelData(0);
  let pk = 0;
  for (let i = 0; i < d.length; i += 4) pk = Math.max(pk, Math.abs(d[i]));
  mic.level = Math.max(pk, mic.level * 0.85);
  const c = T.capture;
  if (!c) return;
  if (c.firstTime == null) c.firstTime = ctx.currentTime - d.length / ctx.sampleRate;
  c.chunks.push(new Float32Array(d));
  if (c.stopAt != null && ctx.currentTime > c.stopAt + 0.35) finalizeCapture();
}
async function recordAudio(tr) {
  if (T.capture) return stopCapture();
  try {
    if (!mic) await openMic();
  } catch (e) {
    toast('Microphone unavailable: ' + (e.message || e.name));
    return;
  }
  stop();
  pushUndo();
  T.capture = { track: tr, chunks: [], firstTime: null, loopStart: null, stopAt: null };
  UI.recording = true;
  play(true);
  onTransport();
}
function stopCapture() {
  if (!T.capture) return;
  if (T.playing) stop();
  else if (T.capture.stopAt == null) T.capture.stopAt = ctx.currentTime;
}
async function finalizeCapture() {
  const c = T.capture;
  T.capture = null;
  UI.recording = false;
  closeMic();
  onTransport();
  if (c.loopStart == null || c.firstTime == null || !c.chunks.length) { toast('Recording cancelled'); return; }
  const sr = ctx.sampleRate;
  const total = c.chunks.reduce((a, b) => a + b.length, 0);
  const all = new Float32Array(total);
  let o = 0;
  for (const ch of c.chunks) { all.set(ch, o); o += ch.length; }
  const lat = (ctx.baseLatency || 0) + (ctx.outputLatency || 0);
  const i0 = clamp(Math.round((c.loopStart + lat - c.firstTime) * sr), 0, total);
  const i1 = clamp(Math.min(i0 + Math.round(loopDur() * sr), Math.round((c.stopAt + lat - c.firstTime) * sr)), i0, total);
  if (i1 - i0 < sr * 0.2) { toast('Recording too short'); return; }
  const pcm = all.slice(i0, i1);
  let pk = 0;
  for (let i = 0; i < pcm.length; i++) pk = Math.max(pk, Math.abs(pcm[i]));
  const gain = pk > 0 ? Math.min(8, 0.9 / pk) : 1; // gentle auto-level
  if (gain > 1) for (let i = 0; i < pcm.length; i++) pcm[i] *= gain;
  await storeAudio(c.track, { sr, ch: [pcm] });
  toast('Recorded! 🎉');
}
async function storeAudio(tr, raw) {
  const id = uid();
  RAW.set(id, raw);
  try { await DB.putAudio(id, raw); } catch (e) { toast('Could not store audio: ' + e.message); }
  tr.audioId = id;
  tr.nudge = 0;
  changed();
}
async function importAudio(tr, file) {
  audio();
  toast('Loading audio…');
  try {
    const buf = await ctx.decodeAudioData(await file.arrayBuffer());
    const len = Math.min(buf.length, Math.round(buf.sampleRate * 60));
    const ch = [];
    for (let i = 0; i < Math.min(2, buf.numberOfChannels); i++) ch.push(buf.getChannelData(i).slice(0, len));
    pushUndo();
    await storeAudio(tr, { sr: buf.sampleRate, ch });
    toast(buf.length > len ? 'Imported (first 60 seconds)' : 'Imported!');
  } catch (e) {
    toast('Could not read that file');
  }
}

// ---------- export ----------
async function renderSong(loops) {
  const sr = 44100;
  const dur = loopDur() * loops + 2.5;
  const off = new OfflineAudioContext(2, Math.ceil(sr * dur), sr);
  const e2 = new Engine(off);
  for (const t of P.tracks) e2.updateBus(t, audible(t), true);
  const sd = stepDur(), n = totalSteps();
  for (let L = 0; L < loops; L++) {
    for (let s = 0; s < n; s++) scheduleNotes(e2, s, 0.02 + (L * n + s) * sd, null);
  }
  return off.startRendering();
}
function b64(u8) {
  let s = '';
  for (let i = 0; i < u8.length; i += 8192) s += String.fromCharCode.apply(null, u8.subarray(i, i + 8192));
  return btoa(s);
}
async function saveFile(name, bytes, mime) {
  if (window.AndroidBridge) {
    AndroidBridge.fileBegin(name);
    const CH = 3 * 65536;
    for (let i = 0; i < bytes.length; i += CH) AndroidBridge.fileChunk(b64(bytes.subarray(i, i + CH)));
    const where = AndroidBridge.fileEnd(name, mime);
    return where ? { where, share: AndroidBridge.canShare() } : null;
  }
  const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
  return { where: 'your Downloads', share: false };
}

// ---------- Android hooks ----------
window.PS_pause = () => {
  if (T.capture) stopCapture();
  stop();
  finishHeld();
  saveNow();
};
window.PS_back = () => {
  if (dismissModal()) return true;
  if (UI.view === 'editor') { UI.view = 'tracks'; render(); return true; }
  return false;
};
