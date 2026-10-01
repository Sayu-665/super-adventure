#!/usr/bin/env python3
"""Builds the Xianxia Cultivation behavior + resource packs and the .mcaddon.

Usage:  python3 tools/build.py
Output: packs/Xianxia_Cultivation_BP, packs/Xianxia_Cultivation_RP,
        dist/Xianxia_Cultivation.mcaddon (+ individual .mcpack files)
"""
import json
import os
import shutil
import sys
import uuid
import zipfile

sys.path.insert(0, os.path.dirname(__file__))
import content as C  # noqa: E402
import textures as T  # noqa: E402
from models import build_entity_models, LOOK_AT  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
BP = os.path.join(ROOT, "packs", "Xianxia_Cultivation_BP")
RP = os.path.join(ROOT, "packs", "Xianxia_Cultivation_RP")
DIST = os.path.join(ROOT, "dist")
SRC_SCRIPTS = os.path.join(ROOT, "src", "scripts")

VERSION = [1, 0, 0]
MIN_ENGINE = [1, 21, 90]
SERVER_VERSION = "2.0.0"
SERVER_UI_VERSION = "2.0.0"
ITEM_FORMAT = "1.21.90"
BLOCK_FORMAT = "1.21.90"
ENTITY_FORMAT = "1.21.0"
NS = C.NS


def uid(name):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, f"xianxia-cultivation/{name}"))


