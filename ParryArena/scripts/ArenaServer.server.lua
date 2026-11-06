--[[
	PARRY ARENA  -  server script (Luduvo)
	=======================================

	Attach this as a SERVER script to an Instance named "ArenaServer" (an empty Part is fine).
	Attach ArenaClient.client.lua as a CLIENT script to a *different* Instance ("ArenaClient").

	This script is authoritative for everything that matters:
	  * builds the arena (floor, walls, parkour corridor, the VFX pack showcase platforms)
	  * owns every fighter (player characters + stone training golems)
	  * runs the parry-based combat rules: 4-hit combo, block, parry window, posture,
	    guard break, critical + feint, weapon skills, stun, knockback, stamina
	  * runs movement abilities that need authority: dash (+ perfect dodge), slide,
	    slide leap, double jump, wall run
	  * drives the training dummies and the AI duelist (5 difficulty presets)
	  * tells clients what happened through the PA_Fx event table so they can draw VFX

	The tuning numbers come from the original "Parry Based Combat System X Movement System"
	configuration (Combat / Health / Stamina / Dodge / Slide / WallRun / AI). Timings that the
	original read from animation markers (swing / hit) are fixed numbers here, because the
	Roblox animations cannot be brought over to Luduvo.
]]

------------------------------------------------------------------------------------------
-- CONFIG
------------------------------------------------------------------------------------------

local CONFIG = {
	-- Combo
	ComboLength = 4,
	ComboResetTime = 1,
	AttackDuration = 0.35,
	M1Cooldown = 0.2,
	LastM1Cooldown = 0.55,

	-- Swing = the moment the strike becomes readable (parry timing is measured from it).
	-- Hit   = the moment the hitbox opens.
	PlayerSwingDelay = 0.12,
	PlayerHitDelay = 0.24,
	DummySwingDelay = 0.26,
	DummyHitDelay = 0.39,

	HitboxSize = Vector3.new(6, 7, 8),
	HitboxForward = 2,
	HitboxLifetime = 0.25,

	-- Critical (R). Feintable (right mouse) until its swing.
	CriticalSwingDelay = 0.4,
	CriticalHitDelay = 0.52,
	CriticalDuration = 0.85,
	CriticalCooldown = 3,
	CriticalHitboxSize = Vector3.new(9, 7, 13),
	CriticalHitboxForward = 3,
	CriticalHitboxLifetime = 0.2,
	CriticalLunge = 5,
	FeintCooldown = 1.25,
	FeintMinimumDelay = 0.04,

	-- Weapon skill (E)
	SkillCooldown = 4,
	SkillStaminaCost = 6,
	SkillWalkSpeed = 6,
	SkillHitboxSize = Vector3.new(9, 7, 10),
	SkillHitboxForward = 3,

	-- Stun / block / posture
	StunDuration = 0.71,
	BlockBreakStunDuration = 1.25,
	HitstunWalkSpeed = 4,
	BlockWalkSpeed = 4,
	BlockCooldown = 0.1,
	MaxPosture = 100,
	PostureGainPerBlock = 31,
	SkillPostureGain = 92,
	ParryPostureGain = 18,
	PostureDecayRate = 1.6,
	PostureDecayDelay = 1.6,
	PostureHighThreshold = 80,
	PostureHighDecayDelay = 4,

	-- Parry. A parry compares when block was pressed with when the attacker swung:
	-- pressed up to ParrySwingTolerance early, or up to ParryWindow late.
	ParryWindow = 0.16,
	ParrySwingTolerance = 0.07,
	CriticalParryWindow = 0.19,
	SkillParryWindow = 0.16,
	SkillParrySwingTolerance = 0.08,
	ParryAttemptDuration = 0.35, -- a block this fresh was a parry try; a miss is punished
	ParryTapGrace = 0.3, -- a released tap still counts for this long
	MistimedParryLock = 0.75,
	ParryRecoveryLock = 0.9,
	ParryTradeCounterDelay = 0.6,
	-- Luduvo has no ping API, so the block press is shifted back by this flat amount.
	LatencyCompensation = 0.05,

	-- Knockback (distance in studs over duration in seconds)
	M4KnockbackDistance = 7,
	M4KnockbackDuration = 0.35,
	CriticalKnockbackDistance = 12,
	CriticalKnockbackDuration = 0.4,
	CriticalLaunchSpeed = 28,
	GuardBreakPushDistance = 3,

	-- Dash (Q) and perfect dodge
	DashSpeed = 135,
	DashDuration = 0.25,
	DashIFrameDuration = 0.12,
	DashCooldown = 0.65,
	PerfectDodgeWindow = 0.18,
	PerfectDodgeIFrames = 0.3,
	PerfectBoostSpeed = 200,
	PerfectBoostDuration = 0.175,

	-- Soft lock: a swing turns toward the nearest enemy inside this cone.
	SwingReaimRadius = 20,
	SwingReaimMaxAngle = 75,
	LungeMaxDistance = 3,

	-- Health / stamina
	MaxHealth = 100,
	BlockBreakDamage = 8.4,
	HealthRegenRate = 4, -- per second, out of combat only
	CombatTagDuration = 8, -- the original used 45s; shortened for a practice arena
	MaxStamina = 100,
	StaminaHitGain = 11,
	StaminaHitLoss = 4,
	CriticalStaminaCost = 22,
	StaminaRegenDelay = 0.75,
	StaminaRegenRate = 19,
	StaminaCombatRegenRate = 7,

	-- Slide (Left Ctrl while running) and slide leap (Space while sliding)
	SlideMinimumEntrySpeed = 10,
	SlideBoost = 18,
	SlideMaxSpeed = 75,
	SlideFriction = 20,
	SlideMinimumSpeed = 8,
	SlideMaxDuration = 6,
	SlideSteerSharpness = 3,
	SlideCooldown = 0.35,
	SlideHipHeightScale = 0.5,
	LeapCarry = 1,
	LeapForward = 40,
	LeapUp = 60,
	LeapDriveDuration = 0.45,

	-- Double jump
	AirJumps = 1,
	DoubleJumpVelocity = 50,

	-- Wall run
	WallRunEnabled = true,
	WallDetectionDistance = 4,
	WallRunMinimumMove = 0.1,
	WallRunMaxSurfaceTilt = 0.3,
	WallRunGroundClearance = 4,
	WallRunEntryNormalDotMinimum = -0.85,
	WallRunSpeed = 45,
	WallRunMaxDuration = 2.5,
	WallRunEndGrace = 0.3,
	WallJumpOutward = 14,
	WallJumpAlong = 54,
	WallJumpUp = 28,
	WallJumpDrive = 0.14,
	WallJumpBlockDuration = 0.4,

	-- Characters
	GroundProbe = 4.2, -- studs below the character root that still count as standing
	GolemRespawnDelay = 3,
	DefaultDifficulty = "Hard",

	ShowHitboxes = false, -- "/hitboxes" in chat toggles it
	HudRate = 10, -- HUD updates per second per player
}

local DAMAGE = {
	M1 = 2.8,
	M2 = 2.8,
	M3 = 3.5,
	M4 = 4.9,
	Critical = 9.8,
	Skill = 0.6,
}

-- Weapon order matters: clients send the index. Visual models are built client side.
local WEAPONS = {
	{
		Name = "Fists",
		WalkSpeed = 26,
		CriticalName = "Smash",
		SkillName = "Barrage",
		SkillDamage = 0.35,
		SkillHits = 9,
		SkillInterval = 0.11,
		SkillFirstHit = 0.12,
	},
	{
		Name = "Blade",
		WalkSpeed = 23,
		CriticalName = "Ichimonji",
		SkillName = "Whirlwind",
		SkillDamage = 2.2,
		SkillHits = 3,
		SkillInterval = 0.22,
		SkillFirstHit = 0.2,
		SkillAllAround = true,
	},
	{
		Name = "Rapier",
		WalkSpeed = 24,
		CriticalName = "Pierce",
		SkillName = "Volley",
		SkillDamage = 0.9,
		SkillHits = 6,
		SkillInterval = 0.13,
		SkillFirstHit = 0.14,
		HitboxSize = Vector3.new(5, 5, 11),
		HitboxForward = 5.5,
	},
}

-- AI duelist presets, ported from the original AI configuration (Base + per difficulty overrides).
local AI_BASE = {
	HealthMultiplier = 1,
	WalkSpeed = 21,
	ThinkInterval = 0.08,
	AttackRange = 7.5,
	PreferredDistance = 5.5,
	MinDistance = 3.5,
	ReactionTimeMin = 0.34,
	ReactionTimeMax = 0.5,
	AttackParryChance = 0.58,
	SkillParryChance = 0.46,
	DodgeChance = 0.11,
	BlockChance = 0.24,
	BackDashWeight = 0.3,
	LeftDashWeight = 0.35,
	AttackChance = 0.75,
	CriticalChance = 0.1,
	SkillChance = 0.12,
	SkillCooldown = 8,
	FeintChance = 0.18,
	FeintCooldown = 4,
	BlockHoldMin = 0.6,
	BlockHoldMax = 1,
	AttackCooldown = 0.55,
	DashCooldown = 1.4,
	CriticalCooldown = 6,
	ParryAttemptCooldown = 0.15,
}

local AI_DIFFICULTY_ORDER = { "Weak", "Normal", "Hard", "Very Hard", "Impossible" }

local AI_OVERRIDES = {
	Weak = {
		HealthMultiplier = 0.8, WalkSpeed = 16, ReactionTimeMin = 0.5, ReactionTimeMax = 0.75,
		AttackParryChance = 0.22, SkillParryChance = 0.15, DodgeChance = 0.05, BlockChance = 0.2,
		AttackChance = 0.45, CriticalChance = 0.03, SkillChance = 0.04, SkillCooldown = 14,
		FeintChance = 0.04, AttackCooldown = 1.1, CriticalCooldown = 9,
	},
	Normal = {
		HealthMultiplier = 1, WalkSpeed = 19, ReactionTimeMin = 0.4, ReactionTimeMax = 0.58,
		AttackParryChance = 0.42, SkillParryChance = 0.32, DodgeChance = 0.08, BlockChance = 0.25,
		AttackChance = 0.65, CriticalChance = 0.07, SkillChance = 0.08, SkillCooldown = 11,
		FeintChance = 0.11, AttackCooldown = 0.75, CriticalCooldown = 7.5,
	},
	Hard = {
		HealthMultiplier = 1.4, WalkSpeed = 23, ThinkInterval = 0.06, PreferredDistance = 5,
		ReactionTimeMin = 0.22, ReactionTimeMax = 0.34, AttackParryChance = 0.7, SkillParryChance = 0.58,
		DodgeChance = 0.12, BlockChance = 0.18, AttackChance = 0.88, CriticalChance = 0.18,
		SkillChance = 0.16, SkillCooldown = 6.5, FeintChance = 0.28, AttackCooldown = 0.44,
		DashCooldown = 1.15, CriticalCooldown = 4.5, ParryAttemptCooldown = 0.12,
	},
	["Very Hard"] = {
		HealthMultiplier = 2.5, WalkSpeed = 28, ThinkInterval = 0.05, PreferredDistance = 4.5,
		ReactionTimeMin = 0.14, ReactionTimeMax = 0.22, AttackParryChance = 0.84, SkillParryChance = 0.75,
		DodgeChance = 0.12, BlockChance = 0.04, BlockHoldMin = 0.35, BlockHoldMax = 0.6,
		AttackChance = 1, CriticalChance = 0.3, SkillChance = 0.2, SkillCooldown = 6, FeintChance = 0.45,
		AttackCooldown = 0.34, DashCooldown = 0.9, CriticalCooldown = 2.2, ParryAttemptCooldown = 0.08,
	},
	Impossible = {
		HealthMultiplier = 3, WalkSpeed = 28, ThinkInterval = 0.05, PreferredDistance = 4.5,
		ReactionTimeMin = 0.05, ReactionTimeMax = 0.08, AttackParryChance = 1, SkillParryChance = 1,
		DodgeChance = 0, BlockChance = 0, BlockHoldMin = 0.35, BlockHoldMax = 0.6, AttackChance = 1,
		CriticalChance = 0.3, SkillChance = 0.28, SkillCooldown = 4.5, FeintChance = 0.45,
		AttackCooldown = 0.34, DashCooldown = 0.05, CriticalCooldown = 2.2, ParryAttemptCooldown = 0.02,
	},
}

