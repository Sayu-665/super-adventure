// Xianxia Cultivation - Script entry point.
import { world, system, WeatherType } from "@minecraft/server";
import * as D from "./playerdata.js";
import {
  rollRoots,
  applyStats,
  secondTick,
  sneakDetectTick,
  tribulationTick,
  meditating,
  realmOf,
  expNeeded,
  maxQi,
  cultivationRate,
  weather,
  LAST_REALM,
} from "./cultivation.js";
import { registerItems, flightTick, flying } from "./items.js";
import { registerBlockComponents, registerWorld } from "./world.js";
import { fmt, say, title, giveItem, actionbarFree, heldItem } from "./util.js";

system.beforeEvents.startup.subscribe(({ blockComponentRegistry }) => {
  registerBlockComponents(blockComponentRegistry);
});

/** @type {[string, number][]} */
const STARTER = [
  ["xian:cultivation_codex", 1],
  ["xian:martial_seal", 1],
  ["xian:root_testing_stone", 1],
  ["xian:manual_basic_breathing", 1],
  ["xian:scroll_qi_bolt", 1],
  ["xian:qi_gathering_pill", 2],
  ["xian:healing_pill", 2],
  ["xian:low_spirit_stone", 8],
];

function initPlayer(p) {
  if (world.getDynamicProperty("xian:owner") === undefined) world.setDynamicProperty("xian:owner", p.id);
  if (D.get(p, "init")) return;
  D.set(p, "init", true);
  D.set(p, "roots", rollRoots().join(","));
  for (const [id, n] of STARTER) giveItem(p, id, n);
  title(p, "§6The Path of Immortality", "§7Your Dao begins here");
  say(p, "§6[Xianxia] §7Welcome, mortal. Heaven and earth are full of spiritual qi - learn to draw it in.");
  say(p, "§7  1. Use the §eSpirit Root Testing Stone§7 to discover your talent.");
  say(p, "§7  2. Read the §eJade Slip§7 to learn a cultivation manual.");
  say(p, "§7  3. Sneak and stand still with an empty hand to §bmeditate§7.");
  say(p, "§7  4. Open the §eCultivation Codex§7 for your status, breakthroughs, sects and the full guide.");
}

function hud(p) {
  const mode = D.get(p, "hud");
  if (mode === 0 || !actionbarFree(p)) return;
  const med = meditating.has(p.id);
  const fly = flying.has(p.id);
  if (mode === 1 && !med && !fly) {
    const h = heldItem(p)?.typeId ?? "";
    if (!/^xian:(cultivation_codex|martial_seal|flying_sword|.*spirit_stone)$/.test(h)) return;
  }
  const { i, R, stageName, stage } = realmOf(p);
  const qi = D.get(p, "qi");
  const parts = [`${R.color}${R.name}${R.stages.length > 1 ? " §7" + stageName : ""}`];
  if (i < LAST_REALM) parts.push(`§aCult ${Math.min(100, Math.floor((D.get(p, "exp") / expNeeded(i, stage)) * 100))}%`);
  parts.push(`§bQi ${fmt(qi)}/${fmt(maxQi(p))}`);
  if (med) parts.push(`§3Meditating +${cultivationRate(p).rate.toFixed(1)}/s`);
  if (fly) parts.push("§fSword Flight");
  if (D.get(p, "deviation") > 0) parts.push("§4Deviation");
  try {
    p.onScreenDisplay.setActionBar(parts.join(" §8| "));
  } catch {}
}

function safe(fn) {
  return () => {
    try {
      fn();
    } catch (e) {
      console.warn(`[xian] ${e}\n${e?.stack ?? ""}`);
    }
  };
}

world.afterEvents.worldLoad.subscribe(() => {
  registerItems();
  registerWorld();

  world.afterEvents.playerSpawn.subscribe(({ player, initialSpawn }) => {
    if (initialSpawn) initPlayer(player);
    system.runTimeout(() => player.isValid && applyStats(player), 10);
  });
  world.afterEvents.playerLeave.subscribe(({ playerId }) => {
    meditating.delete(playerId);
    flying.delete(playerId);
  });
  world.afterEvents.weatherChange.subscribe(({ dimension, newWeather }) => {
    if (dimension !== "minecraft:overworld" && dimension !== "overworld") return;
    weather.thunder = newWeather === WeatherType.Thunder;
    weather.rain = newWeather !== WeatherType.Clear;
  });

  // players already online when the script (re)loads
  for (const p of world.getAllPlayers()) {
    initPlayer(p);
    applyStats(p);
  }

  system.runInterval(
    safe(() => {
      flightTick();
      tribulationTick();
    }),
    1,
  );
  system.runInterval(
    safe(() => {
      for (const p of world.getAllPlayers()) sneakDetectTick(p, heldItem(p));
    }),
    5,
  );
  system.runInterval(
    safe(() => {
      for (const p of world.getAllPlayers()) hud(p);
    }),
    10,
  );
  system.runInterval(
    safe(() => {
      for (const p of world.getAllPlayers()) secondTick(p);
    }),
    20,
  );
  system.runInterval(
    safe(() => {
      for (const p of world.getAllPlayers()) applyStats(p);
    }),
    60,
  );
});
