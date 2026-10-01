// Persistent per-player cultivation data, stored in dynamic properties.
const P = "xian:";

const DEFAULTS = {
  init: false,
  realm: 0,
  stage: 0,
  exp: 0,
  qi: 0,
  roots: "", // comma separated element ids
  rootsKnown: false,
  manual: "",
  manuals: "[]",
  techs: "[]",
  techSel: "",
  bodyExp: 0,
  dao: 0,
  karma: 0,
  tox: 0,
  deviation: 0, // seconds of qi deviation remaining
  btBonus: 0, // pending breakthrough bonus from pills
  tribWard: false,
  alchExp: 0,
  refineExp: 0,
  sect: "",
  contrib: 0,
  mission: "", // JSON {id, progress}
  missionCd: 0,
  lifespanBonus: 0,
  hud: 1, // 0 off, 1 contextual, 2 always
  buffSpeed: true,
  buffJump: true,
  nightVision: false,
  ring: "[]",
  kills: 0,
  medTime: 0,
  breakthroughs: 0,
};

/** @returns {any} */
export function get(player, key) {
  const v = player.getDynamicProperty(P + key);
  return v === undefined ? DEFAULTS[key] : v;
}

export function set(player, key, value) {
  player.setDynamicProperty(P + key, value);
}

/** @returns {any} */
export function add(player, key, delta) {
  const v = (get(player, key) ?? 0) + delta;
  set(player, key, v);
  return v;
}

export function getJSON(player, key, fallback) {
  try {
    const raw = get(player, key);
    return raw ? JSON.parse(raw) : fallback;
  } catch {
    return fallback;
  }
}

export function setJSON(player, key, value) {
  set(player, key, value === undefined || value === null ? "" : JSON.stringify(value));
}

export function roots(player) {
  const r = get(player, "roots");
  return r ? r.split(",") : [];
}

export function reset(player) {
  for (const k of Object.keys(DEFAULTS)) player.setDynamicProperty(P + k, undefined);
}
