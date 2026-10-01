// Item behaviour: pills, spirit stones, manuals, scrolls, talismans, special items, weapons, sword flight.
import { world, system, EntityDamageCause, WeatherType } from "@minecraft/server";
import { ActionFormData } from "@minecraft/server-ui";
import {
  PILLS,
  MANUALS,
  TECHNIQUES,
  ELEMENTS,
  ROOT_GRADES,
  BREAKTHROUGH_PILLS,
  WEAPONS,
  GRADE_COLORS,
} from "./data.js";
import * as D from "./playerdata.js";
import { emit } from "./bus.js";
import {
  realmOf,
  rootGrade,
  rootText,
  gainExp,
  addBodyExp,
  maxQi,
  applyStats,
  meditationSeconds,
  rootMult,
  primaryColor,
  spendQi,
} from "./cultivation.js";
import { cast, castFree, learn } from "./techniques.js";
import { openCodex, chooseTechnique, openSect } from "./ui.js";
import {
  V,
  chance,
  fmt,
  say,
  actionbar,
  title,
  broadcast,
  particle,
  ring,
  sound,
  effect,
  heal,
  hurt,
  consumeHeld,
  heldItem,
  lookTarget,
  aimPoint,
  itemName,
  inv,
  giveItem,
  maxHealth,
} from "./util.js";

const PILL = Object.fromEntries(PILLS.map((p) => [`xian:${p.id}`, p]));
const STONE_VALUE = {
  "xian:low_spirit_stone": 1,
  "xian:mid_spirit_stone": 9,
  "xian:high_spirit_stone": 81,
  "xian:supreme_spirit_stone": 729,
};
const WEAPON = Object.fromEntries(WEAPONS.map((w) => [`xian:${w.id}`, w]));

// ------------------------------------------------------------------ item use (right click)
const USE = {
  "xian:cultivation_codex": (p) => openCodex(p),
  "xian:martial_seal": (p) => (p.isSneaking ? chooseTechnique(p) : cast(p, D.get(p, "techSel"))),
  "xian:sect_token": (p) => openSect(p),
  "xian:spatial_ring": (p) => openRing(p),
  "xian:flying_sword": (p) => toggleFlight(p),
  "xian:root_testing_stone": (p) => testRoots(p),
  "xian:dragon_summoning_pearl": (p) => summonDragon(p),
  "xian:beast_tide_horn": (p) => {
    if (emitTide(p)) consumeHeld(p, "xian:beast_tide_horn");
  },
  "xian:fire_talisman": (p) => talisman(p, () => castFree(p, "fireball", 10)),
  "xian:thunder_talisman": (p) =>
    talisman(p, () => {
      const loc = aimPoint(p, 30);
      try {
        p.dimension.spawnEntity("minecraft:lightning_bolt", loc);
      } catch {}
      return true;
    }),
  "xian:protection_talisman": (p) =>
    talisman(p, () => {
      effect(p, "absorption", 60, 2);
      effect(p, "resistance", 30, 1);
      ring(p.dimension, p.location, 1.2, "xian:rune", [1, 0.85, 0.3], 20, 0.2);
      return true;
    }),
  "xian:divine_travel_talisman": (p) =>
    talisman(p, () => {
      effect(p, "speed", 60, 2);
      effect(p, "jump_boost", 60, 1);
      return true;
    }),
  "xian:escape_talisman": (p) => talisman(p, () => escape(p)),
  "xian:concealment_talisman": (p) =>
    talisman(p, () => {
      effect(p, "invisibility", 60, 0, false);
      particle(p.dimension, "minecraft:large_explosion", p.location);
      return true;
    }),
  "xian:sealing_talisman": (p) =>
    talisman(p, () => {
      const t = lookTarget(p, 20);
      if (!t) return (actionbar(p, "§7No target in sight."), false);
      effect(t, "slowness", 6, 10);
      effect(t, "weakness", 6, 5);
      effect(t, "mining_fatigue", 6, 3);
      ring(t.dimension, t.location, 1, "xian:rune", [0.8, 0.1, 0.3], 16, 0.1);
      ring(t.dimension, t.location, 1, "xian:rune", [0.8, 0.1, 0.3], 16, 1.6);
      return true;
    }),
  "xian:lightning_rod_talisman": (p) =>
    say(
      p,
      "§5Keep this talisman in your inventory. It is consumed automatically to halve heavenly tribulation damage.",
    ),
};

