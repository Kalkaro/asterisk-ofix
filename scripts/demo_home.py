"""Builds a made-up home folder for screenshots: Pong and Game 1 to 4, with black and white
covers drawn like box art (a little scene each, no titles), a fake Steam install with a few
Proton builds, and the art already in the app's cache so nothing is fetched.

    python3 scripts/demo_home.py OUT_DIR
"""

import math
import os
import random
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

FONT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "fonts", "GoogleSansFlex-SemiBold.ttf")
FIRST_ID = 900001
SS = 2  # drawn this many times larger, then shrunk, for smooth edges


def hexc(s, a=255):
    return (int(s[1:3], 16), int(s[3:5], 16), int(s[5:7], 16), a)


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def gradient(w, h, stops, horizontal=False):
    """A linear gradient through (position 0..1, colour) stops."""
    n = w if horizontal else h
    line = Image.new("RGBA", (n, 1))
    px = line.load()
    for i in range(n):
        t = i / max(1, n - 1)
        for (p0, c0), (p1, c1) in zip(stops, stops[1:]):
            if p0 <= t <= p1:
                px[i, 0] = lerp(c0, c1, (t - p0) / max(1e-6, p1 - p0))
                break
    if horizontal:
        return line.resize((w, h))
    return line.transpose(Image.Transpose.ROTATE_270).transpose(Image.Transpose.FLIP_LEFT_RIGHT).resize((w, h))


def layer(w, h):
    return Image.new("RGBA", (w, h), (0, 0, 0, 0))


def glow(base, top, radius, strength=1):
    """Lays `top` over `base` with a soft glow of itself around it."""
    halo = top.filter(ImageFilter.GaussianBlur(radius))
    for _ in range(strength):
        base.alpha_composite(halo)
    base.alpha_composite(top)


def disc(img, cx, cy, r, stops, horizontal=False):
    """A circle filled with a gradient."""
    box = (int(cx - r), int(cy - r), int(cx + r), int(cy + r))
    size = (box[2] - box[0], box[3] - box[1])
    fill = gradient(*size, stops, horizontal)
    mask = Image.new("L", size, 0)
    ImageDraw.Draw(mask).ellipse((0, 0, size[0] - 1, size[1] - 1), fill=255)
    img.paste(fill, box[:2], mask)


def stars(img, rnd, count, top=0.0, bottom=1.0):
    w, h = img.size
    d = ImageDraw.Draw(img)
    unit = min(w, h)
    for _ in range(count):
        x, y = rnd.uniform(0, w), rnd.uniform(top, bottom) * h
        r = rnd.choice([0.6, 0.8, 1.0, 1.0, 1.6]) * unit / 600
        a = rnd.randint(90, 255)
        d.ellipse((x - r, y - r, x + r, y + r), fill=(255, 255, 255, a))