def write_json(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")


def hexc(c):
    return "#%02x%02x%02x" % tuple(c[:3])


def full(i):
    """Namespaced item id ("minecraft:..." ids pass through)."""
    return i if ":" in i else f"{NS}:{i}"


# ---------------------------------------------------------------------------
# Registries filled while building
# ---------------------------------------------------------------------------
item_textures = {}      # short name -> path
terrain_textures = {}   # short name -> path
lang = {}               # RP lang
names = {}              # item id (no ns) -> display name
icons = {}              # item id (no ns) -> texture path for UI


def item_tex(short, canvas):
    path = f"textures/items/{short}"
    canvas.save(os.path.join(RP, path + ".png"))
    item_textures[short] = {"textures": path}
    return path


def block_tex(short, canvas):
    path = f"textures/blocks/{short}"
    canvas.save(os.path.join(RP, path + ".png"))
    terrain_textures[short] = {"textures": path}
    return path


def item_json(iid, name, icon, components, category="items", group=None):
    desc = {"identifier": f"{NS}:{iid}", "menu_category": {"category": category}}
    if group:
        desc["menu_category"]["group"] = group
    comps = {
        "minecraft:icon": {"textures": {"default": icon}},
        "minecraft:display_name": {"value": name},
    }
    comps.update(components)
    write_json(os.path.join(BP, "items", f"{iid}.json"),
               {"format_version": ITEM_FORMAT, "minecraft:item": {"description": desc, "components": comps}})
    names[iid] = name


def simple_item(iid, name, canvas, stack=64, extra=None, category="items", group=None):
    short = f"{NS}_{iid}"
    path = item_tex(short, canvas)
    icons[iid] = path
    comps = {"minecraft:max_stack_size": stack}
    if extra:
        comps.update(extra)
    item_json(iid, name, short, comps, category, group)


def edible(nutrition=1, saturation=0.1, duration=0.8):
    return {
        "minecraft:food": {"nutrition": nutrition, "saturation_modifier": saturation, "can_always_eat": True},
        "minecraft:use_modifiers": {"use_duration": duration, "movement_modifier": 0.35},
        "minecraft:use_animation": "eat",
    }


# ---------------------------------------------------------------------------
# Manifests
# ---------------------------------------------------------------------------

def manifests():
    desc = ("Walk the path of immortality: spirit roots, eleven cultivation realms, heavenly tribulations, "
            "alchemy, artifact refining, formations, talismans, sects, spirit beasts and martial techniques.")
    write_json(os.path.join(BP, "manifest.json"), {
        "format_version": 2,
        "header": {"name": "Xianxia Cultivation (Behavior)", "description": desc, "uuid": uid("bp"),
                   "version": VERSION, "min_engine_version": MIN_ENGINE},
        "modules": [
            {"type": "data", "uuid": uid("bp-data"), "version": VERSION},
            {"type": "script", "language": "javascript", "uuid": uid("bp-script"), "version": VERSION,
             "entry": "scripts/main.js"},
        ],
        "dependencies": [
            {"uuid": uid("rp"), "version": VERSION},
            {"module_name": "@minecraft/server", "version": SERVER_VERSION},
            {"module_name": "@minecraft/server-ui", "version": SERVER_UI_VERSION},
        ],
        "metadata": {"authors": ["super-adventure"], "license": "MIT"},
    })
    write_json(os.path.join(RP, "manifest.json"), {
        "format_version": 2,
        "header": {"name": "Xianxia Cultivation (Resources)", "description": desc, "uuid": uid("rp"),
                   "version": VERSION, "min_engine_version": MIN_ENGINE},
        "modules": [{"type": "resources", "uuid": uid("rp-res"), "version": VERSION}],
        "dependencies": [{"uuid": uid("bp"), "version": VERSION}],
        "metadata": {"authors": ["super-adventure"], "license": "MIT"},
    })
    T.pack_icon(os.path.join(BP, "pack_icon.png"))
    T.pack_icon(os.path.join(RP, "pack_icon.png"), accent=(120, 220, 255))


# ---------------------------------------------------------------------------
# Items
# ---------------------------------------------------------------------------

def items():
    art = {"stone": T.gem, "jade": T.jade, "raw": T.raw_chunk, "ingot": T.ingot, "core": T.core,
           "scale": T.scale, "essence": T.essence, "tail": T.tail, "bone": T.bone, "fang": T.fang,
           "ink": T.ink, "residue": T.residue}
    for m in C.MATERIALS:
        iid, a = m["id"], m["art"]
        if a == "paper":
            canvas = T.paper(iid, m["color"])
        elif a in ("fruit", "peach"):
            canvas = T.fruit(iid, m["color"], peach=(a == "peach"))
        else:
            canvas = art[a](iid, m["color"])
        extra = {}
        if iid in C.EDIBLE:
            extra.update(edible(4, 0.6, 1.0))
        if iid in ("supreme_spirit_stone", "king_beast_core", "tribulation_essence", "immortal_peach"):
            extra["minecraft:glint"] = True
        simple_item(iid, m["name"], canvas, extra=extra)

    for p in C.PILLS:
        extra = edible(20 if p["id"] == "bigu_pill" else 1, 1.2 if p["id"] == "bigu_pill" else 0.1, 0.6)
        if p["level"] >= 5:
            extra["minecraft:glint"] = True
        simple_item(p["id"], p["name"], T.pill(p["id"], p["color"], p["level"]), extra=extra)

    for t in C.TALISMANS:
        simple_item(t["id"], t["name"], T.talisman(t["id"], t["rune"]), stack=16)

    for m in C.MANUALS:
        iid = f"manual_{m['id']}"
        el = C.ELEMENTS[m["element"]]["rgb"]
        extra = {"minecraft:glint": True} if m["grade"] in ("Earth", "Heaven") else {}
        simple_item(iid, f"Jade Slip: {m['name']}", T.jade_slip(iid, m["grade"], el), stack=1, extra=extra)

    for t in C.TECHNIQUES:
        iid = f"scroll_{t['id']}"
        el = C.ELEMENTS[t["element"]]["rgb"]
        simple_item(iid, f"Technique Scroll: {t['name']}", T.scroll(iid, el), stack=1)

    for s in C.SPECIAL_ITEMS:
        simple_item(s["id"], s["name"], T.special_icon(s["id"], s["art"]), stack=s["stack"],
                    extra={"minecraft:glint": True} if s["id"] == "dragon_summoning_pearl" else None)

    for w in C.WEAPONS:
        iid = w["id"]
        short = f"{NS}_{iid}"
        icons[iid] = item_tex(short, T.sword(iid, w["blade"], w["hilt"], w["gem"], w.get("style", "sword")))
        comps = {
            "minecraft:max_stack_size": 1,
            "minecraft:hand_equipped": True,
            "minecraft:damage": w["damage"],
            "minecraft:durability": {"max_durability": w["durability"]},
            "minecraft:enchantable": {"slot": "sword", "value": 15},
            "minecraft:repairable": {"repair_items": [
                {"items": [f"{NS}:profound_iron_ingot"], "repair_amount": "q.max_durability * 0.25"}]},
            "minecraft:tags": {"tags": ["minecraft:is_sword", "xian:spirit_weapon"]},
        }
        if iid == "heaven_severing_sword":
            comps["minecraft:glint"] = True
        item_json(iid, w["name"], short, comps, category="equipment", group="minecraft:itemGroup.name.sword")

    slots = [("helmet", "slot.armor.head", "armor_head"), ("chestplate", "slot.armor.chest", "armor_torso"),
             ("leggings", "slot.armor.legs", "armor_legs"), ("boots", "slot.armor.feet", "armor_feet")]
    for a in C.ARMOR_SETS:
        for idx, (piece, slot, ench) in enumerate(slots):
            iid = f"{a['id']}_{piece}"
            short = f"{NS}_{iid}"
            icons[iid] = item_tex(short, T.armor_icon(iid, piece, a["base"], a["trim"]))
            comps = {
                "minecraft:max_stack_size": 1,
                "minecraft:wearable": {"slot": slot, "protection": a["protection"][idx]},
                "minecraft:durability": {"max_durability": a["durability"]},
                "minecraft:enchantable": {"slot": ench, "value": 18},
                "minecraft:repairable": {"repair_items": [
                    {"items": [f"{NS}:spirit_jade"], "repair_amount": "q.max_durability * 0.25"}]},
                "minecraft:tags": {"tags": ["minecraft:is_armor", f"xian:{a['id']}_armor"]},
            }
            item_json(iid, a["pieces"][idx], short, comps, category="equipment")
        # model textures for the worn armour
        for layer in (1, 2):
            T.armor_layer(f"{a['id']}{layer}", a["base"], a["trim"], layer).save(
                os.path.join(RP, "textures", "models", "armor", f"{NS}_{a['id']}_{layer}.png"))
        for idx, (piece, slot, _) in enumerate(slots):
            iid = f"{a['id']}_{piece}"
            geo = {"helmet": "helmet", "chestplate": "chestplate", "leggings": "leggings", "boots": "boots"}[piece]
            var = {"helmet": "helmet_layer_visible", "chestplate": "chest_layer_visible",
                   "leggings": "leg_layer_visible", "boots": "boot_layer_visible"}[piece]
            write_json(os.path.join(RP, "attachables", f"{iid}.json"), {
                "format_version": "1.10.0",
                "minecraft:attachable": {"description": {
                    "identifier": f"{NS}:{iid}",
                    "materials": {"default": "armor", "enchanted": "armor_enchanted"},
                    "textures": {
                        "default": f"textures/models/armor/{NS}_{a['id']}_{2 if piece == 'leggings' else 1}",
                        "enchanted": "textures/misc/enchanted_actor_glint"},
                    "geometry": {"default": f"geometry.humanoid.armor.{geo}"},
                    "scripts": {"parent_setup": f"variable.{var} = 0.0;"},
                    "render_controllers": ["controller.render.armor"],
                }},
            })

    # herbs are the plantable items; their crop blocks are "<id>_plant"
    soils = ["minecraft:grass_block", "minecraft:dirt", "minecraft:farmland", "minecraft:podzol",
             "minecraft:coarse_dirt", "minecraft:rooted_dirt", "minecraft:moss_block", "minecraft:mycelium",
             "minecraft:sand", "minecraft:red_sand", "minecraft:mud", "minecraft:snow"]
    for h in C.HERBS:
        iid = h["id"]
        short = f"{NS}_{iid}"
        icons[iid] = item_tex(short, T.herb_icon(iid, h["style"], *h["colors"]))
        item_json(iid, h["name"], short, {
            "minecraft:max_stack_size": 64,
            "minecraft:block_placer": {"block": f"{NS}:{iid}_plant"},
        }, category="nature")
    return soils


# ---------------------------------------------------------------------------
# Blocks
# ---------------------------------------------------------------------------

def block_json(bid, components, states=None, permutations=None, category="construction"):
    desc = {"identifier": f"{NS}:{bid}", "menu_category": {"category": category}}
    if states:
        desc["states"] = states
    data = {"description": desc, "components": components}
    if permutations:
        data["permutations"] = permutations
    write_json(os.path.join(BP, "blocks", f"{bid}.json"), {"format_version": BLOCK_FORMAT, "minecraft:block": data})


def loot(path, pools):
    write_json(os.path.join(BP, "loot_tables", path), {"pools": pools})


def entry(item, count=(1, 1), weight=1):
    e = {"type": "item", "name": full(item), "weight": weight}
    if count != (1, 1):
        e["functions"] = [{"function": "set_count", "count": {"min": count[0], "max": count[1]}}]
    return e


def pool(entries, rolls=1, chance=None):
    p = {"rolls": rolls, "entries": entries}
    if chance is not None:
        p["conditions"] = [{"condition": "random_chance", "chance": chance}]
    return p


def geometries():
    """Custom block geometry."""
    def face(u, v, w, h):
        return {"uv": [u, v], "uv_size": [w, h]}

    cross = {
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {"identifier": "geometry.xian_cross", "texture_width": 16, "texture_height": 16},
            "bones": [{"name": "plant", "pivot": [0, 0, 0], "cubes": [
                {"origin": [-8, 0, 0], "size": [16, 16, 0], "pivot": [0, 0, 0], "rotation": [0, 45, 0],
                 "uv": {"north": face(0, 0, 16, 16), "south": face(0, 0, 16, 16)}},
                {"origin": [-8, 0, 0], "size": [16, 16, 0], "pivot": [0, 0, 0], "rotation": [0, -45, 0],
                 "uv": {"north": face(0, 0, 16, 16), "south": face(0, 0, 16, 16)}},
            ]}],
        }],
    }
    cushion = {
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {"identifier": "geometry.xian_cushion", "texture_width": 16, "texture_height": 16},
            "bones": [{"name": "cushion", "pivot": [0, 0, 0], "cubes": [
                {"origin": [-7, 0, -7], "size": [14, 4, 14], "uv": {
                    "north": face(1, 6, 14, 4), "south": face(1, 6, 14, 4), "east": face(1, 6, 14, 4),
                    "west": face(1, 6, 14, 4), "up": face(1, 1, 14, 14), "down": face(1, 1, 14, 14)}},
            ]}],
        }],
    }
    plate = {
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {"identifier": "geometry.xian_plate", "texture_width": 16, "texture_height": 16},
            "bones": [{"name": "plate", "pivot": [0, 0, 0], "cubes": [
                {"origin": [-8, 0, -8], "size": [16, 1, 16], "uv": {
                    "north": face(0, 15, 16, 1), "south": face(0, 15, 16, 1), "east": face(0, 15, 16, 1),
                    "west": face(0, 15, 16, 1), "up": face(0, 0, 16, 16), "down": face(0, 0, 16, 16)}},
            ]}],
        }],
    }
    for name, g in (("xian_cross", cross), ("xian_cushion", cushion), ("xian_plate", plate)):
        write_json(os.path.join(RP, "models", "blocks", f"{name}.geo.json"), g)


