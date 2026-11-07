#!/usr/bin/env python3
"""Build the overlay-expression page from src/bubble.js, src/main.css and
characters/buddy-bot/.

    python3 scripts/make-expression-page.py --out _site
    python3 scripts/make-expression-page.py --self-check

A Generated page under ADR-0011: the published HTML must load the overlay's
bubble module and the bubble rules sliced from main.css, so a reading-time
formula restated on the page cannot deploy. Malformed or missing bubble.js
fails the build the way a bad Character Manifest fails the gallery.

The sprite under the bubble is a shipped Character, not a coloured div, so
its Animations are the third input: which frames Buddy Bot's `idle` and
`talk` strips hold and at what fps is read from its own manifest here, and
the frames are copied beside the page. The committed shell carries the same
block naming the packages where that file can see them, one directory up,
and `require_current_sprite` fails the build when the two disagree — which is
the only way a hand-edited shell could start naming art it does not have.

That path is checkable, not runnable: bubble.css exists nowhere but here, so
the page is judged from `--out`, never from `website/`.

Pure standard library. The CSS slice is text, not a parser, so it cannot
quietly pick up the overlay's transparent fullscreen rules sitting above
`.bubble`. Overlay `@import`s are inlined into that slice: the published
page is one file, and leaving `var(--bubble-*)` unset would paint a
transparent hole.
"""

import argparse
import importlib.util
import json
import pathlib
import re
import shutil
import sys
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUBBLE_JS = ROOT / "src" / "bubble.js"
MAIN_CSS = ROOT / "src" / "main.css"
SHELL = ROOT / "website" / "expression.html"
GALLERY = ROOT / "scripts" / "make-character-gallery.py"
SPRITE = ROOT / "characters" / "buddy-bot" / "character.manifest"

EXPORTS = (
    "export function bubbleDuration",
    "export function createBubbleMachine",
    "export const THINKING_GRACE_MS",
    "export const THINKING_MIN_HOLD_MS",
)

# The page only speaks and idles, so it reads those two strips and leaves the
# rest of the Required set to the gallery.
SPRITE_PACKAGE = "buddy-bot"
SPRITE_ANIMATIONS = ("idle", "talk")

# Where the art sits relative to the page: `website/expression.html` in a
# clone reaches the packages one directory up, and `_site/expression.html`
# reaches the copies beside it. The shell is committed with the first.
CLONE_ART = "../characters/"
SITE_ART = "characters/"

# The class line ADR-0011 makes every page repeat in its own header, because a
# page reached from search arrives without the index. Named here so the two
# cannot drift.
SOURCE_LINE = "Generated from src/bubble.js and characters/buddy-bot/ at deploy"

SPRITE_BLOCK = re.compile(
    r'(<script type="application/json" id="sprite">\n)(.*?)(\n</script>)', re.S
)

FORMULA = re.compile(r"900\s*\+\s*55")
RESTATED_TIMERS = re.compile(
    r"THINKING_GRACE_MS\s*=\s*250|THINKING_MIN_HOLD_MS\s*=\s*600"
)
NOINDEX = re.compile(r'<meta\s+name="robots"\s+content="noindex"', re.I)


class Malformed(Exception):
    """Input the page cannot describe truthfully."""


def gallery_vetting():
    """The gallery generator's frame check, not a second copy of it.

    `frame` there is what stands between a path a manifest names and a public
    URL: no `..`, no absolute path, no symlink out of the package, and PNG
    bytes behind a `.png` name. Loaded by path because the filename is not an
    identifier, and it copies into `<out>/characters/` exactly as the gallery
    does, so the two pages share one tree of art rather than shipping the
    frames twice.
    """
    spec = importlib.util.spec_from_file_location("character_gallery", GALLERY)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require_module(source):
    """Refuse a bubble.js that is not the overlay's machine."""
    if not source.strip():
        raise Malformed("src/bubble.js is empty")
    for token in EXPORTS:
        name = token.rsplit(" ", 1)[-1]
        if token not in source:
            raise Malformed(f"src/bubble.js does not export {name}")


IMPORT = re.compile(
    r"""@import\s+(?:url\(\s*)?["']([^"']+)["']\s*\)?\s*;""",
)


