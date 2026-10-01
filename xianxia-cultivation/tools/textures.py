"""Procedural pixel-art generator for every texture in the add-on.

All art is generated from code (no third-party assets), so the pack can be
redistributed freely. Each painter is deterministic: the random generator is
seeded with the texture name.
"""
import math
import random
from PIL import Image, ImageDraw, ImageFilter

TRANSPARENT = (0, 0, 0, 0)


def clamp(v):
    return max(0, min(255, int(round(v))))


def shade(c, f):
    """Multiply an RGB(A) colour by f (f>1 lightens toward white)."""
    if f >= 1:
        r, g, b = (ch + (255 - ch) * (f - 1) for ch in c[:3])
    else:
        r, g, b = (ch * f for ch in c[:3])
    return (clamp(r), clamp(g), clamp(b), c[3] if len(c) > 3 else 255)


def mix(a, b, t):
    return tuple(clamp(a[i] * (1 - t) + b[i] * t) for i in range(3)) + (255,)


def rgba(c, a=255):
    return (c[0], c[1], c[2], a)


class Canvas:
    def __init__(self, w=16, h=16, seed="x"):
        self.w, self.h = w, h
        self.img = Image.new("RGBA", (w, h), TRANSPARENT)
        self.px = self.img.load()
        self.rng = random.Random(seed)

    def set(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[x, y] = rgba(c, c[3] if len(c) > 3 else 255)

    def get(self, x, y):
        if 0 <= x < self.w and 0 <= y < self.h:
            return self.px[x, y]
        return TRANSPARENT

    def filled(self, x, y):
        return self.get(x, y)[3] > 0

    def fill(self, c):
        for y in range(self.h):
            for x in range(self.w):
                self.set(x, y, c)

    def noise_fill(self, c, amount=0.12):
        for y in range(self.h):
            for x in range(self.w):
                self.set(x, y, shade(c, 1 + self.rng.uniform(-amount, amount)))

    def rect(self, x0, y0, x1, y1, c):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                self.set(x, y, c)

    def line(self, x0, y0, x1, y1, c):
        n = max(abs(x1 - x0), abs(y1 - y0), 1)
        for i in range(n + 1):
            t = i / n
            self.set(int(round(x0 + (x1 - x0) * t)), int(round(y0 + (y1 - y0) * t)), c)

    def disc(self, cx, cy, r, base, light=(-0.6, -0.7), gloss=True):
        """Shaded sphere."""
        for y in range(self.h):
            for x in range(self.w):
                dx, dy = x + 0.5 - cx, y + 0.5 - cy
                d = math.hypot(dx, dy)
                if d <= r:
                    nx, ny = dx / r, dy / r
                    lit = -(nx * light[0] + ny * light[1])
                    f = 0.75 + 0.45 * lit - 0.25 * (d / r) ** 3
                    self.set(x, y, shade(base, f))
        if gloss:
            self.set(int(cx - r * 0.45), int(cy - r * 0.45), (255, 255, 255))
            self.set(int(cx - r * 0.45) + 1, int(cy - r * 0.45), shade(base, 1.6))

    def outline(self, c=(30, 20, 30), only_outer=True):
        src = [[self.filled(x, y) for x in range(self.w)] for y in range(self.h)]
        for y in range(self.h):
            for x in range(self.w):
                if src[y][x]:
                    continue
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < self.w and 0 <= ny < self.h and src[ny][nx]:
                        self.set(x, y, c)
                        break

    def darken_edges(self, f=0.7):
        """Darken filled pixels next to transparency (inner outline)."""
        src = [[self.filled(x, y) for x in range(self.w)] for y in range(self.h)]
        for y in range(self.h):
            for x in range(self.w):
                if not src[y][x]:
                    continue
                edge = False
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if not (0 <= nx < self.w and 0 <= ny < self.h) or not src[ny][nx]:
                        edge = True
                if edge:
                    self.set(x, y, shade(self.get(x, y), f))

    def save(self, path):
        self.img.save(path)


# ---------------------------------------------------------------------------
# Item painters
# ---------------------------------------------------------------------------

def pill(name, color, level=1):
    c = Canvas(seed=name)
    c.disc(8, 8.5, 5.2, rgba(color))
    # swirling dao pattern
    for i in range(6):
        a = i / 6 * math.pi * 2 + 0.6
        x = 8 + math.cos(a) * 2.6
        y = 8.5 + math.sin(a) * 2.6
        c.set(int(x), int(y), shade(color, 1.25))
    if level >= 4:  # pill pattern rings for high grade pills
        for a in range(0, 360, 20):
            r = math.radians(a)
            c.set(int(8 + math.cos(r) * 4.2), int(8.5 + math.sin(r) * 4.2), (255, 230, 120))
    c.outline((40, 25, 30))
    if level >= 3:  # spiritual glow pixels
        for (x, y) in ((2, 3), (13, 4), (12, 14), (3, 13)):
            c.set(x, y, shade(color, 1.5)[:3] + (180,))
    return c


def gem(name, color):
    c = Canvas(seed=name)
    pts = {}
    # hexagonal crystal
    rows = [(5, 10), (4, 11), (3, 12), (3, 12), (3, 12), (3, 12), (4, 11), (5, 10), (6, 9)]
    for i, (a, b) in enumerate(rows):
        y = 3 + i
        for x in range(a, b + 1):
            f = 1.0
            if x < 6:
                f = 1.25
            elif x > 9:
                f = 0.72
            if i < 2:
                f *= 1.2
            if i > 5:
                f *= 0.8
            c.set(x, y, shade(color, f))
    c.rect(7, 4, 8, 4, (255, 255, 255))
    c.set(5, 6, shade(color, 1.6))
    c.line(7, 3, 7, 11, shade(color, 1.1))
    c.outline((20, 30, 50))
    c.set(13, 2, shade(color, 1.6)[:3] + (200,))
    c.set(2, 12, shade(color, 1.6)[:3] + (160,))
    return c


def jade(name, color):
    c = Canvas(seed=name)
    for y in range(16):
        for x in range(16):
            dx, dy = (x + 0.5 - 8) / 5.5, (y + 0.5 - 8.5) / 4.6
            if dx * dx + dy * dy <= 1:
                n = c.rng.uniform(-0.08, 0.08)
                f = 1.15 - 0.35 * (dx + dy) / 2 + n
                c.set(x, y, shade(color, f))
    for x in range(5, 11):
        c.set(x, 8, shade(color, 1.35))
    c.set(5, 6, (255, 255, 255))
    c.outline((20, 50, 35))
    return c


def raw_chunk(name, color):
    c = Canvas(seed=name)
    for y in range(4, 13):
        for x in range(3, 13):
            if c.rng.random() < 0.85 or (5 < x < 11 and 5 < y < 11):
                c.set(x, y, shade(color, c.rng.uniform(0.75, 1.25)))
    c.darken_edges(0.7)
    c.outline((20, 20, 30))
    return c


def ingot(name, color):
    c = Canvas(seed=name)
    for i, y in enumerate(range(6, 12)):
        for x in range(3 + (1 if i == 0 else 0), 13 - (1 if i == 0 else 0)):
            f = 1.3 if i < 2 else (1.0 if i < 4 else 0.75)
            c.set(x, y, shade(color, f))
    c.line(5, 6, 10, 6, shade(color, 1.6))
    c.outline((20, 20, 30))
    return c


def core(name, color):
    c = Canvas(seed=name)
    c.disc(8, 8, 5.5, rgba(color))
    c.disc(8, 8, 2.3, shade(color, 1.6), gloss=False)
    c.set(8, 8, (255, 255, 255))
    for (x, y) in ((8, 1), (8, 15), (1, 8), (15, 8)):
        c.set(x, y, shade(color, 1.4)[:3] + (170,))
    c.outline((30, 10, 10))
    return c


def scale(name, color):
    c = Canvas(seed=name)
    for row in range(3):
        for col in range(3):
            cx = 4 + col * 4 + (2 if row % 2 else 0)
            cy = 5 + row * 3
            for y in range(cy - 2, cy + 2):
                for x in range(cx - 2, cx + 2):
                    if 2 < x < 14:
                        f = 1.2 - (y - cy + 2) * 0.12
                        c.set(x, y, shade(color, f))
            c.set(cx - 1, cy - 2, shade(color, 1.5))
    c.outline((10, 40, 50))
    return c


def essence(name, color):
    c = Canvas(seed=name)
    c.disc(8, 8, 6, rgba(shade(color, 0.6)), gloss=False)
    bolt = [(9, 2), (8, 3), (7, 4), (7, 5), (6, 6), (7, 7), (8, 7), (9, 7), (8, 8), (7, 9), (7, 10), (6, 11),
            (6, 12), (7, 13)]
    for (x, y) in bolt:
        c.set(x, y, (255, 255, 230))
        c.set(x + 1, y, shade(color, 1.5))
    c.outline((30, 10, 50))
    return c


def tail(name, color):
    c = Canvas(seed=name)
    for i in range(13):
        t = i / 12
        cx = 3 + t * 9 + math.sin(t * 3) * 1.5
        cy = 13 - t * 10
        r = 1.2 + math.sin(t * math.pi) * 2.3
        for y in range(16):
            for x in range(16):
                if math.hypot(x + 0.5 - cx, y + 0.5 - cy) <= r:
                    tip = t > 0.78
                    base = (120, 200, 255) if tip else color
                    c.set(x, y, shade(base, c.rng.uniform(0.9, 1.1)))
    c.outline((40, 40, 70))
    return c


def bone(name, color):
    c = Canvas(seed=name)
    c.line(4, 12, 12, 4, rgba(color))
    c.line(4, 11, 11, 4, rgba(color))
    for (x, y) in ((3, 11), (3, 13), (5, 13), (11, 3), (13, 3), (13, 5)):
        c.disc(x, y, 1.4, rgba(color), gloss=False)
    c.outline((60, 50, 40))
    return c


def fang(name, color):
    c = Canvas(seed=name)
    for y in range(3, 14):
        w = max(0, int((14 - y) / 3))
        for x in range(8 - w, 9 + w // 2 + 1):
            c.set(x, y, shade(color, 1.1 - (y - 3) * 0.03))
    c.outline((60, 50, 40))
    return c


def paper(name, color, rune=None, blank=True):
    c = Canvas(seed=name)
    c.rect(5, 1, 10, 14, rgba(color))
    for y in range(1, 15):
        c.set(10, y, shade(color, 0.8))
    if rune:
        rc = rgba(rune)
        ink = (190, 20, 20)
        c.rect(5, 1, 10, 1, ink)
        c.rect(5, 14, 10, 14, ink)
        c.line(6, 3, 9, 3, rc)
        c.line(7, 4, 7, 8, rc)
        c.line(8, 4, 8, 8, rc)
        c.line(6, 6, 9, 6, rc)
        c.disc(7.6, 10.5, 1.9, rc, gloss=False)
        c.set(7, 10, shade(rune, 1.5))
        c.line(6, 13, 9, 12, ink)
    elif blank:
        for y in (3, 6, 9, 12):
            c.set(7, y, shade(color, 0.9))
    c.outline((90, 60, 20))
    return c


def ink(name, color):
    c = Canvas(seed=name)
    c.rect(7, 2, 8, 4, (180, 200, 210))
    c.rect(6, 1, 9, 1, (120, 80, 50))
    c.disc(7.5, 10, 4.5, (200, 220, 230), gloss=False)
    c.disc(7.5, 10.5, 3.6, rgba(color))
    c.outline((30, 30, 40))
    return c


def residue(name, color):
    c = Canvas(seed=name)
    for _ in range(40):
        x, y = int(c.rng.gauss(8, 2.2)), int(c.rng.gauss(10, 1.6))
        c.set(x, y, shade(color, c.rng.uniform(0.7, 1.3)))
    c.outline((20, 15, 10))
    return c


def fruit(name, color, peach=False):
    c = Canvas(seed=name)
    c.disc(8, 9.5, 5, rgba(color))
    if peach:
        c.line(8, 5, 6, 12, shade(color, 0.8))
        for y in range(7, 13):
            c.set(11, y, shade(color, 0.85)[:3] + (255,))
    c.rect(8, 3, 8, 4, (90, 60, 30))
    c.rect(9, 2, 11, 3, (80, 180, 70))
    c.outline((60, 20, 30))
    return c


def herb_icon(name, style, stem, bloom):
    c = Canvas(seed=name)
    paint_plant(c, style, stem, bloom, stage=2, icon=True)
    c.outline((20, 35, 20))
    return c


def paint_plant(c, style, stem, bloom, stage=2, icon=False):
    """Paint a herb at growth stage 0..2 on a 16x16 canvas (bottom-anchored)."""
    stem, bloom = rgba(stem), rgba(bloom)
    h = [5, 9, 13][stage]
    base_y = 15
    if style in ("grass", "fern"):
        blades = 5 if stage else 3
        for i in range(blades):
            x0 = 3 + i * (10 // max(1, blades - 1))
            top = base_y - h + c.rng.randint(0, 2)
            lean = c.rng.choice((-2, -1, 1, 2))
            c.line(x0, base_y, x0 + lean, top, shade(stem, c.rng.uniform(0.85, 1.15)))
            if style == "fern":
                for y in range(top + 1, base_y - 1, 2):
                    t = (y - top) / max(1, base_y - top)
                    xx = x0 + int(lean * (1 - t))
                    c.set(xx - 1, y, shade(stem, 1.2))
                    c.set(xx + 1, y, shade(stem, 1.2))
            if stage == 2:
                c.set(x0 + lean, top, bloom)
                if style == "fern":
                    c.set(x0 + lean, top + 1, shade(bloom, 1.3))
    elif style in ("flower", "lotus", "berry"):
        cx = 8
        c.line(cx, base_y, cx, base_y - h + 3, stem)
        for i in range(1 + stage):
            y = base_y - 2 - i * 3
            c.set(cx - 1, y, shade(stem, 1.2))
            c.set(cx - 2, y - 1, shade(stem, 1.1))
            c.set(cx + 1, y - 1, shade(stem, 1.2))
            c.set(cx + 2, y - 2, shade(stem, 1.0))
        if style == "lotus":
            # wide leaves at the bottom
            c.rect(3, 14, 12, 14, shade(stem, 1.0))
            c.rect(4, 13, 11, 13, shade(stem, 1.2))
        if stage >= 1:
            top = base_y - h + 1
            r = 1.6 if stage == 1 else 2.8
            if style == "berry":
                for (dx, dy) in ((-2, 0), (1, -1), (0, 2), (2, 2), (-1, -2)):
                    c.disc(cx + dx + 0.5, top + 2 + dy + 0.5, 1.2 if stage == 2 else 0.8, bloom, gloss=False)
            else:
                for a in range(0, 360, 60 if style == "flower" else 40):
                    rr = math.radians(a)
                    px_ = cx + 0.5 + math.cos(rr) * r
                    py_ = top + 2 + math.sin(rr) * r * (0.6 if style == "lotus" else 1)
                    c.set(int(px_), int(py_), shade(bloom, c.rng.uniform(0.9, 1.15)))
                c.set(cx, top + 2, (255, 240, 140))
                if style == "lotus" and stage == 2:
                    c.rect(cx - 1, top, cx + 1, top + 2, bloom)
                    c.set(cx, top - 1, shade(bloom, 1.3))
    elif style == "mushroom":
        cx = 8
        c.rect(cx - 1, base_y - h + 3, cx + 1, base_y, stem)
        r = [2, 4, 6][stage]
        top = base_y - h + 2
        for y in range(top - 2, top + 2):
            for x in range(cx - r, cx + r + 1):
                if abs(x - cx) <= r - (top - y if y < top else 0):
                    c.set(x, y, shade(bloom, 1.1 if y < top else 0.85))
        if stage == 2:
            for (x, y) in ((cx - 3, top - 1), (cx + 2, top - 2), (cx, top)):
                c.set(x, y, (240, 230, 255))


def talisman(name, rune):
    return paper(name, (235, 205, 95), rune=rune)


GRADE_SLIP = {"Mortal": (190, 170, 120), "Yellow": (230, 200, 80), "Profound": (90, 200, 220),
              "Earth": (220, 150, 60), "Heaven": (210, 120, 255)}


def jade_slip(name, grade, element_rgb):
    """Cultivation manual: a fanned-open book of bound jade slips."""
    c = Canvas(seed=name)
    col = GRADE_SLIP[grade]
    for i in range(6):
        x = 2 + i * 2
        top = 3 + abs(i - 2.5) * 0.6
        for y in range(int(top), 14 - int(abs(i - 2.5) * 0.4)):
            c.set(x, y, shade(col, 1.2 - 0.04 * i + c.rng.uniform(-0.05, 0.05)))
            c.set(x + 1, y, shade(col, 0.9 - 0.04 * i))
        c.set(x, int(top), shade(col, 1.4))
    # binding cords
    for x in range(2, 14):
        if c.filled(x, 6):
            c.set(x, 6, (150, 40, 30))
        if c.filled(x, 11):
            c.set(x, 11, (150, 40, 30))
    # characters
    for i in range(6):
        x = 2 + i * 2
        for y in (8, 9) if i % 2 else (8,):
            c.set(x, y, shade(col, 0.45))
    er = tuple(int(v * 255) for v in element_rgb)
    c.disc(12.5, 3.5, 1.8, rgba(er), gloss=False)
    c.outline((40, 30, 20))
    return c


def scroll(name, element_rgb):
    c = Canvas(seed=name)
    er = tuple(int(v * 255) for v in element_rgb)
    paper_c = (240, 225, 180)
    for i in range(10):
        x, y = 3 + i, 12 - i
        c.line(x - 1, y - 1, x + 2, y + 2, paper_c)
    c.line(2, 10, 5, 13, (130, 70, 30))
    c.line(10, 2, 13, 5, (130, 70, 30))
    c.line(2, 10, 1, 9, (90, 50, 20))
    c.line(13, 5, 14, 6, (90, 50, 20))
    c.disc(8, 8, 1.7, rgba(er), gloss=False)
    c.line(5, 9, 7, 7, shade(paper_c, 0.75))
    c.outline((50, 35, 20))
    return c


def sword(name, blade, hilt, gem_c, style="sword"):
    c = Canvas(seed=name)
    if style == "spear":
        c.line(2, 14, 11, 5, rgba(hilt))
        c.line(3, 14, 12, 5, shade(hilt, 0.8))
        tip = [(12, 4), (13, 3), (14, 2), (13, 2), (14, 3), (12, 3), (11, 4), (13, 4), (12, 2), (14, 1)]
        for (x, y) in tip:
            c.set(x, y, rgba(blade))
        c.set(13, 2, shade(blade, 1.5))
        c.set(10, 6, rgba(gem_c))
        c.set(11, 7, rgba(gem_c))
        c.outline((25, 20, 30))
        return c
    # blade
    for i in range(10):
        x, y = 5 + i, 10 - i
        c.set(x, y, shade(blade, 1.3))
        c.set(x + 1, y, rgba(blade))
        c.set(x, y + 1, shade(blade, 0.75))
        if style == "saber" and 3 < i < 9:
            c.set(x + 1, y + 1, shade(blade, 0.9))
    c.set(15, 0, shade(blade, 1.4))
    # guard
    c.line(2, 9, 6, 13, rgba(gem_c))
    c.set(4, 11, shade(gem_c, 1.5))
    # grip
    c.line(1, 14, 3, 12, rgba(hilt))
    c.set(0, 15, shade(gem_c, 0.9))
    c.outline((25, 20, 30))
    return c


def armor_icon(name, piece, base, trim):
    c = Canvas(seed=name)
    b, t = rgba(base), rgba(trim)
    if piece == "helmet":  # jade crown / guan
        c.rect(4, 8, 11, 11, b)
        c.rect(6, 5, 9, 8, t)
        c.rect(7, 3, 8, 5, b)
        c.line(2, 6, 13, 6, (200, 180, 120))
        c.set(7, 9, (90, 220, 150))
    elif piece == "chestplate":  # robe
        c.rect(3, 2, 12, 14, b)
        c.rect(1, 3, 3, 9, b)
        c.rect(12, 3, 14, 9, b)
        c.line(5, 2, 8, 7, t)
        c.line(10, 2, 8, 7, t)
        c.rect(3, 8, 12, 9, t)
        c.rect(7, 8, 8, 9, (200, 60, 60))
    elif piece == "leggings":
        c.rect(4, 2, 11, 4, t)
        c.rect(4, 5, 7, 14, b)
        c.rect(8, 5, 11, 14, b)
        c.line(7, 5, 7, 14, shade(base, 0.7))
    else:  # boots
        c.rect(2, 7, 6, 12, b)
        c.rect(9, 7, 13, 12, b)
        c.rect(1, 12, 6, 13, t)
        c.rect(9, 12, 14, 13, t)
    for y in range(16):
        for x in range(16):
            p = c.get(x, y)
            if p[3]:
                c.set(x, y, shade(p, 1 + c.rng.uniform(-0.06, 0.06)))
    c.darken_edges(0.8)
    c.outline((25, 25, 35))
    return c


def special_icon(name, art):
    c = Canvas(seed=name)
    if art == "codex":
        c.rect(3, 2, 12, 14, (120, 30, 30))
        c.rect(4, 3, 12, 13, (150, 40, 40))
        c.rect(12, 3, 13, 14, (235, 225, 200))
        c.disc(8, 8, 3.2, (245, 245, 245), gloss=False)
        for y in range(16):
            for x in range(16):
                dx, dy = x + 0.5 - 8, y + 0.5 - 8
                if math.hypot(dx, dy) <= 3.2 and dx > 0:
                    c.set(x, y, (20, 20, 20))
        c.set(8, 6, (20, 20, 20))
        c.set(7, 9, (245, 245, 245))
        c.rect(3, 2, 3, 14, (230, 190, 60))
    elif art == "seal":
        c.rect(3, 7, 12, 14, (80, 200, 140))
        c.rect(4, 8, 11, 13, (110, 230, 170))
        c.rect(6, 3, 9, 7, (60, 170, 120))
        c.disc(7.5, 3, 2, (90, 220, 160), gloss=False)
        c.rect(6, 10, 9, 11, (200, 30, 30))
    elif art == "orb":
        c.disc(8, 9, 5.5, (200, 230, 255, 255))
        for i, col in enumerate(((255, 80, 80), (255, 200, 60), (80, 220, 80), (80, 150, 255), (200, 120, 255))):
            a = i / 5 * math.pi * 2
            c.set(int(8 + math.cos(a) * 2.5), int(9 + math.sin(a) * 2.5), col)
        c.rect(5, 14, 10, 15, (120, 90, 60))
    elif art == "pearl":
        c.disc(8, 8, 5.5, (90, 200, 255, 255))
        c.disc(8, 8, 2.2, (230, 250, 255, 255), gloss=False)
        for a in range(0, 360, 45):
            r = math.radians(a)
            c.set(int(8 + math.cos(r) * 7), int(8 + math.sin(r) * 7), (150, 230, 255, 150))
    elif art == "horn":
        for i in range(12):
            t = i / 11
            cx, cy = 3 + t * 10, 12 - math.sin(t * math.pi) * 6
            r = 0.8 + t * 1.9
            for y in range(16):
                for x in range(16):
                    if math.hypot(x + 0.5 - cx, y + 0.5 - cy) <= r:
                        c.set(x, y, shade((200, 170, 120), 0.8 + t * 0.4))
        c.set(4, 11, (60, 40, 30))
    elif art == "token":
        c.disc(8, 8, 6, (230, 190, 60, 255))
        c.disc(8, 8, 3.5, (200, 150, 40, 255), gloss=False)
        c.rect(7, 5, 8, 11, (140, 40, 30))
        c.rect(5, 7, 10, 8, (140, 40, 30))
    elif art == "ring":
        for a in range(0, 360, 10):
            r = math.radians(a)
            for rr in (4, 4.8):
                c.set(int(8 + math.cos(r) * rr), int(10 + math.sin(r) * rr * 0.7), (230, 190, 70))
        c.disc(8, 5, 2.3, (120, 255, 200, 255))
    c.outline((30, 25, 30))
    return c


# ---------------------------------------------------------------------------
# Block painters
# ---------------------------------------------------------------------------

def stone_base(name, deep=False):
    c = Canvas(seed=name)
    base = (78, 78, 84) if deep else (125, 125, 125)
    c.noise_fill(base, 0.1)
    for _ in range(14):
        x, y = c.rng.randrange(16), c.rng.randrange(16)
        c.set(x, y, shade(base, 0.75))
        c.set(x + 1, y, shade(base, 0.85))
    if deep:
        for y in (3, 8, 13):
            for x in range(16):
                if c.rng.random() < 0.6:
                    c.set(x, y, shade(base, 0.8))
    return c


def ore(name, host, gem_c):
    c = stone_base(name, host == "deepslate")
    clusters = [(4, 4), (10, 6), (6, 11), (12, 12)]
    for (cx, cy) in clusters:
        for (dx, dy) in ((0, 0), (1, 0), (0, 1), (1, 1), (-1, 0), (0, -1)):
            if c.rng.random() < 0.85:
                f = 1.3 if (dx, dy) == (0, 0) else c.rng.uniform(0.75, 1.05)
                c.set(cx + dx, cy + dy, shade(gem_c, f))
        c.set(cx, cy - 1, shade(gem_c, 1.6))
    return c


def storage_block(name, gem_c):
    c = Canvas(seed=name)
    for y in range(16):
        for x in range(16):
            tx, ty = x % 8, y % 8
            f = 1.0
            if tx == 0 or ty == 0:
                f = 0.55
            elif tx + ty < 5:
                f = 1.3
            elif tx + ty > 10:
                f = 0.8
            c.set(x, y, shade(gem_c, f * c.rng.uniform(0.95, 1.05)))
    return c


def vein(name):
    c = stone_base(name, True)
    pts = [(0, 3), (3, 5), (6, 4), (9, 7), (12, 6), (15, 9)]
    pts2 = [(2, 15), (4, 12), (7, 11), (9, 7), (11, 3), (13, 0)]
    for path in (pts, pts2):
        for (a, b) in zip(path, path[1:]):
            c.line(a[0], a[1], b[0], b[1], (130, 255, 230))
    for _ in range(10):
        x, y = c.rng.randrange(16), c.rng.randrange(16)
        c.set(x, y, (200, 255, 255))
    return c


def bronze(name, top=False, side_variant="furnace"):
    c = Canvas(seed=name)
    base = (150, 95, 45) if side_variant == "furnace" else (70, 70, 80)
    c.noise_fill(base, 0.08)
    if top:
        if side_variant == "furnace":
            c.disc(8, 8, 6, (60, 35, 20, 255), gloss=False)
            c.disc(8, 8, 4, (255, 140, 40, 255), gloss=False)
            c.disc(8, 8, 2, (255, 230, 120, 255), gloss=False)
        else:
            c.rect(2, 2, 13, 13, (40, 40, 48))
            c.rect(3, 3, 12, 12, (110, 110, 125))
            c.rect(4, 7, 11, 8, (60, 60, 70))
        return c
    # side: bands and a flame window / runes
    for x in range(16):
        c.set(x, 1, shade(base, 1.3))
        c.set(x, 14, shade(base, 0.6))
    if side_variant == "furnace":
        c.rect(5, 6, 10, 11, (40, 20, 10))
        c.rect(6, 8, 9, 11, (255, 120, 30))
        c.rect(7, 9, 8, 11, (255, 220, 100))
        for x in (2, 13):
            c.rect(x, 3, x, 12, shade(base, 1.25))
        for (x, y) in ((3, 3), (12, 3)):
            c.set(x, y, (90, 220, 150))
    else:
        c.rect(3, 4, 12, 11, (40, 40, 48))
        c.line(4, 5, 11, 10, (255, 150, 60))
        c.line(4, 10, 11, 5, (255, 150, 60))
        c.rect(7, 7, 8, 8, (255, 230, 140))
    return c


def cushion(name):
    c = Canvas(seed=name)
    base = (220, 170, 50)
    for y in range(16):
        for x in range(16):
            d = math.hypot(x + 0.5 - 8, y + 0.5 - 8)
            f = 1.1 - d / 20
            c.set(x, y, shade(base, f * c.rng.uniform(0.95, 1.05)))
    for a in range(0, 360, 15):
        r = math.radians(a)
        c.set(int(8 + math.cos(r) * 5.5), int(8 + math.sin(r) * 5.5), (180, 40, 40))
    c.disc(8, 8, 1.5, (180, 40, 40, 255), gloss=False)
    return c


def array_top(name, rune):
    c = Canvas(seed=name)
    c.noise_fill((60, 60, 70), 0.08)
    rr = rgba(rune)
    for a in range(0, 360, 6):
        r = math.radians(a)
        c.set(int(8 + math.cos(r) * 7), int(8 + math.sin(r) * 7), rr)
    for a in range(0, 360, 10):
        r = math.radians(a)
        c.set(int(8 + math.cos(r) * 4.3), int(8 + math.sin(r) * 4.3), shade(rune, 0.9))
    for k in range(8):  # trigram spokes
        r = math.radians(k * 45)
        for d in (5.2, 6.0):
            c.set(int(8 + math.cos(r) * d), int(8 + math.sin(r) * d), shade(rune, 1.2))
    c.disc(8, 8, 1.8, rgba(shade(rune, 1.4)), gloss=False)
    return c


def array_side(name, rune):
    c = Canvas(seed=name)
    c.noise_fill((60, 60, 70), 0.08)
    for x in range(0, 16, 3):
        c.set(x, 15, rgba(rune))
    return c


def herb_stage(name, style, stem, bloom, stage):
    c = Canvas(seed=f"{name}{stage}")
    paint_plant(c, style, stem, bloom, stage=stage)
    return c


# ---------------------------------------------------------------------------
# Armor model textures (64x32 classic humanoid armour layout)
# ---------------------------------------------------------------------------

def armor_layer(name, base, trim, layer):
    c = Canvas(64, 32, seed=name)
    b, t = rgba(base), rgba(trim)

    def box(u, v, w, h, d, col, trim_rows=()):
        # classic box UV: top/bottom then sides row
        for x in range(u + d, u + d + w * 2):
            for y in range(v, v + d):
                c.set(x, y, shade(col, c.rng.uniform(0.9, 1.05)))
        for x in range(u, u + 2 * (w + d)):
            for y in range(v + d, v + d + h):
                f = c.rng.uniform(0.9, 1.05)
                col2 = col
                if (y - v - d) in trim_rows:
                    col2 = t
                c.set(x, y, shade(col2, f))

    if layer == 1:
        # helmet: thin crown band around the head (only upper rows visible)
        for x in range(0, 32):
            for y in range(8, 16):
                if y in (8, 9):
                    c.set(x, y, t)
        for x in range(8, 16):
            for y in range(0, 8):
                if 2 <= x - 8 <= 5 and 2 <= y <= 5:
                    c.set(x, y, b)
        box(16, 16, 8, 12, 4, base, trim_rows=(0, 7, 8, 11))      # body
        box(40, 16, 4, 12, 4, base, trim_rows=(10, 11))            # arms
        box(0, 16, 4, 12, 4, base, trim_rows=(11,))                # boots (legs region, lower part)
        # leave only the lower legs for boots
        for x in range(0, 16):
            for y in range(20, 26):
                c.set(x, y, TRANSPARENT)
    else:
        box(0, 16, 4, 12, 4, base, trim_rows=(0,))                 # legs
        box(16, 16, 8, 12, 4, base, trim_rows=(0, 1))              # waist
        for x in range(16, 40):
            for y in range(24, 32):
                c.set(x, y, TRANSPARENT)
    return c


# ---------------------------------------------------------------------------
# Particles atlas (64x64, eight 8x8 sprites in the first row, white so they tint)
# ---------------------------------------------------------------------------

def particle_atlas():
    c = Canvas(64, 64, seed="particles")
    W = (255, 255, 255)

    def sprite(i, fn):
        ox = (i % 8) * 8
        oy = (i // 8) * 8
        for y in range(8):
            for x in range(8):
                a = fn(x + 0.5 - 4, y + 0.5 - 4)
                if a > 0:
                    c.set(ox + x, oy + y, W[:3] + (clamp(a * 255),))

    sprite(0, lambda x, y: max(0.0, 1 - math.hypot(x, y) / 4) ** 1.2)                     # soft orb
    sprite(1, lambda x, y: 1.0 if (abs(x) < 0.6 or abs(y) < 0.6) and math.hypot(x, y) < 4
           else (0.6 if abs(abs(x) - abs(y)) < 0.6 and math.hypot(x, y) < 2.5 else 0))     # sparkle
    sprite(2, lambda x, y: max(0.0, 1 - math.hypot(x * 1.3, (y - 1) * 0.8) / 3.5))          # flame
    sprite(3, lambda x, y: 1.0 if (abs(x) < 0.6 or abs(y) < 0.6 or abs(abs(x) - abs(y)) < 0.7)
           and math.hypot(x, y) < 3.8 else 0)                                               # snowflake
    sprite(4, lambda x, y: 1.0 if (x / 3.5) ** 2 + (y / 2) ** 2 < 1 else 0)                 # leaf
    sprite(5, lambda x, y: 1.0 if 2.4 < math.hypot(x, y + 2) < 3.8 and y < 1 else 0)        # crescent
    sprite(6, lambda x, y: 1.0 if abs(x - (0.8 if y < 0 else -0.8) - y * 0.3) < 0.8 else 0)  # spark
    sprite(7, lambda x, y: 1.0 if 2.6 < math.hypot(x, y) < 3.6 or (abs(x) < 0.6 and abs(y) < 2.5)
           or (abs(y) < 0.6 and abs(x) < 2.5) else 0)                                       # rune
    return c


# ---------------------------------------------------------------------------
# Pack icon
# ---------------------------------------------------------------------------

def pack_icon(path, accent=(230, 190, 70)):
    S = 256
    img = Image.new("RGBA", (S, S))
    d = ImageDraw.Draw(img)
    for y in range(S):
        t = y / S
        col = (int(20 + 30 * t), int(18 + 10 * t), int(50 + 40 * t), 255)
        d.line([(0, y), (S, y)], fill=col)
    # mountains
    rng = random.Random("icon")
    for layer, col in enumerate(((40, 50, 90), (30, 38, 70), (22, 28, 52))):
        pts = [(0, S)]
        x = 0
        while x <= S:
            pts.append((x, int(S * (0.62 + 0.1 * layer) - rng.randint(0, 50 - layer * 10))))
            x += 32
        pts.append((S, S))
        d.polygon(pts, fill=col + (255,))
    # taiji
    cx, cy, r = 128, 104, 70
    d.ellipse([cx - r - 6, cy - r - 6, cx + r + 6, cy + r + 6], fill=accent + (255,))
    d.pieslice([cx - r, cy - r, cx + r, cy + r], 90, 270, fill=(245, 245, 240, 255))
    d.pieslice([cx - r, cy - r, cx + r, cy + r], 270, 90, fill=(20, 20, 25, 255))
    d.ellipse([cx - r / 2, cy - r, cx + r / 2, cy], fill=(20, 20, 25, 255))
    d.ellipse([cx - r / 2, cy, cx + r / 2, cy + r], fill=(245, 245, 240, 255))
    d.ellipse([cx - 9, cy - r / 2 - 9, cx + 9, cy - r / 2 + 9], fill=(245, 245, 240, 255))
    d.ellipse([cx - 9, cy + r / 2 - 9, cx + 9, cy + r / 2 + 9], fill=(20, 20, 25, 255))
    # sword
    d.polygon([(40, 230), (200, 40), (210, 50), (52, 240)], fill=(200, 230, 255, 255))
    d.polygon([(30, 200), (80, 250), (88, 242), (38, 192)], fill=accent + (255,))
    d.line([(20, 255), (60, 215)], fill=(120, 60, 30, 255), width=10)
    glow = img.filter(ImageFilter.GaussianBlur(1))
    glow.save(path)
