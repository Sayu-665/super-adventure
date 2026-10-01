import { world, system, ItemStack, MolangVariableMap, EntityDamageCause } from "@minecraft/server";
import { NAMES } from "./data.js";

export const NS = "xian";

// ---------------------------------------------------------------- vectors
export const V = {
  add: (a, b) => ({ x: a.x + b.x, y: a.y + b.y, z: a.z + b.z }),
  sub: (a, b) => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }),
  mul: (a, s) => ({ x: a.x * s, y: a.y * s, z: a.z * s }),
  len: (a) => Math.hypot(a.x, a.y, a.z),
  dist: (a, b) => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z),
  norm: (a) => {
    const l = Math.hypot(a.x, a.y, a.z) || 1;
    return { x: a.x / l, y: a.y / l, z: a.z / l };
  },
  flat: (a) => {
    const l = Math.hypot(a.x, a.z) || 1;
    return { x: a.x / l, y: 0, z: a.z / l };
  },
  dot: (a, b) => a.x * b.x + a.y * b.y + a.z * b.z,
  floor: (a) => ({ x: Math.floor(a.x), y: Math.floor(a.y), z: Math.floor(a.z) }),
  center: (a) => ({ x: Math.floor(a.x) + 0.5, y: Math.floor(a.y), z: Math.floor(a.z) + 0.5 }),
};

// ---------------------------------------------------------------- random
export const rand = (a, b) => a + Math.random() * (b - a);
export const randInt = (a, b) => Math.floor(rand(a, b + 1));
export const chance = (p) => Math.random() < p;
export const pick = (arr) => arr[Math.floor(Math.random() * arr.length)];
export const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

// ---------------------------------------------------------------- formatting
export function fmt(n) {
  n = Math.floor(n);
  if (n >= 1e9) return (n / 1e9).toFixed(1) + "B";
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e4) return (n / 1e3).toFixed(1) + "K";
  return String(n);
}

export function bar(frac, len = 20, on = "§a", off = "§8") {
  const n = clamp(Math.round(frac * len), 0, len);
  return on + "|".repeat(n) + off + "|".repeat(len - n);
}

export const itemName = (id) => NAMES[id.replace(NS + ":", "")] ?? id.replace(/^.*:/, "").replace(/_/g, " ");

// ---------------------------------------------------------------- messaging
const actionbarLock = new Map();

export function say(player, text) {
  try {
    player.sendMessage(text);
  } catch {}
}

export function actionbar(player, text, holdTicks = 40) {
  try {
    player.onScreenDisplay.setActionBar(text);
    actionbarLock.set(player.id, system.currentTick + holdTicks);
  } catch {}
}

export function actionbarFree(player) {
  return (actionbarLock.get(player.id) ?? 0) <= system.currentTick;
}

export function title(player, main, sub = "", fade = { fadeInDuration: 5, stayDuration: 50, fadeOutDuration: 15 }) {
  try {
    player.onScreenDisplay.setTitle(main, { ...fade, subtitle: sub });
  } catch {}
}

export function broadcast(text) {
  try {
    world.sendMessage(text);
  } catch {}
}

// ---------------------------------------------------------------- effects / fx
export function particle(dim, id, loc, rgb) {
  try {
    if (rgb) {
      const m = new MolangVariableMap();
      m.setColorRGB("variable.color", { red: rgb[0], green: rgb[1], blue: rgb[2] });
      dim.spawnParticle(id, loc, m);
    } else {
      dim.spawnParticle(id, loc);
    }
  } catch {}
}

export function ring(dim, center, radius, id, rgb, points = 16, y = 0.1) {
  for (let i = 0; i < points; i++) {
    const a = (i / points) * Math.PI * 2;
    particle(dim, id, { x: center.x + Math.cos(a) * radius, y: center.y + y, z: center.z + Math.sin(a) * radius }, rgb);
  }
}

export function sound(target, id, opts = {}) {
  try {
    target.dimension.playSound(id, target.location, opts);
  } catch {}
}

export function soundAt(dim, loc, id, opts = {}) {
  try {
    dim.playSound(id, loc, opts);
  } catch {}
}

export function effect(entity, id, seconds, amp = 0, particles = true) {
  try {
    entity.addEffect(id, Math.round(seconds * 20), { amplifier: amp, showParticles: particles });
  } catch {}
}

export function heal(entity, amount) {
  const h = entity.getComponent("minecraft:health");
  if (!h) return 0;
  const before = h.currentValue;
  h.setCurrentValue(Math.min(h.effectiveMax, before + amount));
  return h.currentValue - before;
}

