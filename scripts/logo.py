"""Draws "aster*sk" from a font's outlines into an SVG: white on black, with the asterisk
moved so its centre sits halfway up the lowercase letters."""
import sys
import uharfbuzz as hb
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.boundsPen import BoundsPen

font_path, out, width, height, size = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), float(sys.argv[5])
text = "aster*sk"
ft = TTFont(font_path)
glyphs = ft.getGlyphSet()
upem = ft["head"].unitsPerEm
x_height = ft["OS/2"].sxHeight

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
    dy = 0
    if text[info.cluster] == "*":
        b = BoundsPen(glyphs)
        glyphs[name].draw(b)
        _, ymin, _, ymax = b.bounds
        dy = x_height / 2 - (ymin + ymax) / 2
    pieces.append((name, pen_x + pos.x_offset, pos.y_offset + dy))
    pen_x += pos.x_advance

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
svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <title>asterisk</title>
  <rect width="{width}" height="{height}" fill="#000"/>
  <path fill="#f0f0f2" d="{" ".join(paths)}"/>
</svg>
'''
open(out, "w").write(svg)
print(f"wrote {out}: ink {round((xmax - xmin) * scale)}x{round((ymax - ymin) * scale)} px")
