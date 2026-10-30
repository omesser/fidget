#!/usr/bin/env python3
"""Build the landing page from README.md and characters/.

    python3 scripts/make-landing-page.py --out _site [--release release.json]
    python3 scripts/make-landing-page.py --self-check

A Generated page under ADR-0038. The headline, opening paragraph, feature list,
How It Works labels, Harness names, install notes, FAQ and opening hero video
are read from README.md;
the cast, count and sprites are read from Character Manifests. The page shell
is website/landing.html, with double-brace slots this script fills. A
source it cannot find fails the build.

Two sources are optional. Without the hero video the desk scene stands in.
Each download button links its asset in the release file that
`gh release view --json tagName,assets,body` wrote, and names that release's tag
above its format. Without such a file or such an asset, the button links the
Latest release page and names no version. The What's new strip under the hero
carries up to three lines from that release file's body: the bold title of each
of the first three bullets under a `## Highlights` heading. A release without one
shows no strip — GitHub's own notes for it are commit subjects and "by @author in
<pull request>", which read as housekeeping to a visitor — and so does a release
file with no tag. The macOS Homebrew line is read from the Get It section fenced
command, and claims nothing when there is none.

The "Live Playground" desk plays the hero until a visitor picks another
Character, moves by constants read from the Engine's engine.rs and input.rs and
the app's main.rs, and draws its cues with the overlay's own rules from
src/main.css and src/cue.js. Its picker offers every Character, so each must
carry every loop in PLAY_LOOPS. Its still tiles name their Characters in
PLAY_TILES. A loop or constant it cannot find fails the build.

Every other page in `<out>` gets the same site header, website/site-nav.html,
right after its `<body>`, and every page, this one included, gets the GoatCounter
visit counter right before its `</head>`, so assemble-site.sh runs this script
after the steps that publish pages.

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
import struct
import sys
import tempfile
import tomllib
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
README = ROOT / "README.md"
CHARACTERS = ROOT / "characters"
SHELL = ROOT / "website" / "landing.html"
NAV = ROOT / "website" / "site-nav.html"
MOMENTS = ROOT / "website" / "moments.json"
PAGE = "index.html"
REPO = "https://github.com/omesser/fidget"
# GoatCounter's snippet for the site `fidget`, as its docs gave it on 2026-10-21 (#1753).
COUNTER = '<script data-goatcounter="https://fidget.goatcounter.com/count" async src="//gc.zgo.at/count.js"></script>'
# The README names Buddy Bot as the default Character; the hero is that one.
HERO = "buddy-bot"
# The twist toy's lines; its prompts come from the README's Instance Prompt table.
TWISTS = ROOT / "website" / "twists.json"
# What the README's Instance Prompt setup list names, so the page's small print stays true.
TWIST_SETUP = ("[Harness]", "Model API", "**Window and application names** consent")
# The small print under a prompt that names no setup of its own in twists.json.
TWIST_NEEDS = "Works with any attached model."
# The small print under the twist toy.
TWIST_LABEL = ("Examples pre-generated with real in-app and character prompts. The site isn't connected to an LLM. "
               '<a href="#get">Download Fidget</a> to try the real thing.')
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
# A Highlights bullet, and the bold title one can open with. The colon inside
# the bold is what makes it a title rather than an emphasized first word:
# `- **Chat can be a perch now:** the detail` (docs/agents/writing.md).
BULLET = re.compile(r"^[*-][ \t]+")
TITLE = re.compile(r"^\*\*(?P<title>[^*]+?):\*\*")

# The app sources the desk reads its numbers from, by the name PLAY_TUNING uses.
ENGINE = {"engine.rs": ROOT / "crates" / "core" / "src" / "engine.rs",
          "input.rs": ROOT / "crates" / "core" / "src" / "input.rs",
          "main.rs": ROOT / "src-tauri" / "src" / "main.rs"}
CUES = ROOT / "src" / "main.css"
CHAT_UI = ROOT / "src" / "chat-ui.css"
CHAT_SHARED = ROOT / "src" / "chat-shared.css"
# The Chat UI rules the playground's chat mock wears.
CHAT_RULES = (".log", ".empty", ".empty > div", ".empty p", ".empty p b", ".composer", ".composer textarea",
              ".composer textarea::placeholder", ".composer textarea:disabled", ".composer button",
              ".composer button:disabled", ".composer textarea:focus-visible", ".composer button:focus-visible")
# The playground desk and the page character play these, so a Character
# lacking one fails the build instead of freezing on the page.
PLAY_LOOPS = ("idle", "walk", "react", "hold", "fall", "land", "sit", "sleep", "talk")
# A personality.txt introduces its quoted lines this way, up to a blank line.
SAYS = re.compile(r"(?:heard|known) to say:(.*?)(?:\n[ \t]*\n|\Z)", re.S)
# The still tiles no-JavaScript and reduced-motion visitors get: the gesture,
# the Character, and the Animation whose first frame shows it.
PLAY_TILES = (("Poke", "bmo", "react"), ("Pick up", "timber-wolf", "hold"), ("Throw", "cat", "fall"),
              ("Perch", "nim", "sit"), ("Summon", "buddy-bot", "talk"), ("Hide", "jotaro-kujo", "sleep"))
# The desk moves by the app's numbers: page key -> (source, constant). The
# double-click interval is the argument Pointer's Default passes, as input.rs
# has no constant for it.
PLAY_TUNING = {
    "gravity": ("engine.rs", "GRAVITY"),
    "walkSpeed": ("engine.rs", "WALK_SPEED"),
    "primitiveMs": ("engine.rs", "PRIMITIVE_MS"),
    "pokeCooldownMs": ("engine.rs", "POKE_COOLDOWN_MS"),
    "perchedIdleAfterMs": ("engine.rs", "PERCHED_IDLE_AFTER_MS"),
    "rideAcceleration": ("engine.rs", "RIDE_ACCELERATION"),
    "yankWindowS": ("engine.rs", "YANK_WINDOW_S"),
    "throwMinSpeed": ("input.rs", "THROW_MIN_SPEED"),
    "dragThreshold": ("input.rs", "DRAG_THRESHOLD"),
    "dragDelayMs": ("input.rs", "DRAG_DELAY_MS"),
    "velocityWindowMs": ("input.rs", "VELOCITY_WINDOW_MS"),
    "doubleClickMs": ("input.rs", "with_double_click_ms"),
    "hoverDelayMs": ("main.rs", "HOVER_DELAY_MS"),
}
# The overlay's speech bubble, which the page character speaks in.
BUBBLE_RULES = (".bubble", ".bubble.visible", ".bubble::before", ".bubble::after", ".bubble.inverted::before",
                ".bubble.inverted::after", ".bubble-content")
PLAY_DATA = re.compile(r'<script type="application/json" id="play-data">(.*?)</script>', re.S)

_spec = importlib.util.spec_from_file_location("gallery", ROOT / "scripts" / "make-character-gallery.py")
gallery = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gallery)
Malformed = gallery.Malformed


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
        "faq": read_faq(found),
        "prompts": instance_prompts(text),
    }


def instance_prompts(text):
    """The Give It an Instance Prompt table's prompts, word for word.

    The page's small print names the setup the README says the examples
    need, so the build stops when the README stops asking for it.
    """
    if "### Give It an Instance Prompt" not in text:
        raise Malformed("README.md has no 'Give It an Instance Prompt' section")
    body = re.split(r"^#{1,3} ", text.split("### Give It an Instance Prompt", 1)[1], maxsplit=1, flags=re.M)[0]
    for needed in TWIST_SETUP:
        if needed not in body:
            raise Malformed(f"README.md Give It an Instance Prompt no longer asks for {needed!r}, "
                            "which the page's small print names")
    prompts = re.findall(r"^\| `([^`]+)` \|", body, re.M)
    if not prompts:
        raise Malformed("README.md Give It an Instance Prompt has no `prompt` rows")
    return prompts


def read_faq(found):
    """The FAQ as [(question, [answer paragraphs])], or Malformed.

    Each `### Question?` under `## FAQ` opens a row, and the paragraphs up to
    the next one are its answer.
    """
    rows = []
    for line in section(found, "FAQ"):
        question = re.match(r"### (.+)", line)
        if question:
            rows.append((question.group(1).strip(), []))
        elif rows:
            rows[-1][1].append(line)
        elif line.strip():
            raise Malformed("README.md FAQ has text before its first `### Question`")
    if not rows:
        raise Malformed("README.md FAQ asks no `### Question`")
    faq = []
    for question, lines in rows:
        answer = paragraphs(lines)
        if not answer:
            raise Malformed(f"README.md FAQ does not answer {question!r}")
        faq.append((question, answer))
    return faq


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
    # Lane 2 draws the sink the outputs land on, so it ends this list; the
    # sheet skips it, since no caption names it.
    "director": ("Personality", "Instance Prompt", "Window titles", "Director",
                 "A Behavior", "A line", "Your agent", "ACP", "MCP",
                 "Your fidget"),
}


def how_sheet(how, suffix):
    """A per-lane <dl> of its names and captions, in README bullet order.

    Per lane because lane membership is part of the picture: one flat list
    lost whose caption belongs to which lane. A name with no caption is not
    a term; Your fidget is the sink the lanes feed, and the picture's
    aria-label names it there.
    """
    parts = []
    for name in HOW_SHEET[suffix]:
        caption = how["nodes"][HOW_NODES[name]]
        if not caption:
            continue
        parts.append(f"<dt>{html.escape(name)}</dt>")
        parts.append(f"<dd>{inline(caption)}</dd>")
    return "".join(parts)


def how_svg_label(how, suffix):
    """The lane's spoken label: its title as a sentence, then the names it draws.

    role="img" hides the drawn text, so this is all a screen reader hears. A
    period running into a colon reads oddly, so names follow as the next sentence.
    """
    title = how["lanes"][suffix]
    gap = " " if title.endswith(".") else ". "
    return f"{title}{gap}{', '.join(HOW_SHEET[suffix])}"


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
        "how_svg_always_label": html.escape(how_svg_label(how, "always")),
        "how_svg_director_label": html.escape(how_svg_label(how, "director")),
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
    return {"frames": frames, "fps": fps, "once": animation.get("loop") == "once"}


def gesture_names(declared):
    """What a cast card plays while hovered: wave, else the greet Behavior's
    Animations when it does more than talk, else react."""
    animations = declared.get("animations", {})
    if "wave" in animations:
        return ["wave"]
    greet = declared.get("behaviors", {}).get("greet", {})
    steps = greet.get("play") if isinstance(greet, dict) else None
    if isinstance(steps, list) and steps and all(isinstance(s, str) and s in animations for s in steps) \
            and set(steps) != {"talk"}:
        return steps
    return ["react"]


def gesture(package, declared, default_fps):
    """The hover gesture as data attributes: each frame's fps, as a greet's
    Animations may differ, and once when it holds its last frame."""
    parts = [loop(package, declared, name, default_fps) for name in gesture_names(declared)]
    frames = [f for part in parts for f in part["frames"]]
    fps = [part["fps"] for part in parts for _ in part["frames"]]
    once = " data-gesture-once" if len(parts) > 1 or parts[0]["once"] else ""
    return (f' data-gesture-frames="{html.escape(json.dumps(frames))}"'
            f' data-gesture-fps="{html.escape(json.dumps(fps))}"{once}')


def manifest(package):
    try:
        return tomllib.loads((package / "character.manifest").read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as broken:
        raise Malformed(f"{package.name}: character.manifest does not read as TOML — {broken}") from broken


def personality(package):
    try:
        return (package / "personality.txt").read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as missing:
        raise Malformed(f"{package.name}: has no personality.txt to say lines from") from missing


def said(package):
    """The quotes personality.txt gives after "heard to say:" or "known to say:"."""
    found = SAYS.search(personality(package))
    lines = re.findall(r'"([^"]+)"', " ".join(found.group(1).split())) if found else []
    if not lines:
        raise Malformed(f'{package.name}: personality.txt quotes nothing after "heard to say:" or "known to say:"')
    return lines


def opaque_box(art, name):
    """The smallest x, y, width, height holding every visible pixel of an 8-bit RGBA PNG."""
    width, height, depth, color, interlace = *struct.unpack(">II", art[16:24]), art[24], art[25], art[28]
    if (depth, color, interlace) != (8, 6, 0):
        raise Malformed(f"{name} is not an 8-bit RGBA PNG without interlacing")
    at, data = 8, b""
    while at < len(art):
        size, kind = struct.unpack(">I4s", art[at:at + 8])
        if kind == b"IDAT":
            data += art[at + 8:at + 8 + size]
        at += 12 + size
    raw, stride = zlib.decompress(data), width * 4
    above, xs, ys = bytearray(stride), [], []
    for y in range(height):
        kind, row = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            left, up, corner = row[i - 4] if i >= 4 else 0, above[i], above[i - 4] if i >= 4 else 0
            if kind == 1:
                row[i] = (row[i] + left) & 255
            elif kind == 2:
                row[i] = (row[i] + up) & 255
            elif kind == 3:
                row[i] = (row[i] + (left + up) // 2) & 255
            elif kind == 4:
                guess = left + up - corner
                near = min((abs(guess - left), 0, left), (abs(guess - up), 1, up), (abs(guess - corner), 2, corner))
                row[i] = (row[i] + near[2]) & 255
        seen = [x for x in range(width) if row[x * 4 + 3]]
        if seen:
            xs += (seen[0], seen[-1])
            ys.append(y)
        above = row
    if not ys:
        return [0, 0, width, height]
    return [min(xs), ys[0], max(xs) + 1 - min(xs), ys[-1] + 1 - ys[0]]


def player(package, declared, member, defaults):
    """What the playground and the page character need to play one Character."""
    loops = {name: loop(package, declared, name, defaults["fps"]) for name in PLAY_LOOPS}
    art = (package / declared["animations"]["idle"]["frames"][0]).read_bytes()
    width, height = struct.unpack(">II", art[16:24])
    scale = declared.get("scale", defaults["scale"])
    # The picker shows only this box, so padding around the art doesn't shrink the sprite.
    face = opaque_box(art, f"{package.name}/{declared['animations']['idle']['frames'][0]}")
    return {"name": member["name"], "pixel": member["pixel"], "size": [width * scale, height * scale],
            "face": [n * scale for n in face], "loops": loops, "lines": said(package)}


def read_cast(characters_root, defaults):
    """Every shipped Character as the playground plays it, the hero first."""
    packages = sorted(
        (p for p in characters_root.iterdir()
         if (p / "character.manifest").is_file() and p.name not in gallery.WITHHELD),
        key=lambda p: (p.name != HERO, p.name),
    )
    if not packages:
        raise Malformed(f"{characters_root} holds no Character Package")
    players = {}
    for package in packages:
        declared = manifest(package)
        member = {"name": declared.get("name") or package.name, "pixel": declared.get("render_mode") != "smooth"}
        players[package.name] = player(package, declared, member, defaults)
    if HERO not in players:
        raise Malformed(f"{characters_root} has no {HERO} package for the playground")
    return players


def play_tiles(characters_root, default_fps):
    figures = []
    for title, name, animation in PLAY_TILES:
        package = characters_root / name
        if not (package / "character.manifest").is_file():
            raise Malformed(f"{characters_root} has no {name} package for the {title} tile")
        declared = manifest(package)
        pixel = ' class="pixel"' if declared.get("render_mode") != "smooth" else ""
        first = loop(package, declared, animation, default_fps)["frames"][0]
        figures.append(f'<figure><div class="art"><img src="{first}" alt="{html.escape(declared.get("name") or name)}"'
                       f'{pixel}></div><figcaption>{html.escape(title)}</figcaption></figure>')
    return "\n".join(figures)


def read_moments(data, characters_root, default_fps):
    """The authored scenes, each Character reacting with a Behavior its manifest
    declares and a quote its personality.txt holds, so a sample cannot claim
    something the package does not do."""
    scenes = data.get("scenes") if isinstance(data, dict) else None
    if not isinstance(scenes, list) or not scenes:
        raise Malformed(f"{MOMENTS.name} lists no scenes")
    read = []
    for number, scene in enumerate(scenes, 1):
        where = f"{MOMENTS.name} scene {number}"
        moment = scene.get("moment") if isinstance(scene, dict) else None
        if not isinstance(moment, list) or not moment or not all(isinstance(m, str) and m for m in moment):
            raise Malformed(f"{where} sets up no moment")
        reactions = scene.get("reactions")
        if not isinstance(reactions, list) or len(reactions) != 3:
            raise Malformed(f"{where} needs three reactions, one per column")
        cast = []
        for one in reactions:
            name = one.get("character") if isinstance(one, dict) else None
            package = characters_root / str(name)
            if not isinstance(name, str) or not (package / "character.manifest").is_file() \
                    or name in gallery.WITHHELD:
                raise Malformed(f"{where} names {name!r}, which characters/ does not ship")
            if not isinstance(one.get("line"), str) or not one["line"]:
                raise Malformed(f"{where} gives {name} no line")
            declared = manifest(package)
            behavior = declared.get("behaviors", {}).get(one.get("behavior"))
            if not isinstance(behavior, dict) or not behavior.get("play"):
                raise Malformed(f"{where}: {name} declares no {one.get('behavior')!r} Behavior")
            quote = one.get("quote")
            if not isinstance(quote, str) or not quote.strip() \
                    or " ".join(quote.split()) not in " ".join(personality(package).split()):
                raise Malformed(f"{where}: {name}/personality.txt does not hold {quote!r}")
            cast.append({"name": declared.get("name") or name, "behavior": one["behavior"], "line": one["line"],
                         "quote": quote, "pixel": declared.get("render_mode") != "smooth",
                         "loop": loop(package, declared, behavior["play"][0], default_fps)})
        read.append({"moment": moment, "cast": cast})
    return read


def moments_html(scenes):
    """Every scene in the page, all but the first hidden, so the page without
    JavaScript still shows one whole moment."""
    blocks = []
    for number, scene in enumerate(scenes):
        figures = "".join(
            f'<figure><div class="say">{html.escape(c["line"])}</div>'
            f'<div class="moment-art">{sprite(c["loop"], c["name"], " class=\"pixel\"" if c["pixel"] else "")}</div>'
            f'<figcaption><b>{html.escape(c["name"])}</b> · {html.escape(c["behavior"])}</figcaption>'
            f'<blockquote><span>From its personality.txt</span>{html.escape(c["quote"])}</blockquote></figure>'
            for c in scene["cast"]
        )
        setup = "<br>".join(html.escape(m) for m in scene["moment"])
        blocks.append(f'<div class="moment"{" hidden" if number else ""}>'
                      f'<p class="moment-scene"><span>The moment</span>{setup}</p>'
                      f'<div class="moment-cast">{figures}</div></div><!-- /moment -->')
    return "\n".join(blocks)


def read_engine(sources=ENGINE):
    return {name: path.read_text(encoding="utf-8") for name, path in sources.items()}


def play_tuning(engine):
    tuning = {}
    for key, (source, name) in PLAY_TUNING.items():
        found = re.search(rf"^(?:pub(?:\(crate\))? )?const {name}: (?:f64|u32|u64) = ([\d_.]+);"
                          rf"|Self::{name}\(([\d_]+)\)", engine[source], re.M)
        if not found:
            raise Malformed(f"{source} declares no {name}")
        number = (found.group(1) or found.group(2)).replace("_", "")
        tuning[key] = float(number) if "." in number else int(number)
    return tuning


def top_level_rules(css):
    """Each top-level block of a stylesheet as (prelude, body), comments off the prelude."""
    depth, start = 0, 0
    for at, char in enumerate(css):
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                block = css[start:at + 1]
                prelude, body = block.split("{", 1)
                yield re.sub(r"/\*.*?\*/", "", prelude, flags=re.S).strip(), body[:-1]
                start = at + 1


def cue_css(css):
    """The overlay's cue rules from src/main.css, so the desk's poke ring and
    landing puff are the app's own: `.cue*` rules, `cue-*` keyframes, and the
    :root block that sets the cue colors."""
    kept = []
    for prelude, body in top_level_rules(css):
        selectors = [part.strip() for part in prelude.split(",")]
        if (all(part.startswith(".cue") for part in selectors) or prelude.startswith("@keyframes cue-")
                or (prelude == ":root" and "--cue-" in body)):
            kept.append(f"{prelude} {{{body}}}")
    if not any(rule.startswith(".cue-ring") for rule in kept):
        raise Malformed(f"{CUES.name} holds no .cue-ring rule for the desk's poke")
    return "\n".join(kept) + "\n"


def lift(css, shared, scope, wanted, prefix=""):
    """`shared`'s :root tokens scoped to `scope`, the `wanted` selectors of each
    rule in `css` behind `prefix`, and the wanted selectors no rule has."""
    tokens = [f"{scope} {{{body}}}" for prelude, body in top_level_rules(shared) if prelude == ":root"]
    kept, found = [], set()
    for prelude, body in top_level_rules(css):
        selectors = [part.strip() for part in prelude.split(",") if part.strip() in wanted]
        found.update(selectors)
        if selectors:
            kept.append(", ".join(prefix + part for part in selectors) + f" {{{body}}}")
    return tokens, kept, [rule for rule in wanted if rule not in found]


def chat_css(css, shared, wanted=CHAT_RULES):
    """The minimal Chat UI's own tokens and the `wanted` rules from
    src/chat-ui.css, scoped to `.play-chat-ui`, so the chat mock is the app's
    Chat as the pill is its quick message. Scoped because chat-ui.css styles a
    whole window: its `.bar` alone would restyle every title bar on the desk."""
    theme = [f".play-chat-ui {{{body}}}" for prelude, body in top_level_rules(css) if prelude == ".chat-ui-minimal"]
    if not theme:
        # Without it every --chat-* token is unset and the mock draws transparent.
        raise Malformed(f"{CHAT_UI.name} holds no .chat-ui-minimal block for the chat mock's colours")
    tokens, kept, missing = lift(css, shared, ".play-chat-ui", wanted, ".play-chat-ui ")
    if missing:
        raise Malformed(f"{CHAT_UI.name} holds no {', '.join(missing)} rule for the chat mock")
    return "\n".join(tokens + theme + kept) + "\n"


def bubble_css(css, shared, wanted=BUBBLE_RULES):
    """The overlay's speech bubble from src/main.css, so the page character
    speaks in the app's own bubble. The shared tokens it reads are scoped to
    `.bubble`, as the chat mock's are to `.play-chat-ui`."""
    tokens, kept, missing = lift(css, shared, ".bubble", wanted)
    if missing:
        raise Malformed(f"{CUES.name} holds no {', '.join(missing)} rule for the page character's bubble")
    return "\n".join(tokens + kept) + "\n"


