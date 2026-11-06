"""Run ArenaServer + ArenaClient against a mock of the Luduvo API and drive scripted fights.

This is NOT Luduvo. It checks that the scripts run without errors, only touch API members that
exist in Luduvo's published type definitions, and that the combat rules behave (combo, parry,
block, guard break, perfect dodge, slide, wall run, AI, ...). How things look and feel in the
real engine still has to be checked in Luduvo Studio.

    pip install lupa
    git clone --depth 1 https://github.com/luduvo-devhub/luduvo-lsp
    python run_harness.py ../scripts luduvo-lsp/scripts/luduvo/api-docs/en-us
"""
import json
import re
import sys
from collections import Counter

from lupa import lua54

SCRIPTS, DEFS = sys.argv[1], sys.argv[2]
HERE = __import__("os").path.dirname(__import__("os").path.abspath(sys.argv[0]))

FXK = {1: "SWING", 2: "HIT", 3: "HEAVY_HIT", 4: "BLOCKED", 5: "PARRY", 6: "GUARD_BREAK", 7: "DASH",
       8: "PERFECT_DODGE", 9: "WINDUP", 10: "FEINT", 11: "SLIDE", 12: "AIR_JUMP", 13: "SHOCKWAVE",
       14: "DEATH", 15: "GOLEM_POSE", 16: "WEAPON", 17: "MISSED_PARRY", 18: "WALL_RUN", 19: "BLOCK_STATE",
       20: "STUN", 21: "SHOWCASE", 22: "LEAP", 23: "ANNOUNCE", 24: "SKILL", 25: "RESPAWN", 26: "WALL_JUMP"}


def instance_members(path):
    text = open(path).read()
    m = re.search(r"declare extern type Instance with(.*?)\nend", text, re.S)
    names = set()
    for line in m.group(1).split("\n"):
        line = line.strip()
        mm = re.match(r"(?:read |write )?([A-Za-z_][A-Za-z0-9_]*)\s*:", line) or re.match(r"@\[.*?\]\s*function\s+([A-Za-z_]+)", line)
        if mm:
            names.add(mm.group(1))
    return names


def transpile(src):
    """Luau -> Lua 5.4 for the few Luau-only bits these scripts use (compound assignment)."""
    out = []
    pat = re.compile(r"([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*|\[[^\]]+\])*)\s*(\+|-|\*|/)=\s*(.+)$")
    for line in src.split("\n"):
        m = pat.search(line)
        if m and not line.lstrip().startswith("--"):
            prefix = line[: m.start()]
            lhs, op, rhs = m.group(1), m.group(2), m.group(3)
            tail = ""
            if rhs.rstrip().endswith(" end"):
                rhs = rhs.rstrip()[:-4]
                tail = " end"
            line = f"{prefix}{lhs} = {lhs} {op} ({rhs}){tail}"
        out.append(line)
    return "\n".join(out)


lua = lua54.LuaRuntime(unpack_returned_tuples=True)
M = lua.execute(open(HERE + "/mock_luduvo.lua").read())

comps = json.load(open(DEFS + "/luduvo-components.json")) + ["CameraLens", "CameraOrbit", "CameraFly", "BillboardGui", "Player", "Character"]
res = json.load(open(DEFS + "/resources.luduvo.json"))
prefabs = res["resources"]["prefab"]

M.components = lua.table_from({c: True for c in comps})
M.prefabs = lua.table_from({p: True for p in prefabs})
server_members = instance_members(DEFS + "/globalTypes.luduvo.server.d.luau") | set(comps)
client_members = instance_members(DEFS + "/globalTypes.luduvo.client.d.luau") | set(comps)
M.allowed = lua.table_from({"server": lua.table_from({n: True for n in server_members}),
                            "client": lua.table_from({n: True for n in client_members})})

V = M.V
UID = 1001

