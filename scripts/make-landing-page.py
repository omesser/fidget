#!/usr/bin/env python3
"""Build the landing page from README.md and characters/.

    python3 scripts/make-landing-page.py --out _site [--release release.json]
    python3 scripts/make-landing-page.py --self-check

A Generated page under ADR-0038. The headline, the opening paragraph, the
feature list, the Harness names, the install notes and the hero video are read
from README.md; the cast, its count and every sprite are read from the Character
Manifests. The page shell is docs/design/landing.html, with double-brace slots
this script fills. A source it cannot find fails the build.

Two sources are optional. Without the hero video the desk scene stands in.
Each download button links its asset in the release file that
`gh release view --json tagName,assets` wrote, or the Latest release page when
there is no such file or no such asset.

The page names frames by the paths make-character-gallery.py publishes under
`<out>/characters/`, and copies nothing itself, so the gallery's checks are the
only ones that decide what art reaches the site.

Pure standard library.
"""

import argparse
import html
import importlib.util
import json
import pathlib
import re
import sys
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
README = ROOT / "README.md"
CHARACTERS = ROOT / "characters"
SHELL = ROOT / "docs" / "design" / "landing.html"
PAGE = "index.html"
REPO = "https://github.com/omesser/fidget"
# The README names Buddy Bot as the default Character; the hero is that one.
HERO = "buddy-bot"
# GitHub renders an attachment URL alone on its line as a video player.
ATTACHMENT = re.compile(r"^(https://github\.com/user-attachments/assets/[0-9a-f-]+)[ \t]*$", re.M)
LATEST = f"{REPO}/releases/latest"
# The asset names carry the version, so a button matches its asset by suffix.
DOWNLOADS = (("download_macos", "_aarch64.dmg"), ("download_windows", "-setup.exe"),
             ("download_linux", ".AppImage"))
COUNT = ("No", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight",
         "Nine", "Ten", "Eleven", "Twelve")

_spec = importlib.util.spec_from_file_location("gallery", ROOT / "scripts" / "make-character-gallery.py")
gallery = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gallery)
Malformed = gallery.Malformed


# --------------------------------------------------------------------------
# README.md
# --------------------------------------------------------------------------


def sections(text):
    """The README as {heading: body}, with the H1 under "#", code fences dropped.

    A `# comment` inside a shell fence is not a heading, and a README whose
    only `#` lines sit in fences has no headline.
    """
    found, heading, fenced = {}, None, False
    for line in text.splitlines():
        if line.startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            continue
        title = re.match(r"(#{1,2}) (.+)", line)
        if title:
            heading = "#" if title.group(1) == "#" else title.group(2).strip()
            if heading == "#":
                found.setdefault("#title", title.group(2).strip())
            found.setdefault(heading, [])
            continue
        if heading is not None:
            found[heading].append(line)
    return found


def section(found, heading):
    if heading not in found:
        raise Malformed(f"README.md has no {heading!r} section")
    return found[heading]


def paragraphs(lines):
    return [" ".join(chunk.split()) for chunk in re.split(r"\n\s*\n", "\n".join(lines)) if chunk.strip()]


def sentence(lines, needle):
    for paragraph in paragraphs(lines):
        for said in re.split(r"(?<=[.!?:])\s+", paragraph):
            if needle in said:
                return said
    raise Malformed(f"README.md Get It says nothing containing {needle!r}")


def inline(markdown):
    """Escape, then the three inline forms the README's prose uses."""
    out = html.escape(markdown, quote=False)
    out = re.sub(r"`([^`]+)`", r"<code>\1</code>", out)
    out = re.sub(r"\*\*(.+?)\*\*", r"<b>\1</b>", out)

    def link(found):
        target = found.group(2)
        if target.startswith("#"):
            target = f"{REPO}{target}"
        elif target.startswith("./"):
            target = f"{REPO}/blob/main/{target[2:]}"
        elif not target.startswith("https://"):
            raise Malformed(f"README.md links {target!r}, which the page cannot resolve")
        href = target.replace('"', "&quot;")
        return f'<a href="{href}">{found.group(1)}</a>'

    return re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", link, out)


def read_readme(text):
    found = sections(text)
    if "#title" not in found:
        raise Malformed("README.md has no H1 to use as the headline")
    lede = paragraphs(section(found, "#"))
    lede = [p for p in lede if not p.startswith(("!", "<", "["))]
    if not lede:
        raise Malformed("README.md has no paragraph under its H1")

    features = []
    for line in section(found, "What It Does"):
        bullet = re.match(r"- \*\*(.+?):\*\* (.+)", line)
        if bullet:
            features.append((bullet.group(1), bullet.group(2)))
    if not features:
        raise Malformed("README.md What It Does lists no `- **Feature:** text` bullets")

    harnesses = re.findall(
        r"<tr>\s*<td[^>]*>(?:(?!</td>).)*?<code>([^<]+)</code>\s*</td>",
        "\n".join(section(found, "Harness Support")),
        re.S,
    )
    if not harnesses:
        raise Malformed("README.md Harness Support names no Harness")

    get_it = section(found, "Get It")
    video = ATTACHMENT.search(text)
    return {
        "video": video and video.group(1),
        "headline": found["#title"],
        "lede": lede[0],
        "features": features,
        "harnesses": harnesses,
        "notes": [sentence(get_it, "not signed"), sentence(get_it, "works offline")],
    }


