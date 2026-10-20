#!/usr/bin/env python3
"""Build the Character gallery page from characters/.

    python3 scripts/make-character-gallery.py --out _site
    python3 scripts/make-character-gallery.py --self-check

Reads every Character Manifest, copies the frames the manifests name into
`<out>/characters/`, and writes `<out>/characters.html` — the page shell in
docs/design/characters.html with its data block substituted. A Generated page
under ADR-0011: the manifests are the source of the frame count, fps, loop,
render_mode, scale, weight, variant rings and attribution the page shows, so a
page that disagrees with a package cannot be deployed.

Pure standard library. tomllib reads the manifests and the frames are copied
byte for byte, so nothing here needs Pillow or a TOML dependency installed.

This script decides what gets published. scripts/assemble-site.sh names
files one by one for the hand-written pages, and that reviewed list is the
security model; it cannot name a few hundred PNGs. So the script names this
generator instead, and the guarantee moves here: only the files a manifest
names as frames are copied, only if they are PNGs inside their own package,
and the workflow re-checks every extension in _site afterwards.

Malformed input fails the build rather than reaching the page. A required
Animation a manifest simply does not declare is different: that is a fact
about the package the gallery exists to show, so it renders as a missing tile.
The loader already refuses to install such a package (crates/core/src/
character.rs), so nothing here is the only thing standing between it and a
user.
"""

import argparse
import json
import pathlib
import re
import shutil
import struct
import sys
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHARACTERS = ROOT / "characters"
RUST = ROOT / "crates" / "core" / "src" / "character.rs"
SHELL = ROOT / "docs" / "design" / "characters.html"
PLACEHOLDER = '{"characters": [], "required": [], "defaults": {}}'

# Packages that stay off the published page, and why. The gallery is a public
# URL; a Character this project has no right to publish does not go on it, and
# nothing about the omission reaches the page.
WITHHELD = {}

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


class Malformed(Exception):
    """A package the gallery cannot describe truthfully."""


def from_rust(source):
    """The Required Animation Set and the manifest defaults, from character.rs.

    Retyping the nine names here would make the page's idea of completeness a
    second declaration that nothing keeps in step with the loader's. Same for
    the defaults: a package that declares no fps plays at eight because that
    constant says so, and a gallery hardcoding eight would keep claiming it
    after someone changed it. Weight is the same move — a ring member with no
    `weight` takes DEFAULT_WEIGHT, and typing 10 here would drift from the
    loader the first time that constant moved.
    """
    block = re.search(r"REQUIRED_ANIMATIONS:\s*\[&str;\s*(\d+)\]\s*=\s*\[(.*?)\];", source, re.S)
    if not block:
        raise Malformed(f"{RUST.name} declares no REQUIRED_ANIMATIONS array")
    required = re.findall(r'"([^"]+)"', block.group(2))
    if len(required) != int(block.group(1)):
        raise Malformed(f"REQUIRED_ANIMATIONS says {block.group(1)} names and lists {len(required)}")

    def constant(name):
        found = re.search(rf"pub(?:\(crate\))? const {name}:\s*u32\s*=\s*(\d+);", source)
        if not found:
            raise Malformed(f"{RUST.name} declares no {name}")
        return int(found.group(1))

    return required, {
        "fps": constant("DEFAULT_FPS"),
        "scale": constant("DEFAULT_SCALE"),
        "weight": constant("DEFAULT_WEIGHT"),
    }


def source(package, declared):
    """What the package declares in `[source]`, or None.

    Read, not scraped. The leading comment block this replaced carried both
    the attribution and the art-production notes beside it — row mappings,
    remat logs — and the page published all of it; trimming to the first
    paragraph instead would have dropped `cat`'s license caveat, which sits
    below its mapping with no blank line above. Only a declared key can
    publish the license sentence and nothing else.

    A package that declares nothing gets nothing. Silence is the honest
    rendering; a line asserting the art is this repository's would not be.
    """
    block = declared.get("source")
    if block is None:
        return None
    if not isinstance(block, dict):
        raise Malformed(f"{package.name}: [source] is not a table")

    said = {}
    for key in ("art", "license"):
        value = block.get(key)
        if not isinstance(value, str) or not value.strip():
            raise Malformed(f"{package.name}: [source] declares no {key}")
        said[key] = value.strip()

    # The page writes this into an href, so the scheme is checked here for the
    # same reason frame paths are: a manifest is data, and this script is what
    # stands between it and a public URL.
    url = block.get("url")
    if url is not None:
        if not isinstance(url, str) or not url.startswith(("https://", "http://")):
            raise Malformed(f"{package.name}: [source] url {url!r} is not an http or https address")
        said["url"] = url

    return said


