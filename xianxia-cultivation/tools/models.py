"""Box-model generator for the add-on's entities.

Each model is a list of bones made of cubes. Every cube face gets its own
region on the texture (per-face UVs), so textures are painted directly from
the model description and always line up with the geometry.
"""
import math
from textures import Canvas, shade, rgba, TRANSPARENT


class Cube:
    def __init__(self, origin, size, col, detail=None, inflate=0, rotation=None, pivot=None, faces=None):
        self.origin, self.size, self.col = origin, size, col
        self.detail = detail or {}
        self.inflate = inflate
        self.rotation, self.pivot = rotation, pivot
        self.faces = faces  # optional subset of faces
        self.uv = {}


class Bone:
    def __init__(self, name, pivot, cubes, parent=None, rotation=None):
        self.name, self.pivot, self.cubes = name, pivot, cubes
        self.parent, self.rotation = parent, rotation


FACES = ("north", "south", "east", "west", "up", "down")


def face_dims(cube, face):
    sx, sy, sz = (max(0, int(math.ceil(v))) for v in cube.size)
    if face in ("north", "south"):
        return sx, sy
    if face in ("east", "west"):
        return sz, sy
    return sx, sz


class Model:
    def __init__(self, ident, bones, tex_w=64, seed="m"):
        self.ident, self.bones, self.tex_w, self.seed = ident, bones, tex_w, seed

    def pack(self):
        """Shelf-pack every face rectangle into the texture."""
        rects = []
        for b in self.bones:
            for cu in b.cubes:
                for f in (cu.faces or FACES):
                    w, h = face_dims(cu, f)
                    if w > 0 and h > 0:
                        rects.append((h, w, cu, f))
        rects.sort(key=lambda r: (-r[0], -r[1]))
        x = y = shelf_h = 0
        W = self.tex_w
        for h, w, cu, f in rects:
            if w > W:
                raise ValueError(f"face too wide for texture: {w}")
            if x + w > W:
                x, y, shelf_h = 0, y + shelf_h, 0
            cu.uv[f] = (x, y, w, h)
            x += w
            shelf_h = max(shelf_h, h)
        H = 16
        while H < y + shelf_h:
            H *= 2
        self.tex_h = H

    def geometry(self):
        self.pack()
        bones = []
        for b in self.bones:
            jb = {"name": b.name, "pivot": list(b.pivot)}
            if b.parent:
                jb["parent"] = b.parent
            if b.rotation:
                jb["rotation"] = list(b.rotation)
            cubes = []
            for cu in b.cubes:
                jc = {"origin": list(cu.origin), "size": list(cu.size), "uv": {}}
                if cu.inflate:
                    jc["inflate"] = cu.inflate
                if cu.rotation:
                    jc["rotation"] = list(cu.rotation)
                    jc["pivot"] = list(cu.pivot)
                for f, (u, v, w, h) in cu.uv.items():
                    if f in ("up", "down"):
                        # Bedrock samples up/down faces mirrored; flip via negative size
                        jc["uv"][f] = {"uv": [u + w, v + h], "uv_size": [-w, -h]}
                    else:
                        jc["uv"][f] = {"uv": [u, v], "uv_size": [w, h]}
                cubes.append(jc)
            jb["cubes"] = cubes
            bones.append(jb)
        return {
            "format_version": "1.12.0",
            "minecraft:geometry": [{
                "description": {
                    "identifier": self.ident,
                    "texture_width": self.tex_w,
                    "texture_height": self.tex_h,
                    "visible_bounds_width": 4,
                    "visible_bounds_height": 4,
                    "visible_bounds_offset": [0, 1, 0],
                },
                "bones": bones,
            }],
        }

    def texture(self):
        c = Canvas(self.tex_w, self.tex_h, seed=self.seed)
        for b in self.bones:
            for cu in b.cubes:
                for f, (u, v, w, h) in cu.uv.items():
                    paint_face(c, cu, f, u, v, w, h)
        return c


