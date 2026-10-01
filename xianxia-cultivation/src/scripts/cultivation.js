// Core cultivation: spirit roots, realms, meditation, stats, breakthroughs and heavenly tribulations.
import { world, system, EntityDamageCause, MoonPhase, WeatherType } from "@minecraft/server";
import { REALMS, BODY_TIERS, ELEMENTS, ROOT_GRADES, MANUALS, SECTS } from "./data.js";
import * as D from "./playerdata.js";
import { emit } from "./bus.js";
import { arraysNear } from "./arrays.js";
import {
  V,
  pick,
  clamp,
  chance,
  rand,
  fmt,
  say,
  title,
  broadcast,
  particle,
  ring,
  sound,
  effect,
  heal,
  hurt,
  countItem,
  removeItem,
  maxHealth,
} from "./util.js";

export const weather = { thunder: false, rain: false };
export const LAST_REALM = REALMS.length - 1;

// ------------------------------------------------------------------ spirit roots
const BASIC = ["metal", "wood", "water", "fire", "earth"];

export function rollRoots() {
  const r = Math.random();
  if (r < 0.04) return [pick(["lightning", "ice", "wind"])];
  if (r < 0.07) return [pick(BASIC)];
  const n = r < 0.2 ? 2 : r < 0.5 ? 3 : r < 0.8 ? 4 : 5;
  const pool = [...BASIC].sort(() => Math.random() - 0.5);
  return pool.slice(0, n);
}

export function rootGrade(roots) {
  if (!roots.length) return "pseudo";
  if (roots.length === 1) return ELEMENTS[roots[0]]?.mutated ? "mutated" : "heavenly";
  return { 2: "dual", 3: "triple", 4: "quad" }[roots.length] ?? "pseudo";
}

export function rootText(player) {
  if (!D.get(player, "rootsKnown")) return "§8Unknown (use a Spirit Root Testing Stone)";
  const roots = D.roots(player);
  const g = ROOT_GRADES[rootGrade(roots)];
  const els = roots.map((e) => `${ELEMENTS[e].color}${ELEMENTS[e].name}`).join("§7, ");
  return `${g.color}${g.name}§r §7[${els}§7]`;
}

export const rootMult = (player) => ROOT_GRADES[rootGrade(D.roots(player))].mult;

export function hasElement(player, el) {
  if (el === "none") return true;
  if (D.get(player, "manual") === "primordial_chaos") return true;
  const roots = D.roots(player);
  if (roots.includes(el)) return true;
  // mutated roots also resonate with their parent element
  const parent = { ice: "water", lightning: "metal", wind: "wood" };
  return roots.some((r) => parent[r] === el || parent[el] === r);
}

export function primaryColor(player) {
  const r = D.roots(player)[0] ?? "none";
  return ELEMENTS[r]?.rgb ?? ELEMENTS.none.rgb;
}

// ------------------------------------------------------------------ realms
export function realmOf(player) {
  const i = clamp(D.get(player, "realm"), 0, LAST_REALM);
  const R = REALMS[i];
  const stage = clamp(D.get(player, "stage"), 0, R.stages.length - 1);
  return { i, R, stage, stageName: R.stages[stage], next: REALMS[i + 1] };
}

export const expNeeded = (i, s) => Math.floor(REALMS[i].exp * (1 + s * 0.35));

export function maxQi(player) {
  const { R, stage } = realmOf(player);
  return Math.floor(R.qi * (1 + stage * 0.25));
}

export function realmTitle(player) {
  const { R, stageName } = realmOf(player);
  return R.stages.length > 1 ? `${R.color}${R.name} §7(${stageName})` : `${R.color}${R.name}`;
}

export function powerMult(player) {
  const { i, stage } = realmOf(player);
  return 1 + i * 0.9 + stage * 0.12 + D.get(player, "dao") * 0.01;
}

export function isBottleneck(player) {
  const { i, R, stage } = realmOf(player);
  return i > 0 && i < LAST_REALM && stage === R.stages.length - 1 && D.get(player, "exp") >= expNeeded(i, stage);
}

export function sectOf(player) {
  return SECTS.find((s) => s.id === D.get(player, "sect"));
}