def frame(package, declared, art_root):
    """Copy one frame into the site tree and measure it.

    The path check is the reason this function exists rather than a shutil
    one-liner: a manifest is data, and this script is what stands between it
    and a public URL now that the workflow no longer names each file.
    """
    if not isinstance(declared, str):
        raise Malformed(f"{package.name}: frame {declared!r} is not a path")
    parts = pathlib.PurePosixPath(declared).parts
    if declared.startswith("/") or ".." in parts:
        raise Malformed(f"{package.name}: frame {declared!r} points outside the package")
    if not declared.endswith(".png"):
        raise Malformed(f"{package.name}: frame {declared!r} is not a .png")

    source = package / pathlib.PurePosixPath(declared)
    # The textual check above rules out `..` and an absolute path; a symlink
    # is the third way out, and it survives every one of them. Compare the
    # resolved paths so what gets copied is the file the manifest named.
    if source.resolve().parent != (package / pathlib.PurePosixPath(declared)).parent.resolve() \
            or not source.resolve().is_relative_to(package.resolve()):
        raise Malformed(f"{package.name}: frame {declared!r} resolves outside the package")
    if not source.is_file():
        raise Malformed(f"{package.name}: declares frame {declared!r}, which the package lacks")
    art = source.read_bytes()
    if art[:8] != PNG_SIGNATURE or art[12:16] != b"IHDR":
        raise Malformed(f"{package.name}: frame {declared!r} is not a PNG")
    width, height = struct.unpack(">II", art[16:24])

    destination = art_root / package.name / pathlib.PurePosixPath(declared)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(art)
    return {"src": f"characters/{package.name}/{declared}", "w": width, "h": height}


def emit(package, name, animation, defaults, art_root, extra=None):
    """One Animation as the page plays it: frames on disk, fps/loop/weight as
    the loader would fill them. `extra` is for a Variant's `variant_of` — the
    field that says this tile is a ring member, not a Required Animation slot.
    """
    frames = animation.get("frames")
    if not isinstance(frames, list) or not frames:
        raise Malformed(f"{package.name}: {name!r} declares no frames")
    tile = {
        "name": name,
        "fps": animation.get("fps", defaults["fps"]),
        # The Engine holds the last frame of a `once` Animation forever;
        # the page does the same and offers a replay.
        "loop": animation.get("loop") != "once",
        "frames": [frame(package, path, art_root) for path in frames],
        "weight": animation.get("weight", defaults["weight"]),
    }
    if extra:
        tile.update(extra)
    return tile


def character(package, required, defaults, art_root):
    manifest = package / "character.manifest"
    try:
        text = manifest.read_text(encoding="utf-8")
        declared = tomllib.loads(text)
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as broken:
        raise Malformed(f"{package.name}: {manifest.name} does not read as TOML — {broken}") from broken

    animations = declared.get("animations", {})
    if not isinstance(animations, dict):
        raise Malformed(f"{package.name}: [animations] is not a table")

    strip = []
    for name in required:
        animation = animations.get(name)
        if animation is None:
            strip.append({"name": name, "missing": True})
            continue
        strip.append(emit(package, name, animation, defaults, art_root))

    # Nested on the base, never spliced into the required strip: a Variant is
    # not a tenth Required Animation, and sing is not a climb.
    by_base = {}
    for name, animation in animations.items():
        if name in required:
            continue
        base = animation.get("variant_of")
        if not base:
            continue
        by_base.setdefault(base, []).append(
            emit(package, name, animation, defaults, art_root, {"variant_of": base})
        )

    for slot in strip:
        members = by_base.get(slot["name"])
        if members:
            slot["variants"] = members

    # Optional Animations the Engine already falls back (climb→walk, grab→fall).
    # Absent means absent — a dashed "missing" tile would recast them as required.
    for name in ("climb", "grab"):
        animation = animations.get(name)
        if animation is None or animation.get("variant_of"):
            continue
        extra = emit(package, name, animation, defaults, art_root)
        members = by_base.get(name)
        if members:
            extra["variants"] = members
        strip.append(extra)

    return {
        "dir": package.name,
        "name": declared.get("name") or package.name,
        "smooth": declared.get("render_mode") == "smooth",
        "scale": declared.get("scale", defaults["scale"]),
        "source": source(package, declared),
        "animations": strip,
    }


