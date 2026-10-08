<div align="center">

<!-- Shields split on a single hyphen, so cursor-agent is cursor--agent in the URL. Each color is that harness's own hue, darkened until the white shield text stays readable. -->

[![CI](https://github.com/omesser/fidget/actions/workflows/tests.yml/badge.svg)](https://github.com/omesser/fidget/actions/workflows/tests.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)

[![harness](https://img.shields.io/badge/harness-claude-C25B3A)](#harness-support) [![harness](https://img.shields.io/badge/harness-codex-0E8A6A)](#harness-support) [![harness](https://img.shields.io/badge/harness-cursor--agent-D04200)](#harness-support) [![harness](https://img.shields.io/badge/harness-hermes-5C5AD6)](#harness-support) [![harness](https://img.shields.io/badge/harness-opencode-005BBB)](#harness-support) [![harness](https://img.shields.io/badge/harness-pi-0C7EA8)](#harness-support) [![harness](https://img.shields.io/badge/harness-grok-2B2B2B)](#harness-support) [![harness](https://img.shields.io/badge/harness-copilot-57606A)](#harness-support) [![harness](https://img.shields.io/badge/harness-goose-B83800)](#harness-support) [![harness](https://img.shields.io/badge/harness-antigravity-C5221F)](#harness-support)

<img src="./branding/banner.jpg" width="100%" alt="Fidget, a desktop pet" />

[omesser.github.io/fidget](https://omesser.github.io/fidget/)

</div>

# Fidget is a desktop pet that keeps you company, and pitches in when you ask

An AI character that lives on your screen. It walks, talks, naps, and can assist in whatever you are up to. Pick it up, throw it around, let it nap, or talk to it. Attach the AI harness you already use, and it can do anything that an AI agent can.

Each character has a personality and a life of its own.

An AI harness is optional. Attach the one you already use and it can talk and pitch in with tools; leave it off and the pet still works. It doesn't read your screen without consent and [never takes screenshots](#computer-use).

https://github.com/user-attachments/assets/31bf24c7-7fe7-43d0-92d7-a4a3bd6d4c11

Try the gestures in your browser: [Fidget Cues](https://omesser.github.io/fidget/cues.html)

## What It Does

- **Keeps you company**: It walks, naps, and reacts to your open windows. It works offline, with no AI account or subscription, for presence and play; hook it up to an AI and it takes on a life of its own, with full conversation and tool use.
- **Pick a built-in character, or build your own**: Each character has its own sprites, animation loops, and personality. Edit any character's prompt or behavior using a plain-text personality file and a simple [manifest format](./docs/DEVELOPMENT.md#character-packages). Create your own characters, or [import and convert](./docs/DEVELOPMENT.md#importing-pets) one from the [Pets Codex](https://petscodex.com/) and [Shimeji Shop](https://shimejishop.com/) galleries.
- **Personalize your Fidget's behavior**: Write your own text for any fidget's behavior through its Instance Prompt. It holds on top of its Character's personality, and changes how that fidget reacts. See [Give It an Instance Prompt](#give-it-an-instance-prompt) for examples.
- **Pitches in**: Double-click to chat with the agent you already use (Claude Code, Codex, Cursor, or any ACP [harness](#harness-support)); it acts on your machine and answers in speech and motion.
- **Knows what you're up to**: It can see and react to your open windows - titles only, and only with consent, for context-aware chatter. It never reads screen pixels and [never takes screenshots](#computer-use); Reading titles and application names needs your explicit consent.
- **Gets out of your way when you ask it to**: Automatically fades away when in fullscreen, hides at will on hotkey, and comes back when you want it to.

## Get It

On macOS (Apple Silicon) with [Homebrew](https://brew.sh):

```sh
brew install --cask omesser/fidget/fidget
```

That taps [omesser/homebrew-fidget](https://github.com/omesser/homebrew-fidget) and installs Fidget.app. Later Releases arrive with `brew update` and `brew upgrade --cask fidget`.

Or download a build from [GitHub Releases](https://github.com/omesser/fidget/releases): a `.dmg` for macOS (Apple Silicon), an AppImage and a `.deb` for Linux (x86_64), or an NSIS installer for Windows (x86_64). The builds are not signed yet, so the first open warns. On macOS, open Fidget.app from the disk image, dismiss the Gatekeeper dialog, then System Settings → Privacy & Security → Open Anyway. On Windows, choose More info → Run anyway in SmartScreen.

Or clone and run from the repo root (macOS, Linux, Windows):

```sh
git clone https://github.com/omesser/fidget.git fidget
cd fidget
cargo run -p fidget
```

It works offline with no key. With nothing configured, Static weights pick idle Behaviors from the Character.

Right-click the fidget, or click the tray icon, and choose Settings….

Settings → Character picks which character it wears. Buddy Bot is the default. The others are under [Characters](#characters).

Settings → AI chooses who answers. Under AI source, pick `Harness · claude` or another name from [Harness Support](#harness-support). The Harness signs in on its own. For a Model API, pick Model API in AI source, then set Base URL, Model, and API key under Model / API. Presets fill the Base URL for OpenAI, Anthropic, xAI, and Ollama. Type any other OpenAI-compatible endpoint into Base URL. Apply saves the choice.

Developers and CI can override Settings with optional environment variables:

```sh
# Optional. Overrides Settings → AI → AI source.
FIDGET_HARNESS=claude cargo run -p fidget

# Optional. Overrides Settings → Character.
# Any of: buddy-bot (default), black-mage, bmo, cat, jotaro-kujo, nim, timber-wolf, trump
FIDGET_CHARACTER=nim cargo run -p fidget
```

The full variable list, including `FIDGET_DIRECTOR_*`, is in [harness.md](./docs/harness.md#director-environment). Character packages are in [DEVELOPMENT.md](./docs/DEVELOPMENT.md#character-packages). Keychain dialogs an unsigned build costs are in [harness.md](./docs/harness.md#settings-and-keyring). Full support for window Perches and edges via X11/XWayland (the normal Linux desktop path). Pure Wayland sessions without an X server fall back to screen edges only. Linux packages and the AppImage's FUSE dependency are in [DEVELOPMENT.md](./docs/DEVELOPMENT.md#linux-dependencies).

## Interact

![Buddy Bot react](./docs/readme/buddy-bot-react.gif)

- **Poke** - click once for a react, then it resumes.
- **Summon** - double-click to open a chat window for that fidget.
- **Pick up** - click and drag; it follows the cursor.
- **Throw** - release while moving; it flies on an arc and lands.
- **Perch** - let it settle on a window's top edge; drag slowly to ride, fling to drop.
- **Hide** - Control-Option-Command-B (the default; change it in Settings) toggles the fidget instantly.
- **Fullscreen** - fades out for fullscreen apps, fades back when you exit.

### Talk to It

Summon opens that fidget's chat window. What you type joins the same
conversation that decides what it does on your desktop, so an answer arrives as
speech and a Behavior, not only as text. Lines it says unprompted appear here
too, labelled with what it was reacting to.

<img src="./docs/readme/chat-surface.png" width="420" alt="The chat surface: a line labelled WHEN SUMMONED, a typed question, and BMO's answer, over a status bar naming the Behavior, State and next wake" />

The bar at the bottom says what the fidget is doing and when it next thinks.
Advanced opens the ladder: Behavior, Primitive, Animation, State, Facing, and
the Director's countdown. Answers need a Director; see [Get It](#get-it).

### Give It an Instance Prompt

An Instance Prompt is your own text for one fidget, layered on top of its Character's personality. To write one, double-click the fidget to summon it, open the **Prompt** tab, and type in the **Your own layer** box. That box is the Instance Prompt. Paste one of the examples below and edit it to fit. Saving starts a new conversation, and the Instance Prompt stays with that fidget if you switch its Character.

Every example below assumes this setup:

- A [Harness](#harness-support) is attached. A Model API can't run tools.
- The Harness is allowed to use its own tools.
- The **Window and application names** consent is on in Settings → Privacy. Fidget then reads app names and window titles only, never pixels, and it [never takes screenshots](#computer-use).

| Paste and edit | How it reacts on a wake |
|---|---|
| `If my editor stays in front for a long stretch, ask me to explain the bug to you, like a rubber duck.` | Sees the editor in front on a wake and asks you to walk it through the bug. |
| `You're a theatre critic. When you're perched on a window, review that app in one dramatic line.` | Perch it on a window and it reviews the app it's standing on. |
| `When I ask you to check something on my machine, do it, then report back in one line, in character.` | Does the check with your Harness's tools and reports back in one line, in character. |

## Characters

Buddy Bot is the default. Eight Characters ship; each moves and speaks differently. A name links to its full prompt.

<table>
<tr>
<td align="center" width="25%"><img src="./docs/readme/buddy-bot-walk.gif" height="96" alt="Buddy Bot" /><br><b><a href="./characters/buddy-bot/personality.txt">Buddy Bot</a></b><br><sub>Friendly and curious; he's your helpful assistant.</sub></td>
<td align="center" width="25%"><img src="./docs/readme/black-mage-talk.gif" height="96" alt="Black Mage" /><br><b><a href="./characters/black-mage/personality.txt">Black Mage</a></b><br><sub>Cynical spellcaster, cryptic and theatrical.</sub></td>
<td align="center" width="25%"><img src="./docs/readme/bmo-sing.gif" height="96" alt="BMO" /><br><b><a href="./characters/bmo/personality.txt">BMO</a></b><br><sub>Earnest, childlike, delighted to be here.</sub></td>
<td align="center" width="25%"><img src="./docs/readme/cat-walk.gif" height="96" alt="Cat" /><br><b><a href="./characters/cat/personality.txt">Cat</a></b><br><sub>Every window is furniture. Never helpful.</sub></td>
</tr>
<tr>
<td align="center" width="25%"><img src="./docs/readme/jotaro-kujo-react.gif" height="96" alt="Jotaro Kujo" /><br><b><a href="./characters/jotaro-kujo/personality.txt">Jotaro Kujo</a></b><br><sub>Terse, perpetually bored, tougher than he lets on.</sub></td>
<td align="center" width="25%"><img src="./docs/readme/nim-sleep.gif" height="96" alt="Nim" /><br><b><a href="./characters/nim/personality.txt">Nim</a></b><br><sub>Sleeps eleven hours a day. Soft-spoken.</sub></td>
<td align="center" width="25%"><img src="./docs/readme/timber-wolf-walk.gif" height="96" alt="Timber Wolf" /><br><b><a href="./characters/timber-wolf/personality.txt">Timber Wolf</a></b><br><sub>Patrol mech. Clan warriors don't waste words.</sub></td>
<td align="center" width="25%"><img src="./docs/readme/trump-wave.gif" height="96" alt="Trump" /><br><b><a href="./characters/trump/personality.txt">Trump</a></b><br><sub>The desktop is his rally. Bombastic.</sub></td>
</tr>
</table>

Characters are packages of art, personality, and tuning. See [DEVELOPMENT.md](./docs/DEVELOPMENT.md#character-packages); the format is still evolving.

## Harness Support

Which Harness you attach changes what Fidget can do with it.

<table>
<thead>
<tr>
<th align="left" nowrap width="170">Harness</th>
<th align="left">Command</th>
<th align="left">Standing</th>
</tr>
</thead>
<tbody>
<tr>
<td nowrap width="170"><img src="https://cdn.simpleicons.org/claude" width="14" alt="" />&nbsp;<code>claude</code></td>
<td><code>npx -y @agentclientprotocol/claude-agent-acp@latest</code></td>
<td>Zed's adapter over the Claude Agent SDK; no first-party ACP mode. Fresh and resumed sessions both work.</td>
</tr>
<tr>
<td nowrap width="170"><picture><source media="(prefers-color-scheme: dark)" srcset="./docs/readme/openai-on-dark.svg" /><img src="./docs/readme/openai-on-light.svg" width="14" alt="" /></picture>&nbsp;<code>codex</code></td>
<td><code>npx -y @agentclientprotocol/codex-acp@latest</code></td>
<td>Zed's adapter (<code>codex-acp</code>); no first-party ACP mode. Fresh and resumed sessions both work.</td>
</tr>
<tr>
<td nowrap width="170"><img src="https://cdn.simpleicons.org/githubcopilot" width="14" alt="" />&nbsp;<code>copilot</code></td>
<td><code>copilot --acp</code></td>
<td>First-party, GitHub. <code>copilot</code> alone is the interactive TUI. Fresh and resumed sessions both work, smoked on copilot 1.0.88 (<a href="https://github.com/omesser/fidget/issues/1016">#1016</a>).</td>
</tr>
<tr>
<td nowrap width="170"><img src="https://cdn.simpleicons.org/cursor" width="14" alt="" />&nbsp;<code>cursor-agent</code></td>
<td><code>cursor-agent acp</code></td>
<td>First-party. <code>cursor-agent</code> alone is the interactive TUI. Every attach opens a fresh session, because it advertises no <code>loadSession</code>.</td>
</tr>
<tr>
<td nowrap width="170"><picture><source media="(prefers-color-scheme: dark)" srcset="./docs/readme/grok-on-dark.svg" /><img src="./docs/readme/grok-on-light.svg" width="14" alt="" /></picture>&nbsp;<code>grok</code></td>
<td><code>grok agent stdio</code></td>
<td>First-party, Grok Build. <code>grok</code> alone is the interactive TUI. Fresh and resumed sessions both work.</td>
</tr>
<tr>
<td nowrap width="170"><picture><source media="(prefers-color-scheme: dark)" srcset="./docs/readme/goose-on-dark.svg" /><img src="./docs/readme/goose-on-light.svg" width="14" alt="" /></picture>&nbsp;<code>goose</code></td>
<td><code>goose acp</code></td>
<td>First-party, Block. <code>goose</code> alone is the interactive CLI. Fresh and resumed sessions both work, smoked on goose 1.51.0.</td>
</tr>
<tr>
<td nowrap width="170"><img src="https://cdn.simpleicons.org/opencode" width="14" alt="" />&nbsp;<code>opencode</code></td>
<td><code>opencode acp</code></td>
<td>First-party. Fresh and resumed sessions both work.</td>
</tr>
<tr>
<td nowrap width="170"><img src="./docs/readme/nous.svg" width="14" alt="" />&nbsp;<code>hermes</code></td>
<td><code>hermes acp</code></td>
<td>First-party. Fresh sessions work; a resume that cannot restore the session reopens (<a href="https://github.com/omesser/fidget/issues/448">#448</a>).</td>
</tr>
<tr>
<td nowrap width="170"><img src="https://cdn.simpleicons.org/pi" width="14" alt="" />&nbsp;<code>pi</code></td>
<td><code>npx -y pi-acp@latest</code></td>
<td>Zed-registry adapter (<code>pi-acp</code>); no first-party ACP. Fresh and resumed sessions both work.</td>
</tr>
<tr>
<td nowrap width="170"><img src="https://cdn.simpleicons.org/google" width="14" alt="" />&nbsp;<code>antigravity</code></td>
<td><code>agy_acp_server.par</code> (<code>agy_acp_server.exe</code> on Windows)</td>
<td>First-party, Google's ACP server; <code>agy</code> itself has no ACP mode. Fresh and resumed sessions both work, smoked on agy_acp_server 1.2.1 (<a href="https://github.com/omesser/fidget/issues/604">#604</a>).</td>
</tr>
<tr>
<td nowrap width="170">anything else</td>
<td>as typed, split on whitespace</td>
<td>Unnamed, and it works: any command that speaks ACP on stdio attaches.</td>
</tr>
</tbody>
</table>

Tools each harness keeps under ACP, session and auth behavior, and per-harness setup notes are in [docs/harness.md](./docs/harness.md#harness-support).

### Harness ↔ MCP

Fidget attaches to a Harness over ACP on stdio. The Harness calls back over MCP, on the route the [MCP transport column](./docs/harness.md#harness-support) describes. [harness.md](./docs/harness.md#mcp-server) has the transport details.

**Eight tools** from `crates/core/src/dispatch.rs`. The opening turn of the Character Prompt tells the model to use the tools it has, without naming them, so this table stays the only catalog (#917):

| Tool | Category | What it does |
|---|---|---|
| `speak` | Expression | Make the Character speak dialogue |
| `play_behavior` | Expression | Play a named Behavior |
| `list_windows` | Sensing | List visible windows with bounds, and their names under consent |
| `describe_screen` | Sensing | Describe screen (v1: window metadata only) |
| `recall` | Memory | Read everything Memory holds |
| `remember` | Memory | Write one fact under a heading |
| `list_instances` | Identity | List Character Instances and their names |
| `whereabouts` | Placement | Which display each Instance is on, the other displays, and its feet in that display. Each display includes its name when the platform has one, its origin, and its size, in logical points |

**Three readonly resources** (`resources/list`, `resources/read`; no write, no subscribe):

| URI | What it is |
|---|---|
| `fidget://windows` | Visible windows, frontmost first, with the owning application and the title. Empty without the window-names consent, which covers both (ADR-0032). Same excluded applications as `list_windows`. |
| `fidget://memory` | The Memory Manifest file every Character Instance shares. |
| `fidget://action-log` | The current Action Log file only. Rotated siblings are not this resource. A large current file is tailed to complete JSONL lines. |

**Explicitly not served:** mouse/keyboard/Executor tools (ADR-0003). No click, no type, no input events by design.

### Computer Use

Fidget never reads screen pixels. Sensing is OS window metadata: bounds and idle for free, plus the owning application, the title and the frontmost app under one consent ([ADR-0032](./docs/adr/0032-one-consent-for-titles-and-application-names.md)). So `describe_screen` describes the window layout, not what is on screen. Decline it and the fidget still knows where the windows are, and not what they are. The fidget takes no screenshots, runs no OCR, and embeds no vision model for desktop content. The "Appear in screenshots and screen shares" setting is the other direction: whether the *sprite* shows up in captures you take.

That bounds Fidget's own code, not the agent you attach to it. An agent that needs to see and act on your desktop still can. The capability comes from the Harness itself, or from a computer-use MCP server you attach to the Harness, never through Fidget, whose MCP serves no input events.

The portable option across the Harnesses above is [cua-driver](https://github.com/trycua/cua) (MIT; macOS, Windows, Linux), attached over stdio MCP. [Connect your agent to Cua Driver](https://cua.ai/docs/how-to-guides/driver/connect-your-agent) carries the per-client registration, and [MCP tools](https://cua.ai/docs/reference/cua-driver/mcp-tools) lists what it exposes. Attach it deliberately: it drives the real desktop with your signed-in sessions, and its permission mode is chosen by the process that owns the driver runtime, not by the agent asking. Some Harnesses bring computer use of their own instead; the [Capture decision note](./docs/research/capture-drop-and-harness-cu-path.md) has the per-Harness table and the other drivers surveyed.

## Platform Support

What works today on each OS.

| Capability | macOS | Linux | Windows |
|---|---|---|---|
| Overlay that never takes focus | yes | yes | yes |
| Click-through off the sprite | yes | yes | yes |
| Grab, Throw and Poke | yes | yes | yes |
| Perch on window edges | yes | yes | yes |
| Dock or panel as a Perch | yes | degraded¹ | degraded² |
| Fade out for a fullscreen app | yes | degraded⁵ | degraded³ |
| Capturable; opt-out in settings | yes | degraded⁴ | yes |
| Settings window | yes | yes | yes |

- `yes` - implemented.
- `degraded` - runs in reduced form. A supported mode, not an error.

**Degraded cell notes:**

1. **Linux Dock/panel:** Bottom panels work as Perches; side and top panels stay as reserved strips. Fixable; tracked in [#1300](https://github.com/omesser/fidget/issues/1300).
2. **Windows Dock/panel:** Taskbar from work area (full-width strip) rather than exact island bounds. Taskbar spans the edge by design; no Windows API equivalent to macOS's `CoreDockGetRect`.
3. **Windows fullscreen:** Fades for true fullscreen and properly-sized borderless windowed modes. Apps using non-standard fullscreen or leaving gaps may not trigger fade.
4. **Linux Capturable:** Always capturable. Linux has no platform API to exclude windows from capture tools ([ADR-0024](./docs/adr/0024-capturable-by-default.md)).
5. **Linux fullscreen:** X11 and XWayland work for all apps. Pure Wayland without XWayland: native Wayland fullscreen apps do not trigger fade or move; XWayland clients still trigger correctly ([#1360](https://github.com/omesser/fidget/issues/1360)).

Detailed investigation: [`docs/research/platform-support-degraded-cells.md`](./docs/research/platform-support-degraded-cells.md).

Linux support is full on normal desktops (X11 or XWayland under GNOME/KDE). Rare pure Wayland sessions without an X server fall back to screen edges only - no window Perches, Grab, Throw, or fullscreen fade. See [DEVELOPMENT.md](./docs/DEVELOPMENT.md).

## Developing

**Want to help?** [Open issues](https://github.com/omesser/fidget/issues) welcome bugs, ideas, and PRs. Start with [DEVELOPMENT.md](./docs/DEVELOPMENT.md) for toolchains, hooks, verification, character writing, and imports. See how Fidget compares to other desktop pets in [alternatives.md](./docs/research/alternatives.md).

**Design and decisions:**

- [CONTEXT.md](./CONTEXT.md) - vocabulary
- [DESIGN.md](./DESIGN.md) - design decisions (the chat window ships; the [chat mockups](https://omesser.github.io/fidget/chat-mockups.html) are a Dated page, a frozen proposal, not what ships)
- [docs/SPEC.md](./docs/SPEC.md) - v1 scope
- [docs/adr/](./docs/adr/) - ADRs

## Prior Art and Attribution

Harness brand marks identify each Harness and belong to their owners. The Grok
logomark is xAI's own file from [their brand guidelines](https://x.ai/legal/brand-guidelines),
used unaltered to refer to Grok, which those guidelines permit and may revoke.
The Nous Research mark (`docs/readme/nous.svg`) identifies the Hermes Harness.
The Codex row uses the OpenAI mark from
[Simple Icons](https://simpleicons.org), kept in the repo because the CDN no
longer serves that slug (CC0, trademark reserved to OpenAI).
`docs/readme/openai-on-light.svg` is that glyph in near-black;
`docs/readme/openai-on-dark.svg` is the same path in white, so it stays visible
on a dark README. The Goose mark is the Goose icon from
[Lobe Icons](https://github.com/lobehub/lobe-icons) (MIT), split the same way
(`docs/readme/goose-on-light.svg`, `docs/readme/goose-on-dark.svg`); the
trademark stays with Block. Every other mark is served from Simple Icons (CC0, with each
brand's trademark reserved to its owner).

[WindowPet](https://github.com/SeakMengs/WindowPet) (MIT) inspired the Tauri desktop-pet shape. Fidget is a greenfield build, not a fork ([ADR-0001](./docs/adr/0001-greenfield-tauri-not-fork-windowpet.md)). Overlay code is independent; tray, launch-at-login, and updater follow WindowPet's MIT-licensed patterns.


The Chat window's mind mark - the small brain beside what answers - is the
`brain` glyph from [Font Awesome Free](https://fontawesome.com/) 6.x, used
under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) and inlined as
a path in `src/chat.html`. The license asks for the credit; this is it.

Character provenance is in each Character Package manifest, under `[source]`, and on the [Character Gallery](https://omesser.github.io/fidget/characters.html). In short: Buddy Bot and Nim are this project's own art. Timber Wolf derives, with the creator's permission, from [MekaRamen](https://mekaramen.com/)'s [Sketchfab model](https://sketchfab.com/3d-models/clans-timberwolf-battlemech-74e4d72e0cf3409ba3992cd0d895bc2f). BMO is cut from the [shimejishop BMO pack](https://shimejishop.com/free/bmo-shimeji/). Cat, Jotaro Kujo and Trump are cut from [petscodex](https://petscodex.com/) pets. Black Mage is sliced from GigaGuy's sprite sheet on The Spriters Resource. A package is prose, a manifest and art: the personality and the manifest - animations, Behaviors, Director and cursor tuning - are this project's work and MIT throughout. The art is not always ours. Some characters adapt art that declares no license, and each manifest names what it adapts and whose IP the character is.

## License

MIT.