// ------------------------------------------------------------------ cultivation speed
export function manualOf(player) {
  return MANUALS.find((m) => m.id === D.get(player, "manual"));
}

function timeOfDay() {
  return world.getTimeOfDay();
}

export function isNight() {
  const t = timeOfDay();
  return t >= 13000 && t <= 23000;
}

export function manualMult(player) {
  const man = manualOf(player);
  if (!man) return { mult: 0.6, notes: ["§8No manual: unguided cultivation x0.6"] };
  let m = man.mult;
  const notes = [];
  if (man.element !== "none" && !hasElement(player, man.element)) {
    m *= 0.6;
    notes.push("§cManual element incompatible with your roots x0.6");
  }
  const loc = player.location;
  switch (man.id) {
    case "five_elements": {
      const b = 0.15 * D.roots(player).length;
      m += b;
      notes.push(`§aFive elements resonance +${b.toFixed(2)}`);
      break;
    }
    case "blazing_sun": {
      const t = timeOfDay();
      if (t >= 4000 && t <= 8000 && player.dimension.id === "minecraft:overworld") {
        m *= 2;
        notes.push("§6Noon sun x2");
      }
      break;
    }
    case "profound_water":
      if (player.isInWater || nearBlock(player, /water/, 2)) {
        m *= 1.5;
        notes.push("§9Water nearby x1.5");
      }
      break;
    case "great_earth":
      if (loc.y < -40) {
        m *= 1.8;
        notes.push("§6Deep earth x1.8");
      } else if (loc.y < 0) {
        m *= 1.5;
        notes.push("§6Underground x1.5");
      }
      break;
    case "nine_heavens_thunder":
      if (weather.thunder) {
        m *= 3;
        notes.push("§dThunderstorm x3");
      }
      break;
    case "frost_moon":
      if (isNight()) {
        const full = world.getMoonPhase() === MoonPhase.FullMoon;
        m *= full ? 2 : 1.5;
        notes.push(full ? "§bFull moon x2" : "§bNight x1.5");
      }
      break;
    case "void_wind":
      if (loc.y > 150) {
        m *= 1.5;
        notes.push("§2High altitude x1.5");
      }
      break;
  }
  return { mult: m, notes };
}

function nearBlock(player, re, r) {
  const dim = player.dimension;
  const b = V.floor(player.location);
  for (let x = -r; x <= r; x++)
    for (let y = -1; y <= 1; y++)
      for (let z = -r; z <= r; z++) {
        try {
          const blk = dim.getBlock({ x: b.x + x, y: b.y + y, z: b.z + z });
          if (blk && re.test(blk.typeId)) return true;
        } catch {}
      }
  return false;
}

export function envMult(player) {
  const notes = [];
  let m = 1;
  const st = meditating.get(player.id);
  if (st?.vein) {
    m *= 2;
    notes.push("§bSpirit vein x2");
  }
  const arr = arraysNear("qi_gathering_array", player.dimension.id, player.location, 8).length;
  if (arr > 0) {
    const f = 1 + 0.6 * Math.min(arr, 3);
    m *= f;
    notes.push(`§bQi Gathering Array x${f.toFixed(1)}`);
  }
  if (st?.cushion) {
    m *= 1.3;
    notes.push("§eMeditation cushion x1.3");
  }
  if (isNight() && world.getMoonPhase() === MoonPhase.FullMoon) {
    m *= 1.3;
    notes.push("§fFull moon x1.3");
  }
  if (st?.fox) {
    m *= 1.1;
    notes.push("§fSpirit fox companion x1.1");
  }
  if (D.get(player, "sect") === "heavenly_secrets") {
    m *= 1.15;
    notes.push("§eHeavenly Secrets Pavilion x1.15");
  }
  if (D.get(player, "deviation") > 0) {
    m *= 0.5;
    notes.push("§4Qi deviation x0.5");
  }
  if (D.get(player, "tox") > 150) {
    m *= 0.8;
    notes.push("§2Pill toxicity x0.8");
  }
  return { mult: m, notes };
}

