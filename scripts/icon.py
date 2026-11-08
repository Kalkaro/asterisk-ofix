"""Draws the app icon, the font's asterisk centred on a black rounded square, into an SVG:
    python3 scripts/icon.py fonts/GoogleSansFlex-SemiBold.ttf packaging/asterisk-ofix.svg"""
import sys
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.boundsPen import BoundsPen

font_path, out = sys.argv[1], sys.argv[2]
SIZE, INK = 256, 0.62  # the asterisk spans this share of the square

ft = TTFont(font_path)
glyphs = ft.getGlyphSet()
name = ft.getBestCmap()[ord("*")]
b = BoundsPen(glyphs)
glyphs[name].draw(b)
xmin, ymin, xmax, ymax = b.bounds
scale = SIZE * INK / max(xmax - xmin, ymax - ymin)
# Font units point up; SVG's point down.
dx = SIZE / 2 - (xmin + xmax) / 2 * scale
dy = SIZE / 2 + (ymin + ymax) / 2 * scale

pen = SVGPathPen(glyphs)
glyphs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, dx, dy)))
with open(out, "w") as f:
    f.write(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}" viewBox="0 0 {SIZE} {SIZE}">\n'
        f'  <rect width="{SIZE}" height="{SIZE}" rx="{SIZE * 0.22:g}" fill="#000"/>\n'
        f'  <path fill="#f0f0f2" d="{pen.getCommands()}"/>\n'
        "</svg>\n"
    )