def gallery(characters_root, rust_source, out):
    required, defaults = from_rust(rust_source)
    packages = sorted(p for p in characters_root.iterdir() if (p / "character.manifest").is_file())
    if not packages:
        raise Malformed(f"{characters_root} holds no Character Package")

    packages = [p for p in packages if p.name not in WITHHELD]

    art_root = out / "characters"
    data = {
        "characters": [character(p, required, defaults, art_root) for p in packages],
        "required": required,
        "defaults": defaults,
    }

    # Escaped so the JSON can never close the <script> element it sits in, and
    # so a manifest comment in any language survives the trip.
    block = json.dumps(data, ensure_ascii=True).replace("<", "\\u003c")
    shell = SHELL.read_text(encoding="utf-8")
    if shell.count(PLACEHOLDER) != 1:
        raise Malformed(f"{SHELL.name} holds no single data placeholder to substitute")
    page = out / "characters.html"
    page.write_text(shell.replace(PLACEHOLDER, block), encoding="utf-8")
    return data, page


def self_check():
    """Prove the behaviors the page's honesty rests on.

    A generator that silently emits a broken page is the failure mode worth
    designing out: malformed input has to stop the build, a merely incomplete
    package has to reach the page as incomplete, a Variant must not steal a
    Required Animation slot, and undeclared climb/grab must not become
    "missing". Asserting that here costs less than a test harness this
    repository does not otherwise have for Python.
    """
    art = (CHARACTERS / "nim" / "frames" / "idle-0.png").read_bytes()
    required, defaults = from_rust(RUST.read_text(encoding="utf-8"))
    body = "\n".join(
        f'[animations.{name}]\nframes = ["frames/{name}-0.png"]' for name in required
    )

    def package(root, manifest, frames=("idle-0.png",)):
        root.mkdir(parents=True, exist_ok=True)
        (root / "character.manifest").write_text(manifest, encoding="utf-8")
        (root / "frames").mkdir(exist_ok=True)
        for name in frames:
            (root / "frames" / name).write_bytes(art)

    def raises(root, out):
        try:
            gallery(root, RUST.read_text(encoding="utf-8"), out)
        except Malformed as caught:
            return str(caught)
        return None

    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        out = scratch / "out"
        out.mkdir()

        junk = scratch / "junk"
        package(junk / "broken", "name = = =\n")
        assert raises(junk, out), "a manifest that is not TOML built a page anyway"

        absent = scratch / "absent"
        package(absent / "gappy", f'name = "Gappy"\n{body}\n')
        assert raises(absent, out), "a declared frame the package lacks built a page anyway"

        escape = scratch / "escape"
        package(
            escape / "sneaky",
            'name = "Sneaky"\n[animations.idle]\nframes = ["../../../etc/hosts.png"]\n',
        )
        assert raises(escape, out), "a frame path leaving the package built a page anyway"

        # The page publishes this at an indexed URL, so a package that names
        # its art source and says nothing about the license stops the build
        # rather than reaching the page with the caveat missing.
        quiet = scratch / "quiet"
        package(quiet / "hushed", f'name = "Hushed"\n[source]\nart = "From a pack"\n{body}\n')
        assert raises(quiet, out), "a [source] declaring no license built a page anyway"

        short = scratch / "short"
        names = [n for n in required if n != "walk"]
        package(
            short / "shortish",
            'name = "Shortish"\n'
            + "\n".join(f'[animations.{n}]\nframes = ["frames/{n}-0.png"]' for n in names),
            frames=[f"{n}-0.png" for n in names],
        )
        data, _ = gallery(short, RUST.read_text(encoding="utf-8"), out)
        walk = next(a for a in data["characters"][0]["animations"] if a["name"] == "walk")
        assert walk.get("missing"), "an undeclared Animation did not reach the page as missing"
        assert len(data["characters"][0]["animations"]) == len(required), "the strip lost a slot"
        assert data["characters"][0]["source"] is None, "a package declaring no source got one"

        complete = [f"{n}-0.png" for n in required]
        # DEFAULT_WEIGHT is the ring's undeclared share, same constant the
        # loader uses; a gallery that typed 10 would keep claiming it after
        # the Engine moved.
        rust_text = RUST.read_text(encoding="utf-8")
        expected_weight = int(re.search(
            r"pub(?:\(crate\))? const DEFAULT_WEIGHT:\s*u32\s*=\s*(\d+);",
            rust_text).group(1))
        assert defaults.get("weight") == expected_weight, (
            "DEFAULT_WEIGHT never reached the gallery defaults")

        ring = scratch / "ring"
        package(
            ring / "ringed",
            f'name = "Ringed"\n{body}\n'
            '[animations.idle-blink]\n'
            'frames = ["frames/idle-0.png"]\n'
            'variant_of = "idle"\n',
            frames=complete,
        )
        data, _ = gallery(ring, rust_text, out)
        ch = data["characters"][0]
        names = [a["name"] for a in ch["animations"]]
        assert names[:len(required)] == required, "a variant reordered the Required Animation strip"
        assert len(ch["animations"]) == len(required), "a variant stole a Required Animation slot"
        assert "idle-blink" not in names, "a variant of idle became its own strip"
        idle = next(a for a in ch["animations"] if a["name"] == "idle")
        members = idle.get("variants") or []
        blink = next((m for m in members if m["name"] == "idle-blink"), None)
        assert blink, "idle variants never reached the page data"
        assert blink.get("variant_of") == "idle"
        assert blink.get("frames")
        assert "fps" in blink
        assert "loop" in blink
        assert blink.get("weight") == expected_weight, (
            "an undeclared variant weight is not DEFAULT_WEIGHT")
        assert "climb" not in names and "grab" not in names, (
            "undeclared climb/grab appeared as missing tiles")

        weighed = scratch / "weighed"
        weighed_idle = "\n".join(
            f'[animations.{name}]\nframes = ["frames/{name}-0.png"]'
            + ("\nweight = 20" if name == "idle" else "")
            for name in required
        )
        package(
            weighed / "weighed",
            f'name = "Weighed"\n{weighed_idle}\n'
            "[animations.idle-blink]\n"
            'frames = ["frames/idle-0.png"]\n'
            'variant_of = "idle"\n'
            "weight = 5\n",
            frames=complete,
        )
        data, _ = gallery(weighed, rust_text, out)
        idle = next(a for a in data["characters"][0]["animations"] if a["name"] == "idle")
        blink = next(m for m in idle["variants"] if m["name"] == "idle-blink")
        assert idle.get("weight") == 20, "the base member dropped its declared weight"
        assert blink.get("weight") == 5, "a variant dropped its declared weight"

        climber = scratch / "climber"
        package(
            climber / "climber",
            f'name = "Climber"\n{body}\n'
            '[animations.climb]\nframes = ["frames/idle-0.png"]\n',
            frames=complete,
        )
        data, _ = gallery(climber, rust_text, out)
        names = [a["name"] for a in data["characters"][0]["animations"]]
        assert names[:len(required)] == required, "climb replaced a Required Animation slot"
        assert names[len(required):] == ["climb"], "declared climb did not follow the required strip"
        climb = data["characters"][0]["animations"][len(required)]
        assert not climb.get("missing"), "declared climb reached the page as missing"

        grabber = scratch / "grabber"
        package(
            grabber / "grabber",
            f'name = "Grabber"\n{body}\n'
            '[animations.grab]\nframes = ["frames/idle-0.png"]\n',
            frames=complete,
        )
        data, _ = gallery(grabber, rust_text, out)
        names = [a["name"] for a in data["characters"][0]["animations"]]
        assert names[len(required):] == ["grab"], "declared grab did not follow the required strip"

        looks = scratch / "looks"
        package(
            looks / "shiny",
            f'name = "Shiny"\nrender_mode = "smooth"\nscale = 2\n{body}\n',
            frames=complete,
        )
        data, _ = gallery(looks, rust_text, out)
        shiny = data["characters"][0]
        assert shiny["smooth"] is True, "render_mode = smooth did not reach the page"
        assert shiny["scale"] == 2, "declared scale did not reach the page"

    print(f"self-check: {len(required)} Required Animations, "
          f"defaults fps={defaults['fps']} scale={defaults['scale']} "
          f"weight={defaults.get('weight')}, checks passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=pathlib.Path, default=ROOT / "_site",
                        help="directory to write characters.html and characters/ into")
    parser.add_argument("--self-check", action="store_true",
                        help="run the generator's own checks and exit")
    arguments = parser.parse_args()

    if arguments.self_check:
        self_check()
        return

    # A withheld name that matches no package is not a package being kept off
    # the page — it is one that publishes the next time somebody renames a
    # directory. Checked against the real tree, which self_check does not use.
    stale = sorted(set(WITHHELD) - {p.name for p in CHARACTERS.iterdir() if p.is_dir()})
    if stale:
        sys.exit(f"character gallery: WITHHELD names no such package: {', '.join(stale)}")

    out = arguments.out
    out.mkdir(parents=True, exist_ok=True)
    shutil.rmtree(out / "characters", ignore_errors=True)
    try:
        data, page = gallery(CHARACTERS, RUST.read_text(encoding="utf-8"), out)
    except Malformed as broken:
        sys.exit(f"character gallery: {broken}")

    frames = sum(len(a["frames"]) for c in data["characters"] for a in c["animations"]
                 if not a.get("missing"))
    missing = sum(1 for c in data["characters"] for a in c["animations"] if a.get("missing"))
    print(f"{page}: {len(data['characters'])} Characters, {frames} frames"
          + (f", {missing} Required Animations missing" if missing else ""))


if __name__ == "__main__":
    main()
