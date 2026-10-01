// World systems: stations, herbs, formation arrays, mob abilities, beast tides, merchants, kills & karma.
import { world, system, EntityDamageCause, ItemStack } from "@minecraft/server";
import { ActionFormData, ModalFormData } from "@minecraft/server-ui";
import { MERCHANT_STOCK, SELL_PRICES, ICONS, REALMS } from "./data.js";
import * as D from "./playerdata.js";
import { on, emit } from "./bus.js";
import { addArray, removeArray, findArray, renameArray, arrays, arraysNear } from "./arrays.js";
import {
  realmOf,
  gainExp,
  addBodyExp,
  startMeditation,
  stopMeditation,
  meditating,
  tribulations,
  onPlayerDeath,
} from "./cultivation.js";
import { projectile, noteAttacker } from "./techniques.js";
import { openStation } from "./crafting.js";
import { checkExpulsion } from "./sect.js";
import {
  V,
  rand,
  randInt,
  chance,
  pick,
  say,
  title,
  actionbar,
  broadcast,
  particle,
  ring,
  sound,
  effect,
  heal,
  hurt,
  giveItem,
  countItem,
  walletValue,
  pay,
  itemName,
  dimById,
  maxHealth,
  familiesOf,
} from "./util.js";

const ARRAY_TYPES = ["qi_gathering_array", "protection_array", "teleport_array"];
const DIMS = ["overworld", "nether", "the_end"];
const ABILITY_MOBS = [
  "xian:rogue_cultivator",
  "xian:demonic_cultivator",
  "xian:heart_demon",
  "xian:flood_dragon",
  "xian:flame_tiger",
];

// ------------------------------------------------------------------ block custom components
export function registerBlockComponents(registry) {
  registry.registerCustomComponent("xian:herb", {
    onRandomTick({ block }) {
      const g = block.permutation.getState(/** @type {any} */ ("xian:growth"));
      if (typeof g !== "number" || g >= 2) return;
      const boosted = arraysNear("qi_gathering_array", block.dimension.id, block.location, 8).length > 0;
      if (Math.random() < (boosted ? 0.7 : 0.35))
        block.setPermutation(block.permutation.withState(/** @type {any} */ ("xian:growth"), g + 1));
    },
    onPlayerInteract({ block, player }) {
      if (player) system.run(() => fertilize(block, player));
    },
  });
  registry.registerCustomComponent("xian:station", {
    onPlayerInteract({ block, player }) {
      if (player) system.run(() => station(block, player));
    },
  });
}

function fertilize(block, player) {
  const c = player.getComponent("minecraft:inventory")?.container;
  const held = c?.getItem(player.selectedSlotIndex);
  const g = block.permutation.getState(/** @type {any} */ ("xian:growth"));
  if (held?.typeId !== "minecraft:bone_meal" || typeof g !== "number" || g >= 2) return;
  block.setPermutation(block.permutation.withState(/** @type {any} */ ("xian:growth"), g + 1));
  if (held.amount > 1) {
    held.amount--;
    c.setItem(player.selectedSlotIndex, held);
  } else c.setItem(player.selectedSlotIndex, undefined);
  particle(block.dimension, "minecraft:crop_growth_emitter", V.add(block.location, { x: 0.5, y: 0.5, z: 0.5 }));
}

function station(block, player) {
  const id = block.typeId.replace("xian:", "");
  if (id === "alchemy_furnace") return openStation(player, "alchemy", block);
  if (id === "refining_forge") return openStation(player, "refining", block);
  if (id === "meditation_cushion") {
    player.teleport(V.add(block.location, { x: 0.5, y: 0.3, z: 0.5 }));
    return system.runTimeout(() => startMeditation(player, true), 2);
  }
  if (id === "teleport_array") return teleportMenu(player, block);
  if (id === "qi_gathering_array") {
    const n = arraysNear("qi_gathering_array", block.dimension.id, block.location, 8).length;
    return say(
      player,
      `§b[Array] §7Qi Gathering Array active. ${n} array(s) here boost meditation within 8 blocks by x${(1 + 0.6 * Math.min(3, n)).toFixed(1)}. Herbs nearby grow twice as fast.`,
    );
  }
  if (id === "protection_array")
    say(player, "§6[Array] §7Heaven Guarding Array active: monsters within 12 blocks are repelled.");
}

