'use strict';
// Pocket Studio audio engine: synthesized instruments + drum kits on Web Audio.
// An Engine wraps one AudioContext (live) or OfflineAudioContext (export).

const mtof = m => 440 * Math.pow(2, (m - 69) / 12);

const INSTRUMENTS = [
  { id: 'piano',   name: 'Grand Piano',     icon: '🎹', color: '#4ea8de', oct: 4 },
  { id: 'epiano',  name: 'Electric Piano',  icon: '🎹', color: '#5e60ce', oct: 4 },
  { id: 'organ',   name: 'Organ',           icon: '⛪', color: '#7b2cbf', oct: 4 },
  { id: 'guitar',  name: 'Acoustic Guitar', icon: '🎸', color: '#e9a23b', oct: 3 },
  { id: 'bass',    name: 'Bass Guitar',     icon: '🎸', color: '#43aa8b', oct: 2 },
  { id: 'sub808',  name: '808 Bass',        icon: '🔊', color: '#2a9d8f', oct: 2 },
  { id: 'synth',   name: 'Synth Lead',      icon: '⚡', color: '#f72585', oct: 4 },
  { id: 'pluck',   name: 'Synth Pluck',     icon: '💧', color: '#4cc9f0', oct: 4 },
  { id: 'pad',     name: 'Warm Pad',        icon: '☁️', color: '#9d8df1', oct: 3 },
  { id: 'strings', name: 'Strings',         icon: '🎻', color: '#d62828', oct: 3 },
  { id: 'bells',   name: 'Bells',           icon: '🔔', color: '#f4d35e', oct: 5 },
];
const INST = Object.fromEntries(INSTRUMENTS.map(i => [i.id, i]));

const DRUMS = [
  { id: 'kick',  name: 'Kick' },
  { id: 'snare', name: 'Snare' },
  { id: 'clap',  name: 'Clap' },
  { id: 'hat',   name: 'Hi-Hat' },
  { id: 'ohat',  name: 'Open Hat' },
  { id: 'tomlo', name: 'Low Tom' },
  { id: 'tomhi', name: 'High Tom' },
  { id: 'crash', name: 'Crash' },
];
const KITS = [
  { id: 'acoustic', name: 'Studio Kit' },
  { id: 'electro',  name: 'Electro Kit' },
  { id: '808',      name: '808 Trap Kit' },
  { id: 'lofi',     name: 'Lo-Fi Kit' },
];

function holdParam(p, t) {
  if (p.cancelAndHoldAtTime) p.cancelAndHoldAtTime(t);
  else p.cancelScheduledValues(t);
}

class Voice {
  constructor(eng, dest, t) {
    this.e = eng;
    this.ctx = eng.ctx;
    this.t = t;
    this.out = this.ctx.createGain();
    this.out.connect(dest);
    this.srcs = [];
    this.rel = 0.2;
    this.released = false;
    eng.active.add(this);
  }
  osc(type, freq, detune = 0) {
    const o = this.ctx.createOscillator();
    o.type = type;
    o.frequency.value = freq;
    if (detune) o.detune.value = detune;
    o.start(this.t);
    this.srcs.push(o);
    return o;
  }
  noise(loop = false) {
    const s = this.ctx.createBufferSource();
    s.buffer = this.e.noiseBuf;
    s.loop = loop;
    s.start(this.t, Math.random() * 0.5);
    this.srcs.push(s);
    return s;
  }
  gain(v, from) {
    const g = this.ctx.createGain();
    g.gain.value = v;
    if (from) from.connect(g);
    return g;
  }
  filter(type, freq, q = 0.7) {
    const f = this.ctx.createBiquadFilter();
    f.type = type;
    f.frequency.value = freq;
    f.Q.value = q;
    return f;
  }
  // attack to peak, then decay toward `sustain` fraction
  adsr(a, peak, d, sustain) {
    const g = this.out.gain, t = this.t;
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(peak, t + a);
    g.setTargetAtTime(peak * sustain, t + a, d);
  }
  finish(t) {
    for (const s of this.srcs) { try { s.stop(t); } catch (e) { /* already stopped */ } }
    if (this.srcs[0] && !this.srcs[0].onended) {
      this.srcs[0].onended = () => { this.e.active.delete(this); try { this.out.disconnect(); } catch (e) {} };
    }
  }
  release(t) {
    if (this.released) return;
    this.released = true;
    t = Math.max(t, this.t + 0.005);
    const g = this.out.gain;
    holdParam(g, t);
    g.setTargetAtTime(0, t, this.rel / 4);
    this.finish(t + this.rel * 1.5 + 0.05);
  }
  kill(now) {
    this.released = true;
    const g = this.out.gain;
    holdParam(g, now);
    g.setTargetAtTime(0, now, 0.008);
    this.finish(now + 0.06);
    this.e.active.delete(this);
  }
}

