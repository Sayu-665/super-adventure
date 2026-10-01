// The Cultivation Codex: status, breakthroughs, manuals, techniques, sect, guide, settings.
import { world, GameMode } from "@minecraft/server";
import { ActionFormData, MessageFormData, ModalFormData } from "@minecraft/server-ui";
import {
  REALMS,
  BODY_TIERS,
  MANUALS,
  TECHNIQUES,
  ELEMENTS,
  GRADE_COLORS,
  ICONS,
  PILLS,
  TALISMANS,
  WEAPONS,
  HERBS,
  SPECIAL_ITEMS,
} from "./data.js";
import * as D from "./playerdata.js";
import {
  realmOf,
  realmTitle,
  expNeeded,
  maxQi,
  rootText,
  cultivationRate,
  bodyTier,
  isBottleneck,
  breakthroughChance,
  attemptBreakthrough,
  startMeditation,
  stopMeditation,
  meditating,
  onCushion,
  manualOf,
  applyStats,
  rollRoots,
  LAST_REALM,
  powerMult,
  hasElement,
} from "./cultivation.js";
import { learned, TECH, cooldownLeft } from "./techniques.js";
import { openSect, sectOf, rankOf } from "./sect.js";
import { craftTitle } from "./crafting.js";
import { SECT_RANKS } from "./data.js";
import { fmt, bar, say, sound, giveItem } from "./util.js";

export { openSect };

function karmaLabel(k) {
  if (k >= 50) return `§a${Math.round(k)} (Virtuous)`;
  if (k >= 10) return `§2${Math.round(k)} (Righteous)`;
  if (k > -10) return `§7${Math.round(k)} (Neutral)`;
  if (k > -50) return `§c${Math.round(k)} (Ruthless)`;
  return `§4${Math.round(k)} (Demonic)`;
}

function summary(p) {
  const { i, stage } = realmOf(p);
  const need = expNeeded(i, stage);
  const exp = D.get(p, "exp");
  const mq = maxQi(p);
  const man = manualOf(p);
  return [
    `§7Realm: ${realmTitle(p)}`,
    i < LAST_REALM
      ? `§7Cultivation: ${bar(exp / need, 24)} §f${fmt(exp)}/${fmt(need)}`
      : "§6You stand at the apex of the Dao.",
    `§7Qi: ${bar(D.get(p, "qi") / mq, 24, "§b")} §f${fmt(D.get(p, "qi"))}/${fmt(mq)}`,
    `§7Spirit Root: ${rootText(p)}`,
    `§7Manual: ${man ? GRADE_COLORS[man.grade] + man.name : "§8none"}`,
    `§7Body: §e${BODY_TIERS[bodyTier(p)].name}`,
  ].join("\n");
}

export function openCodex(p) {
  const f = new ActionFormData().title("Cultivation Codex").body(summary(p));
  const actions = [];
  const btn = (text, icon, fn) => {
    f.button(text, icon);
    actions.push(fn);
  };
  btn("Cultivation Status", ICONS.cultivation_codex, () => status(p));
  if (meditating.has(p.id))
    btn("Stop Meditating", ICONS.meditation_cushion, () => stopMeditation(p, "You end your meditation."));
  else btn("Meditate", ICONS.meditation_cushion, () => startMeditation(p, onCushion(p)));
  if (isBottleneck(p)) btn("§6Attempt Breakthrough!", ICONS.foundation_pill, () => breakthroughMenu(p));
  btn("Cultivation Manuals", ICONS.manual_basic_breathing, () => manualsMenu(p));
  btn("Martial Techniques", ICONS.martial_seal, () => chooseTechnique(p));
  btn("Sect", ICONS.sect_token, () => openSect(p));
  btn("Dao Guide", ICONS.manual_primordial_chaos, () => guide(p));
  btn("Settings", undefined, () => settings(p));
  if (p.getGameMode() === GameMode.Creative) btn("§dCreative Tools", undefined, () => creative(p));
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    actions[r.selection]?.();
  });
}