// ------------------------------------------------------------------ teleport arrays
function teleportMenu(p, block) {
  const here = findArray(block.dimension.id, block.location);
  if (!here) addArray("teleport_array", block.dimension.id, block.location);
  if (!findArray(block.dimension.id, block.location)?.n) return nameArray(p, block);
  const others = arrays("teleport_array").filter(
    (a) =>
      !(a.d === block.dimension.id && a.x === block.location.x && a.y === block.location.y && a.z === block.location.z),
  );
  const f = new ActionFormData()
    .title("Teleportation Array")
    .body(
      `§7This array: §f${findArray(block.dimension.id, block.location).n}\n§7Cost: 1 low-grade spirit stone per jump.`,
    );
  f.button("Rename this array");
  for (const a of others) f.button(`${a.n || "Unnamed"}\n§8${a.d.replace("minecraft:", "")} ${a.x}, ${a.y}, ${a.z}`);
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    if (r.selection === 0) return nameArray(p, block);
    const a = others[r.selection - 1];
    const dim = dimById(a.d);
    if (!dim) return;
    if (!pay(p, 1)) return say(p, "§cYou need a spirit stone to power the array.");
    ring(p.dimension, p.location, 1, "xian:rune", [0.8, 0.5, 1], 20, 0.1);
    sound(p, "mob.endermen.portal");
    p.teleport({ x: a.x + 0.5, y: a.y + 1, z: a.z + 0.5 }, { dimension: dim });
    system.runTimeout(() => {
      if (p.isValid) ring(p.dimension, p.location, 1, "xian:rune", [0.8, 0.5, 1], 20, 0.1);
    }, 5);
  });
}

function nameArray(p, block) {
  new ModalFormData()
    .title("Name Teleportation Array")
    .textField("Array name", "e.g. Sect Gate", { defaultValue: findArray(block.dimension.id, block.location)?.n ?? "" })
    .show(p)
    .then((r) => {
      if (r.canceled || !r.formValues) return;
      const name = String(r.formValues[0] ?? "").slice(0, 32) || `Array ${block.location.x},${block.location.z}`;
      renameArray(block.dimension.id, block.location, name);
      say(p, `§d[Array] §7Teleportation array named §f${name}§7.`);
    });
}

function protectionTick() {
  for (const a of arrays("protection_array")) {
    const dim = dimById(a.d);
    if (!dim) continue;
    const loc = { x: a.x + 0.5, y: a.y + 0.5, z: a.z + 0.5 };
    let block;
    try {
      block = dim.getBlock({ x: a.x, y: a.y, z: a.z });
    } catch {
      continue;
    }
    if (!block) continue;
    if (block.typeId !== "xian:protection_array") {
      removeArray(a.d, a);
      continue;
    }
    let mobs = [];
    try {
      mobs = dim.getEntities({ location: loc, maxDistance: 12, families: ["monster"] });
    } catch {}
    if (!mobs.length) continue;
    ring(dim, loc, 2, "xian:rune", [1, 0.85, 0.35], 16, 0.2);
    for (const m of mobs) {
      const d = V.flat(V.sub(m.location, loc));
      try {
        m.applyKnockback({ x: d.x * 1.6, z: d.z * 1.6 }, 0.35);
      } catch {}
      hurt(m, 2, undefined, EntityDamageCause.magic);
      particle(dim, "xian:spark", V.add(m.location, { x: 0, y: 1, z: 0 }), [1, 0.85, 0.35]);
    }
  }
}

// ------------------------------------------------------------------ mob abilities
function nearestPlayer(e, range) {
  try {
    return e.dimension.getPlayers({ location: e.location, maxDistance: range, closest: 1 })[0];
  } catch {
    return undefined;
  }
}

