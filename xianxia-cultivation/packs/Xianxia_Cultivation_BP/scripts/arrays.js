// Registry of placed formation arrays (qi gathering / protection / teleportation).
import { world } from "@minecraft/server";

const KEY = "xian:arrays";
let cache;

function load() {
  if (cache) return cache;
  try {
    cache = JSON.parse(String(world.getDynamicProperty(KEY) ?? "[]"));
  } catch {
    cache = [];
  }
  return cache;
}

function save() {
  world.setDynamicProperty(KEY, JSON.stringify(cache));
}

const same = (a, dim, loc) => a.d === dim && a.x === loc.x && a.y === loc.y && a.z === loc.z;

export function addArray(type, dim, loc, name = "") {
  const list = load();
  if (list.some((a) => same(a, dim, loc))) return;
  list.push({ t: type, d: dim, x: loc.x, y: loc.y, z: loc.z, n: name });
  save();
}

export function removeArray(dim, loc) {
  const list = load();
  const i = list.findIndex((a) => same(a, dim, loc));
  if (i >= 0) {
    list.splice(i, 1);
    save();
  }
}

export function renameArray(dim, loc, name) {
  const a = load().find((x) => same(x, dim, loc));
  if (a) {
    a.n = name;
    save();
  }
}

export function findArray(dim, loc) {
  return load().find((a) => same(a, dim, loc));
}

export function arrays(type) {
  return load().filter((a) => !type || a.t === type);
}

export function arraysNear(type, dim, loc, radius) {
  return arrays(type).filter(
    (a) => a.d === dim && Math.hypot(a.x + 0.5 - loc.x, a.y - loc.y, a.z + 0.5 - loc.z) <= radius,
  );
}