def blocks(soils):
    geometries()
    blocks_json = {"format_version": "1.21.40"}

    for o in C.ORES:
        bid = o["id"]
        short = f"{NS}_{bid}"
        block_tex(short, T.ore(bid, o["host"], o["gem"]))
        comps = {
            "minecraft:destructible_by_mining": {"seconds_to_destroy": 4 if o["host"] == "deepslate" else 3},
            "minecraft:destructible_by_explosion": {"explosion_resistance": 6},
            "minecraft:loot": f"loot_tables/blocks/{bid}.json",
            "minecraft:map_color": hexc(o["gem"]),
            "minecraft:material_instances": {"*": {"texture": short, "render_method": "opaque"}},
            "tag:stone": {},
            "tag:xian_ore": {},
        }
        if o["light"]:
            comps["minecraft:light_emission"] = o["light"]
        block_json(bid, comps, category="nature")
        pools = [pool([entry(o["drop"], o["count"])])]
        if o.get("bonus"):
            pools.append(pool([entry(o["bonus"])], chance=0.08))
        loot(f"blocks/{bid}.json", pools)
        lang[f"tile.{NS}:{bid}.name"] = o["name"]
        blocks_json[f"{NS}:{bid}"] = {"sound": "deepslate" if o["host"] == "deepslate" else "stone"}

    for b in C.FUNCTION_BLOCKS:
        bid, kind = b["id"], b["kind"]
        comps = {
            "minecraft:destructible_by_mining": {"seconds_to_destroy": 2},
            "minecraft:destructible_by_explosion": {"explosion_resistance": 30},
            "minecraft:loot": f"loot_tables/blocks/{bid}.json",
        }
        if b["light"]:
            comps["minecraft:light_emission"] = b["light"]
        if kind in ("furnace", "forge"):
            variant = "furnace" if kind == "furnace" else "forge"
            side, top = f"{NS}_{bid}_side", f"{NS}_{bid}_top"
            icons[bid] = block_tex(side, T.bronze(bid + "s", False, variant))
            block_tex(top, T.bronze(bid + "t", True, variant))
            comps["minecraft:material_instances"] = {"*": {"texture": side, "render_method": "opaque"},
                                                     "up": {"texture": top, "render_method": "opaque"}}
            comps[f"{NS}:station"] = {}
            comps["tag:metal"] = {}
            comps["minecraft:map_color"] = "#b06a2c" if kind == "furnace" else "#4a4a55"
            sound = "metal"
        elif kind == "cushion":
            tex = f"{NS}_{bid}"
            icons[bid] = block_tex(tex, T.cushion(bid))
            comps["minecraft:geometry"] = "geometry.xian_cushion"
            comps["minecraft:material_instances"] = {"*": {"texture": tex, "render_method": "opaque"}}
            comps["minecraft:collision_box"] = {"origin": [-7, 0, -7], "size": [14, 4, 14]}
            comps["minecraft:selection_box"] = {"origin": [-7, 0, -7], "size": [14, 4, 14]}
            comps["minecraft:light_dampening"] = 0
            comps[f"{NS}:station"] = {}
            comps["minecraft:map_color"] = "#dcaa32"
            sound = "cloth"
        elif kind == "array":
            top, side = f"{NS}_{bid}_top", f"{NS}_{bid}_side"
            icons[bid] = block_tex(top, T.array_top(bid, b["rune"]))
            block_tex(side, T.array_side(bid, b["rune"]))
            comps["minecraft:geometry"] = "geometry.xian_plate"
            comps["minecraft:material_instances"] = {
                "*": {"texture": side, "render_method": "opaque"},
                "up": {"texture": top, "render_method": "opaque", "face_dimming": False,
                       "ambient_occlusion": False}}
            comps["minecraft:collision_box"] = {"origin": [-8, 0, -8], "size": [16, 1, 16]}
            comps["minecraft:selection_box"] = {"origin": [-8, 0, -8], "size": [16, 1, 16]}
            comps["minecraft:light_dampening"] = 0
            comps[f"{NS}:station"] = {}
            comps["minecraft:map_color"] = hexc(b["rune"])
            sound = "stone"
        elif kind == "vein":
            tex = f"{NS}_{bid}"
            icons[bid] = block_tex(tex, T.vein(bid))
            comps["minecraft:material_instances"] = {"*": {"texture": tex, "render_method": "opaque"}}
            comps["minecraft:map_color"] = "#82ffe6"
            comps["tag:stone"] = {}
            comps["minecraft:destructible_by_mining"] = {"seconds_to_destroy": 6}
            sound = "amethyst_block"
        else:  # storage
            tex = f"{NS}_{bid}"
            icons[bid] = block_tex(tex, T.storage_block(bid, (110, 200, 255)))
            comps["minecraft:material_instances"] = {"*": {"texture": tex, "render_method": "opaque"}}
            comps["minecraft:map_color"] = "#6ec8ff"
            comps["tag:stone"] = {}
            sound = "amethyst_block"
        block_json(bid, comps)
        if kind == "vein":
            loot(f"blocks/{bid}.json", [pool([entry("low_spirit_stone", (3, 6))]),
                                         pool([entry("mid_spirit_stone", (1, 2))], chance=0.5),
                                         pool([entry("spirit_jade")], chance=0.25)])
        else:
            loot(f"blocks/{bid}.json", [pool([{"type": "item", "name": f"{NS}:{bid}", "weight": 1}])])
        lang[f"tile.{NS}:{bid}.name"] = b["name"]
        blocks_json[f"{NS}:{bid}"] = {"sound": sound}
        names[bid] = b["name"]

    # herb crops
    for h in C.HERBS:
        bid = f"{h['id']}_plant"
        texs = []
        for stage in range(3):
            short = f"{NS}_{bid}_{stage}"
            block_tex(short, T.herb_stage(h["id"], h["style"], *h["colors"], stage))
            texs.append(short)

        def mat(t):
            return {"*": {"texture": t, "render_method": "alpha_test", "ambient_occlusion": False,
                          "face_dimming": False}}

        comps = {
            "minecraft:geometry": "geometry.xian_cross",
            "minecraft:material_instances": mat(texs[0]),
            "minecraft:collision_box": False,
            "minecraft:selection_box": {"origin": [-6, 0, -6], "size": [12, 10, 12]},
            "minecraft:destructible_by_mining": {"seconds_to_destroy": 0},
            "minecraft:destructible_by_explosion": {"explosion_resistance": 0},
            "minecraft:light_dampening": 0,
            "minecraft:loot": f"loot_tables/blocks/{bid}.json",
            "minecraft:placement_filter": {"conditions": [{"allowed_faces": ["up"], "block_filter": soils}]},
            "minecraft:map_color": hexc(h["colors"][1]),
            f"{NS}:herb": {},
        }
        perms = [{"condition": f"q.block_state('{NS}:growth') == {s}",
                  "components": {"minecraft:material_instances": mat(texs[s])}} for s in (1, 2)]
        block_json(bid, comps, states={f"{NS}:growth": [0, 1, 2]}, permutations=perms, category="nature")
        loot(f"blocks/{bid}.json", [pool([entry(h["id"])])])
        lang[f"tile.{NS}:{bid}.name"] = h["name"]
        blocks_json[f"{NS}:{bid}"] = {"sound": "grass"}

    write_json(os.path.join(RP, "blocks.json"), blocks_json)