export function cultivationRate(player) {
  const { R } = realmOf(player);
  const root = rootMult(player);
  const man = manualMult(player);
  const env = envMult(player);
  return {
    rate: R.gain * root * man.mult * env.mult,
    root,
    manual: man.mult,
    env: env.mult,
    notes: [...man.notes, ...env.notes],
  };
}

/** Seconds-of-meditation equivalent, used by pills and spirit stones. */
export function meditationSeconds(player, seconds) {
  const { R } = realmOf(player);
  return R.gain * rootMult(player) * Math.max(1, manualOf(player)?.mult ?? 1) * seconds;
}

// ------------------------------------------------------------------ exp & stages
const bottleneckNotified = new Set();

export function gainExp(player, amount, silent = false) {
  if (amount <= 0) return;
  let { i, R, stage } = realmOf(player);
  if (i >= LAST_REALM) return;
  let exp = D.get(player, "exp") + amount;
  for (;;) {
    const need = expNeeded(i, stage);
    if (exp < need) break;
    if (stage < R.stages.length - 1) {
      exp -= need;
      stage++;
      D.set(player, "stage", stage);
      minorBreakthrough(player);
    } else if (i === 0) {
      // Mortal -> Qi Condensation happens naturally once qi is sensed
      exp = 0;
      D.set(player, "exp", 0);
      advanceRealm(player);
      return;
    } else {
      exp = need;
      if (!bottleneckNotified.has(player.id) && !silent) {
        bottleneckNotified.add(player.id);
        title(player, "§6Bottleneck", "§7Open the Cultivation Codex to attempt a breakthrough");
        say(
          player,
          `§6[Cultivation] §7You have reached the peak of ${R.name}. Your qi presses against the bottleneck. Attempt a §ebreakthrough§7 from the Cultivation Codex.`,
        );
        sound(player, "block.bell.hit");
      }
      break;
    }
  }
  D.set(player, "exp", exp);
}

function minorBreakthrough(player) {
  const { R, stageName } = realmOf(player);
  D.set(player, "qi", maxQi(player));
  title(player, `${R.color}${R.name}`, `§7${stageName}`, { fadeInDuration: 5, stayDuration: 30, fadeOutDuration: 10 });
  sound(player, "random.levelup");
  ring(player.dimension, player.location, 1.5, "xian:spark", primaryColor(player), 20, 0.2);
  applyStats(player);
  emit("stage", player);
}

export function advanceRealm(player) {
  const from = realmOf(player);
  const ni = from.i + 1;
  D.set(player, "realm", ni);
  D.set(player, "stage", 0);
  D.set(player, "exp", 0);
  D.set(player, "btBonus", 0);
  D.set(player, "tribWard", false);
  D.add(player, "breakthroughs", 1);
  bottleneckNotified.delete(player.id);
  const R = REALMS[ni];
  D.set(player, "qi", maxQi(player));
  applyStats(player);
  system.runTimeout(() => {
    if (!player.isValid) return;
    heal(player, 1000);
  }, 2);
  const dim = player.dimension;
  const c = player.location;
  for (let k = 0; k < 4; k++) {
    system.runTimeout(() => ring(dim, c, 1 + k * 1.5, "xian:rune", primaryColor(player), 24, 0.1 + k * 0.4), k * 4);
  }
  particle(dim, "minecraft:totem_particle", V.add(c, { x: 0, y: 1, z: 0 }));
  sound(player, "random.totem");
  if (ni === LAST_REALM) {
    title(player, "§c§lASCENSION", "§6You have become a True Immortal!");
    broadcast(
      `§6§l[Heavens] §r§eThe nine heavens open their gates. §f${player.name}§e has ascended as a §c§lTrue Immortal§r§e!`,
    );
  } else {
    title(player, `${R.color}${R.name}`, "§7Breakthrough!");
    broadcast(
      `§6[Heavens] §7Heaven and earth tremble... §f${player.name}§7 has broken through to ${R.color}${R.name}§7!`,
    );
  }
  emit("realm", player, ni);
}

// ------------------------------------------------------------------ body cultivation
export function bodyTier(player) {
  const e = D.get(player, "bodyExp");
  let t = 0;
  for (let k = 0; k < BODY_TIERS.length; k++) if (e >= BODY_TIERS[k].exp) t = k;
  return t;
}