------------------------------------------------------------------------------------------
-- SHARED PROTOCOL (must match ArenaClient.client.lua exactly)
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

-- HUD state bits
local ST = { BLOCK = 1, STUN = 2, ATTACK = 4, SLIDE = 8, WALL = 16, COMBAT = 32, DASH = 64, BROKEN = 128 }

------------------------------------------------------------------------------------------
-- UTILITIES
------------------------------------------------------------------------------------------

local V3 = Vector3.new
local ZERO = V3(0, 0, 0)
local UP = V3(0, 1, 0)
local FORWARD = V3(0, 0, -1)
local ROOT = self

local function try(fn, ...)
	local ok, result = pcall(fn, ...)
	if ok then
		return result
	end
	return nil
end

local function set(inst, key, value)
	if inst == nil then
		return false
	end
	local ok = pcall(function()
		inst[key] = value
	end)
	return ok
end

local function get(inst, key)
	if inst == nil then
		return nil
	end
	local ok, value = pcall(function()
		return inst[key]
	end)
	if ok then
		return value
	end
	return nil
end

local function call(inst, method, ...)
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

-- Only a definite "no" counts as gone; if the check itself fails we assume it still exists.
local function alive(inst)
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

local function dot(a, b)
	return a.X * b.X + a.Y * b.Y + a.Z * b.Z
end

local function cross(a, b)
	return V3(a.Y * b.Z - a.Z * b.Y, a.Z * b.X - a.X * b.Z, a.X * b.Y - a.Y * b.X)
end

local function len(v)
	return math.sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z)
end

local function unit(v, fallback)
	local m = len(v)
	if m < 1e-5 then
		return fallback or ZERO
	end
	return v / m
end

local function flat(v)
	return V3(v.X, 0, v.Z)
end

local function lerp(a, b, t)
	return a + (b - a) * t
end

local function clamp(x, lo, hi)
	if x < lo then
		return lo
	end
	if x > hi then
		return hi
	end
	return x
end

local function rand(lo, hi)
	return lo + math.random() * (hi - lo)
end

local function rightOf(forward)
	return cross(forward, UP)
end

-- Turning by yaw angle. Lerping two flat vectors stalls when they point in opposite
-- directions (the blend stays on the same line), so turn the angle instead.
local function yawOf(v)
	return math.atan2(-v.X, -v.Z)
end

local function fromYaw(a)
	return V3(-math.sin(a), 0, -math.cos(a))
end

local function turnTowards(current, desired, sharpness, dt)
	local a, b = yawOf(current), yawOf(desired)
	local delta = (b - a + math.pi) % (2 * math.pi) - math.pi
	return fromYaw(a + delta * (1 - math.exp(-sharpness * dt)))
end

-- Orientation (YXZ Euler degrees) that points -Z along `forward`.
local function orientationFor(forward)
	local r = try(function()
		return rotation.to_euler(rotation.look(forward, UP))
	end)
	if r ~= nil then
		return r
	end
	return V3(0, math.deg(math.atan2(-forward.X, -forward.Z)), 0)
end

------------------------------------------------------------------------------------------
-- EVENTS OUT
------------------------------------------------------------------------------------------

local fxSentThisFrame = 0

local function fx(kind, src, dst, pos, dir, num)
	if fxSentThisFrame >= 60 then
		return -- each table carries at most 64 outgoing events per frame
	end
	fxSentThisFrame += 1
	local a = src or ROOT
	local b = dst or a
	pcall(function()
		FX:Push(kind, a, b, pos or ZERO, dir or ZERO, num or 0)
	end)
end

------------------------------------------------------------------------------------------
-- MAP
------------------------------------------------------------------------------------------

local PALETTE = {
	Floor = Color3.fromRGB(38, 41, 52),
	FloorTile = Color3.fromRGB(46, 50, 63),
	Line = Color3.fromRGB(205, 210, 225),
	Wall = Color3.fromRGB(58, 62, 78),
	WallTrim = Color3.fromRGB(255, 176, 106),
	Stone = Color3.fromRGB(150, 150, 160),
	-- The showcase platforms of the Combat VFX Pack, by name
	Red = Color3.fromRGB(216, 74, 74),
	Orange = Color3.fromRGB(238, 138, 46),
	Dark = Color3.fromRGB(46, 47, 56),
	Light = Color3.fromRGB(207, 210, 218),
	Purple = Color3.fromRGB(139, 79, 214),
	Misc = Color3.fromRGB(63, 183, 168),
	Trail = Color3.fromRGB(233, 233, 239),
}

-- The material name that glows. "Neon" on most setups, looked up in case it differs.
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

local function spawnPrefab(name)
	local inst = try(game.Prefabs.Spawn, name)
	if inst == nil and name ~= "Part" then
		inst = try(game.Prefabs.Spawn, "Part")
	end
	if inst == nil then
		inst = try(Instance.new, "Part")
	end
	return inst
end

local MAP = try(Instance.new, "PA_Map")
if MAP ~= nil then
	call(MAP, "AddComponent", "Transient")
end

local function makePart(o)
	local p = spawnPrefab(o.shape or "Part")
	if p == nil then
		return nil
	end
	set(p, "Name", o.name or "Part")
	set(p, "Parent", o.parent or MAP or game.World)
	set(p, "Anchored", o.anchored ~= false)
	if o.size then
		set(p, "Size", o.size)
	end
	if o.color then
		set(p, "Color", o.color)
	end
	set(p, "Transparency", o.transparency or 0)
	if o.material then
		set(p, "Material", o.material)
	end
	if o.collide == false then
		set(p, "Collider", "None")
	end
	if o.trigger then
		set(p, "Trigger", true)
	end
	if o.noShadow then
		set(p, "ShadowOff", true)
	end
	if o.orientation then
		set(p, "Orientation", o.orientation)
	end
	if o.pos then
		set(p, "Position", o.pos)
	end
	return p
end

local function label(part, text, color, offset, size)
	if part == nil then
		return
	end
	call(part, "AddComponent", "Billboard")
	set(part, "BillboardText", text)
	set(part, "BillboardColor", color or V3(1, 1, 1))
	set(part, "BillboardOffset", offset or V3(0, 3, 0))
	set(part, "BillboardWorldSize", size or 1.6)
	set(part, "BillboardMaxDistance", 140)
end

local ARENA_HALF = 120
-- Pads are checked by position every frame instead of through Touched, so they keep
-- working while someone stands still on them.
local showcaseZones = {} -- { centre, half, index, nextAt }
local difficultyZones = {} -- { centre, half, name }
local spawnPoints = {}

local function buildMap()
	-- Floor with a subtle checker so speed reads at a glance.
	makePart({ name = "Floor", size = V3(ARENA_HALF * 2, 2, ARENA_HALF * 2), pos = V3(0, -1, 0), color = PALETTE.Floor })
	for ix = -5, 5 do
		for iz = -5, 5 do
			if (ix + iz) % 2 == 0 then
				makePart({
					name = "Tile",
					size = V3(20, 0.05, 20),
					pos = V3(ix * 20, 0.025, iz * 20),
					color = PALETTE.FloorTile,
					collide = false,
					noShadow = true,
				})
			end
		end
	end

	-- Perimeter walls are tall on purpose: every one of them can be wall-run.
	local wallHeight = 30
	for _, w in ipairs({
		{ V3(0, wallHeight / 2, -ARENA_HALF - 2), V3(ARENA_HALF * 2 + 8, wallHeight, 4) },
		{ V3(0, wallHeight / 2, ARENA_HALF + 2), V3(ARENA_HALF * 2 + 8, wallHeight, 4) },
		{ V3(-ARENA_HALF - 2, wallHeight / 2, 0), V3(4, wallHeight, ARENA_HALF * 2 + 8) },
		{ V3(ARENA_HALF + 2, wallHeight / 2, 0), V3(4, wallHeight, ARENA_HALF * 2 + 8) },
	}) do
		makePart({ name = "Wall", pos = w[1], size = w[2], color = PALETTE.Wall })
		makePart({
			name = "WallTrim",
			pos = w[1] + V3(0, wallHeight / 2 + 0.25, 0),
			size = V3(w[2].X + 0.2, 0.5, w[2].Z + 0.2),
			color = PALETTE.WallTrim,
			material = NEON,
			noShadow = true,
		})
	end

	-- Spawn plaza (south)
	local spawnPad = makePart({ name = "SpawnPlaza", size = V3(26, 0.4, 16), pos = V3(0, 0.2, 78), color = PALETTE.Light })
	label(spawnPad, "PARRY ARENA  -  press H for controls", V3(1, 0.85, 0.6), V3(0, 6, 0), 2.4)
	for i = -1, 1 do
		local sp = spawnPrefab("SpawnLocation")
		if sp ~= nil then
			set(sp, "Parent", MAP or game.World)
			set(sp, "Position", V3(i * 8, 0.6, 80))
			set(sp, "Anchored", true)
			table.insert(spawnPoints, sp)
		end
	end

	-- Duel circle (centre): the AI duelist lives here.
	local duelCentre = V3(0, 0, -24)
	local segments = 48
	local radius = 26
	for i = 1, segments do
		local a = (i / segments) * math.pi * 2
		local p = duelCentre + V3(math.cos(a) * radius, 0.06, math.sin(a) * radius)
		local tangent = V3(-math.sin(a), 0, math.cos(a))
		makePart({
			name = "DuelLine",
			size = V3(0.35, 0.08, (2 * math.pi * radius) / segments + 0.1),
			pos = p,
			orientation = orientationFor(tangent),
			color = PALETTE.WallTrim,
			material = NEON,
			collide = false,
			noShadow = true,
		})
	end

	-- Difficulty pads: walk onto one to set the duelist's difficulty.
	for i, name in ipairs(AI_DIFFICULTY_ORDER) do
		local hue = 0.33 - (i - 1) * 0.0825
		local color = Color3.fromHSV(math.max(hue, 0), 0.75, 0.95)
		local x = (i - 3) * 11
		makePart({ name = "DifficultyBase", size = V3(8, 0.3, 6), pos = V3(x, 0.15, -58), color = PALETTE.Dark })
		local pad = makePart({
			name = "DifficultyPad",
			size = V3(7, 0.2, 5),
			pos = V3(x, 0.4, -58),
			color = color,
			transparency = 0.35,
			collide = false,
			material = NEON,
		})
		label(pad, name, color, V3(0, 2.5, 0), 1.4)
		table.insert(difficultyZones, { centre = V3(x, 0.3, -58), half = V3(4, 0, 3), name = name })
	end

	-- Training row (between spawn and duel circle)
	-- Pads are coloured after the effect each dummy shows off.
	-- (golems are spawned later, on these positions)

	-- Parkour corridor (east): two tall parallel walls for wall runs and wall jumps.
	makePart({ name = "RunWallA", size = V3(3, 26, 80), pos = V3(76, 13, 10), color = PALETTE.Wall })
	makePart({ name = "RunWallB", size = V3(3, 26, 80), pos = V3(94, 13, 10), color = PALETTE.Wall })
	for i = 0, 3 do
		makePart({
			name = "RunWallStripe",
			size = V3(3.1, 0.4, 80),
			pos = V3(76, 4 + i * 6, 10),
			color = PALETTE.WallTrim,
			material = NEON,
			collide = false,
			noShadow = true,
		})
		makePart({
			name = "RunWallStripe",
			size = V3(3.1, 0.4, 80),
			pos = V3(94, 4 + i * 6, 10),
			color = PALETTE.WallTrim,
			material = NEON,
			collide = false,
			noShadow = true,
		})
	end
	local corridorSign = makePart({ name = "CorridorSign", size = V3(14, 0.4, 6), pos = V3(85, 0.2, 54), color = PALETTE.Dark })
	label(corridorSign, "WALL RUN  -  jump at a wall while moving, Space to kick off", V3(1, 0.85, 0.6), V3(0, 4, 0), 1.5)

	-- Slide lane (west): long, glossy, with a kicker at the end for slide leaps.
	makePart({ name = "SlideLane", size = V3(14, 0.3, 120), pos = V3(-90, 0.15, 0), color = PALETTE.Trail })
	for i = -5, 5 do
		makePart({
			name = "SlideArrow",
			size = V3(6, 0.06, 1.2),
			pos = V3(-90, 0.33, i * 10),
			color = PALETTE.Misc,
			material = NEON,
			collide = false,
			noShadow = true,
		})
	end
	local kicker = makePart({
		name = "Kicker",
		shape = "Wedge",
		size = V3(14, 4, 12),
		pos = V3(-90, 2, -66),
		color = PALETTE.Orange,
		orientation = V3(0, 180, 0),
	})
	local laneSign = makePart({ name = "LaneSign", size = V3(14, 0.4, 6), pos = V3(-90, 0.2, 66), color = PALETTE.Dark })
	label(laneSign, "SLIDE LANE  -  run, hold Ctrl to slide, Space to leap", V3(1, 0.85, 0.6), V3(0, 4, 0), 1.5)
	if kicker == nil then
		warn("[ParryArena] could not build the kicker ramp")
	end

	-- VFX PACK SHOWCASE (north-east). The seven coloured platforms from the Combat VFX Pack,
	-- laid out in the same order with their original sizes. Standing on top of one plays its
	-- effect family on loop.
	local showcase = {
		{ "Red", V3(10, 20, 10), "HARD HIT" },
		{ "Orange", V3(10, 11, 10), "PARRY" },
		{ "Dark", V3(15, 15, 18), "DASH / FLASHSTEP" },
		{ "Light", V3(20, 10, 20), "SHOCKWAVE" },
		{ "Purple", V3(10, 11, 10), "PERFECT BLOCK" },
		{ "Misc", V3(10, 11, 10), "STARS / DROPS / ROCKS" },
		{ "Trail", V3(25, 1, 25), "TRAILS / SWING" },
	}
	local x = 30
	for index, item in ipairs(showcase) do
		local size = item[2]
		local centre = V3(x + size.X / 2, size.Y / 2, -96)
		local platform = makePart({ name = item[1] .. "Platform", size = size, pos = centre, color = PALETTE[item[1]] })
		label(platform, item[3], PALETTE[item[1]], V3(0, size.Y / 2 + 3, 0), 1.3)
		table.insert(showcaseZones, {
			centre = centre + V3(0, size.Y / 2, 0),
			half = V3(size.X / 2, 0, size.Z / 2),
			index = index,
			nextAt = 0,
		})
		x += size.X + 4
	end

	-- Light touch on the sky: warm, low sun like the original anime skybox.
	pcall(function()
		game.Lighting.Sun.Elevation = 32
		game.Lighting.Sun.Azimuth = 210
		game.Lighting.Sun.Tint = Color3.fromRGB(255, 226, 200)
	end)
	pcall(function()
		game.Lighting.Ambient.Tint = Color3.fromRGB(150, 160, 200)
	end)
