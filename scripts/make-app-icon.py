#!/usr/bin/env python3
"""Cut the product logo to the macOS app icon grid.

The logo art is a full-bleed square, and nothing downstream rounds it: Tauri
hands the app icon to `NSApp.setApplicationIconImage` on a dev run and packs it
into the bundle for a built one, and neither path applies a mask. Shipped raw,
the Dock draws a hard-edged square that overhangs every neighbouring icon by
the margin they all leave. So the shape is baked in here — Apple's grid, an 824
body on a 1024 canvas, corners on a continuous-curvature squircle.

An authoring tool rather than a build step, so — like import-pet.py and unlike
the make-*-characters scripts — it is allowed to need Pillow.

    python3 scripts/make-app-icon.py

It rewrites branding/logo-art/app-icon-1024.png, src-tauri/icons/icon.png and
src-tauri/icons/icon.ico from branding/logo-art/logo-512.png, and redraws
branding/app-icon-preview.png so the artifact a reviewer looks at is never a
version behind the icon. The tray mark is a different shape for a different
surface and is never touched.
"""

import math
import pathlib

from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / "branding" / "logo-art" / "logo-512.png"
MASTER = ROOT / "branding" / "logo-art" / "app-icon-1024.png"
PREVIEW = ROOT / "branding" / "app-icon-preview.png"
ICONS = ROOT / "src-tauri" / "icons"

CANVAS = 1024
# The size src-tauri/icons/icon.png ships at, for the reason in `main`.
SHIPPED = 512
# Apple's macOS icon grid. The 200px of transparent margin is not padding to
# tighten: the Dock sizes every icon by the canvas and expects the artwork to
# stop here. Measured off a Dock screenshot, WhatsApp is 58px of body in a 72px slot.
BODY = 824
# How much of the body the head fills, on its taller axis (about 73% across).
# Set against the neighbours, not to taste: WhatsApp's glyph is 71% of its body
# and Grok's face about 90%. Past about 0.86 the headphones meet the edge.
GLYPH_SHARE = 0.78
# The squircle as a superellipse, |x/a|^n + |y/a|^n = 1. n = 5 is the standard
# approximation of the shape macOS uses, flatter sides and a longer corner sweep
# than a rounded rectangle; effective corner radius about 22% against Apple's ~22.5%.
SQUIRCLE_N = 5.0
# Draw the mask this many times oversized, then box-average it down: the
# average of an 8x mask is exact coverage, which is what keeps the edge off a
# resampling filter. See `render`.
SUPERSAMPLE = 8
# What the existing icon.ico carried, kept as-is. tauri-build compiles this
# into a Windows Resource whether or not Windows is a bundle target.
ICO_SIZES = (16, 24, 32, 48, 64, 256)


def head_box(art):
    """The box the robot head occupies in the source art.

    The head is scaled against the body, not against the source canvas, so its
    own bounds have to be found rather than assumed. It sits on a flat dark
    field, so anything far enough off the most common colour is head.
    """
    flat = max(art.getcolors(art.width * art.height))[1]
    pixels = art.load()
    box = None
    for y in range(art.height):
        for x in range(art.width):
            if sum(abs(a - b) for a, b in zip(pixels[x, y], flat)) > 18:
                box = (
                    (x, y, x + 1, y + 1)
                    if box is None
                    else (min(box[0], x), min(box[1], y), max(box[2], x + 1), max(box[3], y + 1))
                )
    return box


def squircle(size, body):
    """An alpha mask: `body` wide, centred, with continuous-curvature corners.

    Filled as a many-sided polygon at `SUPERSAMPLE` scale and averaged down. A
    polygon that fine is under a hundredth of a pixel off the curve, and the
    average leaves the edge clean without a filter's ringing.
    """
    scale = size * SUPERSAMPLE
    radius = body * SUPERSAMPLE / 2.0
    centre = scale / 2.0
    power = 2.0 / SQUIRCLE_N
    outline = []
    for step in range(2048):
        angle = step / 2048.0 * math.tau
        cos, sin = math.cos(angle), math.sin(angle)
        outline.append(
            (
                centre + math.copysign(abs(cos) ** power, cos) * radius,
                centre + math.copysign(abs(sin) ** power, sin) * radius,
            )
        )
    mask = Image.new("L", (scale, scale), 0)
    ImageDraw.Draw(mask).polygon(outline, fill=255)
    return mask.resize((size, size), Image.Resampling.BOX)


def spread(art, size, offset):
    """`art` on a `size` canvas, its field carried out to every edge.

    The pixels under the transparent margin still need a colour, or the alpha
    edge fades toward black and shows as a fringe. Stretching the outermost row
    and column outward rather than filling flat: the source's field is not
    quite even, and a flat fill leaves a faint ring where the two meet.
    """
    left, top = offset
    width, height = art.size
    right, bottom = size - left - width, size - top - height
    assert min(left, top, right, bottom) > 0, "the art overhangs the canvas"

    canvas = Image.new("RGB", (size, size))
    canvas.paste(art, offset)
    edge = Image.Resampling.NEAREST
    canvas.paste(art.crop((0, 0, 1, height)).resize((left, height), edge), (0, top))
    canvas.paste(
        art.crop((width - 1, 0, width, height)).resize((right, height), edge),
        (left + width, top),
    )
    # Full width now, so the corners come along with the rows.
    canvas.paste(canvas.crop((0, top, size, top + 1)).resize((size, top), edge), (0, 0))
    canvas.paste(
        canvas.crop((0, top + height - 1, size, top + height)).resize((size, bottom), edge),
        (0, top + height),
    )
    return canvas