def play_data(page):
    return json.loads(PLAY_DATA.search(page).group(1))


def sprite(loop_, alt, extra=""):
    frames = html.escape(json.dumps(loop_["frames"]))
    return (f'<img src="{loop_["frames"][0]}" alt="{html.escape(alt)}" '
            f'data-frames="{frames}" data-fps="{loop_["fps"]}"{extra}>')


def read_twists(data, prompts, characters_root, default_fps):
    """The twist toy: each authored prompt matched to one README prompt, with
    the one wake every Character answers and each Character's line for it.

    A line plays its Behavior's first animation when it names one, else talk.
    """
    cast = []
    for name in data.get("characters") or []:
        package = characters_root / str(name)
        if not (package / "character.manifest").is_file() or name in gallery.WITHHELD:
            raise Malformed(f"twists.json names {name!r}, which characters/ does not ship")
        declared = manifest(package)
        cast.append({"character": name, "name": declared.get("name") or name, "declared": declared, "package": package,
                     "pixel": declared.get("render_mode") != "smooth", "lines": []})
    if not cast:
        raise Malformed("twists.json names no Characters")
    picked = []
    for prompt in data.get("prompts") or []:
        found = [text for text in prompts if prompt["match"] in text]
        if len(found) != 1:
            raise Malformed(f"twists.json: {prompt['match']!r} matches {len(found)} README Instance Prompts, not one")
        if not prompt.get("wake"):
            raise Malformed(f"twists.json gives {prompt['name']} no wake")
        lines = prompt.get("lines") or {}
        if set(lines) != {one["character"] for one in cast}:
            raise Malformed(f"twists.json gives {prompt['name']} lines for {sorted(lines)}, not one per Character")
        for one in cast:
            written, animation = lines[one["character"]], "talk"
            if not written.get("line"):
                raise Malformed(f"twists.json gives {one['character']} no {prompt['name']} line")
            behavior = written.get("behavior")
            if behavior is not None:
                play = one["declared"].get("behaviors", {}).get(behavior, {}).get("play")
                if not play:
                    raise Malformed(f"twists.json: {one['character']} declares no {behavior!r} Behavior")
                animation = play[0]
            one["lines"].append({"line": written["line"],
                                 "loop": loop(one["package"], one["declared"], animation, default_fps)})
        picked.append({"name": prompt["name"], "text": found[0], "needs": prompt.get("needs") or TWIST_NEEDS,
                       "wake": prompt["wake"]})
    if not picked:
        raise Malformed("twists.json names no prompts")
    return {"prompts": picked, "characters": [{k: one[k] for k in ("character", "name", "pixel", "lines")} for one in cast]}


