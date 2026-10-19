#!/usr/bin/env python3
"""Draws every picture and the 3D model in the sample deck, so the deck holds nothing taken from
anywhere else. Run it before build_sample.py:

    python3 make_assets.py OUT_DIR

Needs Pillow and NumPy. Everything is drawn from code with a fixed seed, so the files come out the
same each time.
"""

import json
import math
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

OUT = Path(sys.argv[1] if len(sys.argv) > 1 else "media")
OUT.mkdir(parents=True, exist_ok=True)
RNG = np.random.default_rng(7)


def font(size):
    for name in ("DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            pass
    return ImageFont.load_default()


# ---------- 3D model: the Earth in four layers ----------

def sphere(radius, rows, cols, colour_of):
    pos, nor, col, idx = [], [], [], []
    for r in range(rows + 1):
        lat = math.pi * r / rows
        for c in range(cols + 1):
            lon = 2 * math.pi * c / cols
            n = (math.sin(lat) * math.cos(lon), math.cos(lat), math.sin(lat) * math.sin(lon))
            pos.append(tuple(radius * v for v in n))
            nor.append(n)
            col.append(colour_of(n))
    for r in range(rows):
        for c in range(cols):
            a = r * (cols + 1) + c
            b = a + cols + 1
            idx += [a, b, a + 1, a + 1, b, b + 1]
    return pos, nor, col, idx


def land(n):
    x, y, z = n
    v = (math.sin(3.1 * x + 1.3) * math.cos(2.3 * z - 0.4) + 0.6 * math.sin(5.7 * y + 2.1 * x)
         + 0.35 * math.cos(7.3 * z + 3.9 * y))
    return v


def crust_colour(n):
    if abs(n[1]) > 0.88:
        return (0.95, 0.97, 1.0, 1.0)
    v = land(n)
    if v > 0.55:
        g = min(1.0, (v - 0.55) * 1.5)
        return (0.30 + 0.25 * g, 0.55 - 0.15 * g, 0.22, 1.0)
    return (0.10, 0.32 + 0.08 * v, 0.62, 1.0)


def flat(c):
    return lambda n: (*c, 1.0)


def write_glb(path, layers):
    """layers: [(name, (pos, nor, col, idx))]. One mesh and one node each."""
    blob = bytearray()
    views, accessors, meshes, nodes = [], [], [], []

    def add(data, target, comp, count, kind, minmax=None):
        while len(blob) % 4:
            blob.append(0)
        off = len(blob)
        blob.extend(data)
        views.append({"buffer": 0, "byteOffset": off, "byteLength": len(data), "target": target})
        acc = {"bufferView": len(views) - 1, "componentType": comp, "count": count, "type": kind}
        if minmax:
            acc["min"], acc["max"] = minmax
        accessors.append(acc)
        return len(accessors) - 1

    for i, (name, (pos, nor, col, idx)) in enumerate(layers):
        p = add(b"".join(struct.pack("<3f", *v) for v in pos), 34962, 5126, len(pos), "VEC3",
                ([min(v[k] for v in pos) for k in range(3)], [max(v[k] for v in pos) for k in range(3)]))
        n = add(b"".join(struct.pack("<3f", *v) for v in nor), 34962, 5126, len(nor), "VEC3")
        c = add(b"".join(struct.pack("<4f", *v) for v in col), 34962, 5126, len(col), "VEC4")
        ix = add(b"".join(struct.pack("<I", v) for v in idx), 34963, 5125, len(idx), "SCALAR")
        meshes.append({"name": name, "primitives": [{"attributes": {"POSITION": p, "NORMAL": n, "COLOR_0": c}, "indices": ix, "material": 0}]})
        nodes.append({"name": name, "mesh": i})
    doc = {
        "asset": {"version": "2.0", "generator": "deckrt sample make_assets.py"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": [{"name": "Rock", "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0, "roughnessFactor": 0.85}}],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(blob)}],
    }
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    blob += b"\0" * (-len(blob) % 4)
    out = struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(js) + 8 + len(blob))
    out += struct.pack("<II", len(js), 0x4E4F534A) + js
    out += struct.pack("<II", len(blob), 0x004E4942) + bytes(blob)
    path.write_bytes(out)


write_glb(OUT / "Earth layers.glb", [
    ("Crust", sphere(1.0, 48, 96, crust_colour)),
    ("Mantle", sphere(0.86, 32, 64, flat((0.86, 0.36, 0.10)))),
    ("Outer core", sphere(0.55, 28, 56, flat((0.96, 0.66, 0.12)))),
    ("Inner core", sphere(0.30, 24, 48, flat((1.0, 0.93, 0.62)))),
])


# ---------- Image swap: three phases of the Moon ----------

