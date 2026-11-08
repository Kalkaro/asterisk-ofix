"""Builds a made-up home folder for screenshots: Pong and four placeholder games with covers
of white shapes on black, a fake Steam install with one Proton build, and the art already in
the app's cache so nothing is fetched.

    python3 scripts/demo_home.py OUT_DIR
"""

import os
import random
import sys

from PIL import Image, ImageDraw

GAMES = ["Pong", "Game 1", "Game 2", "Game 3", "Game 4"]
FIRST_ID = 900001
WHITE = (240, 240, 242)


def shapes(w, h, seed):
    """A few random white circles, rings, squares and bars on black."""
    rnd = random.Random(seed)
    img = Image.new("RGB", (w, h), (0, 0, 0))
    d = ImageDraw.Draw(img)
    unit = min(w, h)
    for _ in range(rnd.randint(4, 7)):
        x, y = rnd.uniform(0.1, 0.9) * w, rnd.uniform(0.1, 0.9) * h
        r = rnd.uniform(0.06, 0.2) * unit
        kind = rnd.choice(["disc", "ring", "square", "bar"])
        box = [x - r, y - r, x + r, y + r]
        if kind == "disc":
            d.ellipse(box, fill=WHITE)
        elif kind == "ring":
            d.ellipse(box, outline=WHITE, width=max(4, int(r * 0.18)))
        elif kind == "square":
            d.rectangle(box, fill=WHITE)
        else:
            d.rectangle([x - r * 1.6, y - r * 0.12, x + r * 1.6, y + r * 0.12], fill=WHITE)
    return img


def pong(w, h):
    """Two paddles, a ball and the dashed centre line."""
    img = Image.new("RGB", (w, h), (0, 0, 0))
    d = ImageDraw.Draw(img)
    unit = min(w, h)
    pw, ph = unit * 0.035, unit * 0.22
    d.rectangle([w * 0.1, h * 0.3, w * 0.1 + pw, h * 0.3 + ph], fill=WHITE)
    d.rectangle([w * 0.9 - pw, h * 0.55, w * 0.9, h * 0.55 + ph], fill=WHITE)
    b = unit * 0.035
    d.rectangle([w * 0.62 - b, h * 0.42 - b, w * 0.62 + b, h * 0.42 + b], fill=WHITE)
    dash = unit * 0.04
    y = 0.0
    while y < h:
        d.rectangle([w / 2 - dash * 0.15, y, w / 2 + dash * 0.15, y + dash], fill=WHITE)
        y += dash * 2
    return img


def main():
    home = sys.argv[1]
    games = os.path.join(home, "Games")
    cache = os.path.join(home, ".cache", "asterisk-ofix", "art")
    proton = os.path.join(home, ".local", "share", "Steam", "steamapps", "common", "Proton 9.0")
    for d in (games, cache, proton):
        os.makedirs(d, exist_ok=True)
    open(os.path.join(proton, "proton"), "w").close()
    for i, name in enumerate(GAMES):
        app_id = FIRST_ID + i
        root = os.path.join(games, name)
        os.makedirs(root, exist_ok=True)
        open(os.path.join(root, name.replace(" ", "") + ".exe"), "w").close()
        open(os.path.join(root, "SteamFix64.dll"), "w").close()
        with open(os.path.join(root, "SteamFix.ini"), "w") as f:
            f.write(f"[Main]\nRealAppId={app_id}\nFakeAppId=480\n")
        cover = pong(600, 900) if name == "Pong" else shapes(600, 900, i)
        hero = pong(1920, 620) if name == "Pong" else shapes(1920, 620, i + 100)
        cover.save(os.path.join(cache, f"{app_id}_cover.jpg"), quality=92)
        hero.save(os.path.join(cache, f"{app_id}_hero.jpg"), quality=90)
        with open(os.path.join(cache, f"{app_id}_name.txt"), "w") as f:
            f.write(name)
    print(f"demo home with {len(GAMES)} games in {home}")


main()