function mobBolt(e, target, power, rgb, extra) {
  const from = e.getHeadLocation();
  const dir = V.norm(V.sub(V.add(target.location, { x: rand(-0.3, 0.3), y: 1.2, z: rand(-0.3, 0.3) }), from));
  projectile(e, {
    from: V.add(from, V.mul(dir, 0.8)),
    dir,
    speed: 0.9,
    range: 24,
    particle: "xian:orb",
    rgb,
    onHit: (t) => {
      hurt(t, power, e, EntityDamageCause.magic);
      extra?.(t);
    },
  });
}

function mobTick() {
  for (const dn of DIMS) {
    const dim = world.getDimension(dn);
    let ents = [];
    for (const type of ABILITY_MOBS) {
      try {
        ents = ents.concat(dim.getEntities({ type }));
      } catch {}
    }
    for (const e of ents) {
      if (!e.isValid) continue;
      try {
        ability(e);
      } catch {}
    }
  }
}

function ability(e) {
  switch (e.typeId) {
    case "xian:rogue_cultivator": {
      const p = nearestPlayer(e, 18);
      if (p && V.dist(p.location, e.location) > 3.5 && chance(0.35)) {
        mobBolt(e, p, 4, [0.55, 0.75, 1]);
        sound(e, "mob.blaze.shoot", { pitch: 1.6, volume: 0.5 });
      }
      break;
    }
    case "xian:demonic_cultivator": {
      const p = nearestPlayer(e, 18);
      if (p && chance(0.3)) {
        mobBolt(e, p, 5, [0.75, 0.05, 0.1], (t) => effect(t, "wither", 3, 0));
        sound(e, "mob.wither.shoot", { pitch: 1.4, volume: 0.5 });
      }
      const h = e.getComponent("minecraft:health");
      if (h && h.currentValue < h.effectiveMax * 0.5 && chance(0.1)) {
        heal(e, 8);
        ring(e.dimension, e.location, 1, "xian:orb", [0.8, 0.05, 0.1], 12, 1);
      }
      break;
    }
    case "xian:heart_demon": {
      const victimId = e.getDynamicProperty("xian:victim");
      const p = world.getAllPlayers().find((x) => x.id === victimId) ?? nearestPlayer(e, 24);
      if (!p) break;
      if (chance(0.2)) {
        const d = V.flat(p.getViewDirection());
        try {
          e.teleport(V.sub(p.location, V.mul(d, 2)), { facingLocation: p.location });
        } catch {}
        particle(e.dimension, "xian:rune", e.location, [0.6, 0.3, 1]);
        sound(e, "mob.endermen.portal");
      } else if (chance(0.3)) {
        mobBolt(e, p, 5, [0.6, 0.3, 1], (t) => effect(t, "blindness", 2, 0));
      }
      break;
    }
    case "xian:flood_dragon": {
      const p = nearestPlayer(e, 40);
      if (e.isInWater) heal(e, 3);
      if (!p) break;
      if (chance(0.4)) {
        for (let k = 0; k < 3; k++)
          system.runTimeout(() => e.isValid && p.isValid && mobBolt(e, p, 8, [0.3, 0.7, 1]), k * 4);
        sound(e, "mob.guardian.attack");
      } else if (chance(0.15)) {
        const loc = V.add(p.location, { x: rand(-3, 3), y: 0, z: rand(-3, 3) });
        try {
          e.dimension.spawnEntity("minecraft:lightning_bolt", loc);
        } catch {}
      } else if (chance(0.08)) {
        sound(e, "mob.enderdragon.growl", { volume: 2 });
        for (const pl of e.dimension.getPlayers({ location: e.location, maxDistance: 24 }))
          effect(pl, "slowness", 4, 1);
      }
      break;
    }
    case "xian:flame_tiger": {
      const p = nearestPlayer(e, 7);
      if (p && chance(0.25)) {
        const from = e.getHeadLocation();
        const dir = V.norm(V.sub(p.location, from));
        for (let k = 1; k < 7; k++) particle(e.dimension, "xian:flame", V.add(from, V.mul(dir, k)));
        sound(e, "mob.blaze.breathe");
        if (V.dist(p.location, e.location) < 6) {
          p.setOnFire(3, true);
          hurt(p, 3, e, EntityDamageCause.fire);
        }
      }
      break;
    }
  }
}