def neon_drift(w, h, rnd):
    """A striped sun setting behind a city, over a glowing grid."""
    horizon = h * 0.6
    img = gradient(w, h, [(0, hexc("#000000")), (0.35, hexc("#121212")), (0.6, hexc("#5c5c5c")), (0.6, hexc("#060606")), (1, hexc("#000000"))])
    stars(img, rnd, int(w * h / 9000), 0, 0.4)
    unit = min(w, h)
    sun = layer(w, h)
    r = unit * 0.3
    cx, cy = w / 2, horizon - r * 0.45
    disc(sun, cx, cy, r, [(0, hexc("#ffffff")), (0.55, hexc("#d6d6d6")), (1, hexc("#9a9a9a"))])
    sd = ImageDraw.Draw(sun)
    gap = r * 0.06
    y = cy - r * 0.25
    while y < cy + r:  # bands cut from its lower part, thicker towards the bottom
        t = max(0.0, (y - cy) / r + 0.25)
        sd.rectangle((0, y, w, y + gap * (0.5 + t * 1.6)), fill=(0, 0, 0, 0))
        y += gap * 2.4
    sd.rectangle((0, horizon, w, h), fill=(0, 0, 0, 0))
    glow(img, sun, unit * 0.05, 2)
    city = layer(w, h)
    cd = ImageDraw.Draw(city)
    x = -rnd.uniform(0, w * 0.05)
    while x < w:
        bw = rnd.uniform(0.04, 0.09) * w
        bh = rnd.uniform(0.05, 0.2) * h * (1.4 if abs(x + bw / 2 - w / 2) > w * 0.25 else 0.45)
        cd.rectangle((x, horizon - bh, x + bw, horizon), fill=hexc("#050505"))
        for wy in range(int(horizon - bh + unit * 0.02), int(horizon - unit * 0.01), int(unit * 0.025)):
            for wx in range(int(x + bw * 0.2), int(x + bw * 0.8), int(unit * 0.02)):
                if rnd.random() < 0.18:
                    cd.rectangle((wx, wy, wx + unit * 0.006, wy + unit * 0.009), fill=hexc("#d8d8d8", 200))
        x += bw + rnd.uniform(0, 0.015) * w
    img.alpha_composite(city)
    grid = layer(w, h)
    gd = ImageDraw.Draw(grid)
    lw = max(2, int(unit * 0.004))
    for i in range(-14, 15):  # lines running to a vanishing point on the horizon
        gd.line((w / 2 + i * w * 0.02, horizon, w / 2 + i * w * 0.25, h), fill=hexc("#a8a8a8"), width=lw)
    k = 1.0
    while k < 40:
        y = horizon + (h - horizon) / k
        gd.line((0, y, w, y), fill=hexc("#a8a8a8"), width=lw)
        k *= 1.45
    fade = gradient(w, h, [(0, (0, 0, 0, 0)), (0.6, (0, 0, 0, 0)), (0.66, (255, 255, 255, 255)), (1, (255, 255, 255, 255))]).getchannel("A")
    grid.putalpha(Image.composite(grid.getchannel("A"), Image.new("L", (w, h), 0), fade))
    glow(img, grid, unit * 0.012, 2)
    return img


def low_orbit(w, h, rnd):
    """Space: a ringed planet and its moon over deep blue."""
    img = gradient(w, h, [(0, hexc("#000000")), (0.6, hexc("#080808")), (1, hexc("#141414"))])
    unit = min(w, h)
    stars(img, rnd, int(w * h / 3500))
    nebula = layer(w, h)
    nd = ImageDraw.Draw(nebula)
    for _ in range(5):
        x, y, r = rnd.uniform(0.1, 0.9) * w, rnd.uniform(0.2, 0.8) * h, rnd.uniform(0.15, 0.3) * unit
        nd.ellipse((x - r, y - r, x + r, y + r), fill=rnd.choice([hexc("#3a3a3a", 70), hexc("#2a2a2a", 60)]))
    img.alpha_composite(nebula.filter(ImageFilter.GaussianBlur(unit * 0.12)))
    r = unit * 0.42
    cx, cy = w * 0.62, h * 0.66
    atmosphere = layer(w, h)
    ImageDraw.Draw(atmosphere).ellipse((cx - r * 1.04, cy - r * 1.04, cx + r * 1.04, cy + r * 1.04), fill=hexc("#ffffff", 150))
    img.alpha_composite(atmosphere.filter(ImageFilter.GaussianBlur(unit * 0.04)))
    planet = layer(w, h)
    disc(planet, cx, cy, r, [(0, hexc("#f4f4f4")), (0.45, hexc("#a6a6a6")), (1, hexc("#1a1a1a"))], horizontal=True)
    bands = layer(w, h)
    bd = ImageDraw.Draw(bands)
    y = cy - r
    while y < cy + r:
        bh = rnd.uniform(0.02, 0.07) * r
        bd.rectangle((0, y, w, y + bh), fill=rnd.choice([hexc("#3c3c3c", 70), hexc("#ffffff", 45), hexc("#262626", 60)]))
        y += bh + rnd.uniform(0.03, 0.1) * r
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).ellipse((cx - r, cy - r, cx + r, cy + r), fill=255)
    planet.alpha_composite(Image.composite(bands.filter(ImageFilter.GaussianBlur(r * 0.015)), layer(w, h), mask))
    shade = gradient(w, h, [(0, (0, 0, 0, 0)), (0.5, (0, 0, 0, 0)), (1, (0, 0, 0, 210))], horizontal=True)
    planet.alpha_composite(Image.composite(shade, layer(w, h), mask))
    ring = layer(w, h)
    for i, (scale, col) in enumerate([(1.75, "#ececec"), (1.6, "#9c9c9c"), (1.45, "#f6f6f6")]):
        ImageDraw.Draw(ring).ellipse((cx - r * scale, cy - r * scale * 0.22, cx + r * scale, cy + r * scale * 0.22), outline=hexc(col, 220 - i * 30), width=max(2, int(r * 0.035)))
    back, front = ring.copy(), ring.copy()
    ImageDraw.Draw(front).rectangle((0, 0, w, cy), fill=(0, 0, 0, 0))  # the near half crosses the planet
    img.alpha_composite(back.rotate(-14, center=(cx, cy), resample=Image.Resampling.BICUBIC))
    img.alpha_composite(planet)
    img.alpha_composite(front.rotate(-14, center=(cx, cy), resample=Image.Resampling.BICUBIC))
    moon = layer(w, h)
    mr = unit * 0.06
    disc(moon, w * 0.22, h * 0.36, mr, [(0, hexc("#f2f2f2")), (1, hexc("#5a5a5a"))], horizontal=True)
    glow(img, moon, unit * 0.02)
    return img