end

buildMap()

-- Make sure players actually spawn: a place normally has a PlayerSpawner, but add one if not.
do
	local found = false
	pcall(function()
		for _ in game.World.Each("PlayerSpawner") do
			found = true
			break
		end
	end)
	if not found then
		local spawner = spawnPrefab("PlayerSpawner")
		if spawner ~= nil then
			set(spawner, "Parent", game.World)
			pcall(function()
				spawner.PlayerSpawner.respawnDelay = 2
			end)
		end
	end
end

------------------------------------------------------------------------------------------
-- FIGHTERS
------------------------------------------------------------------------------------------

local fighters = {} -- list
local byInst = {} -- Instance -> fighter (fast path; see fighterOf)
local byUser = {} -- userId -> fighter
local weaponByUser = {} -- userId -> weapon index, survives respawns
local golemRoots = {} -- list of golem root Instances (raycast excludes)

-- Instance handles may not be identical userdata every time the engine hands one out,
-- so a table lookup is only the fast path; fall back to comparing with ==.
local function fighterOf(inst)
	if inst == nil then
		return nil
	end
	local f = byInst[inst]
	if f ~= nil then
		return f
	end
	for _, other in ipairs(fighters) do
		if other.inst == inst then
			return other
		end
	end
	return nil
end

local function newFighter(inst, kind)
	local f = {
		inst = inst,
		kind = kind, -- "player" | "golem"
		dead = false,
		hp = CONFIG.MaxHealth,
		maxHp = CONFIG.MaxHealth,
		stamina = CONFIG.MaxStamina,
		staminaLossAt = -100,
		posture = 0,
		postureGainAt = -100,
		weapon = 1,
		combo = 0,
		windowStart = -100,
		m1ReadyAt = 0,
		attack = nil,
		swingTime = nil,
		blocking = false,
		blockStart = nil,
		blockReadyAt = 0,
		parryTapUntil = 0,
		stunUntil = 0,
		stunKind = nil,
		attackLockUntil = 0,
		iframeUntil = 0,
		dash = nil,
		dashReadyAt = 0,
		critReadyAt = 0,
		feintReadyAt = 0,
		skillReadyAt = 0,
		aim = FORWARD,
		move = ZERO,
		flags = 0,
		slide = nil,
		slideReadyAt = 0,
		wall = nil,
		wallReadyAt = 0,
		drive = nil,
		airJumpsUsed = 0,
		grounded = true,
		combatUntil = 0,
		regenCarry = 0,
		parryStreak = 0,
		lastTarget = nil,
		hudAt = 0,
		faceUntil = 0,
		autoRotateOff = false,
		lastWalk = nil,
		lastJump = nil,
	}
	table.insert(fighters, f)
	byInst[inst] = f
	return f
end

local function removeFighter(f)
	f.dead = true
	byInst[f.inst] = nil
	if f.userId ~= nil and byUser[f.userId] == f then
		byUser[f.userId] = nil
	end
	for i, other in ipairs(fighters) do
		if other == f then
			table.remove(fighters, i)
			break
		end
	end
end

local function posOf(f)
	if f.kind == "golem" then
		return f.pos
	end
	return get(f.inst, "Position") or ZERO
end

local function forwardOf(f)
	if f.kind == "golem" then
		return f.facing
	end
	local aim = unit(flat(f.aim), nil)
	if aim ~= nil and len(aim) > 0.5 then
		return aim
	end
	local look = call(f.inst, "TransformDirection", FORWARD)
	if look ~= nil then
		return unit(flat(look), FORWARD)
	end
	return FORWARD
end

local function getHp(f)
	if f.kind == "golem" then
		return f.hp
	end
	return get(f.inst, "Health") or 0
end

local function getMaxHp(f)
	if f.kind == "golem" then
		return f.maxHp
	end
	return get(f.inst, "MaxHealth") or CONFIG.MaxHealth
end

local function weaponOf(f)
	return WEAPONS[f.weapon] or WEAPONS[1]
end

local function isStunned(f, now)
	return now < f.stunUntil
end

-- Removed, dead, or a golem waiting to respawn.
local function isOut(f)
	return f.dead or (f.kind == "golem" and f.pose == POSE.DEAD)
end

local function tagCombat(f, now)
	f.combatUntil = now + CONFIG.CombatTagDuration
end

local function setPose(f, pose)
	if f.kind ~= "golem" or f.pose == pose then
		return
	end
	f.pose = pose
	fx(FXK.GOLEM_POSE, f.inst, f.inst, f.pos, f.facing, pose)
end

local function excludeListFor(f)
	if f.kind == "golem" then
		return golemRoots
	end
	if f.exclude == nil or (f.excludeAt or 0) < tick() then
		local list = { f.inst }
		local descendants = call(f.inst, "GetDescendants")
		if descendants ~= nil then
			for _, d in ipairs(descendants) do
				table.insert(list, d)
			end
		end
		f.exclude = list
		f.excludeAt = tick() + 3
	end
	return f.exclude
end

local function raycast(origin, direction, distance, exclude)
	return try(function()
		return game.Physics.Raycast(origin, direction, distance, { ExcludeInstances = exclude })
	end)
end

local function velocityOf(f)
	if f.kind == "golem" then
		return f.vel
	end
	return get(f.inst, "Velocity") or ZERO
end

local function setVelocity(f, v)
	if f.kind == "golem" then
		f.vel = v
	else
		set(f.inst, "Velocity", v)
	end
end

-- Movement bursts. Every forced movement goes through here so a new one replaces the old.
--   burst: v = dir * speed * (1 - a)^2   (dashes, matching the original falloff)
--   knock: v = dir * speed * (1 - a)     (knockback, travels speed * duration / 2)
--   leap : pace eases from `speed` to `endSpeed`
--   hold : constant (wall run kick-off drive)
local function drive(f, kind, dir, speed, duration, extra)
	f.drive = {
		kind = kind,
		dir = unit(flat(dir), forwardOf(f)),
		speed = speed,
		duration = duration,
		start = tick(),
		endSpeed = extra and extra.endSpeed or 0,
		launch = extra and extra.launch or nil,
	}
end

local function knockback(f, dir, distance, duration, launch)
	-- distance = speed * duration / 2 for a linear falloff
	drive(f, "knock", dir, 2 * distance / duration, duration, { launch = launch })
end

-- Stop a slide from anywhere (attacks, stuns, dashes), putting the hip height back.
local function cancelSlide(f)
	if f.slide == nil then
		return
	end
	f.slide = nil
	if f.kind == "player" and f.hipHeight ~= nil then
		set(f.inst, "HipHeight", f.hipHeight)
	end
end

------------------------------------------------------------------------------------------
-- STATS
------------------------------------------------------------------------------------------

local function drainStamina(f, amount, now)
	f.stamina = math.max(0, f.stamina - amount)
	f.staminaLossAt = now
end

local function gainStamina(f, amount)
	f.stamina = math.min(CONFIG.MaxStamina, f.stamina + amount)
end

local function addPosture(f, amount, now)
	f.posture = math.min(CONFIG.MaxPosture, f.posture + amount)
	f.postureGainAt = now
	return f.posture
end

local onGolemDied -- forward declaration

local function damage(victim, amount, attacker, now)
	if victim.dead then
		return
	end
	if victim.kind == "golem" then
		victim.hp = math.max(0, victim.hp - amount)
		if victim.hp <= 0 then
			onGolemDied(victim, attacker, now)
		end
	else
		local ok = pcall(function()
			victim.inst:TakeDamage(amount, attacker and attacker.inst or nil)
		end)
		if not ok then
			set(victim.inst, "Health", math.max(0, (get(victim.inst, "Health") or 0) - amount))
		end
	end
end

------------------------------------------------------------------------------------------
-- COMBAT CORE
------------------------------------------------------------------------------------------

local function clearAttack(f)
	f.attack = nil
	if f.kind == "golem" and f.pose ~= POSE.DEAD then
		setPose(f, f.blocking and POSE.BLOCK or POSE.IDLE)
	end
end

local function setBlocking(f, on, now, startTime)
	if f.blocking == on then
		return
	end
	f.blocking = on
	if on then
		f.blockStart = startTime or now
		f.parryTapUntil = (startTime or now) + CONFIG.ParryTapGrace
		setPose(f, POSE.BLOCK)
	else
		f.blockReadyAt = now + CONFIG.BlockCooldown
		if f.kind == "golem" and f.attack == nil and not isStunned(f, now) then
			setPose(f, POSE.IDLE)
		end
	end
	fx(FXK.BLOCK_STATE, f.inst, f.inst, posOf(f), forwardOf(f), on and 1 or 0)