def twist_html(twist):
    """The toy as it stands without JavaScript: the first Character on the
    first prompt, the pickers hidden, and every line in #twist-data."""
    first, prompt = twist["characters"][0], twist["prompts"][0]
    said = first["lines"][0]
    pixel = ' class="pixel"' if first["pixel"] else ""
    characters = "".join(
        f'<button type="button" data-character="{c["character"]}" aria-pressed="{str(i == 0).lower()}">'
        f'{html.escape(c["name"])}</button>' for i, c in enumerate(twist["characters"]))
    prompts = "".join(
        f'<button type="button" data-prompt="{i}" aria-pressed="{str(i == 0).lower()}">{html.escape(p["name"])}</button>'
        for i, p in enumerate(twist["prompts"]))
    return "\n".join([
        '<div class="twist-pick" id="twist-pick" hidden>',
        f'<div role="group" aria-label="Character"><span>Character</span>{characters}</div>',
        f'<div role="group" aria-label="Prompt"><span>Prompt</span>{prompts}</div>',
        "</div>",
        '<figure class="twist-layer"><figcaption>What you\'d type in the app\'s Prompt tab</figcaption>'
        f'<blockquote class="twist-prompt" id="twist-prompt">{html.escape(prompt["text"])}</blockquote></figure>',
        f'<p class="twist-needs" id="twist-needs">{html.escape(prompt["needs"])}</p>',
        '<figure class="twist-scene">',
        '<p class="twist-when"><span class="twist-what">What happened:</span> '
        f'<span id="twist-when">{html.escape(prompt["wake"])}</span></p>',
        # The bubble types a character at a time; screen readers get the whole line from the live region.
        f'<div class="say" id="twist-say" aria-hidden="true">{html.escape(said["line"])}</div>'
        f'<p class="twist-live" id="twist-live" aria-live="polite">{html.escape(said["line"])}</p>',
        f'<div class="twist-art"><img id="twist-sprite" src="{said["loop"]["frames"][0]}" '
        f'alt="{html.escape(first["name"])}"{pixel}></div>',
        "</figure>",
        '<div class="twist-foot">',
        f'<p class="twist-label">{TWIST_LABEL}</p>',
        '<a href="https://github.com/omesser/fidget#give-it-an-instance-prompt">Write your own: how →</a>',
        "</div>",
        # Escaped so the JSON can never close the <script> element it sits in.
        f'<script type="application/json" id="twist-data">{json.dumps(twist).replace("<", "\\u003c")}</script>',
    ])


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