// ------------------------------------------------------------------ beast tides
const tides = new Map(); // playerId -> state

function tideMobs(i) {
  if (i < 2) return ["xian:demonic_wolf"];
  if (i < 4) return ["xian:demonic_wolf", "xian:demonic_wolf", "xian:flame_tiger", "xian:rogue_cultivator"];
  return ["xian:demonic_wolf", "xian:flame_tiger", "xian:rogue_cultivator", "xian:demonic_cultivator"];
}

export function startTide(p) {
  if (tides.has(p.id)) return (say(p, "§7A beast tide is already upon you!"), false);
  tides.set(p.id, {
    wave: 0,
    waves: 3,
    tag: `xian_tide_${tides.size}_${system.currentTick}`,
    deadline: system.currentTick + 20 * 300,
    next: system.currentTick + 60,
  });
  title(p, "§cBeast Tide!", "§7Survive 3 waves of spirit beasts");
  broadcast(`§c[Beast Tide] §7The beasts grow restless around §f${p.name}§7...`);
  sound(p, "raid.horn", { volume: 1.5 });
  return true;
}

function spawnWave(p, t) {
  const { i } = realmOf(p);
  const n = 3 + t.wave * 2 + Math.min(i, 6);
  const types = tideMobs(i);
  const dim = p.dimension;
  for (let k = 0; k < n; k++) {
    const a = Math.random() * Math.PI * 2;
    const r = rand(12, 18);
    const x = p.location.x + Math.cos(a) * r;
    const z = p.location.z + Math.sin(a) * r;
    let y = p.location.y;
    try {
      const top = dim.getTopmostBlock({ x, z });
      if (top && Math.abs(top.location.y - p.location.y) < 20) y = top.location.y + 1;
    } catch {}
    try {
      const m = dim.spawnEntity(pick(types), { x, y, z });
      m.addTag(t.tag);
      if (i >= 3) effect(m, "strength", 600, Math.min(4, i - 2), false);
      if (i >= 3) effect(m, "health_boost", 600, Math.min(9, i * 2), false);
      if (i >= 3) heal(m, 1000);
    } catch {}
  }
  say(p, `§c[Beast Tide] §7Wave ${t.wave}/${t.waves}: ${n} beasts approach!`);
}

function tideTick() {
  for (const [id, t] of tides) {
    const p = world.getAllPlayers().find((x) => x.id === id);
    if (!p || system.currentTick > t.deadline) {
      tides.delete(id);
      if (p) say(p, "§7The beast tide recedes.");
      continue;
    }
    if (t.wave === 0 || system.currentTick < t.next) {
      if (t.wave === 0 && system.currentTick >= t.next) {
        t.wave = 1;
        spawnWave(p, t);
        t.next = system.currentTick + 100;
      }
      continue;
    }
    let alive = 0;
    try {
      alive = p.dimension.getEntities({ tags: [t.tag] }).length;
    } catch {}
    if (alive > 0) {
      if (system.currentTick % 200 === 0)
        actionbar(p, `§cBeast tide: ${alive} beasts remain (wave ${t.wave}/${t.waves})`);
      continue;
    }
    if (t.wave < t.waves) {
      t.wave++;
      spawnWave(p, t);
      t.next = system.currentTick + 100;
    } else {
      tides.delete(id);
      const { i } = realmOf(p);
      giveItem(p, "xian:low_spirit_stone", 20 + i * 5);
      giveItem(p, i >= 3 ? "xian:mid_beast_core" : "xian:low_beast_core", 2 + Math.floor(i / 2));
      if (i >= 2) giveItem(p, "xian:mid_spirit_stone", 2 + i);
      if (chance(0.3))
        giveItem(
          p,
          pick([
            "xian:scroll_frost_spikes",
            "xian:scroll_thunder_palm",
            "xian:manual_five_elements",
            "xian:foundation_pill",
          ]),
        );
      gainExp(p, REALMS[i].gain * 120);
      if (D.get(p, "sect")) D.add(p, "contrib", 80);
      D.add(p, "karma", 3);
      title(p, "§aBeast Tide Repelled!", "§7Rewards have been placed in your bag");
      broadcast(`§a[Beast Tide] §f${p.name}§7 has repelled the beast tide!`);
      sound(p, "random.levelup");
    }
  }
}

