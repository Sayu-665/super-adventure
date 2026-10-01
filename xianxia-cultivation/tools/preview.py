#!/usr/bin/env python3
"""Renders preview images of the generated entity models and item/block textures.

Usage: python3 tools/preview.py   (after build.py)  -> docs/previews/*.png
This is a tiny software rasteriser for sanity-checking geometry without the game.
"""
import json
import math
import os
from PIL import Image, ImageDraw

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
RP = os.path.join(ROOT, "packs", "Xianxia_Cultivation_RP")
OUT = os.path.join(ROOT, "docs", "previews")


def rot_matrix(rx, ry, rz):
    rx, ry, rz = (math.radians(a) for a in (rx, ry, rz))
    cx, sx, cy, sy, cz, sz = math.cos(rx), math.sin(rx), math.cos(ry), math.sin(ry), math.cos(rz), math.sin(rz)
    X = [[1, 0, 0], [0, cx, -sx], [0, sx, cx]]
    Y = [[cy, 0, sy], [0, 1, 0], [-sy, 0, cy]]
    Z = [[cz, -sz, 0], [sz, cz, 0], [0, 0, 1]]

    def mm(a, b):
        return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
    return mm(mm(Z, Y), X)


def apply(m, v):
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def make_transform(rot, pivot):
    if not rot or not any(rot):
        return lambda p: p
    # Bedrock entity geometry: X and Y rotations are inverted relative to a right-handed system
    m = rot_matrix(-rot[0], -rot[1], rot[2])

    def f(p):
        d = [p[i] - pivot[i] for i in range(3)]
        r = apply(m, d)
        return [r[i] + pivot[i] for i in range(3)]
    return f


FACE_CORNERS = {
    # corners ordered: top-left, top-right, bottom-right, bottom-left (as seen on the texture)
    "north": lambda o, s: [(o[0] + s[0], o[1] + s[1], o[2]), (o[0], o[1] + s[1], o[2]), (o[0], o[1], o[2]),
                           (o[0] + s[0], o[1], o[2])],
    "south": lambda o, s: [(o[0], o[1] + s[1], o[2] + s[2]), (o[0] + s[0], o[1] + s[1], o[2] + s[2]),
                           (o[0] + s[0], o[1], o[2] + s[2]), (o[0], o[1], o[2] + s[2])],
    "east": lambda o, s: [(o[0], o[1] + s[1], o[2]), (o[0], o[1] + s[1], o[2] + s[2]), (o[0], o[1], o[2] + s[2]),
                          (o[0], o[1], o[2])],
    "west": lambda o, s: [(o[0] + s[0], o[1] + s[1], o[2] + s[2]), (o[0] + s[0], o[1] + s[1], o[2]),
                          (o[0] + s[0], o[1], o[2]), (o[0] + s[0], o[1], o[2] + s[2])],
    "up": lambda o, s: [(o[0] + s[0], o[1] + s[1], o[2] + s[2]), (o[0], o[1] + s[1], o[2] + s[2]),
                        (o[0], o[1] + s[1], o[2]), (o[0] + s[0], o[1] + s[1], o[2])],
    "down": lambda o, s: [(o[0] + s[0], o[1], o[2]), (o[0], o[1], o[2]), (o[0], o[1], o[2] + s[2]),
                          (o[0] + s[0], o[1], o[2] + s[2])],
}