for (const id of Object.keys(STONE_VALUE)) USE[id] = (p, item) => absorbStone(p, item);
for (const m of MANUALS) USE[`xian:manual_${m.id}`] = (p) => readManual(p, m);
for (const t of TECHNIQUES) USE[`xian:scroll_${t.id}`] = (p) => readScroll(p, t);

function emitTide(p) {
  let started = false;
  emit("beast_tide", p, (ok) => (started = ok));
  return started;
}

function talisman(p, fn) {
  if (fn() === false) return;
  consumeHeld(p);
  sound(p, "fire.ignite", { pitch: 1.6 });
  particle(p.dimension, "xian:flame", V.add(p.location, { x: 0, y: 1.2, z: 0 }));
}

function escape(p) {
  const sp = p.getSpawnPoint();
  try {
    if (sp) {
      p.teleport({ x: sp.x + 0.5, y: sp.y + 0.2, z: sp.z + 0.5 }, { dimension: sp.dimension });
    } else {
      const ws = world.getDefaultSpawnLocation();
      const ow = world.getDimension("overworld");
      const y = ws.y > 320 ? 300 : ws.y + 1;
      p.teleport({ x: ws.x + 0.5, y, z: ws.z + 0.5 }, { dimension: ow });
      if (ws.y > 320) effect(p, "slow_falling", 30, 0);
    }
  } catch {
    return false;
  }
  effect(p, "resistance", 5, 2);
  say(p, "§b[Talisman] §7Space folds around you - you flee a thousand miles in an instant.");
  return true;
}

function testRoots(p) {
  const known = D.get(p, "rootsKnown");
  if (!known) {
    D.set(p, "rootsKnown", true);
    consumeHeld(p, "xian:root_testing_stone");
  }
  const g = rootGrade(D.roots(p));
  const G = ROOT_GRADES[g];
  title(p, `${G.color}${G.name}`, `§7Cultivation speed x${G.mult}`);
  say(p, `§e[Spirit Root] §7The stone glows... ${rootText(p)}`);
  const rgbs = D.roots(p).map((e) => ELEMENTS[e].rgb);
  rgbs.forEach((rgb, k) =>
    system.runTimeout(() => ring(p.dimension, p.location, 1 + k * 0.5, "xian:orb", rgb, 18, 1), k * 5),
  );
  sound(p, g === "heavenly" || g === "mutated" ? "random.totem" : "random.orb");
  if (!known && (g === "heavenly" || g === "mutated")) {
    broadcast(`§6[Heavens] §7A ${G.color}${G.name}§7 has appeared in the world: §f${p.name}§7!`);
  }
}

function absorbStone(p, item) {
  const value = STONE_VALUE[item.typeId];
  const n = p.isSneaking ? item.amount : 1;
  const c = inv(p);
  const slot = p.selectedSlotIndex;
  const it = c?.getItem(slot);
  if (!it || it.typeId !== item.typeId) return;
  if (it.amount > n) {
    it.amount -= n;
    c.setItem(slot, it);
  } else c.setItem(slot, undefined);
  const exp = value * n * 25 * rootMult(p);
  gainExp(p, exp);
  D.set(p, "qi", Math.min(maxQi(p), D.get(p, "qi") + value * n * 5));
  ring(p.dimension, p.location, 0.8, "xian:orb", [0.5, 0.85, 1], 10, 1);
  sound(p, "random.orb", { pitch: 1.4 });
  actionbar(p, `§bAbsorbed ${n}x ${itemName(item.typeId)}: §a+${fmt(exp)} cultivation`);
}

