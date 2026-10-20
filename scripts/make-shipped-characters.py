#!/usr/bin/env python3
"""Draw Nim, the script-drawn shipped Character.

Nim is modern pixel art — a shaded ramp lit from the upper left, a translucent
contact shadow, and enough in-between frames that the motion reads as smooth
rather than stepped. BMO is not drawn here: its frames are static art cut from
a game sprite sheet and quantised to a fixed palette (see
characters/bmo/character.manifest), so the two shipped Characters still prove
the format against real variance — one generated, one drawn by hand elsewhere.

Pure standard library: a build step that needs Pillow installed is a build step
that stops working.

    python3 scripts/make-shipped-characters.py

It rewrites characters/nim/frames/ in place. The manifests, the Personality
Prompts, and all of characters/bmo/ are written by hand and never touched.
"""

import math
import pathlib
import struct
import zlib

SIZE = 32
# The bottom row is where the Character's feet are: `place_sprite` hangs the art
# a whole height above the contact point, so art that stops short of row 31
# floats above the floor by however much it stopped short by.
GROUND = SIZE - 1
ROOT = pathlib.Path(__file__).resolve().parent.parent / "characters"

CLEAR = (0, 0, 0, 0)


def blank():
    return [[CLEAR] * SIZE for _ in range(SIZE)]


def put(px, x, y, color):
    if 0 <= x < SIZE and 0 <= y < SIZE:
        px[y][x] = color


def rect(px, x0, y0, w, h, color):
    for y in range(y0, y0 + h):
        for x in range(x0, x0 + w):
            put(px, x, y, color)


def png(px):
    raw = b"".join(
        b"\x00" + b"".join(struct.pack("BBBB", *px[y][x]) for x in range(SIZE))
        for y in range(SIZE)
    )

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


# A lit ramp rather than a palette of flat fills: every shade of Nim is one of
# these, chosen by which way the surface faces the light.
PLUM = [
    (0x2A, 0x1E, 0x3A, 255),
    (0x40, 0x2D, 0x57, 255),
    (0x5B, 0x40, 0x78, 255),
    (0x7A, 0x57, 0x9C, 255),
    (0x9A, 0x73, 0xBD, 255),
    (0xB8, 0x94, 0xD8, 255),
    (0xD6, 0xBA, 0xEE, 255),
    (0xEF, 0xE1, 0xFA, 255),
]
CREAM = [
    (0xC9, 0x9C, 0x74, 255),
    (0xE8, 0xC0, 0x95, 255),
    (0xF7, 0xDF, 0xBE, 255),
    (0xFF, 0xF3, 0xE0, 255),
]
EYE_WHITE = (0xFB, 0xF6, 0xFF, 255)
IRIS = [(0x18, 0x4E, 0x6B, 255), (0x2F, 0x8F, 0xB5, 255), (0x7A, 0xD8, 0xEE, 255)]
BLUSH = (0xE0, 0x8C, 0xA8, 255)
MOUTH = (0x3A, 0x22, 0x3C, 255)
SPARK = (0xFF, 0xF2, 0xA8, 255)
# The one translucent colour Nim has. Being under the hit-test threshold as
# well as under Nim, it is a shadow a click goes straight through.
SHADOW = (0x2A, 0x1E, 0x3A, 0x4C)

LIGHT = (-0.55, -0.84)


def shade(nx, ny, ramp, lift=0):
    """Which step of `ramp` a point on a blob's surface takes.

    The blob is lit as a sphere: how far the surface faces the light picks the
    step, and the band just inside the far edge takes a rim light, which is
    what stops a shaded ball from reading as a flat disc.
    """
    facing = nx * LIGHT[0] + ny * LIGHT[1]
    step = (facing + 1.0) / 2.0
    index = int(round(step * (len(ramp) - 1))) + lift
    if nx * nx + ny * ny > 0.62 and facing < -0.35:
        index += 2
    return ramp[max(0, min(len(ramp) - 1, index))]


def blob(px, cx, cy, rx, ry, ramp, lift=0):
    """A shaded ellipse. Nim is three of them and a pair of ears."""
    for y in range(int(cy - ry) - 1, int(cy + ry) + 2):
        for x in range(int(cx - rx) - 1, int(cx + rx) + 2):
            nx = (x + 0.5 - cx) / rx
            ny = (y + 0.5 - cy) / ry
            if nx * nx + ny * ny <= 1.0:
                put(px, x, y, shade(nx, ny, ramp, lift))