def inline_css_imports(css, base_dir):
    """Follow overlay `@import`s so the published page is still one stylesheet."""
    chunks = []
    for match in IMPORT.finditer(css):
        imported = (base_dir / match.group(1)).resolve()
        if not imported.is_file():
            raise Malformed(f"src/main.css imports missing {match.group(1)}")
        chunks.append(imported.read_text(encoding="utf-8").rstrip())
    return "\n".join(chunks)


def extract_bubble_css(css, base_dir=None):
    """The bubble rules only.

    Copying main.css wholesale paints a transparent fullscreen overlay onto a
    document that has to stay a readable page. The slice starts at `.bubble {`
    and ends when the thinking-bounce keyframes close — those two markers are
    the overlay's speech and thinking surfaces, and nothing the overlay uses
    for layout sits between them today.
    """
    start = css.find(".bubble {")
    if start < 0:
        raise Malformed("src/main.css declares no .bubble rule")
    key = css.find("@keyframes thinking-bounce", start)
    if key < 0:
        raise Malformed("src/main.css declares no thinking-bounce keyframes")
    brace = css.find("{", key)
    if brace < 0:
        raise Malformed("src/main.css thinking-bounce keyframes never open")
    depth = 0
    for index, char in enumerate(css[brace:], brace):
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                sliced = css[start : index + 1].strip() + "\n"
                if "html," in sliced or "background: transparent" in sliced:
                    raise Malformed("bubble CSS slice picked up overlay layout")
                if base_dir is not None:
                    prefix = inline_css_imports(css, base_dir)
                    if prefix:
                        return prefix + "\n" + sliced
                return sliced
    raise Malformed("src/main.css thinking-bounce keyframes never close")