export function addBodyExp(player, amount) {
  const before = bodyTier(player);
  D.add(player, "bodyExp", amount);
  const after = bodyTier(player);
  if (after > before) {
    const T = BODY_TIERS[after];
    title(player, "§6Body Tempering", `§e${T.name}`);
    say(player, `§6[Body] §7Your flesh and bones are reforged: §e${T.name}§7.`);
    sound(player, "random.anvil_use");
    applyStats(player);
  }
}

// ------------------------------------------------------------------ permanent stats
const LONG = 20000000;

function want(player, id, amp) {
  let cur;
  try {
    cur = player.getEffect(id);
  } catch {}
  const ours = cur && cur.duration > 100000;
  if (amp < 0) {
    if (ours) player.removeEffect(id);
    return;
  }
  if (cur && !ours && cur.amplifier >= amp) return; // a stronger potion is active
  if (!cur || cur.amplifier !== amp || !ours) {
    try {
      player.addEffect(id, LONG, { amplifier: amp, showParticles: false });
    } catch {}
  }
}

export function applyStats(player) {
  if (!player.isValid) return;
  const { i, R } = realmOf(player);
  const B = BODY_TIERS[bodyTier(player)];
  const b = R.buffs;
  const hb = b.health_boost + (B.health >= 0 ? B.health + 1 : 0);
  let res = Math.max(b.resistance, B.resistance);
  if (b.resistance >= 0 && B.resistance >= 0) res += 1;
  want(player, "health_boost", hb);
  want(player, "resistance", Math.min(res, 3));
  want(player, "strength", b.strength);
  want(player, "speed", D.get(player, "buffSpeed") ? b.speed : -1);
  want(
    player,
    "jump_boost",
    D.get(player, "buffJump") ? b.jump_boost + (D.get(player, "manual") === "void_wind" ? 1 : 0) : -1,
  );
  want(player, "regeneration", i >= 4 ? 0 : -1);
  const roots = D.roots(player);
  want(player, "fire_resistance", i >= 3 && roots.includes("fire") ? 0 : -1);
  want(player, "water_breathing", i >= 2 && (roots.includes("water") || roots.includes("ice")) ? 0 : -1);
  want(player, "night_vision", D.get(player, "nightVision") && i >= 2 ? 0 : -1);
}

// ------------------------------------------------------------------ meditation
export const meditating = new Map(); // id -> {start, cushion, vein, veinTick, fox}
const sneakStill = new Map();

export function startMeditation(player, cushion = false) {
  if (meditating.has(player.id)) return;
  if (tribulations.has(player.id)) return;
  const st = {
    start: { ...player.location },
    cushion,
    vein: false,
    veinTick: 0,
    fox: false,
    since: system.currentTick,
  };
  meditating.set(player.id, st);
  scanSurroundings(player, st);
  say(player, "§b[Meditation] §7You sit cross-legged and draw in the qi of heaven and earth. §8(move to stop)");
  sound(player, "beacon.activate", { volume: 0.5, pitch: 1.5 });
  emit("meditate_start", player);
}

export function stopMeditation(player, reason) {
  if (!meditating.has(player.id)) return;
  meditating.delete(player.id);
  if (reason) say(player, `§b[Meditation] §7${reason}`);
}

function scanSurroundings(player, st) {
  st.veinTick = system.currentTick;
  const dim = player.dimension;
  const b = V.floor(player.location);
  st.vein = false;
  try {
    outer: for (let x = -5; x <= 5; x++)
      for (let y = -5; y <= 5; y++)
        for (let z = -5; z <= 5; z++) {
          const blk = dim.getBlock({ x: b.x + x, y: b.y + y, z: b.z + z });
          if (blk?.typeId === "xian:spirit_vein") {
            st.vein = true;
            break outer;
          }
        }
  } catch {}
  try {
    st.fox = dim
      .getEntities({ type: "xian:spirit_fox", location: player.location, maxDistance: 12 })
      .some((f) => f.getDynamicProperty("xian:owner") === player.id);
  } catch {
    st.fox = false;
  }
}