function status(p) {
  const { i, R } = realmOf(p);
  const rate = cultivationRate(p);
  const bt = bodyTier(p);
  const nextBody = BODY_TIERS[bt + 1];
  const sect = sectOf(p);
  const lifespan = R.lifespan < 0 ? "Eternal" : `${R.lifespan + D.get(p, "lifespanBonus")} years`;
  const lines = [
    `§lRealm§r ${realmTitle(p)} §8(${i}/${LAST_REALM})`,
    `§7Cultivation speed: §a${rate.rate.toFixed(1)}/s §8(base ${R.gain} x root ${rate.root} x manual ${rate.manual.toFixed(2)} x env ${rate.env.toFixed(2)})`,
    ...rate.notes.map((n) => "  " + n),
    `§7Technique power: §cx${powerMult(p).toFixed(2)}`,
    `§7Lifespan: §f${lifespan}`,
    "",
    `§lSpirit Root§r ${rootText(p)}`,
    `§lBody§r §e${BODY_TIERS[bt].name}§7 (${fmt(D.get(p, "bodyExp"))}${nextBody ? "/" + fmt(nextBody.exp) : ""})`,
    `§7Dao Comprehension: §e${D.get(p, "dao").toFixed(1)}`,
    `§7Karma: ${karmaLabel(D.get(p, "karma"))}`,
    `§7Pill Toxicity: ${D.get(p, "tox") > 100 ? "§c" : "§a"}${Math.round(D.get(p, "tox"))}`,
    D.get(p, "deviation") > 0 ? `§4Qi Deviation: ${Math.round(D.get(p, "deviation"))}s` : "§7Qi: stable",
    D.get(p, "btBonus") ? `§7Pending breakthrough bonus: §e+${D.get(p, "btBonus")}%` : "",
    "",
    `§lAlchemy§r §f${craftTitle(p, "alchemy")}   §lRefining§r §f${craftTitle(p, "refining")}`,
    `§lSect§r ${sect ? `${sect.color}${sect.name}§7 - ${SECT_RANKS[rankOf(p)].name} (${Math.floor(D.get(p, "contrib"))} pts)` : "§8Lone cultivator"}`,
    `§7Techniques known: §f${learned(p).length}/${TECHNIQUES.length}   §7Manuals: §f${D.getJSON(p, "manuals", []).length}/${MANUALS.length}`,
    `§7Time in meditation: §f${Math.floor(D.get(p, "medTime") / 60)} min   §7Kills: §f${D.get(p, "kills")}   §7Breakthroughs: §f${D.get(p, "breakthroughs")}`,
  ].filter((l) => l !== undefined);
  new ActionFormData()
    .title("Cultivation Status")
    .body(lines.join("\n"))
    .button("Back")
    .show(p)
    .then((r) => {
      if (!r.canceled) openCodex(p);
    });
}

function breakthroughMenu(p) {
  const { next } = realmOf(p);
  const c = breakthroughChance(p);
  const body = [
    `§7Target realm: ${next.color}${next.name}`,
    "",
    ...c.parts.map(([n, v]) => `§7${n}: ${v >= 0 ? "§a+" : "§c"}${v}%`),
    `§lTotal: §e${Math.round(c.total)}%`,
    "",
    next.tribulation
      ? `§5A heavenly tribulation of ${next.tribulation} lightning waves awaits!`
      : "§7No heavenly tribulation for this realm.",
    REALMS.indexOf(next) >= 4 ? "§5You will also face your Heart Demon." : "",
    "",
    "§8Failure: lose 30% cultivation and suffer qi deviation (5 min).",
  ].join("\n");
  new MessageFormData()
    .title("Breakthrough")
    .body(body)
    .button1("Break Through!")
    .button2("Not yet")
    .show(p)
    .then((r) => {
      if (r.selection === 0) attemptBreakthrough(p);
    });
}

function manualsMenu(p) {
  const list = D.getJSON(p, "manuals", []);
  const cur = D.get(p, "manual");
  const f = new ActionFormData()
    .title("Cultivation Manuals")
    .body(
      "§7Choose the art you cultivate. Read jade slips to learn more manuals.\n§8Element-incompatible manuals work at 60% strength.",
    );
  const mans = MANUALS.filter((m) => list.includes(m.id));
  for (const m of mans) {
    const compat = m.element === "none" || hasElement(p, m.element);
    f.button(
      `${m.id === cur ? "§l> " : ""}${GRADE_COLORS[m.grade]}${m.name}\n§8${m.grade} x${m.mult}${compat ? "" : " §c(incompatible)"}`,
      ICONS[`manual_${m.id}`],
    );
  }
  if (!mans.length) f.button("§8(none learned)");
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined || !mans[r.selection]) return;
    const m = mans[r.selection];
    new MessageFormData()
      .title(m.name)
      .body(
        `${GRADE_COLORS[m.grade]}${m.grade} grade§7, element ${ELEMENTS[m.element].color}${ELEMENTS[m.element].name}§7, x${m.mult}\n\n§f${m.desc}`,
      )
      .button1("Cultivate this art")
      .button2("Back")
      .show(p)
      .then((c) => {
        if (c.selection === 0) {
          D.set(p, "manual", m.id);
          applyStats(p);
          say(p, `§e[Manual] §7You now cultivate ${GRADE_COLORS[m.grade]}${m.name}§7.`);
        } else manualsMenu(p);
      });
  });
}