// ---- melodic instruments: fn(v, midi, vel) builds the voice at v.t ----
const VOICES = {
  piano(v, m, vel) {
    const c = v.ctx, t = v.t, f = mtof(m);
    const low = 1 - (m - 21) / 88;
    const lp = v.filter('lowpass', 1000, 0.4);
    lp.frequency.setValueAtTime(Math.min(1500 + vel * 5000 + f * 2, 18000), t);
    lp.frequency.setTargetAtTime(Math.min(f * 2.5 + 500, 18000), t + 0.01, 0.3 + low * 0.8);
    lp.connect(v.out);
    for (const [type, mul, det, g] of [['triangle', 1, 0, 0.6], ['sine', 2, 4, 0.22], ['sine', 3, -3, 0.07], ['sawtooth', 1, 6, 0.05]]) {
      v.gain(g, v.osc(type, f * mul, det)).connect(lp);
    }
    const hammer = v.filter('bandpass', Math.min(f * 4, 9000), 1);
    const hg = v.gain(0, v.noise());
    hg.gain.setValueAtTime(0.12 * vel, t);
    hg.gain.setTargetAtTime(0, t, 0.01);
    hg.connect(hammer); hammer.connect(v.out);
    const peak = 0.5 * (0.3 + 0.7 * vel);
    const g = v.out.gain;
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(peak, t + 0.004);
    g.setTargetAtTime(peak * 0.35, t + 0.004, 0.12);
    g.setTargetAtTime(0, t + 0.3, 0.5 + low * 1.8);
    v.rel = 0.3;
    v.finish(t + 9);
  },
  epiano(v, m, vel) {
    const c = v.ctx, t = v.t, f = mtof(m);
    const car = v.osc('sine', f);
    const mod = v.osc('sine', f);
    const mg = v.gain(0, mod);
    mg.gain.setValueAtTime(f * (0.6 + vel * 2), t);
    mg.gain.setTargetAtTime(f * 0.25, t, 0.25);
    mg.connect(car.frequency);
    const tine = v.osc('sine', f * 14);
    const tg = v.gain(0, tine);
    tg.gain.setValueAtTime(f * 1.5 * vel, t);
    tg.gain.setTargetAtTime(0, t, 0.02);
    tg.connect(car.frequency);
    car.connect(v.out);
    const peak = 0.42 * (0.3 + 0.7 * vel);
    const g = v.out.gain;
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(peak, t + 0.003);
    g.setTargetAtTime(peak * 0.5, t + 0.003, 0.3);
    g.setTargetAtTime(0, t + 0.5, 1.4);
    v.rel = 0.25;
    v.finish(t + 8);
  },
  organ(v, m, vel) {
    const c = v.ctx, t = v.t, f = mtof(m);
    const lfo = c.createOscillator();
    lfo.frequency.value = 6.2;
    const lg = v.gain(7, lfo);
    for (const [mul, g] of [[0.5, 0.3], [1, 0.45], [2, 0.32], [3, 0.16], [4, 0.16], [6, 0.06], [8, 0.08]]) {
      const o = v.osc('sine', f * mul);
      lg.connect(o.detune);
      v.gain(g, o).connect(v.out);
    }
    lfo.start(t);
    v.srcs.push(lfo);
    v.adsr(0.008, 0.26 * (0.6 + 0.4 * vel), 0.1, 1);
    v.rel = 0.08;
  },
  guitar(v, m, vel) {
    const c = v.ctx, t = v.t;
    const ks = v.e.ksBuffer(m);
    const s = c.createBufferSource();
    s.buffer = ks.buf;
    s.playbackRate.value = ks.rate;
    s.start(t);
    v.srcs.push(s);
    const lp = v.filter('lowpass', 2200 + vel * 4500, 0.5);
    const body = v.filter('peaking', 180, 1.2);
    body.gain.value = 4;
    s.connect(lp); lp.connect(body); body.connect(v.out);
    v.out.gain.setValueAtTime(1.1 * (0.3 + 0.7 * vel), t);
    v.rel = 0.12;
    v.finish(t + ks.buf.duration / ks.rate);
  },
  bass(v, m, vel) {
    const t = v.t, f = mtof(m);
    const saw = v.osc('sawtooth', f);
    const lp = v.filter('lowpass', 800, 2);
    lp.frequency.setValueAtTime(f * 4 + 300 + vel * 1200, t);
    lp.frequency.setTargetAtTime(f * 1.5 + 120, t, 0.12);
    saw.connect(lp); lp.connect(v.out);
    v.gain(0.6, v.osc('sine', f)).connect(v.out);
    v.adsr(0.005, 0.5 * (0.4 + 0.6 * vel), 0.4, 0.55);
    v.rel = 0.07;
  },
  sub808(v, m, vel) {
    const t = v.t, f = mtof(m);
    const o = v.osc('sine', f);
    o.frequency.setValueAtTime(f * 2.2, t);
    o.frequency.exponentialRampToValueAtTime(f, t + 0.06);
    const ws = v.ctx.createWaveShaper();
    ws.curve = v.e.distCurve;
    o.connect(ws); ws.connect(v.out);
    const peak = 0.5 * (0.4 + 0.6 * vel);
    const g = v.out.gain;
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(peak, t + 0.004);
    g.setTargetAtTime(0, t + 0.15, 1.2);
    v.rel = 0.12;
    v.finish(t + 7);
  },
  synth(v, m, vel) {
    const t = v.t, f = mtof(m);
    const lp = v.filter('lowpass', 2000, 5);
    lp.frequency.setValueAtTime(Math.min(f * 8 + 3000 * vel, 16000), t);
    lp.frequency.setTargetAtTime(f * 3 + 600, t, 0.25);
    v.gain(0.3, v.osc('sawtooth', f, -7)).connect(lp);
    v.gain(0.3, v.osc('sawtooth', f, 7)).connect(lp);
    v.gain(0.2, v.osc('square', f / 2)).connect(lp);
    lp.connect(v.out);
    v.adsr(0.01, 0.3 * (0.5 + 0.5 * vel), 0.2, 0.75);
    v.rel = 0.18;
  },
  pluck(v, m, vel) {
    const t = v.t, f = mtof(m);
    const lp = v.filter('lowpass', 2000, 6);
    lp.frequency.setValueAtTime(Math.min(f * 10 + 4000 * vel, 16000), t);
    lp.frequency.setTargetAtTime(f * 1.2 + 200, t, 0.09);
    v.gain(0.35, v.osc('square', f)).connect(lp);
    v.gain(0.3, v.osc('sawtooth', f, 6)).connect(lp);
    lp.connect(v.out);
    const peak = 0.36 * (0.4 + 0.6 * vel);
    const g = v.out.gain;
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(peak, t + 0.003);
    g.setTargetAtTime(0, t + 0.003, 0.35);
    v.rel = 0.15;
    v.finish(t + 3);
  },
  pad(v, m, vel) {
    const c = v.ctx, t = v.t, f = mtof(m);
    const lp = v.filter('lowpass', 900 + f * 1.5, 0.7);
    const lfo = c.createOscillator();
    lfo.frequency.value = 0.25;
    v.gain(400, lfo).connect(lp.frequency);
    for (const d of [-14, -5, 5, 14]) v.gain(0.11, v.osc('sawtooth', f, d)).connect(lp);
    v.gain(0.12, v.osc('sine', f / 2)).connect(lp);
    lfo.start(t);
    v.srcs.push(lfo);
    lp.connect(v.out);
    const g = v.out.gain;
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(0.6 * (0.5 + 0.5 * vel), t + 0.7);
    v.rel = 1.2;
  },
  strings(v, m, vel) {
    const c = v.ctx, t = v.t, f = mtof(m);
    const lfo = c.createOscillator();
    lfo.frequency.value = 5.3;
    const lg = v.gain(0, lfo);
    lg.gain.setValueAtTime(0, t);
    lg.gain.linearRampToValueAtTime(9, t + 0.5);
    const lp = v.filter('lowpass', 2400 + f, 0.5);
    for (const d of [-8, 0, 8]) {
      const o = v.osc('sawtooth', f, d);
      lg.connect(o.detune);
      v.gain(0.14, o).connect(lp);
    }
    lfo.start(t);
    v.srcs.push(lfo);
    lp.connect(v.out);
    const g = v.out.gain;
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(0.55 * (0.5 + 0.5 * vel), t + 0.18);
    v.rel = 0.45;
  },
  bells(v, m, vel) {
    const t = v.t, f = mtof(m);
    const car = v.osc('sine', f);
    const mod = v.osc('sine', f * 3.5);
    const mg = v.gain(0, mod);
    mg.gain.setValueAtTime(f * 2 * vel, t);
    mg.gain.setTargetAtTime(0, t, 0.6);
    mg.connect(car.frequency);
    car.connect(v.out);
    v.gain(0.15, v.osc('sine', f * 2.001)).connect(v.out);
    const peak = 0.34 * (0.4 + 0.6 * vel);
    const g = v.out.gain;
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(peak, t + 0.002);
    g.setTargetAtTime(0, t + 0.002, 0.9);
    v.rel = 0.6;
    v.finish(t + 6);
  },
};

