'use strict';
// Music theory helpers, chord strips, beat presets and "Ideas" generators.

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
const FLAT_NAMES = ['C', 'D♭', 'D', 'E♭', 'E', 'F', 'G♭', 'G', 'A♭', 'A', 'B♭', 'B'];
const FLAT_KEYS = { major: [5, 10, 3, 8, 1], minor: [2, 7, 0, 5, 10, 3] };
const SCALES = { major: [0, 2, 4, 5, 7, 9, 11], minor: [0, 2, 3, 5, 7, 8, 10] };
const pc = m => ((m % 12) + 12) % 12;
const isBlack = m => [1, 3, 6, 8, 10].includes(pc(m));
const noteName = m => NOTE_NAMES[pc(m)] + (Math.floor(m / 12) - 1);
const inScale = (m, key, scale) => SCALES[scale].includes(pc(m - key));

const QUAL = {
  maj: { iv: [0, 4, 7], suf: '' },
  min: { iv: [0, 3, 7], suf: 'm' },
  dim: { iv: [0, 3, 6], suf: 'dim' },
  dom7: { iv: [0, 4, 7, 10], suf: '7' },
};
// 8 chord strips per scale: [semitones above key, quality, roman]
const CHORD_SETS = {
  major: [[0, 'maj', 'I'], [2, 'min', 'ii'], [4, 'min', 'iii'], [5, 'maj', 'IV'], [7, 'maj', 'V'], [9, 'min', 'vi'], [10, 'maj', '♭VII'], [7, 'dom7', 'V7']],
  minor: [[0, 'min', 'i'], [2, 'dim', 'ii°'], [3, 'maj', 'III'], [5, 'min', 'iv'], [7, 'min', 'v'], [8, 'maj', 'VI'], [10, 'maj', 'VII'], [7, 'maj', 'V']],
};

function chordList(key, scale) {
  return CHORD_SETS[scale].map(([off, q, roman]) => {
    const root = pc(key + off);
    const flat = roman.includes('♭') || FLAT_KEYS[scale].includes(key) || (scale === 'minor' && [3, 10].includes(off));
    return { root, iv: QUAL[q].iv, name: (flat ? FLAT_NAMES : NOTE_NAMES)[root] + QUAL[q].suf, rootName: (flat ? FLAT_NAMES : NOTE_NAMES)[root], roman };
  });
}

// Voice a chord near the given octave. Returns {notes, bass}.
function voiceChord(ch, oct) {
  let r = 12 * (oct + 1) + ch.root;
  if (ch.root > 7) r -= 12;
  return { notes: ch.iv.map(i => r + i), bass: r - 12 };
}

const PROGRESSIONS = {
  major: [
    { name: 'Pop Anthem', desc: 'I – V – vi – IV', seq: [0, 4, 5, 3] },
    { name: 'Emotional', desc: 'vi – IV – I – V', seq: [5, 3, 0, 4] },
    { name: 'Doo-Wop', desc: 'I – vi – IV – V', seq: [0, 5, 3, 4] },
    { name: 'Rock Out', desc: 'I – ♭VII – IV – I', seq: [0, 6, 3, 0] },
    { name: 'Jazzy', desc: 'ii – V7 – I – I', seq: [1, 7, 0, 0] },
    { name: 'Uplifting', desc: 'IV – I – V – vi', seq: [3, 0, 4, 5] },
  ],
  minor: [
    { name: 'Epic', desc: 'i – VI – III – VII', seq: [0, 5, 2, 6] },
    { name: 'Dark Trap', desc: 'i – VI – iv – V', seq: [0, 5, 3, 7] },
    { name: 'Sad Piano', desc: 'i – iv – VI – V', seq: [0, 3, 5, 7] },
    { name: 'Andalusian', desc: 'i – VII – VI – V', seq: [0, 6, 5, 7] },
    { name: 'Chill', desc: 'i – v – VI – iv', seq: [0, 4, 5, 3] },
  ],
};

const IDEA_STYLES = [
  { id: 'held', name: 'Held chords', desc: 'One long chord per bar' },
  { id: 'pulse', name: 'Pulsing 8ths', desc: 'Driving pop rhythm' },
  { id: 'rhythm', name: 'Piano rhythm', desc: 'Syncopated hits' },
  { id: 'arp', name: 'Arpeggio', desc: 'Notes rolling up and down' },
  { id: 'bass', name: 'Bass line', desc: 'Root notes groove' },
  { id: 'melody', name: 'Random melody', desc: 'A starter melody in key' },
];

