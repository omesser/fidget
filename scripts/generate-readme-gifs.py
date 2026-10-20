#!/usr/bin/env python3
"""Generate showcase GIFs from character frames for the README.
Creates more interesting animations (not boring idles) for the Characters table."""

import argparse
import sys
import tempfile
from pathlib import Path

from PIL import Image

# GIF has no partial alpha and Pillow turns a faint pixel into an opaque speck,
# so 64 drops edge samples. Nim's foot shadow is alpha 76, so a higher cut
# deletes it. The cut is on alpha, never on black: pupils are opaque #000.
ALPHA_CUTOFF = 64

# Brighter than the shell's own edge. The check ignores pixels the source
# already calls drawn (alpha >= UNCOVERED, the mask cut in character.rs).
FRINGE_MIN = 230
UNCOVERED = 128

TARGET_HEIGHT = 96

# output name, character, animation, frame count, fps
SHOWCASES = (
    ("buddy-bot-walk.gif", "buddy-bot", "walk", 8, 8),
    ("buddy-bot-react.gif", "buddy-bot", "react", 5, 12),
    ("black-mage-talk.gif", "black-mage", "talk", 2, 3),
    ("bmo-sing.gif", "bmo", "sing", 4, 3),
    ("cat-walk.gif", "cat", "walk", 8, 8),
    ("jotaro-kujo-react.gif", "jotaro-kujo", "react", 8, 7),
    ("nim-sleep.gif", "nim", "sleep", 4, 3),
    # Permission granted by MekaRamen (Yeoh Kher En) to publish.
    ("timber-wolf-walk.gif", "timber-wolf", "walk", 20, 8),
    ("trump-wave.gif", "trump", "wave", 3, 4),
)


def binarize_alpha(img, cutoff=ALPHA_CUTOFF):
    """Drop coverage a GIF would otherwise paint solid."""
    img = img.convert("RGBA")
    out = img.copy()
    src = img.load()
    dst = out.load()
    for y in range(img.height):
        for x in range(img.width):
            r, g, b, a = src[x, y]
            if 0 < a < cutoff:
                dst[x, y] = (0, 0, 0, 0)
            elif cutoff <= a < 255:
                dst[x, y] = (r, g, b, 255)
    return out


def scale_frame(img, target_height):
    """Nearest-neighbor scale. Resampling would invent new edge colors."""
    if not target_height:
        return img
    aspect_ratio = img.width / img.height
    new_width = int(target_height * aspect_ratio)
    return img.resize((new_width, target_height), Image.NEAREST)


def save_gif(images, output_path, fps):
    duration_ms = int(1000 / fps)
    images[0].save(
        output_path,
        save_all=True,
        append_images=images[1:],
        duration=duration_ms,
        loop=0,
        optimize=False,
        disposal=2,
    )


def frame_paths(chars_dir, character, animation, count):
    return [
        chars_dir / character / "frames" / f"{animation}-{i}.png" for i in range(count)
    ]


def gif_rgba_frames(path):
    gif = Image.open(path)
    frames = []
    for index in range(gif.n_frames):
        gif.seek(index)
        frames.append(gif.convert("RGBA"))
    return frames


def create_looping_gif(frames, output_path, fps, scale=1, target_height=None):
    """Create a looping GIF from a list of frame paths.

    Args:
        frames: List of paths to PNG frames
        output_path: Where to save the GIF
        fps: Frames per second
        scale: Integer scale factor (using nearest neighbor)
        target_height: If set, scale to this height (overrides scale)
    """
    images = []
    for frame_path in frames:
        img = Image.open(frame_path).convert("RGBA")
        if target_height:
            img = scale_frame(img, target_height)
        elif scale > 1:
            img = img.resize((img.width * scale, img.height * scale), Image.NEAREST)
        images.append(binarize_alpha(img))

    save_gif(images, output_path, fps)