let lastTideDay = -1;
function nightlyTides() {
  if (world.getDynamicProperty("xian:tides") === false) return;
  const t = world.getTimeOfDay();
  if (t < 13000 || t > 13200) return;
  const day = Math.floor(world.getAbsoluteTime() / 24000);
  if (day === lastTideDay) return;
  lastTideDay = day;
  for (const p of world.getDimension("overworld").getPlayers()) {
    if (realmOf(p).i >= 1 && chance(0.1)) startTide(p);
  }
}

// ------------------------------------------------------------------ merchant
function openMerchant(p) {
  const f = new ActionFormData()
    .title("Wandering Pill Merchant")
    .body(
      `§7"Pills, talismans, secret arts! Fair prices, young master."\n§7Your spirit stones: §b${walletValue(p)}§7 (low-grade value)`,
    );
  f.button("Buy", ICONS.qi_gathering_pill);
  f.button("Sell spirit materials", ICONS.low_beast_core);
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    if (r.selection === 0) buyMenu(p);
    else sellMenu(p);
  });
}

function buyMenu(p) {
  const f = new ActionFormData().title("Buy").body(`§7Spirit stones: §b${walletValue(p)}`);
  for (const { item, count, price } of MERCHANT_STOCK)
    f.button(`${itemName("xian:" + item)}${count > 1 ? " x" + count : ""}\n§8${price} low-grade stones`, ICONS[item]);
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    const { item, count, price } = MERCHANT_STOCK[r.selection];
    if (!pay(p, price)) return say(p, '§c"No stones, no pills!"');
    giveItem(p, "xian:" + item, count);
    sound(p, "random.orb");
    buyMenu(p);
  });
}

function sellMenu(p) {
  const goods = Object.entries(SELL_PRICES).filter(([id]) => countItem(p, "xian:" + id) > 0);
  const f = new ActionFormData()
    .title("Sell")
    .body(goods.length ? "§7Sell a whole stack of materials." : "§7You have nothing the merchant wants.");
  for (const [id, price] of goods)
    f.button(`${itemName("xian:" + id)} x${countItem(p, "xian:" + id)}\n§8${price} stones each`, ICONS[id]);
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined || !goods[r.selection]) return;
    const [id, price] = goods[r.selection];
    const n = countItem(p, "xian:" + id);
    const c = p.getComponent("minecraft:inventory")?.container;
    if (!c) return;
    for (let k = 0; k < c.size; k++) if (c.getItem(k)?.typeId === "xian:" + id) c.setItem(k, undefined);
    const total = n * price;
    giveStones(p, total);
    sound(p, "random.orb");
    say(p, `§e[Merchant] §7Sold ${n}x ${itemName("xian:" + id)} for ${total} stones.`);
    sellMenu(p);
  });
}

function giveStones(p, value) {
  /** @type {[string, number][]} */
  const denoms = [
    ["xian:high_spirit_stone", 81],
    ["xian:mid_spirit_stone", 9],
    ["xian:low_spirit_stone", 1],
  ];
  for (const [id, v] of denoms) {
    const n = Math.floor(value / v);
    if (n) giveItem(p, id, n);
    value -= n * v;
  }
}

// ------------------------------------------------------------------ kills & karma
function onKill(killer, victim) {
  const { R } = realmOf(killer);
  const mh = maxHealth(victim);
  let exp = mh * 0.25 * R.gain;
  if (D.get(killer, "manual") === "heavenly_devouring") exp *= 5;
  if (D.get(killer, "sect") === "blood_moon") exp *= 2;
  gainExp(killer, exp, true);
  D.add(killer, "kills", 1);
  const fams = familiesOf(victim);
  const fam = (f) => fams.includes(f);
  let karma = 0;
  if (victim.typeId === "minecraft:player") karma = D.get(victim, "karma") < -50 ? 5 : -10;
  else if (fam("villager") || fam("wandering_trader") || fam("merchant") || victim.typeId === "minecraft:iron_golem")
    karma = -5;
  else if (fam("demonic_cultivator") || fam("heart_demon")) karma = 5;
  else if (fam("rogue_cultivator")) karma = 1;
  else if (fam("monster")) karma = 0.1;
  if (karma) {
    D.add(killer, "karma", karma);
    if (karma <= -5) actionbar(killer, `§4Karma ${karma}`);
    checkExpulsion(killer);
  }
  emit("kill", killer, victim, fams);
}

