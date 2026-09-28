#!/usr/bin/env python3
"""Writes the colour fonts the tests of the newer kinds of colour glyph read.

A reader held only to tables its own author wrote proves the author's reading
of the format and nothing else. No font of these kinds is packaged for the
build image's Debian, so fontTools - which is packaged, and is an
implementation of the format that is not this program - writes them: a
`COLR` version 1 font, an `sbix` font and an `SVG ` font, each of glyphs that
use what the format offers, mapped from the private use area so that they
never stand in for a real emoji anywhere else.

What each glyph is drawn as is written out beside it, and the tests hold the
program to that.

Run by the Dockerfile while the image is built:

    python3 tools/make-colour-fonts.py /usr/share/fonts/truetype/wp-colour
"""

import gzip
import os
import struct
import sys
import zlib

from fontTools.colorLib.builder import buildCPAL
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable
from fontTools.ttLib.tables import otTables as ot
from fontTools.ttLib.tables.S_V_G_ import SVGDocument
from fontTools.ttLib.tables.sbixGlyph import Glyph as SbixGlyph
from fontTools.ttLib.tables.sbixStrike import Strike

UNITS = 1000
ASCENT, DESCENT = 800, -200


def rectangle(x0, y0, x1, y1):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()
    return pen.glyph()


def circle(cx, cy, r):
    """A circle of eight quadratic arcs, as near a circle as TrueType draws."""
    import math

    pen = TTGlyphPen(None)
    points = []
    for step in range(8):
        angle = math.pi * step / 4
        points.append((cx + r * math.cos(angle), cy + r * math.sin(angle)))
    control = r / math.cos(math.pi / 8)
    pen.moveTo((round(points[0][0]), round(points[0][1])))
    for step in range(8):
        angle = math.pi * (step + 0.5) / 4
        off = (round(cx + control * math.cos(angle)), round(cy + control * math.sin(angle)))
        on = points[(step + 1) % 8]
        pen.qCurveTo(off, (round(on[0]), round(on[1])))
    pen.closePath()
    return pen.glyph()


def empty():
    return TTGlyphPen(None).glyph()


def base_font(family, names, outlines, characters):
    builder = FontBuilder(UNITS, isTTF=True)
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap(characters)
    builder.setupGlyf(outlines)
    # Each glyph's left side bearing is where its outline begins, as in any
    # real font: a reader that places outlines by the metrics - FreeType does
    # - would otherwise draw every glyph that does not start at nought moved
    # back to it.
    glyf = builder.font["glyf"]
    metrics = {}
    for name in names:
        outline = glyf[name]
        outline.recalcBounds(glyf)
        metrics[name] = (UNITS, getattr(outline, "xMin", 0) if outline.numberOfContours else 0)
    builder.setupHorizontalMetrics(metrics)
    builder.setupHorizontalHeader(ascent=ASCENT, descent=DESCENT)
    builder.setupNameTable({"familyName": family, "styleName": "Regular"})
    builder.setupOS2(sTypoAscender=ASCENT, sTypoDescender=DESCENT, usWinAscent=ASCENT, usWinDescent=-DESCENT)
    builder.setupPost()
    return builder


# ---------------------------------------------------------------------------
# COLR version 1
# ---------------------------------------------------------------------------

# The palette: every colour the glyphs name, by number.
RED, BLUE, GREEN, WHITE, YELLOW, HALF_BLACK = range(6)
PALETTE = [
    (1.0, 0.0, 0.0, 1.0),
    (0.0, 0.0, 1.0, 1.0),
    (0.0, 0.5, 0.0, 1.0),
    (1.0, 1.0, 1.0, 1.0),
    (1.0, 1.0, 0.0, 1.0),
    (0.0, 0.0, 0.0, 0.5),
]
TEXT = 0xFFFF


def stop(offset, colour, alpha=1.0):
    return {"StopOffset": offset, "PaletteIndex": colour, "Alpha": alpha}


def solid(colour, alpha=1.0):
    return {"Format": ot.PaintFormat.PaintSolid, "PaletteIndex": colour, "Alpha": alpha}