export function chooseTechnique(p) {
  const known = learned(p)
    .map((id) => TECH[id])
    .filter(Boolean);
  const sel = D.get(p, "techSel");
  const { i } = realmOf(p);
  const f = new ActionFormData()
    .title("Martial Techniques")
    .body("§7Select the technique your Martial Jade Seal will cast.\n§8Learn more from Technique Scrolls.");
  for (const t of known) {
    const el = ELEMENTS[t.element];
    const locked = i < t.realm;
    const cd = cooldownLeft(p, t.id);
    f.button(
      `${t.id === sel ? "§l> " : ""}${locked ? "§8" : el.color}${t.name}\n§8${t.cost} qi | ${(t.cd / 20).toFixed(1)}s${locked ? " | " + REALMS[t.realm].name : ""}${cd ? " | cd " + (cd / 20).toFixed(0) + "s" : ""}`,
      ICONS[`scroll_${t.id}`],
    );
  }
  if (!known.length) f.button("§8(none learned)");
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined || !known[r.selection]) return;
    const t = known[r.selection];
    D.set(p, "techSel", t.id);
    say(p, `§b[Technique] §7Selected ${ELEMENTS[t.element].color}${t.name}§7. §8${t.desc}`);
    sound(p, "random.click");
  });
}

// ------------------------------------------------------------------ guide
/** @type {[string, string[]][]} */
const GUIDE = [
  [
    "The Path of Cultivation",
    [
      "Every cultivator begins as a §7Mortal§r. Meditate to sense qi and enter §fQi Condensation§r.",
      "",
      "§lRealms§r (each with minor stages):",
      ...REALMS.map(
        (r, k) =>
          `${r.color}${k}. ${r.name}§r - ${r.stages.length} stage(s)${r.tribulation ? `, §5${r.tribulation}-wave tribulation§r` : ""}`,
      ),
      "",
      "Minor stages advance automatically. At the peak of a realm you hit a §6bottleneck§r and must attempt a §ebreakthrough§r from this codex.",
      "Each realm grants more health, strength, resistance, speed, qi and longer lifespan.",
    ],
  ],
  [
    "Meditation",
    [
      "§lHow to meditate:§r",
      "- Sneak and stand still for ~2.5s with an empty hand (or holding the Codex), or",
      "- Use the §eMeditate§r button in the Codex, or",
      "- Right-click a §eMeditation Cushion§r (+30% speed).",
      "Moving ends meditation. Meditating restores qi quickly.",
      "",
      "§lSpeed multipliers:§r spirit root grade, your cultivation manual, §bSpirit Veins§r (rare glowing ore underground, x2), §bQi Gathering Arrays§r (up to x2.8), full moon, a tamed Spirit Fox nearby, and your sect.",
      "Spirit stones can be absorbed directly (use; sneak-use for a whole stack).",
    ],
  ],
  [
    "Spirit Roots",
    [
      "Everyone is born with a spirit root. Test it with a §eSpirit Root Testing Stone§r.",
      "§6Heavenly§r (1 element) x3.0, §dMutated§r (lightning/ice/wind) x2.6, §eDual§r x2.0, §aTriple§r x1.4, §fQuad§r x1.0, §7Pseudo (5)§r x0.6.",
      "Your elements decide which manuals and techniques are compatible. Fire roots help alchemy, metal and fire roots help refining.",
      "A §fBone Marrow Cleansing Pill§r removes an impure element, upgrading your root. A pure root may even mutate!",
    ],
  ],
  [
    "Breakthroughs & Tribulations",
    [
      "Breakthrough chance depends on the target realm, pills (§aFoundation§r / §eGolden Core§r / §6Nascent Soul§r / §5Void Tribulation§r pills), Dao Comprehension, karma and sect.",
      "From §eCore Formation§r onward the heavens send §5tribulation lightning§r. From §6Nascent Soul§r onward you must also slay your §5Heart Demon§r within 90 seconds.",
      "Halve tribulation damage with a §eTribulation Warding Talisman§r (in inventory), the §5Void Tribulation Pill§r or the §dNine Heavens Thunder Art§r.",
      "Failing costs 30% of your cultivation and causes qi deviation - but grants +1 Dao Comprehension.",
    ],
  ],
  [
    "Alchemy & Refining",
    [
      "Craft an §6Alchemy Furnace§r (blast furnace + iron + copper + spirit stones) and an §7Artifact Refining Forge§r (anvil + obsidian + iron + spirit stones).",
      "Right-click them to see recipes. Every attempt grants experience; higher grades raise success and pill yield.",
      "Too many pills cause §2toxicity§r, halving their effect. Purge it with a §aPurifying Detox Pill§r.",
      "",
      "§lHerbs§r grow wild in matching biomes and can be replanted on grass/dirt. Mature herbs drop extra.",
      HERBS.map((h) => h.name).join(", "),
    ],
  ],
  [
    "Techniques & Talismans",
    [
      "Learn techniques from §bTechnique Scrolls§r (sect treasury, merchants, rogue cultivator loot).",
      "Cast with the §aMartial Jade Seal§r; sneak-use it to change technique. Damage scales with realm; incompatible elements cost 50% more qi and deal 25% less.",
      "",
      "Techniques: " + TECHNIQUES.map((t) => ELEMENTS[t.element].color + t.name + "§r").join(", "),
      "",
      "§lTalismans§r are crafted from Blank Talisman Paper + Cinnabar Ink + a catalyst: " +
        TALISMANS.map((t) => t.name).join(", "),
    ],
  ],
  [
    "Formations & Treasures",
    [
      "§bQi Gathering Array§r: boosts meditation within 8 blocks (stacks up to 3).",
      "§6Heaven Guarding Array§r: repels and damages monsters within 12 blocks.",
      "§dTeleportation Array§r: name it, then travel between arrays for 1 low-grade spirit stone.",
      "§fFlying Sword§r: Foundation Establishment+ can ride it. Hold it and use to fly where you look, sneak to hover.",
      "§fSpatial Storage Ring§r: a pocket dimension for stackable items.",
      "Spirit weapons: " + WEAPONS.map((w) => w.name).join(", "),
    ],
  ],
  [
    "Spirit Beasts & Foes",
    [
      "§fSpirit Fox§r: tame with Spirit Fruit. A fox companion boosts cultivation by 10%.",
      "§8Demonic Wolf§r packs roam at night. §6Flame-Striped Tigers§r breathe fire in hot biomes.",
      "§7Rogue Cultivators§r fire qi bolts; §4Demonic Cultivators§r drain blood. Both drop scrolls and manuals.",
      "The §3Azure Flood Dragon§r can be summoned with a Dragon Summoning Pearl - it drops Beast King Cores and dragon scales.",
      "§eWandering Pill Merchants§r trade for spirit stones. Use a §eBeast Tide Horn§r to call a beast tide for rich rewards.",
    ],
  ],
  [
    "Sects & Karma",
    [
      "Join a sect from the Codex. Complete missions for contribution, rise through the ranks and spend points in the treasury.",
      "§aRighteous§r sects reject cultivators with heavy karma. Killing villagers lowers karma; slaying demonic cultivators raises it.",
      "Heavy karma makes breakthroughs and tribulations harder - unless you belong to the §4Blood Moon Demonic Sect§r.",
    ],
  ],
];

