# Fidget — design

A desktop companion in the spirit of Windows 95-era desktop mascots, with a
model behind it. An animated sprite lives on your screen, obeys physics, perches
on your windows, and has a personality. When you summon it, it reaches an agent
Harness you supply and does real work on your machine.

This document records what was decided, why, and what was rejected. Vocabulary
is defined in [CONTEXT.md](./CONTEXT.md) and used precisely here.

## State

Early. Work is tracked as [GitHub issues](https://github.com/omesser/fidget/issues).

The overlay is up and the frame loop runs the Engine, so the sprite falls, lands on the top edge of whatever window is under it, rides that edge when the window is dragged slowly, and drops when the window is yanked or closed, and it stands on the Dock rather than behind it. It can be clicked, picked up, dragged and thrown. It knows when to get out of the way: it moves off a display a fullscreen application has taken and fades out when every display is taken, goes away at once on Control-Option-Command-B and comes back the same way, and appears in screen captures and shares by default, with an opt-out in Presence settings for users who need meeting privacy. Startup stops if no Character Package loads. A Director proposes Behaviors: Static weights with nothing configured, an HTTP Completer if you set a key, or a Harness if you attach one (see [Get It](./README.md#get-it)). Both HTTP and Harness fill the Director role and answer chat ([ADR-0008](./docs/adr/0008-one-harness-session.md)). Double-clicking is a Summon that opens the chat surface; the sprite reacts and the Director answers. Right-clicking the sprite and the tray / menu bar icon open the same menu: Chat, Character, Instances, Director, Do Not Disturb, Go away, Hide rules, Memory, Action Log, Settings, Quit.

The Engine drives all nine required Animations. `idle`, `fall`, `sit`, `sleep` and `walk` each answer a State, `fall` covering being dragged as well; `land` plays when a fall ends, `hold` when a Perch is ridden, and `react` answers a Poke. Eight of the nine are also Primitives a Character can compose into a Behavior — all but `fall`, which is what losing your footing looks like rather than something a Behavior can ask for. A Behavior plays its Primitives in order and the Behaviors it chains into, and is refused or abandoned when the State the sprite is in does not permit it. `talk` plays when a proposal names a Behavior that includes it.

## Shape of the product

Two layers, deliberately separate.

The **Spatial Layer** is always on, entirely local, and contains no model. It
owns physics, window geometry, Behaviors, and the interaction verbs. It works
offline, with no permissions granted, no API key, and no Harness attached. This
is the layer that has to be worth having on screen when everything else is off.

The **Functional Layer** is invoked, asynchronous, and does the real work. It is
reached by Summoning the character. It performs actions through an external Harness
the user attaches. Fidget never bundles one.

The **Director** sits between them. It proposes a Behavior. Static weights
fill that role when nothing is attached. An attached Harness fills it from
the same conversation as chat — one session, not a second model. It never
runs in the frame loop and never drives animation directly. See
[ADR-0008](./docs/adr/0008-one-harness-session.md).

```
┌─────────────────────────────────────────────────────────────┐
│  fidget (Tauri)                                           │
│                                                             │
│  ┌───────────────────────┐   ┌───────────────────────────┐  │
│  │ Spatial Layer (Rust)  │   │ Webview (sprite render)   │  │
│  │ • physics @ 60fps     │──▶│ • PNG frames, pixelated   │  │
│  │ • window poll @ 10Hz  │   │ • integer nearest-neighb. │  │
│  │ • Perch collision     │   │ • per-pixel hit test      │  │
│  │ • Behavior player     │   └───────────────────────────┘  │
│  └──────────┬────────────┘                                  │
│             │ occasional                                    │
│  ┌──────────▼────────────┐   ┌───────────────────────────┐  │
│  │ Director              │   │ Sensing                   │  │
│  │ • proposes Behaviors  │◀──│ • CGWindowList @10Hz      │  │
│  │ • never in frame loop │   │ • titles, app (consented) │  │
│  └───────────────────────┘   └───────────────────────────┘  │
│                                                             │
│  ┌───────────────────────┐                                  │
│  │ MCP server (ours)     │                                  │
│  │ speak / play_behavior │                                  │
│  │ list_windows          │                                  │
│  │ describe_screen       │                                  │
│  │ recall / remember     │                                  │
│  └──────────┬────────────┘                                  │
└─────────────┼───────────────────────────────────────────────┘
              │ MCP
     ┌────────▼─────────┐        ┌──────────────────────────┐
     │ Harness (BYO)    │───────▶│ Executor (theirs)        │
     │ Claude Code /    │        │ native computer use, or  │
     │ Codex / any      │        │ desktop-control MCP srv  │
     └──────────────────┘        └──────────────────────────┘
```

## Decisions

### 1. Nostalgia first, capability second

The character has to be delightful before it is useful. Idle life, animation,
and presence come first; developer and productivity abilities are a second
layer built on top.

The reasoning: agentic desktop control is becoming commodity infrastructure, and
the labs will ship it natively. A companion worth keeping on screen is the
durable part.

### 2. Spatial before functional

Physical presence — reacting to windows, obeying gravity, being grabbed and
thrown — ships before any ability to operate the machine. The spatial layer is
read-only with respect to the system and needs no permissions.

### 3. Cross-platform architecture, macOS first

macOS is the first implemented platform because it is the development machine.
Windows ships behind the same platform interface; Releases attach an NSIS
installer. Remaining Windows cells are `stub` or `degraded` in the README
platform table — overlay and sensing depth, not the package.

**Linux is not one platform.** X11 supports everything the spatial layer wants.
Under the native Wayland protocol each limit belongs to a different layer:

- **Other windows' geometry is permanent.** No compositor offers it.
  `ext-foreign-toplevel-list-v1` carries an identifier, a title and an app id
  and no rectangle. `wlr-foreign-toplevel-management`'s `set_rectangle` is a
  hint the client sends the compositor for its minimize animation, not a query.
  GNOME's private `org.gnome.Shell.Introspect.GetWindows` returns width and
  height off `get_frame_rect()` but no x or y, and its sender allowlist holds
  the two portal backends and nothing else.
- **Placement over the desktop is GNOME's.** `zwlr_layer_shell_v1` anchors a
  surface to screen edges with margins and keeps it above the desktop, which
  is absolute positioning and pinning in one protocol. KWin, Sway, Hyprland,
  niri, river, COSMIC and Mir implement it. Mutter does not.
- **Per-pixel click-through is ours.** `wl_surface.set_input_region` with a
  `wl_region` is core Wayland, and `wl_region.add` takes the same rectangles
  `x11/overlay.rs` hands `XShapeCombineMask`. tao hands us the `wl_surface`
  through `raw_window_handle` (`gdk_wayland_window_get_wl_surface` in tao's
  `linux/window.rs`). The 1x1 input shape it sets for `CursorIgnoreEvents`
  ([research](./docs/research/event-driven-input-vs-polling.md)) is itself a
  `set_input_region` call through GDK. `x11/overlay.rs` matches only Xlib and
  Xcb handles and drops the Wayland one. Wiring it is our work.

A *Wayland session* is a different question from that protocol. Mutter and KWin
both run XWayland, which proxies the X11 requests this app makes, so a GNOME or
KDE desktop takes the X11 lane: the lane is chosen on whether an X server
answers, not on `WAYLAND_DISPLAY`. Degradation is for a session where none does.
XWayland leaves one hole — it does not list native Wayland clients, so Perches
on those stay impossible.

The spatial layer is therefore an *optional capability the platform declares*,
not an assumption, and where the protocol withholds it the character degrades rather
than fails.

### 4. Tauri, greenfield

Rust core with a webview front end. The sprite is 2D animation a webview handles
trivially. The hard parts — window enumeration, global input, tray, platform
capability detection — are Rust-side code that has to be written regardless of
stack, and Tauri puts them where they belong.

Rejected:

- **Electron** — same webview model, but roughly 150MB of binary and 100–200MB
  resident for a program whose pitch is "always there, costs you nothing." That
  number becomes a permanent argument.
- **Native per platform** — best behavior and footprint, three codebases.
  Contradicts the cross-platform requirement.
- **Godot** — genuinely good at sprite animation and state machines, but an odd
  foundation for the Functional Layer, and the chat surface fights the engine.
- **Forking [WindowPet](https://github.com/SeakMengs/WindowPet)** (MIT,
  Tauri + React, Windows/macOS/Linux) — it already solves click-through,
  pixel-perfect drag, tray, autostart, and updates, and has no physics, no
  window awareness, and no model. Rejected because the novel work replaces its
  central loop, and gutting the centre of a codebase is slower than starting
  clean. Its click-through hit-testing and tray/updater code are lifted
  directly under MIT, with attribution.

**Known cost:** in both Tauri and Electron, mouse click-through is per-window,
not per-pixel. A small sprite in a large transparent window swallows clicks
across the whole rectangle unless the cursor is tracked and ignore-mouse-events
toggled by hit-testing the sprite's alpha. WindowPet's implementation is the
reference.

### 5. The Director proposes; it never animates

The model wakes occasionally and emits a short Behavior for the local engine
to play cheaply. It is never in the frame loop. Who the model is, and how
rare a wake is, is [ADR-0008](./docs/adr/0008-one-harness-session.md): an
attached Harness is the Director and chat; a session wake is reactive or
exponentially backed-off, and silent while the display is asleep.

Rejected:

- **Prompt-at-authoring only** (character prompt compiles to static weights, no
  runtime model) — kept as the fallback when no Harness is attached, when the
  Director is off, and when a session call fails.
- **Model in the loop** — paying tokens for a cartoon to decide to scratch
  itself. Unusable battery, cost, and latency.
- **A second inference API beside the Harness** — two minds. The HTTP
  Completer in #11 is a stand-in for that Harness session, not a product
  surface.

This is the decision that keeps the character visibly alive while the Functional
Layer is thinking, which is exactly where a naive design looks broken.

How a wake is actually sent — one cancellable slot per Instance, where starting
a call is the cancellation of that Instance's previous one — is
[ADR-0016](./docs/adr/0016-one-cancellable-slot-per-instance.md).
It makes two exceptions. Nothing replaces a call waiting on the user's answer,
and neither an ambient tick nor a Summon replaces a reply still generating.
The latency cost of the convention it replaced is in
[research](./docs/research/director-in-flight-and-latency.md).

### 6. Characters are packages; the engine owns the vocabulary

A Character Package contains animations, a Character Manifest, a Personality Prompt, and
Behavior declarations. The format is first-class from day one.
`docs/DEVELOPMENT.md` documents it for authors, but it is not frozen until v2.

The engine owns the **Primitives** — the State machine and the units of motion
and expression. No Character can invent one. A Character declares **Behaviors**
as data: named sequences of Primitives with weights and trigger conditions.
Declarative, validatable, not Turing-complete, and diffable.

Rejected:

- **Built-in character enum** — retrofitting a package boundary onto hardcoded
  characters is a rewrite.
- **Character-owned behavior graphs** — this is what
  [Shimeji-ee](https://kilkakon.com/shimeji/) does with per-character XML (New
  BSD, Java, Windows-first). After fifteen years the overwhelming majority of
  community packages are art reskins of the default XML, because the graph was
  too hard to author. The engine-primitives split keeps the distinctiveness
  without owning a scripting language, and is the only version where an
  AI-generated Character Package is safe to load.

A Personality Prompt governs demeanour, never capability. Character Packages are
untrusted input to a model that can reach an agent Harness; prompt injection
through a package is not theoretical.

**Required Animation Set: 9** — `idle`, `walk`, `fall`, `land`, `sit`, `sleep`,
`react`, `talk`, `hold` (ADR-0007 added the last, for riding a dragged Perch).
A declared optional set is used when present. A Character with 9 animations
must work; one with 30 should look better. Nine keeps a hobbyist package to an
evening's drawing.

**Shipped Characters span at least two styles: hard-pixel retro (a small flat
palette, no anti-aliasing, dithering only where a shade between two of its
colours is wanted) and modern pixel art.** Two styles validate the package
abstraction against real variance before the format is published.

The package on disk, as a directory or the same tree inside an archive:

```
mochi/
├── character.manifest    # name, per-Animation frames, fps, loop mode, Behaviors
├── personality.txt       # Personality Prompt — demeanour only, never capability
└── frames/
    ├── idle-0.png        # one PNG per frame, named by the manifest
    ├── idle-1.png
    ├── walk-0.png
    └── ...
```

Frame size and frame count are read from the art rather than declared: a
declared size can disagree with the art, and a derived one cannot. The manifest
is TOML — a table per Animation, a table per Behavior (ADR-0015) — and rejects
every declaration it does not know, which is what stops a package from
declaring itself a capability.

A `weight` is a relative share, and one seeded draw reads them for a Behavior
and for an Animation's variants alike. It is a plain integer in both places —
`weight = 80` — unbounded, defaulting to 10, and meaningful only against its
siblings'. A ring nobody weighs is therefore an even split.

The default is 10 rather than 1 so that an author can weigh a member *down*.
At a default of 1 the default is also the floor: making one variant rarer than
its siblings means raising every other member instead. At 10, `weight = 5` is
half as often and `weight = 1` a tenth, and nothing else in the ring moves.

Shares rather than percentages because the draw runs over a filtered pool.
`StaticDirector::propose` gates on trigger and on `weight > 0`, then on
recency, and draws from whatever survives; a declared percentage would almost
never be the share actually taken. A ratio survives filtering and a percentage
does not. Nothing here totals 100, nothing validates a sum, and nothing
divides — the draw walks a running total over `u32`, so one seed picks the same
member on every machine.

| Ring | Declared shares | Ring total | Share |
|---|---|---|---|
| BMO `idle` | `idle` 80, `sing` 10, `skate` 10 | 100 | 80%, 10%, 10% |
| BMO `walk` | `walk` 30, `ballwalk` 10 | 40 | 75%, 25% |
| Buddy Bot `idle` | `idle` 20, and 10 each to `idle-blink`, `idle-breathe`, `idle-listen` | 50 | 40%, 20%, 20%, 20% |
| Cat `idle` | `idle` 20, `waiting` 10 | 30 | 67%, 33% |
| Jotaro `idle` | `idle` 30, `waiting` 10 | 40 | 75%, 25% |
| Timber Wolf `idle` | `idle` 30, `scan` 10 | 40 | 75%, 25% |
| Trump `idle` | `idle` 30, `waiting` 20 | 50 | 60%, 40% |

Only two of the seven rings declare more than one number, because an undeclared
member is already 10: `idle = 80` against two silent variants is the whole of
80/10/10. The shares above are what a million draws actually produced, not what
the arithmetic promises.

### 7. Physics, Perches, and five verbs

The character obeys gravity. Grab it, fling it, it arcs and lands. This is the
novelty, and it is roughly an integrator plus collision against a rect list that
is already being polled.

**Window collision is top edges only.** Each visible window's top edge is a
one-way **Perch**: land on it, walk along it, fall off the ends, pass up through
it. Sides and bottoms are ignored entirely. That is most of the perceived
aliveness for a fraction of the collision work, and it avoids the bad cases —
sprite trapped inside an occluded window, jitter where windows overlap.

A bottom edge is read for one thing and it is not collision: telling whether a
window has come to contain a resting sprite — dragged over it, or walked into
where two windows overlap — in which case the sprite steps up onto that
window's top edge rather than being left standing inside a rectangle. Unless
that edge is itself nowhere to stand — hidden behind a window in front of it,
or hanging over no display — and then the sprite falls instead, because
stepping up would strand it somewhere the user cannot see it. The floor is
exempt, because it sits at the foot of the usable frame and windows reach below
that, and a sprite on the ground in front of a window is not trapped in
anything.

**A moving Perch carries the sprite.** Standing on a platform that moves means
moving with it, so a window dragged down, up or sideways takes the sprite along
at the place it held on the edge. Dropping the sprite whenever its Perch
shifted only looks right for a downward drag; moved up or sideways, the window
leaves it in mid-air. Riding changes position and never
velocity, or flinging a window across the desktop would launch the sprite
ballistically.

**The gate is on acceleration, not on speed.** There is no top speed a Perch
may travel at. The sprite rides while the edge's speed changes gently and is
left standing where it was — falling from there — when the speed changes by
more than the gate allows: a yank, a maximize, a window flung across the
screen. A speed limit was the obvious alternative and it is the wrong one. A
window already sliding quickly is something the sprite has hold of and keeps
hold of; what breaks a grip is the edge jumping to a speed it was not at a
moment ago. The gate measures that change against the edge's speed of a tenth
of a second ago rather than the previous frame's, because at 16 ms the window
server's own jitter reads as a jump. The number is a feel value, tuned against
a real dragged window rather than derived.

A resize is a move like any other. Dragging a window's top border down carries
the sprite and yanking it drops it, under the same gate, because the Perch is
matched between polls by the id the platform gives each window rather than by
its geometry — an opaque token the Engine only compares for equality, and one
a resize does not change.

Occlusion is a landing rule and never a resting one. An edge hidden behind a
window in front of it is nowhere to land, but a sprite already standing on one
stays there: raising a window over a Perch moves nothing, and re-deriving
visibility every tick drops the sprite through the edge it is sitting on the
first time the user clicks a maximized window.

The verb set is fixed at five: **Grab**, **Throw**, **Poke**, **Menu**,
**Summon**. Every verb is a tax on every Character that will ever exist, so
additions wait for v2.

### 8. Always-on-top, single z-level, aggressive hiding

One window level. On macOS: a non-activating panel at floating level, joining
all Spaces, stationary, never taking focus, never in the app switcher.

"Sits on your window" needs always-on-top. "Hides behind your window" needs
desktop level. One window cannot be both, and restacking dynamically by sprite
state produces flicker on every platform. Peeking out from behind windows is
given up deliberately.

The investment goes into **hide rules** instead: fullscreen and the hotkey. A
fullscreen application moves the Character to a free display, and fades it only
when no display is free. A companion that knows when to disappear is the difference between a pet
and malware.

Do Not Disturb is not a hide rule. Being quiet is not being gone: the Character
stays visible and stops starting things — Director proposals are refused and
unprompted dialogue is not spoken — while Poke, Grab, and Throw still work. That
is #84. Screen capture is not a hide rule either: capturable is a window-level
on/off switch, visible by default ([ADR-0024](./docs/adr/0024-capturable-by-default.md)),
so there is nothing to fade.

### 9. Sensing: no permissions until they buy something

**First run grants nothing.** Window awareness uses
`CGWindowListCopyWindowInfo` polling at ~10Hz, which returns window bounds,
owner app, and layer with no permission prompt. Smoothness comes from
interpolating in the render layer, not from event fidelity. Sitting on a window's
edge needs geometry and nothing else. Naming what the character sits on is a different
question: window *titles* and *application names* both sit behind one consent,
which on macOS 10.15+ is Screen Recording
([ADR-0032](./docs/adr/0032-one-consent-for-titles-and-application-names.md)).

Accessibility becomes a deliberate upgrade tied to the Functional Layer, where
the user understands the trade. The upgrade path is settings: a **What the
fidget can see** pane names each grant, what it buys, and what it costs, and
the system prompt fires only when the user flips one on.

Both of those grants are macOS TCC rows, and Linux has no equivalent: X11
geometry, `WM_CLASS`, frontmost, idle and DPMS need no grant, and there is no
Dock SPI to ask for. So the pane offers no row on Linux and says so instead.
#250 holds what changes when Linux does have a grant to offer.

A third row asks for Input Monitoring, and it buys reaction time rather than
sensing: with it the frame loop hears a mouse-only, listen-only event tap, so a
poke or the cursor arriving over the art lands at once instead of waiting for
the next idle wake a second later (#183, #721). The rule above makes it
shippable: unchecked is the shipped state, the prompt fires when the user
checks the box, and without the grant the loop backs off. The tap's mask holds
six mouse types and no key event, and a listen-only tap can neither modify nor
divert what it hears. X11 needs no row: XI2 raw events are prompt-free (#562).

**Capture tiers (Ambient, On-Demand, Local Gate) are dropped.** Fidget never takes
screenshots, never analyzes screen pixels, and never embeds OCR or vision models for
desktop content awareness. Free sensing — OS metadata without permissions — is the only
sensing tier shipped. Agents that need pixel access or desktop control use harness-native
computer use (Cursor Cloud Agents, Codex Computer Use plugin, Hermes computer_use toolset)
or attach an MCP server like cua-driver.
[ADR-0031](./docs/adr/0031-drop-capture-tiers.md).

### 10. No Executor

Fidget does not post synthetic mouse or keyboard events. It ships an **MCP
server** exposing fidget-side tools — speak, play a Behavior, list windows,
describe the screen, read and write Memory — and attaches a user-configured
Harness. Clicking is the Harness's job. The character is Fidget's.

The verification behind this:

- At the API level, Anthropic's
  [computer use tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool)
  is reasoning only. The model returns actions; the client executes them. The
  [reference implementation](https://github.com/anthropics/anthropic-quickstarts/blob/main/computer-use-demo/README.md)
  is a Docker/Linux container driving X11 with `xdotool`. Embedding an SDK means
  writing the executor.
- At the product level this changed on
  [23–24 March 2026](https://claude.com/blog/dispatch-and-computer-use): Claude
  Code and Claude Cowork do computer use natively on macOS, Windows following
  about ten days later. The Harness genuinely brings its own executor.

Consequences accepted:

- The capability is a research preview gated behind a Pro or Max subscription.
- Not portable across Harnesses. Other vendors follow the API pattern — actions
  out, client executes — so "BYO Harness" does not imply "any Harness can drive
  the desktop." A Harness without an executor can still chat and sense.
- Permissions belong to the Harness, which runs its own consent dialogs.

A `CGEvent` executor stays on the shelf as the answer if the subscription gate
proves fatal. It is not built on spec.

Rejected:

- **Spawn Claude Code as a subprocess** — fastest demo, wrong foundation. It is
  a coding agent in a costume, and Fidget would learn what happened by parsing
  stream output.
- **Provider abstraction layer** — MCP already is that layer.

### 11. Permission surface: as small as possible

Fidget owns consent for **sensing only** — window titles and application
names today, the microphone once voice ships. It owns **no** consent for acting, and does not duplicate the
Harness's confirmation prompts. Two dialogs for one click teaches users to click
through both.

Harness activity is surfaced in a visible Action Log. One denylist stays
Fidget's regardless of what the Harness permits: password fields and
explicitly excluded applications never enter a sensing result.

No undo system. A real undo journal for arbitrary desktop actions is a research
project, and a fake one is worse than none.

### 12. Memory is one shared file the user owns

A single record of what the characters know about the user, shared by every
Character Instance. Instances differ in personality and behavior, never in
knowledge. A second character knows your name on day one.

**One Markdown file**, append-structured under stable headings. Malformed
content is still valid Markdown, so a bad hand-edit degrades rather than breaks.
Headings are advisory and never parsed for correctness. It is the format the
model writes best, which matters because the `remember` tool does the writing.

The user can read it, edit it in any external editor, and wipe it. A single
timestamped backup is kept before each wipe. The loader tolerates malformed
content rather than crashing, and treats the file as untrusted input — the user
can type anything into it and it reaches Harness prompts.

Memory reaches the Harness as **MCP tools** (`recall`, `remember`), not as
injected prompt text. Tools mean Fidget does not own relevance ranking, and
every read and write appears in a log the user can inspect.

Splitting per-Instance memory back out stays possible later. It is not built now.

### 13. Voice: nothing listens by default

- **Trigger** — global hotkey push-to-talk, plus click-to-chat. Wake word is an
  opt-in, and when enabled uses **on-device detection only** (openWakeWord,
  Porcupine). Nothing is transmitted until the name fires.
- **Transcription** — a trait with two implementations. On macOS 26+, Apple's
  `SpeechAnalyzer` / `SpeechTranscriber`: on-device, no model download, no
  binary bloat, and benchmarked around twice as fast as Whisper Large V3 Turbo.
  Everywhere else — Windows, Linux, older macOS — `whisper.cpp` via
  [`whisper-rs`](https://github.com/tazz4843/whisper-rs).

An always-listening microphone in a desktop pet is the fastest available route
to being called spyware.

### 14. Multi-monitor: one coordinate space

Physics runs in a single coordinate space, the one every display shares.
Reconciling per-display physics would be considerably harder.

The overlays are per display, and not by choice: macOS gives each display its
own Space and draws a window spanning two of them on only one, so a window sized
to the union is invisible everywhere but the display it belongs to. Every
display therefore gets its own overlay covering it, and every overlay is told
where the sprite is, in its own coordinates. A sprite straddling a boundary is
drawn by both and clipped by each to its own half, so the halves meet at the
seam. One code path builds and configures an overlay, because click-through,
window level, Spaces membership and hide rules have to be identical across all
of them.

Two real problems to budget for: differing backing scale factors between
displays, and gaps between non-aligned displays. Clamp to the union of visible
frames, not to the bounding rectangle, so the sprite cannot walk into dead
space.

## v1 scope

**In:**

- Spatial Layer: Tauri overlay, physics, Perches, five verbs, hide rules,
  multi-monitor, tray
- Character Package format, engine Primitives, declarative Behaviors
- Shipped Characters in two styles, 9 required animations each
- Director on the free sensing tier — Character Prompt once, then short
  follow-ups (what just happened, time of day, State, frontmost window,
  recent Behaviors). No permissions required.
- Static-weights fallback when no Harness is attached
- MCP server and Harness attach
- Chat surface
- Memory

**Deferred:**

- Voice: hotkey push-to-talk, transcription, wake word
- Published Character Package format and authoring documentation

**Dropped (not deferred):**

- Ambient Capture, On-Demand Capture, and Local Gate. Fidget never takes screenshots,
  never analyzes screen pixels, and never embeds OCR or vision models.
  [ADR-0031](./docs/adr/0031-drop-capture-tiers.md).

**Explicitly not planned:** a Fidget Executor, an undo system, a provider abstraction
layer, per-Instance memory.

With nothing configured, Fidget is a complete product: spatial layer, physics,
Static Director, ambient reactions, and a nudge to connect a Harness. No API
key, no subscription, no permission prompts. That state is the default demo.

## Prior art

Feature comparison versus six software desktop pet alternatives (animated overlay
characters, not chat apps or hardware robots) is in
[docs/research/alternatives.md](./docs/research/alternatives.md): Desktop Mate,
VPet-Simulator, Shimeji-ee, Desktop Pet, OpenPets, MateEngine.

Technical references that informed design:

- **[Shimeji-ee](https://kilkakon.com/shimeji/)** — New BSD, Java/Swing,
  Windows-first, macOS via patched forks. Originally Shimeji by Yuki Yamada,
  Group Finity, 2009, zlib/libpng. Per-character XML behavior graphs. Read for
  its formalisation of what a desktop mascot can do; rejected as a foundation.
- **[WindowPet](https://github.com/SeakMengs/WindowPet)** — MIT, Tauri + React,
  three platforms, 45+ pets, custom pets, pixel-perfect drag, click-through,
  above-taskbar placement. No physics, no window awareness, no model. The
  reference implementation for the overlay mechanics.
- **[desktop-homunculus](https://github.com/not-elm/desktop-homunculus)** —
  MIT/Apache, Bevy, 3D VRM characters, MOD system with a TypeScript SDK, and a
  built-in MCP server for driving characters from Claude Code or Codex. macOS
  supported, Linux planned, early alpha. The closest existing thing to this
  idea.
- **[UI-TARS-desktop](https://github.com/bytedance/UI-TARS-desktop)** — Apache-2.0,
  Electron, vision-only. Evidence that a model-agnostic open-source Executor
  exists, if the Harness ever stops bringing one. Not a companion.

Differentiators, stated deliberately: 2D pixel-art nostalgia rather than 3D VRM;
a Director that gives the character its own life rather than a puppet an agent
poses; window-edge physics, which neither project has.

## Open risks

- **Click-through hit-testing** is the first thing that can look broken. Solve
  it before anything else in the overlay.
- **Director quality** is unproven. A model that proposes dull or repetitive
  Behaviors makes the whole thesis feel worse than static weights. Recent
  Behavior IDs feed back in to suppress repeats; measure this early.
- **The Pro/Max gate** on Harness computer use limits who can use the Functional
  Layer at all.
- **Wayland** degrades the spatial layer to nearly nothing.
- **Prompt injection** reaches a model with Harness access through three paths:
  Character Packages, the Memory file, and window titles.
- **Asset generation is the top risk to the character library.** Pixel art is
  the cheapest format to store and the hardest to generate: image models produce
  pixel-art-*styled* images at high resolution, with anti-aliased edges and
  drifting palettes, rather than grid-aligned sprites. Consistency of one
  character across the six to eight frames of a walk cycle is the hard part, not
  any single frame. Test the mitigation before committing to a library size —
  one high-resolution reference sheet per Character, downscaled by a scripted
  nearest-neighbour and fixed-palette pass so every frame lands on the same grid
  — and prove it on one Character before authoring ten.
- **A disconnected display** strands the sprite on coordinates that no longer
  exist. Handle it explicitly; it is otherwise the first bug report.

## Decision index

Numbers are the original decision log's and do not match the sections above.
The index maps a numbered decision to its ADR.

| # | Decision | Choice |
|---|---|---|
| 1 | Centre of gravity | Nostalgia companion first; productivity as a second layer |
| 2 | Meaning of "interact with screen" | Spatial first (geometry, Perches); functional second (Summoned) |
| 3 | Platforms | Cross-platform architecture, macOS first; Windows ships (NSIS) with remaining stub/degraded cells; one Linux build takes the X11 lane — [ADR-0020](./docs/adr/0020-x11-lane-no-native-wayland.md) |
| 4 | Harness | BYO via MCP — see decision 17 |
| 5 | Runtime | Tauri (Rust + webview) — [ADR-0001](./docs/adr/0001-greenfield-tauri-not-fork-windowpet.md) |
| 6 | Model's role in idle | Director proposes Behaviors occasionally — [ADR-0004](./docs/adr/0004-director-outside-frame-loop.md) |
| 7 | Character | First-class package format, with pre-built Characters shipped |
| 8 | macOS window awareness | `CGWindowListCopyWindowInfo` polling @10Hz, no permissions |
| 9 / 14 | Behavior ownership | Engine-owned Primitives, Character-declared Behaviors — [ADR-0002](./docs/adr/0002-engine-owns-primitives-characters-declare-behaviors.md) |
| 10 | Physics and verbs | Gravity + Throw; Perch = window top edges only; five verbs, capped |
| 11 | Z-order | Always-on-top, non-activating, `canJoinAllSpaces`; aggressive auto-hide |
| 12 / 16 | Sensing | Free tier only (OS metadata; titles and application names behind one consent); Capture dropped — [ADR-0031](./docs/adr/0031-drop-capture-tiers.md), [ADR-0032](./docs/adr/0032-one-consent-for-titles-and-application-names.md) |
| 13 | Codebase origin | Greenfield; WindowPet (MIT) as reference — [ADR-0001](./docs/adr/0001-greenfield-tauri-not-fork-windowpet.md) |
| 15 | Voice | Hotkey PTT + click-to-chat; wake word opt-in, on-device detection only |
| 15b | Transcription | Trait: Apple `SpeechAnalyzer` on macOS 26+, `whisper.cpp` elsewhere |
| 17 / 22 | Computer use | MCP server + MCP host; no first-party Executor — [ADR-0003](./docs/adr/0003-no-executor-harness-owns-desktop-control.md) |
| 18 | Capture processing | Dropped; Fidget never takes screenshots — [ADR-0031](./docs/adr/0031-drop-capture-tiers.md) |
| 19 | Permissions we own | Sensing only. Never duplicate the Harness's action prompts |
| 20 | Memory | One shared plaintext Markdown file the user owns; chat history session-scoped |
| 21 | No Harness attached | Fully charming — full Spatial Layer, chat shows a connect nudge |
| 23 | Art | True pixel art, integer nearest-neighbour — [ADR-0006](./docs/adr/0006-pixel-art-integer-scaling.md) |
| 24 | Displays | One overlay per display, each covering it; physics spans them all |
| 25 | Release staging | v1 is charm, chat, Memory, and Harness attach; voice deferred, Capture dropped |
