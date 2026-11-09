"""Draws "asterofix" from a font's outlines into an SVG: white on black, with the font's
asterisk in the hole of the "o", centred in it and sized to leave a margin."""
import sys
import uharfbuzz as hb
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.recordingPen import RecordingPen

font_path, out, width, height, size = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), float(sys.argv[5])
text = "asterofix"
INK = 0.72  # the asterisk spans this share of the hole
ft = TTFont(font_path)
glyphs = ft.getGlyphSet()
upem = ft["head"].unitsPerEm

blob = hb.Blob.from_file_path(font_path)
hbfont = hb.Font(hb.Face(blob))
buf = hb.Buffer()
buf.add_str(text)
buf.guess_segment_properties()
hb.shape(hbfont, buf, {})
order = ft.getGlyphOrder()

scale = size / upem
pieces, pen_x = [], 0
for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
    name = order[info.codepoint]
    x, y = pen_x + pos.x_offset, pos.y_offset
    pieces.append((name, x, y))
    if text[info.cluster] == "o":
        hole = (name, x, y)
    pen_x += pos.x_advance


def contour_bounds(name):
    """The ink bounds of each of a glyph's contours."""
    rec = RecordingPen()
    glyphs[name].draw(rec)
    out, b = [], None
    for op, args in rec.value:
        if op == "moveTo":
            b = BoundsPen(glyphs)
        getattr(b, op)(*args)
        if op in ("closePath", "endPath"):
            out.append(b.bounds)
    return out


def bounds(name):
    b = BoundsPen(glyphs)
    glyphs[name].draw(b)
    return b.bounds


# The o's hole is its smaller contour. The asterisk goes in its middle, scaled so its larger
# side spans INK of the hole's smaller one.
name, x, y = hole
hx0, hy0, hx1, hy1 = min(contour_bounds(name), key=lambda c: (c[2] - c[0]) * (c[3] - c[1]))
star = order[hbfont.get_nominal_glyph(ord("*"))]
sx0, sy0, sx1, sy1 = bounds(star)
k = INK * min(hx1 - hx0, hy1 - hy0) / max(sx1 - sx0, sy1 - sy0)
star_at = (x + (hx0 + hx1) / 2 - k * (sx0 + sx1) / 2, y + (hy0 + hy1) / 2 - k * (sy0 + sy1) / 2)

# Ink bounds of the whole word, to centre it.
b = BoundsPen(glyphs)
for name, x, y in pieces:
    glyphs[name].draw(TransformPen(b, (1, 0, 0, 1, x, y)))
xmin, ymin, xmax, ymax = b.bounds
ox = width / 2 - (xmin + xmax) / 2 * scale
oy = height / 2 + (ymin + ymax) / 2 * scale  # y flips

paths = []
for name, x, y in pieces:
    p = SVGPathPen(glyphs)
    glyphs[name].draw(TransformPen(p, (scale, 0, 0, -scale, ox + x * scale, oy - y * scale)))
    paths.append(p.getCommands())
p = SVGPathPen(glyphs)
sx, sy = star_at
glyphs[star].draw(TransformPen(p, (scale * k, 0, 0, -scale * k, ox + sx * scale, oy - sy * scale)))
svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <title>asterofix</title>
  <rect width="{width}" height="{height}" fill="#000"/>
  <path fill="#f0f0f2" d="{" ".join(paths)}"/>
  <path fill="#f0f0f2" d="{p.getCommands()}"/>
</svg>
'''
open(out, "w").write(svg)
print(f"wrote {out}: ink {round((xmax - xmin) * scale)}x{round((ymax - ymin) * scale)} px")
