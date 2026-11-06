--[[
	PARRY ARENA  -  client script (Luduvo)
	=======================================

	Attach this as a CLIENT script to an Instance named "ArenaClient" (an empty Part is fine).
	It needs ArenaServer.server.lua running on another Instance.

	What runs here (all local, nothing in this file is authoritative):
	  * input: keyboard + mouse -> PA_Action / PA_Intent event tables
	  * VFX: a small pooled particle system built from plain parts, because Luduvo has no
	    ParticleEmitter or Trail. Every effect of the "Combat VFX Pack #1" is rebuilt from its
	    original numbers (colours, sizes, lifetimes, speeds, emit counts): Hit #1/#2, Hard Hit,
	    Blood Hit, Parry #1/#2, Block, Block Break, Perfect Block, Swing #1-#4, Flashstep, Smoke,
	    Dash Smoke, Shockwave, Rock Debris, Drops, Stars Burst and both Trails.
	  * the stone golems' limbs (the server only moves their torso)
	  * weapon models, block shields, lock-on marker
	  * HUD: health / stamina / posture, cooldowns, enemy panel, parry streak, controls help
	  * camera punch (FOV kick + shake)

	Controls
	  LMB        attack (hold to keep swinging)       RMB   feint a critical
	  F          block - tap it as a hit lands to PARRY
	  R          critical (breaks guards)              E     weapon skill
	  Q          dash (direction = WASD)                Ctrl  slide (while running)
	  Space      jump / double jump / slide leap / wall jump
	  1 2 3      Fists / Blade / Rapier                 Tab   lock on
	  H          show / hide controls
]]

------------------------------------------------------------------------------------------
-- CONFIG
------------------------------------------------------------------------------------------

local CONFIG = {
	Quality = 1, -- multiplies particle counts (0.5 = half, 1.5 = more)
	MaxParticles = 320,
	IntentRate = 20, -- movement/aim updates per second
	HoldAttackInterval = 0.1,
	Blood = true, -- the Blood Hit layer on heavy hits
	CameraShake = true,
	LockOnRange = 45,
	DamageNumbers = true,
}

------------------------------------------------------------------------------------------
-- SHARED PROTOCOL (must match ArenaServer.server.lua exactly)
------------------------------------------------------------------------------------------

local ET = game.EventTable

local ACTION = ET("PA_Action", ET.ToServer, {
	{ "code", ET.U8 },
	{ "dir", ET.Vec3 },
	{ "aim", ET.Vec3 },
})

local INTENT = ET("PA_Intent", ET.ToServer, {
	{ "move", ET.Vec3 },
	{ "aim", ET.Vec3 },
	{ "flags", ET.U8 },
})

local FX = ET("PA_Fx", ET.ToClients, {
	{ "kind", ET.U8 },
	{ "src", ET.Entity },
	{ "dst", ET.Entity },
	{ "pos", ET.Vec3 },
	{ "dir", ET.Vec3 },
	{ "num", ET.F32 },
})

local HUD = ET("PA_Hud", ET.ToClients, {
	{ "hp", ET.F32 },
	{ "maxhp", ET.F32 },
	{ "stamina", ET.F32 },
	{ "posture", ET.F32 },
	{ "state", ET.U8 },
	{ "weapon", ET.U8 },
	{ "crit", ET.F32 },
	{ "dash", ET.F32 },
	{ "skill", ET.F32 },
	{ "combo", ET.U8 },
	{ "streak", ET.U8 },
	{ "target", ET.Entity },
	{ "thp", ET.F32 },
	{ "tmaxhp", ET.F32 },
	{ "tposture", ET.F32 },
})

local ACT = {
	ATTACK = 1, CRITICAL = 2, FEINT = 3, BLOCK_ON = 4, BLOCK_OFF = 5, DASH = 6, SKILL = 7,
	SLIDE_ON = 8, SLIDE_OFF = 9, JUMP = 10, WEAPON = 11, RESET = 12,
}

local FLAG = { BLOCK = 1, SLIDE = 2, JUMP = 4 }

local FXK = {
	SWING = 1, HIT = 2, HEAVY_HIT = 3, BLOCKED = 4, PARRY = 5, GUARD_BREAK = 6, DASH = 7,
	PERFECT_DODGE = 8, WINDUP = 9, FEINT = 10, SLIDE = 11, AIR_JUMP = 12, SHOCKWAVE = 13,
	DEATH = 14, GOLEM_POSE = 15, WEAPON = 16, MISSED_PARRY = 17, WALL_RUN = 18, BLOCK_STATE = 19,
	STUN = 20, SHOWCASE = 21, LEAP = 22, ANNOUNCE = 23, SKILL = 24, RESPAWN = 25, WALL_JUMP = 26,
}

local POSE = {
	IDLE = 0, WINDUP = 1, SWING = 2, BLOCK = 3, STUN = 4, PARRY = 5, BROKEN = 6, DEAD = 7,
	DASH = 8, CRIT_WINDUP = 9, SKILL = 10, PARRIED = 11,
}

local ST = { BLOCK = 1, STUN = 2, ATTACK = 4, SLIDE = 8, WALL = 16, COMBAT = 32, DASH = 64, BROKEN = 128 }

local WEAPON_INFO = {
	{ Name = "Fists", Critical = "Smash", Skill = "Barrage" },
	{ Name = "Blade", Critical = "Ichimonji", Skill = "Whirlwind" },
	{ Name = "Rapier", Critical = "Pierce", Skill = "Volley" },
}

local DIFFICULTIES = { "Weak", "Normal", "Hard", "Very Hard", "Impossible" }

------------------------------------------------------------------------------------------
-- UTILITIES
------------------------------------------------------------------------------------------

local V3 = Vector3.new
local ZERO = V3(0, 0, 0)
local UP = V3(0, 1, 0)
local FORWARD = V3(0, 0, -1)
local HIDDEN = V3(0, -2000, 0)

local U = {}

-- Plain functions handed to pcall, so property access allocates no closure per call.
local function rawWrite(inst, key, value)
	inst[key] = value
end

local function rawRead(inst, key)
	return inst[key]
end

function U.try(fn, ...)
	local ok, result = pcall(fn, ...)
	if ok then
		return result
	end
	return nil
end

function U.set(inst, key, value)
	if inst == nil then
		return false
	end
	return (pcall(rawWrite, inst, key, value))
end

function U.get(inst, key)
	if inst == nil then
		return nil
	end
	local ok, value = pcall(rawRead, inst, key)
	if ok then
		return value
	end
	return nil
end

function U.call(inst, method, ...)
	if inst == nil then
		return nil
	end
	local args = table.pack(...)
	local ok, result = pcall(function()
		return inst[method](inst, table.unpack(args, 1, args.n))
	end)
	if ok then
		return result
	end
	return nil
end

function U.alive(inst)
	if inst == nil then
		return false
	end
	local ok, result = pcall(function()
		return inst:IsAlive()
	end)
	if not ok then
		return true
	end
	return result ~= false
end

function U.dot(a, b)
	return a.X * b.X + a.Y * b.Y + a.Z * b.Z
end

function U.cross(a, b)
	return V3(a.Y * b.Z - a.Z * b.Y, a.Z * b.X - a.X * b.Z, a.X * b.Y - a.Y * b.X)
end

function U.len(v)
	return math.sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z)
end

function U.unit(v, fallback)
	local m = U.len(v)
	if m < 1e-5 then
		return fallback or ZERO
	end
	return v / m
end

function U.flat(v)
	return V3(v.X, 0, v.Z)
end

function U.lerp(a, b, t)
	return a + (b - a) * t
end

function U.rand(lo, hi)
	return lo + math.random() * (hi - lo)
end

function U.count(n)
	return math.max(1, math.floor(n * CONFIG.Quality + 0.5))
end

function U.randomUnit()
	local z = U.rand(-1, 1)
	local a = U.rand(0, math.pi * 2)
	local r = math.sqrt(1 - z * z)
	return V3(r * math.cos(a), z, r * math.sin(a))
end

-- Orthonormal frame around a direction: forward, right, up.
function U.basis(dir)
	local f = U.unit(dir, FORWARD)
	local ref = math.abs(f.Y) > 0.95 and V3(1, 0, 0) or UP
	local r = U.unit(U.cross(f, ref), V3(1, 0, 0))
	local u = U.cross(r, f)
	return f, r, u
end

function U.inCone(dir, degrees)
	local f, r, u = U.basis(dir)
	local spread = math.rad(degrees)
	local a = U.rand(0, math.pi * 2)
	local t = math.tan(U.rand(0, spread))
	return U.unit(f + (r * math.cos(a) + u * math.sin(a)) * t, f)
end

-- Orientation (YXZ Euler degrees) pointing -Z along `forward` with +Y toward `up`.
local hasRotation = pcall(function()
	return rotation.to_euler(rotation.look(FORWARD, UP))
end)

function U.orient(forward, up)
	if hasRotation then
		local ok, result = pcall(function()
			return rotation.to_euler(rotation.look(forward, up or UP))
		end)
		if ok then
			return result
		end
	end
	local f = U.unit(forward, FORWARD)
	return V3(math.deg(math.asin(math.clamp(f.Y, -1, 1))), math.deg(math.atan2(-f.X, -f.Z)), 0)
end

function U.ease(kind, a)
	if kind == "out" then
		return 1 - (1 - a) * (1 - a)
	elseif kind == "out3" then
		local b = 1 - a
		return 1 - b * b * b
	elseif kind == "in" then
		return a * a
	end
	return a
end

------------------------------------------------------------------------------------------
-- LOCAL WORLD REFERENCES
------------------------------------------------------------------------------------------

local REF = {
	player = nil,
	userId = nil,
	character = nil,
	camera = nil,
	baseFov = nil,
	baseSide = nil,
	refreshAt = 0,
}

local NEON = "Neon"
pcall(function()
	for _, entry in ipairs(game.Materials.GetMaterials()) do
		local spec = game.Materials.GetMaterial(entry.Id)
		if spec ~= nil and spec.Surface ~= nil and spec.Surface.Neon then
			NEON = entry.Name
			break
		end
	end
end)

local FX_ROOT = U.try(Instance.new, "PA_LocalFx")
if FX_ROOT ~= nil then
	U.call(FX_ROOT, "AddComponent", "Transient")
end

local function refreshRefs(now)
	if now < REF.refreshAt then
		return
	end
	REF.refreshAt = now + 0.5
	REF.player = U.get(game.Players, "LocalPlayer")
	if REF.player ~= nil then
		REF.userId = U.get(REF.player, "UserId")
		REF.character = U.get(REF.player, "Character")
	end
	if REF.character == nil then
		REF.character = U.try(game.Session.LocalCharacter)
	end
	REF.camera = U.get(game.World, "Camera")
	if REF.camera ~= nil and REF.baseFov == nil then
		local lens = U.get(REF.camera, "CameraLens")
		REF.baseFov = lens and U.get(lens, "fovY") or nil
		local orbit = U.get(REF.camera, "CameraOrbit")
		REF.baseSide = orbit and U.get(orbit, "sideOffset") or nil
	end
end

local function isMe(inst)
	if inst == nil or REF.character == nil then
		return false
	end
	if inst == REF.character then
		return true
	end
	local uid = U.get(inst, "UserId")
	return uid ~= nil and REF.userId ~= nil and uid == REF.userId and U.get(inst, "Name") == U.get(REF.character, "Name")
end

local function posOf(inst)
	return U.get(inst, "Position") or ZERO
end

local function forwardOf(inst)
	local look = U.call(inst, "TransformDirection", FORWARD)
	if look ~= nil then
		return U.unit(U.flat(look), FORWARD)
	end
	return FORWARD
end

local function groundBelow(p)
	local hit = U.try(function()
		return game.Physics.Raycast(p + V3(0, 0.5, 0), V3(0, -1, 0), 12)
	end)
	if hit ~= nil then
		return hit.Position
	end
	return p - V3(0, 3, 0)
end

------------------------------------------------------------------------------------------
-- PART POOL + PARTICLES
------------------------------------------------------------------------------------------

local P = {
	pools = { Part = {}, Sphere = {}, Cylinder = {}, Wedge = {} },
	active = {},
	lights = {},
	activeLights = {},
	failures = 0,
	disabled = false,
}

