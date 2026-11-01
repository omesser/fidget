#!/usr/bin/env python3
"""Import a desktop pet from a standardized ecosystem into a Character Package.

A one-time authoring tool, not a build step: it translates a pet once and the
output is ours to review and hand-tune. Unlike the make-*-characters scripts
this one needs Pillow and defusedxml — petscodex ships webp sprite sheets, and
Shimeji packs ship untrusted actions.xml. It is the one script allowed to.

    python3 scripts/import-pet.py ~/.codex/pets/cat --format petscodex -o characters/cat
    python3 scripts/import-pet.py --self-test

The importer prints the pet's license, and warns when it is undeclared or not
one of the permissive ones in KNOWN_LICENSES. It warns rather than refuses:
whoever imports a pet is who ships it. Success is declared only after the
output passes
`character::load`, through the core crate's `validate` example.
"""

import argparse
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import zipfile

REPO = pathlib.Path(__file__).resolve().parent.parent

# The renderer's scale is an integer 1..=4, so it can only enlarge; art whose
# stand height overshoots the shimeji band (~100-130 logical px, the height
# BMO stands at) is resampled down at import time instead.
TARGET_STAND = 120
STAND_BAND = (100, 130)
MAX_FPS = 60

# Licenses an import may proceed on without a human weighing in: recording the
# license in the manifest's [source] discharges what they ask. SPDX ids plus the
# spellings the ecosystems print. Short on purpose: nobody here has read the rest.
KNOWN_LICENSES = (
    "MIT",
    "CC0-1.0",
    "CC0",
    "CC-BY-4.0",
    "Apache-2.0",
    "BSD-3-Clause",
)


def normalize_license(text):
    """A declared license flattened for comparison, and for nothing else.

    Case and spacing are how one license differs between packs. The
    identifier itself is not ours to guess at, so nothing else is rewritten
    and an unrecognized spelling stops for a human rather than being read
    charitably.
    """
    return "-".join(text.upper().replace("-", " ").replace("_", " ").split())


KNOWN_NORMALIZED = frozenset(normalize_license(name) for name in KNOWN_LICENSES)


def license_warning(license_line):
    """What the importer must say about this license, or `None` when the
    allowlist recognizes it and there is nothing to say.

    A warning rather than a gate. The importer cannot read a license for
    somebody, and refusing to run does not make the judgement any better
    informed — it only teaches whoever is importing to reach for the flag
    that turns the check off. Whoever imports a pet is who ships it, and the
    warning is what makes that theirs knowingly.
    """
    if not normalize_license(license_line or ""):
        return ("warning: this pack declares no license, and the source "
                "ecosystem publishes no license metadata at all. Desktop-pet "
                "packs are typically fan art of third-party characters. "
                "Shipping this art is your call to make, and yours to answer "
                "for")
    if normalize_license(license_line) not in KNOWN_NORMALIZED:
        return (f"warning: {license_line} is not one of the licenses this "
                f"importer recognizes ({', '.join(KNOWN_LICENSES)}). Read it "
                "before shipping the art — what it asks of you is yours to "
                "answer for")
    return None


def license_prose(license_line):
    """What the package will declare as `source.license`.

    The one durable record: the import prints its warning once, into a
    terminal nobody keeps, and this is what the next reader of the package —
    and the gallery's published page (#289) — finds instead. Prose because the
    interesting answers are not identifiers: "none declared anywhere" is the
    sentence that matters most and no SPDX id says it.
    """
    declared = normalize_license(license_line or "")
    if not declared:
        return (
            "MIT for these frames as cut and arranged here, along with the "
            "personality prose and the manifest. The art those frames adapt is "
            "not: it declares no license anywhere in the source ecosystem, and "
            "the importer warned so."
        )
    if declared not in KNOWN_NORMALIZED:
        return (
            f"The art it adapts declares {license_line}, which the importer "
            "does not recognize and warned about. Read it before shipping the "
            "art; this repository's license claims do not cover it."
        )
    return f"{license_line}."


# petscodex (petdex) sheets are a fixed grid of 8 columns, 192x208 cells, in
# two published layouts; integer-scaled variants are accepted. Mirrors
# petdex's src/lib/sprite-atlas.ts.
ATLAS_COLUMNS = 8
ATLAS_CELL = (192, 208)
ATLAS_LAYOUTS = ((1, 9), (2, 11))

# Default semantics from petdex's src/lib/pet-states.ts: (row, frames, duration
# ms). Extra v2 rows have no upstream names; --map assigns their reviewed roles.
PETSCODEX_ROWS = {
    "idle": (0, 6, 1100),
    "running-right": (1, 8, 1060),
    "running-left": (2, 8, 1060),
    "waving": (3, 4, 700),
    "jumping": (4, 5, 840),
    "failed": (5, 8, 1220),
    "waiting": (6, 6, 1010),
    "running": (7, 6, 820),
    "review": (8, 6, 1030),
}

# The Required Animation Set, from petdex rows: (animation, source row, frame
# indices or None for all, loop, variant_of). Walk's entry is a placeholder:
# read_petscodex picks the row. Sleep is synthesized; petdex has no sleep row.
PETSCODEX_MAP = (
    ("idle", "idle", None, "forever", None),
    ("waiting", "waiting", None, "forever", "idle"),
    ("walk", "running-left", None, "forever", None),
    ("talk", "waving", None, "forever", None),
    ("fall", "jumping", (2, 3), "forever", None),
    ("land", "jumping", (3, 4), "once", None),
    ("hold", "jumping", (1, 2), "forever", None),
    ("react", "failed", None, "once", None),
    ("sit", "review", None, "forever", None),
)

# --map keeps petdex's zero-based row numbers, including the extra v2 strips.
PETSCODEX_ROW_TABLE = {
    row: (count, duration) for row, count, duration in PETSCODEX_ROWS.values()
}
PETSCODEX_ROW_TABLE.update({9: (8, 1100), 10: (8, 1100)})


def parse_mapping(text):
    """One --map value, `animation=row[:i,j,...]`, into (animation, row,
    indices or None). petdex's row semantics are labels each generated pet
    interprets loosely — one pet's "jumping" row is a sword lunge — so the
    reviewer remaps animations to the rows this pet actually drew."""
    animation, eq, source = text.partition("=")
    row_text, _, index_text = source.partition(":")
    if not eq or not re.fullmatch(r"[a-z][a-z0-9-]*", animation):
        sys.exit(f"--map {text}: animation names use lowercase letters, digits and hyphens")
    if not row_text.isdigit() or int(row_text) not in PETSCODEX_ROW_TABLE:
        sys.exit(f"--map {text}: the source is a zero-based sheet row, 0-10")
    row = int(row_text)
    indices = None
    if ":" in source:
        try:
            indices = tuple(int(i) for i in index_text.split(","))
        except ValueError:
            sys.exit(f"--map {text}: frame indices are integers, comma-separated")
        if any(i < 0 or i >= ATLAS_COLUMNS for i in indices):
            sys.exit(f"--map {text}: frame indices are 0-{ATLAS_COLUMNS - 1}")
    return animation, row, indices


# Shimeji-ee pose durations count scheduler ticks, ~40ms each.
SHIMEJI_TICK_MS = 40

# Shimeji-ee action names to the Required Animation Set. First declared
# candidate wins; packs rarely have a talk, so talk falls back to Stand, react
# to the pinched wriggle, and sleep to the synthesized idle breath.
SHIMEJI_MAP = (
    ("idle", ("Stand",), "forever"),
    ("walk", ("Walk",), "forever"),
    ("fall", ("Falling", "Fall"), "forever"),
    ("land", ("Bouncing", "Bounce", "Landing"), "once"),
    ("sit", ("Sit", "SitDown"), "forever"),
    ("sleep", ("Sleep", "LieDown", "Lie", "Sprawl"), "forever"),
    ("hold", ("Pinched", "Grabbed", "Dragged"), "forever"),
    ("react", ("Tripping", "Trip", "Pinched"), "once"),
    ("talk", ("Wave", "Greet", "Hello", "Stand"), "forever"),
    ("climb", ("ClimbWall", "Climb"), "forever"),
)