def short_line(bullet):
    """The line a Highlights bullet contributes: its bold title when it opens
    with one, else the bullet as written.

    A release writes a title as the short form of the bullet, and the detail for
    the Release page after it. The strip has three nouns a visitor can take in at
    a glance; the detail is a link away."""
    found = TITLE.match(bullet)
    return found["title"].strip() if found else bullet


def release_lines(body):
    """Up to three strip lines: the short form of each bullet under the release's
    `## Highlights` heading, or none. GitHub writes its own notes for a release —
    commit subjects, "by @author in <pull request>" — and those read as
    housekeeping to a visitor, so a release with no Highlights shows no strip at
    all."""
    body = re.sub(r"\r\n?", "\n", body)
    heading = HIGHLIGHTS.search(body)
    if not heading:
        return []
    section = re.split(r"^##[ \t]", body[heading.end():], maxsplit=1, flags=re.M)[0]
    bullets = [re.sub(BULLET, "", line).strip() for line in section.splitlines()
               if BULLET.match(line)]
    return [short_line(bullet) for bullet in bullets if bullet][:3]


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


def site_nav(hero):
    return NAV.read_text(encoding="utf-8").replace("{{brand}}", hero["loops"]["idle"]["frames"][0])


def stamp_nav(out, nav):
    """Put the site header after the <body> of every page in out but the landing page."""
    for page in sorted(out.glob("*.html")):
        if page.name == PAGE:
            continue
        text = page.read_text(encoding="utf-8")
        if 'class="site-nav"' in text:
            continue
        stamped, found = re.subn(r"<body[^>]*>", lambda body: body.group(0) + "\n" + nav, text, count=1)
        if not found:
            raise Malformed(f"{page.name} has no <body> to put the site header in")
        page.write_text(stamped, encoding="utf-8")