def saltwind(w, h, rnd):
    """A sailboat crossing a sunset sea."""
    horizon = h * 0.58
    unit = min(w, h)
    img = gradient(w, h, [(0, hexc("#000000")), (0.3, hexc("#1c1c1c")), (0.5, hexc("#6a6a6a")), (0.58, hexc("#c4c4c4")), (0.58, hexc("#2e2e2e")), (1, hexc("#030303"))])
    sun = layer(w, h)
    r = unit * 0.16
    ImageDraw.Draw(sun).ellipse((w * 0.5 - r, horizon - r * 0.75, w * 0.5 + r, horizon + r * 1.25), fill=hexc("#ffffff"))
    ImageDraw.Draw(sun).rectangle((0, horizon, w, h), fill=(0, 0, 0, 0))
    glow(img, sun, unit * 0.08, 2)
    sea = layer(w, h)
    sd = ImageDraw.Draw(sea)
    y = horizon + unit * 0.01
    while y < h:  # the sun's path on the water, wider and sparser towards us
        t = (y - horizon) / (h - horizon)
        half = (0.08 + t * 0.35) * w * rnd.uniform(0.5, 1)
        sd.rectangle((w / 2 - half, y, w / 2 + half, y + unit * (0.004 + t * 0.008)), fill=hexc("#e6e6e6", int(200 * (1 - t) + 30)))
        y += unit * (0.012 + t * 0.03)
    img.alpha_composite(sea)
    for k, col in enumerate(["#1e1e1e", "#121212", "#070707"]):  # swells, darker towards us
        wv = layer(w, h)
        base = horizon + (h - horizon) * (0.45 + k * 0.2)
        amp, phase = unit * 0.015 * (k + 1), rnd.uniform(0, 6)
        pts = [(x, base + amp * math.sin(x / w * math.pi * (3 + k) + phase)) for x in range(0, w + 8, 8)]
        ImageDraw.Draw(wv).polygon(pts + [(w, h), (0, h)], fill=hexc(col))
        img.alpha_composite(wv)
    boat = layer(w, h)
    bd = ImageDraw.Draw(boat)
    bx, by, s = w * 0.3, horizon + unit * 0.02, unit * 0.16
    bd.polygon([(bx - s * 0.5, by), (bx + s * 0.5, by), (bx + s * 0.38, by + s * 0.12), (bx - s * 0.4, by + s * 0.12)], fill=hexc("#000000"))
    bd.polygon([(bx, by - s * 1.1), (bx, by - s * 0.05), (bx - s * 0.45, by - s * 0.05)], fill=hexc("#000000"))
    bd.polygon([(bx + s * 0.04, by - s * 0.95), (bx + s * 0.04, by - s * 0.05), (bx + s * 0.4, by - s * 0.05)], fill=hexc("#000000"))
    img.alpha_composite(boat)
    birds = ImageDraw.Draw(img)
    for _ in range(4):
        x, y, s = rnd.uniform(0.55, 0.85) * w, rnd.uniform(0.18, 0.32) * h, unit * rnd.uniform(0.015, 0.025)
        birds.line([(x - s, y - s * 0.4), (x, y), (x + s, y - s * 0.4)], fill=hexc("#0e0e0e"), width=max(2, int(s * 0.18)), joint="curve")
    return img