# Many distributed packs (shimejishop's among them) are bare shime PNGs that
# drop into Shimeji-ee's standard conf. When a pack carries no actions.xml,
# these pose sequences from the standard conf stand in for it.
SHIMEJI_DEFAULT_ACTIONS = {
    "Stand": [("shime1.png", "250")],
    "Walk": [("shime1.png", "6"), ("shime2.png", "6"),
             ("shime1.png", "6"), ("shime3.png", "6")],
    "Falling": [("shime4.png", "250")],
    "Bouncing": [("shime18.png", "4"), ("shime19.png", "4")],
    "Sit": [("shime11.png", "250")],
    "Sprawl": [("shime21.png", "250")],
    "Pinched": [("shime9.png", "5"), ("shime7.png", "5"), ("shime5.png", "5"),
                ("shime1.png", "5"), ("shime6.png", "5"), ("shime8.png", "5"),
                ("shime10.png", "5")],
    "Tripping": [("shime19.png", "8"), ("shime18.png", "4"),
                 ("shime20.png", "4"), ("shime20.png", "10"),
                 ("shime19.png", "4")],
    # One climb cycle; the standard conf repeats it four times with holds.
    "ClimbWall": [("shime14.png", "16"), ("shime12.png", "4"),
                  ("shime13.png", "4"), ("shime13.png", "16"),
                  ("shime12.png", "4"), ("shime14.png", "4")],
}

# A starter life — every shipped character declares behaviors, and these name
# only required animations, so any import can carry them until hand-tuning.
STARTER_BEHAVIORS = """
# A starter life, copied from BMO's — tune it to the pet.
[behaviors.walk]
play = ["walk"]
weight = 1
when = "idle over 30s"

[behaviors.patrol]
play = ["walk", "idle", "walk"]
then = "walk"
weight = 3
when = "idle over 2m"

[behaviors.fidget]
play = ["idle"]
weight = 2
when = "idle over 10s"

[behaviors.report]
play = ["talk", "idle", "talk"]
weight = 3
when = "idle under 1m"

[behaviors.greet]
play = ["talk"]
then = "patrol"
weight = 4
when = "idle under 10s"
"""


def detect_atlas(width, height):
    """Identify a petscodex atlas from sheet dimensions: (version, rows,
    cell width, cell height), or None when no layout fits."""
    for version, rows in ATLAS_LAYOUTS:
        if width % ATLAS_COLUMNS or height % rows:
            continue
        canonical_w = ATLAS_COLUMNS * ATLAS_CELL[0]
        canonical_h = rows * ATLAS_CELL[1]
        if width * canonical_h != height * canonical_w:
            continue
        return version, rows, width // ATLAS_COLUMNS, height // rows
    return None


def fps_for(frames, duration_ms):
    """Map a row's frame count and loop duration to a manifest fps."""
    return max(1, min(MAX_FPS, round(frames * 1000 / duration_ms)))


def solid_mask(frame):
    """The pixels the loader itself will count as drawn: alpha at or above
    the hit-test threshold (`character::ALPHA_THRESHOLD` is 128). Alignment
    measures these rather than the anti-aliased halo."""
    return frame.getchannel("A").point(lambda v: 255 if v >= 128 else 0)


def defringe(frame):
    """Strip the matte halo a generator leaves around the silhouette. The
    halo is painted, not mixed — the cat's sheet keeps its fringe and even
    a one-pixel rim matte green at every alpha, so un-matting math cannot
    recover art from it. Instead, when a frame's fringe is dominated by one
    color channel, every matte-colored pixel adopts the color of its
    nearest normal opaque pixel, alpha kept, so the halo reads as more fur.
    Matte-colored art with no body nearby (a deliberate whisker tip) stays,
    and a frame with an unremarkable fringe (a hand-drawn shimeji pack, or
    one this pass already cleaned) is untouched. Colored garbage hiding
    under alpha zero is cleared on the way."""
    pixels = frame.load()
    width, height = frame.size
    fringe = [0, 0, 0, 0]
    body = [0, 0, 0, 0]
    for y in range(height):
        for x in range(width):
            r, g, b, a = pixels[x, y]
            if 0 < a < 255:
                fringe[0] += r; fringe[1] += g; fringe[2] += b; fringe[3] += 1
            elif a == 255:
                body[0] += r; body[1] += g; body[2] += b; body[3] += 1
    if fringe[3] < 50 or body[3] == 0:
        return frame
    average = [channel / fringe[3] for channel in fringe[:3]]
    dominant = average.index(max(average))
    others = [i for i in range(3) if i != dominant]
    if not all(average[dominant] > 1.6 * average[i] + 1 for i in others):
        return frame
    # A pet that is simply green wears the fringe of its own body; only a
    # tint the opaque art does not share is a matte.
    body_average = [channel / body[3] for channel in body[:3]]
    if all(body_average[dominant] > 1.3 * body_average[i] for i in others):
        return frame

    def tinted(color):
        # 1.3 catches the half-mixed matte left where fringe met art; only
        # the frame's one matte channel is ever tested, so art tinted on
        # any other channel is out of reach by construction.
        return all(color[dominant] > 1.3 * color[i] for i in others) and (
            color[dominant] > 0
        )

    def matte_like(color):
        # The floor splits the halo's bright half (recolored from a donor)
        # from its dark half (just clamped); true black outlines have no
        # dominant channel and match neither.
        return color[dominant] > 25 and tinted(color)

    ring = sorted(
        ((dx, dy) for dx in range(-4, 5) for dy in range(-4, 5) if dx or dy),
        key=lambda step: step[0] ** 2 + step[1] ** 2,
    )

    def neutralized(color, alpha):
        flat = list(color)
        flat[dominant] = max(color[i] for i in others)
        return tuple(flat) + (alpha,)

    # Writes wait until the scan ends, so a recolored pixel never donates.
    changes = []
    for y in range(height):
        for x in range(width):
            r, g, b, a = pixels[x, y]
            if a == 0:
                if r or g or b:
                    changes.append((x, y, (0, 0, 0, 0)))
                continue
            color = (r, g, b)
            if not matte_like(color):
                # A near-black pixel with the matte's tint: too dark for a
                # donor to matter, so just clamp the tint away.
                if tinted(color):
                    changes.append((x, y, neutralized(color, a)))
                continue
            for dx, dy in ring:
                nx, ny = x + dx, y + dy
                if 0 <= nx < width and 0 <= ny < height:
                    nr, ng, nb, na = pixels[nx, ny]
                    if na == 255 and not tinted((nr, ng, nb)):
                        changes.append((x, y, (nr, ng, nb, a)))
                        break
            else:
                # No body within reach — an outer fringe wisp or a whisker
                # stroke. Keep its shape, drop its tint.
                changes.append((x, y, neutralized(color, a)))
    for x, y, value in changes:
        pixels[x, y] = value
    return frame