/** Every 5 ticks: detect "sneak still with empty hand / codex" to begin meditating. */
export function sneakDetectTick(player, held) {
  if (meditating.has(player.id)) return;
  const ok = player.isSneaking && player.isOnGround && (!held || held.typeId === "xian:cultivation_codex");
  const v = player.getVelocity();
  if (ok && Math.hypot(v.x, v.z) < 0.01) {
    const n = (sneakStill.get(player.id) ?? 0) + 5;
    sneakStill.set(player.id, n);
    if (n === 25)
      particle(player.dimension, "xian:aura", V.add(player.location, { x: 0, y: 1, z: 0 }), primaryColor(player));
    if (n >= 50) {
      sneakStill.delete(player.id);
      startMeditation(player, onCushion(player));
    }
  } else sneakStill.delete(player.id);
}

export function onCushion(player) {
  try {
    const b = player.dimension.getBlock(V.floor(player.location));
    return b?.typeId === "xian:meditation_cushion";
  } catch {
    return false;
  }
}

/** Every second. */
export function meditationSecond(player) {
  const st = meditating.get(player.id);
  if (!st) return;
  if (V.dist(player.location, st.start) > 0.7) return stopMeditation(player, "You open your eyes and stand.");
  if (system.currentTick - st.veinTick > 600) scanSurroundings(player, st);
  const { rate } = cultivationRate(player);
  gainExp(player, rate);
  D.add(player, "medTime", 1);
  const mq = maxQi(player);
  D.set(player, "qi", Math.min(mq, D.get(player, "qi") + mq * 0.05));
  const man = D.get(player, "manual");
  if (man === "azure_wood") heal(player, 1);
  if (man === "golden_vajra") addBodyExp(player, Math.max(1, rate * 0.05));
  if (man === "heavenly_devouring" && chance(0.003)) qiDeviation(player, 120, "The demonic art backlashes!");
  // visuals
  const c = primaryColor(player);
  const loc = player.location;
  for (let k = 0; k < 4; k++) {
    const a = Math.random() * Math.PI * 2;
    const r = rand(0.6, 1.4);
    particle(
      player.dimension,
      "xian:aura",
      { x: loc.x + Math.cos(a) * r, y: loc.y + rand(0, 1.6), z: loc.z + Math.sin(a) * r },
      c,
    );
  }
  if ((system.currentTick - st.since) % 60 < 20) ring(player.dimension, loc, 1.2, "xian:orb", c, 12, 0.05);
  emit("meditate_second", player);
}

export function qiDeviation(player, seconds, why) {
  D.set(player, "deviation", Math.max(D.get(player, "deviation"), seconds));
  effect(player, "nausea", 8, 0);
  effect(player, "weakness", seconds, 0);
  hurt(player, maxHealth(player) * 0.15, undefined, EntityDamageCause.magic);
  stopMeditation(player);
  title(player, "§4Qi Deviation!", `§7${why}`);
  say(
    player,
    `§4[Qi Deviation] §7${why} Cultivation speed halved for ${Math.round(seconds)}s. A §fClear Heart Pill§7 cures it.`,
  );
}

// ------------------------------------------------------------------ breakthroughs
export function breakthroughChance(player) {
  const { next, i } = realmOf(player);
  if (!next) return { total: 0, parts: [] };
  /** @type {[string, number][]} */
  const parts = [[`Base (${next.name})`, next.chance]];
  const pill = D.get(player, "btBonus");
  if (pill) parts.push(["Breakthrough pill", pill]);
  const dao = Math.min(20, D.get(player, "dao") * 0.5);
  if (dao) parts.push(["Dao comprehension", dao]);
  if (D.get(player, "sect") === "heavenly_secrets") parts.push(["Heavenly Secrets Pavilion", 5]);
  const karma = D.get(player, "karma");
  if (karma >= 50) parts.push(["Good karma", 5]);
  if (karma <= -50 && D.get(player, "sect") !== "blood_moon") parts.push(["Heavy karma", -10]);
  if (D.get(player, "tox") > 100) parts.push(["Pill toxicity", -10]);
  const total = clamp(
    parts.reduce((s, p) => s + p[1], 0),
    5,
    100,
  );
  return { total, parts, i };
}

