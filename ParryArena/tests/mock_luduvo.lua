-- Mock of the Luduvo scripting API, good enough to run ArenaServer + ArenaClient together.
-- Both scripts share one world (instant "replication"); event tables are bridged per side.
local M = {}

M.clock = 0
M.unknown = {} -- property names scripts touched that the Luduvo defs do not have
M.logs = {}
M.timers = {}
M.instances = {}
M.allowed = {} -- side -> set of Instance member names

---------------------------------------------------------------- vectors
local VMT = {}
local function V(x, y, z)
	return setmetatable({ X = x or 0, Y = y or 0, Z = z or 0 }, VMT)
end
M.V = V
local function isv(v)
	return getmetatable(v) == VMT
end
VMT.__add = function(a, b)
	assert(isv(a) and isv(b), "vector + non-vector")
	return V(a.X + b.X, a.Y + b.Y, a.Z + b.Z)
end
VMT.__sub = function(a, b)
	assert(isv(a) and isv(b), "vector - non-vector")
	return V(a.X - b.X, a.Y - b.Y, a.Z - b.Z)
end
VMT.__mul = function(a, b)
	if type(a) == "number" then
		return V(a * b.X, a * b.Y, a * b.Z)
	elseif type(b) == "number" then
		return V(a.X * b, a.Y * b, a.Z * b)
	end
	return V(a.X * b.X, a.Y * b.Y, a.Z * b.Z)
end
VMT.__div = function(a, b)
	if type(b) == "number" then
		return V(a.X / b, a.Y / b, a.Z / b)
	end
	return V(a.X / b.X, a.Y / b.Y, a.Z / b.Z)
end
VMT.__unm = function(a)
	return V(-a.X, -a.Y, -a.Z)
end
VMT.__eq = function(a, b)
	return a.X == b.X and a.Y == b.Y and a.Z == b.Z
end
VMT.__tostring = function(a)
	return string.format("(%.2f, %.2f, %.2f)", a.X, a.Y, a.Z)
end
VMT.__index = function(v, k)
	if k == "x" or k == "R" then
		return rawget(v, "X")
	elseif k == "y" or k == "G" then
		return rawget(v, "Y")
	elseif k == "z" or k == "B" then
		return rawget(v, "Z")
	elseif k == "Magnitude" then
		return math.sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z)
	elseif k == "Unit" then
		local m = math.sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z)
		return V(v.X / m, v.Y / m, v.Z / m)
	elseif k == "Lerp" then
		return function(a, b, t)
			return a + (b - a) * t
		end
	elseif k == "Dot" then
		return function(a, b)
			return a.X * b.X + a.Y * b.Y + a.Z * b.Z
		end
	end
	error("vector has no member " .. tostring(k))
end

local function checkNum(n, what)
	if type(n) ~= "number" or n ~= n or n == math.huge or n == -math.huge then
		error("bad number for " .. what .. ": " .. tostring(n))
	end
end

local function checkVec(v, what)
	if not isv(v) then
		error(what .. " expects a vector, got " .. type(v))
	end
	checkNum(v.X, what .. ".X")
	checkNum(v.Y, what .. ".Y")
	checkNum(v.Z, what .. ".Z")
end

---------------------------------------------------------------- rotation library (rotation vectors)
local function matFromRotvec(r)
	local th = math.sqrt(r.X * r.X + r.Y * r.Y + r.Z * r.Z)
	if th < 1e-9 then
		return { 1, 0, 0, 0, 1, 0, 0, 0, 1 }
	end
	local x, y, z = r.X / th, r.Y / th, r.Z / th
	local c, s = math.cos(th), math.sin(th)
	local t = 1 - c
	return {
		t * x * x + c, t * x * y - s * z, t * x * z + s * y,
		t * x * y + s * z, t * y * y + c, t * y * z - s * x,
		t * x * z - s * y, t * y * z + s * x, t * z * z + c,
	}
