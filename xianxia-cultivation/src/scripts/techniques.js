// Martial techniques: cast with the Martial Jade Seal, also used by talismans and hostile cultivators.
import { system, EntityDamageCause } from "@minecraft/server";
import { TECHNIQUES, ELEMENTS } from "./data.js";
import * as D from "./playerdata.js";
import { realmOf, powerMult, hasElement, spendQi, primaryColor } from "./cultivation.js";
import {
  V,
  rand,
  say,
  actionbar,
  particle,
  ring,
  sound,
  soundAt,
  effect,
  heal,
  hurt,
  enemiesNear,
  lookTarget,
  lookBlock,
  aimPoint,
  blocksProjectile,
  validTarget,
  compass,
  recentAttackers,
} from "./util.js";

export const TECH = Object.fromEntries(TECHNIQUES.map((t) => [t.id, t]));
const cooldowns = new Map(); // `${id}:${tech}` -> tick

export function learned(player) {
  return D.getJSON(player, "techs", []);
}

export function learn(player, id) {
  const list = learned(player);
  if (list.includes(id)) return false;
  list.push(id);
  D.setJSON(player, "techs", list);
  if (!D.get(player, "techSel")) D.set(player, "techSel", id);
  return true;
}

export function cooldownLeft(player, id) {
  return Math.max(0, (cooldowns.get(`${player.id}:${id}`) ?? 0) - system.currentTick);
}

function elRGB(el) {
  return (ELEMENTS[el] ?? ELEMENTS.none).rgb;
}

/** Cast a technique as a player (checks realm, cooldown and qi). */
export function cast(player, id) {
  const t = TECH[id];
  if (!t) return say(player, "§7Choose a technique first (sneak + use the Martial Jade Seal).");
  if (!learned(player).includes(id)) return say(player, "§7You have not learned that technique.");
  const { i } = realmOf(player);
  if (i < t.realm) return actionbar(player, `§c${t.name} requires a higher cultivation realm.`);
  const cd = cooldownLeft(player, id);
  if (cd > 0) return actionbar(player, `§7${t.name}: §c${(cd / 20).toFixed(1)}s`);
  const compatible = hasElement(player, t.element);
  const sect = D.get(player, "sect");
  let cost = t.cost * (compatible ? 1 : 1.5);
  if (sect === "azure_cloud" && (id === "sword_qi" || id === "sword_rain")) cost *= 0.8;
  cost = Math.ceil(cost);
  if (id === "blood_sacrifice") {
    const h = player.getComponent("minecraft:health");
    const price = 6 + i;
    if (!h || h.currentValue <= price + 1) return actionbar(player, "§cNot enough blood to sacrifice.");
    hurt(player, price, undefined, EntityDamageCause.magic);
    D.add(player, "karma", -1);
  } else if (!spendQi(player, cost)) {
    return actionbar(player, `§cNot enough qi (${cost} needed)`);
  }
  let power = t.power * powerMult(player) * (compatible ? 1 : 0.75);
  if (sect === "azure_cloud") power *= 1.2;
  cooldowns.set(`${player.id}:${id}`, system.currentTick + t.cd);
  const ok = EFFECTS[id]?.(player, power, t);
  if (ok !== false) actionbar(player, `§b${t.name}${compatible ? "" : " §8(incompatible element)"}`, 30);
}

/** Used by talismans and mobs: cast an effect without qi checks. */
export function castFree(caster, id, power) {
  return EFFECTS[id]?.(caster, power, TECH[id]);
}

// ------------------------------------------------------------------ projectile engine
/**
 * Moves a "qi projectile" each tick, drawing particles and hitting entities.
 * opts: {speed, range, radius, particle, rgb, pierce, onHit(e, loc), onEnd(loc), gravity, from, dir}
 */