def paint_face(c, cu, face, u, v, w, h):
    col = cu.col
    d = cu.detail
    light = {"up": 1.18, "down": 0.7, "north": 1.0, "south": 0.92, "east": 0.85, "west": 0.95}[face]
    belly = d.get("belly")
    for y in range(h):
        for x in range(w):
            base = col
            if face == "down" and belly:
                base = belly
            if d.get("gradient") and face not in ("up", "down"):
                t = y / max(1, h - 1)
                base = tuple(int(col[i] * (1 - t) + d["gradient"][i] * t) for i in range(3))
            f = light * (1 + c.rng.uniform(-d.get("noise", 0.07), d.get("noise", 0.07)))
            c.set(u + x, v + y, shade(base, f))
    # stripes (tiger) on sides and top
    if d.get("stripes") and face in ("east", "west", "up", "north", "south"):
        sc = d["stripes"]
        step = 3
        for x in range(0, w, step):
            length = int(h * (0.5 + 0.4 * c.rng.random()))
            for y in range(length):
                xx = x + (y // 3) % 2
                if xx < w:
                    c.set(u + xx, v + y, sc)
    # trim band on vertical faces (robes)
    if d.get("trim") and face not in ("up", "down"):
        tc, rows = d["trim"]
        for r in rows:
            rr = r if r >= 0 else h + r
            if 0 <= rr < h:
                for x in range(w):
                    c.set(u + x, v + rr, tc)
    if d.get("sash") and face in ("north", "south"):
        for x in range(w):
            c.set(u + x, v + d["sash"][1], d["sash"][0])
            if 0 <= d["sash"][1] + 1 < h:
                c.set(u + x, v + d["sash"][1] + 1, shade(d["sash"][0], 0.8))
    if d.get("collar") and face == "north":
        cc = d["collar"]
        mid = w // 2
        for i in range(min(h, 5)):
            c.set(u + mid - 1 - i // 2, v + i, cc)
            c.set(u + mid + i // 2, v + i, cc)
    if face == "north" and d.get("eyes"):
        ec = d["eyes"]
        ey = d.get("eye_row", h // 2 - 1)
        gap = d.get("eye_gap", 1)
        mid = w / 2
        lx = int(mid - gap - 1)
        rx = int(mid + gap)
        c.set(u + lx, v + ey, ec)
        c.set(u + rx, v + ey, ec)
        if d.get("pupils"):
            c.set(u + lx + 1, v + ey, d["pupils"])
            c.set(u + rx - 1, v + ey, d["pupils"])
        if d.get("brows"):
            c.set(u + lx, v + ey - 1, d["brows"])
            c.set(u + rx, v + ey - 1, d["brows"])
        if d.get("mouth"):
            for x in range(int(mid - 1), int(mid + 1)):
                c.set(u + x, v + ey + 2, d["mouth"])
    if face == "north" and d.get("nose"):
        c.set(u + w // 2, v + 0, d["nose"])
        c.set(u + w // 2 - 1, v + 0, d["nose"])
    if d.get("hair_front") and face == "north":
        hc = d["hair_front"]
        for x in range(w):
            c.set(u + x, v, hc)
            if x < 2 or x >= w - 2:
                c.set(u + x, v + 1, hc)
                c.set(u + x, v + 2, hc)
    if d.get("hair") and face in ("up", "south", "east", "west"):
        hc = d["hair"]
        rows = h if face in ("up", "south") else max(1, h // 2)
        for y in range(rows):
            for x in range(w):
                c.set(u + x, v + y, shade(hc, 1 + c.rng.uniform(-0.08, 0.08)))
    if d.get("glow_spots") and face in ("east", "west", "up"):
        gc = d["glow_spots"]
        for _ in range(max(1, w * h // 18)):
            c.set(u + c.rng.randrange(w), v + c.rng.randrange(h), gc)


# ---------------------------------------------------------------------------
# Model builders
# ---------------------------------------------------------------------------

def quadruped(ident, seed, *, fur, belly, body, leg, head, snout, ears, tails, tail_size, eye, extra=None,
              stripes=None, tail_tip=None, tex_w=64, mane=None):
    bw, bh, bl = body
    lw, lh = leg
    hw, hh, hl = head
    body_y = lh
    bones = []
    det = {"belly": belly}
    if stripes:
        det["stripes"] = stripes
    bones.append(Bone("body", [0, body_y + bh / 2, 0], [
        Cube([-bw / 2, body_y, -bl / 2], [bw, bh, bl], fur, det)]))
    if mane:
        bones[-1].cubes.append(Cube([-bw / 2 - 1, body_y - 1, -bl / 2 - 1], [bw + 2, bh + 2, 5], mane,
                                    {"noise": 0.15}))
    head_pivot = [0, body_y + bh * 0.75, -bl / 2]
    hz = -bl / 2 - hl + 2
    hy = body_y + bh - 2
    hcubes = [Cube([-hw / 2, hy, hz], [hw, hh, hl], fur,
                   {"eyes": eye, "eye_row": max(1, hh // 2 - 1), "eye_gap": max(1, hw // 4), "belly": belly,
                    **({"stripes": stripes} if stripes else {})})]
    sw, sh, sl = snout
    hcubes.append(Cube([-sw / 2, hy, hz - sl], [sw, sh, sl], belly, {"nose": (30, 25, 30)}))
    ew, eh = ears
    hcubes.append(Cube([-hw / 2, hy + hh, hz + hl - 3], [ew, eh, 1], shade(fur, 0.8)[:3]))
    hcubes.append(Cube([hw / 2 - ew, hy + hh, hz + hl - 3], [ew, eh, 1], shade(fur, 0.8)[:3]))
    if extra:
        hcubes.extend(extra)
    bones.append(Bone("head", head_pivot, hcubes))
    lx = bw / 2 - lw / 2 - 0.5
    lz_f = -bl / 2 + lw / 2 + 1
    lz_b = bl / 2 - lw / 2 - 1
    legs = [("leg_fr", -lx, lz_f), ("leg_fl", lx, lz_f), ("leg_br", -lx, lz_b), ("leg_bl", lx, lz_b)]
    for name, x, z in legs:
        bones.append(Bone(name, [x, lh, z], [
            Cube([x - lw / 2, 0, z - lw / 2], [lw, lh, lw], shade(fur, 0.9),
                 {"gradient": shade(fur, 0.6)[:3], **({"stripes": stripes} if stripes else {})})]))
    tw, tl = tail_size
    for i in range(tails):
        spread = 0 if tails == 1 else (i / (tails - 1) - 0.5) * 70
        main_len = round(tl * 0.7) if tail_tip else tl
        ty = body_y + bh - 1 - tw / 2
        cubes = [Cube([-tw / 2, ty, bl / 2], [tw, tw, main_len], fur)]
        if tail_tip:
            cubes.append(Cube([-tw / 2, ty, bl / 2 + main_len], [tw, tw, tl - main_len], tail_tip))
        bones.append(Bone(f"tail{i}", [0, body_y + bh - 1, bl / 2], cubes,
                          rotation=[35 if tails > 1 else 20, spread, 0]))
    return Model(ident, bones, tex_w=tex_w, seed=seed), [b[0] for b in legs], [f"tail{i}" for i in range(tails)]


def humanoid(ident, seed, *, skin, robe, trim, hair, eyes, pants=None, sash=None, hat=None, robe_len=8,
             pupils=None, glow=None):
    pants = pants or shade(robe, 0.7)[:3]
    bones = []
    head_detail = {"eyes": eyes, "eye_row": 4, "eye_gap": 1, "hair_front": hair, "hair": hair,
                   "brows": shade(hair, 0.8), "mouth": shade(skin, 0.7)}
    if pupils:
        head_detail["pupils"] = pupils
    body_detail = {"trim": (trim, [0, -1]), "collar": trim}
    if sash:
        body_detail["sash"] = (sash, 7)
    bones.append(Bone("body", [0, 24, 0], [Cube([-4, 12, -2], [8, 12, 4], robe, body_detail)]))
    bones.append(Bone("robe", [0, 12, 0], [
        Cube([-4.5, 12 - robe_len, -2.5], [9, robe_len, 5], robe, {"trim": (trim, [-1, -2])})], parent="body"))
    head_cubes = [Cube([-4, 24, -4], [8, 8, 8], skin, head_detail)]
    if hat == "bun":
        head_cubes.append(Cube([-1.5, 32, -0.5], [3, 3, 3], hair))
        head_cubes.append(Cube([-0.5, 33, -1], [1, 1, 4], trim))
    elif hat == "straw":
        head_cubes.append(Cube([-7, 31, -7], [14, 1, 14], (200, 170, 90), {"noise": 0.12}))
        head_cubes.append(Cube([-4, 32, -4], [8, 2, 8], (190, 160, 80), {"noise": 0.12}))
        head_cubes.append(Cube([-2, 34, -2], [4, 1, 4], (180, 150, 70)))
    elif hat == "horns":
        head_cubes.append(Cube([-4, 32, -2], [1, 3, 1], (60, 10, 20)))
        head_cubes.append(Cube([3, 32, -2], [1, 3, 1], (60, 10, 20)))
    elif hat == "crown":
        head_cubes.append(Cube([-2, 32, -1], [4, 2, 3], trim))
    bones.append(Bone("head", [0, 24, 0], head_cubes))
    arm_detail = {"trim": (trim, [-1])}
    bones.append(Bone("rightArm", [-5, 22, 0], [
        Cube([-8, 12, -2], [4, 12, 4], robe, arm_detail),
        Cube([-7.5, 12, -1.5], [3, 1, 3], skin)]))
    bones.append(Bone("leftArm", [5, 22, 0], [
        Cube([4, 12, -2], [4, 12, 4], robe, arm_detail),
        Cube([4.5, 12, -1.5], [3, 1, 3], skin)]))
    bones.append(Bone("rightLeg", [-1.9, 12, 0], [Cube([-3.9, 0, -2], [4, 12, 4], pants,
                                                      {"trim": ((40, 30, 25), [-1, -2])})]))
    bones.append(Bone("leftLeg", [1.9, 12, 0], [Cube([-0.1, 0, -2], [4, 12, 4], pants,
                                                     {"trim": ((40, 30, 25), [-1, -2])})]))
    if glow:
        bones[0].cubes[0].detail["glow_spots"] = glow
    return Model(ident, bones, tex_w=64, seed=seed)


def dragon(ident, seed):
    scale_c = (50, 150, 185)
    belly = (230, 210, 120)
    segs = 6
    bones = []
    seg_len = 12
    w = 10
    # head first (front, -Z)
    head = Bone("head", [0, 10, -seg_len / 2], [
        Cube([-6, 5, -seg_len / 2 - 14], [12, 9, 14], scale_c,
             {"eyes": (255, 220, 60), "pupils": (20, 10, 0), "eye_row": 2, "eye_gap": 3, "belly": belly}),
        Cube([-4, 2, -seg_len / 2 - 13], [8, 3, 12], belly),
        Cube([-5, 14, -seg_len / 2 - 4], [2, 2, 8], (240, 220, 180), rotation=[30, 0, 0],
             pivot=[-4, 14, -seg_len / 2 - 4]),
        Cube([3, 14, -seg_len / 2 - 4], [2, 2, 8], (240, 220, 180), rotation=[30, 0, 0],
             pivot=[4, 14, -seg_len / 2 - 4]),
        Cube([-8, 7, -seg_len / 2 - 13], [2, 1, 10], (240, 230, 160)),
        Cube([6, 7, -seg_len / 2 - 13], [2, 1, 10], (240, 230, 160)),
        Cube([-1, 14, -seg_len / 2 - 6], [2, 3, 10], (240, 200, 60)),
    ], parent="seg0")
    for i in range(segs):
        taper = 1 - i * 0.1
        sw = w * taper
        z0 = -seg_len / 2 + i * seg_len
        parent = f"seg{i - 1}" if i else None
        cubes = [Cube([-sw / 2, 10 - sw / 2, z0], [sw, sw, seg_len], scale_c,
                      {"belly": belly, "glow_spots": (130, 230, 255), "noise": 0.12}),
                 Cube([-1, 10 + sw / 2, z0 + 2], [2, 2, seg_len - 4], (240, 200, 60))]
        bones.append(Bone(f"seg{i}", [0, 10, z0], cubes, parent=parent))
        if i in (1, 4):
            for side, x in (("r", -sw / 2 - 1), ("l", sw / 2 - 2)):
                bones.append(Bone(f"leg{i}{side}", [x + 1.5, 10, z0 + 6], [
                    Cube([x, 0, z0 + 4], [3, 10, 3], scale_c, {"gradient": (30, 80, 100)}),
                    Cube([x - 0.5, 0, z0 + 2.5], [4, 1, 3], (240, 230, 200))], parent=f"seg{i}"))
    tail = Bone("tail", [0, 10, -seg_len / 2 + segs * seg_len], [
        Cube([-1.5, 8.5, -seg_len / 2 + segs * seg_len], [3, 3, 10], scale_c),
        Cube([-3, 9.5, -seg_len / 2 + segs * seg_len + 6], [6, 1, 8], (130, 230, 255))], parent=f"seg{segs - 1}")
    bones.insert(0, head)
    bones.append(tail)
    # parent order: bones must be listed after parents for some tools; put segs first
    ordered = [b for b in bones if b.name.startswith("seg")] + [b for b in bones if not b.name.startswith("seg")]
    return Model(ident, ordered, tex_w=128, seed=seed), [f"seg{i}" for i in range(segs)]


# ---------------------------------------------------------------------------
# Animations
# ---------------------------------------------------------------------------

def quad_animations(eid, legs, tails):
    walk = {}
    phase = {"leg_fr": 1, "leg_bl": 1, "leg_fl": -1, "leg_br": -1}
    for l in legs:
        sign = "" if phase[l] > 0 else "-"
        walk[l] = {"rotation": [f"{sign}math.cos(query.anim_time * 38.17) * 50.0", 0, 0]}
    idle = {}
    for i, t in enumerate(tails):
        idle[t] = {"rotation": [f"math.sin(query.life_time * 90 + {i * 40}) * 6",
                                f"math.sin(query.life_time * 120 + {i * 60}) * 12", 0]}
    return {
        f"animation.xian.{eid}.walk": {"loop": True, "anim_time_update": "query.modified_distance_moved",
                                       "bones": walk},
        f"animation.xian.{eid}.idle": {"loop": True, "bones": idle},
    }


def humanoid_animations(eid):
    return {
        f"animation.xian.{eid}.walk": {
            "loop": True, "anim_time_update": "query.modified_distance_moved",
            "bones": {
                "rightArm": {"rotation": ["math.cos(query.anim_time * 38.17) * 40.0", 0, 0]},
                "leftArm": {"rotation": ["-math.cos(query.anim_time * 38.17) * 40.0", 0, 0]},
                "rightLeg": {"rotation": ["-math.cos(query.anim_time * 38.17) * 35.0", 0, 0]},
                "leftLeg": {"rotation": ["math.cos(query.anim_time * 38.17) * 35.0", 0, 0]},
            }},
        f"animation.xian.{eid}.idle": {
            "loop": True,
            "bones": {
                "rightArm": {"rotation": [0, 0, "math.sin(query.life_time * 60) * 2.5 + 2.5"]},
                "leftArm": {"rotation": [0, 0, "-math.sin(query.life_time * 60) * 2.5 - 2.5"]},
            }},
        f"animation.xian.{eid}.attack": {
            "loop": True,
            "bones": {
                "rightArm": {"rotation": ["-math.sin(variable.attack_time * 180) * 80 - "
                                          "(query.is_delayed_attacking ? 70 : 0)", 0, 0]},
            }},
    }


def dragon_animations(eid, segs):
    bones = {}
    for i, s in enumerate(segs):
        if i == 0:
            continue
        bones[s] = {"rotation": [f"math.sin(query.life_time * 80 + {i * 50}) * 4",
                                 f"math.sin(query.life_time * 100 + {i * 45}) * 12", 0]}
    bones["tail"] = {"rotation": [0, "math.sin(query.life_time * 120) * 20", 0]}
    return {
        f"animation.xian.{eid}.idle": {"loop": True, "bones": bones},
        f"animation.xian.{eid}.walk": {"loop": True, "anim_time_update": "query.modified_distance_moved",
                                       "bones": {
                                           "leg1r": {"rotation": ["math.cos(query.anim_time * 38.17) * 40", 0, 0]},
                                           "leg1l": {"rotation": ["-math.cos(query.anim_time * 38.17) * 40", 0, 0]},
                                           "leg4r": {"rotation": ["-math.cos(query.anim_time * 38.17) * 40", 0, 0]},
                                           "leg4l": {"rotation": ["math.cos(query.anim_time * 38.17) * 40", 0, 0]},
                                       }},
    }


LOOK_AT = {
    "animation.xian.look_at_target": {
        "loop": True,
        "bones": {"head": {"rotation": ["query.target_x_rotation", "query.target_y_rotation", 0]}},
    }
}


def build_entity_models():
    """Returns {entity_id: (Model, animations_dict, animate_list)}."""
    out = {}

    m, legs, tails = quadruped(
        "geometry.xian.spirit_fox", "fox", fur=(245, 245, 255), belly=(255, 255, 255), body=(6, 6, 11),
        leg=(2, 6), head=(7, 6, 6), snout=(3, 3, 3), ears=(2, 3), tails=3, tail_size=(3, 10),
        eye=(80, 180, 255), tail_tip=(120, 210, 255))
    out["spirit_fox"] = (m, quad_animations("spirit_fox", legs, tails))

    m, legs, tails = quadruped(
        "geometry.xian.demonic_wolf", "wolf", fur=(55, 52, 62), belly=(90, 85, 95), body=(8, 8, 14), leg=(3, 8),
        head=(8, 7, 7), snout=(4, 3, 4), ears=(2, 3), tails=1, tail_size=(3, 10), eye=(255, 40, 40),
        mane=(35, 30, 40), tail_tip=(150, 20, 30))
    out["demonic_wolf"] = (m, quad_animations("demonic_wolf", legs, tails))

    m, legs, tails = quadruped(
        "geometry.xian.flame_tiger", "tiger", fur=(240, 130, 40), belly=(250, 235, 210), body=(10, 10, 20),
        leg=(4, 10), head=(10, 9, 8), snout=(5, 4, 3), ears=(3, 3), tails=1, tail_size=(2, 15),
        eye=(255, 240, 80), stripes=(35, 20, 15), tail_tip=(255, 80, 20), tex_w=128)
    out["flame_tiger"] = (m, quad_animations("flame_tiger", legs, tails))

    m, segs = dragon("geometry.xian.flood_dragon", "dragon")
    out["flood_dragon"] = (m, dragon_animations("flood_dragon", segs))

    out["rogue_cultivator"] = (humanoid(
        "geometry.xian.rogue_cultivator", "rogue", skin=(225, 185, 150), robe=(80, 90, 115), trim=(170, 170, 190),
        hair=(25, 20, 20), eyes=(40, 40, 60), sash=(150, 40, 40), hat="bun"), humanoid_animations("rogue_cultivator"))
    out["demonic_cultivator"] = (humanoid(
        "geometry.xian.demonic_cultivator", "demonic", skin=(200, 170, 160), robe=(60, 10, 20), trim=(200, 20, 40),
        hair=(15, 10, 15), eyes=(255, 30, 30), sash=(20, 0, 5), hat="horns", glow=(255, 60, 80)),
        humanoid_animations("demonic_cultivator"))
    out["wandering_merchant"] = (humanoid(
        "geometry.xian.wandering_merchant", "merchant", skin=(230, 195, 160), robe=(150, 100, 50),
        trim=(235, 210, 120), hair=(70, 60, 60), eyes=(40, 30, 20), sash=(60, 120, 60), hat="straw"),
        humanoid_animations("wandering_merchant"))
    out["heart_demon"] = (humanoid(
        "geometry.xian.heart_demon", "heartdemon", skin=(40, 30, 50), robe=(20, 15, 30), trim=(150, 60, 220),
        hair=(5, 5, 10), eyes=(200, 120, 255), sash=(150, 60, 220), hat="crown", glow=(170, 90, 255)),
        humanoid_animations("heart_demon"))
    return out