def strip_edge_bleed(frame):
    """Remove solid fragments glued to a cell's left or right edge — the
    neighboring frame's art, inherited where a pose crosses the sheet's grid
    line. Only fragments go: a tiny share of the solid pixels, touching a
    vertical edge, disconnected from the body. A pose that itself reaches
    the edge is the largest component and stays whole."""
    from PIL import Image

    mask = solid_mask(frame)
    width, height = mask.size
    data = list(mask.getdata())
    seen = bytearray(len(data))
    components = []
    for start in range(len(data)):
        if data[start] and not seen[start]:
            seen[start] = 1
            stack = [start]
            pixels = []
            while stack:
                index = stack.pop()
                pixels.append(index)
                x, y = index % width, index // width
                for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                    if 0 <= nx < width and 0 <= ny < height:
                        neighbor = ny * width + nx
                        if data[neighbor] and not seen[neighbor]:
                            seen[neighbor] = 1
                            stack.append(neighbor)
            components.append(pixels)
    if len(components) < 2:
        return frame
    largest = max(len(pixels) for pixels in components)
    for pixels in components:
        columns = [index % width for index in pixels]
        if len(pixels) >= largest * 0.05:
            continue
        if 0 not in columns and width - 1 not in columns:
            continue
        # Clear a padded box around the fragment, taking its soft halo too.
        rows = [index // width for index in pixels]
        box = (
            max(0, min(columns) - 2),
            max(0, min(rows) - 2),
            min(width, max(columns) + 3),
            min(height, max(rows) + 3),
        )
        frame.paste(Image.new("RGBA", (box[2] - box[0], box[3] - box[1])), box[:2])
    return frame


def registration_offsets(frames):
    """Per-frame (dx, dy) drawing shifts registering every frame to the
    first, by maximum overlap of solid pixels. Generated sheets place the
    character a little differently in every cell, which plays back as
    sideways drift and vertical jitter; bbox or centroid centering would
    instead swing the body whenever a limb extends, so overlap does the
    registering — the body mass stays put and the limbs animate. The search
    is separable (best x, then y, then x again) rather than exhaustive; on
    a blob-shaped sprite the passes converge."""
    from PIL import ImageChops

    anchor = solid_mask(frames[0])
    width, height = anchor.size
    reach_x, reach_y = max(8, width // 4), max(8, height // 4)

    def registered(mask):
        def overlap(dx, dy):
            slid = mask.crop((dx, dy, dx + width, dy + height))
            return ImageChops.darker(anchor, slid).histogram()[255]

        best_x, best_y = 0, 0
        for axis in ("x", "y", "x"):
            if axis == "x":
                best_x = max(
                    range(-reach_x, reach_x + 1),
                    key=lambda s: (overlap(s, best_y), -abs(s)),
                )
            else:
                best_y = max(
                    range(-reach_y, reach_y + 1),
                    key=lambda s: (overlap(best_x, s), -abs(s)),
                )
        # Drawing at the negation moves this frame's art onto the anchor's.
        return -best_x, -best_y

    return [(0, 0)] + [registered(solid_mask(frame)) for frame in frames[1:]]


def import_scale(stand_height, target=None):
    """(resample factor, manifest scale) landing the sprite in the shimeji
    band — or at `target` logical pixels when the operator names one. Art
    already at home ships untouched at scale 1; taller art is resampled
    down; small pixel art keeps its pixels and scales up."""
    if target is not None:
        if stand_height > target:
            return target / stand_height, 1
        return 1.0, max(1, min(4, round(target / stand_height)))
    low, high = STAND_BAND
    if stand_height > high:
        return TARGET_STAND / stand_height, 1
    if stand_height < low:
        return 1.0, max(1, min(4, round(TARGET_STAND / stand_height)))
    return 1.0, 1


def render_mode(color_count, partial_alpha_fraction):
    """Drawn art anti-aliases its edges and spends colors freely; true pixel
    art has hard alpha and a small palette (ADR-0006). The bounds are loose
    on purpose: pixel art keeps translucency to a deliberate shadow (Nim's
    worst frame is 1.5% of its pixels) and a palette within an eight-bit
    indexed image's 256, while anti-aliased edges alone blow past both
    (BMO's worst frame is 3.9%, Cat's 12.7%)."""
    if partial_alpha_fraction > 0.02 or color_count > 256:
        return "smooth"
    return "pixelated"


def measure(frame):
    """The render_mode inputs, off one representative frame."""
    colors = frame.convert("RGB").getcolors(maxcolors=4096)
    color_count = len(colors) if colors else 4097
    histogram = frame.getchannel("A").histogram()
    partial = sum(histogram[1:255])
    total = sum(histogram)
    return color_count, partial / total if total else 0.0


def union_bbox(frames):
    """The bbox holding every frame's opaque pixels, or None when blank."""
    boxes = [box for box in (f.getchannel("A").getbbox() for f in frames) if box]
    if not boxes:
        return None
    return (
        min(box[0] for box in boxes),
        min(box[1] for box in boxes),
        max(box[2] for box in boxes),
        max(box[3] for box in boxes),
    )


def stillest(frames):
    """The frame differing least from the rest — the pose the animation
    keeps returning to."""
    from PIL import ImageChops

    def restlessness(i):
        return sum(
            value * count
            for other in frames
            for value, count in enumerate(
                ImageChops.difference(frames[i], other).convert("L").histogram()
            )
        )

    return frames[min(range(len(frames)), key=restlessness)]


def lifted(frame):
    """The frame shifted up one pixel — the breath a two-frame sleep needs."""
    from PIL import Image

    breath = Image.new("RGBA", frame.size, (0, 0, 0, 0))
    breath.paste(frame, (0, -1))
    return breath


def synthesized_sleep(idle_frames):
    """A sleep for sources that have none: idle's stillest frame, twice, the
    second lifted a pixel — BMO's breath, made a rule. `registered` keeps
    the per-frame grounding pass from flattening that pixel."""
    still = stillest(idle_frames)
    return {
        "frames": [still, lifted(still)],
        "fps": 1,
        "loop": "forever",
        "variant_of": None,
        "registered": True,
    }


def manifest_text(name, mode, scale, notes, source, animations, facing="right"):
    """The Character Manifest: art-production notes as comments, and where the
    art came from in `[source]`.

    Two audiences, so two places. A row mapping is for whoever re-runs the
    import; the attribution and the license are for whoever reads the gallery,
    which publishes the declared key and nothing else (#289). Before that key
    existed both lived in the leading comment and the page published the lot.
    """
    lines = [f"# {line}".rstrip() for line in notes]
    lines += [
        "",
        f'name = "{name}"',
        f'render_mode = "{mode}"',
        f"scale = {scale}",
    ]
    if facing != "right":
        lines.append(f'facing = "{facing}"')
    lines += [
        "",
        "[source]",
        # json.dumps writes a TOML basic string: same escapes, same quoting.
        f"art = {json.dumps(source['art'], ensure_ascii=False)}",
    ]
    if source["url"]:
        lines.append(f"url = {json.dumps(source['url'], ensure_ascii=False)}")
    lines += [
        f"license = {json.dumps(source['license'], ensure_ascii=False)}",
        "",
        "# Proactive model calls: after each one with no one addressing the character,",
        "# the wait becomes wait * model_base ^ model_power. Two and one doubles.",
        "",
        "[director]",
        "model_base = 2",
        "model_power = 1",
    ]
    for animation, spec in animations.items():
        lines += ["", f"[animations.{animation}]"]
        # "order" repeats written frames the way the source sequenced them
        # (a shimeji walk is 1,2,1,3); a repeated path costs no pixel budget.
        order = spec.get("order") or range(len(spec["frames"]))
        frames = ", ".join(f'"frames/{animation}-{i}.png"' for i in order)
        lines.append(f"frames = [{frames}]")
        lines.append(f"fps = {spec['fps']}")
        if spec["loop"] == "once":
            lines.append('loop = "once"')
        if spec["variant_of"]:
            lines.append(f'variant_of = "{spec["variant_of"]}"')
    turned = [
        f'"frames/{animation}-{i}.png" = "{spec["facing"]}"'
        for animation, spec in animations.items()
        if spec.get("facing", facing) != facing
        for i in range(len(spec["frames"]))
    ]
    if turned:
        lines += ["", "[frame_facing]", *turned]
    lines.append(STARTER_BEHAVIORS)
    return "\n".join(lines).rstrip() + "\n"


def mapping_notes(mappings, animations, synthesized):
    """The manifest's mapping comment, written from the mapping that cut the
    frames, so it names the rows and headings the manifest actually uses."""
    states = {row: state for state, (row, _, _) in PETSCODEX_ROWS.items()}
    parts = []
    for animation, (row, indices) in mappings.items():
        part = f"{animation}<-{states.get(row, 'row')} row {row}"
        if indices:
            part += f"[{','.join(map(str, indices))}]"
        if "facing" in animations[animation]:
            part += f" (drawn heading {animations[animation]['facing']})"
        parts.append(part)
    text = "Mapping: " + ", ".join(parts) + "."
    if synthesized:
        text += " Sleep is idle's stillest frame with a one-pixel breath."
    text += " Frames face right unless declared otherwise."
    return textwrap.wrap(text, 72, break_long_words=False, break_on_hyphens=False)


def read_petscodex(source, walk_row=2, overrides=None, only_mapped=False):
    """A petscodex pet directory (`npx petscodex install <id>` lands one at
    ~/.codex/pets/<id>/) into the importer's common shape.

    `walk_row` picks the walk art. Petdex's semantics say row 1 heads right,
    but the rows render as drawn and every pet sampled so far (cat, labubu,
    tiga, hachiware) draws row 1 heading left — so row 2 is the default, and
    the walk is declared facing the way its row is drawn. Row quality varies
    per pet (hachiware's row 2 mixes camera angles and only its row 1 walks
    cleanly), so a human eyeballs the output and re-runs with --walk-row 1.
    No code can see which way the other rows face; they take the default."""
    from PIL import Image

    pet = json.loads((source / "pet.json").read_text())
    sheet = Image.open(source / pet["spritesheetPath"]).convert("RGBA")
    atlas = detect_atlas(*sheet.size)
    if atlas is None:
        sys.exit(f"{sheet.size[0]}x{sheet.size[1]} matches no petscodex atlas layout")
    _, rows, cell_w, cell_h = atlas

    def row_frames(row, indices):
        return [
            strip_edge_bleed(
                sheet.crop(
                    (c * cell_w, row * cell_h, (c + 1) * cell_w, (row + 1) * cell_h)
                )
            )
            for c in indices
        ]

    overrides = overrides or {}
    walk_state = "running-right" if walk_row == 1 else "running-left"
    mappings = {}
    settings = {}
    for animation, state, indices, loop, variant_of in PETSCODEX_MAP:
        if animation == "walk":
            state = walk_state
        if not only_mapped:
            mappings[animation] = (PETSCODEX_ROWS[state][0], indices)
        settings[animation] = (loop, variant_of)
    mappings.update(overrides)
    if "idle" not in mappings:
        sys.exit("--only-mapped requires --map idle=ROW[:I,J,...] to set standing scale")
    animations = {}
    for animation, (row, indices) in mappings.items():
        if row >= rows:
            sys.exit(f"--map {animation}={row}: this atlas has rows 0-{rows - 1}")
        count, duration = PETSCODEX_ROW_TABLE[row]
        frames = row_frames(row, indices if indices is not None else range(count))
        loop, variant_of = settings.get(animation, ("forever", None))
        animations[animation] = {
            "frames": frames,
            "fps": fps_for(count, duration),
            "loop": loop,
            "variant_of": variant_of,
        }

    # Rows 1 and 2 are the walk rows, drawn heading left and right; a walk
    # cut from any other row (--map) faces no way the importer can see.
    drawn = {1: "left", 2: "right"}
    if "walk" in mappings and mappings["walk"][0] in drawn:
        animations["walk"]["facing"] = drawn[mappings["walk"][0]]

    if "sleep" in overrides:
        animations["sleep"]["fps"] = 1
    elif not only_mapped:
        animations["sleep"] = synthesized_sleep(animations["idle"]["frames"])

    described = pet.get("description", "").strip()
    notes = mapping_notes(mappings, animations, synthesized=not only_mapped and "sleep" not in overrides)
    if only_mapped:
        notes.append("Explicit source mappings; review animation timing and behavior in the manifest.")
    if overrides:
        remapped = ", ".join(
            f"{animation}<-row {row}"
            + (f"[{','.join(map(str, indices))}]" if indices else "")
            for animation, (row, indices) in sorted(overrides.items())
        )
        notes.append(f"Remapped for this pet's art (--map): {remapped}.")
    # A pet.json may carry the key with nothing in it, which is the same
    # as not carrying it: one meaning of "undeclared" for every reader.
    license_line = (pet.get("pet_license") or pet.get("license") or "").strip()
    return {
        "name": pet["displayName"],
        "license": license_line or None,
        "source": {
            "art": f"{pet['displayName']}, cut from the petscodex (petdex) pet "
                   f"`{pet['id']}` by scripts/import-pet.py."
                   + (f" {described}" if described else ""),
            "url": f"https://petscodex.com/pets/{pet['id']}",
        },
        "notes": notes,
        "facing": "right",
        "animations": animations,
    }


def read_shimeji(source, name=None):
    """A Shimeji-ee pack into the importer's common shape. Frames are
    per-pose PNGs (shime1.png…, at the root or under img/); pose semantics
    come from actions.xml, whose Action elements name Pose sequences — a
    richer source than frame numbering alone. Shimeji art faces left, so the
    frames are kept as drawn and the Character declares `facing = "left"`."""
    from defusedxml import DefusedXmlException
    from defusedxml import ElementTree as ET

    from PIL import Image

    frame_root = next(source.rglob("shime1.png"), None)
    if frame_root is None:
        sys.exit(f"{source}: no shime1.png anywhere — not a shimeji pack")
    frame_root = frame_root.parent
    actions_file = next(
        (p for pattern in ("actions.xml", "動作.xml") for p in source.rglob(pattern)),
        None,
    )
    if actions_file is None:
        print("no actions.xml in the pack; assuming Shimeji-ee's standard conf")
        actions = dict(SHIMEJI_DEFAULT_ACTIONS)
    else:
        # A pack is a download from the internet, so its actions.xml is
        # untrusted: defusedxml refuses entity declarations in any encoding.
        try:
            tree = ET.parse(actions_file)
        except DefusedXmlException:
            sys.exit(f"{actions_file}: declares an XML entity - refusing to parse")

        # Tags are namespaced ({http://www.group-finity.com/Mascot}Action);
        # matching on the local name keeps every dialect readable. Sequence
        # actions compose other actions and carry no poses of their own.
        def local(tag):
            return tag.rsplit("}", 1)[-1]

        actions = {}
        for element in tree.iter():
            if local(element.tag) != "Action":
                continue
            poses = [
                (pose.get("Image").lstrip("/"), pose.get("Duration"))
                for pose in element.iter()
                if local(pose.tag) == "Pose" and pose.get("Image")
            ]
            if poses:
                actions.setdefault(element.get("Name"), poses)

    opened = {}

    def frame(image):
        if image not in opened:
            opened[image] = Image.open(frame_root / image).convert("RGBA")
        return opened[image]

    animations = {}
    chosen = []
    missing = []
    for animation, candidates, loop in SHIMEJI_MAP:
        action = next((c for c in candidates if c in actions), None)
        if action is None:
            if animation not in ("sleep", "climb"):
                missing.append(f"{animation} (looked for {', '.join(candidates)})")
            continue
        poses = actions[action]
        # A pose sequence repeats images (walk is 1,2,1,3); write each
        # distinct frame once and let the manifest repeat the path, the way
        # BMO's hand-built manifest does.
        unique = list(dict.fromkeys(image for image, _ in poses))
        ticks = [int(duration) for _, duration in poses if duration]
        fps = (
            fps_for(len(poses), sum(ticks) * SHIMEJI_TICK_MS)
            if len(ticks) == len(poses)
            else 8
        )
        animations[animation] = {
            "frames": [frame(image) for image in unique],
            "order": [unique.index(image) for image, _ in poses],
            "fps": fps,
            "loop": loop,
            "variant_of": None,
        }
        chosen.append(f"{animation}<-{action}")
    if missing:
        where = actions_file.name if actions_file else "the standard conf"
        sys.exit(f"{where} declares no usable " + "; no ".join(missing))

    if "sleep" not in animations:
        animations["sleep"] = synthesized_sleep(animations["idle"]["frames"])
        chosen.append("sleep<-synthesized from idle")
    print("mapped:", ", ".join(chosen))

    name = name or source.name
    notes = [
        "Shimeji art faces left, so the frames are kept as drawn and the",
        "Character declares facing = \"left\"; the renderer mirrors them",
        "for rightward travel.",
        "Mapping (animation <- this pack's actions.xml action):",
        ", ".join(chosen) + ".",
    ]
    return {
        "name": name,
        "license": None,
        "source": {
            "art": f"{name}, cut from a Shimeji-ee pack by scripts/import-pet.py.",
            "url": None,
        },
        "notes": notes,
        "facing": "left",
        "animations": animations,
    }


FORMATS = ("petscodex", "shimeji")


def emit(pet, out, validate=True, stand=None, frames_only=False):
    """Write aligned frames and an optional baseline manifest, then validate the package."""
    from PIL import Image

    animations = pet["animations"]
    fixed_floor = {}
    for animation, spec in animations.items():
        bounds = [
            solid_mask(frame).getbbox() or frame.getchannel("A").getbbox()
            for frame in spec["frames"]
        ]
        fixed_floor[animation] = all(bounds) and len({box[3] for box in bounds}) == 1
    for spec in animations.values():
        spec["frames"] = [defringe(frame) for frame in spec["frames"]]
    unions = {}
    for animation, spec in animations.items():
        union = union_bbox(spec["frames"])
        if union is None:
            sys.exit(f"{animation}: every frame is fully transparent")
        unions[animation] = union

    # One resample factor for the whole character, from how tall it stands
    # idle, so every animation keeps the same proportions.
    idle = unions["idle"]
    factor, scale = import_scale(idle[3] - idle[1], stand)
    mode = render_mode(*measure(animations["idle"]["frames"][0]))
    if factor != 1.0:
        for spec in animations.values():
            resized = []
            for frame in spec["frames"]:
                size = (round(frame.width * factor), round(frame.height * factor))
                if mode == "smooth":
                    # Resample premultiplied: with straight alpha, the RGB
                    # of invisible pixels bleeds into the resampled edge.
                    resized.append(
                        frame.convert("RGBa")
                        .resize(size, Image.Resampling.LANCZOS)
                        .convert("RGBA")
                    )
                else:
                    resized.append(frame.resize(size, Image.Resampling.NEAREST))
            spec["frames"] = resized
        if mode == "pixelated":
            # The ADR-0006 quantisation pass: a resample drifts the palette,
            # so pin it back down. Untouched pixel art keeps its own.
            for spec in animations.values():
                spec["frames"] = [
                    frame.quantize(colors=64).convert("RGBA")
                    for frame in spec["frames"]
                ]
        unions = {a: union_bbox(spec["frames"]) for a, spec in animations.items()}

    # Solid feet set the floor; an antialiasing halo must not make them hover.
    # Preserve a source row's planted baseline, or its registered vertical motion.
    FLOAT_TOLERANCE = 6
    metrics = {}
    for animation, spec in animations.items():
        boxes = [frame.getchannel("A").getbbox() for frame in spec["frames"]]
        if None in boxes:
            sys.exit(f"{animation}: frame {boxes.index(None)} is fully transparent")
        feet = [solid_mask(frame).getbbox() or box for frame, box in zip(spec["frames"], boxes)]
        offsets = (
            [(0, 0)] * len(boxes)
            if spec.get("registered")
            else registration_offsets(spec["frames"])
        )
        ground = max(box[3] + dy for box, (_, dy) in zip(feet, offsets))
        lifts = []
        for box, (_, dy) in zip(feet, offsets):
            lift = ground - (box[3] + dy)
            lifts.append(lift if not fixed_floor[animation] and lift <= FLOAT_TOLERANCE else 0)
        left = min(box[0] + dx for box, (dx, _) in zip(boxes, offsets))
        right = max(box[2] + dx for box, (dx, _) in zip(boxes, offsets))
        height = max(
            foot[3] - box[1] + lift for box, foot, lift in zip(boxes, feet, lifts)
        )
        metrics[animation] = (feet, offsets, lifts, right - left, left, height)

    canvas = (
        max(width for *_, width, _, _ in metrics.values()),
        max(height for *_, height in metrics.values()),
    )
    if frames_only:
        authored = tomllib.loads((out / "character.manifest").read_text())
        referenced = {frame for animation in authored["animations"].values()
                      for frame in animation["frames"]}
        for animation, spec in animations.items():
            expected = {frame for frame in referenced
                        if re.fullmatch(rf"frames/{re.escape(animation)}-\d+\.png", frame)}
            selected = {f"frames/{animation}-{i}.png" for i in range(len(spec["frames"]))}
            if (expected and selected != expected) or (not expected and animation in authored["animations"]):
                sys.exit(f"{animation}: recut frame files differ from the authored manifest "
                         f"({len(selected)} selected, {len(expected)} referenced); "
                         "edit the manifest first or import a new baseline package")
            if not expected:
                print(f"warning: new strip {animation!r} is not referenced by the manifest; "
                      "edit the manifest to use these frames", file=sys.stderr)
        reference = out / authored["animations"]["idle"]["frames"][0]
        with Image.open(reference) as image:
            existing_canvas = image.size
        if canvas[0] > existing_canvas[0] or canvas[1] > existing_canvas[1]:
            sys.exit(f"selected frames need {canvas}, exceeding existing canvas {existing_canvas}; "
                     "import a new baseline package to change its canvas")
        canvas = existing_canvas
    frames_dir = out / "frames"
    frames_dir.mkdir(parents=True, exist_ok=frames_only)
    for animation, spec in animations.items():
        boxes, offsets, lifts, width, left, _ = metrics[animation]
        centering = (canvas[0] - width) // 2 - left
        for i, frame in enumerate(spec["frames"]):
            page = Image.new("RGBA", canvas, (0, 0, 0, 0))
            dx = centering + offsets[i][0]
            dy = canvas[1] - boxes[i][3] - lifts[i]
            page.paste(frame, (dx, dy))
            page.save(frames_dir / f"{animation}-{i}.png")

    if not frames_only:
        source = dict(pet["source"], license=license_prose(pet["license"]))
        facing = pet.get("facing", "right")
        manifest = manifest_text(
            pet["name"], mode, scale, pet["notes"], source, animations, facing
        )
        (out / "character.manifest").write_text(manifest)

    print(f"{pet['name']}: {sum(len(s['frames']) for s in animations.values())} "
          f"frames on a {canvas[0]}x{canvas[1]} canvas, {mode} at scale {scale}")
    if validate:
        done = subprocess.run(
            ["cargo", "run", "-q", "-p", "fidget-core", "--example", "validate",
             "--", str(out)],
            cwd=REPO,
        )
        if done.returncode != 0:
            sys.exit("character::load rejected the output")


def facing_summary(pet):
    """The facing the manifest declares, as the importer prints it."""
    default = pet.get("facing", "right")
    turned = sorted(
        f"{animation} {spec['facing']}"
        for animation, spec in pet["animations"].items()
        if spec.get("facing", default) != default
    )
    return f'facing = "{default}"' + (
        f"; [frame_facing] {', '.join(turned)}" if turned else ""
    )


def require_pillow():
    """A missing Pillow is a setup problem, and gets setup instructions
    rather than a traceback."""
    try:
        import PIL  # noqa: F401 -- the import is itself the check
    except ModuleNotFoundError:
        sys.exit(
            "Pillow is not importable from this Python. Set up the venv and "
            "run the importer with it:\n"
            "  uv venv --python 3.11 && uv pip install pillow\n"
            "  uv run scripts/import-pet.py ..."
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", nargs="?", help="pet directory or zip")
    parser.add_argument("--format", choices=FORMATS, dest="ecosystem")
    parser.add_argument("-o", "--out", type=pathlib.Path)
    parser.add_argument("--walk-row", type=int, choices=(1, 2), default=2,
                        help="petscodex: the sheet row to cut walk from. Row 2 "
                             "(petdex's running-left) is drawn heading right in "
                             "every pet sampled, so it is the default")
    parser.add_argument("--map", action="append", default=[], dest="mappings",
                        metavar="ANIM=ROW[:I,J,...]",
                        help="petscodex: map any animation name to a zero-based "
                             "row and optional frame indices, e.g. --map "
                             "thinking=7 --map look-left=10:0,1,2,1")
    parser.add_argument("--only-mapped", action="store_true",
                        help="petscodex: import exactly the --map strips, "
                             "including idle for standing scale, without default mappings")
    parser.add_argument("--frames-only", action="store_true",
                        help="recut frames in an existing package; preserve the "
                             "manifest, personality, and other files")
    parser.add_argument("--stand", type=int,
                        help="how tall the sprite stands on screen, in "
                             "logical pixels (default: the shimeji band, "
                             "100-130)")
    parser.add_argument("--force", action="store_true",
                        help="replace an existing output directory")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    require_pillow()
    if args.self_test:
        return self_test()
    if not (args.source and args.ecosystem and args.out):
        parser.error("source, --format and -o are required")
    if args.ecosystem != "petscodex" and (args.mappings or args.only_mapped):
        parser.error("--map and --only-mapped require --format petscodex")
    if args.frames_only:
        if args.force:
            parser.error("--frames-only preserves the package; do not combine it with --force")
        if not (args.out / "character.manifest").is_file():
            parser.error("--frames-only requires an existing character.manifest in -o")

    source = pathlib.Path(args.source).expanduser()
    if zipfile.is_zipfile(source):
        unpacked = tempfile.mkdtemp(prefix="import-pet-")
        zipfile.ZipFile(source).extractall(unpacked)
        source = pathlib.Path(unpacked)
    if args.ecosystem == "petscodex":
        overrides = {}
        for text in args.mappings:
            animation, row, indices = parse_mapping(text)
            overrides[animation] = (row, indices)
        pet = read_petscodex(source, args.walk_row, overrides, args.only_mapped)
    else:
        # A zip unpacks to a temp directory, so the pack's own name comes
        # from the path the user gave, not from where it landed.
        pet = read_shimeji(source, pathlib.Path(args.source).expanduser().stem)

    print(f"license: {pet['license'] or 'none declared'}")
    warning = license_warning(pet["license"])
    if warning:
        print(warning, file=sys.stderr)

    if args.out.exists() and not args.frames_only:
        if not args.force:
            sys.exit(f"{args.out} exists; --force replaces it")
        shutil.rmtree(args.out)
    if args.frames_only:
        # Validate the whole package before touching the author's existing assets.
        with tempfile.TemporaryDirectory(prefix="fidget-recut-") as scratch:
            staged = pathlib.Path(scratch) / "package"
            shutil.copytree(args.out, staged)
            emit(pet, staged, stand=args.stand, frames_only=True)
            for animation, spec in pet["animations"].items():
                for index in range(len(spec["frames"])):
                    name = f"{animation}-{index}.png"
                    shutil.copy2(staged / "frames" / name, args.out / "frames" / name)
        print("frames recut; authored manifest, personality and other files preserved. "
              "Review the existing scale, render mode, animation timing and behaviors.")
        print(f"the frames are as drawn, which a fresh import declares as "
              f"{facing_summary(pet)}; check the authored facing matches")
        return
    emit(pet, args.out, stand=args.stand)
    print(f"declared: {facing_summary(pet)}")
    if args.ecosystem == "petscodex":
        print("review the output before shipping it: every frame must face "
              "the way the manifest declares (--walk-row picks the other walk "
              "row; edit facing or [frame_facing] for the rest), and every "
              "animation must read as its name — a pet whose art strays from "
              "the row semantics is recut with --map")
    else:
        print("review the output before shipping it — every frame must face "
              "the way the manifest declares")
    print("no personality.txt is written: author one to fit the art, the way "
          "the shipped characters' read")
    print(f"run it: FIDGET_CHARACTER={args.out.name} cargo run — the next "
          "build refreshes the copy beside the binary; "
          f"FIDGET_CHARACTERS={args.out.parent.resolve()} skips the rebuild")


def self_test():
    """The pure seams, then a synthetic sheet through the whole pipeline."""
    from PIL import Image

    assert detect_atlas(1536, 1872) == (1, 9, 192, 208)
    assert detect_atlas(3072, 3744) == (1, 9, 384, 416)
    assert detect_atlas(1536, 2288) == (2, 11, 192, 208)
    assert detect_atlas(1537, 1872) is None
    assert detect_atlas(100, 100) is None

    # The license check, which warns and never stops: the only thing that
    # reaches the person importing is what it returns.
    assert license_warning("CC0-1.0") is None
    assert license_warning("cc by 4.0") is None, "spelling varies"
    assert license_warning("MIT") is None
    assert "All Rights Reserved" in license_warning("All Rights Reserved")
    assert license_warning(None) == license_warning(""), "blank is undeclared"
    assert license_warning("   ") == license_warning(None)

    # The license the package declares. An import that found none says so in
    # the field the gallery publishes, rather than saying nothing.
    assert license_prose("CC0-1.0") == "CC0-1.0."
    assert "declares no license anywhere" in license_prose(None)
    assert "All Rights Reserved" in license_prose("All Rights Reserved")
    assert license_prose("   ") == license_prose(None)

    assert fps_for(6, 1100) == 5
    assert fps_for(8, 1060) == 8
    assert fps_for(4, 700) == 6
    assert fps_for(5, 840) == 6
    assert fps_for(8, 1220) == 7
    assert fps_for(1000, 1000) == MAX_FPS
    assert fps_for(1, 100000) == 1

    factor, scale = import_scale(174)
    assert scale == 1 and abs(factor - 120 / 174) < 1e-9
    assert import_scale(128) == (1.0, 1)
    assert import_scale(32) == (1.0, 4)
    assert import_scale(60) == (1.0, 2)
    factor, scale = import_scale(174, 100)
    assert scale == 1 and abs(factor - 100 / 174) < 1e-9
    assert import_scale(32, 100) == (1.0, 3)

    assert render_mode(4097, 0.3) == "smooth"
    assert render_mode(16, 0.0) == "pixelated"

    assert parse_mapping("sleep=6:3") == ("sleep", 6, (3,))
    assert parse_mapping("react=7") == ("react", 7, None)
    assert parse_mapping("look-left=10") == ("look-left", 10, None)
    assert parse_mapping("idle=0:0,1,2,3,4,5,6") == ("idle", 0, tuple(range(7)))
    for invalid in ("../idle=0", "idle=11", "idle=-1", "idle=0:8", "idle=0:"):
        try:
            parse_mapping(invalid)
        except SystemExit:
            pass
        else:
            raise AssertionError(f"accepted invalid mapping: {invalid}")

    # Defringe: a red square wearing a green halo — the halo adopts the
    # body's red, a far green wisp survives, hidden matte is cleared.
    matted = Image.new("RGBA", (30, 30), (0, 255, 0, 0))
    for y in range(8, 22):
        for x in range(8, 22):
            matted.putpixel((x, y), (200, 0, 0, 255))
    for i in range(8, 22):
        for edge in ((i, 7), (i, 22), (7, i), (22, i)):
            matted.putpixel(edge, (20, 110, 20, 128))
    matted.putpixel((2, 2), (20, 110, 20, 200))
    matted.putpixel((26, 26), (0, 20, 0, 255))
    cleaned = defringe(matted)
    assert cleaned.getpixel((5, 5)) == (0, 0, 0, 0)
    assert cleaned.getpixel((10, 7)) == (200, 0, 0, 128)
    # A wisp too far from the body keeps its shape and loses its tint.
    assert cleaned.getpixel((2, 2)) == (20, 20, 20, 200)
    assert cleaned.getpixel((26, 26)) == (0, 0, 0, 255)

    # A pet that is simply green keeps its own colors: the fringe shares the
    # body's hue, so nothing reads as a matte.
    frog = Image.new("RGBA", (30, 30), (0, 0, 0, 0))
    for y in range(8, 22):
        for x in range(8, 22):
            frog.putpixel((x, y), (30, 180, 40, 255))
    for i in range(8, 22):
        for edge in ((i, 7), (i, 22), (7, i), (22, i)):
            frog.putpixel(edge, (25, 140, 30, 128))
    kept = defringe(frog)
    assert kept.getpixel((10, 7)) == (25, 140, 30, 128)

    # A faint halo is not floor contact. A moving body over planted feet
    # must keep that motion rather than being registered onto the first body.
    from PIL import ImageDraw
    planted = Image.new("RGBA", (24, 24))
    draw = ImageDraw.Draw(planted)
    draw.rectangle((7, 5, 16, 17), fill=(150, 80, 30, 255))
    draw.rectangle((7, 18, 10, 20), fill=(150, 80, 30, 255))
    draw.rectangle((13, 18, 16, 20), fill=(150, 80, 30, 255))
    draw.rectangle((7, 21, 16, 23), fill=(150, 80, 30, 8))
    bobbed = planted.copy()
    ImageDraw.Draw(bobbed).rectangle((7, 4, 16, 4), fill=(150, 80, 30, 255))
    sample = {
        "name": "Grounding sample", "license": "CC0", "notes": [],
        "source": {"art": "synthetic grounding fixture", "url": None},
        "animations": {"idle": {
            "frames": [planted, bobbed], "fps": 2, "loop": "forever", "variant_of": None,
        }},
    }
    with tempfile.TemporaryDirectory() as tmp:
        emit(sample, pathlib.Path(tmp), validate=False)
        grounded = [Image.open(path) for path in sorted(pathlib.Path(tmp).glob("frames/*.png"))]
        bounds = [solid_mask(frame).getbbox() for frame in grounded]
        assert all(box[3] == frame.height for box, frame in zip(bounds, grounded)), bounds
        assert bounds[0][1] - bounds[1][1] == 1, "planted-foot body motion is preserved"
        rising = Image.new("RGBA", planted.size)
        rising.paste(planted, (0, -2))
        sample["animations"]["idle"].update(frames=[planted, rising], registered=True)
        lifted_out = pathlib.Path(tmp) / "lifted"
        emit(sample, lifted_out, validate=False)
        lifted_bounds = [
            solid_mask(Image.open(path)).getbbox()
            for path in sorted(lifted_out.glob("frames/*.png"))
        ]
        assert lifted_bounds[0][3] - lifted_bounds[1][3] == 2, "authored vertical motion survives"

    # A synthetic v1 sheet, 8x8 cells: each declared frame is a 2x2 dot,
    # deliberately jittered per frame the way generated sheets are, so the
    # grounding and registration passes have something to undo.
    cell = 8
    sheet = Image.new("RGBA", (ATLAS_COLUMNS * cell, 9 * cell), (0, 0, 0, 0))
    for state, (row, count, _) in PETSCODEX_ROWS.items():
        for c in range(count):
            x = c * cell + 3 + (c % 3)
            y = row * cell + 2 + (row % 3) + (c % 2)
            for dx in range(2):
                for dy in range(2):
                    sheet.putpixel((x + dx, y + dy), (200, 10 + row, 10, 255))
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        source = tmp / "pet"
        source.mkdir()
        (source / "pet.json").write_text(json.dumps({
            "id": "dot", "displayName": "Dot", "description": "a test dot",
            "spritesheetPath": "sheet.png",
        }))
        # detect_atlas refuses square cells; stretch to the canonical grid.
        tall = sheet.resize((ATLAS_COLUMNS * 192, 9 * 208), Image.Resampling.NEAREST)
        tall.save(source / "sheet.png")

        pet = read_petscodex(source)
        assert set(pet["animations"]) == {
            "idle", "waiting", "walk", "talk", "fall", "land", "hold",
            "react", "sit", "sleep",
        }
        assert pet["animations"]["land"]["loop"] == "once"
        assert pet["animations"]["waiting"]["variant_of"] == "idle"
        assert pet["license"] is None

        # --walk-row swaps the walk source row (the synthetic sheet colors
        # each row differently, so the swap shows in the pixels), and the
        # walk is declared facing the way its row is drawn.
        other = read_petscodex(source, walk_row=1)
        walk = pet["animations"]["walk"]["frames"][0]
        assert walk.tobytes() != other["animations"]["walk"]["frames"][0].tobytes()
        assert facing_summary(pet) == 'facing = "right"'
        assert facing_summary(other) == 'facing = "right"; [frame_facing] walk left'
        emit(other, tmp / "row-1", validate=False)
        declared = tomllib.loads((tmp / "row-1" / "character.manifest").read_text())
        assert "facing" not in declared
        assert declared["frame_facing"] == {
            f"frames/walk-{i}.png": "left"
            for i in range(len(other["animations"]["walk"]["frames"]))
        }
        # A walk remapped onto a walk row reads that row's heading, and the
        # mapping comment names the row the frames were cut from.
        remapped = read_petscodex(source, overrides={"walk": (1, None)})
        assert remapped["animations"]["walk"]["facing"] == "left"
        noted = " ".join(remapped["notes"])
        assert "walk<-running-right row 1 (drawn heading left)" in noted, noted
        assert "row 2" not in noted, noted
        noted = " ".join(read_petscodex(source, overrides={"walk": (7, None)})["notes"])
        assert "walk<-running row 7," in noted and "heading" not in noted, noted

        out = tmp / "character"
        emit(pet, out, validate=False)
        manifest = (out / "character.manifest").read_text()
        for required in ("idle", "walk", "fall", "land", "sit", "sleep",
                         "react", "talk", "hold"):
            assert f"[animations.{required}]" in manifest
        assert "[director]" in manifest
        assert "model_base = 2" in manifest
        assert "model_power = 1" in manifest
        # A personality is authored, never derived from the pet description.
        assert not (out / "personality.txt").exists()

        # An imported package declares where its art came from, because the
        # gallery publishes that key and publishes nothing else. The
        # license sentence is the part an import must never leave to silence.
        declared = tomllib.loads(manifest)["source"]
        assert "a test dot" in declared["art"]
        assert declared["url"] == "https://petscodex.com/pets/dot"
        assert "declares no license anywhere" in declared["license"]
        assert "Mapping:" not in declared["art"], "art notes stay in the comments"
        assert "# Mapping:" in manifest

        # Grounding and registration: every frame of every animation lands
        # its dot on the canvas bottom at one x — the jitter baked into the
        # sheet is gone, so nothing drifts inside a cycle.
        for animation in set(pet["animations"]) - {"sleep"}:
            placed = {
                Image.open(path).getchannel("A").getbbox()
                for path in sorted(out.glob(f"frames/{animation}-*.png"))
            }
            assert len(placed) == 1, f"{animation} drifts: {placed}"
            frame = Image.open(out / "frames" / f"{animation}-0.png")
            assert frame.getchannel("A").getbbox()[3] == frame.height, animation
        # The sleep breath survives the grounding pass: frame 1 keeps its
        # deliberate one-pixel lift over a planted frame 0.
        sleep0 = Image.open(out / "frames" / "sleep-0.png").getchannel("A").getbbox()
        sleep1 = Image.open(out / "frames" / "sleep-1.png").getchannel("A").getbbox()
        assert sleep0[3] - sleep1[3] == 1

        # --map cuts an animation from the named row: row 5's dots carry
        # row 5's color.
        remapped = read_petscodex(source, overrides={"hold": (5, (0, 1))})
        hold = remapped["animations"]["hold"]["frames"][0]
        assert (200, 15, 10, 255) in {color for _, color in hold.getcolors()}
        assert len(remapped["animations"]["hold"]["frames"]) == 2

        # An agent can recut reviewed strips without resetting authored behavior.
        manifest_before = (out / "character.manifest").read_bytes()
        (out / "personality.txt").write_text("An authored prompt.")
        (out / "frames" / "hand-edited.png").write_bytes(b"untouched")
        emit(read_petscodex(source), out, validate=False, frames_only=True)
        assert (out / "character.manifest").read_bytes() == manifest_before
        assert (out / "personality.txt").read_text() == "An authored prompt."
        assert (out / "frames" / "hand-edited.png").read_bytes() == b"untouched"
        with Image.open(out / "frames" / "idle-0.png") as image:
            original_canvas = image.size
        original_idle = (out / "frames" / "idle-0.png").read_bytes()
        idle_only = read_petscodex(source, overrides={"idle": (0, None)}, only_mapped=True)
        emit(idle_only, out, validate=False, frames_only=True)
        with Image.open(out / "frames" / "idle-0.png") as image:
            assert image.size == original_canvas, "partial recuts retain the package canvas"
        assert (out / "frames" / "idle-0.png").read_bytes() == original_idle
        before_recut = {path: path.read_bytes() for path in out.rglob("*") if path.is_file()}
        for indices in ((0, 1, 2), (*range(6), 0)):
            mismatched = read_petscodex(source, overrides={"idle": (0, indices)}, only_mapped=True)
            try:
                emit(mismatched, out, validate=False, frames_only=True)
            except SystemExit as error:
                assert "recut frame files differ" in str(error)
            else:
                raise AssertionError("a recut must agree with the authored strip's frame files")
            assert {path: path.read_bytes() for path in out.rglob("*") if path.is_file()} == before_recut
        repeated = manifest_before.decode().replace('"frames/idle-1.png"',
                                                   '"frames/idle-1.png", "frames/idle-1.png"')
        (out / "character.manifest").write_text(repeated)
        emit(read_petscodex(source, overrides={"idle": (0, None)}, only_mapped=True),
             out, validate=False, frames_only=True)
        assert (out / "character.manifest").read_text() == repeated
        (out / "character.manifest").write_bytes(manifest_before)
        from contextlib import redirect_stderr
        from io import StringIO
        warning = StringIO()
        added = read_petscodex(source, overrides={"idle": (0, None), "extra": (6, None)},
                              only_mapped=True)
        with redirect_stderr(warning):
            emit(added, out, validate=False, frames_only=True)
        assert "new strip 'extra' is not referenced" in warning.getvalue()
        assert len(list((out / "frames").glob("extra-*.png"))) == 6
        assert (out / "character.manifest").read_bytes() == manifest_before

        extended = Image.new("RGBA", (ATLAS_COLUMNS * 192, 11 * 208))
        for row in range(11):
            for col in range(ATLAS_COLUMNS):
                extended.paste((30 + row, 80 + col, 100, 255),
                               (col * 192 + 20, row * 208 + 20,
                                col * 192 + 40, row * 208 + 50))
        extended.save(source / "sheet.png")
        mappings = {}
        for text in ("idle=0:0,1,2,3,4,5,6", "thinking=7", "look-left=10:0,1,2,1"):
            animation, row, indices = parse_mapping(text)
            mappings[animation] = (row, indices)
        selected = read_petscodex(source, overrides=mappings, only_mapped=True)
        assert set(selected["animations"]) == {"idle", "thinking", "look-left"}
        assert len(selected["animations"]["idle"]["frames"]) == 7
        looks = selected["animations"]["look-left"]["frames"]
        assert len(looks) == 4
        assert looks[1].tobytes() == looks[3].tobytes(), "explicit playback order survives"
        assert (40, 82, 100, 255) in {color for _, color in looks[2].getcolors()}
        extra = read_petscodex(source, overrides={"thinking": (7, None)})
        assert {"walk", "sleep", "thinking"} <= set(extra["animations"])
        try:
            read_petscodex(source, overrides={"thinking": (7, None)}, only_mapped=True)
        except SystemExit as error:
            assert "requires --map idle" in str(error)
        else:
            raise AssertionError("explicit strips need an idle standing reference")
        missing = tmp / "missing-package"
        rejected = subprocess.run(
            [sys.executable, __file__, str(source), "--format", "petscodex",
             "-o", str(missing), "--frames-only"],
            capture_output=True, text=True,
        )
        assert rejected.returncode != 0 and "existing character.manifest" in rejected.stderr
        assert not missing.exists(), "a rejected recut must not create output"
        tall.save(source / "sheet.png")
        try:
            read_petscodex(source, overrides={"look-left": (10, None)})
        except SystemExit as error:
            assert "this atlas has rows 0-8" in str(error)
        else:
            raise AssertionError("a v1 sheet cannot supply a v2 row")

    # A synthetic shimeji pack: three 8x8 frames with one marker pixel near
    # the left edge, and an actions.xml with no lie-down and no wave, so the
    # sleep synthesis and the talk<-Stand fallback both run.
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        (tmp / "img").mkdir()
        (tmp / "conf").mkdir()
        for n in (1, 2, 3):
            shime = Image.new("RGBA", (8, 8), (0, 0, 0, 0))
            shime.putpixel((1, 4 + n % 2), (10 * n, 0, 0, 255))
            shime.save(tmp / "img" / f"shime{n}.png")
        (tmp / "conf" / "actions.xml").write_text("""<?xml version="1.0"?>
<Mascot xmlns="http://www.group-finity.com/Mascot"><ActionList>
<Action Name="Stand" Type="Stay"><Animation><Pose Image="/shime1.png" Duration="250"/></Animation></Action>
<Action Name="Walk" Type="Move"><Animation>
  <Pose Image="/shime1.png" Duration="6"/><Pose Image="/shime2.png" Duration="6"/>
  <Pose Image="/shime1.png" Duration="6"/><Pose Image="/shime3.png" Duration="7"/>
</Animation></Action>
<Action Name="Falling" Type="Embedded"><Animation><Pose Image="/shime2.png" Duration="250"/></Animation></Action>
<Action Name="Bouncing" Type="Animate"><Animation><Pose Image="/shime3.png" Duration="4"/><Pose Image="/shime1.png" Duration="4"/></Animation></Action>
<Action Name="Sit" Type="Stay"><Animation><Pose Image="/shime2.png" Duration="250"/></Animation></Action>
<Action Name="Pinched" Type="Embedded"><Animation><Pose Image="/shime3.png" Duration="5"/></Animation></Action>
<Action Name="Tripping" Type="Animate"><Animation><Pose Image="/shime2.png" Duration="8"/><Pose Image="/shime3.png" Duration="8"/></Animation></Action>
<Action Name="Fall" Type="Sequence"/>
</ActionList></Mascot>""")

        pack = read_shimeji(tmp, "Tester")
        assert set(pack["animations"]) == {
            "idle", "walk", "fall", "land", "sit", "sleep", "react", "talk",
            "hold",
        }, set(pack["animations"])
        # Kept as drawn: the marker pixel stays at x=1, and the Character
        # declares the left-facing art instead.
        idle = pack["animations"]["idle"]["frames"][0]
        assert idle.getpixel((1, 5))[3] == 255 and idle.getpixel((6, 5))[3] == 0
        assert facing_summary(pack) == 'facing = "left"'
        # Walk repeats shime1 (poses 1,2,1,3 over two written frames of it);
        # 25 ticks of 40ms are one second, so four poses make fps 4.
        walk = pack["animations"]["walk"]
        assert len(walk["frames"]) == 3 and walk["order"] == [0, 1, 0, 2]
        assert walk["fps"] == 4
        assert pack["animations"]["land"]["loop"] == "once"
        assert pack["animations"]["react"]["loop"] == "once"
        # No lie-down action: sleep is the synthesized idle-breath pair.
        assert len(pack["animations"]["sleep"]["frames"]) == 2

        out = tmp / "character"
        emit(pack, out, validate=False)
        manifest = (out / "character.manifest").read_text()
        assert manifest.count('"frames/walk-') == 4  # the repeated path
        assert tomllib.loads(manifest)["facing"] == "left"
        assert "[frame_facing]" not in manifest
        assert (out / "frames" / "walk-2.png").exists()
        assert not (out / "frames" / "walk-3.png").exists()

        # Untrusted pack XML: an entity declaration is refused, not expanded,
        # in UTF-16 too, where a scan for the ASCII bytes would miss it.
        bomb = (
            '<!DOCTYPE Mascot [<!ENTITY a "aa">]>'
            '<Mascot><ActionList><Action Name="&a;"/></ActionList></Mascot>'
        )
        for encoding in ("utf-8", "utf-16"):
            (tmp / "conf" / "actions.xml").write_bytes(
                f'<?xml version="1.0" encoding="{encoding}"?>{bomb}'.encode(encoding)
            )
            try:
                read_shimeji(tmp, "Bomb")
            except SystemExit as refused:
                assert "entity" in str(refused), refused
            else:
                raise AssertionError(f"a {encoding} entity declaration was parsed")

    # A pack of bare shime PNGs and no actions.xml — shimejishop distributes
    # these — rides Shimeji-ee's standard conf instead.
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        named = {
            image
            for poses in SHIMEJI_DEFAULT_ACTIONS.values()
            for image, _ in poses
        }
        for image in named:
            shime = Image.new("RGBA", (8, 8), (0, 0, 0, 0))
            shime.putpixel((2, 6), (99, 0, 0, 255))
            shime.save(tmp / image)
        pack = read_shimeji(tmp, "Bare")
        assert "climb" in pack["animations"]
        assert pack["animations"]["sleep"]["variant_of"] is None
        assert len(pack["animations"]["hold"]["frames"]) == len(
            {image for image, _ in SHIMEJI_DEFAULT_ACTIONS["Pinched"]}
        )

    print("self-test: ok")


if __name__ == "__main__":
    main()