def stamp_counter(out):
    """Put the visit counter right before the </head> of every page in out."""
    for page in sorted(out.glob("*.html")):
        text = page.read_text(encoding="utf-8")
        if COUNTER in text:
            continue
        stamped, found = re.subn(r"</head>", lambda head: COUNTER + "\n" + head.group(0), text, count=1)
        if not found:
            raise Malformed(f"{page.name} has no </head> to put the visit counter in")
        page.write_text(stamped, encoding="utf-8")


def render(readme_text, characters_root, rust_source, shell, release=None, engine=None):
    words = read_readme(readme_text)
    _, defaults = gallery.from_rust(rust_source)
    players = read_cast(characters_root, defaults)
    hero = players[HERO]
    desk = {"cast": players, "hero": HERO, "tuning": play_tuning(engine or read_engine())}

    figures = "\n".join(
        f'<a class="card" href="characters.html#{name}"><figure><div class="art">'
        + sprite(c["loops"]["walk"], c["name"], (' class="pixel"' if c["pixel"] else "")
                 + gesture(characters_root / name, manifest(characters_root / name), defaults["fps"]))
        + f'</div><figcaption>{html.escape(c["name"])}</figcaption></figure></a>'
        for name, c in players.items()
    )
    slots = {
        "headline": html.escape(words["headline"], quote=False),
        "description": html.escape(re.sub(r"[*`]", "", re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", words["lede"]))),
        "lede": inline(words["lede"]),
        "notes": " ".join(inline(n) for n in words["notes"]),
        "install_brew": (
            f'Or install with Homebrew: <span class="cmd"><code>{html.escape(words["brew"])}</code> '
            f'<button type="button" class="copy" data-copy="{html.escape(words["brew"])}">Copy</button></span>'
        ) if words["brew"] else "",
        "features": "\n".join(f"<div><b>{inline(t)}</b><p>{inline(b)}</p></div>" for t, b in words["features"]),
        "harnesses": "".join(f"<li>{html.escape(h)}</li>" for h in words["harnesses"]),
        "faq": "\n".join(
            f"<details><summary>{html.escape(q, quote=False)}</summary>"
            + "".join(f"<p>{inline(p)}</p>" for p in answer) + "</details>"
            for q, answer in words["faq"]
        ),
        # The lines name shipped Characters, so they check against characters/
        # even when a fixture cast fills the rest of the page.
        "twist": twist_html(read_twists(json.loads(TWISTS.read_text(encoding="utf-8")), words["prompts"],
                                        CHARACTERS, defaults["fps"])),
        "count": (COUNT[len(players)] if len(players) < len(COUNT) else str(len(players)))
        + (" character ships" if len(players) == 1 else " characters ship"),
        "cast": figures,
        **download_slots(release),
        "whats_new": whats_new(release),
        "nav": site_nav(hero),
        "hero": sprite(hero["loops"]["sit"], f'{hero["name"]}, perched on a window',
                       ' id="hero-sprite" aria-describedby="hero-say"'),
        **how_slots(words["how"], shell),
        "cue_css": cue_css(CUES.read_text(encoding="utf-8")),
        "bubble_css": bubble_css(CUES.read_text(encoding="utf-8"), CHAT_SHARED.read_text(encoding="utf-8")),
        "chat_css": chat_css(CHAT_UI.read_text(encoding="utf-8"), CHAT_SHARED.read_text(encoding="utf-8")),
        "play_tiles": play_tiles(characters_root, defaults["fps"]),
        # The scenes name shipped Characters, so they check against characters/
        # even when a fixture cast fills the rest of the page.
        "moments": moments_html(read_moments(json.loads(MOMENTS.read_text(encoding="utf-8")),
                                             CHARACTERS, defaults["fps"])),
        "play_name": html.escape(hero["name"]),
        "play_sprite": (f'<img src="{hero["loops"]["idle"]["frames"][0]}" alt="{html.escape(hero["name"])}" '
                        f'width="{hero["size"][0]}" height="{hero["size"][1]}"'
                        + (' class="pixel"' if hero["pixel"] else "") + ">"),
        # Escaped so the JSON can never close the <script> element it sits in.
        "play_data": json.dumps(desk).replace("<", "\\u003c"),
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


def self_check():
    """The parsing the page's honesty rests on, against fixtures."""
    rust = gallery.RUST.read_text(encoding="utf-8")
    shell = SHELL.read_text(encoding="utf-8")
    readme = README.read_text(encoding="utf-8")

    def fails(text, characters=CHARACTERS, **extra):
        try:
            render(text, characters, rust, shell, **extra)
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
    assert "FAQ" in (fails(readme.replace("## FAQ", "## Asked Often")) or ""), \
        "a README with no FAQ section built a page"
    assert "does not answer" in (fails(readme.replace("## FAQ\n", "## FAQ\n\n### Unanswered?\n")) or ""), \
        "a FAQ question with no answer built a page"

    authored = json.loads(MOMENTS.read_text(encoding="utf-8"))
    fps = gallery.from_rust(rust)[1]["fps"]

    def moment_fails(change):
        data = json.loads(json.dumps(authored))
        change(data["scenes"][0])
        try:
            read_moments(data, CHARACTERS, fps)
        except Malformed as caught:
            return str(caught)
        return None

    scenes = read_moments(authored, CHARACTERS, fps)
    assert len(scenes) >= 3 and all(len(s["cast"]) == 3 for s in scenes), "the moments lost a scene or a column"
    assert scenes[0]["cast"][0]["loop"]["frames"][0].startswith("characters/cat/"), \
        "a moment's sprite is not its Character's art"
    assert "declares no 'conquer' Behavior" in (
        moment_fails(lambda s: s["reactions"][0].update(behavior="conquer")) or ""), \
        "a moment with a Behavior the manifest lacks built a page"
    assert "does not hold" in (moment_fails(lambda s: s["reactions"][0].update(quote="Loves baths.")) or ""), \
        "a moment quoting words its personality.txt lacks built a page"
    assert "does not ship" in (moment_fails(lambda s: s["reactions"][0].update(character="garfield")) or ""), \
        "a moment naming a Character that does not ship built a page"
    assert "three reactions" in (moment_fails(lambda s: s["reactions"].pop()) or ""), \
        "a moment with two columns built a page"
    assert "no line" in (moment_fails(lambda s: s["reactions"][0].update(line="")) or ""), \
        "a moment with a silent Character built a page"

    words = read_readme(readme)
    assert "claude" in words["harnesses"] and "anything else" not in words["harnesses"]
    assert 5 <= len(words["faq"]) <= 10, f"the README FAQ asks {len(words['faq'])} questions, not 5 to 10"
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
    greets = {"animations": {"react": {}, "talk": {}}, "behaviors": {"greet": {"play": ["talk"]}}}
    assert gesture_names(greets) == ["react"], "a greet that only talks became the hover gesture"
    greets["behaviors"]["greet"]["play"] = ["react", "talk"]
    assert gesture_names(greets) == ["react", "talk"], "the hover gesture is not the greet's Animations"
    greets["animations"]["wave"] = {}
    assert gesture_names(greets) == ["wave"], "a wave Animation is not the hover gesture"
    assert inline("[a](./docs/x.md#y) `b` **c**") == (
        f'<a href="{REPO}/blob/main/docs/x.md#y">a</a> <code>b</code> <b>c</b>')
    assert inline("[a](#harness-support) <i>") == f'<a href="{REPO}#harness-support">a</a> &lt;i&gt;'

    release = {"tagName": "v9", "assets": [{"name": n, "url": f"{REPO}/releases/download/v9/{n}"}
                                           for n in ("F_9_amd64.deb", "F_9_amd64.AppImage", "F_9_aarch64.dmg")]}

    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        art = (CHARACTERS / HERO / "frames" / "idle-0.png").read_bytes()
        played = set(PLAY_LOOPS) | {animation for _, _, animation in PLAY_TILES}

        def fixture(name, title, walk="frames/idle-0.png", without=(),
                    personality='It has been heard to say: "Hello\nthere." — "Beep."\n\nA "Mad Cat" elsewhere.\n'):
            root = scratch / name
            (root / "frames").mkdir(parents=True, exist_ok=True)
            (root / "frames" / "idle-0.png").write_bytes(art)
            loops = "\n".join(f'[animations.{n}]\nframes = ["frames/idle-0.png"]\nfps = 10\nloop = "once"'
                              for n in sorted(played - {"walk"} - set(without)))
            (root / "character.manifest").write_text(
                f'name = "{title}"\n{loops}\n[animations.walk]\nframes = ["{walk}"]\n', encoding="utf-8")
            (root / "personality.txt").unlink(missing_ok=True)
            if personality is not None:
                (root / "personality.txt").write_text(personality, encoding="utf-8")
            return root

        for _, name, _ in PLAY_TILES:
            if name != HERO:
                fixture(name, name.title())
        package = fixture(HERO, "Solo", walk="frames/walk-0.png")
        assert "walk frame" in (fails(readme, scratch) or ""), "a frame the package lacks reached the page"

        (package / "frames" / "walk-0.png").write_bytes(art)
        page = render(readme, scratch, rust, shell)
        assert f'<a class="card" href="characters.html#{HERO}">' in page, "a cast card does not link its gallery section"
        engine = read_engine()
        heavy = dict(engine)
        heavy["engine.rs"] = re.sub(r"const GRAVITY: f64 = [\d_.]+;", "const GRAVITY: f64 = 1234.0;", engine["engine.rs"])
        desk = play_data(render(readme, scratch, rust, shell, engine=heavy))
        assert desk["tuning"]["gravity"] == 1234, "the desk's gravity is not the Engine's"
        slow = dict(engine)
        slow["input.rs"] = engine["input.rs"].replace("with_double_click_ms(400)", "with_double_click_ms(333)")
        slow["main.rs"] = re.sub(r"const HOVER_DELAY_MS: u64 = [\d_]+;", "const HOVER_DELAY_MS: u64 = 2222;", engine["main.rs"])
        timing = play_data(render(readme, scratch, rust, shell, engine=slow))["tuning"]
        assert (timing["doubleClickMs"], timing["hoverDelayMs"]) == (333, 2222), \
            "the desk's double-click or hover delay is not the app's"
        assert desk["cast"][HERO]["loops"]["land"] == {"frames": [f"characters/{HERO}/frames/idle-0.png"], "fps": 10,
                                                       "once": True}, "the desk's land loop is not the manifest's"
        # One opaque pixel at (2, 1) in a 4x3 frame, its rows Up- and Paeth-filtered.
        rows = (b"\x02" + bytes(16) + b"\x04" + bytes(8) + b"\x01\x02\x03\xff\xff\xfe\xfd\x01"
                + b"\x02" + bytes(8) + b"\xff\xfe\xfd\x01" + bytes(4))
        dot = (b"\x89PNG\r\n\x1a\n" + struct.pack(">I4sIIBBBBBI", 13, b"IHDR", 4, 3, 8, 6, 0, 0, 0, 0)
               + struct.pack(">I4s", len(zlib.compress(rows)), b"IDAT") + zlib.compress(rows) + bytes(4))
        assert opaque_box(dot, "dot") == [2, 1, 1, 1], f"the picker crops a frame to {opaque_box(dot, 'dot')}"
        assert desk["cast"][HERO]["lines"] == ["Hello there.", "Beep."], \
            "the lines are not the quotes after heard to say:, up to the blank line"
        assert list(desk["cast"]) == [HERO, *sorted(p.name for p in scratch.iterdir() if p.name != HERO)], \
            "the picker's cast is not every Character, the hero first"
        unthrowable = dict(engine)
        unthrowable["input.rs"] = re.sub(r"const THROW_MIN_SPEED:[^\n]*\n", "", engine["input.rs"])
        assert "THROW_MIN_SPEED" in (fails(readme, scratch, engine=unthrowable) or ""), \
            "a desk with no Throw speed to read built a page"
        assert '<figcaption>Throw</figcaption>' in page and f'src="characters/cat/frames/idle-0.png"' in page, \
            "the Throw tile is not the Cat's first frame"
        for name in HOW_NODES:
            assert name in page, f"{name} did not reach the page"
        for title in HOW_LANES:
            assert title in page, f"{title} did not reach the page"
        assert "attach over ACP on stdio" in page and "the Harness calls back over MCP" in page
        assert "Opening wake" in page
        assert "not a live dump" in page, "the wake sample lost its sample lead-in"
        labels = re.findall(r'aria-label="([^"]*)"', page)
        assert ("Always on. No AI needed. Frame loop, You, Your fidget" in labels
                and any(label.startswith("Optional: the Director.")
                        and label.endswith("ACP, MCP, Your fidget") for label in labels)
                and '<dt>Your fidget</dt>' not in page), \
            "the picture's labels lost the sink or the sheet kept an empty term"
        assert "A pet first. The AI is the part that makes it someone." in page
        assert not re.search(r"<script[^>]*mermaid", page) and 'class="mermaid"' not in page
        assert "Six characters ship with Fidget" in page, "the cast count is not the package count"
        assert "<figcaption>Solo</figcaption>" in page
        faq = re.search(r'<section id="faq">(.*?)</section>', page, re.S)
        assert faq and faq.group(1).count("<details><summary>") == len(words["faq"]), \
            "the Questions section lost a README question"
        assert page.index('id="faq"') > page.index('id="your-own"') \
            and page.index('id="faq"') < page.index("<footer>"), "the Questions section is not above the footer"
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

        fixture(HERO, "Solo", walk="frames/walk-0.png", without=("land",))
        assert "land" in (fails(readme, scratch) or ""), "a desk Character with no land loop built a page"
        fixture(HERO, "Solo", walk="frames/walk-0.png")
        fixture("nim", "Nim", without=("talk",))
        assert "nim: declares no talk" in (fails(readme, scratch) or ""), "a page Character with no talk loop built a page"
        fixture("nim", "Nim", personality='Has been known to say: "BEHOLD." — "Fire." Warriors do not waste words.\n')
        assert play_data(render(readme, scratch, rust, shell))["cast"]["nim"]["lines"] == ["BEHOLD.", "Fire."], \
            "known to say: lost its quotes, or prose after them became a line"
        fixture("nim", "Nim", personality=None)
        assert "nim: has no personality.txt" in (fails(readme, scratch) or ""), "a page Character with no lines built a page"
        fixture("nim", "Nim", personality='It says "hi" but never what it has been heard to.\n')
        assert "nim: personality.txt quotes nothing" in (fails(readme, scratch) or ""), \
            "a personality.txt with no heard to say: built a page"
        fixture("nim", "Nim")
        fixture("bmo", "Bmo", without=("sleep",))
        assert "bmo: declares no sleep" in (fails(readme, scratch) or ""), "a picker Character with no sleep loop built a page"
        fixture("bmo", "Bmo")
        fixture("cat", "Cat", without=("fall",))
        assert "cat: declares no fall" in (fails(readme, scratch) or ""), "a tile with no art built a page"

    assert bubble_css(".bubble { color: red; }\n.bubble.visible, .cue { opacity: 1; }\n.bubble-content { a: b; }\n",
                      ":root { --shared-ink: #fff; }\n", wanted=(".bubble", ".bubble-content")) == (
        ".bubble { --shared-ink: #fff; }\n.bubble { color: red; }\n.bubble-content { a: b; }\n"), \
        "the bubble lifted a selector it does not want, or lost its shared tokens"
    try:
        bubble_css(".bubble { color: red; }", "")
    except Malformed:
        pass
    else:
        raise AssertionError("a main.css with no bubble tail built a page")
    assert cue_css(":root { --cue-ink: #000; }\n.bubble { color: red; }\n.cue-ring { opacity: 1; }\n"
                   "@keyframes cue-poke { to { opacity: 0; } }\n@media (x) { .cue { a: b; } }\n") == (
        ":root { --cue-ink: #000; }\n.cue-ring { opacity: 1; }\n@keyframes cue-poke { to { opacity: 0; } }\n"), \
        "the desk lifted a rule that is not a cue, or dropped one that is"
    try:
        cue_css(".bubble { color: red; }")
    except Malformed:
        pass
    else:
        raise AssertionError("a stylesheet with no cue rules gave the desk no cues and passed")

    sample = (".chat-ui-minimal { --chat-panel: var(--shared-panel); }\n.bar { height: 9px; }\n"
              ".composer textarea,\n.composer button { flex: 1; }\n.composer textarea:focus-visible { outline: 0; }\n")
    assert chat_css(sample, ":root { --shared-panel: #14171e; }\n", wanted=(".composer textarea",)) == (
        ".play-chat-ui { --shared-panel: #14171e; }\n.play-chat-ui { --chat-panel: var(--shared-panel); }\n"
        ".play-chat-ui .composer textarea { flex: 1; }\n"), \
        "the chat mock lifted a rule it does not use, or did not scope one it does"
    try:
        chat_css(sample, ":root { --shared-panel: #14171e; }\n", wanted=(".composer textarea", ".said"))
    except Malformed as err:
        assert ".said" in str(err)
    else:
        raise AssertionError("a Chat UI with no .said rule gave the chat mock no bubble and passed")
    try:
        chat_css(sample.replace(".chat-ui-minimal", ".chat-ui-modern"), ":root { --shared-panel: #14171e; }\n",
                 wanted=(".composer textarea",))
    except Malformed as err:
        assert ".chat-ui-minimal" in str(err)
    else:
        raise AssertionError("a Chat UI with no .chat-ui-minimal block gave the chat mock no colours and passed")

    assert downloads(release) == {
        "macos": (f"{REPO}/releases/download/v9/F_9_aarch64.dmg", "v9"),
        "windows": (LATEST, None),
        "linux": (f"{REPO}/releases/download/v9/F_9_amd64.AppImage", "v9"),
    }, "a button linked an asset that is not its own, or named a version it does not link"

    notes = ("## Highlights\n"
             "- **Chat can be a perch now:** a failed turn shows its error\n"
             "- Tool calls read clearly\n")
    assert release_lines(notes) == ["Chat can be a perch now", "Tool calls read clearly"], \
        "the strip did not read a release's Highlights list, or lost a bullet's short form"
    assert release_lines("## Highlights\n- **Chat** can be a `perch` now\n") == ["**Chat** can be a `perch` now"], \
        "a highlighted first word with no title colon was read as a title"
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

    with tempfile.TemporaryDirectory() as out:
        out = pathlib.Path(out)
        (out / "a.html").write_text('<html><body class="x"><p>a</p></body></html>', encoding="utf-8")
        (out / PAGE).write_text("<body>landing</body>", encoding="utf-8")
        stamp_nav(out, '<header class="site-nav">N</header>')
        stamp_nav(out, '<header class="site-nav">N</header>')
        assert (out / "a.html").read_text(encoding="utf-8") == \
            '<html><body class="x">\n<header class="site-nav">N</header><p>a</p></body></html>', \
            "a page did not get the site header once, right after its <body>"
        assert (out / PAGE).read_text(encoding="utf-8") == "<body>landing</body>", \
            "the landing page got a second header"
        (out / "b.html").write_text("<p>no body</p>", encoding="utf-8")
        try:
            stamp_nav(out, "N")
        except Malformed:
            pass
        else:
            raise AssertionError("a page with no <body> took no header and passed")

    with tempfile.TemporaryDirectory() as out:
        out = pathlib.Path(out)
        (out / "c.html").write_text("<html><head><title>c</title></head><body></body></html>", encoding="utf-8")
        (out / PAGE).write_text("<head>\n</head><body>landing</body>", encoding="utf-8")
        stamp_counter(out)
        stamp_counter(out)
        assert (out / "c.html").read_text(encoding="utf-8") == \
            f"<html><head><title>c</title>{COUNTER}\n</head><body></body></html>", \
            "a page did not get the visit counter once, right before its </head>"
        assert (out / PAGE).read_text(encoding="utf-8") == f"<head>\n{COUNTER}\n</head><body>landing</body>", \
            "the landing page did not get the visit counter once"
        (out / "d.html").write_text("<body>no head</body>", encoding="utf-8")
        try:
            stamp_counter(out)
        except Malformed:
            pass
        else:
            raise AssertionError("a page with no </head> took no visit counter and passed")

    authored = json.loads(TWISTS.read_text(encoding="utf-8"))
    fps = gallery.from_rust(rust)[1]["fps"]
    twist = read_twists(authored, words["prompts"], CHARACTERS, fps)
    assert len(twist["characters"]) == 5 and all(len(c["lines"]) == 4 for c in twist["characters"]), \
        "the twist toy lost a Character or a prompt"
    assert all(p["text"] in readme for p in twist["prompts"]), "a twist prompt is not the README's, word for word"

    def twist_fails(change, text=readme):
        data = json.loads(json.dumps(authored))
        change(data)
        try:
            read_twists(data, instance_prompts(text), CHARACTERS, fps)
        except Malformed as caught:
            return str(caught)
        return None

    for change, wanted in (
        (lambda d: d["prompts"][0].update(match="rubber chicken"), "matches 0 README"),
        (lambda d: d["prompts"][0].update(match="you"), "not one"),
        (lambda d: d["characters"].__setitem__(0, "garfield"), "does not ship"),
        (lambda d: d["prompts"][2]["lines"]["cat"].update(behavior="conquer"), "declares no 'conquer'"),
        (lambda d: d["prompts"][3]["lines"].pop("black-mage"), "not one per Character"),
        (lambda d: d["prompts"][1]["lines"]["trump"].update(line=""), "no Food critic line"),
        (lambda d: d["prompts"][2].pop("wake"), "no wake"),
    ):
        assert wanted in (twist_fails(change) or ""), f"a broken twists.json passed, wanting {wanted!r}"
    unset = readme.replace("**Window and application names** consent", "**Window names** setting")
    assert "no longer asks for" in (twist_fails(lambda d: None, unset) or ""), \
        "a README that dropped the consent kept the page's small print"

    print(f"self-check: {len(words['features'])} features, {len(words['harnesses'])} Harnesses, "
          f"{len(words['faq'])} questions, checks passed")


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
    rust = gallery.RUST.read_text(encoding="utf-8")
    try:
        page = render(arguments.readme.read_text(encoding="utf-8"), CHARACTERS,
                      rust, SHELL.read_text(encoding="utf-8"), release)
        arguments.out.mkdir(parents=True, exist_ok=True)
        (arguments.out / PAGE).write_text(page, encoding="utf-8")
        players = read_cast(CHARACTERS, gallery.from_rust(rust)[1])
        stamp_nav(arguments.out, site_nav(players[HERO]))
        stamp_counter(arguments.out)
    except Malformed as broken:
        sys.exit(f"landing page: {broken}")
    print(f"{arguments.out / PAGE}: generated from {arguments.readme.name} and {CHARACTERS.name}/")


if __name__ == "__main__":
    main()