end
local function rotvecFromMat(m)
	local tr = m[1] + m[5] + m[9]
	local c = math.max(-1, math.min(1, (tr - 1) / 2))
	local th = math.acos(c)
	if th < 1e-9 then
		return V(0, 0, 0)
	end
	if math.abs(math.pi - th) < 1e-6 then
		local x = math.sqrt(math.max(0, (m[1] + 1) / 2))
		local y = math.sqrt(math.max(0, (m[5] + 1) / 2))
		local z = math.sqrt(math.max(0, (m[9] + 1) / 2))
		if m[2] < 0 then y = -y end
		if m[3] < 0 then z = -z end
		return V(x, y, z) * th
	end
	local s = 2 * math.sin(th)
	return V((m[8] - m[6]) / s, (m[3] - m[7]) / s, (m[4] - m[2]) / s) * th
end
local function mul(a, b)
	local r = {}
	for i = 0, 2 do
		for j = 0, 2 do
			r[i * 3 + j + 1] = a[i * 3 + 1] * b[j + 1] + a[i * 3 + 2] * b[3 + j + 1] + a[i * 3 + 3] * b[6 + j + 1]
		end
	end
	return r
end
local function apply(m, v)
	return V(m[1] * v.X + m[2] * v.Y + m[3] * v.Z, m[4] * v.X + m[5] * v.Y + m[6] * v.Z, m[7] * v.X + m[8] * v.Y + m[9] * v.Z)
end
local function rx(a) local c, s = math.cos(a), math.sin(a) return { 1, 0, 0, 0, c, -s, 0, s, c } end
local function ry(a) local c, s = math.cos(a), math.sin(a) return { c, 0, s, 0, 1, 0, -s, 0, c } end
local function rz(a) local c, s = math.cos(a), math.sin(a) return { c, -s, 0, s, c, 0, 0, 0, 1 } end
local function eulerMat(deg)
	return mul(mul(ry(math.rad(deg.Y)), rx(math.rad(deg.X))), rz(math.rad(deg.Z)))
end
local function matEuler(m)
	-- R = Ry * Rx * Rz ; m[6] = -sin(x)
	local x = math.asin(math.max(-1, math.min(1, -m[6])))
	local y = math.atan(m[3], m[9])
	local z = math.atan(m[4], m[5])
	return V(math.deg(x), math.deg(y), math.deg(z))
end
local function cross(a, b)
	return V(a.Y * b.Z - a.Z * b.Y, a.Z * b.X - a.X * b.Z, a.X * b.Y - a.Y * b.X)
end
local function norm(v)
	local m = math.sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z)
	if m < 1e-9 then return V(0, 0, -1) end
	return v / m
end
M.rotation = {
	compose = function(a, b) checkVec(a, "compose a"); checkVec(b, "compose b"); return rotvecFromMat(mul(matFromRotvec(a), matFromRotvec(b))) end,
	apply = function(r, v) checkVec(r, "apply r"); checkVec(v, "apply v"); return apply(matFromRotvec(r), v) end,
	slerp = function(a, b, t) return a + (b - a) * t end,
	look = function(forward, up)
		checkVec(forward, "look forward")
		up = up or V(0, 1, 0)
		local f = norm(forward)
		local r = cross(f, up)
		if math.sqrt(r.X * r.X + r.Y * r.Y + r.Z * r.Z) < 1e-6 then
			r = cross(f, math.abs(f.Y) < 0.9 and V(0, 1, 0) or V(1, 0, 0))
		end
		r = norm(r)
		local u = cross(r, f)
		local back = -f
		return rotvecFromMat({ r.X, u.X, back.X, r.Y, u.Y, back.Y, r.Z, u.Z, back.Z })
	end,
	axis = function(ax, angle) checkVec(ax, "axis"); checkNum(angle, "axis angle"); return norm(ax) * angle end,
	from_euler = function(deg) checkVec(deg, "from_euler"); return rotvecFromMat(eulerMat(deg)) end,
	to_euler = function(r) checkVec(r, "to_euler"); return matEuler(matFromRotvec(r)) end,
}

