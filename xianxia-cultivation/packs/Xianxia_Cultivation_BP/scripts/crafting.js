// Alchemy (pill refining) and artifact refining stations.
import { system } from "@minecraft/server";
import { ActionFormData, MessageFormData } from "@minecraft/server-ui";
import { PILLS, REFINING, ICONS } from "./data.js";
import * as D from "./playerdata.js";
import { emit } from "./bus.js";
import {
  V,
  rand,
  randInt,
  clamp,
  say,
  title,
  sound,
  soundAt,
  particle,
  ring,
  countItem,
  removeItem,
  giveItem,
  itemName,
} from "./util.js";

const LEVELS = [0, 60, 200, 500, 1100, 2200, 4200, 7500, 13000];
const ORD = ["1st", "2nd", "3rd", "4th", "5th", "6th", "7th", "8th", "9th"];

export function craftLevel(p, kind) {
  const e = D.get(p, kind === "alchemy" ? "alchExp" : "refineExp");
  let l = 1;
  LEVELS.forEach((t, k) => {
    if (e >= t) l = k + 1;
  });
  return l;
}

export function craftTitle(p, kind) {
  const l = craftLevel(p, kind);
  const noun = kind === "alchemy" ? "Alchemist" : "Artificer";
  return l >= 9 ? `${noun} Grandmaster` : `${ORD[l - 1]} Grade ${noun}`;
}

const full = (id) => (id.includes(":") ? id : "xian:" + id);
const busy = new Set();

const RECIPES = {
  alchemy: PILLS.map((p) => ({
    out: p.id,
    level: p.level,
    chance: p.chance,
    ingredients: p.ingredients,
    desc: p.desc,
  })),
  refining: REFINING.map((r) => ({ ...r, desc: "" })),
};

function chanceFor(p, kind, r) {
  const lvl = craftLevel(p, kind);
  let c = r.chance + (lvl - r.level) * 6;
  if (lvl < r.level) c -= (r.level - lvl) * 9;
  if (D.get(p, "sect") === "myriad_pill") c += 15;
  const roots = D.roots(p);
  if (kind === "alchemy" && roots.includes("fire")) c += 10;
  if (kind === "refining" && (roots.includes("metal") || roots.includes("fire"))) c += 8;
  return clamp(c, 5, 98);
}

function hasAll(p, r) {
  return Object.entries(r.ingredients).every(([id, n]) => countItem(p, full(id)) >= n);
}

export function openStation(p, kind, block) {
  if (busy.has(p.id)) return say(p, "§7You are already refining.");
  const list = RECIPES[kind];
  const lvl = craftLevel(p, kind);
  const f = new ActionFormData()
    .title(kind === "alchemy" ? "Alchemy Furnace" : "Artifact Refining Forge")
    .body(
      `§7${craftTitle(p, kind)} §8(exp ${Math.floor(D.get(p, kind === "alchemy" ? "alchExp" : "refineExp"))})\n§7Green = you have the ingredients.`,
    );
  for (const r of list) {
    const ok = hasAll(p, r);
    f.button(`${ok ? "§2" : "§8"}${itemName(full(r.out))}\n§8Lv${r.level} | ${chanceFor(p, kind, r)}%`, ICONS[r.out]);
  }
  f.show(p).then((res) => {
    if (res.canceled || res.selection === undefined) return;
    const r = list[res.selection];
    const lines = Object.entries(r.ingredients).map(([id, n]) => {
      const have = countItem(p, full(id));
      return `${have >= n ? "§a" : "§c"}${itemName(full(id))} §7${Math.min(have, n)}/${n}`;
    });
    new MessageFormData()
      .title(itemName(full(r.out)))
      .body(
        `${r.desc ? "§f" + r.desc + "\n\n" : ""}§7Required level: ${r.level} (you: ${lvl})\n§7Success chance: §e${chanceFor(p, kind, r)}%\n\n§7Ingredients:\n${lines.join("\n")}`,
      )
      .button1(kind === "alchemy" ? "Refine Pills" : "Refine Artifact")
      .button2("Back")
      .show(p)
      .then((c) => {
        if (c.selection === 0) start(p, kind, r, block);
        else openStation(p, kind, block);
      });
  });
}

function start(p, kind, r, block) {
  if (!hasAll(p, r)) return say(p, "§cYou are missing ingredients.");
  for (const [id, n] of Object.entries(r.ingredients)) removeItem(p, full(id), n);
  busy.add(p.id);
  const dim = block.dimension;
  const at = V.add(block.location, { x: 0.5, y: 1.1, z: 0.5 });
  const duration = 100;
  let t = 0;
  sound(p, kind === "alchemy" ? "fire.ignite" : "random.anvil_use");
  const h = system.runInterval(() => {
    t += 5;
    for (let k = 0; k < 3; k++) {
      particle(
        dim,
        kind === "alchemy" ? "xian:flame" : "xian:spark",
        { x: at.x + rand(-0.4, 0.4), y: at.y + rand(0, 0.5), z: at.z + rand(-0.4, 0.4) },
        [1, 0.6, 0.2],
      );
    }
    if (t % 20 === 0) {
      ring(dim, at, 0.8, "xian:rune", kind === "alchemy" ? [1, 0.5, 0.2] : [0.6, 0.8, 1], 10, 0);
      soundAt(dim, at, kind === "alchemy" ? "bubble.pop" : "random.anvil_land", { volume: 0.4, pitch: 1.2 });
    }
    const n = Math.round((t / duration) * 10);
    try {
      p.onScreenDisplay.setActionBar(`§6Refining ${itemName(full(r.out))} §e${"▮".repeat(n)}§8${"▯".repeat(10 - n)}`);
    } catch {}
    if (t >= duration) {
      system.clearRun(h);
      busy.delete(p.id);
      if (p.isValid) finish(p, kind, r, dim, at);
    }
  }, 5);
}

function finish(p, kind, r, dim, at) {
  const c = chanceFor(p, kind, r);
  const expKey = kind === "alchemy" ? "alchExp" : "refineExp";
  const before = craftLevel(p, kind);
  if (Math.random() * 100 < c) {
    let count = 1;
    if (kind === "alchemy") {
      count = 1 + Math.floor((craftLevel(p, kind) - 1) / 3) + randInt(0, 1);
      if (D.get(p, "sect") === "myriad_pill") count++;
    }
    giveItem(p, full(r.out), count);
    D.add(p, expKey, r.level * 12);
    title(p, "§aSuccess!", `§7${itemName(full(r.out))}${count > 1 ? " x" + count : ""}`);
    sound(p, "random.levelup", { pitch: 1.5 });
    particle(dim, "minecraft:totem_particle", at);
    if (kind === "alchemy") emit("alchemy_success", p);
  } else {
    D.add(p, expKey, r.level * 5);
    title(p, "§cFailure", kind === "alchemy" ? "§7The pill cauldron cracks..." : "§7The artifact shatters...");
    if (kind === "alchemy") giveItem(p, "xian:pill_residue", 1);
    sound(p, "random.break");
    particle(dim, "minecraft:large_explosion", at);
    if (Math.random() < 0.2) {
      try {
        dim.createExplosion(at, 1.5, { breaksBlocks: false, causesFire: false });
      } catch {}
    }
  }
  const after = craftLevel(p, kind);
  if (after > before)
    say(p, `§6[${kind === "alchemy" ? "Alchemy" : "Refining"}] §7You are now a §e${craftTitle(p, kind)}§7!`);
}