def render(geo_path, tex_path, out_path, yaw=-35, pitch=25, size=360):
    geo = json.load(open(geo_path))["minecraft:geometry"][0]
    tex = Image.open(tex_path).convert("RGBA")
    tw, th = tex.size
    bones = {b["name"]: b for b in geo["bones"]}

    def chain(b):
        fs = []
        while b:
            fs.append(make_transform(b.get("rotation"), b["pivot"]))
            b = bones.get(b.get("parent"))
        return fs

    quads = []
    for b in geo["bones"]:
        tfs = chain(b)
        for c in b.get("cubes", []):
            o, s = c["origin"], c["size"]
            ctf = make_transform(c.get("rotation"), c.get("pivot", [0, 0, 0]))
            for face, uvd in c["uv"].items():
                u, v = uvd["uv"]
                w, h = uvd["uv_size"]
                if w < 0:
                    u, w = u + w, -w
                if h < 0:
                    v, h = v + h, -h
                corners = FACE_CORNERS[face](o, s)
                pts = []
                for p in corners:
                    p = ctf(list(p))
                    for f in tfs:
                        p = f(p)
                    pts.append(p)
                quads.append((pts, (int(u), int(v), int(w), int(h))))

    view = rot_matrix(pitch, yaw, 0)
    tris = []
    for pts, (u, v, w, h) in quads:
        vp = [apply(view, p) for p in pts]
        # subdivide into texels
        for ty in range(h):
            for tx in range(w):
                col = tex.getpixel((min(tw - 1, u + tx), min(th - 1, v + ty)))
                if col[3] < 10:
                    continue

                def lerp(a, b, t):
                    return [a[i] + (b[i] - a[i]) * t for i in range(3)]

                def at(fx, fy):
                    top = lerp(vp[0], vp[1], fx)
                    bot = lerp(vp[3], vp[2], fx)
                    return lerp(top, bot, fy)
                c4 = [at(tx / w, ty / h), at((tx + 1) / w, ty / h), at((tx + 1) / w, (ty + 1) / h),
                      at(tx / w, (ty + 1) / h)]
                depth = sum(p[2] for p in c4) / 4
                tris.append((depth, c4, col))
    if not tris:
        return
    xs = [p[0] for _, c4, _ in tris for p in c4]
    ys = [p[1] for _, c4, _ in tris for p in c4]
    span = max(max(xs) - min(xs), max(ys) - min(ys)) or 1
    scale = (size * 0.85) / span
    cx, cy = (max(xs) + min(xs)) / 2, (max(ys) + min(ys)) / 2
    img = Image.new("RGBA", (size, size), (52, 56, 70, 255))
    d = ImageDraw.Draw(img)
    tris.sort(key=lambda t: -t[0])
    for _, c4, col in tris:
        poly = [(size / 2 + (p[0] - cx) * scale, size / 2 - (p[1] - cy) * scale) for p in c4]
        d.polygon(poly, fill=col)
    img.save(out_path)


def sheet(folder, out, cols=16, scale=3):
    fs = sorted(f for f in os.listdir(folder) if f.endswith(".png"))
    rows = (len(fs) + cols - 1) // cols
    cell = 16 * scale + 6
    img = Image.new("RGBA", (cols * cell, rows * cell), (52, 56, 70, 255))
    for k, f in enumerate(fs):
        t = Image.open(os.path.join(folder, f)).convert("RGBA").resize((16 * scale, 16 * scale), Image.NEAREST)
        img.alpha_composite(t, ((k % cols) * cell + 3, (k // cols) * cell + 3))
    img.save(out)


def main():
    os.makedirs(OUT, exist_ok=True)
    ents = sorted(f[:-9] for f in os.listdir(os.path.join(RP, "models", "entity")) if f.endswith(".geo.json"))
    tiles = []
    for e in ents:
        p = os.path.join(OUT, f"entity_{e}.png")
        render(os.path.join(RP, "models", "entity", f"{e}.geo.json"),
               os.path.join(RP, "textures", "entity", "xian", f"{e}.png"), p, yaw=150, pitch=20)
        tiles.append(p)
    if tiles:
        grid = Image.new("RGBA", (360 * 4, 360 * ((len(tiles) + 3) // 4)), (52, 56, 70, 255))
        for k, p in enumerate(tiles):
            grid.alpha_composite(Image.open(p), ((k % 4) * 360, (k // 4) * 360))
            os.remove(p)
        grid.save(os.path.join(OUT, "entities.png"))
    sheet(os.path.join(RP, "textures", "items"), os.path.join(OUT, "items.png"))
    sheet(os.path.join(RP, "textures", "blocks"), os.path.join(OUT, "blocks.png"), cols=12)
    print("previews written to", OUT)


if __name__ == "__main__":
    main()