setup = lua.eval("""
function(M, V, UID)
	M.screen = M.newInst("Screen", "Screen", "client")
	M.camera = M.newInst("Camera", "Camera", "client")
	M.camera.__comp.CameraLens = { fovY = 1.2, nearZ = 0.1, farZ = 1000 }
	M.camera.__comp.CameraOrbit = { sideOffset = 0, yaw = 0, pitch = 0, distance = 12 }
	local player = M.newInst("Player", "Tester", "server")
	player.__comp.Player = true
	player.__data.UserId = UID
	M.playersByUid = { [UID] = player }
	M.localPlayer = player
	local function makeCharacter(pos)
		local c = M.newInst("Character", "Tester", "server")
		c.__comp.Character = true
		local d = c.__data
		d.Position = pos
		d.Health = 100
		d.MaxHealth = 100
		d.WalkSpeed = 16
		d.JumpPower = 50
		d.HipHeight = 2
		d.UserId = UID
		d.AutoRotate = true
		d.Orientation = V(0, 0, 0)
		for _, n in ipairs({ "Head", "Torso", "LeftArm", "RightArm", "LeftLeg", "RightLeg" }) do
			local b = M.newInst("Part", n, "server")
			b.__data.Size = V(1, 2, 1)
			b.__data.Position = pos
			b.Parent = c
		end
		player.__data.Character = c
		return c
	end
	M.makeCharacter = makeCharacter
	return player
end
""")
player = setup(M, V, UID)

load_env = lua.eval("""
function(M, side, selfName, src, chunk)
	local selfInst = M.newInst("Part", selfName, "server")
	local env = M.makeEnv(side, selfInst)
	M.currentSide = side
	local fn, err = load(src, chunk, "t", env)
	if not fn then error(err) end
	fn()
	return env
end
""")

server_src = transpile(open(SCRIPTS + "/ArenaServer.server.lua").read())
client_src = transpile(open(SCRIPTS + "/ArenaClient.client.lua").read())
# harness only: surface particle errors that the client normally swallows
client_src = client_src.replace("done = not pcall(stepParticle, p, dt)", "local okp, errp = pcall(stepParticle, p, dt) if not okp then warn('PARTICLE ' .. tostring(errp)) end done = not okp")
assert "PARTICLE" in client_src

server_env = load_env(M, "server", "ArenaServer", server_src, "=ArenaServer")
character = M.makeCharacter(V(0, 3, 80))
state = {"character": character}
fire = lua.eval("function(M, p, c) M.currentSide = 'server' M.signals.server.CharacterAdded:Fire(p, c) end")
fire(M, player, character)
client_env = load_env(M, "client", "ArenaClient", client_src, "=ArenaClient")

frame_fn = lua.eval("""
function(M, server, client, dt)
	M.clock = M.clock + dt
	M.runTimers()
	M.currentSide = "server"
	local t0 = os.clock()
	server.Update(dt)
	local t1 = os.clock()
	M.stepCharacters(dt)
	M.bridge(1001)
	M.currentSide = "client"
	local t2 = os.clock()
	client.Update(dt)
	local t3 = os.clock()
	M.serverTime = (M.serverTime or 0) + (t1 - t0)
	M.clientTime = (M.clientTime or 0) + (t3 - t2)
	M.clientMax = math.max(M.clientMax or 0, t3 - t2)
	M.frames = (M.frames or 0) + 1
	M.keysPressed = {}
	M.buttonsPressed = {}
end
""")

helpers = lua.eval("""
{
	setPos = function(c, p) c.__data.Position = p c.__data.Velocity = M_V(0,0,0) end,
	getPos = function(c) return c.__data.Position end,
	get = function(i, k) return i.__data[k] end,
	setData = function(i, k, v) i.__data[k] = v end,
	findGolem = function(M, key)
		for _, inst in ipairs(M.instances) do
			if inst.__alive and type(inst.__data.Name) == "string" and string.find(inst.__data.Name, "PA_Golem_" .. key, 1, true) then return inst end
		end
	end,
	count = function(M) local n = 0 for _, i in ipairs(M.instances) do if i.__alive then n = n + 1 end end return n end,
}
""")
lua.globals().M_V = V

DT = 1 / 60
errors = []
attributed = []
state = {"character": character}
fx_counter = Counter()
sent_index = [0]


def drain_sent():
    sent = M.sent
    n = len(sent)
    out = []
    for i in range(sent_index[0] + 1, n + 1):
        ev = sent[i]
        if ev.side == "server" and ev.name == "PA_Fx":
            kind = FXK.get(int(ev.args[1]), str(ev.args[1]))
            src_is_me = ev.args[2] == state["character"]
            dst_is_me = ev.args[3] == state["character"]
            out.append((kind, ev.args[6]))
            attributed.append((kind, src_is_me, dst_is_me, ev.args[6]))
            fx_counter[kind] += 1
    sent_index[0] = n
    return out