def read_sprite(characters_root, base):
    """Buddy Bot's `idle` and `talk` strips as the page plays them.

    Frame lists and fps come from the package's own manifest, so a strip
    re-cut or re-timed there cannot leave the page playing the old one. The
    demo needs no `loop` or `weight`: both strips loop, and nothing here
    draws against Director weights.
    """
    package = characters_root / SPRITE_PACKAGE
    manifest = package / "character.manifest"
    if not manifest.is_file():
        raise Malformed(f"characters/{SPRITE_PACKAGE}/character.manifest is missing")
    try:
        declared = tomllib.loads(manifest.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as broken:
        raise Malformed(f"{SPRITE_PACKAGE}: character.manifest is not TOML: {broken}") from broken

    animations = declared.get("animations")
    if not isinstance(animations, dict):
        raise Malformed(f"{SPRITE_PACKAGE}: declares no Animations")

    played = {}
    for name in SPRITE_ANIMATIONS:
        animation = animations.get(name)
        if not isinstance(animation, dict):
            raise Malformed(f"{SPRITE_PACKAGE}: declares no {name!r} Animation")
        frames = animation.get("frames")
        if not isinstance(frames, list) or not frames:
            raise Malformed(f"{SPRITE_PACKAGE}: {name!r} declares no frames")
        fps = animation.get("fps")
        if not isinstance(fps, int) or isinstance(fps, bool) or fps <= 0:
            raise Malformed(f"{SPRITE_PACKAGE}: {name!r} declares no usable fps")
        played[name] = {"fps": fps, "frames": frames}

    # The page draws each frame at its own size, which matches the overlay only
    # while the pack ships `scale = 1`, so a pack that asks to be drawn larger
    # stops the build instead of being quietly shown at the wrong size.
    scale = declared.get("scale", 1)
    if scale != 1:
        raise Malformed(
            f"{SPRITE_PACKAGE}: declares scale = {scale}, and the page draws frames "
            "at their own size"
        )

    return {
        "package": SPRITE_PACKAGE,
        "name": declared.get("name") or SPRITE_PACKAGE,
        # `render_mode` is the loader's: a pixel pack reads as blurred at
        # `image-rendering: auto`, so the page says which this is.
        "smooth": declared.get("render_mode") == "smooth",
        "art": base + SPRITE_PACKAGE + "/",
        "animations": played,
    }


def copy_sprite(characters_root, sprite, art_root):
    """Copy the frames the block names, vetted the way the gallery vets them."""
    vet = gallery_vetting()
    package = characters_root / sprite["package"]
    for animation in sprite["animations"].values():
        for path in animation["frames"]:
            vet.frame(package, path, art_root)


def extract_sprite(html):
    """The block the committed shell carries, parsed."""
    found = SPRITE_BLOCK.search(html)
    if not found:
        raise Malformed("expression.html declares no sprite block")
    try:
        return json.loads(found.group(2))
    except json.JSONDecodeError as broken:
        raise Malformed(f"expression.html's sprite block is not JSON: {broken}") from broken


def require_current_sprite(html, characters_root):
    """The shell's own block must be what the manifest says today.

    The shell is a working page on its own — opened from `website/` it
    plays the frames one directory up — and that only stays true while its
    block matches `characters/buddy-bot/`. Comparing here means a re-cut strip
    fails the build rather than leaving a clone playing frames that moved.
    """
    if extract_sprite(html) != read_sprite(characters_root, CLONE_ART):
        raise Malformed(
            "expression.html's sprite block has drifted from "
            f"characters/{SPRITE_PACKAGE}/character.manifest"
        )


def fill_sprite(html, sprite):
    """Point the block at the art beside the published page."""
    filled, count = SPRITE_BLOCK.subn(
        lambda found: found.group(1) + json.dumps(sprite, indent=2) + found.group(3),
        html,
        count=1,
    )
    if count != 1:
        raise Malformed("expression.html declares no sprite block to fill")
    if CLONE_ART in filled:
        raise Malformed("expression.html still reaches outside the site for its art")
    return filled


def require_honest_page(html):
    """The shell must operate the module, not describe it."""
    if 'type="module"' not in html:
        raise Malformed("expression.html is not a module page")
    if 'from "./bubble.js"' not in html and "from './bubble.js'" not in html:
        raise Malformed("expression.html does not load src/bubble.js")
    if "bubbleDuration(" not in html:
        raise Malformed("expression.html never calls bubbleDuration")
    if "createBubbleMachine(" not in html:
        raise Malformed("expression.html never calls createBubbleMachine")
    if FORMULA.search(html):
        raise Malformed("expression.html restates reading-time arithmetic")
    if RESTATED_TIMERS.search(html):
        raise Malformed("expression.html restates grace or hold timers")
    if NOINDEX.search(html):
        raise Malformed("expression.html is Generated and must stay indexed")
    if SOURCE_LINE not in html:
        raise Malformed("expression.html does not name its Generated sources")


def assemble(src_root, out):
    bubble_path = src_root / "src" / "bubble.js"
    css_path = src_root / "src" / "main.css"
    shell_path = src_root / "website" / "expression.html"
    characters_root = src_root / "characters"

    if not bubble_path.is_file():
        raise Malformed("src/bubble.js is missing")
    source = bubble_path.read_text(encoding="utf-8")
    require_module(source)

    if not css_path.is_file():
        raise Malformed("src/main.css is missing")
    css = extract_bubble_css(css_path.read_text(encoding="utf-8"), css_path.parent)

    if not shell_path.is_file():
        raise Malformed("website/expression.html is missing")
    html = shell_path.read_text(encoding="utf-8")
    require_honest_page(html)
    require_current_sprite(html, characters_root)

    out.mkdir(parents=True, exist_ok=True)
    # The art before the page that names it, so a frame the manifest points
    # outside its package fails the build with no HTML written.
    sprite = read_sprite(characters_root, SITE_ART)
    copy_sprite(characters_root, sprite, out / "characters")
    shutil.copyfile(bubble_path, out / "bubble.js")
    (out / "bubble.css").write_text(css, encoding="utf-8")
    (out / "expression.html").write_text(fill_sprite(html, sprite), encoding="utf-8")
    return out / "expression.html"


def _tree(scratch, bubble=None, css=None, html=None, manifest=None):
    root = scratch / "src_root"
    (root / "src").mkdir(parents=True)
    (root / "website").mkdir(parents=True)
    if bubble is not None:
        (root / "src" / "bubble.js").write_text(bubble, encoding="utf-8")
    if css is not None:
        (root / "src" / "main.css").write_text(css, encoding="utf-8")
    if html is not None:
        (root / "website" / "expression.html").write_text(html, encoding="utf-8")
    # The real package, so a case that is not about the art is not also
    # testing a hand-rolled stand-in for it.
    shutil.copytree(ROOT / "characters" / SPRITE_PACKAGE, root / "characters" / SPRITE_PACKAGE)
    if manifest is not None:
        (root / "characters" / SPRITE_PACKAGE / "character.manifest").write_text(
            manifest, encoding="utf-8"
        )
    return root


def self_check():
    """Prove malformed bubble.js cannot ship, and a good tree stays Generated."""
    good_js = BUBBLE_JS.read_text(encoding="utf-8")
    good_css = MAIN_CSS.read_text(encoding="utf-8")
    good_html = SHELL.read_text(encoding="utf-8")

    def raises(root, out):
        try:
            assemble(root, out)
        except Malformed as caught:
            return str(caught)
        return None

    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        out = scratch / "out"

        missing = _tree(scratch / "missing", css=good_css, html=good_html)
        assert raises(missing, out / "missing"), "missing bubble.js built a page anyway"

        empty = _tree(scratch / "empty", bubble="", css=good_css, html=good_html)
        assert raises(empty, out / "empty"), "empty bubble.js built a page anyway"

        mute = _tree(
            scratch / "mute",
            bubble="export function wrapText() {}\n",
            css=good_css,
            html=good_html,
        )
        assert raises(mute, out / "mute"), "bubble.js without the machine built a page anyway"

        bare = _tree(scratch / "bare", bubble=good_js, css="body { color: red; }\n", html=good_html)
        assert raises(bare, out / "bare"), "main.css without .bubble built a page anyway"

        described = good_html.replace("bubbleDuration(", "/* duration = 900 + 55 * length */ bubbleDuration(")
        fake = _tree(scratch / "described", bubble=good_js, css=good_css, html=described)
        assert raises(fake, out / "described"), "a page restating the formula built anyway"

        # A strip re-timed in the manifest and not followed on the page.
        retimed = _tree(
            scratch / "retimed",
            bubble=good_js,
            css=good_css,
            html=good_html,
            manifest=SPRITE.read_text(encoding="utf-8").replace("fps = 6", "fps = 9"),
        )
        assert raises(retimed, out / "retimed"), "a stale sprite block built a page anyway"

        # A frame reaching out of its own package, which is the check the
        # gallery owns and this generator borrows rather than restates.
        escaped = _tree(
            scratch / "escaped",
            bubble=good_js,
            css=good_css,
            html=good_html,
            manifest=SPRITE.read_text(encoding="utf-8").replace(
                '"frames/talk-0.png"', '"../../../etc/hosts.png"'
            ),
        )
        assert raises(escaped, out / "escaped"), "a frame outside the package built a page anyway"
        assert not (out / "escaped" / "expression.html").exists(), "the page was written anyway"

        page = assemble(ROOT, out / "good")
        html = page.read_text(encoding="utf-8")
        require_honest_page(html)
        css = (out / "good" / "bubble.css").read_text(encoding="utf-8")
        assert ".bubble.visible" in css
        assert ".thinking-dots" in css
        assert "--bubble-panel:" in css
        copied = (out / "good" / "bubble.js").read_text(encoding="utf-8")
        assert copied == good_js
        sprite = extract_sprite(html)
        assert sprite["art"] == f"characters/{SPRITE_PACKAGE}/", "the page kept the clone's art path"
        assert CLONE_ART not in html
        for animation in sprite["animations"].values():
            for path in animation["frames"]:
                landed = out / "good" / "characters" / SPRITE_PACKAGE / path
                assert landed.is_file(), f"{path} is named but not published"

    print("self-check: bubble.js exports, CSS slice, honest page, sprite block, 8 checks passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=pathlib.Path, default=ROOT / "_site",
                        help="directory to write expression.html, bubble.js, bubble.css and characters/ into")
    parser.add_argument("--self-check", action="store_true",
                        help="run the generator's own checks and exit")
    arguments = parser.parse_args()

    if arguments.self_check:
        try:
            self_check()
        except FileNotFoundError as missing:
            sys.exit(f"overlay expression: {missing}")
        except AssertionError as failed:
            sys.exit(f"overlay expression: self-check failed: {failed}")
        return

    try:
        page = assemble(ROOT, arguments.out)
    except Malformed as broken:
        sys.exit(f"overlay expression: {broken}")
    sprite = extract_sprite(page.read_text(encoding="utf-8"))
    frames = sum(len(a["frames"]) for a in sprite["animations"].values())
    print(f"{page}: bubble.js, bubble.css and {frames} {SPRITE_PACKAGE} frames")


if __name__ == "__main__":
    main()