---------------------------------------------------------------- signals
local function Signal()
	local s = { fns = {} }
	function s:Connect(fn)
		table.insert(self.fns, fn)
		return { Connected = true, Disconnect = function() end }
	end
	function s:Fire(...)
		for _, fn in ipairs(self.fns) do
			fn(...)
		end
	end
	function s:Emit(...)
		self:Fire(...)
	end
	return s
end
M.Signal = Signal

---------------------------------------------------------------- instances
local IMT = {}
local WORLD

local METHODS = {}
local function newInst(className, name, side)
	local data = {
		Name = name or className,
		Position = V(0, 0, 0),
		Orientation = V(0, 0, 0),
		Size = V(4, 1, 2),
		Transparency = 0,
		Velocity = V(0, 0, 0),
		Color = V(0.6, 0.6, 0.6),
		Anchored = false,
		Collider = "Auto",
	}
	local inst = setmetatable({ __data = data, __class = className, __children = {}, __comp = {}, __alive = true, __side = side, __attr = {} }, IMT)
	data.attr = inst.__attr
	table.insert(M.instances, inst)
	return inst
end
M.newInst = newInst

local INTERNAL = { attr = true }
IMT.__index = function(inst, k)
	if type(k) == "string" and string.sub(k, 1, 2) == "__" then
		return nil
	end
	local m = METHODS[k]
	if m ~= nil then
		return m
	end
	if k == "Parent" then
		return rawget(inst, "__parent")
	end
	if not inst.__alive and k ~= "IsAlive" then
		error("read of destroyed instance")
	end
	local side = M.currentSide or "server"
	if not INTERNAL[k] and not M.allowed[side][k] then
		M.unknown[side .. ":read:" .. tostring(k)] = true
		error("unknown Instance member " .. tostring(k))
	end
	local comp = rawget(inst, "__comp")[k]
	if comp ~= nil and type(comp) == "table" then
		return comp
	end
	if k == "Touched" or k == "TouchEnded" or k == "ChildAdded" or k == "ChildRemoved" or k == "Changed" then
		local d = inst.__data
		d[k] = d[k] or Signal()
		return d[k]
	end
	return inst.__data[k]
end
local realNewindex
IMT.__newindex = function(inst, k, v)
	local ok, err = pcall(realNewindex, inst, k, v)
	if not ok then
		M.badWrites[tostring(k) .. ": " .. tostring(err):gsub("^.-:%d+: ", "")] = (M.badWrites[tostring(k) .. ": " .. tostring(err):gsub("^.-:%d+: ", "")] or 0) + 1
		error(err, 2)
	end
end
M.badWrites = {}
realNewindex = function(inst, k, v)
	if type(k) == "string" and string.sub(k, 1, 2) == "__" then
		rawset(inst, k, v)
		return
	end
	if not inst.__alive then
		error("write to destroyed instance")
	end
	local side = M.currentSide or "server"
	if k == "Parent" then
		local old = rawget(inst, "__parent")
		if old ~= nil then
			for i, c in ipairs(old.__children) do
				if c == inst then table.remove(old.__children, i) break end
			end
		end
		rawset(inst, "__parent", v)
		if v ~= nil then
			table.insert(v.__children, inst)
		end
		return
	end
	if not M.allowed[side][k] then
		M.unknown[side .. ":write:" .. tostring(k)] = true
		error("unknown Instance member " .. tostring(k))
	end
	if k == "Position" or k == "Orientation" or k == "Velocity" then
		if inst.__class ~= "Frame" and inst.__class ~= "TextLabel" and inst.__class ~= "ScreenGui" then
			checkVec(v, k)
		end
	elseif k == "Size" then
		if isv(v) then
			checkVec(v, k)
			if v.X < 0 or v.Y < 0 or v.Z < 0 then
				error("negative size " .. tostring(v))
			end
		end
	elseif k == "Transparency" or k == "Health" or k == "WalkSpeed" or k == "Intensity" or k == "BillboardWorldSize" then
		checkNum(v, k)
	elseif k == "Color" or k == "BillboardColor" or k == "LightColor" or k == "BackgroundColor3" or k == "TextColor3" then
		checkVec(v, k)
	elseif k == "UIPosition" or k == "UISize" then
		assert(type(v) == "table" and v.kind == "UDim2", k .. " expects UDim2")
	elseif k == "Text" or k == "BillboardText" or k == "Name" or k == "Material" then
		assert(type(v) == "string", k .. " expects a string, got " .. type(v))
	end
	if k == "Velocity" then
		inst.__velWritten = true
	end
	inst.__data[k] = v