end

local function stun(f, duration, now, kind)
	f.stunUntil = math.max(f.stunUntil, now + duration)
	f.stunKind = kind
	cancelSlide(f)
	f.wall = nil
	if f.ai ~= nil then
		f.ai.comboLeft = 0
		f.ai.plan = nil
	end
	if f.attack ~= nil then
		clearAttack(f)
	end
	if f.kind == "golem" then
		setPose(f, kind == "Break" and POSE.BROKEN or (kind == "Parry" and POSE.PARRIED or POSE.STUN))
	end
	fx(FXK.STUN, f.inst, f.inst, posOf(f), ZERO, duration)
end

local function faceTowards(f, target)
	local d = unit(flat(posOf(target) - posOf(f)), nil)
	if d == nil or len(d) < 0.5 then
		return
	end
	if f.kind == "golem" then
		f.facing = d
	else
		f.aim = d
		f.faceUntil = tick() + 0.35
	end
end

local function hitboxFor(f, name)
	local w = weaponOf(f)
	if name == "Critical" then
		return CONFIG.CriticalHitboxSize, CONFIG.CriticalHitboxForward, CONFIG.CriticalHitboxLifetime
	elseif name == "Skill" then
		return CONFIG.SkillHitboxSize, CONFIG.SkillHitboxForward, 0.1
	end
	return w.HitboxSize or CONFIG.HitboxSize, w.HitboxForward or CONFIG.HitboxForward, CONFIG.HitboxLifetime
end

local function inHitbox(attacker, victim, size, forwardOffset, allAround)
	local origin = posOf(attacker)
	local target = posOf(victim)
	if allAround then
		local d = flat(target - origin)
		return len(d) <= size.Z / 2 + 1.5 and math.abs(target.Y - origin.Y) <= size.Y / 2 + 1
	end
	local fwd = forwardOf(attacker)
	local right = rightOf(fwd)
	local centre = origin + fwd * forwardOffset
	local d = target - centre
	local pad = 1 -- victim body radius
	return math.abs(dot(d, right)) <= size.X / 2 + pad
		and math.abs(d.Y) <= size.Y / 2 + pad
		and math.abs(dot(d, fwd)) <= size.Z / 2 + pad
end

local function showHitbox(attacker, size, forwardOffset)
	if not CONFIG.ShowHitboxes then
		return
	end
	local fwd = forwardOf(attacker)
	local box = makePart({
		name = "Hitbox",
		size = size,
		pos = posOf(attacker) + fwd * forwardOffset,
		orientation = orientationFor(fwd),
		color = V3(1, 0.2, 0.2),
		transparency = 0.75,
		collide = false,
		noShadow = true,
	})
	if box ~= nil then
		task.delay(0.25, function()
			call(box, "Destroy")
		end)
	end
end

local function isFrontal(attacker, victim)
	local toAttacker = unit(flat(posOf(attacker) - posOf(victim)), nil)
	if toAttacker == nil or len(toAttacker) < 0.5 then
		return true
	end
	return dot(forwardOf(victim), toAttacker) > 0
end

local function parryTiming(attackName)
	if attackName == "Critical" then
		return CONFIG.ParrySwingTolerance, CONFIG.CriticalParryWindow
	elseif attackName == "Skill" then
		return CONFIG.SkillParrySwingTolerance, CONFIG.SkillParryWindow
	end
	return CONFIG.ParrySwingTolerance, CONFIG.ParryWindow
end

local function isParryTimed(victim, attacker, attackName)
	if attacker.swingTime == nil or victim.blockStart == nil then
		return false
	end
	local tolerance, window = parryTiming(attackName)
	local latency = victim.kind == "player" and CONFIG.LatencyCompensation or 0
	local sinceSwing = (victim.blockStart - latency) - attacker.swingTime
	return sinceSwing >= -tolerance and sinceSwing <= window
end

-- A fresh press is a parry attempt, an old held guard is a block. A released tap always counts.
local function wasGoingForParry(victim, now)
	if victim.kind ~= "player" or victim.blockStart == nil then
		return false
	end
	if not victim.blocking then
		return true
	end
	return (now - victim.blockStart) + CONFIG.LatencyCompensation <= CONFIG.ParryAttemptDuration
end

local aiScheduleCounter -- forward declaration

local function breakGuard(attacker, victim, now)
	victim.posture = 0
	damage(victim, CONFIG.BlockBreakDamage, attacker, now)
	drainStamina(victim, CONFIG.StaminaHitLoss, now)
	gainStamina(attacker, CONFIG.StaminaHitGain)
	setBlocking(victim, false, now)
	stun(victim, CONFIG.BlockBreakStunDuration, now, "Break")
	knockback(victim, posOf(victim) - posOf(attacker), CONFIG.GuardBreakPushDistance, 0.25)
	fx(FXK.GUARD_BREAK, attacker.inst, victim.inst, posOf(victim), unit(flat(posOf(victim) - posOf(attacker)), FORWARD), 0)
end

local function applyDirectHit(attacker, victim, attackName, amount, now)
	local dir = unit(flat(posOf(victim) - posOf(attacker)), forwardOf(attacker))
	damage(victim, amount, attacker, now)
	drainStamina(victim, CONFIG.StaminaHitLoss, now)
	gainStamina(attacker, CONFIG.StaminaHitGain)

	-- Interrupt whatever the victim was doing.
	if victim.blocking then
		setBlocking(victim, false, now)
	end
	victim.dash = nil

	local heavy = attackName == "Critical" or attackName == "M4"
	fx(heavy and FXK.HEAVY_HIT or FXK.HIT, attacker.inst, victim.inst, posOf(victim), dir, amount)

	if isOut(victim) then
		return
	end
	stun(victim, CONFIG.StunDuration, now, "Hit")
	faceTowards(victim, attacker)

	if attackName == "M4" then
		knockback(victim, dir, CONFIG.M4KnockbackDistance, CONFIG.M4KnockbackDuration)
	elseif attackName == "Critical" then
		knockback(victim, dir, CONFIG.CriticalKnockbackDistance, CONFIG.CriticalKnockbackDuration, CONFIG.CriticalLaunchSpeed)
		fx(FXK.SHOCKWAVE, victim.inst, victim.inst, posOf(victim), dir, 1)
	end
end

-- The one place every hit gets resolved: dodged, parried, blocked, then a clean hit.
local function resolveHit(attacker, victim, attackName, amount, now)
	if isOut(victim) or isOut(attacker) then
		return false
	end

	-- Dash invincibility, with the perfect dodge on a hit that lands early in the dash.
	if now < victim.iframeUntil then
		if victim.dash ~= nil and not victim.perfectUsed and now - victim.dash.start <= CONFIG.PerfectDodgeWindow then
			victim.perfectUsed = true
			victim.iframeUntil = now + CONFIG.PerfectDodgeIFrames
			drive(victim, "burst", victim.dash.dir, CONFIG.PerfectBoostSpeed, CONFIG.PerfectBoostDuration)
			gainStamina(victim, CONFIG.StaminaHitGain)
			fx(FXK.PERFECT_DODGE, victim.inst, attacker.inst, posOf(victim), victim.dash.dir, 0)
		end
		return false
	end

	tagCombat(attacker, now)
	tagCombat(victim, now)
	attacker.lastTarget = victim
	victim.lastTarget = attacker

	local frontal = isFrontal(attacker, victim)
	local guarding = victim.blocking or (victim.blockStart ~= nil and now <= victim.parryTapUntil)
	local autoParry = victim.kind == "golem" and victim.gtype.autoParry and not isStunned(victim, now)

	if frontal and (guarding or autoParry) then
		local isAttempt = wasGoingForParry(victim, now)
		local isParry = autoParry or isParryTimed(victim, attacker, attackName)

		if isParry then
			faceTowards(victim, attacker)
			addPosture(victim, CONFIG.ParryPostureGain, now)
			victim.parryTapUntil = 0
			victim.parryStreak = math.min(255, victim.parryStreak + 1)
			if victim.kind == "golem" then
				setPose(victim, POSE.PARRY)
				victim.poseHoldUntil = now + 0.35
			end

			stun(attacker, CONFIG.StunDuration, now, "Parry")
			attacker.attackLockUntil = now + CONFIG.ParryRecoveryLock
			attacker.swingTime = nil
			attacker.parryStreak = 0

			local mid = (posOf(attacker) + posOf(victim)) / 2
			fx(FXK.PARRY, attacker.inst, victim.inst, mid, unit(flat(posOf(attacker) - posOf(victim)), FORWARD), victim.parryStreak)

			if victim.kind == "golem" and victim.gtype.counter then
				aiScheduleCounter(victim, attacker, now + CONFIG.ParryTradeCounterDelay)
			end
			return true
		end

		if isAttempt then
			-- Went for the parry and missed: the guard drops and the hit carries on below.
			victim.parryTapUntil = 0
			setBlocking(victim, false, now)
			victim.attackLockUntil = now + CONFIG.MistimedParryLock
			fx(FXK.MISSED_PARRY, victim.inst, attacker.inst, posOf(victim), ZERO, 0)
		elseif victim.blocking then
			local gain = CONFIG.PostureGainPerBlock
			if attackName == "Skill" then
				gain = CONFIG.SkillPostureGain
			end
			local newPosture
			if attackName == "Critical" then
				newPosture = CONFIG.MaxPosture -- criticals break a held guard outright
			else
				newPosture = addPosture(victim, gain, now)
			end
			drainStamina(victim, 1, now)

			if newPosture >= CONFIG.MaxPosture then
				breakGuard(attacker, victim, now)
			else
				faceTowards(victim, attacker)
				fx(FXK.BLOCKED, attacker.inst, victim.inst, posOf(victim), unit(flat(posOf(attacker) - posOf(victim)), FORWARD), newPosture)
			end
			return true
		end
	end

	applyDirectHit(attacker, victim, attackName, amount, now)
	return true
end