// ---- drums: fn(v, vel, kit) — one-shots, each calls v.finish ----
const DRUM_VOICES = {
  kick(v, vel, kit) {
    const t = v.t, is808 = kit === '808';
    const f0 = is808 ? 110 : kit === 'electro' ? 170 : 140, f1 = is808 ? 42 : 50;
    const dec = is808 ? 0.9 : kit === 'electro' ? 0.35 : 0.28;
    const o = v.osc('sine', f0);
    o.frequency.setValueAtTime(f0, t);
    o.frequency.exponentialRampToValueAtTime(f1, t + (is808 ? 0.12 : 0.08));
    const g = v.gain(0, o);
    g.gain.setValueAtTime(vel * 1.1, t);
    g.gain.setTargetAtTime(0, t + 0.01, dec / 3.5);
    if (is808) {
      const ws = v.ctx.createWaveShaper();
      ws.curve = v.e.distCurve;
      g.connect(ws); ws.connect(v.out);
    } else {
      g.connect(v.out);
      const hp = v.filter('highpass', 3000);
      const cg = v.gain(0, v.noise());
      cg.gain.setValueAtTime(0.25 * vel, t);
      cg.gain.setTargetAtTime(0, t, 0.004);
      cg.connect(hp); hp.connect(v.out);
    }
    v.finish(t + dec * 1.6 + 0.05);
  },
  snare(v, vel, kit) {
    const t = v.t, bf = kit === '808' ? 210 : 185;
    const body = v.osc('triangle', bf);
    body.frequency.setValueAtTime(bf * 1.4, t);
    body.frequency.exponentialRampToValueAtTime(bf, t + 0.03);
    const bg = v.gain(0, body);
    bg.gain.setValueAtTime(vel * 0.55, t);
    bg.gain.setTargetAtTime(0, t, 0.05);
    bg.connect(v.out);
    const f = kit === 'electro' ? v.filter('bandpass', 3000, 0.7) : v.filter('highpass', kit === '808' ? 1800 : 1200);
    const ng = v.gain(0, v.noise());
    ng.gain.setValueAtTime(vel * 0.7, t);
    ng.gain.setTargetAtTime(0, t, kit === '808' ? 0.07 : kit === 'electro' ? 0.05 : 0.06);
    ng.connect(f); f.connect(v.out);
    v.finish(t + 0.4);
  },
  clap(v, vel) {
    const t = v.t;
    const bp = v.filter('bandpass', 1400, 1.2);
    const g = v.gain(0, v.noise());
    g.gain.setValueAtTime(0, t);
    for (let i = 0; i < 3; i++) {
      g.gain.setValueAtTime(vel * 1.5, t + i * 0.011);
      g.gain.setTargetAtTime(0.05, t + i * 0.011 + 0.001, 0.003);
    }
    g.gain.setValueAtTime(vel * 1.5, t + 0.033);
    g.gain.setTargetAtTime(0, t + 0.034, 0.06);
    g.connect(bp); bp.connect(v.out);
    v.finish(t + 0.45);
  },
  hat(v, vel, kit) { hatVoice(v, vel, kit, false); },
  ohat(v, vel, kit) { hatVoice(v, vel, kit, true); },
  tomlo(v, vel, kit) { tomVoice(v, vel, kit, 105); },
  tomhi(v, vel, kit) { tomVoice(v, vel, kit, 165); },
  crash(v, vel, kit) {
    const t = v.t;
    const hp = v.filter('highpass', kit === 'lofi' ? 2500 : 4500);
    const pk = v.filter('peaking', 8000, 1);
    pk.gain.value = 6;
    const g = v.gain(0, v.noise(true));
    g.gain.setValueAtTime(vel * 0.4, t);
    g.gain.setTargetAtTime(0, t + 0.01, 0.45);
    g.connect(hp); hp.connect(pk); pk.connect(v.out);
    v.finish(t + 2.4);
  },
};