def hollow_pines(w, h, rnd):
    """A pine forest in mist under a full moon."""
    unit = min(w, h)
    img = gradient(w, h, [(0, hexc("#000000")), (0.5, hexc("#141414")), (0.75, hexc("#5a5a5a")), (1, hexc("#a4a4a4"))])
    stars(img, rnd, int(w * h / 12000), 0, 0.35)
    moon = layer(w, h)
    mr = unit * 0.11
    ImageDraw.Draw(moon).ellipse((w * 0.68 - mr, h * 0.3 - mr, w * 0.68 + mr, h * 0.3 + mr), fill=hexc("#f6f6f6"))
    glow(img, moon, unit * 0.06, 2)
    rows = [("#3e3e3e", 0.62, 0.22), ("#2a2a2a", 0.72, 0.3), ("#171717", 0.84, 0.4), ("#050505", 0.98, 0.55)]
    for i, (col, base, tall) in enumerate(rows):
        trees = layer(w, h)
        td = ImageDraw.Draw(trees)
        x = -rnd.uniform(0, 0.05) * w
        while x < w * 1.05:
            th = h * tall * rnd.uniform(0.6, 1.0)
            tw = th * 0.32
            top, bottom = h * base - th, h * base
            pts = [(x, top)]
            tiers = 6
            for k in range(1, tiers + 1):  # a jagged right side down, then the left side up
                y = top + th * k / tiers
                pts += [(x + tw / 2 * k / tiers, y), (x + tw / 2 * (k - 0.6) / tiers, y)]
            right = pts[1:]
            left = [(2 * x - px, py) for px, py in reversed(right)]
            td.polygon(pts + left, fill=hexc(col))
            td.rectangle((x - tw * 0.03, bottom - th * 0.05, x + tw * 0.03, h), fill=hexc(col))
            td.rectangle((0, bottom, w, h), fill=hexc(col))
            x += tw * rnd.uniform(0.7, 1.4)
        img.alpha_composite(trees)
        mist = gradient(w, h, [(0, (0, 0, 0, 0)), (max(0, base - 0.12), (0, 0, 0, 0)), (base, (235, 235, 235, 110 - i * 22)), (1, (235, 235, 235, 40))])
        img.alpha_composite(mist)
    return img


def pong(w, h, rnd):
    """An arcade table: neon paddles, the ball's trail and the score."""
    unit = min(w, h)
    img = gradient(w, h, [(0, hexc("#000000")), (1, hexc("#0a0a0a"))])
    vignette = layer(w, h)
    ImageDraw.Draw(vignette).ellipse((-w * 0.2, -h * 0.1, w * 1.2, h * 1.1), fill=hexc("#2a2a2a", 120))
    img.alpha_composite(vignette.filter(ImageFilter.GaussianBlur(unit * 0.15)))
    lines = layer(w, h)
    ld = ImageDraw.Draw(lines)
    dash = unit * 0.04
    y = 0.0
    while y < h:
        ld.rectangle((w / 2 - dash * 0.12, y, w / 2 + dash * 0.12, y + dash), fill=hexc("#f0f0f2", 110))
        y += dash * 2
    glow(img, lines, unit * 0.01)
    neon = layer(w, h)
    nd = ImageDraw.Draw(neon)
    pw, ph = unit * 0.04, unit * 0.24
    nd.rounded_rectangle((w * 0.08, h * 0.28, w * 0.08 + pw, h * 0.28 + ph), radius=pw / 2, fill=hexc("#f0f0f2"))
    nd.rounded_rectangle((w * 0.92 - pw, h * 0.56, w * 0.92, h * 0.56 + ph), radius=pw / 2, fill=hexc("#f0f0f2"))
    b = unit * 0.035
    bx, by = w * 0.66, h * 0.46
    for k in range(6, 0, -1):  # the trail behind the ball
        tx, ty = bx - k * b * 1.4, by - k * b * 0.55
        nd.ellipse((tx - b * 0.7, ty - b * 0.7, tx + b * 0.7, ty + b * 0.7), fill=(255, 255, 255, 18 * (7 - k)))
    nd.ellipse((bx - b, by - b, bx + b, by + b), fill=hexc("#ffffff"))
    glow(img, neon, unit * 0.03, 2)
    score = layer(w, h)
    font = ImageFont.truetype(FONT, int(unit * 0.16))
    sd = ImageDraw.Draw(score)
    sd.text((w * 0.35, h * 0.13), "3", font=font, fill=hexc("#f0f0f2", 200), anchor="mm")
    sd.text((w * 0.65, h * 0.13), "2", font=font, fill=hexc("#f0f0f2", 200), anchor="mm")
    glow(img, score, unit * 0.02)
    return img