# ---------------------------------------------------------------------------
# World generation
# ---------------------------------------------------------------------------

def biome_filter(tags):
    return [{"any_of": [{"test": "has_biome_tag", "operator": "==", "value": t} for t in tags]}]


def worldgen():
    for o in C.ORES:
        fid = f"{NS}:{o['id']}_feature"
        rules = [{"places_block": f"{NS}:{o['id']}", "may_replace": o["replace"]}]
        write_json(os.path.join(BP, "features", f"{o['id']}_feature.json"), {
            "format_version": "1.13.0",
            "minecraft:ore_feature": {"description": {"identifier": fid}, "count": o["size"],
                                      "replace_rules": rules}})
        write_json(os.path.join(BP, "feature_rules", f"{o['id']}_rule.json"), {
            "format_version": "1.13.0",
            "minecraft:feature_rules": {
                "description": {"identifier": f"{NS}:{o['id']}_rule", "places_feature": fid},
                "conditions": {"placement_pass": "underground_pass",
                               "minecraft:biome_filter": [{"test": "has_biome_tag", "value": "overworld"}]},
                "distribution": {
                    "iterations": o["iterations"], "coordinate_eval_order": "zyx",
                    "x": {"distribution": "uniform", "extent": [0, 16]},
                    "y": {"distribution": "uniform", "extent": list(o["y"])},
                    "z": {"distribution": "uniform", "extent": [0, 16]}}}})

    # spirit veins: rare underground clusters
    write_json(os.path.join(BP, "features", "spirit_vein_feature.json"), {
        "format_version": "1.13.0",
        "minecraft:ore_feature": {"description": {"identifier": f"{NS}:spirit_vein_feature"}, "count": 5,
                                  "replace_rules": [{"places_block": f"{NS}:spirit_vein",
                                                     "may_replace": ["minecraft:stone", "minecraft:deepslate",
                                                                     "minecraft:tuff"]}]}})
    write_json(os.path.join(BP, "feature_rules", "spirit_vein_rule.json"), {
        "format_version": "1.13.0",
        "minecraft:feature_rules": {
            "description": {"identifier": f"{NS}:spirit_vein_rule", "places_feature": f"{NS}:spirit_vein_feature"},
            "conditions": {"placement_pass": "underground_pass",
                           "minecraft:biome_filter": [{"test": "has_biome_tag", "value": "overworld"}]},
            "distribution": {
                "iterations": 1, "scatter_chance": {"numerator": 1, "denominator": 6},
                "coordinate_eval_order": "zyx",
                "x": {"distribution": "uniform", "extent": [0, 16]},
                "y": {"distribution": "uniform", "extent": [-50, 30]},
                "z": {"distribution": "uniform", "extent": [0, 16]}}}})

    for h in C.HERBS:
        fid = f"{NS}:{h['id']}_feature"
        write_json(os.path.join(BP, "features", f"{h['id']}_feature.json"), {
            "format_version": "1.13.0",
            "minecraft:single_block_feature": {
                "description": {"identifier": fid},
                "places_block": {"name": f"{NS}:{h['id']}_plant", "states": {f"{NS}:growth": 2}},
                "enforce_placement_rules": False,
                "enforce_survivability_rules": True,
                "may_replace": ["minecraft:air"],
                "may_attach_to": {"bottom": ["minecraft:grass_block", "minecraft:dirt", "minecraft:podzol",
                                             "minecraft:sand", "minecraft:red_sand", "minecraft:snow",
                                             "minecraft:mycelium", "minecraft:moss_block", "minecraft:mud",
                                             "minecraft:coarse_dirt"]},
            }})
        write_json(os.path.join(BP, "feature_rules", f"{h['id']}_rule.json"), {
            "format_version": "1.13.0",
            "minecraft:feature_rules": {
                "description": {"identifier": f"{NS}:{h['id']}_rule", "places_feature": fid},
                "conditions": {"placement_pass": "surface_pass",
                               "minecraft:biome_filter": biome_filter(h["biomes"])},
                "distribution": {
                    "iterations": 1, "scatter_chance": {"numerator": 1, "denominator": h["rarity"]},
                    "x": {"distribution": "uniform", "extent": [0, 16]},
                    "y": "q.heightmap(v.worldx, v.worldz)",
                    "z": {"distribution": "uniform", "extent": [0, 16]}}}})


# ---------------------------------------------------------------------------
# Recipes
# ---------------------------------------------------------------------------

def recipes():
    rdir = os.path.join(BP, "recipes")
    for r in C.SHAPED:
        write_json(os.path.join(rdir, f"shaped_{r['id']}.json"), {
            "format_version": "1.20.10",
            "minecraft:recipe_shaped": {
                "description": {"identifier": f"{NS}:shaped_{r['id']}"},
                "tags": ["crafting_table"],
                "pattern": r["pattern"],
                "key": {k: {"item": full(v)} for k, v in r["key"].items()},
                "result": {"item": full(r["id"]), "count": r["count"]}}})

    def shapeless(ident, ingredients, result, count):
        counted = {}
        for i in ingredients:
            counted[full(i)] = counted.get(full(i), 0) + 1
        write_json(os.path.join(rdir, f"shapeless_{ident}.json"), {
            "format_version": "1.20.10",
            "minecraft:recipe_shapeless": {
                "description": {"identifier": f"{NS}:shapeless_{ident}"},
                "tags": ["crafting_table"],
                "ingredients": [{"item": k, "count": v} for k, v in counted.items()],
                "result": {"item": full(result), "count": count}}})

    for r in C.SHAPELESS:
        shapeless(r.get("tag", r["id"]), r["ingredients"], r["id"], r["count"])
    for t in C.TALISMANS:
        shapeless(t["id"], t["recipe"], t["id"], 2)
    for s in C.SMELTING:
        write_json(os.path.join(rdir, f"smelt_{s['output']}.json"), {
            "format_version": "1.20.10",
            "minecraft:recipe_furnace": {
                "description": {"identifier": f"{NS}:smelt_{s['output']}"},
                "tags": ["furnace", "blast_furnace"],
                "input": full(s["input"]), "output": full(s["output"])}})


