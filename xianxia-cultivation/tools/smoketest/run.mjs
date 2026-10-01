// Headless smoke test: loads the built behavior-pack scripts against mocked Minecraft APIs
// and drives the main gameplay paths. Run: node tools/smoketest/run.mjs (after build.py)
import fs from "fs";
import os from "os";
import path from "path";
import { fileURLToPath, pathToFileURL } from "url";

const here = path.dirname(fileURLToPath(import.meta.url));
const scripts = path.join(here, "..", "..", "packs", "Xianxia_Cultivation_BP", "scripts");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "xian-smoke-"));
fs.copyFileSync(path.join(here, "mock-server.mjs"), path.join(tmp, "mock-server.mjs"));
fs.copyFileSync(path.join(here, "mock-server-ui.mjs"), path.join(tmp, "mock-server-ui.mjs"));
for (const f of fs.readdirSync(scripts)) {
  let src = fs.readFileSync(path.join(scripts, f), "utf8");
  src = src.replace(/from "@minecraft\/server"/g, 'from "./mock-server.mjs"').replace(/from "@minecraft\/server-ui"/g, 'from "./mock-server-ui.mjs"');
  fs.writeFileSync(path.join(tmp, f.replace(/\.js$/, ".mjs")), src.replace(/from "\.\/(\w+)\.js"/g, 'from "./$1.mjs"'));
}

const errors = [];
console.warn = (...a) => errors.push(a.join(" "));
process.on("unhandledRejection", (e) => errors.push("unhandled rejection: " + (e?.stack ?? e)));

const M = await import(pathToFileURL(path.join(tmp, "mock-server.mjs")).href);
const UI = await import(pathToFileURL(path.join(tmp, "mock-server-ui.mjs")).href);
await import(pathToFileURL(path.join(tmp, "main.mjs")).href);
const D = await import(pathToFileURL(path.join(tmp, "playerdata.mjs")).href);
const C = await import(pathToFileURL(path.join(tmp, "cultivation.mjs")).href);
const T = await import(pathToFileURL(path.join(tmp, "techniques.mjs")).href);
const DATA = await import(pathToFileURL(path.join(tmp, "data.mjs")).href);

const { world, system, advance, ItemStack } = M;
let failures = 0;
const check = (cond, msg) => {
  if (!cond) {
    failures++;
    console.log("  FAIL:", msg);
  } else console.log("  ok:", msg);
};
const flush = () => new Promise((r) => setTimeout(r, 0));

// --- boot
const registered = {};
system.beforeEvents.startup.fire({
  blockComponentRegistry: { registerCustomComponent: (n, c) => (registered[n] = c) },
  itemComponentRegistry: { registerCustomComponent: () => {} },
});
const p = M.addPlayer("Tester");
world.afterEvents.worldLoad.fire({});
world.afterEvents.playerSpawn.fire({ player: p, initialSpawn: true });
advance(20);
console.log("boot");
check(registered["xian:herb"] && registered["xian:station"], "block custom components registered");
check(D.get(p, "init") === true, "player initialised");
check(D.roots(p).length >= 1, `spirit root rolled (${D.roots(p).join(",")})`);
check(p.container.slots.some((s) => s?.typeId === "xian:cultivation_codex"), "starter kit given");

const use = (typeId, sneaking = false) => {
  p.container.setItem(0, new ItemStack(typeId, 1));
  p.selectedSlotIndex = 0;
  p.isSneaking = sneaking;
  world.afterEvents.itemUse.fire({ source: p, itemStack: new ItemStack(typeId, 1) });
  advance(2);
  p.isSneaking = false;
};

// --- roots, manuals, scrolls
console.log("items");
use("xian:root_testing_stone");
check(D.get(p, "rootsKnown") === true, "root testing stone reveals root");
use("xian:manual_basic_breathing");
check(D.get(p, "manual") === "basic_breathing", "reading a jade slip sets the manual");
use("xian:scroll_qi_bolt");
check(T.learned(p).includes("qi_bolt"), "technique scroll teaches technique");