function readManual(p, m) {
  const list = D.getJSON(p, "manuals", []);
  if (!list.includes(m.id)) list.push(m.id);
  D.setJSON(p, "manuals", list);
  D.set(p, "manual", m.id);
  consumeHeld(p, `xian:manual_${m.id}`);
  title(p, `${GRADE_COLORS[m.grade]}${m.name}`, "§7Comprehended! Now your active cultivation manual.");
  say(
    p,
    `§e[Manual] §7The jade slip dissolves into your sea of consciousness: ${GRADE_COLORS[m.grade]}${m.name}§7 (${m.grade} grade, x${m.mult}). §8${m.desc}`,
  );
  if (m.id === "heavenly_devouring") {
    D.add(p, "karma", -10);
    say(p, "§4[Karma] §7You have stepped onto the demonic path.");
  }
  sound(p, "random.levelup", { pitch: 1.4 });
  applyStats(p);
}

function readScroll(p, t) {
  if (!learn(p, t.id)) return say(p, `§7You already know §f${t.name}§7.`);
  consumeHeld(p, `xian:scroll_${t.id}`);
  D.set(p, "techSel", t.id);
  const el = ELEMENTS[t.element];
  title(p, `${el.color}${t.name}`, "§7Technique learned! Use the Martial Jade Seal to cast it.");
  say(
    p,
    `§b[Technique] §7Learned ${el.color}${t.name}§7 (${el.name}, ${t.cost} qi, needs ${realmName(t.realm)}). §8${t.desc}`,
  );
  sound(p, "random.levelup", { pitch: 1.2 });
}

function realmName(i) {
  return [
    "Mortal",
    "Qi Condensation",
    "Foundation Establishment",
    "Core Formation",
    "Nascent Soul",
    "Soul Transformation",
    "Void Refinement",
    "Body Integration",
    "Mahayana",
    "Tribulation Transcendence",
    "True Immortal",
  ][i];
}

function summonDragon(p) {
  const d = V.flat(p.getViewDirection());
  const loc = V.add(p.location, { x: d.x * 10, y: 1, z: d.z * 10 });
  try {
    p.dimension.spawnEntity("xian:flood_dragon", loc);
    if (p.dimension.id === "minecraft:overworld") p.dimension.setWeather(WeatherType.Thunder, 3600);
  } catch {
    return say(p, "§7The pearl stays silent here.");
  }
  consumeHeld(p, "xian:dragon_summoning_pearl");
  title(p, "§3Azure Flood Dragon", "§7The waters roar!");
  broadcast(`§3[Heavens] §7A dragon's roar shakes the sky near §f${p.name}§7!`);
  sound(p, "mob.enderdragon.growl", { volume: 2 });
}

// ------------------------------------------------------------------ spatial ring
const RING_SLOTS = 27;

export function openRing(p) {
  const store = D.getJSON(p, "ring", []);
  const f = new ActionFormData()
    .title("Spatial Storage Ring")
    .body(
      `§7A pocket dimension bound to your soul. §8(${store.length}/${RING_SLOTS} kinds stored; stackable items only)`,
    );
  f.button("§aStore held stack");
  f.button("§aStore all herbs & materials");
  for (const s of store) f.button(`${itemName(s.id)}\n§8x${s.n} - tap to take out`);
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    if (r.selection === 0) return ringStoreHeld(p);
    if (r.selection === 1) return ringStoreAll(p);
    const s = store[r.selection - 2];
    if (!s) return;
    giveItem(p, s.id, s.n);
    store.splice(r.selection - 2, 1);
    D.setJSON(p, "ring", store);
    sound(p, "random.pop");
  });
}

function ringAdd(store, id, n) {
  const e = store.find((s) => s.id === id);
  if (e) e.n += n;
  else if (store.length < RING_SLOTS) store.push({ id, n });
  else return false;
  return true;
}