end

function METHODS.IsAlive(inst)
	return inst.__alive
end
function METHODS.Destroy(inst)
	inst.__alive = false
end
function METHODS.AddComponent(inst, ...)
	for _, name in ipairs({ ... }) do
		if not M.components[name] then
			error("unknown component " .. tostring(name))
		end
		inst.__comp[name] = inst.__comp[name] or true
	end
	return inst
end
function METHODS.RemoveComponent(inst, name)
	inst.__comp[name] = nil
end
function METHODS.HasComponent(inst, name)
	return inst.__comp[name] ~= nil
end
function METHODS.GetChildren(inst)
	local out = {}
	for _, c in ipairs(inst.__children) do
		if c.__alive then table.insert(out, c) end
	end
	return out
end
function METHODS.GetDescendants(inst)
	local out = {}
	local function walk(n)
		for _, c in ipairs(n.__children) do
			if c.__alive then
				table.insert(out, c)
				walk(c)
			end
		end
	end
	walk(inst)
	return out
end
function METHODS.FindFirstChild(inst, name, recursive)
	for _, c in ipairs(inst.__children) do
		if c.__alive and c.__data.Name == name then
			return c
		end
	end
	if recursive then
		for _, c in ipairs(inst.__children) do
			local r = METHODS.FindFirstChild(c, name, true)
			if r then return r end
		end
	end
	return nil
end
function METHODS.FindAncestorWithComponent(inst, name)
	local p = inst
	while p ~= nil and p ~= WORLD do
		if p.__comp and p.__comp[name] then
			return p
		end
		p = rawget(p, "__parent")
	end
	return nil
end
function METHODS.TransformDirection(inst, d)
	checkVec(d, "TransformDirection")
	return apply(eulerMat(inst.__data.Orientation), d)
end
function METHODS.TransformPoint(inst, p)
	checkVec(p, "TransformPoint")
	return inst.__data.Position + apply(eulerMat(inst.__data.Orientation), p)
end
function METHODS.LookAt(inst, target, up)
	checkVec(target, "LookAt")
	local d = target - inst.__data.Position
	inst.__data.Orientation = M.rotation.to_euler(M.rotation.look(d, up or V(0, 1, 0)))
end
function METHODS.TakeDamage(inst, amount, attacker)
	checkNum(amount, "TakeDamage")
	if inst.__comp.Invulnerable then return end
	inst.__data.Health = math.max(0, (inst.__data.Health or 0) - amount)
end
function METHODS.Clone(inst)
	local c = newInst(inst.__class, inst.__data.Name, M.currentSide)
	for k, v in pairs(inst.__data) do c.__data[k] = v end
	return c
end
function METHODS.GetSignal(inst, name)
	inst.__data["sig_" .. name] = inst.__data["sig_" .. name] or Signal()
	return inst.__data["sig_" .. name]
end