function hatVoice(v, vel, kit, open) {
  const t = v.t, e = v.e;
  if (e.openHat && e.openHat !== v && e.openHat.t < t) {
    const oh = e.openHat.out.gain; // choke the ringing open hat
    holdParam(oh, t);
    oh.setTargetAtTime(0, t, 0.01);
  }
  const hp = v.filter('highpass', kit === 'lofi' ? 2500 : 7000);
  const g = v.ctx.createGain();
  if (kit === '808' || kit === 'electro') {
    const bp = v.filter('bandpass', 10000, 0.8);
    for (const f of [205.3, 304.4, 369.6, 522.7, 540, 800]) v.gain(0.12, v.osc('square', f * 1.7)).connect(bp);
    bp.connect(g);
  }
  v.gain(0.8, v.noise()).connect(g);
  const tc = open ? 0.12 : kit === '808' ? 0.02 : 0.015;
  g.gain.setValueAtTime(vel * (kit === 'lofi' ? 0.9 : 0.42), t);
  g.gain.setTargetAtTime(0, t, tc);
  g.connect(hp); hp.connect(v.out);
  if (open) e.openHat = v;
  v.finish(t + (open ? 0.9 : 0.18));
}

function tomVoice(v, vel, kit, base) {
  const t = v.t;
  if (kit === '808') base *= 0.9;
  const o = v.osc('sine', base * 1.6);
  o.frequency.setValueAtTime(base * 1.6, t);
  o.frequency.exponentialRampToValueAtTime(base, t + 0.12);
  const g = v.gain(0, o);
  g.gain.setValueAtTime(vel * 0.75, t);
  g.gain.setTargetAtTime(0, t, kit === '808' ? 0.2 : 0.12);
  g.connect(v.out);
  const ng = v.gain(0, v.noise());
  ng.gain.setValueAtTime(vel * 0.08, t);
  ng.gain.setTargetAtTime(0, t, 0.02);
  ng.connect(v.out);
  v.finish(t + 0.9);
}

