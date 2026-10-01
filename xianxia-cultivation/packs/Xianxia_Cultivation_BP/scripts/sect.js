// Sects: joining, ranks, missions and the contribution treasury.
import { ActionFormData, MessageFormData } from "@minecraft/server-ui";
import { SECTS, SECT_RANKS, SECT_MISSIONS, SECT_TREASURY, ICONS } from "./data.js";
import * as D from "./playerdata.js";
import { on } from "./bus.js";
import { say, title, sound, giveItem, countItem, removeItem, itemName, broadcast } from "./util.js";

export function sectOf(p) {
  return SECTS.find((s) => s.id === D.get(p, "sect"));
}

export function rankOf(p) {
  const c = D.get(p, "contrib");
  let r = 0;
  SECT_RANKS.forEach((R, k) => {
    if (c >= R.contribution) r = k;
  });
  return r;
}

function missionOf(p) {
  const m = D.getJSON(p, "mission", null);
  if (!m) return null;
  const def = SECT_MISSIONS.find((x) => x.id === m.id);
  return def ? { ...m, def } : null;
}

const icon = (id) => ICONS[id];

export function openSect(p) {
  const sect = sectOf(p);
  if (!sect) return joinMenu(p);
  const rank = rankOf(p);
  const contrib = D.get(p, "contrib");
  const next = SECT_RANKS[rank + 1];
  const m = missionOf(p);
  const f = new ActionFormData()
    .title(`${sect.color}${sect.name}`)
    .body(
      [
        `§7Rank: §f${SECT_RANKS[rank].name}`,
        `§7Contribution: §e${Math.floor(contrib)}${next ? ` §8/ ${next.contribution} for ${next.name}` : ""}`,
        `§7Perk: §f${sect.perk}`,
        m ? `§7Mission: §f${m.def.name} §8(${Math.floor(m.progress)}/${m.def.target})` : "§7Mission: §8none",
      ].join("\n"),
    );
  const actions = [];
  if (m?.def.kind === "deliver") {
    f.button(
      `§aDeliver: ${itemName(m.def.item)}\n§8have ${countItem(p, "xian:" + m.def.item)}/${m.def.target}`,
      icon(m.def.item),
    );
    actions.push(() => deliver(p, m));
  }
  f.button(m ? "Abandon Mission" : "Mission Hall", icon("sect_token"));
  actions.push(() => (m ? abandon(p) : missionHall(p)));
  f.button("Sect Treasury", icon("high_spirit_stone"));
  actions.push(() => treasury(p));
  f.button("§cLeave Sect");
  actions.push(() => leave(p));
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    actions[r.selection]?.();
  });
}

function joinMenu(p) {
  const karma = D.get(p, "karma");
  const f = new ActionFormData()
    .title("Sects of the Cultivation World")
    .body(
      "§7A lone cultivator walks a hard road. Join a sect to receive missions, contribution points and access to its treasury.\n§8Your karma: " +
        Math.round(karma),
    );
  for (const s of SECTS) f.button(`${s.color}${s.name}\n§8${s.alignment}`);
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    const s = SECTS[r.selection];
    new MessageFormData()
      .title(`${s.color}${s.name}`)
      .body(
        `§7Alignment: §f${s.alignment}\n\n§7Perk: §f${s.perk}\n\n${s.alignment === "righteous" ? "§8Righteous sects will not accept cultivators with heavy karma (below -30) and will expel members whose karma falls below -50." : ""}`,
      )
      .button1("Join")
      .button2("Back")
      .show(p)
      .then((c) => {
        if (c.selection !== 0) return joinMenu(p);
        if (s.alignment === "righteous" && karma < -30)
          return say(p, `§c${s.name} refuses you: the stench of blood clings to you.`);
        D.set(p, "sect", s.id);
        D.set(p, "contrib", 0);
        D.setJSON(p, "mission", null);
        title(p, `${s.color}${s.name}`, "§7You are now an Outer Disciple");
        broadcast(`§7[Sect] §f${p.name}§7 has joined the ${s.color}${s.name}§7.`);
        giveItem(p, "xian:sect_token", 1);
        sound(p, "random.levelup");
      });
  });
}

function missionHall(p) {
  const sect = sectOf(p);
  const avail = SECT_MISSIONS.filter(
    (m) =>
      !m.alignment || m.alignment === sect.alignment || (m.alignment === "righteous" && sect.alignment === "neutral"),
  );
  const choices = [];
  const pool = [...avail];
  while (choices.length < 3 && pool.length) choices.push(pool.splice(Math.floor(Math.random() * pool.length), 1)[0]);
  const f = new ActionFormData()
    .title("Mission Hall")
    .body("§7Choose a mission. Rewards are paid in contribution and spirit stones.");
  for (const m of choices) f.button(`${m.name}\n§8+${m.reward} contribution, ${m.stones} stones`);
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    const m = choices[r.selection];
    D.setJSON(p, "mission", { id: m.id, progress: 0 });
    say(p, `§e[Mission] §f${m.name}§7: ${m.desc}`);
  });
}