function P.create(shape)
	if P.disabled then
		return nil
	end
	local part = U.try(game.Prefabs.Spawn, shape)
	if part == nil and shape ~= "Part" then
		part = U.try(game.Prefabs.Spawn, "Part")
	end
	if part == nil then
		part = U.try(Instance.new, "Part")
	end
	if part == nil then
		P.failures += 1
		if P.failures > 25 then
			P.disabled = true
			warn("[ParryArena] this client cannot create parts, VFX are off")
		end
		return nil
	end
	P.failures = 0
	U.set(part, "Name", "PA_Fx" .. shape)
	U.set(part, "Parent", FX_ROOT or game.World)
	U.set(part, "Anchored", true)
	U.set(part, "Collider", "None")
	U.set(part, "ShadowOff", true)
	U.set(part, "Transparency", 1)
	U.set(part, "Position", HIDDEN)
	return part
end

function P.acquire(shape)
	local pool = P.pools[shape] or P.pools.Part
	local part = table.remove(pool)
	if part ~= nil then
		return part
	end
	return P.create(shape)
end

function P.release(part, shape)
	U.set(part, "Transparency", 1)
	U.set(part, "Position", HIDDEN)
	table.insert(P.pools[shape] or P.pools.Part, part)
end

--[[
	emit{
		shape, pos, vel, accel, drag, life, delay,
		size | size0 + size1, sizeEase,
		tr0, tr1, fadeIn, color, color1, neon,
		dir + up (orientation), faceVel (orient along velocity),
		to + ease (tween the position instead of simulating it),
		follow = Instance + offset (stick to a moving thing)
	}
]]
function P.emit(o)
	if #P.active >= CONFIG.MaxParticles then
		return nil
	end
	local shape = o.shape or "Part"
	local part = P.acquire(shape)
	if part == nil then
		return nil
	end
	local size0 = o.size0 or o.size or V3(1, 1, 1)
	local p = {
		part = part,
		shape = shape,
		t = -(o.delay or 0),
		life = math.max(o.life or 0.3, 0.01),
		pos = o.pos or ZERO,
		from = o.pos or ZERO,
		to = o.to,
		ease = o.ease or "out",
		vel = o.vel or ZERO,
		accel = o.accel or ZERO,
		drag = o.drag or 0,
		size0 = size0,
		size1 = o.size1 or size0,
		sizeEase = o.sizeEase or "linear",
		tr0 = o.tr0 or 0,
		tr1 = o.tr1 or 1,
		fadeIn = o.fadeIn or 0,
		color0 = o.color or V3(1, 1, 1),
		color1 = o.color1,
		faceVel = o.faceVel,
		up = o.up or UP,
		follow = o.follow,
		offset = o.offset,
		spin = o.spin,
		spinAxis = o.spinAxis,
		angle = 0,
	}
	U.set(part, "Color", p.color0)
	U.set(part, "Material", o.neon == false and "Plastic" or NEON)
	U.set(part, "Size", size0)
	if o.dir ~= nil then
		U.set(part, "Orientation", U.orient(o.dir, p.up))
		p.dir = o.dir
	elseif o.orientation ~= nil then
		U.set(part, "Orientation", o.orientation)
	end
	if p.t >= 0 then
		U.set(part, "Position", p.pos)
		U.set(part, "Transparency", p.fadeIn > 0 and 1 or p.tr0)
	end
	table.insert(P.active, p)
	return p
end

-- One particle, one frame. Called through pcall so a broken part only loses itself.
local function stepParticle(p, dt)
	local a = p.t / p.life
	if p.to ~= nil then
		p.pos = U.lerp(p.from, p.to, U.ease(p.ease, a))
	else
		if p.drag > 0 then
			p.vel = p.vel * math.exp(-p.drag * dt)
		end
		p.vel = p.vel + p.accel * dt
		p.pos = p.pos + p.vel * dt
	end
	local world = p.pos
	if p.follow ~= nil then
		world = p.follow.Position + (p.offset or ZERO) + (p.pos - p.from)
	end
	local part = p.part
	part.Position = world
	part.Size = U.lerp(p.size0, p.size1, U.ease(p.sizeEase, a))
	local tr = p.tr0 + (p.tr1 - p.tr0) * a
	if p.fadeIn > 0 and p.t < p.fadeIn then
		tr = 1 - (1 - tr) * (p.t / p.fadeIn)
	end
	part.Transparency = math.clamp(tr, 0, 1)
	if p.color1 ~= nil then
		part.Color = U.lerp(p.color0, p.color1, a)
	end
	if p.faceVel and U.len(p.vel) > 0.5 then
		part.Orientation = U.orient(p.vel, UP)
	elseif p.spin ~= nil and p.dir ~= nil then
		p.angle += p.spin * dt
		local axis = p.spinAxis or p.dir
		local _, r, u = U.basis(axis)
		local spun = u * math.cos(p.angle) + r * math.sin(p.angle)
		part.Orientation = U.orient(axis, spun)
	end
end