---------------------------------------------------------------- world
WORLD = setmetatable({ __children = {} }, {
	__index = function(t, k)
		if k == "Each" then
			return function(component)
				if type(component) == "table" then component = rawget(t, "__lastEach") end
				if not M.components[component] then error("unknown component " .. tostring(component)) end
				local list = {}
				for _, inst in ipairs(M.instances) do
					if inst.__alive and inst.__comp[component] then table.insert(list, inst) end
				end
				local i = 0
				return function()
					i = i + 1
					return list[i]
				end
			end
		elseif k == "FindFirstChild" then
			return function(a, b)
				local name = type(a) == "string" and a or b
				for _, c in ipairs(t.__children) do
					if c.__alive and c.__data.Name == name then return c end
				end
				return nil
			end
		elseif k == "GetChildren" then
			return function()
				local out = {}
				for _, c in ipairs(t.__children) do
					if c.__alive then table.insert(out, c) end
				end
				return out
			end
		elseif k == "Camera" then
			return M.currentSide == "client" and M.camera or nil
		elseif k == "Screen" then
			return M.screen
		elseif k == "Name" then
			return "World"
		end
		error("World has no member " .. tostring(k))
	end,
})
M.WORLD = WORLD

---------------------------------------------------------------- physics (AABB raycasts)
local function rayBox(o, d, minv, maxv)
	local tmin, tmax = -math.huge, math.huge
	local normal = nil
	local axes = { "X", "Y", "Z" }
	local nmin
	for _, a in ipairs(axes) do
		local oa, da = o[a], d[a]
		if math.abs(da) < 1e-9 then
			if oa < minv[a] or oa > maxv[a] then return nil end
		else
			local t1 = (minv[a] - oa) / da
			local t2 = (maxv[a] - oa) / da
			local sign = -1
			if t1 > t2 then t1, t2 = t2, t1; sign = 1 end
			if t1 > tmin then
				tmin = t1
				nmin = { a, sign }
			end
			if t2 < tmax then tmax = t2 end
			if tmin > tmax then return nil end
		end
	end
	if tmax < 0 then return nil end
	local t = tmin >= 0 and tmin or 0
	local n = V(0, 0, 0)
	if nmin then
		local a, s = nmin[1], nmin[2]
		n = V(a == "X" and s or 0, a == "Y" and s or 0, a == "Z" and s or 0)
		-- normal points against the ray along the hit axis
		local dn = d[a]
		n = V(a == "X" and (dn > 0 and -1 or 1) or 0, a == "Y" and (dn > 0 and -1 or 1) or 0, a == "Z" and (dn > 0 and -1 or 1) or 0)
	end
	return t, n
end

function M.raycast(origin, dir, maxDist, params)
	checkVec(origin, "Raycast origin")
	checkVec(dir, "Raycast dir")
	checkNum(maxDist, "Raycast distance")
	local exclude = {}
	if type(params) == "table" and getmetatable(params) ~= IMT and params.ExcludeInstances then
		for _, e in ipairs(params.ExcludeInstances) do exclude[e] = true end
	elseif getmetatable(params) == IMT then
		exclude[params] = true
	end
	local m = math.sqrt(dir.X ^ 2 + dir.Y ^ 2 + dir.Z ^ 2)
	if m < 1e-6 or maxDist <= 0 then return nil end
	local d = dir / m
	local best, bestT, bestN = nil, maxDist, nil
	for _, inst in ipairs(M.instances) do
		local data = inst.__data
		if inst.__alive and not exclude[inst] and inst.__solid and data.Collider ~= "None" and not data.Trigger then
			local half = data.Size / 2
			local t, n = rayBox(origin, d, data.Position - half, data.Position + half)
			if t and t <= bestT then
				best, bestT, bestN = inst, t, n
			end
		end
	end
	if best == nil then return nil end
	return { Position = origin + d * bestT, Normal = bestN, Entity = best, Instance = best, Distance = bestT }
end

---------------------------------------------------------------- event tables
M.tables = { server = {}, client = {} }
M.sent = {}
M.outgoing = { server = {}, client = {} }

