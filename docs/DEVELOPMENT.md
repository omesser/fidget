# Development Guide

Toolchains, hooks, trace variables, verification, Character Packages, and platform dependencies. Completers, Harnesses, and the MCP server are in [harness.md](./harness.md).

## Quick Start

```sh
# Clone and run
git clone https://github.com/omesser/fidget.git fidget
cd fidget
cargo run -p fidget
```

## Local Data

The data folder:

- macOS: `~/Library/Application Support/fidget`
- Linux: `~/.local/share/fidget`
- Windows: `%APPDATA%\fidget` (e.g. `C:\Users\<user>\AppData\Roaming\fidget`)

It holds two files:

- **`memory.md`**: everything the fidgets know about you, shared across all Character Instances. Human-editable Markdown with no size limit.
- **`action-log.jsonl`**: one JSON line per Harness action (prompts, tool calls, usage). Append-only, rotated.

### Action Log rotation

[`file-rotate`](https://crates.io/crates/file-rotate) 0.8.x rotates the log.

- A write that pushes the current file past 2 MB rotates it (`ContentLimit::BytesSurpassed`). `ContentLimit::Bytes` would split a write mid-line and break JSONL.
- Files are `action-log.jsonl`, then `.1` through `.10`. The oldest drops off. Disk ceiling is about 22 MB.
- One writer only. Concurrent writes from several processes are not supported.
- Rotation and write failures are dropped, with at most one stderr line per 60s. The log must never block a Director turn.
- JSON is serialized with `serde_json::to_string` before the write.

To inspect it:

- macOS: `tail -f ~/Library/Application\ Support/fidget/action-log.jsonl`
- Linux: `tail -f ~/.local/share/fidget/action-log.jsonl`
- Windows (PowerShell): `Get-Content -Wait -Tail 50 $env:APPDATA\fidget\action-log.jsonl`
- The **Action Log…** row in the tray and sprite menus opens the current file in the system editor.

### Memory size

Memory has no automatic size limit, on purpose: it is user-owned, and auto-deletion or write refusal would break that. Trim it in any editor, wipe it in Settings (a timestamped backup is kept), and watch what the Harness writes in the Action Log.

## Toolchains

| Toolchain | Needed for | Needed to build? |
|---|---|---|
| **Rust** | Core crate and Tauri shell | yes |
| **Python** | pre-commit, frame generators, pet importer | no |
| **Node** | Renderer unit tests | no |

Any Node with `node --test` works. No package manager, no `node_modules`; `package.json` only declares ESM.

## Pre-commit Hooks

```sh
pre-commit install
```

Covers whitespace, YAML/JSON/TOML, spelling, shell (shfmt + shellcheck), `cargo fmt`, and `cargo clippy --workspace --all-targets -- -D warnings`, as CI runs it. The toolchain is pinned in `rust-toolchain.toml`.

## Trace Variables

All off by default. Switches take `1`/`on`/`true`/`yes` or `0`/`off`/`false`/`no`, in any case.

| Variable | Effect |
|---|---|
| `FIDGET_TRACE_HITTEST` | Click-through decisions |
| `FIDGET_TRACE_FRAMES` | Engine frames per tick: `Grounded pos(x,y)`, `Dragged`, `Perched`, animation |
| `FIDGET_TRACE_DIRECTOR` | Session wakes: prompt, reply, Behavior played or refused |
| `FIDGET_TRACE_ENGINE` | Behavior, Primitive, Animation, and State changes |
| `FIDGET_TRACE_CADENCE` | Bench-only, with no Settings row. The overlay's display frames as `cadence:` lines and the loop's tick count as `cadence-ticks:` once a second, for `scripts/bench-frame-cadence-macos.sh`. Read at launch only |
| `FIDGET_TRACE_WINDOWS` | Windows only, set to any value: window count and the first 3 bounds on first read |
| `FIDGET_DEBUG_REINFORCE` | Windows only: debug logging for overlay `reinforce_overlay` calls (DWM frame extension and style restoration) |
| `FIDGET_CAPTURABLE` | `1` forces the overlay into screen captures, `0` excludes it. Overrides Settings → Presence → "Appear in screenshots and screen shares". For verify scripts and CI. |

## Verifying the Overlay

### Unit Tests

```sh
cargo test -p fidget-core     # Pure core, builds anywhere
cargo test                    # Everything including the platform shell
node --test tests/*.test.js   # Renderer and design pages
```

### Automated Scripts

```sh
scripts/verify-overlay.sh       # macOS: overlay, physics, hit-testing
scripts/verify-overlay-x11.sh   # Linux X11: EWMH states, click-through
scripts/verify-overlay-win.ps1  # Windows: WS_EX_NOACTIVATE, Perch on dual display
node scripts/chat-ask-order.mjs # Chat surface in headless Chromium: a typed turn, an ask, the answer under it
```

The overlay scripts need a real desktop. `chat-ask-order.mjs` loads `src/chat.html` with `window.__TAURI__` stubbed, so it needs no app and activates no window. It needs a headless Chromium; `FIDGET_CHROME` names one other than Playwright's shell.

```sh
scripts/verify-settings-webview-select-macos.sh     # macOS: AI-source <select> opens above the overlay (#849)
scripts/verify-settings-keyboard-webview.sh         # macOS: keyboard-only Settings (#848)
scripts/verify-settings-webview-clipboard-macos.sh  # macOS: Copy on the BYO row writes the pasteboard (#855)
scripts/verify-settings-webview-phase2-win.ps1      # Windows: window, tabs, Sound round-trip, z-order
scripts/verify-settings-zorder-x11.sh               # Linux: Settings above the overlay
scripts/verify-anchor-taskbar-win.ps1               # Windows: no auto-open Settings, taskbar click opens Settings
scripts/bench-rss-macos.sh                          # macOS: resident set over a run, per process
scripts/bench-rss-linux.sh                          # Linux: RSS baseline (needs a working display)
scripts/bench-rss-windows.ps1                       # Windows: RSS baseline
scripts/bench-frame-cadence-macos.sh                # macOS and Linux: display-frame cadence and Engine tick rate
```

CI does not run the Settings sittings. `verify-settings-keyboard-webview.sh` drives Tab, Space, Enter, and Escape, compares each tab's focus sequence with the AX dump, requires a focused still per tab and a committed `<select>` value, and writes a pass/fail table and stills under `.verify/`. Its pure checks run on fixtures in `scripts/test_verify_settings_keyboard.sh`. The AX / UIA / AT-SPI helpers are `scripts/ax-settings.swift`, `scripts/ax-settings-win.ps1`, and `scripts/ax-settings-linux.py`.

Build the debug binary, then run from the repo root:

```sh
cargo build -p fidget
./scripts/verify-settings-keyboard-webview.sh
.\scripts\verify-settings-webview-phase2-win.ps1
```

The scripts use `target/debug/fidget`. `FIDGET_VERIFY_BIN` names another binary.

On macOS, grant Accessibility to the terminal or IDE that runs a sitting (System Settings > Privacy & Security > Accessibility). Without it the helper exits before it dumps the window. UI Automation on Windows needs no grant.

The bench-rss scripts measure rather than check. They sample the app and its webview helpers and print RSS and peak memory. The default is brief (settle ~3s, sample ~10s). `--research` (bash) or `-Research` (PowerShell) runs the long soak (settle 300s, sample 300s). Results are in [docs/research/memory-rss-and-multi-monitor.md](research/memory-rss-and-multi-monitor.md).

The frame-cadence bench needs `FIDGET_BENCH_GREEN_LIGHT=1` because every scenario launches fidget. On macOS the still-tick rate of one binary drifts by several Hz over a few minutes, so two separate runs do not compare. Pass `--bin` twice to A/B two builds in one run. Each round runs every scenario on both, alternating which goes first, and the report gives each side's mean over `--rounds` (default 3), each round, and B minus A:

```sh
FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-frame-cadence-macos.sh idle --bin /tmp/fidget-main --bin /tmp/fidget-branch --rounds 3
```

### Manual Verification Checklist

Only the window server can answer these. Run the app, then confirm:

1. **Clicks pass through empty space.** Click anywhere the sprite is not. The click lands underneath.
2. **Clicks on the sprite do not pass through.** Click the sprite's body. The window underneath gets nothing.
3. **Typing is never interrupted.** Type in another application and click the sprite mid-sentence. Every keystroke reaches that application and focus never moves.
4. **Follows you across Spaces.** Switch Spaces. The sprite is on the new one, in the same place.
5. **Motion is continuous.** Watch it fall. It slides rather than jumps, and does not judder at a window's edge.
6. **The art is crisp.** On a Retina display the pixels are hard squares, all the same size. Blur means the integer scale or nearest-neighbour filtering was lost.
7. **It rests on the Dock, not behind it.** Its feet stand on the Dock's top edge. Turn on Dock auto-hiding: within a poll it falls to the bottom of the screen. Turn it off and it is lifted again.
8. **Declared cadence is honoured.** Give a copy of Black Mage a faster idle `fps`. The idle is visibly faster than at the declared 1.
9. **A click makes it react.** Click once without moving. It plays `react` for about half a second, then resumes.
10. **Press and drag picks it up.** It follows the cursor. Release over a window and it lands on that window's top edge.
11. **A flick throws it.** Release while moving and it leaves on an arc. Hold still before releasing and it drops straight down.
12. **It can be put down over the Dock, and does not stay there.** Drop it over the Dock. It settles back onto the Dock's top edge, fully visible.
13. **A window you drag slowly carries it.** With the sprite on a window's top edge, drag the window slowly. The sprite rides the edge and keeps its place.
14. **A window you fling leaves it behind.** Throw the same window by its title bar. The sprite stays where it stood, in the air, and falls.
15. **Two Characters are two companions.** Run BMO, then Nim. BMO hums through a four-frame singing loop; Nim eases through six, blinks, and carries a translucent shadow.
16. **Fullscreen takes the screen and the fidget leaves it.** Enter fullscreen in any app. Within about a tenth of a second the sprite fades out, and fades back when you leave.
17. **Ordinary window switching changes nothing.** Command-Tab, open, close, and drag windows, switch Spaces. The sprite never blinks.
18. **The hotkey puts it away and brings it back at once.** Control-Option-Command-B hides it with no fade. Press again and it is back.
19. **The hotkey outranks the rules.** Hide it with the hotkey, then enter and leave fullscreen. It stays away.
20. **It can leave a real screen share.** Turn off Settings → Presence → "Appear in screenshots and screen shares", then share your whole screen in Zoom, Meet, or Teams. The sprite is on your screen and not in theirs.

With a second display:

21. **A fidget on a seam is whole.** Hold the sprite across the boundary, half on each display. Both halves are drawn and meet.
22. **Either half can be clicked.** Click the half on each display in turn. Both pick it up.
23. **A display can come and go.** Unplug a display while running. The sprite carries on. Plug it back in and it can be dragged onto it within a second or so.

For multiple instances, start with `FIDGET_INSTANCES="bmo:One,bmo:Two,nim:Nim"` and confirm each fidget acts independently.

## Character Packages

Search paths, in order. An earlier directory wins when two packages share a name.

`FIDGET_CHARACTERS`, when set, is searched first. It adds directories and does not replace the rest. Separate entries with `:` on macOS and Linux, and with `;` on Windows.

| Platform | User directory | System directories | Shipped |
|---|---|---|---|
| macOS | `~/Library/Application Support/fidget/characters` | none | bundled `characters/` |
| Linux | `$XDG_DATA_HOME/fidget/characters`, or `~/.local/share/fidget/characters` when that variable is unset | each `$XDG_DATA_DIRS` entry plus `/fidget/characters`. Unset or empty is `/usr/local/share/fidget/characters`, then `/usr/share/fidget/characters` | bundled `characters/` |
| Windows | `%APPDATA%\fidget\characters` | none | bundled `characters/` |

The user directory is the same per-platform data directory as `settings.json`, with `characters` under it. The Settings window's Character picker shows that path.

Eight characters ship: **Buddy Bot** (default), BMO, Nim, Black Mage, Cat, Jotaro Kujo, Timber Wolf, Trump.

### Writing a Character

A Character Package is a directory or `.zip` holding a `character.manifest`, an optional `personality.txt`, and the PNG frames its Character Manifest names. The Character Manifest is documented below, but it is not frozen: a key can still change before v2, and `character::load` in `crates/core/src/character.rs` is the authority when this page and the loader disagree.

#### Create a character

1. Copy a shipped package into a directory of your own, under the folder name you want to start it by, and point `FIDGET_CHARACTERS` at that directory. A `.zip` of the same files works too:

   ```sh
   mkdir -p ~/fidget-characters
   cp -R characters/buddy-bot ~/fidget-characters/blip
   export FIDGET_CHARACTERS=~/fidget-characters
   ```

   `FIDGET_CHARACTERS` is searched first and the other directories stay, so this copy wins when names collide and the shipped characters remain available. Drop a package in the user directory from the table above and leave the variable unset to have it read from there.

2. Replace the PNGs in `frames/` with your own 8-bit RGBA art, facing right. Keep one size per Animation.
3. Edit `character.manifest`: set `name`, point each Animation's `frames` at your files, and delete `[source]` or rewrite it for your art.
4. Rewrite `personality.txt`, as [Writing a personality](#writing-a-personality) describes.
5. Start it by folder name:

   ```sh
   cd src-tauri && FIDGET_CHARACTER=blip cargo run
   ```

   A package the loader rejects prints `character: <path> is not a valid Character Package:` on stderr, followed by every mistake at once.

#### The Character Manifest

`character.manifest` is TOML. The loader rejects any key it does not know, so a typo is an error, not a silent default. Top-level keys come before the first table, as TOML requires. In the tables below, "Required" means the loader rejects the package without it.

| Key | Required | Value | Default |
|---|---|---|---|
| `name` | Yes | The Character's name, as the UI shows it. Not empty. | |
| `render_mode` | No | `"pixelated"` keeps hard pixels when scaling; `"smooth"` filters drawn art. | `"pixelated"` |
| `scale` | No | Whole number from 1 to 4: the factor the art is drawn at. | 4 |
| `[source]` | No | Where the art came from. See [Declaring where the art came from](#declaring-where-the-art-came-from). | |
| `[director]` | No | How proactive model calls space themselves. | |
| `[cursor]` | No | How the Character reacts to the cursor. | |
| `[animations.<name>]` | Nine required | One table per Animation. | |
| `[behaviors.<name>]` | No | One table per Behavior. | |

A Character Manifest is at most 1 MiB. A package is at most 64 MiB, 4096 files, and 8 directories deep.

#### Animations

Every Character supplies these nine: `idle`, `walk`, `fall`, `land`, `sit`, `sleep`, `react`, `talk`, and `hold`. The Engine also asks for three optional ones, and draws a stand-in when a package omits them:

| Animation | Plays when | Without it |
|---|---|---|
| `grab` | The user drags the sprite | `fall` |
| `climb` | The sprite climbs | `walk` |
| `jump` | A Behavior plays the `jump` Primitive | `fall` |

Any other name draws only as a Variant or a Left Strip of one of these.

| Key | Required | Value | Default |
|---|---|---|---|
| `frames` | Yes | Frame file paths relative to the package root, in play order. 1 to 256 entries. | |
| `fps` | No | Whole number from 1 to 60. | 8 |
| `loop` | No | `"forever"` repeats; `"once"` holds the last frame. | `"forever"` |
| `variant_of` | No | Another Animation's name. Starting that base draws one member of its ring by weight: the base or any of its Variants. | |
| `weight` | No | Whole number: this Animation's share of its Variant ring. Read only in a ring. | 10 |
| `left_of` | No | Another Animation's name. This strip draws in place of the base when the sprite travels left. | |

The loader checks each frame against the art:

- Every frame is an 8-bit RGBA PNG in the package, at most 1024 pixels on either side, and every frame of one Animation is the same size.
- All of a package's distinct frames add up to at most 256 megapixels (256 × 1024 × 1024 pixels). A frame two Animations share counts once.
- A `variant_of` base is declared, is not itself a Variant, and both it and the variant loop `"forever"`.
- A `left_of` base is declared and is not itself a Left Strip. The strip has as many frames as its base, at the same size, and only one strip faces each base.

Draw every Animation facing right. The renderer mirrors it for leftward travel unless another Animation declares `left_of` for it. Pixels with alpha below 128 do not catch clicks.

#### Behaviors

A Behavior is a named sequence of Primitives that the Static Director picks by weight, or that a model proposes. A Character composes Behaviors from Primitives and cannot define new ones.

| Key | Required | Value | Default |
|---|---|---|---|
| `play` | No | A list of Primitives, played in order. A Behavior with none plays nothing. | `[]` |
| `then` | No | The name of the Behavior that follows this one. | |
| `weight` | No | Whole number: how likely the Static Director is to pick this Behavior against its siblings. 0 leaves it reachable only through `then` or a model proposal. | 10 |
| `when` | No | The condition that must hold before the Behavior is picked. | Any time |

The Primitives, and the Animation each plays:

| Primitive | Animation |
|---|---|
| `idle`, `walk`, `land`, `sit`, `sleep`, `react`, `talk`, `hold` | The Animation of the same name |
| `chase` | `walk`, steered toward the cursor's x along the ground |
| `jump` | `jump`, or `fall` without it |

`when` takes one of three forms. A duration is a whole number followed by `s` or `m`.

| Condition | Holds while |
|---|---|
| `"idle over 2m"` | The user has been away for longer than the duration |
| `"idle under 30s"` | The user has been away for less than the duration |
| `"app Google Chrome"` | That application is frontmost, by the name the platform reports |

Every `then` names a declared Behavior, and every chain ends. A chain that returns to a Behavior it already ran is an error.

#### Director and cursor tuning

| Key | Value | Default |
|---|---|---|
| `director.model_base` | Whole number from 1. | 2 |
| `director.model_power` | Whole number from 0. | 1 |
| `cursor.near_reaction` | What the Character does when the cursor comes near. | `"indifferent"` |
| `cursor.rush_reaction` | What the Character does when the cursor rushes at it. | `"indifferent"` |

After each proactive model call, the wait before the next one is multiplied by `model_base` raised to `model_power`, up to two hours. Addressing the Character resets the wait. The defaults double it; a `model_base` of 1 keeps it constant.

A cursor reaction is one of `"indifferent"` (carry on), `"speak"` (play `talk`), `"face"` (turn toward the cursor), `"toward"` (walk toward it), `"away"` (walk away from it), or `"react"` (play `react`).

#### A worked example

`characters/buddy-bot/character.manifest` ships with the default Character. These tables are quoted from it, with comments and most frames left out:

```toml
name = "Buddy Bot"
render_mode = "smooth"
scale = 1

[director]
model_base = 1
model_power = 1

[cursor]
near_reaction = "speak"
rush_reaction = "react"

[animations.idle]
frames = [
  "frames/idle-0.png",
  # ... frames/idle-1.png to frames/idle-15.png
]
fps = 5
weight = 20

[animations.idle-blink]
frames = [
  "frames/idle-blink-0.png",
  # ... frames/idle-blink-1.png to frames/idle-blink-5.png
]
fps = 8
variant_of = "idle"

[animations.land]
frames = [
  "frames/land-0.png",
  "frames/land-1.png",
  "frames/land-2.png",
  "frames/land-3.png",
]
fps = 8
loop = "once"

[behaviors.greet]
play = ["talk"]
then = "stroll"
weight = 40
when = "idle under 10s"

[behaviors.stroll]
play = ["walk"]
weight = 30
when = "idle over 30s"

[behaviors.settle]
play = ["sit"]
then = "nap"
weight = 30
when = "idle over 1m"

[behaviors.nap]
play = ["sit", "sleep"]
weight = 40
when = "idle over 2m"
```

Buddy Bot is drawn art at its own size, so it renders `smooth` at scale 1. Its proactive calls keep their first wait, two minutes by default, instead of backing off. `idle` weighs 20 against 10 for each of its three variants (`idle-blink` is one), so a bare idle draws on two turns in five. `land` plays once and holds its last frame into idle. `greet` hands over to `stroll`, and `settle` hands over to `nap`, which ends the chain. Buddy Bot declares no `grab`, so a drag plays its `fall`.

#### Declaring where the art came from

`[source]` says what the art is, where it came from, and what license covers it. The loader does not need it, and a local package can leave it out.

```toml
[source]
art     = "What the Character is, and where its frames came from."
url     = "https://example.com/the-pack"   # optional, http or https only
license = "The license the art carries, or that none is declared."
```

When `[source]` is present, the loader rejects it without both `art` and `license`. "None is declared" is a valid `license`; a missing key is not, because it reads as an unfinished declaration.

A Character shipped from `characters/` in this repository is expected to declare `[source]`, and `cargo test -p fidget-core --test character_packages` checks it. The [Character Gallery](https://omesser.github.io/fidget/characters.html) publishes it as the attribution panel. `art` names the Character and the pack or process its frames came from. `url` links that pack. `license` names the license the art carries, or says that none is declared.

#### Writing a personality

`personality.txt` is plain prose the loader never interprets, up to 2000 characters. Temperament alone is not enough: a model given only that converges on the same few assistant-flavored lines. Include three things, unlabeled (#156):

1. **Who the character is and how it carries itself.** Skip what the sprite already shows; spend the words on how it speaks and what it notices.
2. **Fixations:** three to five strong, specific opinions - things it loves, resents, takes personally, or takes credit for.
3. **Sample lines**, verbatim, introduced in prose ("It has been heard to say: …"). They carry the character's recurring bits and catchphrases. Be generous; `characters/black-mage/` shows how far that goes.

#### Universal rules

Leave these out of a personality file. `character_prompt` in `crates/core/src/director.rs` injects them for every Character:

- Stay in character, and never mention being a model or an assistant.
- Fit the bubble - five short sentences at the most.
- Vary, preferring an unused line, while a signature phrase may recur.
- Lean away from the Behaviors that just played.
- React to the moment - what just happened, and what the sprite stands on - when there is something worth remarking on.
- Dialogue is demeanour, never capability: no promising actions on the machine, no claiming abilities.

### Running Multiple Instances

```sh
cd src-tauri && FIDGET_INSTANCES="buddy-bot:One,buddy-bot:Two,nim:Nim" cargo run
```

## Importing Pets

Translate [Pets Codex](https://petscodex.com/), [petdex](https://petdex.dev/), or [Shimeji Shop](https://shimejishop.com/) packs to Character Packages. `scripts/import-pet.py` needs Python 3.11 or newer and Pillow. Set it up once, from the repository root:

```sh
uv venv --python 3.11 && uv pip install pillow
```

The importer writes the frames and a `character.manifest` to the `-o` directory, and `--force` replaces one that exists. It then runs `character::load` on the output and fails if the loader rejects it. It prints the pack's license and warns when none is declared.

The result is a naive but valid Character Package. It loads and plays, but it is not yet the character. Tune it by hand:

- The `[behaviors]` weights and triggers, which start as a copy of BMO's.
- The animation and action names, so each reads as what the art shows.
- `personality.txt`, the character prompt. The importer writes none; see [Writing a personality](#writing-a-personality).

A coding agent can make these edits in a few prompts. See [Character Packages](#character-packages) for the manifest.

### Pets Codex

A petdex pack shares the Pets Codex sheet layout and imports the same way, with `--format petscodex`.

```sh
npx petscodex install labubu
uv run scripts/import-pet.py ~/.codex/pets/labubu --format petscodex -o characters/labubu
```

`[source]` is filled from the pet's `pet.json`, with `url` pointing at its page. Also check every animation reads as its name, and that walk heads right. A pet whose art strays from petdex's row semantics is recut with `--walk-row`, `--mirror-walk`, or `--map`.

### Shimeji Shop

Download the pack's `.zip` from its gallery page. The importer wants per-pose PNGs named `shime1.png`, `shime2.png`, and so on, together in one folder at the pack's root or under it. Without an `actions.xml` it assumes Shimeji-ee's standard conf, which reads `shime1`-`shime14` and `shime18`-`shime21`. Pass the zip as is:

```sh
uv run scripts/import-pet.py ~/Downloads/my-pet.zip --format shimeji -o characters/my-pet
```

Every frame is mirrored to head right, and the manifest's leading comment records which Shimeji action fed each animation. Also edit:

- `name` is the zip's file name. Set the display name.
- `[source]` has no `url` and a generic `art` line. Add the gallery page's URL and say what the art is.
- `talk` falls back to the stand pose when the pack has no Wave, Greet, or Hello action. A bare pack has none of them.

## Linux Dependencies

```sh
# Debian/Ubuntu (build from source)
sudo apt install libayatana-appindicator3-dev
```

### Install packages (tray, cue audio, AppImage)

**Tray.** The tray icon is how you reach Settings, Character, Memory, and Quit, so the `.deb` depends on `libayatana-appindicator3-1` (the older `libappindicator3-1` is not accepted). The AppImage carries its own copy. Showing the icon also needs a StatusNotifier host, which the desktop provides and no package can declare: GNOME Shell, KDE Plasma, and XFCE's Status Tray plugin are hosts; Plank and a Wayland compositor with no tray protocol are not. Without a host, the sprite's right-click menu is the same menu.

**Cue audio** is Web Audio in WebKitGTK, played through GStreamer. `libwebkit2gtk-4.1-0` already depends on `gstreamer1.0-plugins-base` and `gstreamer1.0-plugins-good` (which ships `pulsesink`), enough under PipeWire-pulse or PulseAudio. An ALSA-only machine also needs `gstreamer1.0-alsa`. With no sound device the fidget stays silent and still draws the visual cue.

The AppImage bundles `libgstreamer` but not the plugin pack (`bundleMediaFramework` stays off; it would add tens of megabytes). Cue audio then needs the host's `gstreamer1.0-plugins-good` (plus `gstreamer1.0-alsa` without Pulse/PipeWire) and a running sink. If those are installed and the AppImage is still mute, GStreamer is looking for plugins inside the image.

**AppImage on Ubuntu** needs `libfuse2` (22.04) or `libfuse2t64` (24.04+).

### Linux X11/Wayland

One build, lane chosen at runtime. When an X server answers (real X11 or XWayland under GNOME/KDE), full spatial support: window Perches, edges, Grab, Throw, and fullscreen fade. Rare pure Wayland sessions with no X server degrade to screen-edge physics only — no window Perches.

### Windows

The NSIS installer ships. The README platform table lists the degraded cells.

#### Harness Process Termination

The ACP Harness child and its descendants (e.g. `npx` spawning Node) go in a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The child is spawned suspended, assigned to the job, then resumed, so no grandchild outlives a quit or detach. It also gets its own process group (`CREATE_NEW_PROCESS_GROUP`), so Ctrl+C into `cargo run` does not reach it.

## Homebrew

Apple Silicon macOS, without a Rust toolchain:

```sh
brew install --cask omesser/fidget/fidget
```

That command taps `omesser/homebrew-fidget`. Homebrew only reads a top-level `Casks/` directory, so the tap holds a copy of [`packaging/homebrew/Casks/fidget.rb`](../packaging/homebrew/Casks/fidget.rb) at `Casks/fidget.rb`. The copy in the tap is what `brew update` tracks.

Publishing a GitHub Release builds the packages, then bumps this cask and pushes `Casks/fidget.rb` to the tap. The Release workflow checks out the default branch, runs `scripts/bump-homebrew-cask.sh` with the Release tag, runs `scripts/verify-homebrew-cask.sh`, and copies the cask into `omesser/homebrew-fidget`. A prerelease does not move the cask. `livecheck` uses `:github_latest`, which follows the marked Latest release and skips drafts and prereleases.

The default branch requires a pull request. The job pushes the cask commit when it can, and otherwise opens or updates the `homebrew-cask-bump` pull request. The tap push does not wait for that pull request.

A `workflow_dispatch` package build leaves the tap alone. To reconcile a tag that is already published, dispatch the Release workflow and set the sync tap tag input to that tag, for example `v0.1.0`. That run skips the package build. The tag has to be the latest stable Release, because verify refuses any other pin. Re-running the homebrew job on a Release run reconciles the same way.

`scripts/bump-homebrew-cask.sh` and a hand copy into the tap are for when that job cannot run. The bump reads that tag's Apple Silicon `.dmg`. The cask depends on arm64 because that is the only macOS disk image the Release ships.

## Further Reading

- [README](../README.md): what it does, how to run, platform support
- [CONTEXT.md](../CONTEXT.md): vocabulary
- [DESIGN.md](../DESIGN.md): design decisions
- [docs/SPEC.md](./SPEC.md): v1 scope and requirements
- [docs/harness.md](./harness.md): Completers, Director environment, local model servers, and the MCP server
- [docs/adr/](./adr/): Architecture Decision Records
