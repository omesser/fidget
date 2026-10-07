# Fidget v1 — specification

Vocabulary is defined in [CONTEXT.md](../CONTEXT.md) and used precisely throughout.
Decisions and their rejected alternatives are recorded in [DESIGN.md](../DESIGN.md)
and in [docs/adr/](./adr/). This document does not re-argue them.

Scope is v1 as cut in DESIGN.md: Spatial Layer, Character Packages, Director on
desktop sensing without screen recording, MCP server, Harness attach, chat, Memory.
Voice is deferred.
Ambient Capture, On-Demand Capture, and Local Gate are
dropped (not deferred) per [ADR-0031](./adr/0031-drop-capture-tiers.md).

## Problem Statement

Desktop mascots from the Windows 95 era had presence. They lived on your screen, reacted
to what you were doing, and cost nothing to keep around. They also could not do anything
useful, and the genre died.

The modern replacement is a chat window. It has no presence, no continuity, and no
awareness of the machine it runs on. You go to it; it never comes to you. Meanwhile
agent harnesses have become genuinely capable of operating a computer, and they are
presented as a text box.

Someone who wants both — a companion that is pleasant to have on screen *and* able to do
real work — currently has to choose. The existing desktop pets have no model behind them.
The projects that put a model behind a mascot make the character a puppet the agent
poses, so it has no life of its own and looks broken whenever the model is thinking or
absent.

## Solution

Fidget is a desktop companion with two separable layers.

The **Spatial Layer** is always on, entirely local, and contains no model. A sprite lives
on your screen, obeys gravity, can be grabbed and thrown, and treats the top edge of
every visible window as a **Perch** it can land on and walk along. It needs no
permissions, no API key, no network, and no Harness. With nothing configured at all,
Fidget is still a complete product.

The **Director** gives the character a life. It observes low-cost context — the
frontmost application's name, how long you have been idle, the time of day, what
the character has done recently — and proposes a **Behavior** for the local engine to
play. It never runs in the frame loop and never drives animation directly, so the
character stays visibly alive while anything else is slow or unavailable. Static
weights fill the role when no Harness is attached. An attached Harness fills it
from the same conversation as chat. See [ADR-0008](./adr/0008-one-harness-session.md).

The **Functional Layer** is invoked deliberately. Fidget exposes an MCP server of
fidget-side tools and attaches a **Harness** the user supplies. The Harness reasons and
acts; Fidget never posts synthetic mouse or keyboard events itself. Summoning the character
opens a chat surface that reaches the attached Harness.

**Memory** is one shared Markdown file recording what the characters know about the user.
Every **Character Instance** reads the same file. The user can open it in any text editor,
edit it, and wipe it.

Characters are packages, and several ship with the app. The format is first-class from day one so
that adding a character is drawing, not programming.

## User Stories

### Presence and first run

1. As a new user, I want Fidget to work immediately after install with no account, no
   API key, and no permission prompts, so that I can see whether I like it before
   committing anything.
2. As a new user, I want the character to appear on screen as soon as the app launches, so
   that I know it is running.
3. As a user, I want the character to stay visible above my other windows, so that it remains
   a companion rather than something I have to go looking for.
4. As a user, I want the character to follow me between desktops and Spaces, so that it does
   not vanish when I switch context.
5. As a user, I want the character never to steal keyboard focus, so that it cannot interrupt
   what I am typing.
6. As a user, I want Fidget not to appear in my application switcher, so that it does
   not clutter the list of things I am actually working in.
7. As a user, I want Fidget to launch at login if I choose, so that the character is simply
   always there.
8. As a user, I want a tray or menu bar icon, so that I can reach settings and quit
   without finding the sprite.

### Physics and spatial life

9. As a user, I want the character to obey gravity and fall to the bottom of the screen, so
   that it feels like a physical thing rather than a floating decal.
10. As a user, I want to grab the character with my mouse and drag it anywhere, so that I can
    put it where I want it.
11. As a user, I want to throw the character by releasing a drag while moving, so that it
    arcs across the screen and lands, because that is the part that is fun.
12. As a user, I want the character to land on the top edge of my windows and sit there, so
    that it appears to inhabit my desktop rather than float over it.
13. As a user, I want the character to walk along a window's top edge and fall off the end,
    so that its movement has consequences.