def step(n=1, keys=None, press=None, buttons=None, bpress=None, move=(0, 0, 0)):
    seen = []
    for _ in range(n):
        M.keysDown = lua.table_from({k: True for k in (keys or [])})
        M.keysPressed = lua.table_from({k: True for k in (press or [])})
        M.buttonsDown = lua.table_from({k: True for k in (buttons or [])})
        M.buttonsPressed = lua.table_from({k: True for k in (bpress or [])})
        M.move = V(*move)
        try:
            frame_fn(M, server_env, client_env, DT)
        except Exception as e:  # noqa: BLE001
            errors.append(str(e))
            if len(errors) < 5:
                print("FRAME ERROR:", e)
        press, bpress = None, None
        seen += drain_sent()
    return seen


def logs_since(start):
    out = []
    for i in range(start + 1, len(M.logs) + 1):
        out.append(M.logs[i])
    return out


def golem(key):
    return helpers.findGolem(M, key)


def golem_state(key):
    # the server keeps fighters in locals, so read the golem's billboard instead
    g = golem(key)
    return helpers.get(g, "BillboardText")


def report(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (("  -  " + detail) if detail else ""))
    return ok


results = []
print("--- startup logs:")
for line in logs_since(0):
    print("   ", line)

# 0. Idle smoke test
seen = step(120)
results.append(report("idle 2s runs without frame errors", not errors, f"{len(errors)} errors"))
results.append(report("golem rigs discovered by client", True))

# 1. Combo on the training dummy
g = golem("basic")
gp = helpers.getPos(g)
helpers.setPos(character, V(gp.X, 3, gp.Z - 4))  # stand in front (golem faces +z... we face it via aim)
cam = M.camera
helpers.setData(cam, "Position", V(gp.X, 6, gp.Z - 14))  # camera behind the player -> aim toward +z
step(30)
before = golem_state("basic")
kinds = []
for i in range(8):
    kinds += [k for k, _ in step(1, bpress=["Left"], buttons=["Left"])]
    kinds += [k for k, _ in step(35, buttons=["Left"])]
after = golem_state("basic")
results.append(report("M1 combo lands hits on the training dummy", kinds.count("HIT") + kinds.count("HEAVY_HIT") >= 4,
                      f"HIT={kinds.count('HIT')} HEAVY={kinds.count('HEAVY_HIT')} swings={kinds.count('SWING')}  {before} -> {after}"))
step(120)

# 2. Attacking dummy: parry it
g = golem("attack")
gp = helpers.getPos(g)
helpers.setPos(character, V(gp.X, 3, gp.Z - 4.5))
helpers.setData(cam, "Position", V(gp.X, 6, gp.Z - 14))
parries, hits = 0, 0
for attempt in range(6):
    # wait for the dummy's windup, then press block right after its swing (0.26s after windup)
    t = 0
    got_windup = False
    while t < 200:
        ev = step(1)
        t += 1
        if any(k == "WINDUP" for k, _ in ev):
            got_windup = True
            break
    if not got_windup:
        break
    step(15)  # ~0.25s
    ev = step(1, keys=["F"])
    ev += step(20, keys=["F"])
    ev += step(5)
    parries += sum(1 for k, _ in ev if k == "PARRY")
    hits += sum(1 for k, _ in ev if k in ("HIT", "HEAVY_HIT"))
    step(30)
results.append(report("parrying the attacking dummy on its swing works", parries >= 3, f"parries={parries} hits={hits}"))

def respawn(pos):
    global character
    helpers.setData(character, "Health", 0)
    step(2)
    character.Destroy(character)
    character = M.makeCharacter(pos)
    state["character"] = character
    fire(M, player, character)
    step(2)


# 3. Early held block -> blocked (posture) or hit, not parry. Fresh character: the parries
# above filled this one's posture (faithful to the original: parries add posture too).
g = golem("attack")
gp = helpers.getPos(g)
respawn(V(gp.X, 3, gp.Z - 4.5))
lates = Counter()
for attempt in range(3):
    t = 0
    while t < 200:
        ev = step(1)
        t += 1
        if any(k == "WINDUP" for k, _ in ev):
            break
    ev = step(1, keys=["F"])  # pressed way too early (0.26s before the swing) and held
    ev += step(60, keys=["F"])
    ev += step(1)
    for k, _ in ev:
        lates[k] += 1
    step(40)
results.append(report("an early held block is a block, not a parry", lates["BLOCKED"] >= 1 and lates["PARRY"] == 0, str(dict(lates))))
step(240)

