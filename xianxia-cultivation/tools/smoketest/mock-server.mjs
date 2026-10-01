// Minimal mock of @minecraft/server for headless smoke tests of the add-on scripts.
// It implements just enough behaviour (dynamic properties, inventory, effects, scheduling)
// for the gameplay code paths to execute; world queries return empty/air results.

export const EntityDamageCause = new Proxy({}, { get: (_, k) => String(k) });
export const WeatherType = { Clear: "Clear", Rain: "Rain", Thunder: "Thunder" };
export const MoonPhase = { FullMoon: 0, NewMoon: 4 };
export const GameMode = { Creative: "Creative", Survival: "Survival" };

export const log = [];

// ------------------------------------------------------------------ scheduling
const jobs = [];
let tick = 0;
export const system = {
  get currentTick() {
    return tick;
  },
  beforeEvents: { startup: signal() },
  run(fn) {
    return system.runTimeout(fn, 0);
  },
  runTimeout(fn, delay = 0) {
    const j = { fn, at: tick + Math.max(1, delay), every: 0, id: jobs.length + 1 };
    jobs.push(j);
    return j.id;
  },
  runInterval(fn, every = 1) {
    const j = { fn, at: tick + every, every, id: jobs.length + 1 };
    jobs.push(j);
    return j.id;
  },
  clearRun(id) {
    const j = jobs.find((x) => x.id === id);
    if (j) j.dead = true;
  },
  runJob(gen) {
    const j = system.runInterval(() => {
      if (gen.next().done) system.clearRun(j);
    }, 1);
    return j;
  },
};

export function advance(n = 1) {
  for (let k = 0; k < n; k++) {
    tick++;
    for (const j of [...jobs]) {
      if (j.dead || j.at > tick) continue;
      j.fn();
      if (j.every) j.at = tick + j.every;
      else j.dead = true;
    }
  }
}

function signal() {
  const subs = [];
  return {
    subs,
    subscribe(fn) {
      subs.push(fn);
      return fn;
    },
    unsubscribe() {},
    fire(ev) {
      for (const s of subs) s(ev);
    },
  };
}

// ------------------------------------------------------------------ items & containers
const MAX_STACK = (id) => (/(sword|saber|spear|blade|helmet|chestplate|leggings|boots|codex|seal|ring|pearl|horn|token|manual_|scroll_)/.test(id) ? 1 : 64);

export class ItemStack {
  constructor(typeId, amount = 1) {
    if (!typeId.includes(":")) typeId = "minecraft:" + typeId;
    this.typeId = typeId;
    this.amount = amount;
    this.maxAmount = MAX_STACK(typeId);
    this.isStackable = this.maxAmount > 1;
  }
}

class Container {
  constructor(size = 36) {
    this.size = size;
    this.slots = new Array(size).fill(undefined);
  }
  getItem(i) {
    const s = this.slots[i];
    return s ? Object.assign(new ItemStack(s.typeId, s.amount), {}) : undefined;
  }
  setItem(i, it) {
    this.slots[i] = it ? { typeId: it.typeId, amount: it.amount } : undefined;
  }
  addItem(it) {
    let left = it.amount;
    for (let i = 0; i < this.size && left > 0; i++) {
      const s = this.slots[i];
      if (s && s.typeId === it.typeId && s.amount < it.maxAmount) {
        const n = Math.min(left, it.maxAmount - s.amount);
        s.amount += n;
        left -= n;
      }
    }
    for (let i = 0; i < this.size && left > 0; i++) {
      if (!this.slots[i]) {
        const n = Math.min(left, it.maxAmount);
        this.slots[i] = { typeId: it.typeId, amount: n };
        left -= n;
      }
    }
    return left > 0 ? new ItemStack(it.typeId, left) : undefined;
  }
}

// ------------------------------------------------------------------ entities
let nextId = 1;
export class MolangVariableMap {
  setColorRGB() {}
  setColorRGBA() {}
  setFloat() {}
}