def field():
    """The art on the 1024 grid, opaque everywhere, and the head size it took.

    Still a full square at this point: the squircle goes on per output size in
    `render`, over a canvas that is painted to every edge.
    """
    art = Image.open(SOURCE).convert("RGB")
    box = head_box(art)
    # 512 is the largest the art has ever existed at, so the grid render is an
    # upscale however it is cut. Lanczos over the whole canvas once, rather
    # than per output size, keeps every size derived from it consistent.
    scale = BODY * GLYPH_SHARE / max(box[2] - box[0], box[3] - box[1])
    scaled = art.resize(
        (round(art.width * scale), round(art.height * scale)),
        Image.Resampling.LANCZOS,
    )
    # Centre the head, not the source canvas: the art sits a few pixels high
    # and right of centre in logo-512.png, and the Dock shows that as a lean.
    offset = (
        round((CANVAS - (box[0] + box[2]) * scale) / 2),
        round((CANVAS - (box[1] + box[3]) * scale) / 2),
    )
    return spread(scaled, CANVAS, offset), round((box[3] - box[1]) * scale)


def render(grid, size):
    """The icon at `size`: the art resampled, then masked at that same size.

    The mask is rebuilt at each size rather than resampled along with the art,
    because Lanczos on an alpha channel rings: it widens the body by a pixel or
    two and leaves faint alpha out in the corners, and the smaller the icon the
    further that reaches. Rebuilt, every edge is exact coverage.
    """
    art = grid if size == grid.width else grid.resize((size, size), Image.Resampling.LANCZOS)
    art = art.convert("RGBA")
    art.putalpha(squircle(size, BODY * size / CANVAS))
    return art


def font(size, bold=False):
    """A system sans face, or Pillow's built-in if this is not a Mac."""
    for path in (
        f"/System/Library/Fonts/Supplemental/Arial{' Bold' if bold else ''}.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ):
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            continue
    return ImageFont.load_default(size)


def preview(icon):
    """The sheet a reviewer reads instead of taking the diff's word for it.

    The "before" is logo-512.png itself: the full-bleed square is what the
    source art still is, so the comparison needs nothing kept behind for it.
    """
    paper, ink, faint = (232, 235, 240), (28, 30, 34), (96, 100, 108)
    sheet = Image.new("RGB", (1340, 840), paper)
    draw = ImageDraw.Draw(sheet)

    big = icon.resize((400, 400), Image.Resampling.LANCZOS)
    sheet.paste(big, (56, 76), big)
    draw.text((56, 496), f"{CANVAS} × {CANVAS} · {BODY}px squircle body on Apple's macOS grid", fill=faint, font=font(19))

    slot = 240
    before = Image.open(SOURCE).convert("RGBA").resize((slot, slot), Image.Resampling.LANCZOS)
    after = icon.resize((slot, slot), Image.Resampling.LANCZOS)
    for x, title, art, note in (
        (620, "Before", before, "fills the Dock slot,\nsquare to the edge"),
        (960, "After", after, "same slot, the inset and\ncorners its neighbours keep"),
    ):
        draw.text((x, 76), title, fill=ink, font=font(26, bold=True))
        sheet.paste(art, (x, 120), art)
        draw.text((x, 120 + slot + 20), note, fill=faint, font=font(18), spacing=8)

    # Read the frames back out of the file rather than re-rendering them, so
    # the sheet shows what shipped; drawn at true size on one baseline.
    written = Image.open(ICONS / "icon.ico")
    draw.text((56, 552), " · ".join(str(s) for s in ICO_SIZES), fill=ink, font=font(26, bold=True))
    x, baseline = 56, 800
    for size in ICO_SIZES:
        frame = written.ico.getimage((size, size)).convert("RGBA")
        sheet.paste(frame, (x, baseline - size), frame)
        x += size + 28
    draw.text((x + 4, baseline - 26), "every entry in icon.ico, straight from the file", fill=faint, font=font(18))
    return sheet


def main():
    grid, head = field()
    icon = render(grid, CANVAS)

    icon.save(MASTER)
    # Shipped at 512 rather than the 1024 master because both packagers narrow
    # to it: the icns element table has no 1024-at-1x entry, and hicolor size
    # directories stop at 512. At 512 the bundler builds a valid .icns itself.
    render(grid, SHIPPED).save(ICONS / "icon.png")
    # Pillow resizes any .ico size it is not handed, so hand it all of them
    # and every frame comes off the same exact mask.
    frames = [render(grid, size) for size in ICO_SIZES]
    frames[-1].save(
        ICONS / "icon.ico",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=frames[:-1],
    )

    preview(icon).save(PREVIEW)

    print(f"{MASTER.relative_to(ROOT)}: {CANVAS}×{CANVAS}, {BODY}px body")
    print(f"head: {head}px, {head / BODY:.0%} of the body")
    print(f"{(ICONS / 'icon.png').relative_to(ROOT)}: {SHIPPED}×{SHIPPED}")
    print(f"{(ICONS / 'icon.ico').relative_to(ROOT)}: {', '.join(map(str, ICO_SIZES))}")
    print(f"{PREVIEW.relative_to(ROOT)}: redrawn")


if __name__ == "__main__":
    main()