local function makeEventTable(side, name, direction, fields)
	local existing = M.tables[side][name]
	if existing then return existing end
	assert(type(name) == "string", "EventTable name")
	assert(type(fields) == "table", "EventTable fields")
	local et = { count = 0, sender = {}, __fields = fields, __name = name, __dir = direction }
	for _, f in ipairs(fields) do
		assert(type(f[1]) == "string" and type(f[2]) == "table", "bad field spec in " .. name)
		et[f[1]] = {}
	end
	function et.Push(self, ...)
		local args = table.pack(...)
		assert(args.n == #fields, name .. ":Push got " .. args.n .. " values, expects " .. #fields)
		for i, f in ipairs(fields) do
			local kind = f[2].kind
			local v = args[i]
			if kind == "F32" or kind == "I32" or kind == "U8" then
				checkNum(v, name .. "." .. f[1])
				if kind == "U8" then assert(v >= 0 and v <= 255 and math.floor(v) == v, name .. "." .. f[1] .. " U8 out of range: " .. v) end
			elseif kind == "Vec3" or kind == "Color" then
				checkVec(v, name .. "." .. f[1])
			elseif kind == "Entity" then
				assert(getmetatable(v) == IMT, name .. "." .. f[1] .. " expects an Instance")
			elseif kind == "Bool" then
				assert(type(v) == "boolean")
			end
		end
		table.insert(M.outgoing[side], { name = name, args = args, to = nil })
		M.sent[#M.sent + 1] = { side = side, name = name, args = args }
	end
	function et.PushTo(self, userId, ...)
		checkNum(userId, "PushTo userId")
		local before = #M.outgoing[side]
		self:Push(...)
		M.outgoing[side][before + 1].to = userId
	end
	M.tables[side][name] = et
	return et
end

local function deliver(et, rows, senders)
	et.count = #rows
	for _, f in ipairs(et.__fields) do
		local col = {}
		for i, row in ipairs(rows) do
			col[i] = row[f.__index]
		end
		et[f[1]] = col
	end
	et.sender = senders or {}
end

function M.bridge(clientUserId)
	-- client -> server (this frame), server -> client (this frame)
	local function route(fromSide, toSide, filterTo)
		local byName = {}
		for _, ev in ipairs(M.outgoing[fromSide]) do
			if filterTo == nil or ev.to == nil or ev.to == filterTo then
				byName[ev.name] = byName[ev.name] or { rows = {}, senders = {} }
				local row = {}
				for i = 1, ev.args.n do row[i] = ev.args[i] end
				table.insert(byName[ev.name].rows, row)
				table.insert(byName[ev.name].senders, clientUserId)
			end
		end
		for name, et in pairs(M.tables[toSide]) do
			local bucket = byName[name] or { rows = {}, senders = {} }
			et.count = #bucket.rows
			for fi, f in ipairs(et.__fields) do
				local col = {}
				for ri, row in ipairs(bucket.rows) do col[ri] = row[fi] end
				et[f[1]] = col
			end
			et.sender = bucket.senders
		end
		M.outgoing[fromSide] = {}
	end
	route("client", "server", nil)
	route("server", "client", clientUserId)
end

---------------------------------------------------------------- environments
local function etGlobal(side)
	local ET = setmetatable({
		ToServer = { kind = "ToServer" }, ToClients = { kind = "ToClients" },
		F32 = { kind = "F32" }, I32 = { kind = "I32" }, U8 = { kind = "U8" }, Bool = { kind = "Bool" },
		Vec3 = { kind = "Vec3" }, Color = { kind = "Color" }, Entity = { kind = "Entity" }, Instance = { kind = "Entity" }, String = { kind = "String" },
	}, {
		__call = function(_, name, direction, fields)
			return makeEventTable(side, name, direction, fields)
		end,
	})
	return ET
end

local function prefabSpawn(side)
	return function(name)
		assert(type(name) == "string")
		if not M.prefabs[name] then return nil end
		local inst = newInst(name, name, side)
		inst.__solid = (name == "Part" or name == "Sphere" or name == "Cylinder" or name == "Wedge" or name == "Cone" or name == "SpawnLocation")
		if name == "SpawnLocation" then inst.__comp.SpawnPoint = true end
		if name == "PlayerSpawner" then inst.__comp.PlayerSpawner = { character = "Character", respawnDelay = 3 } end
		if name == "PointLight" then inst.__data.Intensity = 1 inst.__solid = false end
		if name == "Frame" or name == "TextLabel" or name == "ScreenGui" then inst.__solid = false end
		return inst
	end
end

function M.makeEnv(side, selfInst)
	local env = {}
	local luauTable = setmetatable({
		clone = function(t) local c = {} for k, v in pairs(t) do c[k] = v end return c end,
		find = function(t, v) for i, x in ipairs(t) do if x == v then return i end end return nil end,
	}, { __index = table })
	local luauMath = setmetatable({
		clamp = function(x, lo, hi) checkNum(x, "math.clamp") if x < lo then return lo elseif x > hi then return hi end return x end,
		atan2 = function(y, x) return math.atan(y, x) end,
	}, { __index = math })
	local bit32 = {
		band = function(a, b) return math.tointeger(a) & math.tointeger(b) end,
		bor = function(a, b) return math.tointeger(a) | math.tointeger(b) end,
		bnot = function(a) return (~math.tointeger(a)) & 0xFFFFFFFF end,
	}
	local Players = {
		PlayerAdded = Signal(), PlayerRemoving = Signal(), CharacterAdded = Signal(), CharacterDied = Signal(), PlayerChatted = Signal(),
		Get = function(uid) return M.playersByUid[uid] end,
		All = function() local out = {} for _, p in pairs(M.playersByUid) do table.insert(out, p) end return out end,
		FromCharacter = function(c) return nil end,
	}
	if side == "client" then
		Players.LocalPlayer = M.localPlayer
	end
	M.signals = M.signals or {}
	M.signals[side] = Players
	local game = {
		EventTable = etGlobal(side),
		World = WORLD,
		Prefabs = { Spawn = prefabSpawn(side), Exists = function(n) return M.prefabs[n] == true end },
		Physics = { Raycast = M.raycast, Gravity = 196.2 },
		Players = Players,
		Session = { LocalCharacter = function() return M.localPlayer and M.localPlayer.__data.Character end, PlayerJoined = Signal(), PlayerLeft = Signal(), CharacterDied = Signal() },
		Lighting = { Sun = {}, Ambient = {} },
		Materials = {
			GetMaterials = function() return { { Id = "plastic", Name = "Plastic" }, { Id = "neon", Name = "Neon" } } end,
			GetMaterial = function(id) return { Surface = { Neon = id == "neon" } } end,
		},
		Tween = { TweenProperty = function() end },
		Sound = { PlayAt = function() return true end },
	}
	if side == "client" then
		game.Input = M.input
		game.Overlay = M.screen
	end
	env.game = game
	env.self = selfInst
	env.Vector3 = { new = function(x, y, z) return V(x or 0, y or 0, z or 0) end, zero = V(0, 0, 0), one = V(1, 1, 1), xAxis = V(1, 0, 0), yAxis = V(0, 1, 0), zAxis = V(0, 0, 1) }
	env.Vector2 = { new = function(x, y) return V(x or 0, y or 0, 0) end }
	env.Color3 = {
		new = function(r, g, b) return V(r or 0, g or 0, b or 0) end,
		fromRGB = function(r, g, b) return V((r or 0) / 255, (g or 0) / 255, (b or 0) / 255) end,
		fromHSV = function(h, s, v) checkNum(h, "hsv") return V(v, v * (1 - s), v * (1 - s * h)) end,
		fromHex = function() return V(1, 1, 1) end,
	}
	env.UDim2 = { new = function(a, b, c, d) return { kind = "UDim2", a or 0, b or 0, c or 0, d or 0 } end }
	env.Instance = { new = function(name) local i = newInst(name == "Part" and "Part" or "Instance", name, side) i.__solid = name == "Part" i.__data.Parent = nil table.insert(WORLD.__children, i) rawset(i, "__parent", WORLD) return i end }
	env.task = {
		delay = function(sec, fn, ...) local args = table.pack(...) table.insert(M.timers, { at = M.clock + sec, fn = fn, args = args }) return {} end,
		defer = function(fn, ...) local args = table.pack(...) table.insert(M.timers, { at = M.clock, fn = fn, args = args }) return {} end,
		spawn = function(fn, ...) fn(...) return {} end,
		wait = function() error("task.wait not supported in harness") end,
	}
	env.rotation = M.rotation
	env.print = function(...) local t = {} for i, v in ipairs({ ... }) do t[i] = tostring(v) end table.insert(M.logs, side .. ": " .. table.concat(t, " ")) end
	env.warn = function(...) local t = {} for i, v in ipairs({ ... }) do t[i] = tostring(v) end table.insert(M.logs, side .. " WARN: " .. table.concat(t, " ")) end
	env.tick = function() return M.clock end
	env.pcall = pcall
	env.type = type
	env.typeof = type
	env.tostring = tostring
	env.tonumber = tonumber
	env.ipairs = ipairs
	env.pairs = pairs
	env.next = next
	env.select = select
	env.error = error
	env.assert = assert
	env.setmetatable = setmetatable
	env.getmetatable = getmetatable
	env.rawget = rawget
	env.rawset = rawset
	env.math = luauMath
	env.string = string
	env.table = luauTable
	env.bit32 = bit32
	return env
end

---------------------------------------------------------------- input + character "engine"
M.keysDown, M.keysPressed, M.buttonsDown, M.buttonsPressed = {}, {}, {}, {}
local keyNames = { "W", "A", "S", "D", "F", "Q", "R", "E", "H", "Space", "Tab", "LeftControl", "RightControl", "One", "Two", "Three" }
local Key = {}
for _, n in ipairs(keyNames) do Key[n] = n end
M.input = {
	Key = Key,
	Button = { Left = "Left", Right = "Right", Middle = "Middle" },
	KeyDown = function(k) assert(k ~= nil, "KeyDown(nil)") return M.keysDown[k] == true end,
	KeyPressed = function(k) assert(k ~= nil, "KeyPressed(nil)") return M.keysPressed[k] == true end,
	ButtonDown = function(b) return M.buttonsDown[b] == true end,
	ButtonPressed = function(b) return M.buttonsPressed[b] == true end,
	PointerRay = function() return nil, nil end,
	MousePosition = V(0, 0, 0), MouseDelta = V(0, 0, 0), Scroll = 0, MouseBehavior = 0, RotationType = 0,
}

M.move = V(0, 0, 0)
M.wantJump = false
function M.stepCharacters(dt)
	for _, inst in ipairs(M.instances) do
		if inst.__alive and inst.__comp.Character then
			local d = inst.__data
			local v = d.Velocity
			if not inst.__velWritten then
				local speed = d.WalkSpeed or 16
				v = V(M.move.X * speed, v.Y, M.move.Z * speed)
			end
			inst.__velWritten = false
			local hit = M.raycast(d.Position, V(0, -1, 0), 3.05)
			local grounded = hit ~= nil and v.Y <= 0.01
			if grounded and M.wantJump and (d.JumpPower or 0) > 0 then
				v = V(v.X, d.JumpPower, v.Z)
				grounded = false
			end
			M.wantJump = false
			if not grounded then
				v = V(v.X, v.Y - 196.2 * dt, v.Z)
			end
			local p = d.Position + v * dt
			local g = M.raycast(V(p.X, d.Position.Y + 0.5, p.Z), V(0, -1, 0), 60)
			if g ~= nil and p.Y <= g.Position.Y + 3 and v.Y <= 0 then
				p = V(p.X, g.Position.Y + 3, p.Z)
				v = V(v.X, 0, v.Z)
			end
			d.Position = p
			d.Velocity = v
		end
	end
end

function M.runTimers()
	local due = {}
	local keep = {}
	for _, t in ipairs(M.timers) do
		if t.at <= M.clock then table.insert(due, t) else table.insert(keep, t) end
	end
	M.timers = keep
	for _, t in ipairs(due) do t.fn(table.unpack(t.args, 1, t.args.n)) end
end

M.IMT = IMT
return M