export class Entity {
  constructor(typeId, dim, loc = { x: 0, y: 64, z: 0 }) {
    this.typeId = typeId;
    this.id = String(nextId++);
    this.dimension = dim;
    this.location = { ...loc };
    this.isValid = true;
    this.props = new Map();
    this.effects = new Map();
    this.tags = new Set();
    this.health = { currentValue: 20, effectiveMax: 20 };
    this.nameTag = "";
    this.isOnGround = true;
    this.isInWater = false;
    this.isSneaking = false;
    this.families = [];
  }
  getDynamicProperty(k) {
    return this.props.get(k);
  }
  setDynamicProperty(k, v) {
    if (v === undefined) this.props.delete(k);
    else this.props.set(k, v);
  }
  getComponent(id) {
    if (id === "minecraft:health" || id === "health") {
      const h = this.health;
      return {
        get currentValue() {
          return h.currentValue;
        },
        get effectiveMax() {
          return h.effectiveMax;
        },
        setCurrentValue: (v) => ((h.currentValue = Math.max(0, Math.min(h.effectiveMax, v))), true),
        resetToMaxValue: () => (h.currentValue = h.effectiveMax),
      };
    }
    if (id === "minecraft:type_family") return { getTypeFamilies: () => this.families };
    if (id === "minecraft:inventory" && this.container) return { container: this.container };
    return undefined;
  }
  matches(o) {
    return (o.families ?? []).every((f) => this.families.includes(f));
  }
  addEffect(id, duration, opts = {}) {
    if (typeof id !== "string") throw new Error("bad effect id");
    this.effects.set(id, { typeId: id, duration, amplifier: opts.amplifier ?? 0 });
    if (id === "health_boost") this.health.effectiveMax = 20 + 4 * ((opts.amplifier ?? 0) + 1);
    return this.effects.get(id);
  }
  getEffect(id) {
    return this.effects.get(id);
  }
  removeEffect(id) {
    if (id === "health_boost") this.health.effectiveMax = 20;
    return this.effects.delete(id);
  }
  applyDamage(n, opts) {
    if (!Number.isFinite(n)) throw new Error("non-finite damage " + n);
    this.health.currentValue -= n;
    log.push(["damage", this.typeId, Math.round(n * 10) / 10, opts?.cause]);
    return true;
  }
  applyKnockback(h, v) {
    if (typeof h !== "object" || typeof v !== "number") throw new Error("applyKnockback signature");
  }
  applyImpulse() {}
  clearVelocity() {}
  getVelocity() {
    return { x: 0, y: 0, z: 0 };
  }
  getViewDirection() {
    return { x: 0, y: 0, z: 1 };
  }
  getHeadLocation() {
    return { x: this.location.x, y: this.location.y + 1.6, z: this.location.z };
  }
  getRotation() {
    return { x: 0, y: 0 };
  }
  teleport(loc) {
    this.location = { ...loc };
  }
  tryTeleport(loc) {
    this.location = { ...loc };
    return true;
  }
  setOnFire() {
    return true;
  }
  addTag(t) {
    this.tags.add(t);
    return true;
  }
  hasTag(t) {
    return this.tags.has(t);
  }
  kill() {
    this.isValid = false;
    return true;
  }
  remove() {
    this.isValid = false;
  }
  runCommand() {
    return { successCount: 1 };
  }
}

export class Player extends Entity {
  constructor(name, dim) {
    super("minecraft:player", dim);
    this.name = name;
    this.container = new Container();
    this.selectedSlotIndex = 0;
    this.families = ["player"];
    this.messages = [];
    this.gameMode = GameMode.Survival;
    this.onScreenDisplay = {
      setActionBar: (t) => this.messages.push(["actionbar", String(t)]),
      setTitle: (t, o) => this.messages.push(["title", String(t), o?.subtitle]),
    };
  }
  sendMessage(m) {
    this.messages.push(["chat", String(m)]);
  }
  playSound() {}
  spawnParticle() {}
  getSpawnPoint() {
    return undefined;
  }
  getGameMode() {
    return this.gameMode;
  }
}

class Block {
  constructor(dim, loc, typeId = "minecraft:air") {
    this.dimension = dim;
    this.location = loc;
    this.typeId = typeId;
    this.isAir = typeId === "minecraft:air";
    this.isLiquid = false;
    this.permutation = { getState: () => 0, withState: () => this.permutation, type: { id: typeId } };
  }
  setPermutation() {}
}

