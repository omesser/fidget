#!/usr/bin/env python3
"""Build the landing page from README.md and characters/.

    python3 scripts/make-landing-page.py --out _site [--release release.json]
    python3 scripts/make-landing-page.py --self-check

A Generated page under ADR-0038. The headline, opening paragraph, feature list,
How It Works labels, Harness names, install notes and opening hero video are
read from README.md;
the cast, count and sprites are read from Character Manifests. The page shell
is docs/design/landing.html, with double-brace slots this script fills. A
source it cannot find fails the build.

Two sources are optional. Without the hero video the desk scene stands in.
Each download button links its asset in the release file that
`gh release view --json tagName,assets,body` wrote, and names that release's tag
above its format. Without such a file or such an asset, the button links the
Latest release page and names no version. The What's new strip under the hero
carries up to three lines from that release file's body: the bullets under a
`## Highlights` heading. A release without one shows no strip — GitHub's own
notes for it are commit subjects and "by @author in <pull request>", which read
as housekeeping to a visitor — and so does a release file with no tag. The
macOS Homebrew line is read from the Get It section fenced command, and claims
nothing when there is none.

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
RELEASES = f"{REPO}/releases"
# The asset names carry the version, so a button matches its asset by suffix.
DOWNLOADS = (("macos", "_aarch64.dmg"), ("windows", "-setup.exe"), ("linux", ".AppImage"))
COUNT = ("No", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight",
         "Nine", "Ten", "Eleven", "Twelve")
# README `- **Name:** caption` → slot suffix. Extras are Malformed. "Your fidget"
# is the sink the lanes feed: a name, no caption.
HOW_NODES = {
    "Frame loop": "frame_loop",
    "You": "you",
    "Your fidget": "fidget",
    "Personality": "personality",
    "Instance Prompt": "instance_prompt",
    "Window titles": "window_titles",
    "Director": "director",
    "A Behavior": "behavior",
    "A line": "line",
    "Your agent": "your_agent",
    "ACP": "acp",
    "MCP": "mcp",
}
HOW_LANES = {
    "Always on. No AI needed.": "always",
    "Optional: the Director": "director",
}
# Light-up hooks the shell must carry; the node test pins the script's array
# to this same order: frame loop, then the Director waking, then a Behavior.
HOW_LIGHT = ("frame", "director", "behavior")
# The What's new strip reads the release's own Highlights list, so the one
# heading it looks for is a constant the self-check can point at.
HIGHLIGHTS = re.compile(r"^##[ \t]+Highlights[ \t]*$", re.M)

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
    opening = text.split("## What It Does", 1)[0]
    video = ATTACHMENT.search(opening)
    # sections() drops code fences, so the command is read from the raw text;
    # an absent command renders nothing, since the page cannot claim what the
    # README does not.
    get_it_only = text.split("## Get It", 1)[1].split("\n## ", 1)[0]
    brew = re.search(r"^brew install --cask \S+$", get_it_only, re.M)
    return {
        "video": video and video.group(1),
        "headline": found["#title"],
        "lede": lede[0],
        "features": features,
        "harnesses": harnesses,
        "notes": [sentence(get_it, "not signed"), sentence(get_it, "works offline")],
        "brew": brew.group(0) if brew else "",
        "how": read_how(text, found),
    }


def read_how(text, found):
    """How It Works as lanes, nodes, mermaid, and wake sample, or Malformed.

    Bullets and lane titles come from sections(); fences are read from the raw
    slice because sections() drops them, the same way Get It reads brew.
    """
    body = section(found, "How It Works")
    paras = paragraphs(body)
    if len(paras) < 2:
        raise Malformed("README.md How It Works has no opening paragraph and sample lead-in")

    lanes = {}
    for line in body:
        heading = re.match(r"### (.+)", line)
        if heading and heading.group(1).strip() in HOW_LANES:
            title = heading.group(1).strip()
            lanes[HOW_LANES[title]] = title
    for title, suffix in HOW_LANES.items():
        if suffix not in lanes:
            raise Malformed(f"README.md How It Works is missing lane {title!r}")

    nodes = {}
    for line in body:
        bullet = re.match(r"- \*\*(.+?):\*\* (.+)", line)
        if not bullet:
            continue
        name, caption = bullet.group(1), bullet.group(2)
        if name not in HOW_NODES:
            raise Malformed(f"README.md How It Works has unknown bullet {name!r}")
        if name == "Your fidget":
            raise Malformed("README.md How It Works must not caption Your fidget")
        nodes[HOW_NODES[name]] = caption
    for name, slot in HOW_NODES.items():
        if slot == "fidget":
            nodes[slot] = ""
            continue
        if slot not in nodes:
            raise Malformed(f"README.md How It Works is missing bullet {name!r}")

    how_only = text.split("## How It Works", 1)[1].split("\n## ", 1)[0]
    fences = re.findall(r"```(\w*)\n(.*?)```", how_only, re.S)
    if len(fences) != 2 or {lang for lang, _ in fences} != {"mermaid", "text"}:
        raise Malformed("README.md How It Works allows only a mermaid fence and one text sample")
    found_fences = {lang: body for lang, body in fences}
    # The lead-in is the paragraph the sample introduces: the last one before
    # the text fence, and a colon is what a lead-in ends with.
    lead = paragraphs(how_only.split("```text", 1)[0].splitlines())[-1]
    if not lead.endswith(":"):
        raise Malformed("README.md How It Works puts no lead-in before its wake sample")
    return {
        "sub": paras[0],
        "wake_lead": lead,
        "lanes": lanes,
        "nodes": nodes,
        "mermaid": found_fences["mermaid"],
        "wake": found_fences["text"].strip("\n"),
    }


HOW_SHEET = {
    "always": ("Frame loop", "You", "Your fidget"),
    # README bullet order, so the alternative reads like the captions above.
    "director": ("Personality", "Instance Prompt", "Window titles", "Director",
                 "A Behavior", "A line", "Your agent", "ACP", "MCP"),
}


def how_sheet(how, suffix):
    """A per-lane <dl> of its names and captions, in README bullet order.

    Per lane because lane membership is part of the picture: one flat list
    lost whose caption belongs to which lane.
    """
    parts = []
    for name in HOW_SHEET[suffix]:
        parts.append(f"<dt>{html.escape(name)}</dt>")
        caption = how["nodes"][HOW_NODES[name]]
        if caption:
            parts.append(f"<dd>{inline(caption)}</dd>")
    return "".join(parts)


def how_slots(how, shell):
    """{{how_*}} markers, values escaped, and the parities that make them honest.

    The mermaid must still name the picture; the shell still has every marker.
    Both run from render(), every build.
    """
    missing = [name for name in HOW_NODES if name not in how["mermaid"]]
    missing += [title for title in HOW_LANES if title not in how["mermaid"]]
    missing += [how["nodes"][slot].rstrip(".") for slot in ("acp", "mcp")
                if how["nodes"][slot].rstrip(".") not in how["mermaid"]]
    if missing:
        raise Malformed(
            "README.md How It Works mermaid does not name "
            + ", ".join(repr(item) for item in missing)
        )
    slots = {
        "how_sub": inline(how["sub"]),
        "how_wake_lead": inline(how["wake_lead"]),
        "how_wake": html.escape(how["wake"]),
        "how_sheet_always": how_sheet(how, "always"),
        "how_sheet_director": how_sheet(how, "director"),
    }
    for suffix, title in how["lanes"].items():
        slots[f"how_lane_{suffix}"] = html.escape(title)
    for name, slot in HOW_NODES.items():
        slots[f"how_{slot}_name"] = html.escape(name)
    # Captions reach the page through the two per-lane sheets instead.
    for name in slots:
        marker = "{{" + name + "}}"
        if marker not in shell:
            raise Malformed(f"{SHELL.name} holds no {marker} marker")
    return slots


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
    """Each button's URL and version: its asset and the release tag, or the Latest page and None."""
    assets = release.get("assets", []) if release else []
    found = {}
    for button, suffix in DOWNLOADS:
        url = next((a["url"] for a in assets if a["name"].endswith(suffix)), None)
        found[button] = (url, release.get("tagName")) if url else (LATEST, None)
    return found