export function projectile(caster, opts) {
  const dim = caster.dimension;
  let pos = opts.from ?? V.add(caster.getHeadLocation(), V.mul(caster.getViewDirection(), 0.8));
  let dir = V.norm(opts.dir ?? caster.getViewDirection());
  const speed = opts.speed ?? 1.2;
  const maxSteps = Math.ceil((opts.range ?? 30) / speed);
  const hit = new Set();
  let step = 0;
  const handle = system.runInterval(() => {
    let finished = false;
    const sub = Math.max(1, Math.ceil(speed / 0.5));
    for (let s = 0; s < sub && !finished; s++) {
      pos = V.add(pos, V.mul(dir, speed / sub));
      if (opts.gravity) dir = V.norm(V.add(dir, { x: 0, y: -opts.gravity, z: 0 }));
      if (blocksProjectile(dim, pos)) {
        finished = true;
        break;
      }
      let ents = [];
      try {
        ents = dim.getEntities({ location: pos, maxDistance: opts.radius ?? 1.2 });
      } catch {}
      for (const e of ents) {
        if (hit.has(e.id) || !validTarget(caster, e, false)) continue;
        hit.add(e.id);
        opts.onHit?.(e, pos);
        if (!opts.pierce) {
          finished = true;
          break;
        }
      }
    }
    particle(dim, opts.particle ?? "xian:orb", pos, opts.rgb);
    if (opts.trail) particle(dim, opts.trail, pos, opts.rgb);
    step++;
    if (finished || step >= maxSteps || !caster.isValid) {
      system.clearRun(handle);
      opts.onEnd?.(pos);
    }
  }, 1);
}

function bolt(dim, loc, rgb = [0.75, 0.6, 1]) {
  for (let y = 0; y < 12; y += 0.7) {
    particle(dim, "xian:lightning", { x: loc.x + rand(-0.4, 0.4), y: loc.y + y, z: loc.z + rand(-0.4, 0.4) });
  }
  particle(dim, "xian:spark", V.add(loc, { x: 0, y: 0.5, z: 0 }), rgb);
  soundAt(dim, loc, "ambient.weather.lightning.impact", { volume: 0.8 });
}

function burst(dim, loc, id, rgb, n = 10, r = 1) {
  for (let k = 0; k < n; k++) {
    particle(dim, id, { x: loc.x + rand(-r, r), y: loc.y + rand(0, r * 1.5), z: loc.z + rand(-r, r) }, rgb);
  }
}

function knockFrom(e, from, h = 1.2, v = 0.4) {
  try {
    const d = V.flat(V.sub(e.location, from));
    e.applyKnockback({ x: d.x * h, z: d.z * h }, v);
  } catch {}
}