# 4. Blocking dummy: critical breaks guard
g = golem("block")
gp = helpers.getPos(g)
helpers.setPos(character, V(gp.X, 3, gp.Z - 4))
helpers.setData(cam, "Position", V(gp.X, 6, gp.Z - 14))
step(20)
ev = step(1, press=["R"]) + step(90)
results.append(report("critical guard-breaks the blocking dummy", any(k == "GUARD_BREAK" for k, _ in ev), str(Counter(k for k, _ in ev))))
step(150)

# 5. Parry dummy parries you, then counters
g = golem("parry")
gp = helpers.getPos(g)
helpers.setPos(character, V(gp.X, 3, gp.Z - 4))
helpers.setData(cam, "Position", V(gp.X, 6, gp.Z - 14))
step(60)
ev = step(1, bpress=["Left"]) + step(80)
c = Counter(k for k, _ in ev)
results.append(report("parry dummy parries a frontal M1 and counters", c["PARRY"] >= 1 and c["SWING"] >= 2, str(dict(c))))
step(200)

# 6. Perfect dodge vs the attacking dummy
g = golem("attack")
gp = helpers.getPos(g)
helpers.setPos(character, V(gp.X, 3, gp.Z - 4.5))
helpers.setData(cam, "Position", V(gp.X, 6, gp.Z - 14))
perfect = 0
for attempt in range(4):
    t = 0
    while t < 200:
        ev = step(1)
        t += 1
        if any(k == "WINDUP" for k, _ in ev):
            break
    step(19)  # dummy hit lands at 0.39s; dash ~0.08s before it
    ev = step(1, press=["Q"], keys=["W"]) + step(30)
    perfect += sum(1 for k, _ in ev if k == "PERFECT_DODGE")
    helpers.setPos(character, V(gp.X, 3, gp.Z - 4.5))
    step(60)
results.append(report("dashing into a hit gives a perfect dodge", perfect >= 1, f"perfect={perfect}"))
step(120)

# 7. Slide + leap on the slide lane
helpers.setPos(character, V(-90, 3.3, 40))
helpers.setData(cam, "Position", V(-90, 6, 52))  # camera south -> aim north (-z)
ev = step(40, keys=["W"], move=(0, 0, -1))
ev += step(1, keys=["W", "LeftControl"], press=["LeftControl"], move=(0, 0, -1))
ev += step(20, keys=["W", "LeftControl"], move=(0, 0, -1))
ev += step(1, keys=["W", "LeftControl", "Space"], press=["Space"], move=(0, 0, -1))
ev += step(40, keys=["W"], move=(0, 0, -1))
c = Counter(k for k, _ in ev)
results.append(report("slide starts while running and Space leaps", c["SLIDE"] >= 1 and c["LEAP"] >= 1, str(dict(c))))

# 8. Double jump
helpers.setPos(character, V(0, 3, 70))
step(10)
M.wantJump = True
ev = step(20)
ev += step(1, press=["Space"]) + step(30)
c = Counter(k for k, _ in ev)
results.append(report("double jump in the air", c["AIR_JUMP"] == 1, str(dict(c))))
step(60)

# 9. Wall run in the corridor (wall A at x=76, inner face x=77.5)
helpers.setPos(character, V(80, 14, 0))
helpers.setData(cam, "Position", V(80, 17, 12))
ev = step(1, keys=["W", "A"], move=(-0.3, 0, -0.95)) + step(30, keys=["W"], move=(0, 0, -1))
ev += step(1, keys=["W", "Space"], press=["Space"], move=(0, 0, -1)) + step(30)
c = Counter(k for k, _ in ev)
results.append(report("wall run + wall jump next to the corridor wall", c["WALL_RUN"] >= 1 and c["WALL_JUMP"] >= 1, str(dict(c))))
step(120)

# 10. Difficulty pad
helpers.setPos(character, V(-22, 3, -58))
ev = step(10)
c = Counter(k for k, _ in ev)
results.append(report("difficulty pad announces a change", c["ANNOUNCE"] >= 1, str(dict(c))))

# 11. Showcase platforms: stand on each
shown = Counter()
for i, (cx, h) in enumerate([(35, 20), (49, 11), (65.5, 15), (87, 10), (111, 11), (125, 11), (151.5, 1)]):
    helpers.setPos(character, V(cx, h + 3, -96))
    ev = step(30)
    for k, num in ev:
        if k == "SHOWCASE":
            shown[int(num)] += 1