def download_slots(release):
    slots = {}
    for button, (url, version) in downloads(release).items():
        slots[f"download_{button}"] = html.escape(url)
        slots[f"version_{button}"] = f"<span>{html.escape(version)}</span>" if version else ""
    return slots


def release_lines(body):
    """Up to three strip lines: the bullets under the release's `## Highlights`
    heading, or none. GitHub writes its own notes for a release — commit
    subjects, "by @author in <pull request>" — and those read as housekeeping to
    a visitor, so a release with no Highlights shows no strip at all."""
    body = re.sub(r"\r\n?", "\n", body)
    heading = HIGHLIGHTS.search(body)
    if not heading:
        return []
    section = re.split(r"^##[ \t]", body[heading.end():], maxsplit=1, flags=re.M)[0]
    written = [re.sub(r"^[*-][ \t]+", "", line).strip()
               for line in section.splitlines() if re.match(r"^[*-][ \t]+\S", line)]
    return written[:3]


def strip_text(line):
    """A release line as inline Markdown. A link only the repository could
    resolve is left as its text, since a release note is not the README whose
    malformed links may fail the build."""
    try:
        return inline(line)
    except Malformed:
        return html.escape(line, quote=False)


def whats_new(release):
    """The strip under the hero, or "" with no tagged release to name, or nothing
    in that release's notes a visitor should read."""
    release = release or {}
    tag = release.get("tagName")
    lines = release_lines(release.get("body") or "") if tag else []
    if not lines:
        return ""
    shown = " · ".join(strip_text(line) for line in lines)
    return (f'<p class="whats-new"><b>New in {html.escape(tag, quote=False)}</b> · {shown} '
            f'<a href="{RELEASES}">All releases →</a></p>')


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
        "install_brew": (
            f'Or install with Homebrew: <code>{html.escape(words["brew"])}</code> '
            f'<button type="button" class="copy" data-copy="{html.escape(words["brew"])}">Copy</button>'
        ) if words["brew"] else "",
        "features": "\n".join(f"<div><b>{inline(t)}</b><p>{inline(b)}</p></div>" for t, b in words["features"]),
        "harnesses": "".join(f"<li>{html.escape(h)}</li>" for h in words["harnesses"]),
        "count": (COUNT[len(cast)] if len(cast) < len(COUNT) else str(len(cast)))
        + (" character ships" if len(cast) == 1 else " characters ship"),
        "cast": figures,
        **download_slots(release),
        "whats_new": whats_new(release),
        "brand": hero["idle"]["frames"][0],
        "hero": sprite(hero["sit"], f'{hero["name"]}, perched on a window',
                       ' id="hero-sprite" aria-describedby="hero-say"'),
        **how_slots(words["how"], shell),
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
    assert "How It Works" in (fails(readme.replace("## How It Works", "## How It Happens")) or ""), \
        "a README with no How It Works section built a page"
    assert fails(readme.replace("- **Director:** Wakes now and then and decides what's next.\n", "")), \
        "a README that drops a How It Works bullet built a page"
    assert fails(readme.replace('|"attach over ACP on stdio"|', '|"stdio"|')), \
        "a mermaid that drops the ACP caption built a page"
    assert fails(readme.replace("```text\n", "```json\n")), \
        "a How It Works slice with no text sample built a page"
    assert fails(readme.replace("```mermaid", "```")), \
        "a fence-poor How It Works built a page"
    assert fails(readme.replace("<code>", "<kbd>")), "a Harness table with no names built a page"

    words = read_readme(readme)
    assert "claude" in words["harnesses"] and "anything else" not in words["harnesses"]
    how = words["how"]
    assert how["sub"] == "A pet first. The AI is the part that makes it someone."
    assert how["lanes"]["always"] == "Always on. No AI needed."
    assert how["lanes"]["director"] == "Optional: the Director"
    assert how["nodes"]["fidget"] == ""
    assert how["nodes"]["frame_loop"] == "Walks, falls, lands, perches. Every frame, on your machine."
    assert how["nodes"]["you"] == "Poke, throw, drag, summon."
    assert how["nodes"]["personality"] == "Who it is. A plain text file."
    assert how["nodes"]["instance_prompt"] == "Your own twist, on top."
    assert how["nodes"]["window_titles"] == "Only if you say yes. Never pixels."
    assert how["nodes"]["director"] == "Wakes now and then and decides what's next."
    assert how["nodes"]["behavior"] == "nap, prowl, patrol…"
    assert how["nodes"]["line"] == "in character."
    assert how["nodes"]["your_agent"] == "Attach the one you already use. It can talk and use tools."
    assert how["nodes"]["acp"] == "attach over ACP on stdio."
    assert how["nodes"]["mcp"] == "the Harness calls back over MCP."
    assert how["wake_lead"] == "A sample of an opening wake and a later follow-up, not a live dump:"
    assert fails(readme.replace("A sample of an opening wake and a later follow-up, not a live dump:", "")), \
        "a How It Works with no sample lead-in built a page"
    dim = list(re.finditer(r"([^{]*)\[data-light\][^{]*\{[^}]*opacity[^}]*\}", shell))
    assert dim, "the shell has no [data-light] opacity rule"
    assert all(".how.how-motion" in m.group(1) for m in dim), \
        "the dim rule is not under .how.how-motion"
    assert all(f'data-light="{step}"' in shell for step in HOW_LIGHT), \
        "the shell is missing a data-light hook"
    attachment = "https://github.com/user-attachments/assets/0a1b2c3d-4e5f-6789-abcd-ef0123456789"
    readme_without_attachments = ATTACHMENT.sub("", readme)
    opening_video = readme_without_attachments.replace(
        "## What It Does", f"{attachment}\n\n## What It Does")
    showcase_video = readme_without_attachments.replace(
        "## Characters", f"## Characters\n\n{attachment}")
    assert read_readme(opening_video)["video"] == attachment, \
        "an opening attachment stopped becoming the website hero video"
    assert read_readme(showcase_video)["video"] is None, \
        "a Character showcase attachment became the website hero video"
    fenced = sections("## Get It\nClone and run:\n\n```sh\ncargo run\n```\n\nIt works offline.\n")
    assert sentence(fenced["Get It"], "offline") \
        == "It works offline.", "an install note ran across a code fence into the paragraph before it"
    assert inline("[a](./docs/x.md#y) `b` **c**") == (
        f'<a href="{REPO}/blob/main/docs/x.md#y">a</a> <code>b</code> <b>c</b>')
    assert inline("[a](#harness-support) <i>") == f'<a href="{REPO}#harness-support">a</a> &lt;i&gt;'

    release = {"tagName": "v9", "assets": [{"name": n, "url": f"{REPO}/releases/download/v9/{n}"}
                                           for n in ("F_9_amd64.deb", "F_9_amd64.AppImage", "F_9_aarch64.dmg")]}

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
        for name in HOW_NODES:
            assert name in page, f"{name} did not reach the page"
        for title in HOW_LANES:
            assert title in page, f"{title} did not reach the page"
        assert "attach over ACP on stdio" in page and "the Harness calls back over MCP" in page
        assert "Opening wake" in page
        assert "not a live dump" in page, "the wake sample lost its sample lead-in"
        assert "A pet first. The AI is the part that makes it someone." in page
        assert not re.search(r"<script[^>]*mermaid", page) and 'class="mermaid"' not in page
        assert "One character ships with Fidget" in page, "the cast count is not the package count"
        assert "<figcaption>Solo</figcaption>" in page
        page = render(readme, scratch, rust, shell, release)
        assert '<p class="download-label" id="download-label">Download for</p>' in page, \
            "the Download for label lacks an id"
        assert '<div class="get" id="get" role="group" aria-labelledby="download-label">' in page, \
            "#get is not an aria group labeled by download-label"
        assert page.index('id="download-label"') < page.index('id="get"'), \
            "the Download for label is not before #get"
        assert "<b>macOS</b><span>v9</span><span>Apple Silicon · .dmg</span>" in page, \
            "the release tag is not above the format"
        assert "<b>Windows</b><span>x86_64 · installer</span>" in page, "a button without its asset named a version"
        assert "<code>brew install --cask omesser/fidget/fidget</code>" in page, \
            "the Homebrew note lost the command"
        assert 'data-copy="brew install --cask omesser/fidget/fidget"' in page, \
            "the Copy button does not carry the command it copies"
        assert "btn primary" not in page and "btn quiet" not in page, \
            "the static page led with a row before any OS was detected"
        assert all(f'data-os="{row}"' in page for row in ("macos", "windows", "linux")), \
            "the OS-lead script has no row to find"
        naked = render(readme.replace("brew install --cask omesser/fidget/fidget", ""),
                       scratch, rust, shell, release)
        assert '<p class="note" data-os-note="macos" hidden></p>' in naked, \
            "the Homebrew note did not render empty and hidden"
        assert "Or install with Homebrew" not in naked, \
            "the page claimed a Homebrew line the README does not make"

    assert downloads(release) == {
        "macos": (f"{REPO}/releases/download/v9/F_9_aarch64.dmg", "v9"),
        "windows": (LATEST, None),
        "linux": (f"{REPO}/releases/download/v9/F_9_amd64.AppImage", "v9"),
    }, "a button linked an asset that is not its own, or named a version it does not link"

    notes = ("## Highlights\n"
             "- Chat can be a perch now\n"
             "- Tool calls read clearly\n")
    assert release_lines(notes) == ["Chat can be a perch now", "Tool calls read clearly"], \
        "the strip did not read a release's Highlights list"
    assert release_lines(notes.replace("\n", "\r\n")) == release_lines(notes), \
        "a CRLF release body — what `gh release view` writes — lost a line"
    assert release_lines("## What's Changed\n"
                         "* fix(site): Keep the strip honest by @omesser in https://github.com/omesser/fidget/pull/2\n"
                         "* build(deps): bump serde by @dependabot[bot] in https://github.com/omesser/fidget/pull/3\n") == [], \
        "a release GitHub wrote its own notes for still reached the strip"
    assert release_lines("## Highlights\n\n## Summary\n- not a highlight\n") == [], \
        "a Highlights heading with no bullets of its own reached the strip"
    assert whats_new(None) == "" and whats_new({"tagName": None}) == "", "an absent release drew a strip"
    assert whats_new({"tagName": "v9", "body": "## What's Changed\n* chore: Tidy up\n"}) == "", \
        "a release with nothing written for a visitor drew a strip"

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
                        help="`gh release view --json tagName,assets,body` output; without it every "
                        "download button links the Latest release page and the What's new strip "
                        "is absent")
    arguments = parser.parse_args()

    if arguments.self_check:
        self_check()
        return

    release = None
    if arguments.release and arguments.release.is_file():
        release = json.loads(arguments.release.read_text(encoding="utf-8"))
    else:
        print(f"no release file, so every download button links {LATEST} and the What's new strip is absent")
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