def ear(px, x, top, sway, flip, length=7, splay=0.0, flat=0.0):
    """An ear that trails the head: it is anchored where it meets the skull and
    leans further the nearer the tip, so a sway is a curve rather than a slide.
    A drooping ear is a short one. `splay` opens the tip outward; `flat` lays
    the same length along the head. The row that meets the skull stays put."""
    for row in range(7 - length, 7):
        t = (6 - row) / 6.0
        lean = sway * t * t * 2.0 + splay * t
        ux, uy = x + flip * lean, top + row
        fx, fy = x + flip * (6 - row), top + 6
        rx = int(round(ux * (1.0 - flat) + fx * flat))
        ry = int(round(uy * (1.0 - flat) + fy * flat))
        ramp = PLUM[6 - row // 3] if flip > 0 else PLUM[4 - row // 3]
        put(px, rx, ry, ramp)
        # Upright: two wide. Flat: two tall — highlight over the dark row
        # that still sits on the skull. One extra tip pixel lengthens the
        # bar without walking the touchpoint off the head.
        if flat > 0.5:
            hi = PLUM[6] if flip > 0 else PLUM[4]
            put(px, rx, ry - 1, hi)
            put(px, rx, ry, PLUM[2])
            if row == 7 - length:
                put(px, rx + flip, ry - 1, hi)
                put(px, rx + flip, ry, PLUM[2])
        else:
            put(px, rx + flip, ry, PLUM[2])


def eyes(px, cy, open_amount, look=0):
    """Two large eyes with an iris ramp and a highlight. `open_amount` is 0 to
    1, and everything between is a frame of a blink."""
    for side in (-4, 4):
        cx = 16 + side
        if open_amount < 0.35:
            rect(px, cx - 2, cy + 1, 4, 1, PLUM[1])
            continue
        height = 3 if open_amount < 0.75 else 4
        rect(px, cx - 2, cy, 4, height, EYE_WHITE)
        rect(px, cx - 1 + look, cy + height - 3, 2, height - 1, IRIS[1])
        rect(px, cx - 1 + look, cy + height - 2, 2, 1, IRIS[0])
        put(px, cx - 1 + look, cy + height - 3, IRIS[2])


def nim(bob=0.0, squash=0.0, sway=0.0, open_amount=1.0, look=0, mouth=0, step=0.0, arms=0, ears=7, mark=None, shadow=True, reach=False, splay=0.0, flat=0.0):
    """Nim, posed. Every argument is continuous, which is what buys the
    in-between frames: a walk is the same pose sampled eight times."""
    px = blank()
    # A contact shadow needs contact: the only Animation the Engine plays with
    # the sprite off the ground turns it off, or Nim falls with the floor
    # attached to its feet.
    if shadow:
        rect(px, 11, GROUND, 10, 1, SHADOW)
        rect(px, 12, GROUND - 1, 8, 1, SHADOW)

    body_cy = 23.0 + bob + squash
    head_cy = 14.5 + bob

    for side, phase in ((-1, 0.0), (1, math.pi)):
        swing = math.sin(step + phase)
        fx = 13 + (0 if side < 0 else 5) + int(round(swing * 2))
        fy = GROUND - 2 - max(0, int(round(math.sin(step + phase) * 1.5)))
        rect(px, fx, fy, 4, 2, PLUM[2 if swing < 0 else 3])
        rect(px, fx, fy, 4, 1, PLUM[4])

    blob(px, 16, body_cy, 8.5, 6.5 - squash, PLUM)
    blob(px, 16, body_cy + 1.0, 5.0, max(1.5, 4.0 - squash), CREAM)

    for side in (-1, 1):
        ax = 16 + side * 9
        ay = int(body_cy) - 1 + (abs(arms) if reach else arms * side)
        # Hold keeps the shoulder and grows the rest of the limb to the floor.
        height = GROUND - ay + 1 if reach else 4
        rect(px, ax - 1, ay, 2, height, PLUM[3 if side < 0 else 5])

    ear(px, 11, int(head_cy) - 11, sway, -1, ears, splay, flat)
    ear(px, 20, int(head_cy) - 11, sway, 1, ears, splay, flat)
    blob(px, 16, head_cy, 7.5, 6.5, PLUM, lift=1)
    eyes(px, int(head_cy) - 1, open_amount, look)
    put(px, 11, int(head_cy) + 2, BLUSH)
    put(px, 21, int(head_cy) + 2, BLUSH)
    if mouth:
        rect(px, 15, int(head_cy) + 3, 2, min(mouth, 3), MOUTH)
    else:
        put(px, 15, int(head_cy) + 3, MOUTH)
        put(px, 16, int(head_cy) + 3, MOUTH)

    if mark == "spark":
        for x, y in ((25, 4), (24, 5), (26, 5), (25, 6), (25, 3), (25, 7)):
            put(px, x, y, SPARK)
    elif mark is not None:
        zx, zy = 23, 5 - mark
        rect(px, zx, zy, 4, 1, PLUM[6])
        rect(px, zx, zy + 3, 4, 1, PLUM[6])
        for i in range(2):
            put(px, zx + 2 - i, zy + 1 + i, PLUM[6])
    return px


def nim_animations():
    """Nim's nine Animations. Twice the frames of BMO's everywhere, because
    the whole difference between them is that Nim eases and BMO snaps."""
    idle = []
    for i in range(6):
        phase = i / 6.0 * math.tau
        idle.append(
            nim(
                bob=math.sin(phase) * 0.9,
                sway=math.sin(phase - 0.6),
                open_amount=0.0 if i == 4 else 1.0,
            )
        )

    walk = []
    for i in range(8):
        phase = i / 8.0 * math.tau
        walk.append(
            nim(
                bob=abs(math.sin(phase)) * -1.2,
                sway=math.sin(phase - 1.2) * 0.9,
                step=phase,
                arms=1 if math.sin(phase) > 0 else -1,
            )
        )

    fall = [
        nim(bob=-1.5, sway=-1.0, arms=-2, mouth=2, shadow=False),
        nim(bob=-1.0, sway=1.0, arms=-2, mouth=3, shadow=False),
        nim(bob=-1.4, sway=-0.7, arms=-2, mouth=2, shadow=False),
    ]
    land = [
        nim(squash=3.0, bob=2.0, sway=1.0, open_amount=0.2, arms=1, ears=5),
        nim(squash=1.8, bob=1.0, sway=0.8, open_amount=0.5, ears=6),
        nim(squash=0.8, sway=0.5, open_amount=0.9),
        nim(squash=0.2, sway=0.2),
    ]
    sit = [
        nim(squash=1.4 + i * 0.2, bob=1.0, sway=0.4 - i * 0.2, look=i % 2, ears=6)
        for i in range(4)
    ]
    sleep = []
    for i in range(4):
        sleep.append(
            nim(
                squash=2.0 + math.sin(i / 4.0 * math.tau) * 0.4,
                bob=1.2,
                sway=0.2,
                open_amount=0.0,
                ears=4,
                mark=i,
            )
        )
    react = [
        nim(bob=-2.0, sway=-0.6, mouth=3, arms=-1, mark="spark"),
        nim(bob=-2.6, sway=0.6, mouth=2, arms=-1, mark="spark"),
        nim(bob=-1.4, sway=0.8, mouth=2, mark="spark"),
        nim(bob=-0.6, sway=0.4, mouth=1),
        nim(bob=0.0, sway=0.2, open_amount=0.8),
    ]
    talk = []
    for i in range(6):
        phase = i / 6.0 * math.tau
        talk.append(
            nim(
                bob=math.sin(phase) * 0.6,
                sway=math.sin(phase) * 0.5,
                mouth=(0, 2, 3, 2, 1, 0)[i],
                look=(0, 0, 1, 0, -1, 0)[i],
            )
        )

    return {
        "idle": idle,
        "walk": walk,
        "fall": fall,
        "land": land,
        "sit": sit,
        # Two keys: the approved grip, then the same grip with ears laid out.
        # The frames between are the flatten, not a breath.
        "hold": [
            nim(squash=2.2, bob=1.5, sway=0.4, ears=5, arms=1, reach=True, splay=4.0, flat=i / 3.0)
            for i in range(4)
        ],
        "sleep": sleep,
        "react": react,
        "talk": talk,
    }


def colours(animations):
    return {px[y][x] for frames in animations.values() for px in frames for y in range(SIZE) for x in range(SIZE)}


def write(name, animations):
    out = ROOT / name / "frames"
    out.mkdir(parents=True, exist_ok=True)
    for existing in out.glob("*.png"):
        existing.unlink()
    for animation, frames in animations.items():
        for index, px in enumerate(frames):
            (out / f"{animation}-{index}.png").write_bytes(png(px))
    print(f"{name}: {sum(len(f) for f in animations.values())} frames, "
          f"{len(colours(animations) - {CLEAR})} colours")


def main():
    nim_art = nim_animations()

    # The check the style is held to: a shaded ramp, not a flat reskin. Two
    # Characters that differ only in their palette are a reskin.
    assert len(colours(nim_art) - {CLEAR}) > 16, "Nim fits in a sixteen-colour palette"

    assert set(nim_art) == {
        "idle", "walk", "fall", "land", "sit", "sleep", "react", "talk", "hold"
    }
    write("nim", nim_art)


if __name__ == "__main__":
    main()