function ringStoreHeld(p) {
  const c = inv(p);
  const it = c?.getItem(p.selectedSlotIndex);
  if (!it) return say(p, "§7Hold the stack you want to store.");
  if (it.maxAmount <= 1 || it.typeId === "xian:spatial_ring")
    return say(p, "§7Only stackable items can enter the ring.");
  const store = D.getJSON(p, "ring", []);
  if (!ringAdd(store, it.typeId, it.amount)) return say(p, "§7The ring is full.");
  c.setItem(p.selectedSlotIndex, undefined);
  D.setJSON(p, "ring", store);
  sound(p, "random.pop", { pitch: 0.7 });
}

function ringStoreAll(p) {
  const c = inv(p);
  if (!c) return;
  const store = D.getJSON(p, "ring", []);
  let moved = 0;
  for (let i = 9; i < c.size; i++) {
    const it = c.getItem(i);
    if (!it || !it.typeId.startsWith("xian:") || it.maxAmount <= 1) continue;
    if (/(pill|talisman|spirit_stone|scroll_|manual_)/.test(it.typeId)) continue;
    if (ringAdd(store, it.typeId, it.amount)) {
      c.setItem(i, undefined);
      moved += it.amount;
    }
  }
  D.setJSON(p, "ring", store);
  say(p, `§b[Ring] §7Stored ${moved} items from your backpack.`);
}

// ------------------------------------------------------------------ pills (on finished eating)
function onConsume(p, typeId) {
  if (typeId === "xian:spirit_fruit") {
    heal(p, 4);
    D.set(p, "qi", Math.min(maxQi(p), D.get(p, "qi") + maxQi(p) * 0.2));
    return;
  }
  if (typeId === "xian:immortal_peach") {
    gainExp(p, meditationSeconds(p, 600));
    D.add(p, "lifespanBonus", 500);
    heal(p, 1000);
    effect(p, "absorption", 120, 4);
    title(p, "§dImmortal Peach", "§7+500 years of lifespan");
    return;
  }
  const pill = PILL[typeId];
  if (!pill) return;
  const tox = D.get(p, "tox");
  const eff = tox > 100 ? 0.5 : 1;
  if (pill.toxicity) {
    const nt = D.add(p, "tox", pill.toxicity);
    if (nt > 150) {
      effect(p, "poison", 5, 0);
      effect(p, "nausea", 6, 0);
      actionbar(p, "§2Pill toxicity is building up in your body! (Detox Pill)");
    }
  }
  const realm = realmOf(p);
  switch (pill.id) {
    case "qi_gathering_pill":
    case "spirit_condensing_pill": {
      const secs = pill.id === "qi_gathering_pill" ? 40 : 120;
      const exp = meditationSeconds(p, secs) * eff;
      gainExp(p, exp);
      actionbar(p, `§a+${fmt(exp)} cultivation${eff < 1 ? " §2(toxicity halves effect)" : ""}`);
      break;
    }
    case "healing_pill":
      heal(p, 10 + realm.i * 4);
      effect(p, "regeneration", 10, 1);
      break;
    case "bigu_pill":
      effect(p, "saturation", 1, 4, false);
      say(p, "§7Your stomach is full for days. A cultivator has no need of mortal food.");
      break;
    case "qi_recovery_pill":
      D.set(p, "qi", Math.min(maxQi(p), D.get(p, "qi") + maxQi(p) * 0.5 * eff));
      break;
    case "detox_pill":
      D.set(p, "tox", 0);
      for (const e of ["poison", "wither", "nausea", "hunger", "fatal_poison"]) {
        try {
          p.removeEffect(e);
        } catch {}
      }
      actionbar(p, "§aYour meridians are purified.");
      break;
    case "body_tempering_pill":
      addBodyExp(p, Math.floor(200 * (1 + realm.i * 0.5) * eff));
      effect(p, "nausea", 3, 0);
      break;
    case "berserk_blood_pill":
      effect(p, "strength", 30, 2);
      effect(p, "speed", 30, 1);
      system.runTimeout(() => {
        if (p.isValid) effect(p, "weakness", 20, 1);
      }, 600);
      break;
    case "clear_heart_pill":
      D.set(p, "deviation", 0);
      D.add(p, "karma", 2);
      for (const e of ["nausea", "weakness", "darkness"]) {
        try {
          p.removeEffect(e);
        } catch {}
      }
      actionbar(p, "§fYour dao heart is clear and still.");
      break;
    case "enlightenment_pill":
      D.add(p, "dao", 5 * eff);
      title(p, "§eEnlightenment", "§7+Dao Comprehension");
      break;
    case "bone_marrow_pill":
      cleanseMarrow(p);
      break;
    case "longevity_pill":
      D.add(p, "lifespanBonus", 100);
      heal(p, 1000);
      gainExp(p, meditationSeconds(p, 300) * eff);
      title(p, "§dLongevity Pill", "§7+100 years of lifespan");
      break;
    case "foundation_pill":
    case "golden_core_pill":
    case "nascent_soul_pill":
    case "void_tribulation_pill": {
      const [best, bonus] = BREAKTHROUGH_PILLS[pill.id];
      const nextI = realm.i + 1;
      const val = Math.round((best === -1 || best === nextI ? bonus : bonus / 2) * eff);
      D.set(p, "btBonus", Math.max(D.get(p, "btBonus"), val));
      if (pill.id === "void_tribulation_pill") D.set(p, "tribWard", true);
      title(p, "§eBreakthrough Pill", `§7+${val}% chance on your next breakthrough`);
      break;
    }
  }
  ring(
    p.dimension,
    p.location,
    0.7,
    "xian:orb",
    pill.color.map((c) => c / 255),
    10,
    1.2,
  );
}