function guide(p) {
  const f = new ActionFormData()
    .title("Dao Guide")
    .body("§7The accumulated wisdom of ten thousand years of cultivators.");
  for (const [t] of GUIDE) f.button(t);
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    const [t, lines] = GUIDE[r.selection];
    new ActionFormData()
      .title(t)
      .body(lines.join("\n"))
      .button("Back")
      .show(p)
      .then((c) => {
        if (!c.canceled) guide(p);
      });
  });
}

// ------------------------------------------------------------------ settings
function isOwner(p) {
  return world.getDynamicProperty("xian:owner") === p.id || p.getGameMode() === GameMode.Creative;
}

function settings(p) {
  const owner = isOwner(p);
  const f = new ModalFormData()
    .title("Settings")
    .dropdown("Cultivation HUD (action bar)", ["Off", "When holding cultivation items", "Always"], {
      defaultValueIndex: D.get(p, "hud"),
    })
    .toggle("Realm speed bonus", { defaultValue: D.get(p, "buffSpeed") })
    .toggle("Realm jump bonus", { defaultValue: D.get(p, "buffJump") })
    .toggle("Spirit Eyes (night vision, Foundation+)", { defaultValue: D.get(p, "nightVision") });
  if (owner) {
    f.toggle("§eWorld: techniques can hurt players (PvP)", {
      defaultValue: world.getDynamicProperty("xian:pvp") === true,
    });
    f.toggle("§eWorld: random night beast tides", { defaultValue: world.getDynamicProperty("xian:tides") !== false });
  }
  f.show(p).then((r) => {
    if (r.canceled || !r.formValues) return;
    const v = r.formValues;
    D.set(p, "hud", Number(v[0]));
    D.set(p, "buffSpeed", !!v[1]);
    D.set(p, "buffJump", !!v[2]);
    D.set(p, "nightVision", !!v[3]);
    if (owner) {
      world.setDynamicProperty("xian:pvp", !!v[4]);
      world.setDynamicProperty("xian:tides", !!v[5]);
    }
    applyStats(p);
    say(p, "§7Settings saved.");
  });
}