# ---------------------------------------------------------------------------
# Entities
# ---------------------------------------------------------------------------

ENTITY_LOOT = {
    "spirit_fox": [pool([entry("fox_spirit_tail")], chance=0.6), pool([entry("low_beast_core")], chance=0.5),
                   pool([entry("spirit_fruit", (1, 2))], chance=0.4)],
    "demonic_wolf": [pool([entry("wolf_fang", (1, 2))]), pool([entry("low_beast_core")], chance=0.55),
                     pool([entry("mid_beast_core")], chance=0.06), pool([entry("blood_ginseng")], chance=0.1)],
    "flame_tiger": [pool([entry("tiger_bone", (1, 2))]), pool([entry("mid_beast_core")], chance=0.6),
                    pool([entry("low_beast_core", (1, 2))]), pool([entry("flame_lotus")], chance=0.3),
                    pool([entry("high_beast_core")], chance=0.05)],
    "flood_dragon": [pool([entry("dragon_scale", (4, 8))]), pool([entry("king_beast_core")]),
                     pool([entry("high_spirit_stone", (3, 6))]), pool([entry("tribulation_essence", (1, 2))]),
                     pool([entry("manual_nine_heavens_thunder", weight=3), entry("manual_profound_water", weight=4),
                           entry("manual_primordial_chaos", weight=1), entry("scroll_tidal_palm", weight=4)]),
                     pool([entry("immortal_peach")], chance=0.5)],
    "rogue_cultivator": [pool([entry("low_spirit_stone", (2, 6))]),
                         pool([entry("qi_gathering_pill", weight=4), entry("healing_pill", weight=4),
                               entry("qi_recovery_pill", weight=2), entry("fire_talisman", weight=2),
                               entry("protection_talisman", weight=1)], chance=0.6),
                         pool([entry(f"scroll_{t}") for t in ("qi_bolt", "wind_step", "fireball", "sword_qi",
                                                               "frost_spikes", "healing_spring", "vine_bind",
                                                               "tidal_palm", "thunder_palm", "earth_shield")],
                              chance=0.12),
                         pool([entry(f"manual_{m}") for m in ("five_elements", "azure_wood", "blazing_sun",
                                                               "profound_water", "golden_vajra", "great_earth")],
                              chance=0.06),
                         pool([entry("spirit_jade")], chance=0.15)],
    "demonic_cultivator": [pool([entry("demon_core")]), pool([entry("low_spirit_stone", (3, 8))]),
                           pool([entry("mid_spirit_stone")], chance=0.25),
                           pool([entry("blood_ginseng", (1, 2))], chance=0.4),
                           pool([entry("scroll_blood_sacrifice", weight=3), entry("manual_heavenly_devouring"),
                                 entry("scroll_phoenix_flame", weight=2), entry("scroll_void_step")], chance=0.1),
                           pool([entry("berserk_blood_pill"), entry("sealing_talisman")], chance=0.3)],
    "heart_demon": [pool([entry("clear_heart_pill")], chance=0.5), pool([entry("demon_core")])],
    "wandering_merchant": [],
}


