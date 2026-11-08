"""Builds a made-up home folder for screenshots: eight invented games with generated cover
art, a fake Steam install with one Proton build, and the art already in the app's cache so
nothing is fetched. Nothing in it is a real game.

    python3 scripts/demo_home.py OUT_DIR FONT.ttf
"""

import math
import os
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

GAMES = [
    # folder, title, top colour, bottom colour, sun colour
    ("Neon.Harbor", "Neon Harbor", (40, 10, 70), (230, 40, 140), (255, 190, 90)),
    ("Moss_and_Lantern", "Moss & Lantern", (12, 40, 24), (110, 150, 60), (255, 200, 110)),
    ("Paper.Moons", "Paper Moons", (30, 34, 52), (190, 180, 160), (250, 245, 230)),
    ("Quiet.Orbit", "Quiet Orbit", (6, 14, 40), (20, 120, 140), (180, 240, 255)),
    ("Saltwind", "Saltwind", (20, 60, 110), (220, 190, 140), (255, 240, 200)),
    ("Tiny.Kingdoms", "Tiny Kingdoms", (60, 20, 80), (250, 120, 100), (255, 225, 140)),
    ("Velvet.Static", "Velvet Static", (10, 6, 10), (160, 20, 40), (255, 90, 90)),
    ("Glasshouse", "Glasshouse", (8, 40, 34), (90, 210, 170), (230, 255, 240)),
]
FIRST_ID = 900001


def gradient(w, h, top, bottom, horizontal=False):
    img = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(img)
    n = w if horizontal else h
    for i in range(n):
        t = i / max(n - 1, 1)
        c = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom))
        d.line([(i, 0), (i, h)] if horizontal else [(0, i), (w, i)], fill=c)
    return img


def art(w, h, top, bottom, sun, seed, title, font_path, horizontal=False):
    img = gradient(w, h, top, bottom, horizontal)
    # A soft sun or planet, with a glow.
    r = int(min(w, h) * 0.28)
    cx, cy = int(w * (0.3 + 0.4 * ((seed * 37) % 100) / 100)), int(h * 0.38)
    glow = Image.new("L", (w, h), 0)
    ImageDraw.Draw(glow).ellipse([cx - r * 1.6, cy - r * 1.6, cx + r * 1.6, cy + r * 1.6], fill=110)
    img.paste(Image.new("RGB", (w, h), sun), mask=glow.filter(ImageFilter.GaussianBlur(r * 0.6)))
    disc = Image.new("L", (w, h), 0)
    ImageDraw.Draw(disc).ellipse([cx - r, cy - r, cx + r, cy + r], fill=255)
    img.paste(Image.new("RGB", (w, h), sun), mask=disc.filter(ImageFilter.GaussianBlur(2)))
    # A horizon of hills.
    hills = Image.new("L", (w, h), 0)
    pts = [(0, h)]
    for x in range(0, w + 20, 20):
        y = h * 0.62 + math.sin(x / w * 6 + seed) * h * 0.04 + math.sin(x / w * 17 + seed * 2) * h * 0.015
        pts.append((x, y))
    pts.append((w, h))
    ImageDraw.Draw(hills).polygon(pts, fill=255)
    img.paste(Image.new("RGB", (w, h), tuple(c // 3 for c in top)), mask=hills)
    # Small stars.
    d = ImageDraw.Draw(img)
    for i in range(40):
        x = (seed * 7919 + i * 104729) % w
        y = (seed * 31 + i * 7477) % int(h * 0.55)
        s = 1 + (i % 3)
        d.ellipse([x - s, y - s, x + s, y + s], fill=(255, 255, 255))
    if title:
        size = int(w * 0.12)
        font = ImageFont.truetype(font_path, size)
        words, lines, line = title.split(), [], ""
        for word in words:
            test = (line + " " + word).strip()
            if d.textlength(test, font=font) > w * 0.84 and line:
                lines.append(line)
                line = word
            else:
                line = test
        lines.append(line)
        y = h * 0.92 - len(lines) * size * 1.1
        for ln in lines:
            d.text(((w - d.textlength(ln, font=font)) / 2, y), ln, font=font, fill=(255, 255, 255))
            y += size * 1.1
    return img


def main():
    home, font = sys.argv[1], sys.argv[2]
    games = os.path.join(home, "Games")
    cache = os.path.join(home, ".cache", "asterisk-ofix", "art")
    steam = os.path.join(home, ".local", "share", "Steam", "steamapps", "common", "Proton 9.0")
    for d in (games, cache, steam):
        os.makedirs(d, exist_ok=True)
    open(os.path.join(steam, "proton"), "w").close()
    for i, (folder, title, top, bottom, sun) in enumerate(GAMES):
        app_id = FIRST_ID + i
        root = os.path.join(games, folder)
        os.makedirs(root, exist_ok=True)
        open(os.path.join(root, folder.replace(".", "") + ".exe"), "w").close()
        open(os.path.join(root, "SteamFix64.dll"), "w").close()
        with open(os.path.join(root, "SteamFix.ini"), "w") as f:
            f.write(f"[Main]\nRealAppId={app_id}\nFakeAppId=480\n")
        art(600, 900, top, bottom, sun, i + 1, title, font).save(os.path.join(cache, f"{app_id}_cover.jpg"), quality=90)
        art(1920, 620, top, bottom, sun, i + 1, None, font, horizontal=True).save(
            os.path.join(cache, f"{app_id}_hero.jpg"), quality=88
        )
        with open(os.path.join(cache, f"{app_id}_name.txt"), "w") as f:
            f.write(title)
    print(f"demo home with {len(GAMES)} games in {home}")


main()