// ------------------------------------------------------------------ creative tools
function creative(p) {
  const f = new ActionFormData().title("Creative Tools").body("§7Testing helpers (creative mode only).");
  f.button("Set realm & stage");
  f.button("Fill qi & heal");
  f.button("Reroll spirit root");
  f.button("Give all manuals & scrolls");
  f.button("Give all pills & talismans");
  f.button("Give herbs & materials");
  f.button("Give weapons & armor");
  f.button("+1000 alchemy & refining exp");
  f.button("+1000 sect contribution");
  f.button("§cReset all cultivation data");
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    switch (r.selection) {
      case 0:
        return setRealm(p);
      case 1:
        D.set(p, "qi", maxQi(p));
        p.getComponent("minecraft:health")?.resetToMaxValue();
        return;
      case 2:
        D.set(p, "roots", rollRoots().join(","));
        D.set(p, "rootsKnown", true);
        say(p, "Root: " + rootText(p));
        return applyStats(p);
      case 3:
        MANUALS.forEach((m) => giveItem(p, `xian:manual_${m.id}`));
        return TECHNIQUES.forEach((t) => giveItem(p, `xian:scroll_${t.id}`));
      case 4:
        PILLS.forEach((x) => giveItem(p, `xian:${x.id}`, 4));
        return TALISMANS.forEach((x) => giveItem(p, `xian:${x.id}`, 4));
      case 5:
        HERBS.forEach((h) => giveItem(p, `xian:${h.id}`, 16));
        for (const id of [
          "low_spirit_stone",
          "mid_spirit_stone",
          "high_spirit_stone",
          "spirit_jade",
          "profound_iron_ingot",
          "low_beast_core",
          "mid_beast_core",
          "high_beast_core",
          "king_beast_core",
          "tribulation_essence",
          "dragon_scale",
          "talisman_paper",
          "cinnabar_ink",
          "immortal_peach",
          "spirit_fruit",
        ])
          giveItem(p, `xian:${id}`, 16);
        return SPECIAL_ITEMS.forEach((s) => giveItem(p, `xian:${s.id}`));
      case 6:
        WEAPONS.forEach((w) => giveItem(p, `xian:${w.id}`));
        for (const s of ["daoist", "celestial"])
          for (const piece of ["helmet", "chestplate", "leggings", "boots"]) giveItem(p, `xian:${s}_${piece}`);
        return;
      case 7:
        D.add(p, "alchExp", 1000);
        return D.add(p, "refineExp", 1000);
      case 8:
        return D.add(p, "contrib", 1000);
      case 9:
        D.reset(p);
        return say(p, "§cCultivation data reset. Rejoin the world to receive a new spirit root.");
    }
  });
}

function setRealm(p) {
  const { i, stage } = realmOf(p);
  new ModalFormData()
    .title("Set Realm")
    .dropdown(
      "Realm",
      REALMS.map((r) => r.name),
      { defaultValueIndex: i },
    )
    .slider("Stage (clamped to realm)", 0, 8, { valueStep: 1, defaultValue: stage })
    .toggle("Fill cultivation to bottleneck", { defaultValue: false })
    .show(p)
    .then((r) => {
      if (r.canceled || !r.formValues) return;
      const ri = Number(r.formValues[0]);
      const st = Math.min(Number(r.formValues[1]), REALMS[ri].stages.length - 1);
      D.set(p, "realm", ri);
      D.set(p, "stage", st);
      D.set(p, "exp", r.formValues[2] ? expNeeded(ri, st) : 0);
      D.set(p, "qi", maxQi(p));
      applyStats(p);
      say(p, `§dRealm set to ${realmTitle(p)}`);
    });
}