def entity_behavior(e):
    eid = e["id"]
    model = e["model"]
    size = {"fox": (0.7, 0.8), "wolf": (0.9, 1.0), "tiger": (1.3, 1.4), "dragon": (2.6, 2.0),
            "humanoid": (0.6, 1.9)}[model]
    comps = {
        "minecraft:type_family": {"family": e["family"]},
        "minecraft:health": {"value": e["health"], "max": e["health"]},
        "minecraft:collision_box": {"width": size[0], "height": size[1]},
        "minecraft:movement": {"value": e["speed"]},
        "minecraft:movement.basic": {},
        "minecraft:jump.static": {},
        "minecraft:can_climb": {},
        "minecraft:physics": {},
        "minecraft:breathable": {"total_supply": 15, "suffocate_time": 0},
        "minecraft:nameable": {},
        "minecraft:loot": {"table": f"loot_tables/entities/{eid}.json"},
        "minecraft:experience_reward": {"on_death": f"query.last_hit_by_player ? {max(5, e['health'] // 4)} : 0"},
        "minecraft:navigation.walk": {"can_path_over_water": True, "avoid_water": True, "avoid_damage_blocks": True,
                                      "can_open_doors": model == "humanoid"},
        "minecraft:behavior.float": {"priority": 0},
        "minecraft:behavior.random_stroll": {"priority": 7, "speed_multiplier": 0.9},
        "minecraft:behavior.look_at_player": {"priority": 8, "look_distance": 8, "probability": 0.02},
        "minecraft:behavior.random_look_around": {"priority": 9},
        "minecraft:follow_range": {"value": 24, "max": 32},
    }
    groups = {}
    events = {}
    if e["hostile"]:
        comps["minecraft:attack"] = {"damage": e["damage"]}
        comps["minecraft:behavior.melee_attack"] = {"priority": 3, "speed_multiplier": 1.25, "track_target": True}
        comps["minecraft:behavior.hurt_by_target"] = {"priority": 1}
        targets = [{"filters": {"test": "is_family", "subject": "other", "value": "player"}, "max_dist": 24}]
        if eid in ("demonic_cultivator", "demonic_wolf"):
            targets.append({"filters": {"test": "is_family", "subject": "other", "value": "villager"},
                            "max_dist": 16})
        comps["minecraft:behavior.nearest_attackable_target"] = {
            "priority": 2, "must_see": True, "reselect_targets": True, "within_radius": 24,
            "entity_types": targets}
    if e.get("spawn"):
        comps["minecraft:despawn"] = {"despawn_from_distance": {}}
    if e.get("boss"):
        comps["minecraft:boss"] = {"hud_range": 64, "name": e["name"], "should_darken_sky": True}
        comps["minecraft:knockback_resistance"] = {"value": 1.0}
        comps["minecraft:fire_immune"] = {}
        comps["minecraft:navigation.generic"] = {"can_swim": True, "can_walk": True, "can_path_over_water": True,
                                                 "can_breach": True, "avoid_damage_blocks": True}
        del comps["minecraft:navigation.walk"]
        comps["minecraft:underwater_movement"] = {"value": 0.18}
        comps["minecraft:breathable"] = {"breathes_water": True, "breathes_air": True, "total_supply": 15,
                                         "suffocate_time": 0}
        comps["minecraft:damage_sensor"] = {"triggers": [{"cause": "fall", "deals_damage": "no"},
                                                         {"cause": "drowning", "deals_damage": "no"}]}
        comps["minecraft:behavior.melee_attack"]["reach_multiplier"] = 3.0
    if model == "tiger":
        comps["minecraft:fire_immune"] = {}
        comps["minecraft:behavior.leap_at_target"] = {"priority": 4, "yd": 0.45, "must_be_on_ground": True}
    if model == "wolf":
        comps["minecraft:behavior.leap_at_target"] = {"priority": 4, "yd": 0.4, "must_be_on_ground": True}
    if eid == "heart_demon":
        comps["minecraft:damage_sensor"] = {"triggers": [{"cause": "fall", "deals_damage": "no"}]}
        comps["minecraft:fire_immune"] = {}
    if eid == "wandering_merchant":
        comps["minecraft:interact"] = {"interactions": [{
            "on_interact": {"filters": {"test": "is_family", "subject": "other", "value": "player"},
                            "event": "xian:on_interact", "target": "self"},
            "interact_text": "action.interact.xian_trade", "swing": True}]}
        comps["minecraft:behavior.panic"] = {"priority": 1, "speed_multiplier": 1.4}
        comps["minecraft:behavior.avoid_mob_type"] = {"priority": 2, "entity_types": [
            {"filters": {"test": "is_family", "subject": "other", "value": "monster"}, "max_dist": 10,
             "walk_speed_multiplier": 1.0, "sprint_speed_multiplier": 1.3}]}
        comps["minecraft:timer"] = {"looping": False, "time": 1500, "time_down_event": {"event": "xian:leave"}}
        groups["xian:leaving"] = {"minecraft:instant_despawn": {}}
        events["xian:leave"] = {"add": {"component_groups": ["xian:leaving"]}}
        events["xian:on_interact"] = {}
    if e.get("tameable"):
        groups["xian:wild"] = {
            "minecraft:tameable": {"probability": 0.34, "tame_items": f"{NS}:spirit_fruit",
                                   "tame_event": {"event": "xian:on_tame", "target": "self"}},
            "minecraft:despawn": {"despawn_from_distance": {}},
            "minecraft:behavior.avoid_mob_type": {"priority": 3, "entity_types": [
                {"filters": {"test": "is_family", "subject": "other", "value": "monster"}, "max_dist": 8,
                 "walk_speed_multiplier": 1.1, "sprint_speed_multiplier": 1.4}]},
        }
        groups["xian:tamed"] = {
            "minecraft:is_tamed": {},
            "minecraft:health": {"value": 50, "max": 50},
            "minecraft:attack": {"damage": 6},
            "minecraft:behavior.melee_attack": {"priority": 3, "speed_multiplier": 1.3, "track_target": True},
            "minecraft:behavior.follow_owner": {"priority": 5, "speed_multiplier": 1.2, "start_distance": 10,
                                                "stop_distance": 2},
            "minecraft:behavior.owner_hurt_by_target": {"priority": 1},
            "minecraft:behavior.owner_hurt_target": {"priority": 2},
            "minecraft:sittable": {},
            "minecraft:behavior.stay_while_sitting": {"priority": 3},
            "minecraft:healable": {"items": [{"item": f"{NS}:spirit_fruit", "heal_amount": 15}]},
            "minecraft:leashable": {},
        }
        if "minecraft:despawn" in comps:
            del comps["minecraft:despawn"]
        events["minecraft:entity_spawned"] = {"add": {"component_groups": ["xian:wild"]}}
        events["xian:on_tame"] = {"remove": {"component_groups": ["xian:wild"]},
                                  "add": {"component_groups": ["xian:tamed"]}}
    data = {
        "description": {"identifier": f"{NS}:{eid}", "is_spawnable": True, "is_summonable": True,
                        "is_experimental": False},
        "components": comps,
    }
    if groups:
        data["component_groups"] = groups
    if events:
        data["events"] = events
    write_json(os.path.join(BP, "entities", f"{eid}.json"),
               {"format_version": ENTITY_FORMAT, "minecraft:entity": data})


def spawn_rule(e):
    s = e.get("spawn")
    if not s:
        return
    cond = {
        "minecraft:spawns_on_surface": {},
        "minecraft:weight": {"default": s["weight"]},
        "minecraft:herd": {"min_size": s["herd"][0], "max_size": s["herd"][1]},
        "minecraft:biome_filter": biome_filter(s["biomes"]),
        "minecraft:density_limit": {"surface": 2 if not e["hostile"] else 4},
    }
    if s.get("night"):
        cond["minecraft:brightness_filter"] = {"min": 0, "max": 7, "adjust_for_weather": True}
        cond["minecraft:difficulty_filter"] = {"min": "easy", "max": "hard"}
    elif s.get("day"):
        cond["minecraft:brightness_filter"] = {"min": 9, "max": 15, "adjust_for_weather": False}
    if e["hostile"]:
        cond.setdefault("minecraft:difficulty_filter", {"min": "easy", "max": "hard"})
    write_json(os.path.join(BP, "spawn_rules", f"{e['id']}.json"), {
        "format_version": "1.8.0",
        "minecraft:spawn_rules": {
            "description": {"identifier": f"{NS}:{e['id']}",
                            "population_control": "monster" if e["hostile"] else "animal"},
            "conditions": [cond]}})


def entities():
    models = build_entity_models()
    write_json(os.path.join(RP, "render_controllers", "xian.render_controllers.json"), {
        "format_version": "1.8.0",
        "render_controllers": {"controller.render.xian_default": {
            "geometry": "Geometry.default", "materials": [{"*": "Material.default"}],
            "textures": ["Texture.default"]}}})
    write_json(os.path.join(RP, "animations", "xian_common.animation.json"),
               {"format_version": "1.8.0", "animations": LOOK_AT})
    for e in C.ENTITIES:
        eid = e["id"]
        entity_behavior(e)
        spawn_rule(e)
        loot(f"entities/{eid}.json", ENTITY_LOOT[eid])
        model, anims = models[eid]
        write_json(os.path.join(RP, "models", "entity", f"{eid}.geo.json"), model.geometry())
        model.texture().save(os.path.join(RP, "textures", "entity", NS, f"{eid}.png"))
        write_json(os.path.join(RP, "animations", f"{eid}.animation.json"),
                   {"format_version": "1.8.0", "animations": anims})
        anim_map = {"look": "animation.xian.look_at_target"}
        animate = ["look"]
        for key in anims:
            short = key.rsplit(".", 1)[1]
            anim_map[short] = key
            if short == "walk":
                animate.append({"walk": "query.modified_move_speed"})
            elif short == "attack":
                animate.append({"attack": "variable.attack_time > 0.0"})
            else:
                animate.append(short)
        scale = {"flood_dragon": "1.4", "flame_tiger": "1.1"}.get(eid, "1.0")
        write_json(os.path.join(RP, "entity", f"{eid}.entity.json"), {
            "format_version": "1.10.0",
            "minecraft:client_entity": {"description": {
                "identifier": f"{NS}:{eid}",
                "materials": {"default": "entity_alphatest"},
                "textures": {"default": f"textures/entity/{NS}/{eid}"},
                "geometry": {"default": model.ident},
                "animations": anim_map,
                "scripts": {"animate": animate, "scale": scale},
                "render_controllers": ["controller.render.xian_default"],
                "spawn_egg": {"base_color": hexc(e["egg"][0]), "overlay_color": hexc(e["egg"][1])},
            }}})
        lang[f"entity.{NS}:{eid}.name"] = e["name"]
        lang[f"item.spawn_egg.entity.{NS}:{eid}.name"] = f"Spawn {e['name']}"