def generate(repo_root):
    chars_dir = repo_root / "characters"
    out_dir = repo_root / "docs" / "readme"
    out_dir.mkdir(parents=True, exist_ok=True)

    for name, character, animation, count, fps in SHOWCASES:
        frames = frame_paths(chars_dir, character, animation, count)
        output = out_dir / name
        create_looping_gif(frames, output, fps=fps, target_height=TARGET_HEIGHT)
        print(f"✓ {name} ({output.stat().st_size / 1024:.1f} KB)")

    print(f"\nAll showcase GIFs generated in {out_dir}/")
    print(f"Target height: {TARGET_HEIGHT}px for consistent visual sizing")
    print("More interesting animations selected (walk/react/sing/scan/talk vs boring idles)")


def _adjacent_to_clear(frame, x, y):
    for dy, dx in (
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (1, 1),
        (-1, 1),
        (1, -1),
    ):
        ny, nx = y + dy, x + dx
        if 0 <= ny < frame.height and 0 <= nx < frame.width and frame.getpixel((nx, ny))[3] == 0:
            return True
    return False


def assert_no_white_fringe(gif_frames, source_frames, label):
    """Uncovered source pixels must not survive as bright opaque specks."""
    assert len(gif_frames) == len(source_frames), label
    for index, (gif_frame, source) in enumerate(zip(gif_frames, source_frames)):
        assert gif_frame.size == source.size, f"{label} frame {index} size"
        gif_px = gif_frame.load()
        src_px = source.load()
        for y in range(gif_frame.height):
            for x in range(gif_frame.width):
                sr, sg, sb, sa = src_px[x, y]
                gr, gg, gb, ga = gif_px[x, y]
                if sa == 255 and (sr, sg, sb) == (0, 0, 0):
                    assert ga != 0, f"{label} frame {index} punched a black pupil at {(x, y)}"
                if sa >= UNCOVERED or ga == 0:
                    continue
                if min(gr, gg, gb) < FRINGE_MIN:
                    continue
                if _adjacent_to_clear(gif_frame, x, y):
                    raise AssertionError(
                        f"{label} frame {index} has a white fringe pixel at {(x, y)}"
                    )


def self_check():
    """A near-clear white edge pixel stays clear; an opaque black pupil does not."""
    synthetic = Image.new("RGBA", (8, 8), (0, 0, 0, 0))
    drawn = synthetic.load()
    for y in range(2, 6):
        for x in range(2, 6):
            drawn[x, y] = (220, 220, 220, 255)
    drawn[3, 3] = (0, 0, 0, 255)
    drawn[1, 2] = (255, 255, 255, 1)
    drawn[6, 2] = (0, 0, 0, 1)

    with tempfile.TemporaryDirectory() as scratch:
        gif_path = Path(scratch) / "speck.gif"
        save_gif([binarize_alpha(synthetic)], gif_path, fps=8)
        frame = gif_rgba_frames(gif_path)[0]
        assert frame.getpixel((1, 2))[3] == 0, "alpha-1 white speck stayed opaque"
        assert frame.getpixel((6, 2))[3] == 0, "alpha-1 black speck stayed opaque"
        pupil = frame.getpixel((3, 3))
        assert pupil[3] == 255 and pupil[:3] == (0, 0, 0), f"pupil became {pupil}"
        assert_no_white_fringe([frame], [synthetic], "synthetic")

    repo_root = Path(__file__).parent.parent
    chars_dir = repo_root / "characters"
    out_dir = repo_root / "docs" / "readme"
    for name, character, animation, count, _fps in SHOWCASES:
        sources = [
            scale_frame(Image.open(path).convert("RGBA"), TARGET_HEIGHT)
            for path in frame_paths(chars_dir, character, animation, count)
        ]
        assert_no_white_fringe(gif_rgba_frames(out_dir / name), sources, name)

    print("self-check: white fringe absent, black pupils kept")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self-check",
        action="store_true",
        help="assert showcase GIFs have no white fringe and keep black pupils",
    )
    args = parser.parse_args(argv)
    repo_root = Path(__file__).parent.parent
    if args.self_check:
        try:
            self_check()
        except AssertionError as failed:
            sys.exit(f"readme gifs: self-check failed: {failed}")
        return
    generate(repo_root)


if __name__ == "__main__":
    main()