export function hurt(target, amount, source, cause = EntityDamageCause.entityAttack) {
  try {
    if (!target?.isValid) return false;
    const opts = { cause };
    if (source?.isValid) opts.damagingEntity = source;
    return target.applyDamage(Math.max(0, amount), opts);
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------- targeting
const IGNORED_TYPES = [
  "minecraft:item",
  "minecraft:xp_orb",
  "minecraft:arrow",
  "minecraft:armor_stand",
  "minecraft:painting",
  "minecraft:lightning_bolt",
  "minecraft:fireball",
  "minecraft:small_fireball",
  "minecraft:snowball",
  "minecraft:egg",
  "minecraft:ender_pearl",
  "minecraft:fishing_hook",
  "minecraft:tnt",
  "minecraft:falling_block",
  "minecraft:minecart",
  "minecraft:boat",
  "minecraft:chest_boat",
  "minecraft:leash_knot",
  "minecraft:area_effect_cloud",
];
const FRIENDLY_FAMILIES = ["villager", "wandering_trader", "merchant", "inanimate", "npc"];

export function isPvp() {
  return world.getDynamicProperty("xian:pvp") === true;
}

/** Can `caster` hurt `e` with a technique?  aoe=true only hits clear enemies. */
export function validTarget(caster, e, aoe = false) {
  if (!e?.isValid || e.id === caster.id) return false;
  if (IGNORED_TYPES.includes(e.typeId)) return false;
  if (!e.getComponent("minecraft:health")) return false;
  const casterIsPlayer = caster.typeId === "minecraft:player";
  if (e.typeId === "minecraft:player") {
    if (!casterIsPlayer) return true;
    return isPvp();
  }
  if (!casterIsPlayer) {
    // mobs casting techniques only hurt players and tamed pets
    return !!e.getComponent("minecraft:is_tamed");
  }
  const tame = e.getComponent("minecraft:tameable");
  if (tame?.tamedToPlayerId) return false;
  if (e.getComponent("minecraft:is_tamed")) return false;
  if (aoe) {
    if (e.matches({ families: ["monster"] })) return true;
    return recentAttackers.get(caster.id)?.has(e.id) ?? false;
  }
  for (const f of FRIENDLY_FAMILIES) if (e.matches({ families: [f] })) return false;
  return true;
}

/** player id -> Set of entity ids that hurt the player recently (so AOE hits them). */
export const recentAttackers = new Map();

export function enemiesNear(caster, loc, radius, aoe = true) {
  try {
    return caster.dimension
      .getEntities({ location: loc, maxDistance: radius, excludeTypes: IGNORED_TYPES })
      .filter((e) => validTarget(caster, e, aoe));
  } catch {
    return [];
  }
}

export function lookTarget(caster, range = 24) {
  try {
    const hits = caster.dimension.getEntitiesFromRay(caster.getHeadLocation(), caster.getViewDirection(), {
      maxDistance: range,
      excludeTypes: IGNORED_TYPES,
    });
    for (const h of hits) if (validTarget(caster, h.entity, false)) return h.entity;
  } catch {}
  return undefined;
}

export function lookBlock(caster, range = 32) {
  try {
    const hit = caster.dimension.getBlockFromRay(caster.getHeadLocation(), caster.getViewDirection(), {
      maxDistance: range,
      includeLiquidBlocks: false,
      includePassableBlocks: false,
    });
    if (hit) return V.add(hit.block.location, { x: 0.5, y: 1, z: 0.5 });
  } catch {}
  return undefined;
}

/** Location the caster is aiming at (entity, block, or max range point). */
export function aimPoint(caster, range = 24) {
  const t = lookTarget(caster, range);
  if (t) return t.location;
  const b = lookBlock(caster, range);
  if (b) return b;
  return V.add(caster.getHeadLocation(), V.mul(caster.getViewDirection(), range));
}

const PASSABLE =
  /(air|grass|flower|fern|vine|torch|sapling|_plant|bush|snow_layer|carpet|button|lever|rail|sign|mushroom|tulip|dandelion|poppy|orchid|allium|bluet|daisy|cornflower|lily|rose|kelp|seagrass|wheat|carrots|potatoes|beetroot|reeds|sugar_cane|water|lava|light_block|structure_void|web)/;

export function blocksProjectile(dim, loc) {
  try {
    const b = dim.getBlock(loc);
    if (!b) return true;
    if (b.isAir || b.isLiquid) return false;
    return !PASSABLE.test(b.typeId);
  } catch {
    return true;
  }
}

// ---------------------------------------------------------------- inventory
export function inv(player) {
  return player.getComponent("minecraft:inventory")?.container;
}

export function countItem(player, id) {
  const c = inv(player);
  if (!c) return 0;
  let n = 0;
  for (let i = 0; i < c.size; i++) {
    const it = c.getItem(i);
    if (it?.typeId === id) n += it.amount;
  }
  return n;
}

export function removeItem(player, id, amount) {
  const c = inv(player);
  if (!c || countItem(player, id) < amount) return false;
  for (let i = 0; i < c.size && amount > 0; i++) {
    const it = c.getItem(i);
    if (it?.typeId !== id) continue;
    if (it.amount > amount) {
      it.amount -= amount;
      c.setItem(i, it);
      amount = 0;
    } else {
      amount -= it.amount;
      c.setItem(i, undefined);
    }
  }
  return true;
}

export function giveItem(player, id, amount = 1) {
  const c = inv(player);
  let left = amount;
  while (left > 0) {
    let stack;
    try {
      stack = new ItemStack(id, 1);
    } catch {
      return;
    }
    const n = Math.min(left, stack.maxAmount);
    stack.amount = n;
    left -= n;
    const rest = c?.addItem(stack);
    if (rest) {
      try {
        player.dimension.spawnItem(rest, player.location);
      } catch {}
    }
  }
}

/** Consume one of the item in the player's selected slot. */
export function consumeHeld(player, typeId) {
  const c = inv(player);
  if (!c) return false;
  const slot = player.selectedSlotIndex;
  const it = c.getItem(slot);
  if (!it || (typeId && it.typeId !== typeId)) return false;
  if (it.amount > 1) {
    it.amount -= 1;
    c.setItem(slot, it);
  } else c.setItem(slot, undefined);
  return true;
}

export function heldItem(player) {
  try {
    return inv(player)?.getItem(player.selectedSlotIndex);
  } catch {
    return undefined;
  }
}

// ---------------------------------------------------------------- spirit stone wallet
/** @type {[string, number][]} */
export const STONES = [
  ["xian:supreme_spirit_stone", 729],
  ["xian:high_spirit_stone", 81],
  ["xian:mid_spirit_stone", 9],
  ["xian:low_spirit_stone", 1],
];

export function walletValue(player) {
  return STONES.reduce((s, [id, v]) => s + countItem(player, id) * v, 0);
}

/** Pay `cost` (in low-grade stones), breaking larger stones and returning change. */
export function pay(player, cost) {
  if (walletValue(player) < cost) return false;
  let remaining = cost;
  // spend from smallest to largest so big stones are kept when possible
  for (const [id, v] of [...STONES].reverse()) {
    if (remaining <= 0) break;
    const have = countItem(player, id);
    const use = Math.min(have, Math.floor(remaining / v));
    if (use > 0) {
      removeItem(player, id, use);
      remaining -= use * v;
    }
  }
  // break one larger stone for the rest and give change
  if (remaining > 0) {
    for (const [id, v] of [...STONES].reverse()) {
      if (v >= remaining && countItem(player, id) > 0) {
        removeItem(player, id, 1);
        giveChange(player, v - remaining);
        remaining = 0;
        break;
      }
    }
  }
  return remaining <= 0;
}

export function giveChange(player, value) {
  for (const [id, v] of STONES) {
    const n = Math.floor(value / v);
    if (n > 0) giveItem(player, id, n);
    value -= n * v;
  }
}

// ---------------------------------------------------------------- misc
export function dimById(id) {
  try {
    return world.getDimension(id);
  } catch {
    return undefined;
  }
}

export function playerById(id) {
  return world.getAllPlayers().find((p) => p.id === id);
}

export function compass(from, to) {
  const dx = to.x - from.x;
  const dz = to.z - from.z;
  const a = (Math.atan2(dx, -dz) * 180) / Math.PI;
  const dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
  return dirs[Math.round(((a + 360) % 360) / 45) % 8];
}

/** Type families of an entity (falls back to its type id if the component is unavailable). */
export function familiesOf(e) {
  try {
    const fams = e.getComponent("minecraft:type_family")?.getTypeFamilies();
    if (fams?.length) return fams;
  } catch {}
  const t = e?.typeId ?? "";
  const guess = [];
  if (/villager|wandering_trader/.test(t)) guess.push("villager");
  if (/rogue_cultivator|demonic_cultivator|wandering_merchant/.test(t)) guess.push("cultivator");
  if (/demonic_cultivator/.test(t)) guess.push("demonic_cultivator", "monster");
  if (/rogue_cultivator/.test(t)) guess.push("rogue_cultivator", "monster");
  if (/wandering_merchant/.test(t)) guess.push("merchant");
  if (/xian:(spirit_fox|demonic_wolf|flame_tiger|flood_dragon)/.test(t)) guess.push("spirit_beast");
  if (/xian:(demonic_wolf|flame_tiger|flood_dragon|heart_demon)/.test(t)) guess.push("monster");
  if (/heart_demon/.test(t)) guess.push("heart_demon");
  if (t === "minecraft:player") guess.push("player");
  return guess;
}

export function maxHealth(e) {
  return e.getComponent("minecraft:health")?.effectiveMax ?? 20;
}