class Engine {
  constructor(ctx) {
    this.ctx = ctx;
    this.active = new Set();
    this.buses = new Map();
    this.ks = new Map();
    this.bufs = new Map();
    this.openHat = null;
    this.noiseBuf = this.makeNoise();
    this.distCurve = this.makeDist();

    this.master = ctx.createGain();
    this.master.gain.value = 0.8;
    const comp = ctx.createDynamicsCompressor();
    comp.threshold.value = -8;
    comp.knee.value = 6;
    comp.ratio.value = 10;
    comp.attack.value = 0.003;
    comp.release.value = 0.2;
    this.master.connect(comp);
    comp.connect(ctx.destination);

    this.reverbIn = ctx.createGain();
    const rv = ctx.createConvolver();
    rv.buffer = this.makeImpulse(2.4);
    this.reverbIn.connect(rv);
    rv.connect(this.master);
  }

  makeNoise() {
    const sr = this.ctx.sampleRate, b = this.ctx.createBuffer(1, sr * 2, sr), d = b.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
    return b;
  }
  makeDist() {
    const n = 1024, c = new Float32Array(n);
    for (let i = 0; i < n; i++) { const x = i / (n - 1) * 2 - 1; c[i] = Math.tanh(2.5 * x) / Math.tanh(2.5); }
    return c;
  }
  makeImpulse(sec) {
    const sr = this.ctx.sampleRate, len = Math.floor(sr * sec), b = this.ctx.createBuffer(2, len, sr);
    for (let ch = 0; ch < 2; ch++) {
      const d = b.getChannelData(ch);
      for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 3.2) * 0.5;
    }
    return b;
  }
  // Karplus-Strong plucked string, cached per note
  ksBuffer(m) {
    let k = this.ks.get(m);
    if (k) return k;
    const sr = this.ctx.sampleRate, f = mtof(m);
    const N = Math.max(2, Math.floor(sr / f - 0.5));
    const actual = sr / (N + 0.5);
    const len = Math.floor(sr * 3);
    const buf = this.ctx.createBuffer(1, len, sr), d = buf.getChannelData(0);
    const ring = new Float32Array(N);
    let last = 0;
    for (let i = 0; i < N; i++) { last = last * 0.4 + (Math.random() * 2 - 1) * 0.6; ring[i] = last; }
    const T60 = Math.max(1, Math.min(3.5, 3.5 - 2 * (m - 40) / 50));
    const damp = Math.exp(Math.log(0.001) / (f * T60));
    let idx = 0;
    for (let i = 0; i < len; i++) {
      const a = ring[idx], ni = idx + 1 === N ? 0 : idx + 1;
      ring[idx] = damp * 0.5 * (a + ring[ni]);
      d[i] = a;
      idx = ni;
    }
    k = { buf, rate: f / actual };
    this.ks.set(m, k);
    return k;
  }

  bus(track) {
    let b = this.buses.get(track.id);
    if (!b) {
      const c = this.ctx;
      b = { in: c.createGain(), lofi: c.createBiquadFilter(), pan: c.createStereoPanner(), vol: c.createGain(), send: c.createGain() };
      b.lofi.type = 'lowpass';
      b.lofi.frequency.value = 4200;
      b.lofi.connect(b.in);
      b.in.connect(b.pan);
      b.pan.connect(b.vol);
      b.vol.connect(this.master);
      b.vol.connect(b.send);
      b.send.connect(this.reverbIn);
      this.buses.set(track.id, b);
    }
    return b;
  }
  updateBus(track, audible, immediate) {
    const b = this.bus(track), t = this.ctx.currentTime;
    const vals = [[b.vol.gain, audible ? track.vol * track.vol : 0], [b.pan.pan, track.pan || 0], [b.send.gain, track.reverb || 0]];
    for (const [p, v] of vals) {
      if (immediate) p.value = v;
      else p.setTargetAtTime(v, t, 0.02);
    }
  }
  removeBus(id) {
    const b = this.buses.get(id);
    if (b) { b.vol.disconnect(); this.buses.delete(id); }
  }

  note(track, midi, t, dur, vel) {
    const v = new Voice(this, this.bus(track).in, t);
    (VOICES[track.instrument] || VOICES.piano)(v, midi, vel);
    if (dur != null) v.release(t + dur);
    return v;
  }
  drum(track, id, t, vel) {
    const kit = track.kit || 'acoustic';
    const b = this.bus(track);
    const v = new Voice(this, kit === 'lofi' ? b.lofi : b.in, t);
    v.out.gain.value = kit === 'lofi' ? 1.15 : 1;
    (DRUM_VOICES[id] || DRUM_VOICES.kick)(v, vel, kit);
    return v;
  }
  click(t, accent) {
    const v = new Voice(this, this.master, t);
    const o = v.osc('sine', accent ? 1600 : 1000);
    o.connect(v.out);
    v.out.gain.setValueAtTime(accent ? 0.35 : 0.22, t);
    v.out.gain.setTargetAtTime(0, t, 0.015);
    v.finish(t + 0.1);
  }
  // raw = {sr, ch: [Float32Array...]}
  buffer(id, raw) {
    let b = this.bufs.get(id);
    if (!b && raw) {
      b = this.ctx.createBuffer(raw.ch.length, raw.ch[0].length || 1, raw.sr);
      raw.ch.forEach((d, i) => b.copyToChannel(d, i));
      this.bufs.set(id, b);
    }
    return b;
  }
  clip(track, buf, t, offset, dur) {
    if (dur <= 0.01) return null;
    const v = new Voice(this, this.bus(track).in, t);
    const s = this.ctx.createBufferSource();
    s.buffer = buf;
    s.connect(v.out);
    s.start(t, offset, dur);
    v.srcs.push(s);
    v.out.gain.setValueAtTime(1, t);
    v.out.gain.setValueAtTime(1, t + dur - 0.01);
    v.out.gain.linearRampToValueAtTime(0, t + dur);
    v.finish(t + dur + 0.02);
    return v;
  }
  killAll(now) {
    for (const v of [...this.active]) v.kill(now);
    this.openHat = null;
  }
}