# --------------------------------------------------------------------------
# characters/
# --------------------------------------------------------------------------


def loop(package, declared, name, default_fps):
    animation = declared.get("animations", {}).get(name)
    if not isinstance(animation, dict) or not animation.get("frames"):
        raise Malformed(f"{package.name}: declares no {name} frames")
    frames = []
    for path in animation["frames"]:
        if not isinstance(path, str) or not re.fullmatch(r"[\w./-]+\.png", path) \
                or path.startswith("/") or ".." in path or not (package / path).is_file():
            raise Malformed(f"{package.name}: {name} frame {path!r} is not a PNG in the package")
        frames.append(f"characters/{package.name}/{path}")
    fps = animation.get("fps", default_fps)
    if not isinstance(fps, int) or isinstance(fps, bool) or fps < 1:
        raise Malformed(f"{package.name}: {name} fps {fps!r} is not a positive integer")
    return {"frames": frames, "fps": fps}


def read_cast(characters_root, default_fps):
    packages = sorted(
        p for p in characters_root.iterdir()
        if (p / "character.manifest").is_file() and p.name not in gallery.WITHHELD
    )
    if not packages:
        raise Malformed(f"{characters_root} holds no Character Package")
    cast, hero = [], None
    for package in packages:
        try:
            declared = tomllib.loads((package / "character.manifest").read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as broken:
            raise Malformed(f"{package.name}: character.manifest does not read as TOML — {broken}") from broken
        member = {
            "name": declared.get("name") or package.name,
            "pixel": declared.get("render_mode") != "smooth",
            "walk": loop(package, declared, "walk", default_fps),
        }
        if package.name == HERO:
            hero = dict(member, sit=loop(package, declared, "sit", default_fps),
                        idle=loop(package, declared, "idle", default_fps))
            cast.insert(0, member)
        else:
            cast.append(member)
    if hero is None:
        raise Malformed(f"{characters_root} has no {HERO} package for the hero")
    return cast, hero


# --------------------------------------------------------------------------
# The page
# --------------------------------------------------------------------------


def sprite(loop_, alt, extra=""):
    frames = html.escape(json.dumps(loop_["frames"]))
    return (f'<img src="{loop_["frames"][0]}" alt="{html.escape(alt)}" '
            f'data-frames="{frames}" data-fps="{loop_["fps"]}"{extra}>')


def downloads(release):
    """Each button's slot and URL, the Latest release page where the release lacks its asset."""
    assets = release.get("assets", []) if release else []
    return {slot: next((a["url"] for a in assets if a["name"].endswith(suffix)), LATEST)
            for slot, suffix in DOWNLOADS}


def render(readme_text, characters_root, rust_source, shell, release=None):
    words = read_readme(readme_text)
    _, defaults = gallery.from_rust(rust_source)
    cast, hero = read_cast(characters_root, defaults["fps"])

    figures = "\n".join(
        f'<figure><div class="art">'
        + sprite(c["walk"], c["name"], ' class="pixel"' if c["pixel"] else "")
        + f'</div><figcaption>{html.escape(c["name"])}</figcaption></figure>'
        for c in cast
    )
    slots = {
        "headline": html.escape(words["headline"], quote=False),
        "description": html.escape(re.sub(r"[*`]", "", re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", words["lede"]))),
        "lede": inline(words["lede"]),
        "notes": " ".join(inline(n) for n in words["notes"]),
        "features": "\n".join(f"<div><b>{inline(t)}</b><p>{inline(b)}</p></div>" for t, b in words["features"]),
        "harnesses": "".join(f"<li>{html.escape(h)}</li>" for h in words["harnesses"]),
        "count": (COUNT[len(cast)] if len(cast) < len(COUNT) else str(len(cast)))
        + (" character ships" if len(cast) == 1 else " characters ship"),
        "cast": figures,
        **{slot: html.escape(url) for slot, url in downloads(release).items()},
        "brand": hero["idle"]["frames"][0],
        "hero": sprite(hero["sit"], f'{hero["name"]}, perched on a window',
                       ' id="hero-sprite" aria-describedby="hero-say"'),
        # The desk stays in the page. CSS hides it behind the video, and it
        # shows again when a video that fails to load removes itself.
        "video": (
            f'<video class="clip" src="{words["video"]}" autoplay muted loop playsinline '
            f'preload="metadata" aria-label="Fidget demo video" onerror="this.remove()">'
            f'<a href="{words["video"]}">Watch the demo video</a></video>'
        ) if words["video"] else "",
    }
    page = shell
    for name, value in slots.items():
        marker = "{{" + name + "}}"
        if marker not in page:
            raise Malformed(f"{SHELL.name} holds no {marker} marker")
        page = page.replace(marker, value)
    left = re.search(r"\{\{\w+\}\}", page)
    if left:
        raise Malformed(f"{SHELL.name} holds {left.group(0)}, which this script does not fill")
    return page


# --------------------------------------------------------------------------
# The check
# --------------------------------------------------------------------------


def self_check():
    """The parsing the page's honesty rests on, against fixtures."""
    rust = gallery.RUST.read_text(encoding="utf-8")
    shell = SHELL.read_text(encoding="utf-8")
    readme = README.read_text(encoding="utf-8")

    def fails(text, characters=CHARACTERS):
        try:
            render(text, characters, rust, shell)
        except Malformed as caught:
            return str(caught)
        return None

    headline = re.search(r"^# .+$", readme, re.M).group(0)
    headless = readme.replace(headline, "")
    assert "no H1" in (fails(headless) or ""), "a README with no H1 built a page"
    assert "no H1" in (fails("```sh\n# a shell comment\n```\n" + headless) or ""), \
        "a `#` line inside a code fence became the headline"
    assert fails(readme.replace("## What It Does", "## What It Is")), \
        "a README with no What It Does section built a page"
    assert fails(readme.replace("<code>", "<kbd>")), "a Harness table with no names built a page"

    words = read_readme(readme)
    assert "claude" in words["harnesses"] and "anything else" not in words["harnesses"]
    fenced = sections("## Get It\nClone and run:\n\n```sh\ncargo run\n```\n\nIt works offline.\n")
    assert sentence(fenced["Get It"], "offline") \
        == "It works offline.", "an install note ran across a code fence into the paragraph before it"
    assert inline("[a](./docs/x.md#y) `b` **c**") == (
        f'<a href="{REPO}/blob/main/docs/x.md#y">a</a> <code>b</code> <b>c</b>')
    assert inline("[a](#harness-support) <i>") == f'<a href="{REPO}#harness-support">a</a> &lt;i&gt;'

    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        art = (CHARACTERS / HERO / "frames" / "idle-0.png").read_bytes()
        package = scratch / HERO
        (package / "frames").mkdir(parents=True)
        (package / "frames" / "idle-0.png").write_bytes(art)
        loops = "\n".join(f'[animations.{n}]\nframes = ["frames/idle-0.png"]' for n in ("idle", "sit"))
        (package / "character.manifest").write_text(
            f'name = "Solo"\n{loops}\n[animations.walk]\nframes = ["frames/walk-0.png"]\n', encoding="utf-8")
        assert "walk frame" in (fails(readme, scratch) or ""), "a frame the package lacks reached the page"

        (package / "frames" / "walk-0.png").write_bytes(art)
        page = render(readme, scratch, rust, shell)
        assert "One character ships with Fidget" in page, "the cast count is not the package count"
        assert "<figcaption>Solo</figcaption>" in page

    release = {"assets": [{"name": n, "url": f"{REPO}/releases/download/v9/{n}"}
                          for n in ("F_9_amd64.deb", "F_9_amd64.AppImage", "F_9_aarch64.dmg")]}
    assert downloads(release) == {
        "download_macos": f"{REPO}/releases/download/v9/F_9_aarch64.dmg",
        "download_windows": LATEST,
        "download_linux": f"{REPO}/releases/download/v9/F_9_amd64.AppImage",
    }, "a button linked an asset that is not its own"

    print(f"self-check: {len(words['features'])} features, {len(words['harnesses'])} Harnesses, checks passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=pathlib.Path, default=ROOT / "_site",
                        help=f"directory to write {PAGE} into")
    parser.add_argument("--readme", type=pathlib.Path, default=README,
                        help="the README to read the page's words from")
    parser.add_argument("--self-check", action="store_true",
                        help="run the generator's own checks and exit")
    parser.add_argument("--release", type=pathlib.Path,
                        help="`gh release view --json tagName,assets` output; without it every "
                        "download button links the Latest release page")
    arguments = parser.parse_args()

    if arguments.self_check:
        self_check()
        return

    release = None
    if arguments.release and arguments.release.is_file():
        release = json.loads(arguments.release.read_text(encoding="utf-8"))
    else:
        print(f"no release file, so every download button links {LATEST}")
    try:
        page = render(arguments.readme.read_text(encoding="utf-8"), CHARACTERS,
                      gallery.RUST.read_text(encoding="utf-8"), SHELL.read_text(encoding="utf-8"), release)
    except Malformed as broken:
        sys.exit(f"landing page: {broken}")
    arguments.out.mkdir(parents=True, exist_ok=True)
    (arguments.out / PAGE).write_text(page, encoding="utf-8")
    print(f"{arguments.out / PAGE}: generated from {arguments.readme.name} and {CHARACTERS.name}/")


if __name__ == "__main__":
    main()