class Dimension {
  constructor(id) {
    this.id = "minecraft:" + id;
    this.entities = [];
  }
  getBlock(loc) {
    return new Block(this, loc, loc.y < 60 ? "minecraft:stone" : "minecraft:air");
  }
  getBlockFromRay() {
    return undefined;
  }
  getTopmostBlock(xz) {
    return new Block(this, { x: Math.floor(xz.x), y: 63, z: Math.floor(xz.z) }, "minecraft:grass_block");
  }
  getEntities(o = {}) {
    return this.entities.filter((e) => {
      if (!e.isValid) return false;
      if (o.type && e.typeId !== o.type) return false;
      if (o.tags && !o.tags.every((t) => e.tags.has(t))) return false;
      if (o.families && !o.families.every((f) => e.families.includes(f))) return false;
      if (o.location && o.maxDistance !== undefined) {
        const d = Math.hypot(e.location.x - o.location.x, e.location.y - o.location.y, e.location.z - o.location.z);
        if (d > o.maxDistance) return false;
      }
      return true;
    });
  }
  getEntitiesFromRay(from) {
    return this.getEntities({ location: from, maxDistance: 30 })
      .filter((e) => e.typeId !== "minecraft:player")
      .map((entity) => ({ entity, distance: 1 }));
  }
  getPlayers(o = {}) {
    let ps = world.getAllPlayers().filter((p) => p.dimension === this);
    if (o.location && o.maxDistance !== undefined)
      ps = ps.filter((p) => Math.hypot(p.location.x - o.location.x, p.location.z - o.location.z) <= o.maxDistance);
    return ps;
  }
  spawnEntity(typeId, loc) {
    if (typeof typeId !== "string") throw new Error("spawnEntity id");
    const e = new Entity(typeId, this, loc);
    if (typeId.startsWith("xian:")) e.families = ["monster"];
    e.health = { currentValue: 40, effectiveMax: 40 };
    this.entities.push(e);
    log.push(["spawn", typeId]);
    return e;
  }
  spawnItem(stack, loc) {
    log.push(["drop", stack.typeId, stack.amount]);
    return new Entity("minecraft:item", this, loc);
  }
  spawnParticle(id) {
    if (typeof id !== "string") throw new Error("particle id");
  }
  playSound() {}
  setWeather(w) {
    log.push(["weather", w]);
  }
  createExplosion() {
    return true;
  }
  runCommand() {
    return { successCount: 1 };
  }
}

const dims = { overworld: new Dimension("overworld"), nether: new Dimension("nether"), the_end: new Dimension("the_end") };
const players = [];
const worldProps = new Map();

const afterNames = [
  "worldLoad", "playerSpawn", "playerLeave", "weatherChange", "itemUse", "itemCompleteUse", "entityHitEntity",
  "playerPlaceBlock", "playerBreakBlock", "entityDie", "entityHurt", "playerInteractWithEntity",
  "dataDrivenEntityTrigger",
];
export const world = {
  afterEvents: Object.fromEntries(afterNames.map((n) => [n, signal()])),
  beforeEvents: {},
  getDimension: (id) => dims[id.replace("minecraft:", "")],
  getAllPlayers: () => players.filter((p) => p.isValid),
  getPlayers: () => players.filter((p) => p.isValid),
  getEntity: (id) => Object.values(dims).flatMap((d) => d.entities).find((e) => e.id === id && e.isValid),
  getDynamicProperty: (k) => worldProps.get(k),
  setDynamicProperty: (k, v) => (v === undefined ? worldProps.delete(k) : worldProps.set(k, v)),
  sendMessage: (m) => log.push(["broadcast", String(m)]),
  getTimeOfDay: () => 6000,
  getAbsoluteTime: () => tick,
  getMoonPhase: () => MoonPhase.FullMoon,
  getDefaultSpawnLocation: () => ({ x: 0, y: 32767, z: 0 }),
};

export function addPlayer(name) {
  const p = new Player(name, dims.overworld);
  players.push(p);
  return p;
}

export function addMob(typeId, loc, families = ["monster"]) {
  const e = dims.overworld.spawnEntity(typeId, loc);
  e.families = families;
  return e;
}