-- Find the best enemy in front of a swing and turn toward it (the original's swing re-aim).
local function reaim(f)
	local origin = posOf(f)
	local fwd = forwardOf(f)
	local best, bestScore = nil, math.huge
	local cosLimit = math.cos(math.rad(CONFIG.SwingReaimMaxAngle))
	for _, other in ipairs(fighters) do
		if other ~= f and not isOut(other) then
			local d = flat(posOf(other) - origin)
			local distance = len(d)
			if distance > 0.1 and distance <= CONFIG.SwingReaimRadius then
				local cosAngle = dot(d / distance, fwd)
				if cosAngle >= cosLimit then
					local score = distance * (2 - cosAngle)
					if score < bestScore then
						best, bestScore = other, score
					end
				end
			end
		end
	end
	if best ~= nil then
		faceTowards(f, best)
		local distance = len(flat(posOf(best) - origin))
		return best, distance
	end
	return nil, nil
end

local aiReact -- forward declaration

local function canAct(f, now)
	return not f.dead
		and not isStunned(f, now)
		and now >= f.attackLockUntil
		and f.attack == nil
		and f.dash == nil
		and f.wall == nil
end

local function startAttack(f, name, now)
	local isPlayer = f.kind == "player"
	local swing, hit, duration
	if name == "Critical" then
		swing, hit, duration = CONFIG.CriticalSwingDelay, CONFIG.CriticalHitDelay, CONFIG.CriticalDuration
	else
		swing = isPlayer and CONFIG.PlayerSwingDelay or CONFIG.DummySwingDelay
		hit = isPlayer and CONFIG.PlayerHitDelay or CONFIG.DummyHitDelay
		duration = math.max(CONFIG.AttackDuration, hit + 0.06)
	end
	local size, forwardOffset, lifetime = hitboxFor(f, name)

	if f.blocking then
		setBlocking(f, false, now)
	end
	cancelSlide(f)

	local step = 0
	if name ~= "Critical" then
		step = tonumber(string.sub(name, 2)) or 1
	end

	f.attack = {
		name = name,
		step = step,
		start = now,
		swingAt = now + swing,
		hitAt = now + hit,
		hitEnd = now + hit + lifetime,
		endAt = now + duration,
		size = size,
		forward = forwardOffset,
		hits = {},
		swung = false,
	}
	f.faceUntil = now + duration + 0.55

	if f.kind == "golem" then
		setPose(f, name == "Critical" and POSE.CRIT_WINDUP or POSE.WINDUP)
	end
	-- num: 2 = critical telegraph, 10 + step = a combo swing (clients pick the golem's hand)
	fx(FXK.WINDUP, f.inst, f.inst, posOf(f), forwardOf(f), name == "Critical" and 2 or (10 + step))

	-- Let nearby AI duelists decide how to answer this attack.
	for _, other in ipairs(fighters) do
		if other ~= f and other.ai ~= nil and not isOut(other) then
			if len(flat(posOf(other) - posOf(f))) <= 14 then
				aiReact(other, f, f.attack, now)
			end
		end
	end
end

local function startM1(f, now)
	if not canAct(f, now) or now < f.m1ReadyAt then
		return false
	end
	local continuing = (now - f.windowStart) < CONFIG.ComboResetTime
	local step = continuing and (f.combo % CONFIG.ComboLength) + 1 or 1
	f.combo = step
	startAttack(f, "M" .. step, now)
	return true
end

local function startCritical(f, now)
	if not canAct(f, now) or now < f.critReadyAt or f.stamina < CONFIG.CriticalStaminaCost then
		return false
	end
	drainStamina(f, CONFIG.CriticalStaminaCost, now)
	f.critReadyAt = now + CONFIG.CriticalCooldown
	startAttack(f, "Critical", now)
	return true
end

local function tryFeint(f, now)
	local atk = f.attack
	if atk == nil or atk.name ~= "Critical" or atk.swung or now < f.feintReadyAt then
		return false
	end
	if now - atk.start < CONFIG.FeintMinimumDelay then
		return false
	end
	f.feintReadyAt = now + CONFIG.FeintCooldown
	f.critReadyAt = now + 0.6 -- a feinted critical comes back quickly
	clearAttack(f)
	fx(FXK.FEINT, f.inst, f.inst, posOf(f), forwardOf(f), 0)
	return true
end

local function startSkill(f, now)
	if not canAct(f, now) or now < f.skillReadyAt or f.stamina < CONFIG.SkillStaminaCost then
		return false
	end
	local w = weaponOf(f)
	drainStamina(f, CONFIG.SkillStaminaCost, now)
	f.skillReadyAt = now + CONFIG.SkillCooldown
	if f.blocking then
		setBlocking(f, false, now)
	end
	cancelSlide(f)
	local hits = {}
	for i = 1, w.SkillHits do
		table.insert(hits, now + w.SkillFirstHit + (i - 1) * w.SkillInterval)
	end
	f.attack = {
		name = "Skill",
		step = 0,
		start = now,
		swingAt = hits[1] - 0.1,
		hitAt = hits[1],
		hitEnd = hits[#hits] + 0.1,
		endAt = hits[#hits] + 0.25,
		size = CONFIG.SkillHitboxSize,
		forward = CONFIG.SkillHitboxForward,
		schedule = hits,
		nextHit = 1,
		allAround = w.SkillAllAround == true,
		swung = false,
		hits = {},
	}
	f.faceUntil = f.attack.endAt + 0.4
	if f.kind == "golem" then
		setPose(f, POSE.SKILL)
	end
	fx(FXK.SKILL, f.inst, f.inst, posOf(f), forwardOf(f), f.weapon)
	for _, other in ipairs(fighters) do
		if other ~= f and other.ai ~= nil and not isOut(other) and len(flat(posOf(other) - posOf(f))) <= 14 then
			aiReact(other, f, f.attack, now)
		end
	end
	return true
end

local function finishAttack(f, now)
	local atk = f.attack
	if atk == nil then
		return
	end
	if atk.name ~= "Critical" and atk.name ~= "Skill" then
		f.windowStart = now
		f.m1ReadyAt = now + (atk.step >= CONFIG.ComboLength and CONFIG.LastM1Cooldown or CONFIG.M1Cooldown)
	end
	clearAttack(f)
end

local function stepAttack(f, now)
	local atk = f.attack
	if atk == nil then
		return
	end

	if atk.name == "Skill" then
		while atk.nextHit <= #atk.schedule and now >= atk.schedule[atk.nextHit] do
			local w = weaponOf(f)
			f.swingTime = atk.schedule[atk.nextHit] - 0.1
			atk.swung = true
			fx(FXK.SWING, f.inst, f.inst, posOf(f), forwardOf(f), 6 + (atk.nextHit % 2))
			if not atk.allAround then
				reaim(f)
			end
			showHitbox(f, atk.size, atk.allAround and 0 or atk.forward)
			for _, victim in ipairs(fighters) do
				if victim ~= f and not isOut(victim) and inHitbox(f, victim, atk.size, atk.forward, atk.allAround) then
					resolveHit(f, victim, "Skill", w.SkillDamage or DAMAGE.Skill, now)
					if f.attack == nil then
						return -- parried
					end
				end
			end
			atk.nextHit += 1
		end
		if now >= atk.endAt then
			finishAttack(f, now)
		end
		return
	end

	if not atk.swung and now >= atk.swingAt then
		atk.swung = true
		f.swingTime = now
		local target, distance = reaim(f)
		-- Small lunge to close the gap, like the original's targeted lunge.
		if target ~= nil and distance ~= nil and distance > 3.5 then
			local lungeDistance = math.min(distance - 3, atk.name == "Critical" and CONFIG.CriticalLunge or CONFIG.LungeMaxDistance)
			if lungeDistance > 0.2 then
				knockback(f, posOf(target) - posOf(f), lungeDistance, 0.12)
			end
		elseif atk.name == "Critical" then
			knockback(f, forwardOf(f), CONFIG.CriticalLunge * 0.6, 0.15)
		end
		if f.kind == "golem" then
			setPose(f, POSE.SWING)
		end
		fx(FXK.SWING, f.inst, f.inst, posOf(f), forwardOf(f), atk.name == "Critical" and 5 or atk.step)
	end

	if now >= atk.hitAt and now <= atk.hitEnd then
		if not atk.shown then
			atk.shown = true
			showHitbox(f, atk.size, atk.forward)
		end
		local amount = DAMAGE[atk.name] or DAMAGE.M1
		for _, victim in ipairs(fighters) do
			if victim ~= f and not isOut(victim) and not atk.hits[victim] then
				if inHitbox(f, victim, atk.size, atk.forward, false) then
					atk.hits[victim] = true
					resolveHit(f, victim, atk.name, amount, now)
					if f.attack == nil then
						return -- the swing was parried and cancelled
					end
				end
			end
		end
	end

	if now >= atk.endAt then
		finishAttack(f, now)
	end
end

------------------------------------------------------------------------------------------
-- MOVEMENT
------------------------------------------------------------------------------------------

local function isGrounded(f)
	if f.kind == "golem" then
		return f.grounded
	end
	local origin = posOf(f)
	local hit = raycast(origin, V3(0, -1, 0), CONFIG.GroundProbe, excludeListFor(f))
	return hit ~= nil
end

local function startDash(f, direction, now)
	if f.dead or isStunned(f, now) or now < f.dashReadyAt or f.attack ~= nil then
		return false
	end
	local dir = unit(flat(direction), forwardOf(f))
	if f.blocking then
		setBlocking(f, false, now)
	end
	cancelSlide(f)
	f.wall = nil
	f.dash = { dir = dir, start = now, endAt = now + CONFIG.DashDuration }
	f.perfectUsed = false
	f.iframeUntil = now + CONFIG.DashIFrameDuration
	f.dashReadyAt = now + CONFIG.DashDuration + CONFIG.DashCooldown
	drive(f, "burst", dir, CONFIG.DashSpeed, CONFIG.DashDuration)
	if f.kind == "golem" then
		setPose(f, POSE.DASH)
	end
	fx(FXK.DASH, f.inst, f.inst, posOf(f), dir, 0)
	return true
end

local function startSlide(f, direction, now)
	if f.kind ~= "player" or f.dead or isStunned(f, now) or f.attack ~= nil or f.blocking then
		return
	end
	if f.slide ~= nil or f.dash ~= nil or now < f.slideReadyAt or not isGrounded(f) then
		return
	end
	local v = velocityOf(f)
	local speed = len(flat(v))
	if speed < CONFIG.SlideMinimumEntrySpeed then
		return
	end
	local dir = unit(flat(direction), unit(flat(v), forwardOf(f)))
	local startSpeed = math.min(speed + CONFIG.SlideBoost, CONFIG.SlideMaxSpeed)
	f.slide = { dir = dir, speed = startSpeed, start = now }
	if f.hipHeight == nil then
		f.hipHeight = get(f.inst, "HipHeight")
	end
	if f.hipHeight ~= nil then
		set(f.inst, "HipHeight", f.hipHeight * CONFIG.SlideHipHeightScale)
	end
	fx(FXK.SLIDE, f.inst, f.inst, posOf(f), dir, 1)
end

local function endSlide(f, now)
	if f.slide == nil then
		return
	end
	f.slide = nil
	f.slideReadyAt = now + CONFIG.SlideCooldown
	if f.hipHeight ~= nil then
		set(f.inst, "HipHeight", f.hipHeight)
	end
	fx(FXK.SLIDE, f.inst, f.inst, posOf(f), ZERO, 0)
end

local function slideLeap(f, now)
	local s = f.slide
	if s == nil then
		return false
	end
	endSlide(f, now)
	local forward = s.speed * CONFIG.LeapCarry + CONFIG.LeapForward
	local walk = weaponOf(f).WalkSpeed
	drive(f, "leap", s.dir, forward, CONFIG.LeapDriveDuration, { endSpeed = math.min(walk, forward), launch = CONFIG.LeapUp })
	fx(FXK.LEAP, f.inst, f.inst, posOf(f), s.dir, 0)
	return true
end

local function stepSlide(f, now, dt)
	local s = f.slide
	if s == nil then
		return
	end
	s.speed -= CONFIG.SlideFriction * dt
	local holding = bit32.band(f.flags, FLAG.SLIDE) ~= 0
	if s.speed < CONFIG.SlideMinimumSpeed or now - s.start > CONFIG.SlideMaxDuration or not holding then
		endSlide(f, now)
		return
	end
	local wish = unit(flat(f.move), nil)
	if wish ~= nil and len(wish) > 0.5 then
		s.dir = unit(lerp(s.dir, wish, 1 - math.exp(-CONFIG.SlideSteerSharpness * dt)), s.dir)
	end
	local v = velocityOf(f)
	setVelocity(f, V3(s.dir.X * s.speed, v.Y, s.dir.Z * s.speed))
end

local function tryWallRun(f, now)
	if not CONFIG.WallRunEnabled or f.kind ~= "player" or f.wall ~= nil or now < f.wallReadyAt then
		return
	end
	if f.dash ~= nil or f.slide ~= nil or f.attack ~= nil or f.blocking or isStunned(f, now) then
		return
	end
	local move = flat(f.move)
	if len(move) < CONFIG.WallRunMinimumMove then
		return
	end
	move = unit(move, FORWARD)
	local origin = posOf(f)
	-- Feet must be well clear of the floor, so a hop next to a wall stays a hop.
	if raycast(origin, V3(0, -1, 0), CONFIG.GroundProbe + CONFIG.WallRunGroundClearance, excludeListFor(f)) ~= nil then
		return
	end
	local right = rightOf(move)
	for _, side in ipairs({ right, -right, move }) do
		local hit = raycast(origin, side, CONFIG.WallDetectionDistance, excludeListFor(f))
		if hit ~= nil and math.abs(hit.Normal.Y) <= CONFIG.WallRunMaxSurfaceTilt then
			local other = hit.Entity and call(hit.Entity, "FindAncestorWithComponent", "Character")
			if other == nil and fighterOf(hit.Entity) == nil then
				local normal = unit(flat(hit.Normal), -side)
				local along = flat(move - normal * dot(move, normal))
				-- Running straight into the wall is not a wall run.
				if len(along) > 0.2 and dot(move, normal) >= CONFIG.WallRunEntryNormalDotMinimum then
					f.wall = { normal = normal, along = unit(along, move), start = now, lostAt = nil }
					f.airJumpsUsed = 0
					fx(FXK.WALL_RUN, f.inst, f.inst, hit.Position, normal, 1)
					return
				end
			end
		end
	end
end

local function endWallRun(f, now, jumped)
	local w = f.wall
	if w == nil then
		return
	end
	f.wall = nil
	f.wallReadyAt = now + (jumped and CONFIG.WallJumpBlockDuration or 0.25)
	fx(FXK.WALL_RUN, f.inst, f.inst, posOf(f), w.normal, 0)
end

local function stepWallRun(f, now)
	local w = f.wall
	if w == nil then
		return
	end
	if f.grounded or now - w.start > CONFIG.WallRunMaxDuration or isStunned(f, now) then
		endWallRun(f, now, false)
		return
	end
	local hit = raycast(posOf(f), -w.normal, CONFIG.WallDetectionDistance + 0.5, excludeListFor(f))
	if hit == nil then
		w.lostAt = w.lostAt or now
		if now - w.lostAt > CONFIG.WallRunEndGrace then
			endWallRun(f, now, false)
			return
		end
	else
		w.lostAt = nil
		w.normal = unit(flat(hit.Normal), w.normal)
	end
	-- Run along the wall with gravity cancelled, hugging it slightly.
	setVelocity(f, w.along * CONFIG.WallRunSpeed - w.normal * 2)
end

local function wallJump(f, now)
	local w = f.wall
	if w == nil then
		return false
	end
	endWallRun(f, now, true)
	local push = w.normal * CONFIG.WallJumpOutward + w.along * CONFIG.WallJumpAlong
	drive(f, "hold", push, len(push), CONFIG.WallJumpDrive, { launch = CONFIG.WallJumpUp })
	fx(FXK.WALL_JUMP, f.inst, f.inst, posOf(f), unit(push, FORWARD), 0)
	return true
end

local function onJump(f, now)
	if f.dead or isStunned(f, now) then
		return
	end
	if f.slide ~= nil then
		slideLeap(f, now)
		return
	end
	if f.wall ~= nil then
		wallJump(f, now)
		return
	end
	-- Double jump: the engine handles the ground jump, we add the air ones. The grace stops
	-- a ground jump that reached us a frame late from turning into a second jump.
	local airborneFor = now - (f.groundedAt or 0)
	if not f.grounded and airborneFor > 0.18 and f.airJumpsUsed < CONFIG.AirJumps and f.dash == nil then
		f.airJumpsUsed += 1
		local v = velocityOf(f)
		setVelocity(f, V3(v.X, CONFIG.DoubleJumpVelocity, v.Z))
		fx(FXK.AIR_JUMP, f.inst, f.inst, posOf(f), flat(v), 0)
	end
end

local function stepDrive(f, now)
	local d = f.drive
	if d == nil then
		return
	end
	local a = (now - d.start) / d.duration
	if a >= 1 then
		f.drive = nil
		return
	end
	local pace
	if d.kind == "burst" then
		pace = d.speed * (1 - a) * (1 - a)
	elseif d.kind == "knock" then
		pace = d.speed * (1 - a)
	elseif d.kind == "leap" then
		pace = d.speed + (d.endSpeed - d.speed) * (a * a)
	else
		pace = d.speed
	end
	local v = velocityOf(f)
	local vy = v.Y
	if d.launch ~= nil then
		vy = d.launch
		d.launch = nil
	end
	setVelocity(f, V3(d.dir.X * pace, vy, d.dir.Z * pace))
end

------------------------------------------------------------------------------------------
-- GOLEMS (training dummies + AI duelist)
------------------------------------------------------------------------------------------

local GOLEM_TYPES = {
	basic = { label = "Training Dummy", accent = Color3.fromRGB(235, 235, 245) },
	block = { label = "Blocking Dummy", accent = Color3.fromRGB(84, 170, 255), alwaysBlock = true },
	parry = { label = "Parry Dummy", accent = Color3.fromRGB(255, 176, 106), autoParry = true, counter = true },
	attack = { label = "Attacking Dummy", accent = Color3.fromRGB(255, 73, 73), attacker = true },
	ai = { label = "Duelist", accent = Color3.fromRGB(170, 0, 255), ai = true },
}

local TORSO_HEIGHT = 3 -- torso centre above the ground
local GRAVITY = 196.2
pcall(function()
	local g = game.Physics.Gravity
	if type(g) == "number" and g > 0 then
		GRAVITY = g
	end
end)

local difficulty = CONFIG.DefaultDifficulty

local function aiSettings(name)
	local s = table.clone(AI_BASE)
	for k, v in pairs(AI_OVERRIDES[name] or {}) do
		s[k] = v
	end
	return s
end

local function spawnGolem(typeKey, index, position, facing)
	local gtype = GOLEM_TYPES[typeKey]
	local root = makePart({
		name = "PA_Golem_" .. typeKey .. "_" .. index,
		size = V3(2, 2, 1),
		pos = position + V3(0, TORSO_HEIGHT, 0),
		color = PALETTE.Stone,
		anchored = true,
	})
	if root == nil then
		warn("[ParryArena] could not spawn golem " .. typeKey)
		return nil
	end
	table.insert(golemRoots, root)

	local f = newFighter(root, "golem")
	f.gtype = gtype
	f.typeKey = typeKey
	f.home = position + V3(0, TORSO_HEIGHT, 0)
	f.homeFacing = unit(flat(facing), FORWARD)
	f.pos = f.home
	f.vel = ZERO
	f.facing = f.homeFacing
	f.walkVel = ZERO
	f.grounded = true
	f.pose = POSE.IDLE
	f.weapon = 1
	f.lastWritePos = nil
	f.lastWriteFacing = nil
	f.labelText = nil
	f.attackTimer = 0
	if gtype.ai then
		f.ai = { settings = aiSettings(difficulty), nextThink = 0, attackReadyAt = 0, parryReadyAt = 0, plan = nil, comboLeft = 0 }
		f.maxHp = CONFIG.MaxHealth * f.ai.settings.HealthMultiplier
		f.hp = f.maxHp
	end
	call(root, "AddComponent", "Billboard")
	set(root, "BillboardOffset", V3(0, 3.6, 0))
	set(root, "BillboardColor", gtype.accent)
	set(root, "BillboardWorldSize", 1.1)
	set(root, "BillboardMaxDistance", 120)
	return f
end

local function golemLabel(f)
	local bars = math.floor((f.posture / CONFIG.MaxPosture) * 10 + 0.5)
	local postureText = string.rep("|", bars) .. string.rep(".", 10 - bars)
	local name = f.gtype.label
	if f.ai ~= nil then
		name = name .. " (" .. difficulty .. ")"
	end
	return string.format("%s   HP %d/%d   [%s]", name, math.ceil(f.hp), math.ceil(f.maxHp), postureText)
end

function onGolemDied(f, attacker, now)
	if f.pose == POSE.DEAD then
		return
	end
	f.blocking = false
	f.attack = nil
	f.dash = nil
	f.drive = nil
	setPose(f, POSE.DEAD)
	fx(FXK.DEATH, f.inst, attacker and attacker.inst or f.inst, f.pos, f.facing, 0)
	f.respawnAt = now + CONFIG.GolemRespawnDelay
	f.hp = 0
	set(f.inst, "Transparency", 1)
	-- Park the body under the floor while it waits, so nobody bumps into an invisible golem.
	set(f.inst, "Position", f.home - V3(0, 12, 0))
	f.lastWritePos = nil
end

local function respawnGolem(f, now)
	f.respawnAt = nil
	f.hp = f.maxHp
	f.posture = 0
	f.stamina = CONFIG.MaxStamina
	f.pos = f.home
	f.vel = ZERO
	f.facing = f.homeFacing
	f.stunUntil = 0
	f.attackLockUntil = 0
	f.combo = 0
	f.counter = nil
	set(f.inst, "Transparency", 0)
	set(f.inst, "Position", f.pos)
	f.lastWritePos = f.pos
	setPose(f, POSE.IDLE)
	fx(FXK.RESPAWN, f.inst, f.inst, f.pos, f.facing, 0)
end

local function nearestPlayer(f, maxDistance)
	local origin = posOf(f)
	local best, bestDistance = nil, maxDistance
	for _, other in ipairs(fighters) do
		if other.kind == "player" and not other.dead then
			local d = len(flat(posOf(other) - origin))
			if d < bestDistance then
				best, bestDistance = other, d
			end
		end
	end
	return best, bestDistance
end

function aiScheduleCounter(f, target, at)
	f.counter = { target = target, at = at }
end

function aiReact(g, attacker, atk, now)
	if g.dead or g.pose == POSE.DEAD or isStunned(g, now) then
		return
	end
	if g.attack ~= nil and g.attack.swung then
		return -- committed to its own swing
	end
	local s = g.ai.settings
	local parryChance = atk.name == "Skill" and s.SkillParryChance or s.AttackParryChance
	local roll = math.random()
	if roll < parryChance and now >= g.ai.parryReadyAt then
		-- Press inside the window, before the hit lands.
		local tolerance, window = parryTiming(atk.name)
		local latest = math.min(window, atk.hitAt - atk.swingAt - 0.02)
		local pressAt = atk.swingAt + rand(-tolerance * 0.5, math.max(latest, 0))
		g.ai.plan = { kind = "block", at = math.max(pressAt, now), releaseAt = pressAt + 0.3 }
		g.ai.parryReadyAt = now + s.ParryAttemptCooldown
	elseif roll < parryChance + s.BlockChance then
		local pressAt = atk.start + rand(s.ReactionTimeMin, s.ReactionTimeMax)
		g.ai.plan = { kind = "block", at = pressAt, releaseAt = pressAt + rand(s.BlockHoldMin, s.BlockHoldMax), held = true }
	elseif roll < parryChance + s.BlockChance + s.DodgeChance and now >= g.dashReadyAt then
		g.ai.plan = { kind = "dash", at = atk.start + rand(s.ReactionTimeMin, s.ReactionTimeMax) * 0.6, from = attacker }
	end
end

local function aiExecutePlan(g, now)
	local plan = g.ai and g.ai.plan or nil
	if plan == nil or now < plan.at then
		return
	end
	if isStunned(g, now) or g.pose == POSE.DEAD then
		g.ai.plan = nil
		return
	end
	if plan.kind == "block" then
		if not plan.pressed then
			plan.pressed = true
			if g.attack ~= nil and not g.attack.swung then
				clearAttack(g) -- abandon its own wind-up to defend
			end
			if not g.blocking then
				setBlocking(g, true, now)
			else
				g.blockStart = now -- re-press
				g.parryTapUntil = now + CONFIG.ParryTapGrace
			end
		elseif now >= plan.releaseAt then
			setBlocking(g, false, now)
			g.ai.plan = nil
		end
	elseif plan.kind == "dash" then
		g.ai.plan = nil
		local away = unit(flat(posOf(g) - posOf(plan.from)), -g.facing)
		local right = rightOf(g.facing)
		local s = g.ai.settings
		local roll = math.random()
		local dir = away
		if roll > s.BackDashWeight then
			dir = (roll < s.BackDashWeight + s.LeftDashWeight) and -right or right
		end
		startDash(g, dir, now)
	end
end

local function golemThink(g, now, dt)
	g.walkVel = ZERO
	if g.pose == POSE.DEAD then
		return
	end

	-- Counter attack after a parry (the "parry trade" dummy).
	if g.counter ~= nil and now >= g.counter.at then
		local target = g.counter.target
		g.counter = nil
		if target ~= nil and not target.dead and canAct(g, now) then
			faceTowards(g, target)
			g.combo = 0
			startM1(g, now)
		end
	end

	if g.gtype.alwaysBlock then
		if not isStunned(g, now) and not g.blocking then
			setBlocking(g, true, now, now - 10)
		end
		local target = nearestPlayer(g, 14)
		if target ~= nil and not isStunned(g, now) then
			faceTowards(g, target)
		end
		return
	end

	if g.gtype.attacker then
		local target, distance = nearestPlayer(g, 10)
		if target ~= nil and not isStunned(g, now) then
			if g.attack == nil then
				faceTowards(g, target)
			end
			g.attackTimer -= dt
			if g.attackTimer <= 0 and distance ~= nil and distance <= 8 and canAct(g, now) then
				g.attackTimer = 1.6
				g.windowStart = now -- keep cycling M1 -> M4 so every swing looks different
				g.m1ReadyAt = 0
				startM1(g, now)
			end
		else
			g.attackTimer = 0.8
		end
		return
	end

	if g.gtype.ai then
		aiExecutePlan(g, now)
		local s = g.ai.settings
		local target, distance = nearestPlayer(g, 60)
		if target ~= nil and len(flat(posOf(target) - g.home)) > 40 then
			target = nil
		end
		if target == nil then
			local toHome = flat(g.home - g.pos)
			if len(toHome) > 2 and g.attack == nil and not isStunned(g, now) then
				g.walkVel = unit(toHome, ZERO) * s.WalkSpeed * 0.6
				g.facing = turnTowards(g.facing, unit(toHome, g.facing), 10, dt)
			end
			if g.blocking and g.ai.plan == nil then
				setBlocking(g, false, now)
			end
			return
		end

		if isStunned(g, now) or g.dash ~= nil then
			return
		end
		if g.attack == nil then
			local desired = unit(flat(posOf(target) - g.pos), g.facing)
			g.facing = turnTowards(g.facing, desired, 14, dt)
		end

		-- Continue a combo string.
		if g.attack == nil and g.ai.comboLeft > 0 and now >= g.m1ReadyAt and canAct(g, now) then
			if distance ~= nil and distance <= s.AttackRange + 1 then
				g.ai.comboLeft -= 1
				startM1(g, now)
				if g.ai.comboLeft == 0 then
					g.ai.attackReadyAt = now + 0.35 + s.AttackCooldown
				end
				return
			end
			g.ai.comboLeft = 0
		end

		-- Feint a critical now and then.
		if g.attack ~= nil and g.attack.name == "Critical" and g.attack.feintAt ~= nil and now >= g.attack.feintAt then
			if tryFeint(g, now) then
				g.feintReadyAt = now + s.FeintCooldown
			end
			g.ai.attackReadyAt = now + 0.15
			return
		end

		if g.attack ~= nil or g.blocking then
			return
		end

		-- Footsies: hold the preferred distance.
		if distance ~= nil then
			local toTarget = unit(flat(posOf(target) - g.pos), g.facing)
			if distance > s.PreferredDistance + 0.6 then
				g.walkVel = toTarget * s.WalkSpeed
			elseif distance < s.MinDistance then
				g.walkVel = -toTarget * s.WalkSpeed * 0.5
			end
		end

		if now < g.ai.nextThink then
			return
		end
		g.ai.nextThink = now + s.ThinkInterval

		if distance == nil or distance > s.AttackRange or now < g.ai.attackReadyAt or now < g.attackLockUntil then
			return
		end
		if math.random() >= s.AttackChance then
			g.ai.attackReadyAt = now + s.AttackCooldown
			return
		end
		local roll = math.random()
		if roll < s.CriticalChance and now >= g.critReadyAt and g.stamina >= CONFIG.CriticalStaminaCost then
			startCritical(g, now)
			g.critReadyAt = now + s.CriticalCooldown
			if g.attack ~= nil and math.random() < s.FeintChance and now >= g.feintReadyAt then
				g.attack.feintAt = now + rand(0.12, CONFIG.CriticalSwingDelay - 0.08)
			end
			g.ai.attackReadyAt = now + CONFIG.CriticalDuration + s.AttackCooldown
		elseif roll < s.CriticalChance + s.SkillChance and now >= g.skillReadyAt then
			startSkill(g, now)
			g.skillReadyAt = now + s.SkillCooldown
			g.ai.attackReadyAt = now + 1.4 + s.AttackCooldown
		else
			g.combo = 0
			g.windowStart = -100
			g.ai.comboLeft = math.random(1, CONFIG.ComboLength) - 1
			startM1(g, now)
			if g.ai.comboLeft == 0 then
				g.ai.attackReadyAt = now + 0.35 + s.AttackCooldown
			end
		end
		return
	end

	-- Idle dummies turn (smoothly, so you can still get behind them) to whoever comes close.
	local target = nearestPlayer(g, 12)
	if target ~= nil and g.attack == nil and not isStunned(g, now) then
		local desired = unit(flat(posOf(target) - g.pos), g.facing)
		g.facing = turnTowards(g.facing, desired, 5, dt)
	end
end

local function golemPhysics(g, now, dt)
	if g.pose == POSE.DEAD then
		return
	end
	local v = g.vel
	if g.drive == nil then
		-- Walk toward the wanted velocity on the ground, keep momentum in the air.
		local wish = isStunned(g, now) and ZERO or g.walkVel
		local horizontal = flat(v)
		local sharp = g.grounded and 12 or 2
		horizontal = lerp(horizontal, wish, 1 - math.exp(-sharp * dt))
		v = V3(horizontal.X, v.Y, horizontal.Z)
	end
	v = V3(v.X, v.Y - GRAVITY * dt, v.Z)

	local nextPos = g.pos + v * dt

	-- Walls: stop horizontal motion that would run into something.
	local horizontalStep = flat(v * dt)
	local stepLength = len(horizontalStep)
	if stepLength > 0.001 then
		local hit = raycast(g.pos, horizontalStep / stepLength, stepLength + 1.1, golemRoots)
		if hit ~= nil and fighterOf(hit.Entity) == nil then
			nextPos = V3(g.pos.X, nextPos.Y, g.pos.Z)
			v = V3(0, v.Y, 0)
		end
	end

	-- Ground.
	local groundHit = raycast(V3(nextPos.X, g.pos.Y + 1, nextPos.Z), V3(0, -1, 0), 60, golemRoots)
	g.grounded = false
	if groundHit ~= nil then
		local standY = groundHit.Position.Y + TORSO_HEIGHT
		if nextPos.Y <= standY + 0.05 and v.Y <= 0 then
			nextPos = V3(nextPos.X, standY, nextPos.Z)
			v = V3(v.X, 0, v.Z)
			g.grounded = true
		end
	end
	if nextPos.Y < -50 then
		nextPos = g.home
		v = ZERO
	end

	-- Keep everyone inside the arena.
	local limit = ARENA_HALF - 2
	nextPos = V3(clamp(nextPos.X, -limit, limit), nextPos.Y, clamp(nextPos.Z, -limit, limit))

	g.pos = nextPos
	g.vel = v
end

local function golemWrite(g)
	if g.lastWritePos == nil or len(g.pos - g.lastWritePos) > 0.01 then
		set(g.inst, "Position", g.pos)
		g.lastWritePos = g.pos
	end
	if g.lastWriteFacing == nil or dot(g.facing, g.lastWriteFacing) < 0.9995 then
		set(g.inst, "Orientation", orientationFor(g.facing))
		g.lastWriteFacing = g.facing
	end
	local text = golemLabel(g)
	if text ~= g.labelText then
		g.labelText = text
		set(g.inst, "BillboardText", text)
	end
end

local function spawnGolems()
	local row = 30
	spawnGolem("basic", 1, V3(-36, 0, row), V3(0, 0, 1))
	spawnGolem("block", 1, V3(-12, 0, row), V3(0, 0, 1))
	spawnGolem("parry", 1, V3(12, 0, row), V3(0, 0, 1))
	spawnGolem("attack", 1, V3(36, 0, row), V3(0, 0, 1))
	spawnGolem("ai", 1, V3(0, 0, -24), V3(0, 0, 1))

	for i, info in ipairs({
		{ -36, "TRAINING DUMMY  -  practise the 4-hit combo", PALETTE.Light },
		{ -12, "BLOCKING DUMMY  -  build posture, Critical (R) breaks guard", Color3.fromRGB(84, 170, 255) },
		{ 12, "PARRY DUMMY  -  parries everything and counters: parry back!", PALETTE.Orange },
		{ 36, "ATTACKING DUMMY  -  steady swings: tap F as it strikes", PALETTE.Red },
	}) do
		local pad = makePart({ name = "DummyPad", size = V3(10, 0.3, 10), pos = V3(info[1], 0.15, row), color = PALETTE.Dark })
		makePart({
			name = "DummyPadRim",
			size = V3(10.4, 0.1, 10.4),
			pos = V3(info[1], 0.05, row),
			color = info[3],
			material = NEON,
			collide = false,
			noShadow = true,
		})
		local sign = makePart({ name = "DummySign", size = V3(8, 0.2, 1), pos = V3(info[1], 0.1, row + 7), color = PALETTE.Dark, collide = false })
		label(sign, info[2], info[3], V3(0, 1.5, 0), 1)
		if pad == nil and i == 1 then
			warn("[ParryArena] could not build dummy pads")
		end
	end
end

spawnGolems()

local function setDifficulty(name)
	if AI_OVERRIDES[name] == nil or name == difficulty then
		return
	end
	difficulty = name
	for _, f in ipairs(fighters) do
		if f.ai ~= nil then
			f.ai.settings = aiSettings(name)
			local ratio = f.maxHp > 0 and f.hp / f.maxHp or 1
			f.maxHp = CONFIG.MaxHealth * f.ai.settings.HealthMultiplier
			f.hp = math.max(1, f.maxHp * ratio)
		end
	end
	local index = table.find(AI_DIFFICULTY_ORDER, name) or 3
	fx(FXK.ANNOUNCE, ROOT, ROOT, V3(0, 0, -24), ZERO, index)
end

------------------------------------------------------------------------------------------
-- PLAYERS
------------------------------------------------------------------------------------------

local function playerFighter(userId)
	local f = byUser[userId]
	if f ~= nil and not f.dead and alive(f.inst) then
		return f
	end
	return nil
end

local function applyWeapon(f, index)
	if WEAPONS[index] == nil then
		return
	end
	f.weapon = index
	if f.userId ~= nil then
		weaponByUser[f.userId] = index
	end
	pcall(function()
		f.inst.attr.PA_Weapon = index
	end)
	fx(FXK.WEAPON, f.inst, f.inst, posOf(f), ZERO, index)
end

local function registerCharacter(player, character)
	if character == nil or fighterOf(character) ~= nil then
		return
	end
	local userId = get(player, "UserId") or get(character, "UserId") or 0
	local old = byUser[userId]
	if old ~= nil then
		removeFighter(old)
	end
	local f = newFighter(character, "player")
	f.userId = userId
	f.player = player
	f.weapon = weaponByUser[userId] or 1
	byUser[userId] = f
	set(character, "MaxHealth", CONFIG.MaxHealth)
	set(character, "Health", CONFIG.MaxHealth)
	f.jumpPower = get(character, "JumpPower")
	f.hipHeight = get(character, "HipHeight")
	applyWeapon(f, f.weapon)
end

pcall(function()
	game.Players.CharacterAdded:Connect(function(player, character)
		registerCharacter(player, character)
	end)
end)

pcall(function()
	game.Players.CharacterDied:Connect(function(player, character)
		local f = fighterOf(character)
		if f ~= nil then
			fx(FXK.DEATH, f.inst, f.inst, posOf(f), forwardOf(f), 1)
			removeFighter(f)
		end
	end)
end)

pcall(function()
	game.Players.PlayerRemoving:Connect(function(player)
		local userId = get(player, "UserId")
		local f = userId and byUser[userId] or nil
		if f ~= nil then
			removeFighter(f)
		end
	end)
end)

-- Players who were already in when this script started.
local function adoptExistingPlayers()
	local list = {}
	local ok = pcall(function()
		for p in game.World.Each("Player") do
			table.insert(list, p)
		end
	end)
	if not ok or #list == 0 then
		list = try(game.Players.All) or {}
	end
	for _, p in ipairs(list) do
		local character = get(p, "Character")
		if character ~= nil then
			registerCharacter(p, character)
		end
	end
end
adoptExistingPlayers()

-- Pads (difficulty + VFX showcase), checked by position.
local function standingOn(zone, p)
	local d = p - zone.centre
	return math.abs(d.X) <= zone.half.X and math.abs(d.Z) <= zone.half.Z and d.Y >= -1 and d.Y <= 6
end

local function stepZones(now)
	for _, f in ipairs(fighters) do
		if f.kind == "player" and not f.dead then
			local p = posOf(f)
			for _, zone in ipairs(difficultyZones) do
				if standingOn(zone, p) then
					setDifficulty(zone.name)
				end
			end
			for _, zone in ipairs(showcaseZones) do
				if standingOn(zone, p) and now >= zone.nextAt then
					zone.nextAt = now + 1.4
					fx(FXK.SHOWCASE, ROOT, ROOT, zone.centre + V3(0, 2.5, 0), FORWARD, zone.index)
				end
			end
		end
	end
end

-- Chat commands for testing.
pcall(function()
	game.Players.PlayerChatted:Connect(function(player, message)
		if type(message) ~= "string" then
			return
		end
		local text = string.lower(message)
		if text == "/hitboxes" then
			CONFIG.ShowHitboxes = not CONFIG.ShowHitboxes
		elseif string.sub(text, 1, 6) == "/diff " then
			local wanted = string.sub(text, 7)
			for _, name in ipairs(AI_DIFFICULTY_ORDER) do
				if string.lower(name) == wanted then
					setDifficulty(name)
				end
			end
		elseif string.sub(text, 1, 8) == "/weapon " then
			local userId = get(player, "UserId")
			local f = userId and playerFighter(userId) or nil
			local wanted = string.sub(text, 9)
			for i, w in ipairs(WEAPONS) do
				if f ~= nil and string.lower(w.Name) == wanted then
					applyWeapon(f, i)
				end
			end
		end
	end)
end)

------------------------------------------------------------------------------------------
-- INPUT FROM CLIENTS
------------------------------------------------------------------------------------------

local function handleAction(f, code, dir, aim, now)
	if aim ~= nil and len(flat(aim)) > 0.1 then
		f.aim = unit(flat(aim), f.aim)
	end
	if code == ACT.ATTACK then
		startM1(f, now)
	elseif code == ACT.CRITICAL then
		startCritical(f, now)
	elseif code == ACT.FEINT then
		tryFeint(f, now)
	elseif code == ACT.BLOCK_ON then
		if not f.dead and not isStunned(f, now) and now >= f.blockReadyAt and f.attack == nil and f.dash == nil then
			endSlide(f, now)
			setBlocking(f, true, now)
		end
	elseif code == ACT.BLOCK_OFF then
		setBlocking(f, false, now)
	elseif code == ACT.DASH then
		startDash(f, len(flat(dir)) > 0.1 and dir or forwardOf(f), now)
	elseif code == ACT.SKILL then
		startSkill(f, now)
	elseif code == ACT.SLIDE_ON then
		f.flags = bit32.bor(f.flags, FLAG.SLIDE)
		startSlide(f, dir, now)
	elseif code == ACT.SLIDE_OFF then
		f.flags = bit32.band(f.flags, bit32.bnot(FLAG.SLIDE))
		endSlide(f, now)
	elseif code == ACT.JUMP then
		onJump(f, now)
	elseif code == ACT.WEAPON then
		local index = math.floor(dir.X + 0.5)
		if f.attack == nil and WEAPONS[index] ~= nil and index ~= f.weapon then
			applyWeapon(f, index)
		end
	elseif code == ACT.RESET then
		set(f.inst, "Health", 0)
	end
end

local function readInputs(now)
	for i = 1, INTENT.count do
		local f = playerFighter(INTENT.sender[i])
		if f ~= nil then
			f.move = INTENT.move[i] or ZERO
			local aim = INTENT.aim[i]
			if aim ~= nil and len(flat(aim)) > 0.1 then
				f.aim = unit(flat(aim), f.aim)
			end
			f.flags = INTENT.flags[i] or 0
		end
	end
	for i = 1, ACTION.count do
		local userId = ACTION.sender[i]
		local f = playerFighter(userId)
		if f == nil then
			-- A character we have not seen yet (e.g. this script started late).
			local player = try(game.Players.Get, userId)
			local character = player and get(player, "Character") or nil
			if character ~= nil then
				registerCharacter(player, character)
				f = playerFighter(userId)
			end
		end
		if f ~= nil then
			handleAction(f, ACTION.code[i], ACTION.dir[i] or ZERO, ACTION.aim[i], now)
		end
	end
end

------------------------------------------------------------------------------------------
-- PER-FRAME
------------------------------------------------------------------------------------------

local function stepStats(f, now, dt)
	-- Stamina
	if now - f.staminaLossAt >= CONFIG.StaminaRegenDelay then
		local rate = now < f.combatUntil and CONFIG.StaminaCombatRegenRate or CONFIG.StaminaRegenRate
		gainStamina(f, rate * dt)
	end
	-- Posture decays after a pause; a guard close to breaking waits longer.
	local delay = f.posture >= CONFIG.PostureHighThreshold and CONFIG.PostureHighDecayDelay or CONFIG.PostureDecayDelay
	if f.posture > 0 and now - f.postureGainAt >= delay then
		f.posture = math.max(0, f.posture - CONFIG.PostureDecayRate * dt)
	end
	-- Health regen out of combat
	if now >= f.combatUntil then
		local hp, maxHp = getHp(f), getMaxHp(f)
		if hp > 0 and hp < maxHp then
			f.regenCarry += CONFIG.HealthRegenRate * dt
			if f.regenCarry >= 1 then
				local amount = math.floor(f.regenCarry)
				f.regenCarry -= amount
				if f.kind == "golem" then
					f.hp = math.min(maxHp, f.hp + amount)
				else
					set(f.inst, "Health", math.min(maxHp, hp + amount))
				end
			end
		end
	end
end

local function stepPlayerMovementStats(f, now)
	local w = weaponOf(f)
	local walk = w.WalkSpeed
	local canJump = true
	if isStunned(f, now) then
		walk = CONFIG.HitstunWalkSpeed
		canJump = false
	elseif f.blocking then
		walk = CONFIG.BlockWalkSpeed
		canJump = false
	elseif f.attack ~= nil and f.attack.name == "Skill" then
		walk = CONFIG.SkillWalkSpeed
	end
	if walk ~= f.lastWalk then
		f.lastWalk = walk
		set(f.inst, "WalkSpeed", walk)
	end
	if f.jumpPower ~= nil and canJump ~= f.lastJump then
		f.lastJump = canJump
		set(f.inst, "JumpPower", canJump and f.jumpPower or 0)
	end

	-- Face the aim while fighting (the original's attack / block facing).
	local wantFacing = f.blocking or f.attack ~= nil or now < f.faceUntil
	if wantFacing ~= f.autoRotateOff then
		f.autoRotateOff = wantFacing
		set(f.inst, "AutoRotate", not wantFacing)
	end
	if wantFacing and f.slide == nil and f.wall == nil then
		local p = posOf(f)
		call(f.inst, "LookAt", p + forwardOf(f) * 10)
	end
end

local function stateBits(f, now)
	local bits = 0
	if f.blocking then bits += ST.BLOCK end
	if isStunned(f, now) then bits += ST.STUN end
	if f.attack ~= nil then bits += ST.ATTACK end
	if f.slide ~= nil then bits += ST.SLIDE end
	if f.wall ~= nil then bits += ST.WALL end
	if now < f.combatUntil then bits += ST.COMBAT end
	if f.dash ~= nil then bits += ST.DASH end
	if isStunned(f, now) and f.stunKind == "Break" then bits += ST.BROKEN end
	return bits
end

local hudSentThisFrame = 0
local function pushHud(f, now)
	if hudSentThisFrame >= 60 or now < f.hudAt then
		return
	end
	f.hudAt = now + 1 / CONFIG.HudRate
	hudSentThisFrame += 1
	local target = f.lastTarget
	if target ~= nil and (target.dead or now >= f.combatUntil) then
		target = nil
	end
	pcall(function()
		HUD:PushTo(
			f.userId,
			getHp(f),
			getMaxHp(f),
			f.stamina,
			f.posture,
			stateBits(f, now),
			f.weapon,
			math.max(0, f.critReadyAt - now),
			math.max(0, f.dashReadyAt - now),
			math.max(0, f.skillReadyAt - now),
			f.attack and f.attack.step or f.combo,
			f.parryStreak,
			target and target.inst or f.inst,
			target and getHp(target) or -1,
			target and getMaxHp(target) or -1,
			target and target.posture or -1
		)
	end)
end

function Update(dt)
	local now = tick()
	fxSentThisFrame = 0
	hudSentThisFrame = 0
	dt = math.min(dt, 0.1)

	readInputs(now)

	for index = #fighters, 1, -1 do
		local f = fighters[index]
		if f ~= nil then
			if f.kind == "player" then
				local hp = get(f.inst, "Health")
				if not alive(f.inst) or (hp ~= nil and hp <= 0) then
					removeFighter(f)
					f = nil
				end
			end
		end
		if f ~= nil then
			if f.kind == "golem" and f.pose == POSE.DEAD then
				if f.respawnAt ~= nil and now >= f.respawnAt then
					respawnGolem(f, now)
				end
			else
				f.grounded = isGrounded(f)
				if f.grounded and f.kind == "player" then
					f.airJumpsUsed = 0
					f.groundedAt = now
				end
				if f.dash ~= nil and now >= f.dash.endAt then
					f.dash = nil
					if f.kind == "golem" and f.attack == nil then
						setPose(f, f.blocking and POSE.BLOCK or POSE.IDLE)
					end
				end
				if f.kind == "golem" and f.poseHoldUntil ~= nil and now >= f.poseHoldUntil then
					f.poseHoldUntil = nil
					if f.attack == nil and not isStunned(f, now) then
						setPose(f, f.blocking and POSE.BLOCK or POSE.IDLE)
					end
				end
				if f.kind == "golem" and f.stunUntil > 0 and now >= f.stunUntil then
					f.stunUntil = 0
					if f.attack == nil then
						setPose(f, f.blocking and POSE.BLOCK or POSE.IDLE)
					end
				end

				stepAttack(f, now)
				stepStats(f, now, dt)

				if f.kind == "golem" then
					golemThink(f, now, dt)
					stepDrive(f, now)
					golemPhysics(f, now, dt)
					golemWrite(f)
				else
					if f.wall == nil and not f.grounded then
						tryWallRun(f, now)
					end
					stepWallRun(f, now)
					stepSlide(f, now, dt)
					stepDrive(f, now)
					stepPlayerMovementStats(f, now)
					pushHud(f, now)
				end
			end
		end
	end

	stepZones(now)
end

print("[ParryArena] server ready  -  " .. #fighters .. " fighters, difficulty " .. difficulty)