function encodeWav(buffer) {
  const ch = buffer.numberOfChannels, sr = buffer.sampleRate, len = buffer.length;
  const bytes = 44 + len * ch * 2;
  const dv = new DataView(new ArrayBuffer(bytes));
  const str = (o, s) => { for (let i = 0; i < s.length; i++) dv.setUint8(o + i, s.charCodeAt(i)); };
  str(0, 'RIFF'); dv.setUint32(4, bytes - 8, true); str(8, 'WAVE');
  str(12, 'fmt '); dv.setUint32(16, 16, true); dv.setUint16(20, 1, true); dv.setUint16(22, ch, true);
  dv.setUint32(24, sr, true); dv.setUint32(28, sr * ch * 2, true); dv.setUint16(32, ch * 2, true); dv.setUint16(34, 16, true);
  str(36, 'data'); dv.setUint32(40, len * ch * 2, true);
  const data = [];
  for (let c = 0; c < ch; c++) data.push(buffer.getChannelData(c));
  let o = 44;
  for (let i = 0; i < len; i++) {
    for (let c = 0; c < ch; c++) {
      const s = Math.max(-1, Math.min(1, data[c][i]));
      dv.setInt16(o, s < 0 ? s * 0x8000 : s * 0x7fff, true);
      o += 2;
    }
  }
  return new Uint8Array(dv.buffer);
}