// ------------------------------------------------------------------ technique effects
const EFFECTS = {
  qi_bolt(c, power) {
    sound(c, "mob.blaze.shoot", { pitch: 1.6, volume: 0.6 });
    projectile(c, {
      speed: 1.4,
      range: 32,
      particle: "xian:orb",
      rgb: c.typeId === "minecraft:player" ? primaryColor(c) : [0.6, 0.6, 1],
      onHit: (e) => hurt(e, power, c),
    });
  },

  wind_step(c) {
    const d = V.flat(c.getViewDirection());
    c.applyKnockback({ x: d.x * 3.2, z: d.z * 3.2 }, 0.25);
    effect(c, "slow_falling", 1.5, 0, false);
    burst(c.dimension, c.location, "xian:aura", elRGB("wind"), 12, 0.8);
    sound(c, "mob.phantom.flap", { pitch: 1.4 });
  },

  fireball(c, power) {
    sound(c, "mob.ghast.fireball", { volume: 0.7 });
    projectile(c, {
      speed: 1.0,
      range: 30,
      particle: "xian:flame",
      trail: "minecraft:basic_flame_particle",
      onHit: (e) => {
        hurt(e, power, c, EntityDamageCause.fire);
        e.setOnFire(4, true);
      },
      onEnd: (loc) => {
        burst(c.dimension, loc, "xian:flame", undefined, 14, 1.2);
        particle(c.dimension, "minecraft:large_explosion", loc);
        soundAt(c.dimension, loc, "random.explode", { volume: 0.4, pitch: 1.4 });
        for (const e of enemiesNear(c, loc, 2.5)) {
          hurt(e, power * 0.5, c, EntityDamageCause.fire);
          e.setOnFire(3, true);
        }
      },
    });
  },

  sword_qi(c, power) {
    sound(c, "item.trident.throw", { pitch: 1.3 });
    const rgb = elRGB("metal");
    projectile(c, {
      speed: 1.6,
      range: 18,
      radius: 1.8,
      pierce: true,
      particle: "xian:sword",
      rgb,
      onHit: (e, loc) => {
        hurt(e, power, c);
        particle(c.dimension, "minecraft:critical_hit_emitter", loc);
      },
    });
  },

  frost_spikes(c, power) {
    const loc = c.location;
    const r = 5 + realmOf(c).i * 0.4;
    for (let k = 1; k <= 3; k++) {
      system.runTimeout(() => ring(c.dimension, loc, (r * k) / 3, "xian:frost", undefined, 10 + k * 6, 0.2), k * 2);
    }
    sound(c, "random.glass", { pitch: 0.6 });
    for (const e of enemiesNear(c, loc, r)) {
      hurt(e, power, c, EntityDamageCause.freezing);
      effect(e, "slowness", 5, 3);
      burst(c.dimension, e.location, "xian:frost", undefined, 6, 0.5);
    }
  },

  healing_spring(c) {
    const amount = 6 + realmOf(c).i * 3;
    heal(c, amount);
    effect(c, "regeneration", 8, 1);
    for (const p of c.dimension.getPlayers({ location: c.location, maxDistance: 8 })) {
      if (p.id !== c.id) {
        heal(p, amount / 2);
        effect(p, "regeneration", 6, 0);
      }
    }
    for (let k = 0; k < 3; k++)
      system.runTimeout(() => ring(c.dimension, c.location, 1 + k, "xian:leaf", undefined, 12, 0.5), k * 3);
    sound(c, "random.orb", { pitch: 0.8 });
  },

  spirit_sense(c) {
    const { i } = realmOf(c);
    const r = 16 + i * 8;
    let ents = [];
    try {
      ents = c.dimension.getEntities({
        location: c.location,
        maxDistance: r,
        excludeTypes: ["minecraft:item", "minecraft:xp_orb"],
      });
    } catch {}
    const groups = new Map();
    for (const e of ents) {
      if (e.id === c.id || !e.getComponent("minecraft:health")) continue;
      const name = e.typeId === "minecraft:player" ? e.name : e.typeId.replace(/^.*:/, "").replace(/_/g, " ");
      const d = V.dist(c.location, e.location);
      const g = groups.get(name) ?? { n: 0, d: 1e9, loc: e.location };
      g.n++;
      if (d < g.d) {
        g.d = d;
        g.loc = e.location;
      }
      groups.set(name, g);
      try {
        c.spawnParticle("xian:rune", V.add(e.location, { x: 0, y: 2.2, z: 0 }));
      } catch {}
    }
    const lines = [...groups.entries()]
      .sort((a, b) => a[1].d - b[1].d)
      .slice(0, 8)
      .map(([n, g]) => `§f${g.n}x ${n} §7(${Math.round(g.d)}m ${compass(c.location, g.loc)})`);
    say(c, `§b[Divine Sense] §7Radius ${r}m: ${lines.length ? lines.join("§8, ") : "§8nothing stirs."}`);
    // sense treasures in the earth
    system.runJob(senseOres(c, 6 + Math.min(6, i)));
    ring(c.dimension, c.location, 2, "xian:rune", primaryColor(c), 16, 0.1);
    sound(c, "beacon.ambient", { pitch: 1.6 });
  },

  vine_bind(c, power) {
    const t = lookTarget(c, 22);
    if (!t) return (actionbar(c, "§7No target in sight."), false);
    hurt(t, power, c);
    effect(t, "slowness", 4, 6);
    effect(t, "weakness", 4, 1);
    burst(c.dimension, t.location, "xian:leaf", undefined, 16, 0.8);
    sound(t, "dig.grass");
  },

  tidal_palm(c, power) {
    const dir = V.flat(c.getViewDirection());
    const origin = c.location;
    for (let d = 1; d < 9; d++) {
      system.runTimeout(() => {
        const p = V.add(origin, V.mul(dir, d));
        for (let w = -2; w <= 2; w++) {
          particle(c.dimension, "xian:orb", V.add(p, { x: -dir.z * w, y: 0.6, z: dir.x * w }), elRGB("water"));
        }
      }, d);
    }
    sound(c, "random.splash");
    for (const e of enemiesNear(c, origin, 9)) {
      const to = V.flat(V.sub(e.location, origin));
      if (V.dot(to, dir) < 0.5) continue;
      hurt(e, power, c);
      knockFrom(e, origin, 2.2, 0.5);
    }
  },

  earth_shield(c) {
    effect(c, "resistance", 15, 2);
    effect(c, "absorption", 15, 2);
    effect(c, "slowness", 15, 0);
    ring(c.dimension, c.location, 1.2, "xian:rune", elRGB("earth"), 20, 0.1);
    sound(c, "dig.stone", { pitch: 0.5 });
  },

  golden_bell(c) {
    const secs = 4 + realmOf(c).i * 0.5;
    effect(c, "resistance", secs, 4);
    for (let k = 0; k < 4; k++)
      system.runTimeout(() => ring(c.dimension, c.location, 1.3, "xian:rune", [1, 0.85, 0.3], 20, k * 0.6), k * 3);
    sound(c, "block.bell.hit", { pitch: 0.7 });
  },

  thunder_palm(c, power) {
    const t = lookTarget(c, 28);
    const loc = t?.location ?? lookBlock(c, 28);
    if (!loc) return (actionbar(c, "§7No target in sight."), false);
    if (V.dist(loc, c.location) > 5) {
      try {
        c.dimension.spawnEntity("minecraft:lightning_bolt", loc);
      } catch {}
    } else bolt(c.dimension, loc);
    for (const e of enemiesNear(c, loc, 2.5)) hurt(e, power, c, EntityDamageCause.lightning);
    if (t && !enemiesNear(c, loc, 2.5).includes(t)) hurt(t, power, c, EntityDamageCause.lightning);
  },

  phoenix_flame(c, power) {
    const dir = V.norm(c.getViewDirection());
    const head = c.getHeadLocation();
    for (let k = 0; k < 6; k++) {
      system.runTimeout(() => {
        for (let n = 0; n < 10; n++) {
          const spread = V.norm(V.add(dir, { x: rand(-0.35, 0.35), y: rand(-0.2, 0.2), z: rand(-0.35, 0.35) }));
          particle(c.dimension, "xian:flame", V.add(head, V.mul(spread, rand(1, 10))));
        }
      }, k * 2);
    }
    sound(c, "mob.blaze.breathe", { volume: 1 });
    for (const e of enemiesNear(c, head, 11)) {
      const to = V.norm(V.sub(e.location, head));
      if (V.dot(to, dir) < 0.75) continue;
      hurt(e, power, c, EntityDamageCause.fire);
      e.setOnFire(6, true);
    }
  },

  buddha_palm(c, power) {
    const loc = aimPoint(c, 36);
    const dim = c.dimension;
    sound(c, "beacon.power", { pitch: 0.5 });
    for (let k = 0; k < 12; k++) {
      system.runTimeout(() => {
        const y = 14 - k;
        ring(dim, { x: loc.x, y: loc.y + y, z: loc.z }, 2.5, "xian:rune", [1, 0.8, 0.3], 14, 0);
      }, k);
    }
    system.runTimeout(() => {
      if (!c.isValid) return;
      particle(dim, "minecraft:huge_explosion_emitter", loc);
      soundAt(dim, loc, "random.explode", { volume: 1, pitch: 0.6 });
      ring(dim, loc, 4, "xian:spark", [1, 0.85, 0.3], 24, 0.2);
      for (const e of enemiesNear(c, loc, 5)) {
        hurt(e, power, c);
        knockFrom(e, loc, 0.6, 0.9);
      }
    }, 13);
  },

  heavenly_thunder(c, power) {
    const targets = enemiesNear(c, c.location, 16).slice(0, 6 + realmOf(c).i);
    if (!targets.length) return (actionbar(c, "§7No enemies nearby."), false);
    sound(c, "ambient.weather.thunder", { volume: 1 });
    targets.forEach((e, k) => {
      system.runTimeout(() => {
        if (!e.isValid) return;
        bolt(c.dimension, e.location);
        hurt(e, power, c, EntityDamageCause.lightning);
      }, k * 4);
    });
  },

  void_step(c) {
    const range = 20 + realmOf(c).i * 2;
    const dir = c.getViewDirection();
    const b = lookBlock(c, range);
    let dest = b ? V.sub(b, V.mul(V.flat(dir), 0.6)) : V.add(c.location, V.mul(dir, range));
    burst(c.dimension, c.location, "xian:rune", [0.6, 0.4, 1], 6, 0.6);
    if (!c.tryTeleport(dest, { checkForBlocks: true, keepVelocity: false })) {
      dest = V.add(c.location, V.mul(V.flat(dir), Math.min(range, 8)));
      if (!c.tryTeleport(dest, { checkForBlocks: true })) return (actionbar(c, "§7The void resists you."), false);
    }
    effect(c, "slow_falling", 2, 0, false);
    burst(c.dimension, c.location, "xian:rune", [0.6, 0.4, 1], 6, 0.6);
    sound(c, "mob.endermen.portal");
  },

  sword_rain(c, power) {
    const loc = aimPoint(c, 32);
    const dim = c.dimension;
    sound(c, "item.trident.riptide_3", { pitch: 1.5 });
    for (let k = 0; k < 8; k++) {
      system.runTimeout(() => {
        for (let n = 0; n < 8; n++) {
          const p = { x: loc.x + rand(-6, 6), y: loc.y, z: loc.z + rand(-6, 6) };
          for (let y = 8; y >= 0; y -= 2) particle(dim, "xian:sword", { x: p.x, y: p.y + y, z: p.z }, elRGB("metal"));
        }
        if (k % 2 === 0) {
          for (const e of enemiesNear(c, loc, 6.5)) hurt(e, power * 0.5, c);
          soundAt(dim, loc, "random.anvil_land", { volume: 0.3, pitch: 1.8 });
        }
      }, k * 5);
    }
  },

  blood_sacrifice(c, power) {
    const t = lookTarget(c, 16);
    if (!t) return (actionbar(c, "§7No target in sight."), false);
    hurt(t, power, c);
    heal(c, power * 0.5);
    for (let k = 0; k < 16; k++) {
      const p = V.add(c.location, V.mul(V.sub(t.location, c.location), k / 16));
      particle(c.dimension, "xian:orb", V.add(p, { x: 0, y: 1, z: 0 }), [0.8, 0.05, 0.1]);
    }
    sound(c, "mob.wither.hurt", { pitch: 1.2 });
  },

  heavenly_domain(c, power) {
    const center = { ...c.location };
    const dim = c.dimension;
    sound(c, "beacon.activate", { pitch: 0.5, volume: 1.5 });
    for (let s = 0; s < 10; s++) {
      system.runTimeout(() => {
        if (!c.isValid) return;
        ring(dim, center, 14, "xian:rune", primaryColor(c), 40, 0.2);
        for (const e of enemiesNear(c, center, 14)) {
          effect(e, "slowness", 2, 3);
          effect(e, "weakness", 2, 1);
          hurt(e, power, c, EntityDamageCause.magic);
        }
      }, s * 20);
    }
  },

  starfall(c, power) {
    const loc = aimPoint(c, 48);
    const dim = c.dimension;
    sound(c, "mob.enderdragon.growl", { volume: 1, pitch: 1.4 });
    for (let k = 0; k < 10; k++) {
      const impact = { x: loc.x + rand(-8, 8), y: loc.y, z: loc.z + rand(-8, 8) };
      system.runTimeout(() => {
        for (let y = 24; y > 0; y -= 2) {
          system.runTimeout(
            () => particle(dim, "xian:flame", { x: impact.x + y * 0.3, y: impact.y + y, z: impact.z }),
            (24 - y) / 4,
          );
        }
        system.runTimeout(() => {
          if (!c.isValid) return;
          particle(dim, "minecraft:huge_explosion_emitter", impact);
          soundAt(dim, impact, "random.explode", { volume: 1.2, pitch: 0.7 });
          for (const e of enemiesNear(c, impact, 4.5)) {
            hurt(e, power, c, EntityDamageCause.entityExplosion);
            e.setOnFire(5, true);
          }
        }, 7);
      }, k * 6);
    }
  },
};