# ---------------------------------------------------------------------------
# Particles
# ---------------------------------------------------------------------------

PARTICLES = {
    # name: (sprite index, size, lifetime, motion, uses colour variable)
    "orb": (0, 0.22, 0.6, "still", True),
    "aura": (0, 0.12, 1.6, "rise", True),
    "spark": (1, 0.18, 0.5, "burst", True),
    "flame": (2, 0.25, 0.6, "rise", False),
    "frost": (3, 0.2, 1.0, "fall", False),
    "leaf": (4, 0.15, 1.2, "fall", False),
    "sword": (5, 0.6, 0.35, "still", True),
    "lightning": (6, 0.3, 0.3, "burst", False),
    "rune": (7, 0.6, 1.2, "spin", True),
}
FIXED_COLORS = {"flame": (1.0, 0.55, 0.15), "frost": (0.7, 0.95, 1.0), "leaf": (0.4, 0.95, 0.4),
                "lightning": (0.8, 0.7, 1.0)}


def particles():
    T.particle_atlas().save(os.path.join(RP, "textures", "particle", f"{NS}_particles.png"))
    for name, (idx, size, life, motion, colored) in PARTICLES.items():
        u = (idx % 8) * 8
        v = (idx // 8) * 8
        if colored:
            color = ["variable.color.r", "variable.color.g", "variable.color.b",
                     "1 - variable.particle_age / variable.particle_lifetime"]
        else:
            r, g, b = FIXED_COLORS[name]
            color = [r, g, b, "1 - variable.particle_age / variable.particle_lifetime"]
        comps = {
            "minecraft:emitter_rate_instant": {"num_particles": 1 if motion != "burst" else 4},
            "minecraft:emitter_lifetime_once": {"active_time": 0.05},
            "minecraft:emitter_shape_sphere": {"radius": 0.1 if motion != "burst" else 0.3,
                                               "direction": "outwards"},
            "minecraft:particle_lifetime_expression": {"max_lifetime": life},
            "minecraft:particle_initial_speed": {"still": 0, "rise": 0.4, "fall": 0.3, "burst": 2.5,
                                                 "spin": 0}[motion],
            "minecraft:particle_motion_dynamic": {
                "linear_acceleration": {"still": [0, 0, 0], "rise": [0, 1.2, 0], "fall": [0, -1.5, 0],
                                        "burst": [0, -2, 0], "spin": [0, 0.2, 0]}[motion],
                "linear_drag_coefficient": 1.5},
            "minecraft:particle_appearance_billboard": {
                "size": [f"{size} * (1 - variable.particle_age / variable.particle_lifetime * 0.6)",
                         f"{size} * (1 - variable.particle_age / variable.particle_lifetime * 0.6)"],
                "facing_camera_mode": "rotate_xyz",
                "uv": {"texture_width": 64, "texture_height": 64, "uv": [u, v], "uv_size": [8, 8]}},
            "minecraft:particle_appearance_tinting": {"color": color},
        }
        if motion == "spin":
            comps["minecraft:particle_initial_spin"] = {"rotation": 0, "rotation_rate": 180}
        write_json(os.path.join(RP, "particles", f"{name}.particle.json"), {
            "format_version": "1.10.0",
            "particle_effect": {
                "description": {"identifier": f"{NS}:{name}",
                                "basic_render_parameters": {"material": "particles_blend",
                                                            "texture": f"textures/particle/{NS}_particles"}},
                "components": comps}})


# ---------------------------------------------------------------------------
# Texts, atlases, scripts, packaging
# ---------------------------------------------------------------------------

def texts():
    lang["action.interact.xian_trade"] = "Trade"
    lang["pack.name"] = "Xianxia Cultivation"
    lang["pack.description"] = "Cultivate to immortality."
    for root in (RP, BP):
        d = os.path.join(root, "texts")
        os.makedirs(d, exist_ok=True)
        with open(os.path.join(d, "en_US.lang"), "w", encoding="utf-8") as f:
            for k, v in sorted(lang.items()):
                f.write(f"{k}={v}\n")
        write_json(os.path.join(d, "languages.json"), ["en_US"])
    write_json(os.path.join(RP, "textures", "item_texture.json"),
               {"resource_pack_name": "xianxia", "texture_name": "atlas.items", "texture_data": item_textures})
    write_json(os.path.join(RP, "textures", "terrain_texture.json"),
               {"resource_pack_name": "xianxia", "texture_name": "atlas.terrain", "padding": 8, "num_mip_levels": 4,
                "texture_data": terrain_textures})


def strip(d, keys):
    return {k: v for k, v in d.items() if k in keys}


def scripts():
    dst = os.path.join(BP, "scripts")
    os.makedirs(dst, exist_ok=True)
    for fn in sorted(os.listdir(SRC_SCRIPTS)):
        if fn.endswith(".js"):
            shutil.copy(os.path.join(SRC_SCRIPTS, fn), os.path.join(dst, fn))
    data = {
        "REALMS": C.REALMS,
        "BODY_TIERS": C.BODY_TIERS,
        "ELEMENTS": C.ELEMENTS,
        "ROOT_GRADES": C.ROOT_GRADES,
        "MANUALS": C.MANUALS,
        "GRADE_COLORS": C.GRADE_COLORS,
        "TECHNIQUES": C.TECHNIQUES,
        "PILLS": C.PILLS,
        "BREAKTHROUGH_PILLS": C.BREAKTHROUGH_PILLS,
        "TALISMANS": [strip(t, ("id", "name", "desc")) for t in C.TALISMANS],
        "WEAPONS": [strip(w, ("id", "name", "effect", "flying")) for w in C.WEAPONS],
        "HERBS": [strip(h, ("id", "name")) for h in C.HERBS],
        "REFINING": C.REFINING,
        "SECTS": C.SECTS,
        "SECT_RANKS": C.SECT_RANKS,
        "SECT_MISSIONS": C.SECT_MISSIONS,
        "SECT_TREASURY": C.SECT_TREASURY,
        "MERCHANT_STOCK": [dict(item=i, count=n, price=p) for i, n, p in C.MERCHANT_STOCK],
        "SELL_PRICES": C.SELL_PRICES,
        "SPECIAL_ITEMS": [strip(s, ("id", "name", "desc")) for s in C.SPECIAL_ITEMS],
        "NAMES": names,
        "ICONS": icons,
    }
    with open(os.path.join(dst, "data.js"), "w", encoding="utf-8") as f:
        f.write("// AUTO-GENERATED by tools/build.py from tools/content.py - do not edit by hand.\n")
        for k, v in data.items():
            f.write(f"export const {k} = {json.dumps(v, ensure_ascii=False)};\n")


def zipdir(zf, folder, arc_prefix):
    for base, _, files in os.walk(folder):
        for fn in sorted(files):
            p = os.path.join(base, fn)
            zf.write(p, os.path.join(arc_prefix, os.path.relpath(p, folder)))


def package():
    os.makedirs(DIST, exist_ok=True)
    for f in os.listdir(DIST):
        os.remove(os.path.join(DIST, f))
    with zipfile.ZipFile(os.path.join(DIST, "Xianxia_Cultivation.mcaddon"), "w", zipfile.ZIP_DEFLATED) as z:
        zipdir(z, BP, "Xianxia_Cultivation_BP")
        zipdir(z, RP, "Xianxia_Cultivation_RP")
    for folder, name in ((BP, "Xianxia_Cultivation_BP.mcpack"), (RP, "Xianxia_Cultivation_RP.mcpack")):
        with zipfile.ZipFile(os.path.join(DIST, name), "w", zipfile.ZIP_DEFLATED) as z:
            zipdir(z, folder, "")


# ---------------------------------------------------------------------------
# Generated content reference (docs/CONTENT.md)
# ---------------------------------------------------------------------------

def nm(i):
    return names.get(i, i.replace("minecraft:", "").replace("_", " ").title())


def ing(d):
    return ", ".join(f"{n}x {nm(k)}" for k, n in d.items())


def content_docs():
    L = ["# Xianxia Cultivation - Content Reference", "",
         "_Auto-generated by `tools/build.py` from `tools/content.py`._", ""]
    L += ["## Cultivation realms", "", "| # | Realm | Stages | Exp / stage | Meditation exp/s | Max qi | Breakthrough | Tribulation waves |",
          "|---|---|---|---|---|---|---|---|"]
    for k, r in enumerate(C.REALMS):
        L.append(f"| {k} | {r['name']} | {len(r['stages'])} | {r['exp']:,} | {r['gain']} | {r['qi']:,} | {r['chance']}% | {r['tribulation'] or '-'} |")
    L += ["", "## Body cultivation", "", "| Tier | Body exp |", "|---|---|"]
    L += [f"| {b['name']} | {b['exp']:,} |" for b in C.BODY_TIERS]
    L += ["", "## Spirit roots", "", "| Grade | Multiplier |", "|---|---|"]
    L += [f"| {g['name']} | x{g['mult']} |" for g in C.ROOT_GRADES.values()]
    L += ["", "## Cultivation manuals", "", "| Manual | Grade | Element | Speed | Notes |", "|---|---|---|---|---|"]
    L += [f"| {m['name']} | {m['grade']} | {C.ELEMENTS[m['element']]['name']} | x{m['mult']} | {m['desc']} |" for m in C.MANUALS]
    L += ["", "## Martial techniques", "", "| Technique | Element | Min. realm | Qi | Cooldown | Description |", "|---|---|---|---|---|---|"]
    L += [f"| {t['name']} | {C.ELEMENTS[t['element']]['name']} | {C.REALMS[t['realm']]['name']} | {t['cost']} | {t['cd'] / 20:g}s | {t['desc']} |" for t in C.TECHNIQUES]
    L += ["", "## Pills (Alchemy Furnace)", "", "| Pill | Level | Base chance | Ingredients | Effect |", "|---|---|---|---|---|"]
    L += [f"| {p['name']} | {p['level']} | {p['chance']}% | {ing(p['ingredients'])} | {p['desc']} |" for p in C.PILLS]
    L += ["", "## Artifact refining (Refining Forge)", "", "| Result | Level | Base chance | Ingredients |", "|---|---|---|---|"]
    L += [f"| {nm(r['out'])} | {r['level']} | {r['chance']}% | {ing(r['ingredients'])} |" for r in C.REFINING]
    L += ["", "## Talismans (crafting table, shapeless, makes 2)", "", "| Talisman | Recipe | Effect |", "|---|---|---|"]
    L += [f"| {t['name']} | {', '.join(nm(full(x)[5:] if full(x).startswith('xian:') else x) for x in t['recipe'])} | {t['desc']} |" for t in C.TALISMANS]
    L += ["", "## Crafting table recipes", ""]
    for r in C.SHAPED:
        key = ", ".join(f"`{k}` = {nm(v)}" for k, v in r["key"].items())
        grid = " / ".join(f"`{row}`" for row in r["pattern"])
        L.append(f"- **{nm(r['id'])}** x{r['count']}: {grid} ({key})")
    for r in C.SHAPELESS:
        L.append(f"- **{nm(r['id'])}** x{r['count']}: shapeless {', '.join(nm(i) for i in r['ingredients'])}")
    L.append("- **Profound Iron Ingot**: also smelt Raw Profound Iron in a furnace")
    L += ["", "## Herbs", "", "| Herb | Grows wild in |", "|---|---|"]
    L += [f"| {h['name']} | {', '.join(h['biomes'])} |" for h in C.HERBS]
    L += ["", "## Ores & world generation", "", "| Block | Y range | Drops |", "|---|---|---|"]
    L += [f"| {o['name']} | {o['y'][0]} to {o['y'][1]} | {o['count'][0]}-{o['count'][1]}x {nm(o['drop'])} |" for o in C.ORES]
    L.append("| Spirit Vein | -50 to 30 (rare) | spirit stones, spirit jade; doubles meditation speed nearby |")
    L += ["", "## Creatures", "", "| Creature | Health | Hostile | Spawns in |", "|---|---|---|---|"]
    for e in C.ENTITIES:
        sp = ", ".join(e["spawn"]["biomes"]) if e.get("spawn") else "summoned"
        L.append(f"| {e['name']} | {e['health']} | {'yes' if e['hostile'] else 'no'} | {sp} |")
    L += ["", "## Sects", "", "| Sect | Alignment | Perk |", "|---|---|---|"]
    L += [f"| {s['name']} | {s['alignment']} | {s['perk']} |" for s in C.SECTS]
    L += ["", "Ranks: " + ", ".join(f"{r['name']} ({r['contribution']})" for r in C.SECT_RANKS), ""]
    L += ["## Wandering merchant stock", "", "| Item | Price (low-grade spirit stones) |", "|---|---|"]
    L += [f"| {nm(i)}{' x' + str(n) if n > 1 else ''} | {p} |" for i, n, p in C.MERCHANT_STOCK]
    os.makedirs(os.path.join(ROOT, "docs"), exist_ok=True)
    with open(os.path.join(ROOT, "docs", "CONTENT.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(L) + "\n")


def main():
    for d in (BP, RP):
        if os.path.exists(d):
            shutil.rmtree(d)
        os.makedirs(d)
    for sub in ("textures/items", "textures/blocks", "textures/entity/xian", "textures/particle",
                "textures/models/armor"):
        os.makedirs(os.path.join(RP, sub), exist_ok=True)
    manifests()
    soils = items()
    blocks(soils)
    worldgen()
    recipes()
    entities()
    particles()
    texts()
    scripts()
    content_docs()
    package()
    n_items = len(os.listdir(os.path.join(BP, "items")))
    n_blocks = len(os.listdir(os.path.join(BP, "blocks")))
    n_ent = len(os.listdir(os.path.join(BP, "entities")))
    print(f"Built: {n_items} items, {n_blocks} blocks, {n_ent} entities -> {DIST}")


if __name__ == "__main__":
    main()