def glyph(name, paint):
    return {"Format": ot.PaintFormat.PaintGlyph, "Glyph": name, "Paint": paint}


def colr_font(folder):
    outlines = {
        ".notdef": empty(),
        "space": empty(),
        # The square every gradient is seen through: the whole em box above
        # the baseline.
        "square": rectangle(0, 0, 1000, 800),
        "disc": circle(500, 400, 400),
        # A bar lying along the x axis through the middle, to be turned.
        "bar": rectangle(100, 350, 900, 450),
        "left": rectangle(0, 0, 500, 800),
        "right": rectangle(500, 0, 1000, 800),
        "dot": rectangle(400, 300, 600, 500),
    }
    bases = {
        0xE000: "linear",
        0xE001: "radial",
        0xE002: "sweep",
        0xE003: "turned",
        0xE004: "multiplied",
        0xE005: "inked",
        0xE006: "borrowed",
        0xE007: "repeated",
        0xE008: "reflected",
        0xE009: "skewed",
        0xE00A: "kept",
        0xE00B: "half",
        0xE010: "layered",
    }
    for name in bases.values():
        outlines[name] = empty()
    names = list(outlines)
    characters = {code: name for code, name in bases.items()}
    builder = base_font("WP Colour One", names, outlines, characters)

    colour_glyphs = {
        # Red at the left edge to blue at the right, the same all the way up:
        # p2 straight above p0 keeps the colours in vertical lines.
        "linear": glyph(
            "square",
            {
                "Format": ot.PaintFormat.PaintLinearGradient,
                "ColorLine": {"Extend": "pad", "ColorStop": [stop(0, RED), stop(1, BLUE)]},
                "x0": 0, "y0": 0, "x1": 1000, "y1": 0, "x2": 0, "y2": 800,
            },
        ),
        # White at the centre of the disc to green at its edge.
        "radial": glyph(
            "disc",
            {
                "Format": ot.PaintFormat.PaintRadialGradient,
                "ColorLine": {"Extend": "pad", "ColorStop": [stop(0, WHITE), stop(1, GREEN)]},
                "x0": 500, "y0": 400, "r0": 0, "x1": 500, "y1": 400, "r1": 400,
            },
        ),
        # Red to the right of the centre, yellow a half turn round (to the
        # left), anticlockwise, and red again.
        "sweep": glyph(
            "square",
            {
                "Format": ot.PaintFormat.PaintSweepGradient,
                "ColorLine": {
                    "Extend": "pad",
                    "ColorStop": [stop(0, RED), stop(0.5, YELLOW), stop(1, RED)],
                },
                "centerX": 500, "centerY": 400, "startAngle": 0, "endAngle": 360,
            },
        ),
        # The bar, blue, turned a quarter turn about the middle: upright.
        "turned": {
            "Format": ot.PaintFormat.PaintRotateAroundCenter,
            "angle": 90,
            "centerX": 500,
            "centerY": 400,
            "Paint": glyph("bar", solid(BLUE)),
        },
        # A red disc multiplied onto a yellow square: red where both are,
        # yellow where only the square is.
        "multiplied": {
            "Format": ot.PaintFormat.PaintComposite,
            "SourcePaint": glyph("disc", solid(RED)),
            "CompositeMode": "multiply",
            "BackdropPaint": glyph("square", solid(YELLOW)),
        },
        # The square in the colour of the text, with a green dot on top.
        "inked": {
            "Format": ot.PaintFormat.PaintColrLayers,
            "Layers": [glyph("square", solid(TEXT)), glyph("dot", solid(GREEN))],
        },
        # The linear glyph again, moved a hundred units up and clipped by
        # its own box.
        "borrowed": {
            "Format": ot.PaintFormat.PaintTranslate,
            "dx": 0,
            "dy": 100,
            "Paint": {"Format": ot.PaintFormat.PaintColrGlyph, "Glyph": "linear"},
        },
        # Red to blue over the first quarter of the width, repeated.
        "repeated": glyph(
            "square",
            {
                "Format": ot.PaintFormat.PaintLinearGradient,
                "ColorLine": {"Extend": "repeat", "ColorStop": [stop(0, RED), stop(1, BLUE)]},
                "x0": 0, "y0": 0, "x1": 250, "y1": 0, "x2": 0, "y2": 800,
            },
        ),
        # And reflected.
        "reflected": glyph(
            "square",
            {
                "Format": ot.PaintFormat.PaintLinearGradient,
                "ColorLine": {"Extend": "reflect", "ColorStop": [stop(0, RED), stop(1, BLUE)]},
                "x0": 0, "y0": 0, "x1": 250, "y1": 0, "x2": 0, "y2": 800,
            },
        ),
        # The left half, green, skewed by forty-five degrees along x about
        # the origin: its top edge moves left by its height.
        "skewed": {
            "Format": ot.PaintFormat.PaintSkew,
            "xSkewAngle": 45,
            "ySkewAngle": 0,
            "Paint": glyph("left", solid(GREEN)),
        },
        # A blue square kept only where a red disc is: source-in with the
        # disc as the backdrop.
        "kept": {
            "Format": ot.PaintFormat.PaintComposite,
            "SourcePaint": glyph("square", solid(BLUE)),
            "CompositeMode": "src_in",
            "BackdropPaint": glyph("disc", solid(RED)),
        },
        # Half-transparent black over the right half.
        "half": glyph("right", solid(HALF_BLACK)),
        # The older kind, in the same table: two layers.
        "layered": [("left", RED), ("right", BLUE)],
    }
    clip_boxes = {"linear": (0, 0, 1000, 800), "borrowed": (0, 0, 1000, 800)}
    builder.setupCOLR(colour_glyphs, clipBoxes=clip_boxes)
    builder.setupCPAL([PALETTE])
    builder.save(os.path.join(folder, "wp-colour-one.ttf"))