function abandon(p) {
  D.setJSON(p, "mission", null);
  say(p, "§7Mission abandoned.");
}

function deliver(p, m) {
  const id = "xian:" + m.def.item;
  if (!removeItem(p, id, m.def.target)) return say(p, `§cYou need ${m.def.target}x ${itemName(id)}.`);
  completeMission(p, m.def);
}

function completeMission(p, def) {
  const before = rankOf(p);
  D.add(p, "contrib", def.reward);
  D.setJSON(p, "mission", null);
  giveItem(p, "xian:low_spirit_stone", def.stones);
  title(p, "§eMission Complete", `§7+${def.reward} contribution`);
  sound(p, "random.levelup", { pitch: 1.3 });
  const after = rankOf(p);
  if (after > before) {
    const sect = sectOf(p);
    say(p, `§6[Sect] §7You have been promoted to §f${SECT_RANKS[after].name}§7!`);
    broadcast(`§7[Sect] §f${p.name}§7 is now a ${SECT_RANKS[after].name} of the ${sect.color}${sect.name}§7.`);
  }
}

function progress(p, kind, amount = 1) {
  const m = missionOf(p);
  if (!m || m.def.kind !== kind) return;
  m.progress += amount;
  if (m.progress >= m.def.target) return completeMission(p, m.def);
  D.setJSON(p, "mission", { id: m.id, progress: m.progress });
}

function treasury(p) {
  const rank = rankOf(p);
  const contrib = D.get(p, "contrib");
  const f = new ActionFormData()
    .title("Sect Treasury")
    .body(`§7Contribution: §e${Math.floor(contrib)}  §7Rank: §f${SECT_RANKS[rank].name}`);
  for (const t of SECT_TREASURY) {
    const locked = t.rank > rank;
    f.button(
      `${locked ? "§8" : contrib >= t.cost ? "§2" : "§4"}${itemName(t.item)}${t.count > 1 ? " x" + t.count : ""}\n§8${t.cost} pts${locked ? " - " + SECT_RANKS[t.rank].name : ""}`,
      icon(t.item),
    );
  }
  f.show(p).then((r) => {
    if (r.canceled || r.selection === undefined) return;
    const t = SECT_TREASURY[r.selection];
    if (t.rank > rank) return say(p, `§cOnly ${SECT_RANKS[t.rank].name}s may take this.`);
    if (D.get(p, "contrib") < t.cost) return say(p, "§cNot enough contribution.");
    D.add(p, "contrib", -t.cost);
    giveItem(p, "xian:" + t.item, t.count);
    sound(p, "random.pop");
    say(p, `§e[Treasury] §7Received ${itemName(t.item)} x${t.count}.`);
    treasury(p);
  });
}

function leave(p) {
  new MessageFormData()
    .title("Leave Sect")
    .body("§cYou will lose all contribution and rank. Are you sure?")
    .button1("Leave")
    .button2("Stay")
    .show(p)
    .then((r) => {
      if (r.selection !== 0) return;
      const s = sectOf(p);
      D.set(p, "sect", "");
      D.set(p, "contrib", 0);
      D.setJSON(p, "mission", null);
      say(p, `§7You have left the ${s?.name ?? "sect"}.`);
    });
}

export function checkExpulsion(p) {
  const s = sectOf(p);
  if (s?.alignment === "righteous" && D.get(p, "karma") < -50) {
    D.set(p, "sect", "");
    D.set(p, "contrib", 0);
    D.setJSON(p, "mission", null);
    title(p, "§4Expelled!", `§7${s.name} casts you out for your crimes.`);
    broadcast(`§4[Sect] §f${p.name}§7 has been expelled from the ${s.color}${s.name}§7 for heavy karma.`);
  }
}

// mission progress hooks
on("kill", (p, victim, fams) => {
  const fam = (f) => fams.includes(f);
  if (fam("monster")) progress(p, "kill_hostile");
  if (fam("spirit_beast")) progress(p, "kill_beast");
  if (fam("cultivator") && !fam("merchant")) progress(p, "kill_demonic");
  if (fam("villager")) progress(p, "kill_villager");
});
on("meditate_second", (p) => progress(p, "meditate"));
on("alchemy_success", (p) => progress(p, "alchemy"));