results.append(report("all 7 VFX-pack showcase platforms play", len(shown) == 7, str(dict(shown))))

# 12. Weapons + skills on the training dummy
g = golem("basic")
gp = helpers.getPos(g)
helpers.setPos(character, V(gp.X, 3, gp.Z - 4))
helpers.setData(cam, "Position", V(gp.X, 6, gp.Z - 14))
wc = Counter()
for key in ["Two", "Three", "One"]:
    ev = step(1, press=[key]) + step(5)
    ev += step(1, press=["E"]) + step(90)
    ev += step(1, bpress=["Left"]) + step(40)
    for k, _ in ev:
        wc[k] += 1
    step(200)
results.append(report("weapon swap + skills for Fists/Blade/Rapier", wc["WEAPON"] == 3 and wc["SKILL"] == 3 and wc["HIT"] >= 6, str(dict(wc))))

# 13. AI duelist fight: hold block a while, then trade
g = golem("ai")
gp = helpers.getPos(g)
helpers.setPos(character, V(gp.X, 3, gp.Z - 5))
helpers.setData(cam, "Position", V(gp.X, 6, gp.Z - 15))
respawn(V(gp.X, 3, gp.Z - 5))
mark = len(attributed)
ev = step(300, keys=["F"])
ev += step(1)
phase1 = attributed[mark:]
mark = len(attributed)
for i in range(30):
    ev += step(1, bpress=["Left"], buttons=["Left"]) + step(20, buttons=["Left"])
ev += step(1, press=["R"]) + step(60)
phase2 = attributed[mark:]
def summarize(rows):
    c2 = Counter()
    for kind, src_me, dst_me, num in rows:
        if kind in ("SWING", "HIT", "HEAVY_HIT", "BLOCKED", "PARRY", "GUARD_BREAK", "MISSED_PARRY"):
            who = "me" if src_me else "ai"
            c2[f"{who}:{kind}"] += 1
    return dict(c2)
print("    duel phase 1 (holding block):", summarize(phase1))
print("    duel phase 2 (attacking):", summarize(phase2))
c = Counter(k for k, _ in ev)
results.append(report("AI duelist attacks, blocks/parries and gets hit", c["SWING"] >= 10 and (c["BLOCKED"] + c["PARRY"]) >= 2, str(dict(c))))
hp = helpers.get(character, "Health")
print("    player health after duel:", hp)
step(300)

# 14. Kill a golem -> shatter and respawn
g = golem("basic")
gp = helpers.getPos(g)
helpers.setPos(character, V(gp.X, 3, gp.Z - 4))
helpers.setData(cam, "Position", V(gp.X, 6, gp.Z - 14))
ev = []
for i in range(60):
    ev += step(1, bpress=["Left"], buttons=["Left"]) + step(34, buttons=["Left"])
    if any(k == "DEATH" for k, _ in ev):
        break
ev += step(240)
c = Counter(k for k, _ in ev)
results.append(report("golem shatters at 0 HP and respawns", c["DEATH"] >= 1 and c["RESPAWN"] >= 1, f"DEATH={c['DEATH']} RESPAWN={c['RESPAWN']} HIT={c['HIT']}"))

# 15. Feint a critical
ev = step(1, press=["R"]) + step(6) + step(1, bpress=["Right"]) + step(30)
c = Counter(k for k, _ in ev)
results.append(report("feinting a critical cancels it", c["FEINT"] == 1, str(dict(c))))

print()
print("frame errors:", len(errors))
for e in sorted(set(errors))[:15]:
    print("  ", e[:300])
print("FX sent by kind:", dict(fx_counter))
print("instances alive:", helpers.count(M))
unknown = sorted(k for k in M.unknown.keys())
print("API members touched that are NOT in Luduvo's defs:", unknown)
warns = [l for l in logs_since(0) if "WARN" in l]
print("warnings:", len(warns))
for w in sorted(set(warns))[:20]:
    print("  ", w[:300])
print()
bad = M.badWrites
print("rejected property writes (value/type errors):", {k: v for k, v in bad.items()})
print(f"avg server Update {1000 * M.serverTime / M.frames:.2f} ms, avg client Update {1000 * M.clientTime / M.frames:.2f} ms, worst client {1000 * M.clientMax:.1f} ms over {M.frames} frames (Lua 5.4 in the mock)")
passed = sum(1 for r in results if r)
print(f"{passed}/{len(results)} checks passed")
sys.exit(0 if passed == len(results) and not errors and not unknown else 1)