function cleanseMarrow(p) {
  const roots = D.roots(p);
  D.set(p, "rootsKnown", true);
  if (roots.length > 1) {
    roots.splice(Math.floor(Math.random() * roots.length), 1);
    D.set(p, "roots", roots.join(","));
    const G = ROOT_GRADES[rootGrade(roots)];
    title(p, "§fMarrow Cleansed", `${G.color}${G.name}`);
    say(p, `§f[Bone Marrow] §7Impurities are expelled from your body. Your root is now ${rootText(p)}`);
    effect(p, "nausea", 6, 0);
    hurt(p, 4, undefined, EntityDamageCause.magic);
  } else if (!ELEMENTS[roots[0]]?.mutated && chance(0.25)) {
    const mut =
      { water: "ice", wood: "wind", metal: "lightning", fire: "lightning", earth: "ice" }[roots[0]] ?? "lightning";
    D.set(p, "roots", mut);
    title(p, "§dRoot Mutation!", `§7Your root mutated into ${ELEMENTS[mut].color}${ELEMENTS[mut].name}`);
    broadcast(
      `§6[Heavens] §f${p.name}§7's spirit root has mutated into ${ELEMENTS[mut].color}${ELEMENTS[mut].name}§7!`,
    );
  } else {
    D.add(p, "dao", 2);
    say(p, "§7Your root is already pure. The pill's essence deepens your comprehension instead (+2 Dao).");
  }
  applyStats(p);
}

// ------------------------------------------------------------------ sword flight
export const flying = new Map(); // id -> since tick

function toggleFlight(p) {
  if (flying.has(p.id)) return stopFlight(p, "§7You step off your flying sword.");
  if (realmOf(p).i < 2)
    return say(p, "§7Only cultivators at §aFoundation Establishment§7 or above can ride a flying sword.");
  if (D.get(p, "qi") < 10) return actionbar(p, "§cNot enough qi to control the sword.");
  flying.set(p.id, system.currentTick);
  say(p, "§b[Sword Flight] §7Look where you want to fly. §fSneak§7 to hover/descend, use the sword again to land.");
  sound(p, "item.trident.riptide_1", { pitch: 1.4 });
}