14. As a user, I want the character to ride a window I drag slowly and fall when I yank
    it or close it, so that a Perch that is still underfoot stays underfoot.
15. As a user, I want the character to pass upward through a window edge rather than being
    blocked from below, so that it never gets stuck under something.
16. As a user, I want the character never to become trapped inside or behind a window, so
    that I do not have to hunt for it.
17. As a user, I want the character to move smoothly rather than jumping between positions,
    so that it looks animated rather than teleporting.
18. As a user with several displays, I want the character to walk between them, so that it
    can reach whichever screen I am working on.
19. As a user with displays of different resolutions, I want the character to stay the right
    apparent size on each, so that it does not become tiny or enormous when it crosses.
20. As a user with displays that are not aligned, I want the character never to walk into the
    empty space between them, so that it does not disappear.

### Interaction

21. As a user, I want to click the character and get a reaction, so that it acknowledges me.
22. As a user, I want clicks that land on the transparent area around the sprite to reach
    the window underneath, so that the character never blocks my work.
23. As a user, I want right-clicking the character to open a menu, so that I can switch
    characters, reach settings, or quit from where I am looking.
24. As a user, I want a deliberate way to summon the character into a conversation, so that
    chatting is something I choose rather than something that happens by accident.
25. As a user, I want the character to get out of the way when I go fullscreen, so that it
    does not appear in my video or my presentation.
26. As a user, I want to keep the character out of screen captures and shares, so that I do
    not have to explain it in a meeting.