function generateIdea(prog, style, P, oct) {
  const chords = chordList(P.key, P.scale);
  const notes = [];
  const add = (s, n, l, v = 0.8) => notes.push({ s, n, l, v });
  for (let bar = 0; bar < P.bars; bar++) {
    const ch = chords[prog.seq[bar % prog.seq.length]];
    const { notes: tones, bass } = voiceChord(ch, oct);
    const b = bar * 16;
    if (style === 'held') {
      tones.forEach(n => add(b, n, 16, 0.7));
    } else if (style === 'pulse') {
      for (let i = 0; i < 16; i += 2) tones.forEach(n => add(b + i, n, 2, i % 4 === 0 ? 0.8 : 0.6));
    } else if (style === 'rhythm') {
      for (const [s, l] of [[0, 3], [3, 3], [6, 4], [10, 2], [12, 4]]) tones.forEach(n => add(b + s, n, l, s === 0 ? 0.85 : 0.65));
    } else if (style === 'arp') {
      const seq = [...tones, tones[0] + 12, ...tones.slice(1).reverse(), tones[0] + 12].slice(0, 8);
      for (let i = 0; i < 8; i++) add(b + i * 2, seq[i % seq.length], 2, 0.75);
    } else if (style === 'bass') {
      const root = 12 * (oct + 1) + ch.root - (ch.root > 7 ? 12 : 0);
      for (const [s, l, d] of [[0, 3, 0], [3, 1, 0], [6, 2, 0], [8, 3, 0], [11, 1, 7], [14, 2, 12]]) add(b + s, root + d, l, s === 0 ? 0.9 : 0.75);
    } else if (style === 'melody') {
      const scale = [];
      for (let m = 12 * (oct + 1) + P.key; m < 12 * (oct + 1) + P.key + 15; m++) if (inScale(m, P.key, P.scale)) scale.push(m);
      const chordPcs = ch.iv.map(i => pc(ch.root + i));
      let idx = scale.findIndex(m => chordPcs.includes(pc(m)));
      for (let s = 0; s < 16;) {
        const l = [2, 2, 4, 1, 3][Math.floor(Math.random() * 5)];
        if (Math.random() < 0.8) {
          idx = Math.max(0, Math.min(scale.length - 1, idx + [-2, -1, 1, 2, 0][Math.floor(Math.random() * 5)]));
          let n = scale[idx];
          if (s === 0) n = scale.find(m => chordPcs.includes(pc(m))) || n;
          add(b + s, n, Math.min(l, 16 - s), 0.8);
        }
        s += l;
      }
    }
  }
  return notes;
}

// Beat presets: rows in DRUMS order (kick snare clap hat ohat tomlo tomhi crash)
const BEATS = [
  { name: 'Pop', kit: 'acoustic', rows: { kick: 'x.......x.x.....', snare: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.' } },
  { name: 'Rock', kit: 'acoustic', rows: { kick: 'x.....x.x.x.....', snare: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.', crash: 'x...............' } },
  { name: 'Hip-Hop', kit: 'lofi', swing: 0.35, rows: { kick: 'x.....x...x..x..', snare: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.', ohat: '..............x.' } },
  { name: 'Trap', kit: '808', rows: { kick: 'x......x..x.....', clap: '........x.......', hat: 'x.x.x.x.xxx.x.xx', ohat: '......x.........' } },
  { name: 'House', kit: 'electro', rows: { kick: 'x...x...x...x...', clap: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.', ohat: '..x...x...x...x.' } },
  { name: 'Funk', kit: 'acoustic', rows: { kick: 'x..x..x...x..x..', snare: '....x..x.x..x...', hat: 'xxxxxxxxxxxxxxxx' } },
  { name: 'Reggaeton', kit: 'electro', rows: { kick: 'x...x...x...x...', snare: '...x..x....x..x.', hat: 'x.x.x.x.x.x.x.x.' } },
  { name: 'Disco', kit: 'electro', rows: { kick: 'x...x...x...x...', snare: '....x.......x...', ohat: '..x...x...x...x.', hat: 'x...x...x...x...' } },
  { name: 'Lo-Fi Chill', kit: 'lofi', swing: 0.4, rows: { kick: 'x.......x.x.....', snare: '....x.......x...', hat: 'x.xxx.x.x.xxx.x.' } },
  { name: 'Rock Fill', kit: 'acoustic', rows: { kick: 'x.......x.......', snare: '....x...xxxx....', tomhi: '............xx..', tomlo: '..............xx' } },
];

function beatNotes(beat, bars) {
  const notes = [];
  for (let bar = 0; bar < bars; bar++) {
    DRUMS.forEach((d, row) => {
      const pat = beat.rows[d.id];
      if (!pat) return;
      for (let i = 0; i < 16; i++) {
        if (pat[i] === 'x') notes.push({ s: bar * 16 + i, n: row, l: 1, v: (i % 4 === 0 ? 0.95 : 0.8) });
      }
    });
  }
  return notes;
}