export function attemptBreakthrough(player) {
  if (!isBottleneck(player)) return say(player, "§7You are not yet at the peak of your realm.");
  if (tribulations.has(player.id)) return;
  if (D.get(player, "deviation") > 0) return say(player, "§4Your qi is in chaos. Cure your qi deviation first.");
  stopMeditation(player);
  const { total } = breakthroughChance(player);
  const { next } = realmOf(player);
  title(player, "§eBreakthrough...", `§7Chance: ${Math.round(total)}%`);
  sound(player, "beacon.power", { pitch: 0.8 });
  const dim = player.dimension;
  for (let k = 0; k < 6; k++) {
    system.runTimeout(() => {
      if (player.isValid) ring(dim, player.location, 2.5 - k * 0.35, "xian:orb", primaryColor(player), 20, k * 0.3);
    }, k * 8);
  }
  system.runTimeout(() => {
    if (!player.isValid) return;
    if (Math.random() * 100 < total) {
      if (next.tribulation > 0) startTribulation(player);
      else advanceRealm(player);
    } else failBreakthrough(player, "Your meridians could not bear the surge of qi.");
  }, 60);
}

export function failBreakthrough(player, why) {
  D.set(player, "exp", Math.floor(D.get(player, "exp") * 0.7));
  D.add(player, "dao", 1);
  D.set(player, "btBonus", 0);
  bottleneckNotified.delete(player.id);
  qiDeviation(player, 300, why);
  title(player, "§cBreakthrough Failed", `§7${why}`);
  sound(player, "random.break");
  say(player, "§c[Breakthrough] §7Failure is also a lesson: §e+1 Dao Comprehension§7.");
}

// ------------------------------------------------------------------ heavenly tribulation
export const tribulations = new Map(); // id -> state

export function startTribulation(player) {
  const { next, i } = realmOf(player);
  let mult = 1;
  const notes = [];
  if (D.get(player, "manual") === "nine_heavens_thunder") {
    mult *= 0.5;
    notes.push("Nine Heavens Thunder Art");
  }
  if (D.get(player, "tribWard")) {
    mult *= 0.5;
    notes.push("Void Tribulation Pill");
  }
  if (countItem(player, "xian:lightning_rod_talisman") > 0) {
    removeItem(player, "xian:lightning_rod_talisman", 1);
    mult *= 0.5;
    notes.push("Tribulation Warding Talisman");
  }
  const karma = D.get(player, "karma");
  if (karma <= -50 && D.get(player, "sect") !== "blood_moon") {
    mult *= 1.5;
    notes.push("§cHeavy karma");
  } else if (karma >= 50) {
    mult *= 0.85;
    notes.push("Good karma");
  }
  tribulations.set(player.id, {
    waves: next.tribulation,
    wave: 0,
    next: system.currentTick + 100,
    mult,
    demon: i + 1 >= 4,
    demonId: undefined,
    deadline: 0,
    phase: "lightning",
  });
  try {
    if (player.dimension.id === "minecraft:overworld") player.dimension.setWeather(WeatherType.Thunder, 2400);
  } catch {}
  title(player, "§5§lHeavenly Tribulation", `§7Survive ${next.tribulation} waves of tribulation lightning!`, {
    fadeInDuration: 10,
    stayDuration: 80,
    fadeOutDuration: 20,
  });
  if (notes.length) say(player, `§5[Tribulation] §7Damage modifiers: ${notes.join("§7, ")} §7(x${mult.toFixed(2)})`);
  say(
    player,
    "§5[Tribulation] §7Dark clouds gather above you. Heal between strikes - pills, talismans and armor will save your life.",
  );
  broadcast(
    `§5[Heavens] §7Tribulation clouds gather over §f${player.name}§7, who seeks to enter ${next.color}${next.name}§7!`,
  );
  sound(player, "ambient.weather.thunder", { volume: 2 });
}