// ------------------------------------------------------------------ registration
export function registerWorld() {
  world.afterEvents.playerPlaceBlock.subscribe(({ block, player }) => {
    const id = block.typeId.replace("xian:", "");
    if (!ARRAY_TYPES.includes(id)) return;
    addArray(id, block.dimension.id, block.location);
    if (id === "teleport_array") system.runTimeout(() => nameArray(player, block), 5);
  });

  world.afterEvents.playerBreakBlock.subscribe(({ block, brokenBlockPermutation, player }) => {
    const id = brokenBlockPermutation.type.id;
    if (!id.startsWith("xian:")) return;
    const short = id.slice(5);
    if (ARRAY_TYPES.includes(short)) removeArray(block.dimension.id, block.location);
    if (short.endsWith("_plant") && brokenBlockPermutation.getState(/** @type {any} */ ("xian:growth")) === 2) {
      const herb = short.replace(/_plant$/, "");
      const at = V.add(block.location, { x: 0.5, y: 0.3, z: 0.5 });
      const drop = (iid, n = 1) => {
        try {
          block.dimension.spawnItem(new ItemStack(iid, n), at);
        } catch {}
      };
      drop("xian:" + herb, randInt(1, 2));
      if (chance(0.15)) drop("xian:spirit_fruit");
      if (chance(0.02)) {
        drop("xian:immortal_peach");
        say(player, "§d[Herb] §7A heavenly fragrance... you found an §dImmortal Peach§7!");
      }
    }
  });

  world.afterEvents.entityDie.subscribe(({ deadEntity, damageSource }) => {
    if (deadEntity.typeId === "minecraft:player") onPlayerDeath(deadEntity);
    const killer = damageSource?.damagingEntity;
    if (killer?.typeId === "minecraft:player" && killer.id !== deadEntity.id) onKill(killer, deadEntity);
  });

  world.afterEvents.entityHurt.subscribe(({ hurtEntity, damage, damageSource }) => {
    if (hurtEntity.typeId !== "minecraft:player") return;
    const attacker = damageSource?.damagingEntity;
    if (attacker && attacker.id !== hurtEntity.id) noteAttacker(hurtEntity, attacker);
    if (damage > 0 && meditating.has(hurtEntity.id) && damageSource.cause !== EntityDamageCause.starve) {
      stopMeditation(hurtEntity, "§cYour meditation is interrupted!");
    }
    if (damage > 0 && !tribulations.has(hurtEntity.id)) {
      const { i } = realmOf(hurtEntity);
      addBodyExp(hurtEntity, Math.min(damage, 20) * 0.5 * (1 + i * 0.3));
    } else if (tribulations.has(hurtEntity.id)) {
      addBodyExp(hurtEntity, damage * 2); // tribulation lightning tempers the body
    }
  });

  world.afterEvents.playerInteractWithEntity.subscribe(({ player, target }) => {
    if (target.typeId === "xian:wandering_merchant") system.run(() => openMerchant(player));
  });

  world.afterEvents.dataDrivenEntityTrigger.subscribe(({ entity, eventId }) => {
    if (eventId !== "xian:on_tame") return;
    const p = entity.dimension.getPlayers({ location: entity.location, maxDistance: 8, closest: 1 })[0];
    if (p) {
      entity.setDynamicProperty("xian:owner", p.id);
      say(
        p,
        "§f[Spirit Fox] §7The fox's tails sway happily - it has bonded with you. §8(+10% cultivation while it is near)",
      );
    }
  });

  on("beast_tide", (p, cb) => cb(startTide(p)));

  system.runInterval(mobTick, 20);
  system.runInterval(protectionTick, 40);
  system.runInterval(() => {
    tideTick();
    nightlyTides();
  }, 20);
}