# Name on disk and in Steam's data, and the scene on its cover.
GAMES = [
    ("Pong", pong),
    ("Game 1", neon_drift),
    ("Game 2", low_orbit),
    ("Game 3", saltwind),
    ("Game 4", hollow_pines),
]


def art(scene, w, h, seed):
    img = scene(w * SS, h * SS, random.Random(seed))
    return img.resize((w, h), Image.Resampling.LANCZOS).convert("RGB")


def main():
    home = sys.argv[1]
    games = os.path.join(home, "Games")
    cache = os.path.join(home, ".cache", "asterofix", "art")
    steam = os.path.join(home, ".local", "share", "Steam")
    for d in (games, cache):
        os.makedirs(d, exist_ok=True)
    # A few Proton builds, long GE names among them, for the Proton menu. Steam's default
    # stays Proton 9.0.
    for tool in ("Proton 9.0", "Proton - Experimental", "Proton Hotfix"):
        os.makedirs(os.path.join(steam, "steamapps", "common", tool), exist_ok=True)
        open(os.path.join(steam, "steamapps", "common", tool, "proton"), "w").close()
    for tool in ("GE-Proton10-34", "GE-Proton11-12-x86_64"):
        d = os.path.join(steam, "compatibilitytools.d", tool)
        os.makedirs(d, exist_ok=True)
        open(os.path.join(d, "proton"), "w").close()
        with open(os.path.join(d, "compatibilitytool.vdf"), "w") as f:
            f.write(f'"compatibilitytools" {{ "compat_tools" {{ "{tool}" {{ "install_path" "." }} }} }}\n')
    os.makedirs(os.path.join(steam, "config"), exist_ok=True)
    with open(os.path.join(steam, "config", "config.vdf"), "w") as f:
        f.write('"InstallConfigStore" { "Software" { "Valve" { "Steam" { "CompatToolMapping" '
                '{ "0" { "name" "proton_9" } } } } } }\n')
    for i, (name, scene) in enumerate(GAMES):
        app_id = FIRST_ID + i
        root = os.path.join(games, name)
        os.makedirs(root, exist_ok=True)
        open(os.path.join(root, name.replace(" ", "") + ".exe"), "w").close()
        open(os.path.join(root, "SteamFix64.dll"), "w").close()
        with open(os.path.join(root, "SteamFix.ini"), "w") as f:
            f.write(f"[Main]\nRealAppId={app_id}\nFakeAppId=480\n")
        art(scene, 600, 900, i).save(os.path.join(cache, f"{app_id}_cover.jpg"), quality=92)
        art(scene, 1920, 620, i).save(os.path.join(cache, f"{app_id}_hero.jpg"), quality=90)
        with open(os.path.join(cache, f"{app_id}_name.txt"), "w") as f:
            f.write(name)
    print(f"demo home with {len(GAMES)} games in {home}")


if __name__ == "__main__":
    main()