# ---------------------------------------------------------------------------
# sbix
# ---------------------------------------------------------------------------


def png(width, height, pixel):
    """A PNG of the given size, each pixel's RGBA from a function."""
    rows = b""
    for y in range(height):
        rows += b"\0" + b"".join(bytes(pixel(x, y)) for x in range(width))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(rows))
        + chunk(b"IEND", b"")
    )


def tiff(width, height, pixel):
    """An uncompressed TIFF of RGBA pixels, little-endian."""
    data = b"".join(bytes(pixel(x, y)) for y in range(height) for x in range(width))
    entries = [
        (256, 3, 1, width),  # ImageWidth
        (257, 3, 1, height),  # ImageLength
        (258, 3, 4, None),  # BitsPerSample, 8 8 8 8, written after the entries
        (259, 3, 1, 1),  # no compression
        (262, 3, 1, 2),  # RGB
        (273, 4, 1, None),  # StripOffsets
        (277, 3, 1, 4),  # SamplesPerPixel
        (278, 3, 1, height),  # RowsPerStrip
        (279, 4, 1, len(data)),  # StripByteCounts
        (338, 3, 1, 2),  # ExtraSamples: unassociated alpha
    ]
    ifd_at = 8
    after_ifd = ifd_at + 2 + len(entries) * 12 + 4
    bits_at = after_ifd
    data_at = bits_at + 8
    out = b"II*\0" + struct.pack("<I", ifd_at) + struct.pack("<H", len(entries))
    for tag, kind, count, value in entries:
        if tag == 258:
            value = bits_at
        if tag == 273:
            value = data_at
        out += struct.pack("<HHI", tag, kind, count)
        out += struct.pack("<I", value) if kind == 4 or tag == 258 else struct.pack("<HH", value, 0)
    out += struct.pack("<I", 0)
    out += struct.pack("<HHHH", 8, 8, 8, 8)
    return out + data