// --- meditation via sneaking with empty hand
console.log("meditation");
p.container.setItem(0, undefined);
p.isSneaking = true;
advance(60);
check(C.meditating.has(p.id), "sneaking still starts meditation");
p.isSneaking = false;
advance(20 * 120);
check(C.realmOf(p).i >= 1, `meditation advanced realm to ${C.realmOf(p).R.name} ${C.realmOf(p).stageName}`);

// --- stats
C.applyStats(p);
check(p.effects.has("health_boost"), "realm grants health boost");

// --- bottleneck & breakthrough (forced success)
console.log("breakthrough");
D.set(p, "realm", 1);
D.set(p, "stage", 8);
D.set(p, "exp", C.expNeeded(1, 8));
check(C.isBottleneck(p), "bottleneck detected at peak");
const rnd = Math.random;
Math.random = () => 0.01;
C.attemptBreakthrough(p);
advance(80);
check(C.realmOf(p).i === 2, "breakthrough into Foundation Establishment");

// --- tribulation into core formation
console.log("tribulation");
D.set(p, "stage", 3);
D.set(p, "exp", C.expNeeded(2, 3));
p.health.currentValue = p.health.effectiveMax;
C.attemptBreakthrough(p);
advance(80);
check(C.tribulations.has(p.id), "tribulation started for Core Formation");
for (let k = 0; k < 20 && C.tribulations.has(p.id); k++) {
  p.health.currentValue = p.health.effectiveMax;
  advance(60);
}
check(C.realmOf(p).i === 3, "survived tribulation -> Core Formation");
check(M.log.some((l) => l[0] === "spawn" && l[1] === "minecraft:lightning_bolt"), "tribulation lightning spawned");

// --- heart demon phase (into Nascent Soul)
D.set(p, "stage", 3);
D.set(p, "exp", C.expNeeded(3, 3));
C.attemptBreakthrough(p);
for (let k = 0; k < 20; k++) {
  p.health.currentValue = p.health.effectiveMax;
  advance(60);
}
const demon = world.getDimension("overworld").entities.find((e) => e.typeId === "xian:heart_demon" && e.isValid);
check(!!demon, "heart demon spawned");
if (demon) demon.isValid = false;
advance(5);
check(C.realmOf(p).i === 4, "slaying the heart demon completes Nascent Soul breakthrough");
Math.random = rnd;

// --- techniques: learn everything and cast each against a mob
console.log("techniques");
D.set(p, "realm", 9);
D.set(p, "qi", 1e9);
for (const t of DATA.TECHNIQUES) T.learn(p, t.id);
const wolf = M.addMob("xian:demonic_wolf", { x: 0, y: 64, z: 4 }, ["spirit_beast", "monster", "mob"]);
for (const t of DATA.TECHNIQUES) {
  D.set(p, "qi", 1e9);
  p.health.currentValue = p.health.effectiveMax;
  wolf.health.currentValue = 1000;
  const before = errors.length;
  T.cast(p, t.id);
  advance(220);
  check(errors.length === before, `cast ${t.id} without errors`);
}
check(M.log.some((l) => l[0] === "damage" && l[1] === "xian:demonic_wolf"), "techniques damaged the wolf");

// --- pills & food
console.log("pills");
for (const pill of [...DATA.PILLS.map((x) => x.id), "spirit_fruit", "immortal_peach"]) {
  const before = errors.length;
  let threw = null;
  try {
    world.afterEvents.itemCompleteUse.fire({ source: p, itemStack: new ItemStack("xian:" + pill, 1) });
  } catch (e) {
    threw = e;
  }
  advance(5);
  check(!threw && errors.length === before, `consume ${pill}${threw ? " -> " + threw.message : ""}`);
}