27. As a user, I want the character to respect Do Not Disturb (#84), so that it stays visible
    but does not start unprompted wakes — the Shell refuses proactive wakes and Behavior
    proposals are not applied — while replies to prompted interactions still show in both the
    speech bubble and Chat, and Poke, Grab, and Throw still work.
28. As a user, I want a hotkey that hides and shows the character instantly, so that I can
    banish it without opening settings.

### Character and life

29. As a user, I want to choose between the characters that ship with the app, so that I
    can pick one I like looking at.
30. As a user, I want each character to move and react differently, so that switching
    feels like a different companion rather than a reskin.
31. As a user, I want the character to do things on its own while I work, so that it feels
    alive rather than parked.
32. As a user, I want its idle behavior to reflect its personality, so that a lazy
    character and an energetic one are visibly different.
33. As a user, I want the character to react to me opening a different application, so that it
    seems aware of what I am doing.
34. As a user, I want the character to settle down or sleep when I have been idle a long time,
    so that it is not distracting when I am away.
35. As a user, I want its behavior to shift with the time of day, so that late-night use
    feels different from the morning.
36. As a user, I want the character not to repeat the same behavior over and over, so that it
    does not become obviously mechanical.
37. As a user with no Harness attached, I want the character to still have idle life, so that
    the app is worth running offline or without an agent.
38. As a user, I want the character to keep moving and reacting while the model is thinking,
    so that latency never makes it look frozen or broken.
39. As a user, I want to spawn more than one character, so that I can have several on screen.
40. As a user, I want to give each character a name when I spawn it, so that I can tell them
    apart and address them.
41. As a user, I want to dismiss a character I no longer want, so that I can prune them.

### Character packages

42. As a character author, I want to create a character by supplying artwork and a
    Character Manifest, so that making one is drawing rather than programming.
43. As a character author, I want a small required animation set, so that I can ship a
    working character in an evening.
44. As a character author, I want to supply extra animations beyond the required set and
    have them used automatically, so that effort is rewarded.
45. As a character author, I want to describe my character's personality in plain
    language, so that its idle life matches how it looks.
46. As a character author, I want to declare my own behaviors by composing engine
    primitives, so that my character moves distinctly.
47. As a character author, I want clear validation errors when my package is wrong, so
    that I can fix it without reading source.
48. As a user, I want a broken or malicious character package to be rejected rather than
    crashing or hanging Fidget, so that installing one is safe.
49. As a user, I want a character's personality to be unable to grant it new abilities, so
    that no character can talk its way into doing more than the others.

### Memory

50. As a user, I want the character to remember things about me between restarts, so that I do
    not re-explain myself daily.
51. As a user, I want a second character to already know my name, so that spawning one does
    not feel like starting over.
52. As a user, I want to open the memory in a normal text editor and read exactly what it
    knows, so that nothing about me is hidden in an opaque store.
53. As a user, I want to edit the memory by hand and have Fidget pick up my changes, so
    that I can correct or remove something without asking the character nicely.
54. As a user, I want a malformed hand-edit to degrade rather than break the app, so that
    a typo does not cost me the character.
55. As a user, I want to wipe the memory completely in one action, so that I can start
    clean.
56. As a user, I want a backup kept before a wipe, so that an accidental wipe is
    recoverable.
57. As a user, I want to see when the character writes something to memory, so that I am not
    surprised by what accumulates.

### Functional Layer

58. As a user, I want to attach an agent Harness I already have, so that I am not forced
    onto a provider Fidget chose.
59. As a user with no Harness attached, I want everything else to keep working with a
    clear prompt to connect one, so that the app is never inert.
60. As a user, I want to chat with the character in a window that belongs to the character, so
    that the conversation feels like it is with the character rather than with a text box.
61. As a user, I want the character to visibly react while the Harness works, so that I can
    tell something is happening.
62. As a user, I want the Harness to be able to read what the fidget can see, so that I can
    ask about what is on my screen.
63. As a user, I want the Harness to be able to make the character speak and act, so that
    answers arrive through the character.
64. As a user, I want Fidget not to add a second confirmation on top of the Harness's
    own, so that approving an action takes one decision, not two.
65. As a user, I want a visible log of what the Harness did, so that I can review actions
    after the fact.
66. As a power user, I want to point any MCP-capable harness at Fidget directly, so that
    I can wire it into my existing setup.
67. As a user, I want password fields and applications I exclude never to be readable by
    the character, so that there are places it categorically cannot look.

### Settings and control

68. As a user, I want to see exactly what context the Director sends, so that I can judge
    for myself what leaves my machine.
69. As a user, I want to turn the Director off entirely and keep the static-weights
    fallback, so that I can run the character with no model calls at all.
70. As a user, I want ambient Director wakes to stay cheap — backoff, and none while
    the display is asleep — so that liveliness does not mean a heartbeat bill.
71. As a user, I want Fidget to update itself, so that I do not have to track releases.

## Implementation Decisions

### Overall shape

Tauri application. A Rust core owns state, physics, platform access, the MCP server, and
Memory. A webview front end renders the sprite and the chat surface, and holds no
authoritative state.

Three concentric parts:

- **Engine** — pure, synchronous, no I/O. Owns physics, State, Perch collision,
  Behavior selection and playback, and Character validation.
- **Adapters** — trait implementations that reach the outside world: `WindowSource`,
  `Director`, `Clock`, renderer. Memory is not one of them: it is a concrete type
  that reads and writes the Memory Manifest directly.
- **Shell** — the Tauri app, tray, windows, settings, MCP server, Harness attachment.

These are enforced by the crate layout, not only by convention. The workspace has
a **core crate** carrying the Engine, the overlay hit-testing arithmetic, and
Memory, which depends on neither `tauri` nor any platform binding; and a **shell
crate** depending on core, which holds `main.rs`, the Tauri setup, the AppKit
panel work, and `WindowSource`. Memory performs file I/O and belongs in core
regardless: the rule is no platform or toolkit dependency, not no I/O.

The purpose is to make the Engine's purity a build property rather than a
property of the source that the next change can quietly remove. It also lets the
core crate's tests and lints run on a Linux runner, where the shell cannot be
compiled at all. `cargo tree` on the core crate is the check, run in CI against
every target rather than the host alone: a platform dependency hidden behind a
`cfg` would be invisible to a host-only tree.

### The Engine seam

The Engine is the single seam for the entire Spatial Layer and the Director's observable
effect. Its contract:

- Input: a `WorldSnapshot` carrying display frames, Perch-eligible window rectangles in
  descending z-order, cursor position, pending interaction verbs, elapsed time since the
  previous tick, and any Behavior proposal delivered since the last tick.
- Output: a `Frame` carrying sprite position and velocity, current State, the current
  animation identifier with how long that Animation has been playing, the draw that
  picks among that Animation's variants, and optional dialogue.

Which frame of that Animation is on screen is not the Engine's to say. Selecting a frame
needs the fps and the loop mode, and a Character Manifest declares both per Animation.
The Engine reports the elapsed play time it does own, and `Animation::frame_at` maps that
onto an index. Nor is which art plays under one name: a Character may declare a variant
of an Animation the Engine already asks for, and the Engine reports only a draw — taken
when the Animation starts, from a seed it is handed — that `Character::draw` weighs
against what the Manifest declared.

The Engine performs no I/O, holds no timers, and reads no clock. Time enters only as
elapsed milliseconds on the snapshot. This makes every physics and behavior property
testable by constructing snapshots and asserting frames, with no windowing system, no
model, and no waiting.

### Platform layer

`WindowSource` is a trait producing `WorldSnapshot` geometry. The macOS implementation
polls `CGWindowListCopyWindowInfo` at 10Hz while the sprite is still, and at the Engine tick
(16 ms) while it is riding a moving Perch, which returns window bounds,
owning application name, and layer with no permission prompt. Geometry is what that
buys without consent. Window titles and the owning application name both require the
one sensing consent (Screen Recording on macOS, or the platform equivalent, e.g.,
xdg-desktop-portal ScreenCast on Wayland) and reach `WindowRect` only when it is
usable; without it the snapshot carries geometry and no names (ADR-0032). MCP
resources answer to the same consent.

Window level is a platform concept and stays in the Shell. The Shell drops every window
above or below the ordinary application level while assembling the snapshot, so the menu
bar, the Dock and other system overlays are never Perches, and the Engine is handed a
world in which every rectangle is one. Elapsed time is clamped to one poll interval on
the same seam: after a sleep or a suspend the world is stale anyway, and integrating
minutes of gravity against it would fling the sprite off the desktop.

Smoothness is the renderer's responsibility. The Engine interpolates between polls rather
than depending on event fidelity.

Each platform implementation declares its capabilities rather than assuming them.
`window_geometry` and `absolute_positioning` are declared capabilities. The Wayland arm
declares neither, and the Spatial Layer degrades to screen-edge physics only. This is a
supported degraded mode, not an error state. Geometry is permanent. Positioning is
`zwlr_layer_shell_v1`, unwired here and absent on Mutter (`DESIGN.md` decision 3).

Multi-monitor uses one coordinate space. Physics clamps to the union of visible display
frames rather than to its bounding rectangle, so the sprite cannot enter gaps between
non-aligned displays. Backing scale factor is resolved per display at render time, not in
the Engine.

There is one overlay window per display, each covering that display. No window can be
sized to the union: macOS gives each display its own Space and draws a window spanning
two of them on only one, so a union-sized overlay is invisible on every display but the
one it belongs to. The set follows the desktop, so a display attached or removed while
the app runs gains or loses its overlay without a restart.

An Instance reports which display it is on as the index of that display in the
current arrangement, and its placement as its feet in that display's full frame.
The origin is the frame's top-left. Y grows downward. The units are logical
points. The index lasts for one arrangement. Index 0 is whichever monitor
the platform listed first. macOS does not keep CGDirectDisplayID, Windows does
not keep HMONITOR, and Linux does not keep the GDK monitor. A display attached
or removed is in the next successful read. Wayland with no X server does not
refresh the list. The `whereabouts` MCP tool returns that report: every
connected display, with its name when the platform has one, its origin, and
its size; and for each Instance, which display it is on, the other connected
displays, and its feet in that display. A caller does not convert coordinates.

Every overlay is told where the sprite is, in its own coordinates, and each draws the
part that falls inside it. A Character straddling a seam is therefore whole: the two
halves are clipped by the two overlays and meet at the seam. The Engine is unaffected —
it works in the space all displays share and knows nothing about windows.

### Overlay and input

One always-on-top overlay window per display per Character Instance. On macOS: a
non-activating panel at floating level, joining all Spaces, stationary, excluded from the
application switcher, never accepting first responder status. Identical on every overlay,
which is why one code path builds and configures them all.

Click-through is decided once per tick, in the shared point space, and applied to the
overlay the cursor is on; every other overlay passes clicks through, so a sprite on one
display never swallows a click on another.

Click-through is per-window rather than per-pixel in Tauri. The overlay tracks the cursor
and toggles ignore-mouse-events by hit-testing the sprite's current alpha, so transparent
regions pass clicks through. WindowPet's implementation is the reference under MIT.

Z-order is a single fixed level. Restacking by sprite state is rejected. Hiding is
implemented as visibility rules: fullscreen and a global hotkey. Fullscreen moves the
character to a free display, and hides it only when every display is taken. Screen
capture is a separate window-level switch, capturable by default
([ADR-0024](./adr/0024-capturable-by-default.md)). Do Not Disturb keeps the character
visible and quiet.

### Character Package

A directory or archive containing a Character Manifest, animation frames, a
Personality Prompt, and Behavior declarations.

The engine owns **Primitives**. A Character composes them and cannot define new ones. A
**Behavior** is a named sequence of Primitives with weights and trigger conditions,
declared as data. Behaviors are validated on load: unknown Primitives, missing required
animations, and cyclic or unbounded sequences are rejected with a specific error naming
the offending declaration.

Required Animation Set is exactly nine: `idle`, `walk`, `fall`, `land`, `sit`, `sleep`,
`react`, `talk`, `hold`. A declared optional set is used when present and silently absent
otherwise: `climb` falls back to `walk`, and `grab` — the sprite held by the cursor — and
`jump` both fall back to `fall`.

A Personality Prompt influences Director output only. It cannot reference Primitives that
do not exist, cannot enable capabilities, and is never forwarded to the Harness as
instructions.

Package loading is a pure function from bytes to either a validated Character or a list
of errors, which places it inside the Engine seam.

### Character Instance

A Character Instance is a Character plus a user-supplied name and a generated stable id.
Instances differ in Character, name, position, and current Behavior. They do not differ in
knowledge.

### Director

A trait returning an optional Behavior proposal given a context record.

v1 context is desktop sensing only — how long the frontmost application has been
in front, up to three earlier fronts with their stays, idle minutes, weekday and
time of day, State, what just happened, what the feet stand on (window owner,
display floor above the Dock, or screen edge), recent Behavior identifiers, and
the two authored prompt layers — the active Character's Personality Prompt and
this Instance's own Instance Prompt (opening turn only, ADR-0012). Application
names and window titles both require the window-names consent (Screen Recording
on macOS, ScreenCast on Wayland); without it the context carries no application
names and no title, and the Perch is a window rather than a named application's
window (ADR-0032). No screen capture, no clipboard, no input contents.

The opening payload is the Character Prompt. Later session turns are a short
follow-up. Both are inspectable in settings as the last user turn.

Two implementations ship:

- **Static** — weighted selection over the Character's declared Behaviors using their
  trigger conditions. No model, no network. Used when no Harness is attached, when the
  Director is disabled, and as the fallback on any session error or timeout. The Shell
  may wake it often: the path is free.
- **Session** — an attached Harness, the same conversation as chat. Proposes a Behavior
  identifier plus optional dialogue. The identifier is read from the first line that is
  a name and nothing else, so a Harness writing its own text ahead of the answer does
  not cost the turn (#609); every other line is the spoken one, whichever side of the
  name it falls, because nothing tells a stray sentence from a Harness's chrome. A
  reply that is not a declared Behavior plays
  `talk` and is spoken (`say:`) in a bubble. An HTTP Completer fills the same role
  behind the same trait ([ADR-0008](./adr/0008-one-harness-session.md)).

A session wake is reactive (the user addressed the character: Poke, Throw, Grab
start, landing on a Perch, Summon, a chat turn) or proactive (exponential
backoff whose `model_base` and `model_power` the Character Manifest names).
It does not run on a fixed interval, and it does not run while the display
is asleep. The first proactive model call waits two minutes. A Poke wakes it
once the OS double-click interval passes with no second click, so a
double-click wakes it once, as a Summon.

A proposal is advisory. The Engine may refuse it if the proposed Behavior is unknown,
disallowed in the current State, or would repeat a recently played Behavior. Recent
Behavior identifiers are supplied back as context to suppress repetition.

The Director is never awaited on the render path. A pending proposal is applied on the
next tick after it arrives, or discarded.

### Memory

One Markdown file, append-structured under stable headings. Headings are advisory and are
never parsed for correctness. Malformed content is still valid Markdown, so a bad
hand-edit degrades rather than breaks.

Shared by every Character Instance. Every recall reads the file, so an edit made outside
Fidget is visible to the next recall without a watcher or a reload path. A single
timestamped backup is written before a wipe.

Memory is treated as untrusted input. It reaches Harness prompts, and the user can type
anything into it.

Memory reaches the Harness as MCP tools rather than injected prompt text, so Fidget owns
no relevance ranking and every read and write appears in the tool log.

### MCP server and Harness

Fidget exposes an MCP server. Tool surface, by responsibility:

- **Expression** — make the character speak; play a named Behavior.
- **Sensing** — list visible windows with bounds and owning application; describe what is
  on screen (window metadata only; Capture is dropped per
  [ADR-0031](./adr/0031-drop-capture-tiers.md)).
- **Memory** — recall; remember.
- **Identity** — list Character Instances and their names.
- **Placement** — report connected displays and where each Instance sits on them.

There is no tool that posts mouse or keyboard events. Fidget ships no Executor. See
[ADR-0003](./adr/0003-no-executor-harness-owns-desktop-control.md).

A Harness is attached by user configuration. Any MCP-capable harness can attach
directly. No provider abstraction layer is built; MCP is that layer.

Actions taken by the Harness are surfaced in the Action Log. Fidget adds no confirmation
of its own for acting, and owns consent only for sensing. A denylist is Fidget's
regardless of Harness permissions: password fields and user-excluded applications never
enter any sensing tool result.

### Chat surface

A webview window owned by the Character Instance that was Summoned. Messages route to the
attached Harness. With no Harness attached, the surface explains how to connect one rather
than failing.

While a Harness turn is in flight, the Spatial Layer continues to run normally. The character's
visible reaction comes from Behaviors the Harness plays through the expression tools and
from the Engine's own idle life, never from blocking on the turn.

## Testing Decisions

### What makes a good test here

Tests assert external behavior at a seam, not internal structure. A test constructs
inputs, runs the unit under test, and asserts on its outputs. It does not reach into
private state, does not assert that a particular function was called, and does not depend
on the order of internal operations.

Concretely for this project: a physics test asserts where the sprite ends up, not that a
particular integrator method ran. A Behavior test asserts which animation plays and for
how long, not which selection branch was taken. A Character validation test asserts the
error the author sees, not which validator produced it.

Tests must not sleep, poll, or depend on wall-clock time. Time enters the Engine as
elapsed milliseconds on a snapshot, so a test advances time by constructing the next
snapshot. `Clock` is a trait for the same reason.

### Primary seam: the Engine

The overwhelming majority of tests drive the Engine directly, feeding `WorldSnapshot`
sequences and asserting `Frame` output. No windowing system, no model, no network, no
waiting. Coverage:

- **Physics** — gravity produces expected fall; a Throw produces a ballistic arc; the
  sprite comes to rest; it never leaves the union of visible display frames; a Throw that
  meets the usable top bumps it rather than flying off; it never enters a gap between
  non-aligned displays.
- **Perch collision** — the sprite lands on a window's top edge; walks along it; falls off
  either end; passes upward through an edge from below; rides a Perch dragged under the
  ride-acceleration gate and Holds while it does; treats a resized top edge as a moved one,
  riding or dropping by that same gate; falls when the window is yanked or closed; is not
  Lifted onto a Perch it just lost; falls when the window disappears; behaves correctly
  when two windows overlap; is never left inside a window that has come to contain it;
  never lands on an edge hidden behind a window in front of it, on one no display covers,
  or on one flush with the usable top; falls when a window is raised in front of the edge
  it is standing on.
- **State machine** — every transition reachable from every State; no State is a dead end;
  Grab overrides any State; releasing a Grab with velocity enters Throw and without
  velocity enters fall.
- **Verbs** — each of the five verbs produces its expected State or output, and verbs
  arriving in the same tick resolve deterministically.
- **Behavior playback** — a Behavior plays its Primitives in order; a Behavior that
  becomes invalid mid-play is abandoned cleanly; a Behavior is refused when its Primitives
  are not permitted in the current State.
- **Behavior selection** — weighted selection is deterministic given a seeded source;
  trigger conditions gate correctly; recently played Behaviors are suppressed.
- **Director proposals** — a valid proposal is applied on the next tick; an unknown
  Behavior identifier is refused without disrupting current play; a proposal arriving
  during a Grab is deferred or dropped rather than yanking the sprite.
- **Character validation** — the nine required animations are enforced; optional
  animations are used when present and absent silently when not; unknown Primitives are
  rejected by name; a Behavior that cannot terminate is rejected; every rejection produces
  an error message naming the offending declaration.

### Second seam: MCP tools

Dispatch is tested at tool-call level with a fake `WindowSource` and a temporary Memory
file, not over the MCP transport. Coverage: each tool's success shape; behavior when no
Character Instance exists; that no tool posts input events; that the denylist removes
excluded applications and password fields from every sensing result.

The published stdio binary dispatches nothing: it relays to the app, which dispatches
where the Instances live (ADR-0026). It is tested against a stand-in app on loopback.
Coverage: the endpoint is discovered from the environment and the token travels in a
header; a URL naming anything but this machine is refused; no app, a refused token, an
oversized answer and an answer with no body are each reported as a failure rather than a
success; a notification is answered with nothing.

### Memory

Tested as a store against a temporary file. Coverage: round-trip of a remembered fact; an
edit made outside Fidget is visible to the next recall; malformed content still reads
back and preserves what it can; wipe writes a backup first; a hand-written file that has
never been touched by Fidget reads back correctly.

### Fakes, not mocks

`WindowSource`, `Director`, and `Clock` get hand-written fakes with straightforward
behavior. Assertions are on Engine output, not on fake interactions. Memory is
exercised as itself against a temporary file, because a single-implementation trait
faked in place of a file buys nothing the file does not already give.

### Not unit tested

Rendering, the Tauri shell, tray behavior, panel level and Spaces membership, and
click-through alpha hit-testing are verified by hand on a real machine. They are thin,
platform-specific, and expensive to fake convincingly. Click-through in particular is
listed as a known risk and is checked manually against overlapping windows on multiple
displays.

This list exempts **behavior that needs a window server**, not code that happens to sit
beside it. Arithmetic is never exempt, wherever it lives. A coordinate conversion in
`main.rs` is as testable as one in the Engine, and the file it occupies says nothing
about whether it can fail quietly.

### A bug in pure logic earns a test before the fix ships

Whichever file it was found in, and whether or not the seam was agreed in advance.

Four bugs taught this, each pure arithmetic and each found by measuring a running
program rather than by review:

- The display union computed in physical pixels, producing a 7296x2234 overlay on a
  3648x1117 desktop.
- The cursor converted with one scale factor across two different physical spaces.
- The window level discarded, so the Dock and the menu bar became Perches.
- The one-way platform rule, which could be inverted with every test still passing.

The two fixed without a test were rediscovered later by mutation. A commit message warns a
reader who may never arrive; a test warns the next change.

## Out of Scope

Deferred to a later version, decided but not built:

- Voice: hotkey push-to-talk, wake word, transcription. Transcription will be a trait
  with Apple `SpeechAnalyzer` on macOS 26+ and `whisper.cpp` elsewhere.
- Freezing the Character Package format. `docs/DEVELOPMENT.md` documents it for
  authors, but a key can still change until v2.

Decided against, not merely deferred:

- A Fidget Executor. Desktop control belongs to the Harness.
- A second confirmation layer over the Harness's own action prompts.
- An undo system for desktop actions.
- A provider abstraction layer over harnesses.
- Per-Instance memory. May become configurable later; not built now.
- Ambient Capture, On-Demand Capture, and the Local Gate. Fidget never takes screenshots,
  never analyzes screen pixels, and never embeds OCR or vision models. Agents that need
  pixel access or desktop control use harness-native computer use or attach an MCP server
  like cua-driver. [ADR-0031](./adr/0031-drop-capture-tiers.md).
- Desktop-level or dynamically restacked z-order, and peeking out from behind windows.
- Window side and bottom collision.
- Interaction verbs beyond the five.

## Further Notes

**The riskiest unproven thing is Director quality**, not any of the platform work. A
Director that proposes dull or repetitive Behaviors makes the product feel worse than
static weights, which is a thesis failure rather than a bug. Measure it early against the
static fallback, with the same Characters.

**Prompt injection reaches a model with Harness access through three paths**: Character
Packages, the Memory file, and window titles under consent.

**Attribution.** WindowPet (MIT) is the reference for click-through and the tray/updater
shape. Attribution lives in the README's prior-art section.