export function stopFlight(p, msg) {
  if (!flying.delete(p.id)) return;
  effect(p, "slow_falling", 6, 0, false);
  if (msg) say(p, msg);
}

export function flightTick() {
  for (const [id, since] of flying) {
    const p = world.getAllPlayers().find((x) => x.id === id);
    if (!p) {
      flying.delete(id);
      continue;
    }
    if (heldItem(p)?.typeId !== "xian:flying_sword") {
      stopFlight(p, "§7Your flying sword returns to your hand.");
      continue;
    }
    const { i } = realmOf(p);
    if ((system.currentTick - since) % 20 === 0) {
      if (!spendQi(p, 1 + i)) {
        stopFlight(p, "§cYour qi is exhausted - the sword wavers and you drop!");
        continue;
      }
      effect(p, "slow_falling", 2, 0, false);
    }
    const sp = Math.min(1.5, 0.55 + i * 0.07);
    const d = p.getViewDirection();
    try {
      if (p.isSneaking) p.applyKnockback({ x: d.x * 0.1, z: d.z * 0.1 }, -0.05);
      else p.applyKnockback({ x: d.x * sp, z: d.z * sp }, d.y * sp + 0.05);
    } catch {}
    if (system.currentTick % 2 === 0) {
      const l = p.location;
      particle(p.dimension, "xian:sword", { x: l.x, y: l.y - 0.1, z: l.z }, primaryColor(p));
      particle(p.dimension, "xian:aura", { x: l.x - d.x, y: l.y, z: l.z - d.z }, primaryColor(p));
    }
  }
}

// ------------------------------------------------------------------ weapon effects
function onWeaponHit(p, target) {
  const w = WEAPON[heldItem(p)?.typeId ?? ""];
  if (!w?.effect || !target?.isValid) return;
  switch (w.effect) {
    case "frost":
      effect(target, "slowness", 3, 2);
      particle(target.dimension, "xian:frost", V.add(target.location, { x: 0, y: 1, z: 0 }));
      break;
    case "flame":
      target.setOnFire(4, true);
      break;
    case "thunder":
      if (chance(0.2)) {
        system.runTimeout(() => {
          if (!target.isValid) return;
          for (let y = 0; y < 8; y++)
            particle(target.dimension, "xian:lightning", V.add(target.location, { x: 0, y, z: 0 }));
          hurt(target, 8, p, EntityDamageCause.lightning);
          sound(target, "ambient.weather.lightning.impact", { volume: 0.6 });
        }, 11);
      }
      break;
    case "lifesteal":
      heal(p, 2);
      particle(p.dimension, "xian:orb", V.add(p.location, { x: 0, y: 1, z: 0 }), [0.8, 0.05, 0.1]);
      break;
    case "sever":
      system.runTimeout(() => {
        if (!target.isValid) return;
        hurt(target, Math.min(40, maxHealth(target) * 0.08 + 4), p, EntityDamageCause.magic);
        particle(target.dimension, "xian:sword", V.add(target.location, { x: 0, y: 1, z: 0 }), [1, 0.95, 0.8]);
      }, 11);
      break;
  }
}

// ------------------------------------------------------------------ registration
export function registerItems() {
  world.afterEvents.itemUse.subscribe(({ source, itemStack }) => {
    if (source?.typeId !== "minecraft:player") return;
    const fn = USE[itemStack.typeId];
    if (fn) system.run(() => fn(source, itemStack));
  });
  world.afterEvents.itemCompleteUse.subscribe(({ source, itemStack }) => {
    if (source?.typeId !== "minecraft:player" || !itemStack.typeId.startsWith("xian:")) return;
    onConsume(source, itemStack.typeId);
  });
  world.afterEvents.entityHitEntity.subscribe(({ damagingEntity, hitEntity }) => {
    if (damagingEntity?.typeId !== "minecraft:player") return;
    onWeaponHit(damagingEntity, hitEntity);
    if (D.get(damagingEntity, "sect") === "blood_moon") heal(damagingEntity, 0.5);
  });
}