function P.step(dt)
	local list = P.active
	local i = 1
	while i <= #list do
		local p = list[i]
		p.t += dt
		local done = p.t >= p.life
		if not done and p.t >= 0 then
			done = not pcall(stepParticle, p, dt)
		end
		if done then
			P.release(p.part, p.shape)
			list[i] = list[#list]
			list[#list] = nil
		else
			i += 1
		end
	end

	-- lights
	local lights = P.activeLights
	local j = 1
	while j <= #lights do
		local l = lights[j]
		l.t += dt
		if l.t >= l.life then
			U.set(l.inst, "Intensity", 0)
			U.set(l.inst, "Position", HIDDEN)
			table.insert(P.lights, l.inst)
			lights[j] = lights[#lights]
			lights[#lights] = nil
		else
			U.set(l.inst, "Intensity", l.intensity * (1 - l.t / l.life))
			if l.follow ~= nil then
				U.set(l.inst, "Position", posOf(l.follow))
			end
			j += 1
		end
	end
end

function P.light(pos, color, intensity, range, life, follow)
	if #P.activeLights >= 12 then
		return
	end
	local inst = table.remove(P.lights)
	if inst == nil then
		inst = U.try(game.Prefabs.Spawn, "PointLight")
		if inst == nil then
			return
		end
		U.set(inst, "Parent", FX_ROOT or game.World)
		U.set(inst, "Anchored", true)
		U.set(inst, "Collider", "None")
		U.set(inst, "Transparency", 1)
	end
	U.set(inst, "Position", pos)
	U.set(inst, "LightColor", color)
	U.set(inst, "Range", range)
	U.set(inst, "Intensity", intensity)
	table.insert(P.activeLights, { inst = inst, t = 0, life = life, intensity = intensity, follow = follow })
end

------------------------------------------------------------------------------------------
-- VFX BUILDING BLOCKS
------------------------------------------------------------------------------------------

local C = {
	white = Color3.fromRGB(255, 255, 255),
	grey = Color3.fromRGB(120, 120, 120),
	ink = Color3.fromRGB(37, 37, 37),
	hitRed = Color3.fromRGB(255, 73, 73),
	hitDark = Color3.fromRGB(78, 21, 21),
	hitDeep = Color3.fromRGB(180, 31, 31),
	red = Color3.fromRGB(255, 0, 0),
	amber = Color3.fromRGB(255, 170, 0),
	parry = Color3.fromRGB(255, 176, 106),
	parry2 = Color3.fromRGB(255, 167, 69),
	block = Color3.fromRGB(84, 170, 255),
	blockDeep = Color3.fromRGB(0, 84, 255),
	breakRed = Color3.fromRGB(255, 52, 52),
	breakShard = Color3.fromRGB(255, 92, 92),
	purple = Color3.fromRGB(170, 0, 255),
	pink = Color3.fromRGB(255, 170, 255),
	yellow = Color3.fromRGB(255, 198, 24),
	blood = Color3.fromRGB(158, 18, 23),
	smoke = Color3.fromRGB(157, 151, 157),
	rock = Color3.fromRGB(161, 161, 161),
	dirt = Color3.fromRGB(65, 41, 4),
	stone = Color3.fromRGB(150, 150, 160),
	teal = Color3.fromRGB(63, 183, 168),
}

local V = {}
local E = {}
local UI = {}
local GRAVITY = V3(0, -60, 0) -- particles fall slower than characters; reads better

-- Expanding sphere ("Glow", "Impact", "Flash" textures in the pack).
function V.flash(pos, color, s0, s1, life, tr0, delay)
	P.emit({
		shape = "Sphere",
		pos = pos,
		size0 = V3(s0, s0, s0),
		size1 = V3(s1, s1, s1),
		sizeEase = "out3",
		life = life,
		tr0 = tr0 or 0.1,
		tr1 = 1,
		color = color,
		delay = delay,
	})
end

-- Ring of segments that expands from r0 to r1 ("Ripple", "Shockwave", "impactframe").
function V.ring(centre, normal, r0, r1, life, color, o)
	o = o or {}
	local segments = o.segments or 16
	local f, r, u = U.basis(normal)
	local width = o.width or 0.25
	local thickness = o.thickness or 0.08
	for i = 1, segments do
		local a = (i / segments) * math.pi * 2
		local radial = r * math.cos(a) + u * math.sin(a)
		local tangent = U.cross(f, radial)
		local l0 = (2 * math.pi * r0) / segments + 0.05
		local l1 = (2 * math.pi * r1) / segments + 0.05
		P.emit({
			shape = "Part",
			pos = centre + radial * r0,
			to = centre + radial * r1 + (o.lift or ZERO),
			ease = o.ease or "out3",
			size0 = V3(width, thickness, l0),
			size1 = V3(width * (o.widthEnd or 0.3), thickness, l1),
			sizeEase = o.ease or "out3",
			life = life,
			tr0 = o.tr0 or 0.05,
			tr1 = 1,
			color = color,
			color1 = o.color1,
			dir = tangent,
			up = f,
			delay = o.delay,
		})
	end
end

-- Thin streaks flying outward ("Lines", "line4x4").
function V.streaks(pos, dir, count, o)
	o = o or {}
	for _ = 1, U.count(count) do
		local d = o.cone and U.inCone(dir, o.cone) or U.randomUnit()
		local speed = U.rand(o.speed and o.speed[1] or 20, o.speed and o.speed[2] or 30)
		local length = U.rand(o.length and o.length[1] or 1.5, o.length and o.length[2] or 3)
		local width = o.width or 0.12
		P.emit({
			shape = "Part",
			pos = pos + d * (o.start or 0.3),
			vel = d * speed,
			drag = o.drag or 6,
			accel = o.gravity and GRAVITY * o.gravity or ZERO,
			size0 = V3(width, width, length),
			size1 = V3(width * 0.4, width * 0.4, length * 0.15),
			life = U.rand(o.life and o.life[1] or 0.12, o.life and o.life[2] or 0.3),
			tr0 = o.tr0 or 0,
			tr1 = 1,
			color = o.color or C.white,
			color1 = o.color1,
			dir = d,
			faceVel = o.gravity ~= nil,
			delay = o.delay,
		})
	end
end

-- Small chunks ("Cubes", "Dots", "Shards", "Drops", "Rock").
function V.bits(pos, dir, count, o)
	o = o or {}
	for _ = 1, U.count(count) do
		local d = o.cone and U.inCone(dir, o.cone) or U.randomUnit()
		if o.upward then
			d = U.unit(d + V3(0, o.upward, 0), d)
		end
		local s = U.rand(o.size and o.size[1] or 0.25, o.size and o.size[2] or 0.45)
		local size = o.stretch and V3(s * 0.5, s * 0.5, s * o.stretch) or V3(s, s, s)
		P.emit({
			shape = o.shape or "Part",
			pos = pos + d * (o.start or 0.2),
			vel = d * U.rand(o.speed and o.speed[1] or 8, o.speed and o.speed[2] or 16),
			drag = o.drag or 2,
			accel = GRAVITY * (o.gravity or 0),
			size0 = size,
			size1 = size * (o.shrink or 0.1),
			life = U.rand(o.life and o.life[1] or 0.3, o.life and o.life[2] or 0.6),
			tr0 = o.tr0 or 0,
			tr1 = o.tr1 or 1,
			color = o.color or C.white,
			color1 = o.color1,
			dir = d,
			faceVel = o.stretch ~= nil,
			spin = o.spin,
			neon = o.neon,
			delay = o.delay,
		})
	end
end

-- An arc of slash segments swept around `centre` ("Swing VFX" / "Cresent" textures).
--   tilt: radians the slash plane is rolled around forward (0 flat, pi/2 vertical)
--   sweep: +1 right->left, -1 left->right
function V.arc(centre, forward, tilt, sweep, radius, color, o)
	o = o or {}
	local f = U.unit(U.flat(forward), FORWARD)
	local r = U.cross(f, UP)
	local rr = r * math.cos(tilt) + UP * math.sin(tilt)
	local normal = U.cross(rr, f)
	local span = math.rad(o.span or 150)
	local segments = o.segments or 14
	local sweepTime = o.sweepTime or 0.08
	local width = o.width or 0.5
	for i = 0, segments - 1 do
		local k = i / (segments - 1)
		local phi = (sweep > 0) and (span / 2 - k * span) or (-span / 2 + k * span)
		local radial = f * math.cos(phi) + rr * math.sin(phi)
		local tangent = (f * -math.sin(phi) + rr * math.cos(phi)) * sweep
		local chord = 2 * radius * math.sin(span / segments / 2) + 0.1
		local taper = math.sin(k * math.pi) * 0.75 + 0.25
		P.emit({
			shape = "Part",
			pos = centre + radial * radius,
			to = centre + radial * (radius + (o.grow or 0.6)),
			ease = "out",
			size0 = V3(width * taper, 0.06, chord),
			size1 = V3(width * taper * 0.2, 0.04, chord),
			life = o.life or 0.2,
			delay = k * sweepTime,
			tr0 = o.tr0 or 0.05,
			tr1 = 1,
			color = color,
			color1 = o.color1,
			dir = tangent,
			up = normal,
		})
	end
end

-- Ghost silhouette left behind by dashes and dodges ("Flashstep" / afterimages).
function V.afterimage(pos, facing, color, life, tr0)
	local f = U.unit(U.flat(facing), FORWARD)
	local parts = {
		{ V3(0, 1.5, 0), V3(1.15, 1.15, 1.15), "Sphere" },
		{ V3(0, 0, 0), V3(2, 2, 1), "Part" },
		{ V3(-1.5, 0, 0), V3(0.9, 2, 0.9), "Part" },
		{ V3(1.5, 0, 0), V3(0.9, 2, 0.9), "Part" },
		{ V3(-0.5, -2, 0), V3(0.95, 2, 0.95), "Part" },
		{ V3(0.5, -2, 0), V3(0.95, 2, 0.95), "Part" },
	}
	local r = U.cross(f, UP)
	for _, item in ipairs(parts) do
		local offset = r * item[1].X + UP * item[1].Y - f * item[1].Z
		P.emit({
			shape = item[3],
			pos = pos + offset,
			size = item[2],
			size1 = item[2] * 1.06,
			life = life,
			tr0 = tr0 or 0.35,
			tr1 = 1,
			color = color,
			dir = f,
		})
	end
end

-- Floating text (damage numbers, PARRY!, etc.), built on the Billboard component.
local TEXT_POOL = {}
local ACTIVE_TEXT = {}

function V.text(pos, text, color, o)
	o = o or {}
	local part = table.remove(TEXT_POOL)
	if part == nil then
		part = P.create("Part")
		if part == nil then
			return
		end
		U.set(part, "Size", V3(0.1, 0.1, 0.1))
		U.call(part, "AddComponent", "Billboard")
		U.call(part, "AddComponent", "BillboardOnTop")
	end
	U.set(part, "BillboardText", text)
	U.set(part, "BillboardColor", color)
	U.set(part, "BillboardWorldSize", o.size or 1.4)
	U.set(part, "BillboardMaxDistance", 150)
	U.set(part, "BillboardOffset", ZERO)
	U.set(part, "Position", pos)
	U.set(part, "Transparency", 1)
	table.insert(ACTIVE_TEXT, {
		part = part,
		t = 0,
		start = pos,
		rise = o.rise or 1.7,
		riseTime = o.riseTime or 0.22,
		fall = o.fall or 3.2,
		fallTime = o.fallTime or 0.75,
		size = o.size or 1.4,
		drift = V3(U.rand(-1, 1), 0, U.rand(-1, 1)) * (o.spread or 1.3),
	})
end

function V.stepText(dt)
	local i = 1
	while i <= #ACTIVE_TEXT do
		local item = ACTIVE_TEXT[i]
		item.t += dt
		local total = item.riseTime + item.fallTime
		if item.t >= total then
			U.set(item.part, "Position", HIDDEN)
			U.set(item.part, "BillboardText", "")
			table.insert(TEXT_POOL, item.part)
			ACTIVE_TEXT[i] = ACTIVE_TEXT[#ACTIVE_TEXT]
			ACTIVE_TEXT[#ACTIVE_TEXT] = nil
		else
			-- Pops up, then falls and shrinks (the original damage indicator's rise/fall).
			local y
			local scale = 1
			if item.t < item.riseTime then
				y = item.rise * U.ease("out", item.t / item.riseTime)
			else
				local b = (item.t - item.riseTime) / item.fallTime
				y = item.rise - (item.rise + item.fall) * U.ease("in", b) * 0.6
				scale = 1 - b * 0.7
			end
			local spread = item.drift * math.min(item.t / item.riseTime, 1)
			U.set(item.part, "Position", item.start + V3(0, y, 0) + spread)
			U.set(item.part, "BillboardWorldSize", item.size * scale)
			i += 1
		end
	end
end

------------------------------------------------------------------------------------------
-- CAMERA PUNCH
------------------------------------------------------------------------------------------

local CAM = { kick = 0, shake = 0, shakeTime = 0, kicking = false, shaking = false, side = nil }

local function punch(fovKick, shake)
	fovKick = fovKick or 0
	if fovKick >= 0 then
		CAM.kick = math.max(CAM.kick, fovKick)
	else
		CAM.kick = math.min(CAM.kick, fovKick)
	end
	if CONFIG.CameraShake then
		CAM.shake = math.max(CAM.shake, shake or 0)
	end
end

-- Only touches the camera while an effect is running, then hands it back untouched, so
-- shift lock and the player's own settings keep working.
local function stepCamera(dt)
	local cam = REF.camera
	if cam == nil then
		return
	end
	CAM.kick = CAM.kick * math.exp(-10 * dt)
	CAM.shake = CAM.shake * math.exp(-9 * dt)
	CAM.shakeTime += dt

	local lens = U.get(cam, "CameraLens")
	if lens ~= nil then
		if math.abs(CAM.kick) > 0.05 then
			if not CAM.kicking then
				CAM.kicking = true
				REF.baseFov = U.get(lens, "fovY") or REF.baseFov
			end
			if REF.baseFov ~= nil then
				U.set(lens, "fovY", REF.baseFov + math.rad(CAM.kick))
			end
		elseif CAM.kicking then
			CAM.kicking = false
			if REF.baseFov ~= nil then
				U.set(lens, "fovY", REF.baseFov)
			end
		end
	end

	local orbit = U.get(cam, "CameraOrbit")
	if orbit ~= nil then
		if CAM.shake > 0.02 then
			if not CAM.shaking then
				CAM.shaking = true
				CAM.side = U.get(orbit, "sideOffset")
			end
			if CAM.side ~= nil then
				local wobble = math.sin(CAM.shakeTime * 61) * 0.6 + math.sin(CAM.shakeTime * 37) * 0.4
				U.set(orbit, "sideOffset", CAM.side + wobble * CAM.shake)
			end
		elseif CAM.shaking then
			CAM.shaking = false
			if CAM.side ~= nil then
				U.set(orbit, "sideOffset", CAM.side)
			end
		end
	end
end

------------------------------------------------------------------------------------------
-- EFFECTS (one per gameplay event)
------------------------------------------------------------------------------------------

-- Hit VFX #1 (white) / #2 (red + amber): impact, winds, shockwaves, lines, cubes.
function E.hit(pos, dir, amount, attacker, victim)
	local hitDir = U.unit(dir, FORWARD)
	local at = pos + V3(0, 0.4, 0) - hitDir * 0.6
	local mine = isMe(attacker) or isMe(victim)
	V.flash(at, C.white, 0.6, 3.2, 0.09, 0.05)
	V.flash(at, C.ink, 1.2, 3.6, 0.12, 0.6)
	V.ring(at, hitDir, 0.6, 3.4, 0.17, C.white, { segments = 14, width = 0.3 })
	V.ring(at, hitDir, 0.3, 2.4, 0.12, C.grey, { segments = 10, width = 0.2, delay = 0.02 })
	V.streaks(at, hitDir, 8, { cone = 70, speed = { 24, 32 }, length = { 1.6, 3.4 }, life = { 0.12, 0.3 } })
	V.bits(at, hitDir, 6, { cone = 80, speed = { 24, 32 }, size = { 0.25, 0.41 }, life = { 0.06, 0.3 }, drag = 5 })
	P.light(at, C.white, 3, 12, 0.14)
	if CONFIG.DamageNumbers and amount > 0 then
		V.text(pos + V3(0, 2.4, 0), string.format("%.1f", amount), mine and C.white or C.parry, { size = 1.2 })
	end
	if mine then
		punch(2.5, 0.25)
	end
end

-- Hard Hit VFX #1/#3 (red) with a white core, Drops, and the Blood Hit layer.
function E.heavyHit(pos, dir, amount, attacker, victim)
	local hitDir = U.unit(dir, FORWARD)
	local at = pos + V3(0, 0.4, 0) - hitDir * 0.5
	V.flash(at, C.white, 1, 6.4, 0.1, 0)
	V.flash(at, C.hitRed, 2, 9.4, 0.2, 0.25, 0.02)
	V.flash(at, C.hitDeep, 3, 8.4, 0.07, 0.5)
	V.ring(at, hitDir, 0.8, 6, 0.22, C.hitRed, { segments = 20, width = 0.45, color1 = C.hitDark })
	V.ring(at, hitDir, 0.6, 4.2, 0.16, C.white, { segments = 16, width = 0.3, delay = 0.03 })
	V.streaks(at, hitDir, 14, { cone = 85, speed = { 26, 40 }, length = { 2.4, 5.5 }, width = 0.16, life = { 0.12, 0.34 }, color = C.white, color1 = C.hitRed })
	V.bits(at, hitDir, 15, { cone = 100, speed = { 15, 30 }, size = { 0.2, 0.5 }, life = { 0.35, 0.4 }, gravity = 0.6, stretch = 3, color = C.white })
	if CONFIG.Blood then
		V.bits(at, hitDir, 10, { cone = 60, speed = { 10, 22 }, size = { 0.25, 0.55 }, life = { 0.35, 0.55 }, gravity = 1.4, shape = "Sphere", color = C.blood, neon = false, shrink = 0.4 })
	end
	P.light(at, C.hitRed, 6, 18, 0.25)
	if CONFIG.DamageNumbers and amount > 0 then
		V.text(pos + V3(0, 2.8, 0), string.format("%.1f", amount), C.hitRed, { size = 1.7, rise = 2.2 })
	end
	if isMe(attacker) or isMe(victim) then
		punch(6, 0.9)
	end
end

-- Block VFX: blue shield, impact frames, Drops.
function E.blocked(pos, dir, posture, victim)
	local toAttacker = U.unit(dir, FORWARD)
	local at = pos + toAttacker * 1.4 + V3(0, 0.6, 0)
	local f, r, u = U.basis(toAttacker)
	P.emit({
		shape = "Part",
		pos = at,
		size0 = V3(2.4, 2.4, 0.08),
		size1 = V3(3.4, 3.4, 0.08),
		sizeEase = "out3",
		life = 0.4,
		tr0 = 0.35,
		tr1 = 1,
		color = C.block,
		dir = toAttacker,
		up = U.unit(u + r, u),
	})
	for i = 0, 2 do
		V.ring(at, toAttacker, 0.5, 2.5, 0.6 - i * 0.12, C.blockDeep, { segments = 12, width = 0.18, delay = i * 0.05, tr0 = 0.3 })
	end
	V.bits(at, toAttacker, 15, { cone = 70, speed = { 15, 30 }, size = { 0.2, 0.5 }, life = { 0.35, 0.4 }, gravity = 0.8, stretch = 3, color = C.block })
	P.light(at, C.block, 3, 12, 0.2)
	if isMe(victim) then
		punch(1.5, 0.2)
	end
	if posture ~= nil and posture >= 70 then
		V.text(pos + V3(0, 3, 0), "GUARD " .. math.floor(posture) .. "%", C.breakShard, { size = 0.9, rise = 1, fall = 0.6 })
	end
end

-- Parry #1 + #2 (orange): glow, ripples, star lines, dots, cubes, crescents.
function E.parry(pos, dir, streak, attacker, parrier)
	local at = pos + V3(0, 0.6, 0)
	local d = U.unit(dir, FORWARD)
	V.flash(at, C.white, 0.8, 5, 0.12, 0)
	V.flash(at, C.parry, 2, 7, 0.45, 0.2)
	V.ring(at, d, 0.6, 5.2, 0.25, C.parry, { segments = 20, width = 0.4 })
	V.ring(at, d, 0.4, 3.4, 0.25, C.parry2, { segments = 16, width = 0.3, delay = 0.06 })
	V.ring(at, UP, 1, 6.5, 0.45, C.parry, { segments = 22, width = 0.22, tr0 = 0.3 })
	V.streaks(at, d, 8, { speed = { 18, 48 }, length = { 3, 5.9 }, width = 0.14, life = { 0.15, 0.35 }, color = C.parry, drag = 7 })
	V.streaks(at, d, 10, { speed = { 12, 20 }, length = { 0.8, 1.6 }, width = 0.1, life = { 0.25, 0.65 }, color = C.parry2, drag = 3 })
	V.bits(at, d, 15, { speed = { 3, 12 }, size = { 0.15, 0.38 }, life = { 0.25, 1.25 }, gravity = 0.25, shape = "Sphere", color = C.parry, drag = 2.4 })
	V.bits(at, d, 5, { speed = { 8, 16 }, size = { 0.25, 0.35 }, life = { 0.35, 0.5 }, color = C.parry2, spin = 12 })
	V.arc(at, d, 0.4, 1, 2.6, C.parry, { span = 120, segments = 10, life = 0.5, width = 0.7, grow = 1.4, sweepTime = 0.03 })
	V.arc(at, -d, -0.4, -1, 2.6, C.parry2, { span = 120, segments = 10, life = 0.5, width = 0.7, grow = 1.4, sweepTime = 0.03 })
	P.light(at, C.parry, 9, 22, 0.35)
	local word = (streak ~= nil and streak > 1) and ("PARRY x" .. math.floor(streak)) or "PARRY!"
	V.text(at + V3(0, 2.6, 0), word, C.parry, { size = 1.8, rise = 1.2, fall = 0.4, fallTime = 0.6 })
	if isMe(attacker) or isMe(parrier) then
		punch(7, 1)
	end
end

-- Block Break VFX: red shield shattering into crescent shards.
function E.guardBreak(pos, dir, victim)
	local d = U.unit(dir, FORWARD)
	local at = pos + V3(0, 0.6, 0) - d * 1.2
	V.flash(at, C.white, 1, 5, 0.1, 0)
	V.flash(at, C.breakRed, 2, 8, 0.3, 0.3)
	P.emit({ shape = "Part", pos = at, size0 = V3(2.7, 2.7, 0.1), size1 = V3(3.4, 3.4, 0.1), life = 1.1, tr0 = 0.2, tr1 = 1, color = C.breakRed, dir = -d, up = U.unit(UP + U.cross(d, UP), UP) })
	V.bits(at, -d, 25, { cone = 120, speed = { 10, 26 }, size = { 0.5, 1.25 }, life = { 0.4, 0.8 }, gravity = 1, shape = "Wedge", color = C.breakShard, spin = 14 })
	V.ring(at, -d, 0.5, 5, 0.3, C.breakRed, { segments = 18, width = 0.4 })
	V.ring(groundBelow(pos) + V3(0, 0.1, 0), UP, 1, 7, 0.4, C.breakRed, { segments = 22, width = 0.3, tr0 = 0.3 })
	P.light(at, C.breakRed, 7, 18, 0.4)
	V.text(at + V3(0, 3, 0), "GUARD BREAK", C.breakRed, { size = 1.7, rise = 1.2, fall = 0.5 })
	if isMe(victim) then
		punch(5, 0.8)
	end
end

-- Perfect Block VFX (purple): impact frames, Flys, Shards, white-purple core.
function E.perfectDodge(pos, dir, dodger)
	local at = pos + V3(0, 0.5, 0)
	V.flash(at, C.pink, 1, 4, 0.2, 0.1)
	V.flash(at, C.purple, 2, 6, 0.4, 0.4)
	V.ring(at, UP, 0.6, 5, 0.6, C.purple, { segments = 20, width = 0.35 })
	V.ring(at, U.unit(dir, FORWARD), 0.6, 4, 0.6, C.pink, { segments = 16, width = 0.3, delay = 0.05, color1 = C.white })
	V.bits(at, UP, 25, { speed = { 7, 20 }, size = { 0.4, 1 }, life = { 0.4, 0.6 }, color = C.purple, stretch = 2.5 })
	V.bits(at, UP, 25, { speed = { 7, 20 }, size = { 0.2, 0.5 }, life = { 0.4, 0.7 }, shape = "Wedge", color = C.purple, spin = 10 })
	V.afterimage(pos, dir, C.purple, 0.6, 0.3)
	P.light(at, C.purple, 7, 18, 0.5)
	V.text(at + V3(0, 3, 0), "PERFECT DODGE", C.pink, { size = 1.5, rise = 1.4 })
	if isMe(dodger) then
		punch(-4, 0.4)
		UI.screenFlash(C.purple, 0.55, 0.45)
	end
end

-- Flashstep + Smoke + Dash Smoke: ground puff, crescent, black/white afterimages.
local DASHES = {} -- active dash trails: { inst, until, nextAt, facing }

function E.dash(inst, pos, dir)
	local feet = groundBelow(pos)
	local d = U.unit(dir, FORWARD)
	for _ = 1, U.count(14) do
		local a = U.rand(0, math.pi * 2)
		local radial = V3(math.cos(a), 0, math.sin(a))
		P.emit({
			shape = "Sphere",
			pos = feet + radial * 0.5 + V3(0, 0.3, 0),
			vel = (radial * U.rand(10, 18) - d * 6) + V3(0, U.rand(0, 3), 0),
			drag = 4,
			size0 = V3(1, 1, 1) * U.rand(0.8, 1.4),
			size1 = V3(1, 1, 1) * U.rand(2.5, 4),
			life = U.rand(0.5, 1),
			tr0 = 0.45,
			tr1 = 1,
			color = C.smoke,
			neon = false,
		})
	end
	V.ring(feet + V3(0, 0.15, 0), UP, 0.8, 4.5, 0.4, C.white, { segments = 16, width = 0.35, tr0 = 0.4 })
	V.arc(pos - d * 0.5, -d, 0, 1, 1.8, C.white, { span = 160, segments = 10, life = 0.3, width = 0.5, grow = 1, sweepTime = 0.02 })
	V.flash(pos, C.white, 1.5, 3, 0.12, 0.5)
	table.insert(DASHES, { inst = inst, untilAt = tick() + 0.26, nextAt = 0, dir = d, toggle = false })
end

function E.stepDashes(now)
	local i = 1
	while i <= #DASHES do
		local item = DASHES[i]
		if now >= item.untilAt or not U.alive(item.inst) then
			DASHES[i] = DASHES[#DASHES]
			DASHES[#DASHES] = nil
		else
			if now >= item.nextAt then
				item.nextAt = now + 0.035
				item.toggle = not item.toggle
				V.afterimage(posOf(item.inst), item.dir, item.toggle and C.white or C.ink, 0.35, item.toggle and 0.45 or 0.6)
			end
			i += 1
		end
	end
end

-- Movement trails (Trail #1 / #2): a ribbon of fading segments behind a moving body.
local TRAILS = {} -- { inst, untilAt, last, color, width }

function E.trail(inst, duration, color, width)
	table.insert(TRAILS, { inst = inst, untilAt = tick() + duration, last = nil, color = color or C.white, width = width or 0.5 })
end

function E.stepTrails(now)
	local i = 1
	while i <= #TRAILS do
		local item = TRAILS[i]
		if now >= item.untilAt or not U.alive(item.inst) then
			TRAILS[i] = TRAILS[#TRAILS]
			TRAILS[#TRAILS] = nil
		else
			local p = posOf(item.inst) + V3(0, 0.2, 0)
			if item.last ~= nil then
				local seg = p - item.last
				local length = U.len(seg)
				if length > 0.4 then
					P.emit({
						shape = "Part",
						pos = (p + item.last) / 2,
						size0 = V3(item.width, 0.06, length + 0.1),
						size1 = V3(0.02, 0.04, length + 0.1),
						life = 0.3,
						tr0 = 0.25,
						tr1 = 1,
						fadeIn = 0.03,
						color = item.color,
						dir = seg,
						up = UP,
					})
					item.last = p
				end
			else
				item.last = p
			end
			i += 1
		end
	end
end

-- Swing VFX #1-#4: one slash arc per combo step.
local SWING_SHAPES = {
	[1] = { tilt = math.rad(18), sweep = 1 },
	[2] = { tilt = math.rad(-18), sweep = -1 },
	[3] = { tilt = math.rad(55), sweep = 1 },
	[4] = { tilt = math.rad(88), sweep = -1 },
}

local WEAPON_BY_INST = {} -- name/key -> weapon index (for swing styling)

local function weaponIndexFor(inst)
	local key = U.get(inst, "Name") or "?"
	local w = WEAPON_BY_INST[key]
	if w == nil then
		local ok, attrWeapon = pcall(function()
			return inst.attr.PA_Weapon
		end)
		if ok and type(attrWeapon) == "number" then
			w = attrWeapon
		end
	end
	return w or 1
end

function E.swing(inst, pos, dir, step)
	local f = U.unit(U.flat(dir), FORWARD)
	local weapon = weaponIndexFor(inst)
	local centre = pos + f * 0.6 + V3(0, 0.4, 0)
	if step == 5 then
		-- Critical: a huge two-layer slash with an amber edge.
		V.arc(centre, f, math.rad(30), 1, 4.5, C.white, { span = 200, segments = 22, width = 1.1, life = 0.32, grow = 1.6, sweepTime = 0.07, color1 = C.amber })
		V.arc(centre + V3(0, 0.3, 0), f, math.rad(36), 1, 3.6, C.hitRed, { span = 170, segments = 16, width = 0.6, life = 0.3, grow = 1.2, sweepTime = 0.08 })
		V.streaks(centre + f * 2, f, 10, { cone = 25, speed = { 30, 50 }, length = { 3, 6 }, life = { 0.12, 0.25 }, color = C.white })
		P.light(centre + f * 2, C.amber, 5, 16, 0.25)
		return
	end
	if step == 6 or step == 7 then
		if weapon == 2 then
			-- Whirlwind: full circle sweeps.
			V.arc(centre, f, math.rad(step == 6 and 8 or -8), step == 6 and 1 or -1, 4, C.white, { span = 340, segments = 26, width = 0.8, life = 0.26, grow = 1, sweepTime = 0.12 })
		elseif weapon == 3 then
			-- Volley: straight thrust streaks.
			local _, r, u = U.basis(f)
			local offset = r * U.rand(-0.8, 0.8) + u * U.rand(-0.5, 0.6)
			V.streaks(centre + offset, f, 3, { cone = 4, speed = { 40, 60 }, length = { 4, 7 }, width = 0.14, life = { 0.1, 0.16 }, start = 0.8 })
			V.flash(centre + offset + f * 6, C.white, 0.3, 1.6, 0.08, 0.2)
		else
			-- Barrage: quick punches popping in front.
			local _, r, u = U.basis(f)
			local spot = centre + f * U.rand(2, 3.5) + r * U.rand(-1.2, 1.2) + u * U.rand(-0.6, 0.9)
			V.flash(spot, C.white, 0.4, 2.2, 0.08, 0.1)
			V.ring(spot, f, 0.3, 1.6, 0.12, C.white, { segments = 10, width = 0.15 })
			V.streaks(spot - f * 2, f, 2, { cone = 6, speed = { 30, 40 }, length = { 2, 3 }, life = { 0.08, 0.12 } })
		end
		return
	end
	local shape = SWING_SHAPES[step] or SWING_SHAPES[1]
	if weapon == 3 then
		-- Rapier M1s are thrusts.
		V.streaks(centre, f, 4, { cone = 3, speed = { 45, 60 }, length = { 5, 8 }, width = 0.12, life = { 0.1, 0.15 }, start = 0.5 })
		V.ring(centre + f * 5.5, f, 0.2, 1.6, 0.14, C.white, { segments = 10, width = 0.12 })
		return
	end
	local radius = weapon == 2 and 4.2 or 3.2
	local width = weapon == 2 and 0.8 or 0.55
	V.arc(centre, f, shape.tilt, shape.sweep, radius, C.white, { span = weapon == 2 and 170 or 140, segments = 14, width = width, life = 0.22, grow = 0.7 })
	if step == 4 then
		V.arc(centre, f, shape.tilt, shape.sweep, radius * 0.75, C.grey, { span = 120, segments = 10, width = width * 0.6, life = 0.2, grow = 0.4, sweepTime = 0.1 })
	end
end

-- Telegraph for a critical: a red ember swelling in front of the attacker.
function E.windup(inst, pos, dir, kind)
	if kind ~= 2 then
		return
	end
	local f = U.unit(U.flat(dir), FORWARD)
	P.emit({
		shape = "Sphere",
		pos = pos + f * 1.4 + V3(0, 0.6, 0),
		follow = inst,
		offset = f * 1.4 + V3(0, 0.6, 0),
		size0 = V3(0.3, 0.3, 0.3),
		size1 = V3(1.8, 1.8, 1.8),
		sizeEase = "in",
		life = 0.42,
		tr0 = 0.4,
		tr1 = 0.1,
		color = C.hitRed,
		color1 = C.amber,
	})
	P.light(pos + f * 1.4, C.hitRed, 4, 12, 0.42, inst)
end

function E.feint(pos)
	V.flash(pos + V3(0, 0.6, 0), C.pink, 1.4, 0.2, 0.18, 0.3)
	V.bits(pos + V3(0, 0.6, 0), UP, 8, { speed = { 4, 9 }, size = { 0.12, 0.25 }, life = { 0.2, 0.4 }, color = C.pink })
	V.text(pos + V3(0, 3.2, 0), "FEINT", C.pink, { size = 1, rise = 0.8, fall = 0.4 })
end

function E.missedParry(pos)
	V.bits(pos + V3(0, 1, 0), UP, 6, { speed = { 3, 6 }, size = { 0.15, 0.3 }, life = { 0.3, 0.5 }, color = C.grey, gravity = 0.5 })
	V.text(pos + V3(0, 3.2, 0), "MISTIMED", C.grey, { size = 1, rise = 0.6, fall = 0.4 })
end

-- Misc. Shockwave + Rock Debris (+ Dirt).
function E.shockwave(pos, scale)
	local ground = groundBelow(pos) + V3(0, 0.15, 0)
	scale = scale or 1
	V.ring(ground, UP, 1, 9 * scale, 0.45, C.white, { segments = 24, width = 0.6, color1 = C.grey })
	V.ring(ground, UP, 0.5, 6 * scale, 0.35, C.white, { segments = 18, width = 0.3, delay = 0.05 })
	V.flash(ground + V3(0, 0.5, 0), C.white, 1, 4 * scale, 0.15, 0.3)
	V.bits(ground, UP, 6, { cone = 50, upward = 0.5, speed = { 10, 18 }, size = { 0.4, 0.9 }, life = { 0.6, 0.8 }, gravity = 1.5, color = C.rock, neon = false, shrink = 0.6, spin = 8 })
	V.bits(ground, UP, 5, { cone = 70, speed = { 3, 6 }, size = { 1, 1.6 }, life = { 0.8, 1 }, shape = "Sphere", color = C.dirt, neon = false, tr0 = 0.4, shrink = 2 })
end

-- Stars Burst VFX: yellow bubble + star, used for stuns that matter.
local STARS = {}

function E.stars(inst, duration)
	V.flash(posOf(inst) + V3(0, 2.6, 0), C.yellow, 1, 4, 0.45, 0.5)
	table.insert(STARS, { inst = inst, untilAt = tick() + duration, t = 0, parts = {} })
end

function E.stepStars(dt, now)
	local i = 1
	while i <= #STARS do
		local item = STARS[i]
		item.t += dt
		if now >= item.untilAt or not U.alive(item.inst) then
			for _, part in ipairs(item.parts) do
				P.release(part, "Part")
			end
			STARS[i] = STARS[#STARS]
			STARS[#STARS] = nil
		else
			if #item.parts == 0 then
				for _ = 1, 3 do
					local part = P.acquire("Part")
					if part ~= nil then
						U.set(part, "Color", C.yellow)
						U.set(part, "Material", NEON)
						U.set(part, "Size", V3(0.35, 0.35, 0.35))
						U.set(part, "Transparency", 0)
						table.insert(item.parts, part)
					end
				end
			end
			local head = posOf(item.inst) + V3(0, 2.4, 0)
			for k, part in ipairs(item.parts) do
				local a = item.t * 6 + k * (math.pi * 2 / 3)
				U.set(part, "Position", head + V3(math.cos(a) * 1, 0.2 * math.sin(a * 2), math.sin(a) * 1))
				U.set(part, "Orientation", V3(45, math.deg(a) * 2, 45))
			end
			i += 1
		end
	end
end

function E.airJump(pos, dir)
	local feet = pos - V3(0, 2.5, 0)
	V.ring(feet, UP, 0.5, 3.6, 0.3, C.white, { segments = 16, width = 0.3 })
	V.ring(feet - V3(0, 0.3, 0), UP, 0.3, 2.4, 0.25, C.white, { segments = 12, width = 0.2, delay = 0.04 })
	V.bits(feet, -UP, 8, { cone = 70, speed = { 6, 12 }, size = { 0.15, 0.3 }, life = { 0.2, 0.35 }, color = C.white })
end

function E.death(inst, pos, dir, isPlayer)
	V.flash(pos, C.white, 1, 7, 0.25, 0)
	V.ring(pos, UP, 1, 8, 0.5, C.white, { segments = 22, width = 0.5 })
	if not isPlayer then
		V.bits(pos, UP, 22, { speed = { 10, 26 }, size = { 0.5, 1.2 }, life = { 0.8, 1.3 }, gravity = 1.6, color = C.stone, neon = false, shrink = 0.7, spin = 9 })
	end
	V.bits(pos, UP, 10, { speed = { 3, 8 }, size = { 1.2, 2 }, life = { 0.8, 1.2 }, shape = "Sphere", color = C.smoke, neon = false, tr0 = 0.4, shrink = 2.2 })
	P.light(pos, C.white, 6, 20, 0.4)
	V.text(pos + V3(0, 3.5, 0), isPlayer and "K.O." or "SHATTERED", C.white, { size = 1.8, rise = 1.5 })
end

function E.respawn(pos)
	P.emit({ shape = "Part", pos = pos - V3(0, 3, 0), to = pos + V3(0, 6, 0), size0 = V3(2.5, 0.2, 2.5), size1 = V3(0.2, 9, 0.2), life = 0.5, tr0 = 0.2, tr1 = 1, color = C.white })
	V.ring(pos - V3(0, 2.9, 0), UP, 0.5, 4, 0.4, C.white, { segments = 16, width = 0.3 })
end

-- Showcase: each VFX pack platform loops its effect family.
function E.showcase(index, pos)
	local d = V3(1, 0, 0)
	if index == 1 then
		E.heavyHit(pos, d, 0, nil, nil)
	elseif index == 2 then
		E.parry(pos, d, 1, nil, nil)
	elseif index == 3 then
		E.dash(nil, pos, d)
		V.afterimage(pos, d, C.white, 0.5, 0.4)
		V.afterimage(pos - d * 2, d, C.ink, 0.5, 0.5)
	elseif index == 4 then
		E.shockwave(pos, 1.2)
	elseif index == 5 then
		E.perfectDodge(pos, d, nil)
	elseif index == 6 then
		V.flash(pos, C.yellow, 1, 10, 0.45, 0.5)
		V.flash(pos, C.white, 0.5, 4.25, 0.4, 0.2)
		V.bits(pos, UP, 15, { speed = { 15, 30 }, size = { 0.25, 0.5 }, life = { 0.35, 0.4 }, gravity = 0.6, stretch = 3, color = C.white })
		E.shockwave(pos, 0.6)
	else
		for k = 1, 4 do
			local shape = SWING_SHAPES[k]
			V.arc(pos + V3(0, 0.5, 0), V3(math.cos(k), 0, math.sin(k)), shape.tilt, shape.sweep, 3.2, C.white, { span = 140, segments = 14, width = 0.55, life = 0.22, grow = 0.7, sweepTime = 0.08 })
		end
		V.ring(pos, UP, 2, 6, 0.6, C.white, { segments = 20, width = 0.6, tr0 = 0.5 })
	end
end

------------------------------------------------------------------------------------------
-- UI (HUD)
------------------------------------------------------------------------------------------

local UIS = {
	root = nil,
	ok = false,
	hp = 100, maxHp = 100, stamina = 100, posture = 0,
	shownHp = 100, shownStamina = 100, shownPosture = 0,
	state = 0, weapon = 1, crit = 0, dash = 0, skill = 0, combo = 0, streak = 0,
	target = nil, thp = -1, tposture = -1,
	helpOpen = true,
	helpUntil = 0,
	banner = nil, bannerUntil = 0,
	flash = nil, flashT = 1, flashLife = 1, flashMax = 0,
}

function UI.spawn(kind, parent, props)
	local inst = U.try(game.Prefabs.Spawn, kind)
	if inst == nil then
		inst = U.try(Instance.new, kind)
	end
	if inst == nil then
		return nil
	end
	U.set(inst, "Parent", parent)
	for key, value in pairs(props) do
		if key == "pos" then
			if not U.set(inst, "UIPosition", value) then
				U.set(inst, "Position", value)
			end
		elseif key == "size" then
			if not U.set(inst, "UISize", value) then
				U.set(inst, "Size", value)
			end
		else
			U.set(inst, key, value)
		end
	end
	return inst
end

function UI.panel(parent, pos, size, color, transparency)
	return UI.spawn("Frame", parent, {
		pos = pos,
		size = size,
		BackgroundColor3 = color or V3(0.07, 0.08, 0.1),
		BackgroundTransparency = transparency or 0.25,
		BorderSizePixel = 0,
		CornerRadius = 6,
	})
end

function UI.label(parent, pos, size, text, color, textSize, align)
	return UI.spawn("TextLabel", parent, {
		pos = pos,
		size = size,
		Text = text,
		TextColor3 = color or V3(1, 1, 1),
		TextSize = textSize or 16,
		BackgroundTransparency = 1,
		TextXAlignment = align or "Left",
		Bold = true,
		TextStrokeTransparency = 0.5,
	})
end

function UI.bar(parent, y, labelText, color)
	local back = UI.panel(parent, UDim2.new(0, 14, 0, y), UDim2.new(1, -28, 0, 14), V3(0.15, 0.16, 0.2), 0.1)
	local fill = back and UI.panel(back, UDim2.new(0, 0, 0, 0), UDim2.new(1, 0, 1, 0), color, 0) or nil
	local text = UI.label(parent, UDim2.new(0, 14, 0, y - 18), UDim2.new(1, -28, 0, 16), labelText, V3(0.85, 0.87, 0.92), 13)
	return { back = back, fill = fill, text = text }
end

function UI.setBar(bar, ratio, text)
	if bar == nil or bar.fill == nil then
		return
	end
	ratio = math.clamp(ratio, 0, 1)
	U.set(bar.fill, "UISize", UDim2.new(ratio, 0, 1, 0))
	if text ~= nil and bar.text ~= nil then
		U.set(bar.text, "Text", text)
	end
end

function UI.build()
	local screen = U.get(game, "Overlay") or U.get(game.World, "Screen")
	local gui = UI.spawn("ScreenGui", screen or game.World, { Name = "PA_Hud" })
	if gui == nil then
		warn("[ParryArena] could not build the HUD")
		return
	end
	UIS.root = gui

	-- Bottom-left: player bars
	local panel = UI.panel(gui, UDim2.new(0, 16, 1, -168), UDim2.new(0, 300, 0, 152))
	UIS.weaponText = UI.label(panel, UDim2.new(0, 14, 0, 8), UDim2.new(1, -28, 0, 20), "FISTS", V3(1, 0.72, 0.42), 18)
	UIS.hpBar = UI.bar(panel, 50, "HEALTH", V3(0.86, 0.24, 0.27))
	UIS.staminaBar = UI.bar(panel, 88, "STAMINA", V3(0.98, 0.78, 0.25))
	UIS.postureBar = UI.bar(panel, 126, "POSTURE", V3(1, 0.55, 0.2))

	-- Bottom-centre: cooldowns
	UIS.cooldownText = UI.label(gui, UDim2.new(0.5, -260, 1, -44), UDim2.new(0, 520, 0, 24), "", V3(0.9, 0.92, 1), 16, "Center")

	-- Centre-right: combo / streak
	UIS.comboText = UI.label(gui, UDim2.new(1, -260, 0.5, -40), UDim2.new(0, 240, 0, 40), "", V3(1, 1, 1), 30, "Right")
	UIS.streakText = UI.label(gui, UDim2.new(1, -260, 0.5, 2), UDim2.new(0, 240, 0, 24), "", V3(1, 0.69, 0.42), 18, "Right")

	-- Top-centre: enemy panel
	UIS.enemyPanel = UI.panel(gui, UDim2.new(0.5, -170, 0, 16), UDim2.new(0, 340, 0, 84))
	UIS.enemyName = UI.label(UIS.enemyPanel, UDim2.new(0, 14, 0, 6), UDim2.new(1, -28, 0, 20), "", V3(1, 1, 1), 16)
	UIS.enemyHp = UI.bar(UIS.enemyPanel, 46, "HEALTH", V3(0.86, 0.24, 0.27))
	UIS.enemyPosture = UI.bar(UIS.enemyPanel, 74, "", V3(1, 0.55, 0.2))
	U.set(UIS.enemyPanel, "Visible", false)

	-- Banner (difficulty changes etc.)
	UIS.banner = UI.label(gui, UDim2.new(0.5, -300, 0.22, 0), UDim2.new(0, 600, 0, 44), "", V3(1, 1, 1), 32, "Center")

	-- Controls help
	UIS.help = UI.panel(gui, UDim2.new(1, -336, 0, 16), UDim2.new(0, 320, 0, 300))
	local lines = {
		"PARRY ARENA  -  controls",
		"LMB  attack (4-hit combo)",
		"F  block  -  tap as a hit lands = PARRY",
		"R  critical (breaks guard)   RMB  feint it",
		"E  weapon skill",
		"Q  dash (WASD direction)  -  dodge early = perfect",
		"Ctrl  slide while running, Space to leap",
		"Space  double jump / wall jump",
		"1 2 3  Fists / Blade / Rapier",
		"Tab  lock on      H  hide this",
		"Chat: /diff hard   /weapon blade   /hitboxes",
	}
	for i, line in ipairs(lines) do
		UI.label(UIS.help, UDim2.new(0, 12, 0, 8 + (i - 1) * 26), UDim2.new(1, -24, 0, 22), line, i == 1 and V3(1, 0.72, 0.42) or V3(0.9, 0.92, 1), i == 1 and 17 or 14)
	end

	-- Full-screen flash (perfect dodge desaturation stand-in)
	UIS.flash = UI.panel(gui, UDim2.new(0, 0, 0, 0), UDim2.new(1, 0, 1, 0), V3(1, 1, 1), 1)
	U.set(UIS.flash, "ZIndex", 50)
	UIS.ok = true
end

function UI.screenFlash(color, strength, life)
	if UIS.flash == nil then
		return
	end
	U.set(UIS.flash, "BackgroundColor3", color)
	UIS.flashT = 0
	UIS.flashLife = life
	UIS.flashMax = strength
end

function UI.showBanner(text, color, duration)
	if UIS.banner == nil then
		return
	end
	U.set(UIS.banner, "Text", text)
	U.set(UIS.banner, "TextColor3", color or V3(1, 1, 1))
	UIS.bannerUntil = tick() + (duration or 2)
end

local function fmtCooldown(key, name, seconds)
	if seconds <= 0.05 then
		return key .. " " .. name
	end
	return string.format("%s %s %.1fs", key, name, seconds)
end

function UI.step(dt, now)
	if not UIS.ok then
		return
	end
	local k = 1 - math.exp(-12 * dt)
	UIS.shownHp = UIS.shownHp + (UIS.hp - UIS.shownHp) * k
	UIS.shownStamina = UIS.shownStamina + (UIS.stamina - UIS.shownStamina) * k
	UIS.shownPosture = UIS.shownPosture + (UIS.posture - UIS.shownPosture) * k

	UI.setBar(UIS.hpBar, UIS.shownHp / math.max(UIS.maxHp, 1), string.format("HEALTH  %d / %d", math.ceil(UIS.hp), math.ceil(UIS.maxHp)))
	UI.setBar(UIS.staminaBar, UIS.shownStamina / 100, string.format("STAMINA  %d", math.floor(UIS.stamina)))
	local postureLabel = "POSTURE"
	if bit32.band(UIS.state, ST.BROKEN) ~= 0 then
		postureLabel = "GUARD BROKEN"
	elseif bit32.band(UIS.state, ST.BLOCK) ~= 0 then
		postureLabel = "POSTURE  (blocking)"
	end
	UI.setBar(UIS.postureBar, UIS.shownPosture / 100, postureLabel)

	local info = WEAPON_INFO[UIS.weapon] or WEAPON_INFO[1]
	U.set(UIS.weaponText, "Text", string.upper(info.Name))
	U.set(UIS.cooldownText, "Text", table.concat({
		fmtCooldown("R", info.Critical, UIS.crit),
		fmtCooldown("E", info.Skill, UIS.skill),
		fmtCooldown("Q", "Dash", UIS.dash),
	}, "     "))

	U.set(UIS.comboText, "Text", UIS.combo > 0 and bit32.band(UIS.state, ST.ATTACK) ~= 0 and ("M" .. UIS.combo) or "")
	U.set(UIS.streakText, "Text", UIS.streak > 0 and ("PARRY STREAK " .. UIS.streak) or "")

	local showEnemy = UIS.thp ~= nil and UIS.thp >= 0 and UIS.target ~= nil
	U.set(UIS.enemyPanel, "Visible", showEnemy)
	if showEnemy then
		local name = U.get(UIS.target, "Name") or "Enemy"
		local typeKey = string.match(name, "^PA_Golem_(%a+)_")
		local pretty = ({ basic = "Training Dummy", block = "Blocking Dummy", parry = "Parry Dummy", attack = "Attacking Dummy", ai = "Duelist" })[typeKey or ""]
		U.set(UIS.enemyName, "Text", pretty or name)
		local maxHp = UIS.targetMaxHp or 100
		UI.setBar(UIS.enemyHp, UIS.thp / math.max(maxHp, UIS.thp, 1), string.format("HEALTH  %d", math.ceil(UIS.thp)))
		UI.setBar(UIS.enemyPosture, UIS.tposture / 100, string.format("POSTURE  %d%%", math.floor(UIS.tposture)))
	end

	U.set(UIS.help, "Visible", UIS.helpOpen)

	if UIS.banner ~= nil then
		U.set(UIS.banner, "Visible", now < UIS.bannerUntil)
	end

	if UIS.flash ~= nil and UIS.flashT < UIS.flashLife then
		UIS.flashT += dt
		local a = math.clamp(UIS.flashT / UIS.flashLife, 0, 1)
		U.set(UIS.flash, "BackgroundTransparency", 1 - UIS.flashMax * (1 - a))
	end
end

------------------------------------------------------------------------------------------
-- GOLEM RIGS (client-side limbs for the server's torso)
------------------------------------------------------------------------------------------

local GOLEM_ACCENT = {
	basic = Color3.fromRGB(235, 235, 245),
	block = Color3.fromRGB(84, 170, 255),
	parry = Color3.fromRGB(255, 176, 106),
	attack = Color3.fromRGB(255, 73, 73),
	ai = Color3.fromRGB(170, 0, 255),
}

-- Limb layout relative to the torso centre. Arms/legs hang from a pivot.
local LIMBS = {
	{ key = "Head", pivot = V3(0, 1, 0), hang = V3(0, 0.65, 0), size = V3(1.25, 1.25, 1.25) },
	{ key = "LeftArm", pivot = V3(-1.5, 0.9, 0), hang = V3(0, -0.9, 0), size = V3(1, 2, 1) },
	{ key = "RightArm", pivot = V3(1.5, 0.9, 0), hang = V3(0, -0.9, 0), size = V3(1, 2, 1) },
	{ key = "LeftLeg", pivot = V3(-0.5, -1, 0), hang = V3(0, -1, 0), size = V3(1, 2, 1) },
	{ key = "RightLeg", pivot = V3(0.5, -1, 0), hang = V3(0, -1, 0), size = V3(1, 2, 1) },
}

-- Target angles per pose, in degrees: { pitch, roll } per limb.
-- Positive pitch swings a limb forward and up. Roll moves the hand sideways: positive roll
-- goes toward +X (the golem's right), so outward is +roll on the right arm, -roll on the left.
local POSES = {
	[POSE.IDLE] = { LeftArm = { 12, -6 }, RightArm = { 12, 6 }, Head = { 0, 0 } },
	[POSE.WINDUP] = { LeftArm = { 30, -10 }, RightArm = { -55, 20 }, Head = { -5, 0 } },
	[POSE.SWING] = { LeftArm = { -20, -10 }, RightArm = { 100, -15 }, Head = { 5, 0 } },
	[POSE.BLOCK] = { LeftArm = { 85, 40 }, RightArm = { 85, -40 }, Head = { 10, 0 } },
	[POSE.STUN] = { LeftArm = { -20, -50 }, RightArm = { -20, 50 }, Head = { -25, 10 } },
	[POSE.PARRY] = { LeftArm = { 110, -15 }, RightArm = { 110, 15 }, Head = { 0, 0 } },
	[POSE.BROKEN] = { LeftArm = { -5, -5 }, RightArm = { -5, 5 }, Head = { 35, 0 }, drop = 0.3 },
	[POSE.DASH] = { LeftArm = { -50, -15 }, RightArm = { -50, 15 }, Head = { 10, 0 } },
	[POSE.CRIT_WINDUP] = { LeftArm = { 165, 10 }, RightArm = { 165, -10 }, Head = { -15, 0 } },
	[POSE.SKILL] = { LeftArm = { 90, 0 }, RightArm = { 90, 0 }, Head = { 0, 0 } },
	[POSE.PARRIED] = { LeftArm = { 150, -45 }, RightArm = { 150, 45 }, Head = { -30, 0 } },
}

local RIGS = {} -- name -> rig

local function rigFor(root, name)
	local rig = RIGS[name]
	if rig ~= nil then
		rig.root = root
		return rig
	end
	local typeKey = string.match(name, "^PA_Golem_(%a+)_") or "basic"
	local accent = GOLEM_ACCENT[typeKey] or C.white
	rig = { root = root, name = name, typeKey = typeKey, parts = {}, angles = {}, pose = POSE.IDLE, step = 1, t = 0, poseAt = 0 }
	for _, limb in ipairs(LIMBS) do
		local part = P.create("Part")
		if part ~= nil then
			U.set(part, "Name", "PA_Rig" .. limb.key)
			U.set(part, "Size", limb.size)
			U.set(part, "Color", C.stone)
			U.set(part, "Material", "Plastic")
			U.set(part, "Transparency", 0)
			rig.parts[limb.key] = part
			rig.angles[limb.key] = { 0, 0 }
		end
	end
	-- Glowing eyes and a belt in the dummy's colour
	for _, key in ipairs({ "EyeL", "EyeR", "Belt" }) do
		local part = P.create("Part")
		if part ~= nil then
			U.set(part, "Name", "PA_Rig" .. key)
			U.set(part, "Size", key == "Belt" and V3(2.06, 0.35, 1.06) or V3(0.25, 0.18, 0.1))
			U.set(part, "Color", accent)
			U.set(part, "Material", NEON)
			U.set(part, "Transparency", 0)
			rig.parts[key] = part
		end
	end
	RIGS[name] = rig
	return rig
end

local function setRigVisible(rig, visible)
	for _, part in pairs(rig.parts) do
		U.set(part, "Transparency", visible and 0 or 1)
		if not visible then
			U.set(part, "Position", HIDDEN)
		end
	end
	rig.visible = visible
end

local function findMap()
	local map = U.try(function()
		return game.World.FindFirstChild("PA_Map")
	end)
	if map == nil then
		map = U.try(function()
			return game.World:FindFirstChild("PA_Map")
		end)
	end
	return map
end

local discoverAt = 0
local function discoverGolems(now)
	if now < discoverAt then
		return
	end
	discoverAt = now + 1
	local map = findMap()
	local children = map and U.call(map, "GetChildren") or nil
	if children == nil then
		-- The server parents everything to the World when it could not make the PA_Map folder.
		children = U.try(function()
			return game.World.GetChildren()
		end)
	end
	if children == nil then
		return
	end
	for _, child in ipairs(children) do
		local name = U.get(child, "Name")
		if type(name) == "string" and string.sub(name, 1, 9) == "PA_Golem_" then
			rigFor(child, name)
		end
	end
end

local function rootRotation(root)
	if not hasRotation then
		return nil
	end
	local o = U.get(root, "Orientation")
	if o == nil then
		return nil
	end
	return U.try(function()
		return rotation.from_euler(o)
	end)
end

local function stepRigs(dt, now)
	for name, rig in pairs(RIGS) do
		local root = rig.root
		if not U.alive(root) then
			setRigVisible(rig, false)
			RIGS[name] = nil
		elseif rig.pose == POSE.DEAD then
			if rig.visible ~= false then
				setRigVisible(rig, false)
			end
		else
			if rig.visible == false then
				setRigVisible(rig, true)
			end
			rig.t += dt
			local origin = posOf(root)
			local rot = rootRotation(root)
			local fwd = forwardOf(root)
			local right = U.cross(fwd, UP)
			local target = POSES[rig.pose] or POSES[POSE.IDLE]
			-- Mirror single-arm poses on even combo steps so M1-M4 alternate hands.
			local mirror = (rig.pose == POSE.WINDUP or rig.pose == POSE.SWING) and rig.step % 2 == 0
			local drop = target.drop or 0
			local bob = math.sin(rig.t * 2.2) * 0.05
			local sharp = 1 - math.exp(-20 * dt)

			for _, limb in ipairs(LIMBS) do
				local part = rig.parts[limb.key]
				if part ~= nil then
					local key = limb.key
					if mirror and key == "LeftArm" then
						key = "RightArm"
					elseif mirror and key == "RightArm" then
						key = "LeftArm"
					end
					local want = target[key] or { 0, 0 }
					local pitch, roll = want[1], want[2]
					if mirror and (limb.key == "LeftArm" or limb.key == "RightArm") then
						roll = -roll
					end
					if rig.pose == POSE.SKILL and (limb.key == "LeftArm" or limb.key == "RightArm") then
						local phase = limb.key == "LeftArm" and 0 or math.pi
						pitch = 60 + math.sin(rig.t * 28 + phase) * 40
					end
					if limb.key == "LeftLeg" or limb.key == "RightLeg" then
						local stride = U.len(U.flat(origin - (rig.lastPos or origin))) / math.max(dt, 1e-3)
						local swing = math.clamp(stride / 20, 0, 1) * 35
						pitch = math.sin(rig.t * 10 + (limb.key == "LeftLeg" and 0 or math.pi)) * swing
						roll = 0
					end
					local cur = rig.angles[limb.key]
					cur[1] = cur[1] + (pitch - cur[1]) * sharp
					cur[2] = cur[2] + (roll - cur[2]) * sharp

					local isLeg = limb.key == "LeftLeg" or limb.key == "RightLeg"
					local pivot = limb.pivot
					if not isLeg then
						pivot = pivot + V3(0, bob - drop, 0)
					end
					local localRot = U.try(function()
						return rotation.compose(rotation.axis(V3(1, 0, 0), math.rad(cur[1])), rotation.axis(V3(0, 0, 1), math.rad(cur[2])))
					end)
					local centreLocal
					if localRot ~= nil then
						centreLocal = pivot + rotation.apply(localRot, limb.hang)
					else
						centreLocal = pivot + limb.hang
					end
					local world
					if rot ~= nil then
						world = origin + rotation.apply(rot, centreLocal)
						U.set(part, "Position", world)
						local full = localRot and U.try(function()
							return rotation.to_euler(rotation.compose(rot, localRot))
						end) or nil
						if full ~= nil then
							U.set(part, "Orientation", full)
						end
					else
						world = origin + right * centreLocal.X + UP * centreLocal.Y - fwd * centreLocal.Z
						U.set(part, "Position", world)
						U.set(part, "Orientation", U.orient(fwd, UP))
					end
				end
			end
			-- Eyes and belt
			local head = rig.parts.Head
			local headPos = head and posOf(head) or origin + UP * 1.65
			local eyeL, eyeR, belt = rig.parts.EyeL, rig.parts.EyeR, rig.parts.Belt
			if eyeL ~= nil then
				U.set(eyeL, "Position", headPos + fwd * 0.64 - right * 0.28 + UP * 0.12)
				U.set(eyeL, "Orientation", U.orient(fwd, UP))
			end
			if eyeR ~= nil then
				U.set(eyeR, "Position", headPos + fwd * 0.64 + right * 0.28 + UP * 0.12)
				U.set(eyeR, "Orientation", U.orient(fwd, UP))
			end
			if belt ~= nil then
				U.set(belt, "Position", origin - UP * (0.75 + drop))
				U.set(belt, "Orientation", U.orient(fwd, UP))
			end
			rig.lastPos = origin
		end
	end
end

------------------------------------------------------------------------------------------
-- WEAPON MODELS, BLOCK SHIELDS, SLIDE / WALL-RUN DUST
------------------------------------------------------------------------------------------

local WEAPON_MODELS = {} -- key -> { inst, weapon, parts }
local SHIELDS = {} -- key -> { inst, part }
local SLIDERS = {} -- key -> { inst, nextAt }
local WALLRUNNERS = {} -- key -> { inst, normal, nextAt }

local function keyOf(inst)
	return (U.get(inst, "Name") or "?") .. "#" .. tostring(U.get(inst, "UserId") or "")
end

local function findRightArm(character)
	for _, name in ipairs({ "RightHand", "RightLowerArm", "RightArm", "Right Arm", "RightUpperArm" }) do
		local part = U.call(character, "FindFirstChild", name, true)
		if part ~= nil then
			return part
		end
	end
	return nil
end

local function buildWeapon(weapon)
	local parts = {}
	local function add(size, color, neon, offset)
		local part = P.create("Part")
		if part ~= nil then
			U.set(part, "Size", size)
			U.set(part, "Color", color)
			U.set(part, "Material", neon and NEON or "Metal")
			U.set(part, "Transparency", 0)
			table.insert(parts, { part = part, offset = offset })
		end
	end
	if weapon == 2 then
		-- Blade: grip, guard, long edge
		add(V3(0.25, 0.25, 1.1), Color3.fromRGB(40, 30, 30), false, V3(0, 0, 0.1))
		add(V3(0.9, 0.18, 0.18), Color3.fromRGB(255, 176, 106), true, V3(0, 0, -0.5))
		add(V3(0.12, 0.3, 4.2), Color3.fromRGB(220, 225, 235), false, V3(0, 0, -2.7))
	elseif weapon == 3 then
		-- Rapier: grip, cup guard, thin needle
		add(V3(0.2, 0.2, 0.9), Color3.fromRGB(40, 30, 30), false, V3(0, 0, 0.1))
		add(V3(0.6, 0.6, 0.15), Color3.fromRGB(255, 198, 24), true, V3(0, 0, -0.4))
		add(V3(0.07, 0.07, 4.4), Color3.fromRGB(230, 230, 240), false, V3(0, 0, -2.6))
	end
	return parts
end

local function setWeaponModel(inst, weapon)
	local key = keyOf(inst)
	WEAPON_BY_INST[U.get(inst, "Name") or "?"] = weapon
	local current = WEAPON_MODELS[key]
	if current ~= nil and current.weapon == weapon then
		current.inst = inst
		return
	end
	if current ~= nil then
		for _, item in ipairs(current.parts) do
			P.release(item.part, "Part")
		end
		WEAPON_MODELS[key] = nil
	end
	if weapon == 2 or weapon == 3 then
		WEAPON_MODELS[key] = { inst = inst, weapon = weapon, parts = buildWeapon(weapon) }
	end
end

local function stepWeapons()
	for key, model in pairs(WEAPON_MODELS) do
		if not U.alive(model.inst) then
			for _, item in ipairs(model.parts) do
				P.release(item.part, "Part")
			end
			WEAPON_MODELS[key] = nil
		else
			model.arm = model.arm or findRightArm(model.inst)
			local arm = model.arm
			local base, forward, up
			if arm ~= nil then
				local size = U.get(arm, "Size") or V3(1, 2, 1)
				base = U.call(arm, "TransformPoint", V3(0, -size.Y / 2 - 0.1, 0)) or posOf(arm)
				forward = U.call(arm, "TransformDirection", FORWARD) or forwardOf(model.inst)
				up = U.call(arm, "TransformDirection", UP) or UP
			else
				local fwd = forwardOf(model.inst)
				base = posOf(model.inst) + U.cross(fwd, UP) * 1.5 - UP * 0.6
				forward = fwd
				up = UP
			end
			for _, item in ipairs(model.parts) do
				U.set(item.part, "Position", base + forward * -item.offset.Z + up * item.offset.Y)
				U.set(item.part, "Orientation", U.orient(forward, up))
			end
		end
	end
end

local function setShield(inst, on)
	local key = keyOf(inst)
	local current = SHIELDS[key]
	if on and current == nil then
		local part = P.acquire("Part")
		if part ~= nil then
			U.set(part, "Size", V3(2.6, 3, 0.08))
			U.set(part, "Color", C.block)
			U.set(part, "Material", NEON)
			U.set(part, "Transparency", 0.75)
			SHIELDS[key] = { inst = inst, part = part, t = 0 }
		end
	elseif not on and current ~= nil then
		P.release(current.part, "Part")
		SHIELDS[key] = nil
	end
end

local function stepShields(dt)
	for key, shield in pairs(SHIELDS) do
		if not U.alive(shield.inst) then
			P.release(shield.part, "Part")
			SHIELDS[key] = nil
		else
			shield.t += dt
			local fwd = forwardOf(shield.inst)
			U.set(shield.part, "Position", posOf(shield.inst) + fwd * 1.5 + UP * 0.3)
			U.set(shield.part, "Orientation", U.orient(fwd, UP))
			U.set(shield.part, "Transparency", 0.72 + math.sin(shield.t * 8) * 0.06)
		end
	end
end

local function stepDust(now)
	for key, item in pairs(SLIDERS) do
		if not U.alive(item.inst) or now > item.untilAt then
			SLIDERS[key] = nil
		elseif now >= item.nextAt then
			item.nextAt = now + 0.05
			local feet = groundBelow(posOf(item.inst))
			P.emit({
				shape = "Sphere",
				pos = feet + V3(U.rand(-0.6, 0.6), 0.3, U.rand(-0.6, 0.6)),
				vel = V3(U.rand(-3, 3), U.rand(1, 4), U.rand(-3, 3)),
				drag = 3,
				size0 = V3(0.6, 0.6, 0.6),
				size1 = V3(2.2, 2.2, 2.2),
				life = U.rand(0.4, 0.7),
				tr0 = 0.5,
				tr1 = 1,
				color = C.smoke,
				neon = false,
			})
		end
	end
	for key, item in pairs(WALLRUNNERS) do
		if not U.alive(item.inst) or now > item.untilAt then
			WALLRUNNERS[key] = nil
		elseif now >= item.nextAt then
			item.nextAt = now + 0.04
			local contact = posOf(item.inst) - item.normal * 1.2
			V.bits(contact, item.normal, 2, { cone = 50, speed = { 4, 10 }, size = { 0.15, 0.3 }, life = { 0.2, 0.4 }, gravity = 0.5, color = C.parry })
		end
	end
end

------------------------------------------------------------------------------------------
-- LOCK-ON
------------------------------------------------------------------------------------------

local LOCK = { target = nil, marker = nil, t = 0 }

local function candidates()
	local list = {}
	for _, rig in pairs(RIGS) do
		if rig.pose ~= POSE.DEAD and U.alive(rig.root) then
			table.insert(list, rig.root)
		end
	end
	pcall(function()
		for p in game.World.Each("Player") do
			local character = U.get(p, "Character")
			if character ~= nil and not isMe(character) then
				table.insert(list, character)
			end
		end
	end)
	return list
end

local function toggleLock(aim)
	if LOCK.target ~= nil then
		LOCK.target = nil
		return
	end
	local me = REF.character
	if me == nil then
		return
	end
	local origin = posOf(me)
	local best, bestScore = nil, math.huge
	for _, inst in ipairs(candidates()) do
		local d = U.flat(posOf(inst) - origin)
		local distance = U.len(d)
		if distance > 0.5 and distance <= CONFIG.LockOnRange then
			local facing = U.dot(d / distance, aim)
			local score = distance * (2.2 - facing)
			if score < bestScore then
				best, bestScore = inst, score
			end
		end
	end
	LOCK.target = best
end

local function stepLock(dt)
	local target = LOCK.target
	if target ~= nil and (not U.alive(target) or U.len(posOf(target) - posOf(REF.character)) > CONFIG.LockOnRange * 1.3) then
		LOCK.target = nil
		target = nil
	end
	if target == nil then
		if LOCK.marker ~= nil then
			P.release(LOCK.marker, "Part")
			LOCK.marker = nil
		end
		return
	end
	if LOCK.marker == nil then
		LOCK.marker = P.acquire("Part")
		if LOCK.marker ~= nil then
			U.set(LOCK.marker, "Size", V3(0.6, 0.6, 0.6))
			U.set(LOCK.marker, "Color", C.parry)
			U.set(LOCK.marker, "Material", NEON)
			U.set(LOCK.marker, "Transparency", 0.1)
		end
	end
	LOCK.t += dt
	if LOCK.marker ~= nil then
		U.set(LOCK.marker, "Position", posOf(target) + V3(0, 3.6 + math.sin(LOCK.t * 4) * 0.15, 0))
		U.set(LOCK.marker, "Orientation", V3(45, math.deg(LOCK.t * 3), 45))
	end
end

------------------------------------------------------------------------------------------
-- INPUT
------------------------------------------------------------------------------------------

local IN = {
	blockHeld = false,
	slideHeld = false,
	nextIntent = 0,
	nextHoldAttack = 0,
	aim = FORWARD,
	move = ZERO,
	ok = true,
}

local function push(code, dir, aim)
	pcall(function()
		ACTION:Push(code, dir or ZERO, aim or IN.aim)
	end)
end

local function keyDown(name)
	local ok, result = pcall(function()
		return game.Input.KeyDown(game.Input.Key[name])
	end)
	return ok and result == true
end

local function keyPressed(name)
	local ok, result = pcall(function()
		return game.Input.KeyPressed(game.Input.Key[name])
	end)
	return ok and result == true
end

local function buttonDown(name)
	local ok, result = pcall(function()
		return game.Input.ButtonDown(game.Input.Button[name])
	end)
	return ok and result == true
end

local function buttonPressed(name)
	local ok, result = pcall(function()
		return game.Input.ButtonPressed(game.Input.Button[name])
	end)
	return ok and result == true
end

local function computeAim()
	local me = REF.character
	if me == nil then
		return IN.aim
	end
	local mePos = posOf(me)
	if LOCK.target ~= nil then
		return U.unit(U.flat(posOf(LOCK.target) - mePos), IN.aim)
	end
	if REF.camera ~= nil then
		local camPos = posOf(REF.camera)
		local look = U.flat(mePos - camPos)
		if U.len(look) > 0.5 then
			return U.unit(look, IN.aim)
		end
		local camLook = U.call(REF.camera, "TransformDirection", FORWARD)
		if camLook ~= nil then
			return U.unit(U.flat(camLook), IN.aim)
		end
	end
	return forwardOf(me)
end

local function readInput(now)
	if REF.character == nil then
		return
	end
	IN.aim = computeAim()
	local f = IN.aim
	local r = U.cross(f, UP)
	local wish = ZERO
	if keyDown("W") then
		wish += f
	end
	if keyDown("S") then
		wish -= f
	end
	if keyDown("D") then
		wish += r
	end
	if keyDown("A") then
		wish -= r
	end
	IN.move = U.unit(wish, ZERO)

	-- Attack: press, or hold to keep swinging (the server enforces the cadence).
	if buttonPressed("Left") then
		push(ACT.ATTACK)
		IN.nextHoldAttack = now + 0.25
	elseif buttonDown("Left") and now >= IN.nextHoldAttack then
		push(ACT.ATTACK)
		IN.nextHoldAttack = now + CONFIG.HoldAttackInterval
	end
	if buttonPressed("Right") then
		push(ACT.FEINT)
	end

	local blockDown = keyDown("F")
	if blockDown and not IN.blockHeld then
		push(ACT.BLOCK_ON)
	elseif not blockDown and IN.blockHeld then
		push(ACT.BLOCK_OFF)
	end
	IN.blockHeld = blockDown

	if keyPressed("Q") then
		push(ACT.DASH, U.len(IN.move) > 0.1 and IN.move or f)
	end
	if keyPressed("R") then
		push(ACT.CRITICAL)
	end
	if keyPressed("E") then
		push(ACT.SKILL)
	end

	local slideDown = keyDown("LeftControl") or keyDown("RightControl")
	if slideDown and not IN.slideHeld then
		push(ACT.SLIDE_ON, U.len(IN.move) > 0.1 and IN.move or f)
	elseif not slideDown and IN.slideHeld then
		push(ACT.SLIDE_OFF)
	end
	IN.slideHeld = slideDown

	if keyPressed("Space") then
		push(ACT.JUMP, IN.move)
	end

	for index, keyName in ipairs({ "One", "Two", "Three" }) do
		if keyPressed(keyName) then
			push(ACT.WEAPON, V3(index, 0, 0))
		end
	end

	if keyPressed("Tab") then
		toggleLock(f)
	end
	if keyPressed("H") then
		UIS.helpOpen = not UIS.helpOpen
	end

	if now >= IN.nextIntent then
		IN.nextIntent = now + 1 / CONFIG.IntentRate
		local flags = 0
		if IN.blockHeld then
			flags += FLAG.BLOCK
		end
		if IN.slideHeld then
			flags += FLAG.SLIDE
		end
		if keyDown("Space") then
			flags += FLAG.JUMP
		end
		pcall(function()
			INTENT:Push(IN.move, IN.aim, flags)
		end)
	end
end

------------------------------------------------------------------------------------------
-- EVENTS IN
------------------------------------------------------------------------------------------

local function readHud()
	for i = 1, HUD.count do
		UIS.hp = HUD.hp[i] or UIS.hp
		UIS.maxHp = HUD.maxhp[i] or UIS.maxHp
		UIS.stamina = HUD.stamina[i] or UIS.stamina
		UIS.posture = HUD.posture[i] or UIS.posture
		UIS.state = HUD.state[i] or 0
		UIS.weapon = HUD.weapon[i] or 1
		UIS.crit = HUD.crit[i] or 0
		UIS.dash = HUD.dash[i] or 0
		UIS.skill = HUD.skill[i] or 0
		UIS.combo = HUD.combo[i] or 0
		UIS.streak = HUD.streak[i] or 0
		local target = HUD.target[i]
		local thp = HUD.thp[i] or -1
		if target ~= nil and thp >= 0 and not isMe(target) then
			UIS.target = target
		else
			UIS.target = nil
		end
		UIS.thp = thp
		UIS.targetMaxHp = HUD.tmaxhp[i] or 100
		UIS.tposture = HUD.tposture[i] or -1
	end
end

local function onFx(kind, src, dst, pos, dir, num)
	if kind == FXK.SWING then
		E.swing(src, pos, dir, num)
		local rig = RIGS[U.get(src, "Name") or ""]
		if rig ~= nil then
			rig.step = math.floor(num)
		end
		if isMe(src) then
			punch(0.8, 0)
		end
	elseif kind == FXK.HIT then
		E.hit(pos, dir, num, src, dst)
	elseif kind == FXK.HEAVY_HIT then
		E.heavyHit(pos, dir, num, src, dst)
	elseif kind == FXK.BLOCKED then
		E.blocked(pos, dir, num, dst)
	elseif kind == FXK.PARRY then
		E.parry(pos, dir, num, src, dst)
	elseif kind == FXK.GUARD_BREAK then
		E.guardBreak(pos, dir, dst)
	elseif kind == FXK.DASH then
		E.dash(src, pos, dir)
		E.trail(src, 0.3, C.white, 0.7)
	elseif kind == FXK.PERFECT_DODGE then
		E.perfectDodge(pos, dir, src)
	elseif kind == FXK.WINDUP then
		-- num: 2 = critical, 10 + step = combo step N (tells the golem rig which hand to use)
		if num >= 10 then
			local rig = RIGS[U.get(src, "Name") or ""]
			if rig ~= nil then
				rig.step = math.floor(num - 10)
			end
		end
		E.windup(src, pos, dir, num)
	elseif kind == FXK.FEINT then
		E.feint(pos)
	elseif kind == FXK.SLIDE then
		local key = keyOf(src)
		if num > 0 then
			SLIDERS[key] = { inst = src, nextAt = 0, untilAt = tick() + 6.5 }
			E.trail(src, 0.6, C.white, 0.4)
			V.streaks(pos - V3(0, 2, 0), -U.unit(dir, FORWARD), 4, { cone = 10, speed = { 20, 30 }, length = { 2, 4 }, life = { 0.15, 0.25 } })
		else
			SLIDERS[key] = nil
		end
	elseif kind == FXK.AIR_JUMP then
		E.airJump(pos, dir)
		E.trail(src, 0.25, C.white, 0.35)
	elseif kind == FXK.LEAP then
		E.airJump(pos, dir)
		E.trail(src, 0.45, C.white, 0.5)
		V.streaks(pos, U.unit(dir + UP, UP), 6, { cone = 25, speed = { 25, 35 }, length = { 2, 4 }, life = { 0.15, 0.3 } })
	elseif kind == FXK.WALL_RUN then
		local key = keyOf(src)
		if num > 0 then
			WALLRUNNERS[key] = { inst = src, normal = U.unit(dir, V3(1, 0, 0)), nextAt = 0, untilAt = tick() + 3 }
			E.trail(src, 2.5, C.parry, 0.35)
		else
			WALLRUNNERS[key] = nil
		end
	elseif kind == FXK.WALL_JUMP then
		V.ring(pos, U.unit(dir, UP), 0.5, 3.5, 0.3, C.white, { segments = 16, width = 0.3 })
		E.trail(src, 0.4, C.white, 0.45)
	elseif kind == FXK.SHOCKWAVE then
		E.shockwave(pos, num > 0 and num or 1)
	elseif kind == FXK.DEATH then
		E.death(src, pos, dir, num > 0)
	elseif kind == FXK.GOLEM_POSE then
		local name = U.get(src, "Name")
		if type(name) == "string" then
			local rig = rigFor(src, name)
			rig.pose = math.floor(num)
		end
	elseif kind == FXK.WEAPON then
		setWeaponModel(src, math.floor(num))
	elseif kind == FXK.MISSED_PARRY then
		E.missedParry(pos)
	elseif kind == FXK.BLOCK_STATE then
		-- Golems show their guard with a pose; characters get a shield plane.
		local name = U.get(src, "Name")
		if not (type(name) == "string" and string.sub(name, 1, 9) == "PA_Golem_") then
			setShield(src, num > 0)
		end
	elseif kind == FXK.STUN then
		if num >= 1 then
			E.stars(src, num)
		end
	elseif kind == FXK.SHOWCASE then
		E.showcase(math.floor(num), pos)
	elseif kind == FXK.ANNOUNCE then
		local name = DIFFICULTIES[math.floor(num)] or "?"
		UI.showBanner("DUELIST DIFFICULTY:  " .. string.upper(name), C.parry, 2.5)
	elseif kind == FXK.SKILL then
		V.flash(pos + V3(0, 0.6, 0), C.white, 1, 4, 0.2, 0.4)
		V.ring(groundBelow(pos) + V3(0, 0.1, 0), UP, 1, 5, 0.35, C.white, { segments = 18, width = 0.3 })
		if isMe(src) then
			punch(2, 0.2)
		end
	elseif kind == FXK.RESPAWN then
		E.respawn(pos)
	end
end

local function readFx()
	for i = 1, FX.count do
		local ok, err = pcall(onFx, FX.kind[i], FX.src[i], FX.dst[i], FX.pos[i] or ZERO, FX.dir[i] or ZERO, FX.num[i] or 0)
		if not ok then
			warn("[ParryArena] fx error: " .. tostring(err))
		end
	end
end

------------------------------------------------------------------------------------------
-- MAIN LOOP
------------------------------------------------------------------------------------------

pcall(UI.build)

local lastWeaponScan = 0
local function scanWeapons(now)
	-- Late joiners: pick up weapons other players already hold from their attribute.
	if now < lastWeaponScan then
		return
	end
	lastWeaponScan = now + 2
	pcall(function()
		for p in game.World.Each("Player") do
			local character = U.get(p, "Character")
			if character ~= nil then
				local ok, weapon = pcall(function()
					return character.attr.PA_Weapon
				end)
				if ok and type(weapon) == "number" then
					setWeaponModel(character, weapon)
				end
			end
		end
	end)
end

function Update(dt)
	local now = tick()
	dt = math.min(dt, 0.1)
	refreshRefs(now)

	local ok, err = pcall(readInput, now)
	if not ok and IN.ok then
		IN.ok = false
		warn("[ParryArena] input error: " .. tostring(err))
	end

	readHud()
	readFx()

	discoverGolems(now)
	scanWeapons(now)
	stepRigs(dt, now)
	stepWeapons()
	stepShields(dt)
	stepDust(now)
	E.stepDashes(now)
	E.stepTrails(now)
	E.stepStars(dt, now)
	stepLock(dt)
	P.step(dt)
	V.stepText(dt)
	stepCamera(dt)
	UI.step(dt, now)
end

print("[ParryArena] client ready")