def sbix_font(folder):
    names = [".notdef", "space", "square", "same", "tiled"]
    outlines = {name: empty() for name in names}
    characters = {0xE000: "square", 0xE001: "same", 0xE002: "tiled"}
    builder = base_font("WP Colour Pictures", names, outlines, characters)

    table = newTable("sbix")
    table.version = 1
    table.flags = 1
    for size in (20, 40):
        strike = Strike(ppem=size, resolution=72)
        # A red square the full em across, standing on the baseline, with a
        # blue top half so that which way up it is can be seen.
        picture = png(
            size, size, lambda x, y: (0, 0, 255, 255) if y < size // 2 else (255, 0, 0, 255)
        )
        strike.glyphs["square"] = SbixGlyph(
            glyphName="square", graphicType="png ", imageData=picture, originOffsetX=0, originOffsetY=0
        )
        strike.glyphs["same"] = SbixGlyph(
            glyphName="same", graphicType="dupe", imageData=struct.pack(">H", names.index("square"))
        )
        strike.glyphs["tiled"] = SbixGlyph(
            glyphName="tiled",
            graphicType="tiff",
            imageData=tiff(size, size, lambda x, y: (0, 128, 0, 255)),
            originOffsetX=size // 4,
            originOffsetY=-size // 4,
        )
        table.strikes[size] = strike
    builder.font["sbix"] = table
    builder.save(os.path.join(folder, "wp-colour-pictures.ttf"))


# ---------------------------------------------------------------------------
# SVG
# ---------------------------------------------------------------------------

SVG_DOCUMENTS = {
    # A red square the whole em, with a linear gradient over its right half
    # from yellow to blue, left to right. y runs down and the baseline is at
    # nought, so the em above the baseline is from -800 to 0.
    "gradient": """<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <defs>
    <linearGradient id="fade" x1="500" y1="0" x2="1000" y2="0" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#FFFF00"/>
      <stop offset="1" stop-color="blue"/>
    </linearGradient>
  </defs>
  <g id="glyph{gradient}">
    <rect x="0" y="-800" width="1000" height="800" fill="#ff0000"/>
    <rect x="500" y="-800" width="500" height="800" fill="url(#fade)"/>
  </g>
</svg>""",
    # A green circle, moved into place by a transform, and a half-opaque
    # black rectangle drawn with a use of one defined elsewhere.
    "shapes": """<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <defs>
    <rect id="shade" width="500" height="800"/>
  </defs>
  <g id="glyph{shapes}">
    <g transform="translate(500 -400)">
      <circle r="300" fill="rgb(0, 128, 0)"/>
    </g>
    <use xlink:href="#shade" x="500" y="-800" fill="black" opacity="0.5"/>
  </g>
</svg>""",
    # A radial gradient from white at the middle to red at the edge of a
    # disc, compressed as a font may compress it.
    "radial": """<svg xmlns="http://www.w3.org/2000/svg">
  <radialGradient id="glow" cx="500" cy="-400" r="400" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="white"/>
    <stop offset="1" stop-color="red"/>
  </radialGradient>
  <path id="glyph{radial}" d="M100-400A400 400 0 1 0 900-400A400 400 0 1 0 100-400Z" fill="url(#glow)"/>
</svg>""",
}


def svg_font(folder):
    names = [".notdef", "space", "gradient", "shapes", "radial"]
    outlines = {name: empty() for name in names}
    characters = {0xE000: "gradient", 0xE001: "shapes", 0xE002: "radial"}
    builder = base_font("WP Colour Drawings", names, outlines, characters)

    table = newTable("SVG ")
    documents = []
    for name, text in SVG_DOCUMENTS.items():
        number = names.index(name)
        text = text.replace("{" + name + "}", str(number))
        compressed = name == "radial"
        data = gzip.compress(text.encode()) if compressed else text
        documents.append(SVGDocument(data, number, number, compressed))
    table.docList = documents
    builder.font["SVG "] = table
    builder.save(os.path.join(folder, "wp-colour-drawings.ttf"))


def main():
    folder = sys.argv[1] if len(sys.argv) > 1 else "."
    os.makedirs(folder, exist_ok=True)
    colr_font(folder)
    sbix_font(folder)
    svg_font(folder)


if __name__ == "__main__":
    main()