// --- talismans, stones and special items
console.log("special items");
for (const id of [...DATA.TALISMANS.map((t) => t.id), "low_spirit_stone", "mid_spirit_stone", "flying_sword", "dragon_summoning_pearl", "beast_tide_horn"]) {
  let threw = null;
  try {
    use("xian:" + id);
  } catch (e) {
    threw = e;
  }
  check(!threw, `use ${id}${threw ? " -> " + threw.message : ""}`);
}
// sword flight runs while holding the sword
p.container.setItem(0, new ItemStack("xian:flying_sword", 1));
advance(40);
check(errors.length === 0, "sword flight ticks cleanly");

// --- beast tide progresses and rewards
console.log("beast tide");
for (let k = 0; k < 6; k++) {
  advance(140);
  for (const e of world.getDimension("overworld").entities) if ([...e.tags].some((t) => t.startsWith("xian_tide"))) e.isValid = false;
}
check(M.log.some((l) => l[0] === "broadcast" && l[1].includes("repelled")), "beast tide completes with rewards");

// --- kills, karma and sect missions
console.log("sects & kills");
UI.answers.push(0, 0); // join first sect, confirm
use("xian:sect_token");
await flush();
await flush();
advance(2);
check(D.get(p, "sect") === DATA.SECTS[0].id, "joined a sect through the UI");
D.setJSON(p, "mission", { id: "cull_beasts", progress: 0 });
for (let k = 0; k < 15; k++) {
  const m = M.addMob("minecraft:zombie", { x: 2, y: 64, z: 2 }, ["zombie", "monster", "mob"]);
  world.afterEvents.entityDie.fire({ deadEntity: m, damageSource: { damagingEntity: p, cause: "entityAttack" } });
}
check(D.get(p, "contrib") >= 60, "mission completed by kills -> contribution awarded");
const v = M.addMob("minecraft:villager_v2", { x: 2, y: 64, z: 2 }, ["villager", "mob"]);
const karma = D.get(p, "karma");
world.afterEvents.entityDie.fire({ deadEntity: v, damageSource: { damagingEntity: p, cause: "entityAttack" } });
check(D.get(p, "karma") < karma, "killing a villager lowers karma");

// --- every codex menu opens (answer each top-level button once)
console.log("menus");
const codexButtons = () => {
  UI.shown.length = 0;
  use("xian:cultivation_codex");
};
for (let b = 0; b < 9; b++) {
  UI.answers.length = 0;
  UI.answers.push(b);
  codexButtons();
  await flush();
  await flush();
  advance(2);
}
check(errors.length === 0, "codex menus open without errors");
use("xian:martial_seal", true);
await flush();
check(UI.shown.some((f) => f._title === "Martial Techniques"), "sneak-use seal opens technique picker");

// --- stations
console.log("stations");
const blk = { typeId: "xian:alchemy_furnace", location: { x: 5, y: 64, z: 5 }, dimension: world.getDimension("overworld") };
for (const [id, n] of Object.entries(DATA.PILLS[0].ingredients)) p.container.addItem(new ItemStack("xian:" + id, n));
UI.answers.push(0, 0); // first recipe, confirm
registered["xian:station"].onPlayerInteract({ block: blk, player: p });
advance(2);
await flush();
await flush();
advance(120);
check(D.get(p, "alchExp") > 0, "alchemy furnace refines a pill batch");
UI.answers.push(["Sect Gate"]);
registered["xian:station"].onPlayerInteract({ block: { ...blk, typeId: "xian:teleport_array" }, player: p });
advance(2);
await flush();
check(errors.length === 0, "teleport array menu works");

// --- death penalty
console.log("death");
D.set(p, "exp", 1000);
world.afterEvents.entityDie.fire({ deadEntity: p, damageSource: { cause: "fall" } });
check(D.get(p, "exp") === 900, "death costs 10% cultivation");

advance(200);
console.log(`\n${errors.length} runtime errors`);
for (const e of errors.slice(0, 20)) console.log("  ERR", e);
console.log(failures ? `${failures} check(s) FAILED` : "all checks passed");
fs.rmSync(tmp, { recursive: true, force: true });
process.exit(failures || errors.length ? 1 : 0);