/** Divine sense ore scan, spread over several ticks via system.runJob. */
function* senseOres(player, r) {
  const found = new Map();
  const b = V.floor(player.location);
  const dim = player.dimension;
  const interesting = /(xian:|diamond_ore|ancient_debris|emerald_ore)/;
  let n = 0;
  for (let x = -r; x <= r; x++) {
    for (let y = -r; y <= r; y++) {
      for (let z = -r; z <= r; z++) {
        let blk;
        try {
          blk = dim.getBlock({ x: b.x + x, y: b.y + y, z: b.z + z });
        } catch {}
        if (blk && interesting.test(blk.typeId) && !blk.typeId.endsWith("_plant")) {
          const g = found.get(blk.typeId) ?? { n: 0, loc: blk.location };
          g.n++;
          found.set(blk.typeId, g);
        }
        if (++n % 300 === 0) yield;
      }
    }
  }
  if (!player.isValid) return;
  if (found.size) {
    const txt = [...found.entries()]
      .map(
        ([id, g]) =>
          `§e${g.n}x ${id.replace(/^.*:/, "").replace(/_/g, " ")} §7(${compass(player.location, g.loc)}, y${g.loc.y})`,
      )
      .join("§8, ");
    say(player, `§b[Divine Sense] §7Treasures within ${r} blocks: ${txt}`);
  }
}

export function noteAttacker(player, attacker) {
  if (!attacker) return;
  let set = recentAttackers.get(player.id);
  if (!set) recentAttackers.set(player.id, (set = new Set()));
  set.add(attacker.id);
  system.runTimeout(() => set.delete(attacker.id), 400);
}