def moon(phase, path):
    W, H = 900, 640
    img = Image.new("RGB", (W, H), (8, 12, 30))
    d = ImageDraw.Draw(img)
    for _ in range(160):
        x, y = RNG.integers(0, W), RNG.integers(0, H)
        b = int(RNG.integers(120, 255))
        d.point((int(x), int(y)), fill=(b, b, b))
    cx, cy, r = W // 2, H // 2 - 20, 210
    yy, xx = np.mgrid[0:H, 0:W]
    dx, dy = (xx - cx) / r, (yy - cy) / r
    inside = dx * dx + dy * dy <= 1
    # The lit part: x beyond the terminator, an ellipse that sweeps across with the phase.
    term = math.cos(phase) * np.sqrt(np.clip(1 - dy * dy, 0, 1))
    bright = dx >= term
    a = np.array(img).astype(float)
    noise = RNG.normal(0, 1, (H, W))
    noise = np.array(Image.fromarray(((noise + 3) * 40).clip(0, 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(6))).astype(float) / 255
    grey = 170 + 70 * (noise - 0.5) * 2
    for k in range(3):
        a[..., k] = np.where(inside & bright, grey * [1.0, 0.98, 0.92][k], np.where(inside, 28 + 8 * noise, a[..., k]))
    img = Image.fromarray(a.clip(0, 255).astype(np.uint8))
    img.save(path, quality=88)


moon(math.pi * 0.25, OUT / "Moon crescent.jpg")
moon(math.pi * 0.5, OUT / "Moon first quarter.jpg")
moon(math.pi * 0.999, OUT / "Moon full.jpg")


# ---------- Wipe: one hill in winter and in spring ----------

def hill(season, path):
    W, H = 1000, 680
    yy, xx = np.mgrid[0:H, 0:W].astype(float)
    sky_top = np.array([150, 185, 225] if season == "winter" else [90, 160, 235], float)
    sky_bot = np.array([225, 232, 240] if season == "winter" else [200, 228, 250], float)
    t = (yy / H)[..., None]
    img = sky_top * (1 - t) + sky_bot * t
    ridge = H * 0.55 + 60 * np.sin(xx / 170 + 0.6) + 25 * np.sin(xx / 53)
    near = H * 0.72 + 40 * np.sin(xx / 260 + 2.0)
    snow = np.array([238, 242, 248], float)
    grass = np.array([96, 160, 70], float)
    far_snow = np.array([214, 222, 236], float)
    far_grass = np.array([70, 128, 90], float)
    far = far_snow if season == "winter" else far_grass
    front = snow if season == "winter" else grass
    shade = 0.92 + 0.08 * np.sin(xx / 40 + yy / 23)[..., None]
    img = np.where((yy >= ridge)[..., None], far * shade, img)
    img = np.where((yy >= near)[..., None], front * shade, img)
    im = Image.fromarray(img.clip(0, 255).astype(np.uint8))
    d = ImageDraw.Draw(im)
    # Trees: bare in winter, green in spring.
    for x0 in range(70, W, 115):
        base = int(H * 0.72 + 40 * math.sin(x0 / 260 + 2.0)) + 6
        d.rectangle([x0 - 5, base - 70, x0 + 5, base], fill=(92, 64, 40))
        if season == "winter":
            for k in range(4):
                y = base - 70 + k * 12
                d.line([x0, y, x0 - 22 + k * 3, y - 18], fill=(92, 64, 40), width=3)
                d.line([x0, y, x0 + 22 - k * 3, y - 18], fill=(92, 64, 40), width=3)
        else:
            d.ellipse([x0 - 38, base - 140, x0 + 38, base - 50], fill=(60, 140, 60))
            for _ in range(5):
                fx, fy = x0 + int(RNG.integers(-28, 28)), base - int(RNG.integers(60, 130))
                d.ellipse([fx - 4, fy - 4, fx + 4, fy + 4], fill=(250, 210, 230))
    if season == "spring":
        for _ in range(140):
            fx, fy = int(RNG.integers(0, W)), int(RNG.integers(int(H * 0.78), H))
            c = [(250, 220, 60), (240, 240, 250), (230, 120, 160)][int(RNG.integers(0, 3))]
            d.ellipse([fx - 3, fy - 3, fx + 3, fy + 3], fill=c)
    im.save(path, quality=88)


hill("winter", OUT / "Hill in winter.jpg")
hill("spring", OUT / "Hill in spring.jpg")


# ---------- Scrub: a bean plant on four days ----------

def plant(day, path):
    W, H = 900, 680
    im = Image.new("RGB", (W, H), (246, 242, 232))
    d = ImageDraw.Draw(im)
    d.rectangle([0, 520, W, H], fill=(222, 214, 196))
    d.polygon([(330, 470), (570, 470), (540, 640), (360, 640)], fill=(186, 98, 58))
    d.rectangle([316, 450, 584, 482], fill=(170, 86, 50))
    d.ellipse([340, 440, 560, 470], fill=(90, 62, 40))
    height = {0: 0, 3: 60, 7: 170, 14: 300}[day]
    if day == 0:
        d.ellipse([430, 444, 470, 462], fill=(210, 190, 140))
    else:
        top = 455 - height
        d.line([450, 455, 452, top], fill=(70, 140, 60), width=7)
        leaves = {3: 1, 7: 2, 14: 4}[day]
        for k in range(leaves):
            y = top + 14 + k * (height // max(leaves, 1)) * 0.8
            side = -1 if k % 2 else 1
            size = 34 + 10 * (leaves - k)
            d.ellipse([452 + side * 6 - (size if side < 0 else 0), y - size // 3, 452 + side * 6 + (size if side > 0 else 0), y + size // 3],
                      fill=(88, 170, 70))
    d.text((40, 36), f"Day {day}", fill=(60, 60, 60), font=font(46))
    im.save(path, quality=90)


for day in (0, 3, 7, 14):
    plant(day, OUT / f"Day {day}.jpg")


# ---------- Zoom region: a town map with small labels ----------

def town(path):
    W, H = 1200, 1350
    im = Image.new("RGB", (W, H), (236, 233, 222))
    d = ImageDraw.Draw(im)
    d.polygon([(0, 980), (W, 900), (W, 1080), (0, 1150)], fill=(150, 196, 230))
    for x in range(0, W, 150):
        d.rectangle([x, 0, x + 18, H], fill=(255, 255, 255))
    for y in range(0, H, 170):
        d.rectangle([0, y, W, y + 18], fill=(255, 255, 255))
    small = font(20)
    names = ["Bakery", "Library", "School", "Clinic", "Station", "Park", "Post office", "Market", "Pool", "Museum", "Hotel", "Cafe"]
    k = 0
    for gx in range(0, W, 150):
        for gy in range(0, H, 170):
            if gy + 170 > 900 and gy < 1150:
                continue
            c = [(214, 196, 170), (190, 205, 180), (205, 190, 205), (220, 205, 165)][(gx // 150 + gy // 170) % 4]
            d.rectangle([gx + 26, gy + 26, gx + 142, gy + 162], fill=c)
            if (gx // 150 * 7 + gy // 170 * 3) % 5 == 0:
                d.text((gx + 32, gy + 80), names[k % len(names)], fill=(50, 50, 50), font=small)
                k += 1
    # The market square the zoom box goes to.
    d.rectangle([780, 360, 1050, 600], fill=(250, 214, 120), outline=(180, 120, 40), width=4)
    d.text((812, 460), "Market square", fill=(80, 50, 10), font=font(30))
    for i in range(6):
        d.rectangle([800 + i * 40, 400, 828 + i * 40, 428], fill=[(220, 70, 60), (60, 140, 220), (90, 170, 80)][i % 3])
    im.save(path)


town(OUT / "Town map.png")


# ---------- Measure and injection map: a drawn face, front on ----------

def face(path):
    W, H = 760, 880
    im = Image.new("RGB", (W, H), (234, 236, 240))
    d = ImageDraw.Draw(im)
    skin, line = (236, 196, 170), (150, 100, 80)
    cx = W // 2
    d.ellipse([cx - 230, 70, cx + 230, 830], fill=skin)
    d.chord([cx - 250, 40, cx + 250, 420], 180, 360, fill=(80, 56, 40))  # hair
    d.rectangle([cx - 240, 230, cx - 214, 420], fill=(80, 56, 40))
    d.rectangle([cx + 214, 230, cx + 240, 420], fill=(80, 56, 40))
    d.ellipse([cx - 262, 400, cx - 222, 520], fill=skin, outline=line, width=2)  # ears
    d.ellipse([cx + 222, 400, cx + 262, 520], fill=skin, outline=line, width=2)
    brow = 360
    for s in (-1, 1):
        x0 = cx + s * 40
        x1 = cx + s * 150
        d.line([min(x0, x1), brow, max(x0, x1), brow - 8], fill=(90, 60, 45), width=10)
        ex = cx + s * 95
        d.ellipse([ex - 48, 410, ex + 48, 446], fill=(255, 255, 255), outline=line, width=2)
        d.ellipse([ex - 17, 411, ex + 17, 445], fill=(90, 120, 90))
        d.ellipse([ex - 7, 421, ex + 7, 435], fill=(20, 20, 20))
    d.line([cx, 440, cx - 10, 560], fill=line, width=3)
    d.arc([cx - 46, 540, cx + 46, 600], 20, 160, fill=line, width=3)
    d.chord([cx - 80, 620, cx + 80, 690], 0, 180, fill=(200, 110, 110))
    d.line([cx - 80, 655, cx + 80, 655], fill=(150, 70, 70), width=3)
    im.save(path)


face(OUT / "Face drawing.png")
print("made", sorted(p.name for p in OUT.iterdir()))