export function tribulationTick() {
  for (const [id, t] of tribulations) {
    const player = world.getAllPlayers().find((p) => p.id === id);
    if (!player) {
      tribulations.delete(id);
      continue;
    }
    const loc = player.location;
    const dim = player.dimension;
    if (t.phase === "lightning") {
      if (system.currentTick % 10 === 0) {
        for (let k = 0; k < 6; k++) {
          particle(
            dim,
            "xian:spark",
            { x: loc.x + rand(-6, 6), y: loc.y + rand(8, 14), z: loc.z + rand(-6, 6) },
            [0.6, 0.4, 1],
          );
        }
      }
      if (system.currentTick < t.next) continue;
      t.wave++;
      try {
        dim.spawnEntity("minecraft:lightning_bolt", loc);
      } catch {}
      for (let y = 0; y < 14; y += 1)
        particle(dim, "xian:lightning", { x: loc.x + rand(-0.3, 0.3), y: loc.y + y, z: loc.z + rand(-0.3, 0.3) });
      const dmg = maxHealth(player) * (0.08 + 0.01 * t.wave) * t.mult;
      hurt(player, dmg, undefined, EntityDamageCause.lightning);
      title(player, `§5Tribulation ${t.wave}/${t.waves}`, "", {
        fadeInDuration: 0,
        stayDuration: 20,
        fadeOutDuration: 5,
      });
      if (t.wave >= t.waves) {
        if (t.demon) {
          t.phase = "demon";
          const spot = V.add(loc, { x: rand(-4, 4), y: 0.5, z: rand(-4, 4) });
          try {
            const d = dim.spawnEntity("xian:heart_demon", spot);
            d.nameTag = `§5${player.name}'s Heart Demon`;
            d.setDynamicProperty("xian:victim", player.id);
            t.demonId = d.id;
          } catch {}
          t.deadline = system.currentTick + 20 * 90;
          title(player, "§5Heart Demon Trial", "§7Slay your heart demon within 90 seconds!");
          say(player, "§5[Tribulation] §7Your inner demons take form. §cDefeat your Heart Demon within 90 seconds!");
        } else tribulationSuccess(player);
      } else t.next = system.currentTick + 50;
    } else if (t.phase === "demon") {
      const demon = t.demonId ? world.getEntity(t.demonId) : undefined;
      if (!demon || !demon.isValid) tribulationSuccess(player);
      else if (system.currentTick > t.deadline) {
        try {
          demon.remove();
        } catch {}
        tribulationFail(player, "Your heart demon devoured your resolve.");
      }
    }
  }
}

function tribulationSuccess(player) {
  tribulations.delete(player.id);
  try {
    if (player.dimension.id === "minecraft:overworld") player.dimension.setWeather(WeatherType.Clear, 6000);
  } catch {}
  say(player, "§5[Tribulation] §aThe clouds part. You have survived the heavenly tribulation!");
  advanceRealm(player);
}

export function tribulationFail(player, why) {
  tribulations.delete(player.id);
  try {
    if (player.isValid && player.dimension.id === "minecraft:overworld")
      player.dimension.setWeather(WeatherType.Clear, 6000);
  } catch {}
  if (player.isValid) failBreakthrough(player, why);
}

// ------------------------------------------------------------------ per-second upkeep
export function secondTick(player) {
  const mq = maxQi(player);
  const qi = D.get(player, "qi");
  if (qi < mq) D.set(player, "qi", Math.min(mq, qi + mq * 0.01 + 0.2));
  const dev = D.get(player, "deviation");
  if (dev > 0) {
    D.set(player, "deviation", Math.max(0, dev - 1));
    if (dev - 1 <= 0) say(player, "§a[Cultivation] §7Your qi settles. The deviation has passed.");
  }
  const tox = D.get(player, "tox");
  if (tox > 0) D.set(player, "tox", Math.max(0, tox - 0.1));
  meditationSecond(player);
}

export function spendQi(player, amount) {
  const qi = D.get(player, "qi");
  if (qi < amount) return false;
  D.set(player, "qi", qi - amount);
  return true;
}

export function onPlayerDeath(player) {
  stopMeditation(player);
  if (tribulations.has(player.id)) tribulationFail(player, "You perished beneath the heavenly lightning.");
  const lost = Math.floor(D.get(player, "exp") * 0.1);
  if (lost > 0) {
    D.set(player, "exp", D.get(player, "exp") - lost);
    say(player, `§c[Cultivation] §7Death shakes your foundation: §c-${fmt(lost)} cultivation§7.`);
  }
  D.set(player, "qi", 0);
}
