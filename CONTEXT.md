# Fidget

A desktop companion in the spirit of Windows 95-era desktop mascots: an animated
sprite that lives on your screen, reacts to the windows around it, and can be
asked to do real work on your machine.

## Language

### The product

**Fidget**:
The product, written in title case in UX copy, docs, and headings.
_Avoid_: Lowercase for this display name. Paths, slugs, and variables use the lowercase slug

### The character

**Character**:
The shippable unit a user installs and chooses between — identity, art,
personality, and tuning bundled together.
_Avoid_: Pet, mascot, avatar. Title-case Fidget is the product, not a Character

**Character Package**:
The on-disk form of a Character: a directory or archive containing its
animations, Character Manifest, personality prompt, and behavior tuning.
_Avoid_: Skin, theme, mod, plugin

**Character Manifest**:
The declaration at the root of a Character Package: the frames, fps and loop
mode of each Animation, the Behaviors the Character declares, and how
proactive model calls space themselves. Frame size and frame count are read
from the art rather than declared.
_Avoid_: Manifest on its own — a Memory Manifest is one too

**Personality Prompt**:
The natural-language description of who a Character is, carried in its package
and given to the Director. Governs demeanour only, never capability.
_Avoid_: System prompt — that is the Character Prompt, which carries this and more

**Character Prompt**:
The opening turn of the Director session: Personality Prompt, Instance Prompt,
the Behaviors it may propose, and this moment. Later wakes send a short
follow-up (what just happened, recent Behaviors, time of day, State, what the
user is doing) in the same conversation. Assembled rather than written: two of its
layers are authored, the whole is never hand-authored. The Prompt tab shows
the three concatenated layers — app-level instructions, Personality Prompt,
Instance Prompt — and empty ones say Empty. Inspectable in settings as well.
ADR-0012.
_Avoid_: Persona, preamble, prompt template

**Instance Prompt**:
The user's own layer of the Character Prompt, written per Character Instance and
empty by default. Follows the Personality Prompt as a second voice layer, bound
by the same rule: demeanour only, never capability. ADR-0012.
_Avoid_: System prompt, custom instructions, jailbreak

**Blank AI**:
The Director mode that empties the built-in prompt layers — the package
Personality Prompt and the app-level instructions (voice rules, Behavior roster,
reply contract) — and still sends an Instance Prompt the user wrote. Off by
default. The Prompt tab shows those emptied fields as Empty: what you see is
what is sent. With no contract the reply is prose, so the character talks and plays
no Behavior unless the Instance Prompt asks for one. That is the control run
for telling a model's misbehaviour apart from the shipped prompt, and for
iterating a prompt under Fidget's conditions. #657, #680.
_Avoid_: Empty personality — that is a Character with an empty file, which
still gets the voice rules and the contract

**Animation**:
A named frame sequence belonging to a Character. Pure art with no logic.
_Avoid_: Clip, sprite, sequence

**Variant**:
An Animation declared `variant_of` another. A base Animation and its variants
form a ring, and when the engine asks for the base it draws one member of that
ring instead — chosen once, when the Animation starts, and played until the
engine asks for a different Animation. A member's `weight` is its share against
the others and 10 when undeclared, so a ring nobody weighs is an even split and
a member can be weighed below the default as well as above it.
Nothing bounds the total. The draw is seeded: one seed and one Character draw
the same members on every machine. #316.
_Avoid_: Alternate, costume, skin, random animation, mood

**Left Strip**:
An Animation declared `left_of` another, drawn in its place while the sprite
travels left instead of the renderer mirroring the base. The same number of
frames and the same frame size as the base, so turning round changes only which
art plays. For asymmetric art: a mark on one cheek survives the turn, where a
mirror would move it to the other. Optional — an Animation with no strip is
mirrored as every Character's has always been. #345.
_Avoid_: Flipped animation, left walk, direction variant

**Required Animation Set**:
The animations every Character Package must supply for the engine to drive it.
_Avoid_: Base set, defaults

**Character Instance**:
One spawned character: a Character plus a user-given name and a stable id. Instances
differ in personality and behavior, never in what they know — the Instance
Prompt is where that difference is written. ADR-0012.
_Avoid_: Session, spawn, copy, clone

**fidget**:
In prose, a synonym for a Character Instance: the presence on the desk people
used to call a buddy, not the Character Package and not the sprites being
rendered. Prose may say character or fidget for that presence; the same
lowercase spelling is the product's slug in paths and identifiers.
_Avoid_: Title-case Fidget for this presence — that is the product

**Memory**:
The single durable record of what the characters know about the user. Shared by
every Character Instance, and owned by the user: readable, editable in any text
editor, and wipeable.
_Avoid_: History, context, knowledge base, store, profile

**Memory Manifest**:
The on-disk form of Memory: one Markdown file of facts under stable headings,
which the user may read and edit by hand.
_Avoid_: Manifest on its own — a Character Package has one too

### Life on screen

**State**:
Where the sprite is anchored and which physics apply — grounded, falling,
dragged, perched, climbing, asleep.
_Avoid_: Mode, status, pose

**Primitive**:
An engine-owned unit of motion or expression that Behaviors are composed from.
Characters may compose Primitives but never define new ones.
_Avoid_: Action, command, step

**Behavior**:
A named sequence of Primitives with weights and trigger conditions, declared as
data in a Character Package. The unit the Director proposes and the engine plays.
_Avoid_: Routine, script, macro

**Director**:
The role that proposes a Behavior, and Speech when the session is on. Static
weights fill Behaviors and never speak; an attached Harness is that role and
proposes Speech by calling speak. Never runs in the frame loop and never
drives animation directly. Covers both Static Director and AI Director (ModelDirector).
The environment variables `FIDGET_DIRECTOR_*` configure the HTTP Completer,
which is one fill of this role (#466). `DirectorSettings` and `DirectorConfig`
keep those names. They hold the HTTP knobs under this role (#589).
_Avoid_: Brain, agent, planner. In user-facing Settings and README: the role
name "Director" when it means the on/off switch or HTTP configuration — say
"AI" / "AI on" for the toggle, "Model" / "API" for HTTP knobs instead

**Director session**:
The one conversation per Character Instance that proposes Behaviors and answers
the Chat surface. `harness::Session` is the ACP process that holds it.
_Avoid_: Harness session, chat thread

**Proactive model call**:
A Director session wake that fires because the character was left alone long
enough, not because the user addressed it.
_Avoid_: Unused model call, unused wake, active prompting

**Near Miss**:
A Behavior name the Director proposed that the Character declares none of —
`prowll` for `prowl`. Reported, never corrected: it reaches the user as
Speech like any other unparsed reply, and the Shell traces it so the miss is
not invisible.
_Avoid_: Typo, hallucination, invalid behavior

**Perch**:
A window's top edge treated as a one-way platform the sprite can land on, walk
along, and fall off. Window sides and bottoms are not Perches, and neither is
the length of an edge that cannot be seen: one hidden behind a window in front
of it, hanging over no display, or so close to the usable top that the art
would sit behind the menu bar. That governs landing and staying: an unseen
edge is gone, and the sprite falls. A yank past the ride gate drops it too.
A slow drag is still the same edge: the sprite Holds and rides.
_Avoid_: Ledge, platform, surface

**Hold**:
The Primitive and required Animation of gripping a moving Perch so the sprite
keeps its place on the edge. Engine-played, like Land: no Director proposes it
in time. Not a State — the sprite stays Perched.
_Avoid_: Squat, cling, grab (Grab is the verb that picks the sprite up)

**Talk**:
The Primitive and Required Animation of a talking mouth. Art, not words —
Speech may play it; a silent reaction may too.
_Avoid_: speak

**Surface**:
What the sprite stands on: a display's floor, or a Perch. The umbrella over
both, not a synonym for Perch — a Perch is one kind of Surface, and "surface"
as a loose word for a Perch stays on that entry's avoid list.
_Avoid_: Ground, platform

**Contact**:
What one tick of physics reports back to the State machine: the sprite landed
on a Surface, was lifted onto one, stands where it stood, hangs in the air, or
met a wall or the ceiling. An observation only — what the sprite becomes as a
result is the State machine's decision, never the Contact's.
_Avoid_: Collision, hit, event

### Layers

**Spatial Layer**:
The always-on, local, model-free system: physics, window geometry, Behaviors,
and the interaction verbs. Works offline with no permissions granted.
_Avoid_: Idle mode, pet mode

**Functional Layer**:
The invoked system that performs real work on the machine through an attached
Harness. Asynchronous, explicitly Summoned, and reported on by the Spatial Layer.
_Avoid_: Agent mode, assistant mode, copilot

**Harness**:
An external agent runtime the user attaches, which reasons and acts on their
behalf. Supplied by the user, never bundled.
_Avoid_: Backend, provider, model

**Completer**:
The session trait the Director role uses to answer Character Prompts, filled by
either HTTP chat-completions or an attached Harness over ACP
([ADR-0008](./docs/adr/0008-one-harness-session.md),
[ADR-0022](./docs/adr/0022-acp-client-over-official-sdk-and-named-harnesses.md)).
Director is the role; Completer is the umbrella trait both fills implement.
When no Harness is attached, the HTTP Completer streams chat-completions (first
token arrives long before the reply; a dropped call stops the host generating
rather than merely going unheard; non-streaming hosts are answered whole). When
a Harness is attached, it fills the trait instead. Settings names the HTTP
fill's timeout and turn ceiling. A Harness turn has its own budget
(#690). The environment variables
`FIDGET_DIRECTOR_BASE_URL`, `FIDGET_DIRECTOR_MODEL`, and
`FIDGET_DIRECTOR_API_KEY` configure the HTTP Completer (#466).
_Avoid_: Using "Completer" as Settings or README brand (say AI, Model, API, AI
source, or Harness), or as synonym for HTTP-only fill

**Settings draft**:
The Settings window's uncommitted rows. A row that still matches what is live
stays out of the patch.
_Avoid_: Dirty form, unsaved settings

**Turn ceiling**:
The token bound on one HTTP Completer turn, thought and answer together. A
safeguard against a model that will not stop, not a budget sized for a reply,
so one number covers every wake, local or hosted (#877). A host seen to mark
its thinking is given a higher one (#606). Settings names the row, and
`FIDGET_DIRECTOR_MAX_TOKENS` outranks both numbers.
_Avoid_: Reply cap, reply length, token budget

**Executor**:
Whatever posts synthetic mouse and keyboard events to the operating system.
Owned by the Harness or a desktop-control MCP server, not by Fidget.
_Avoid_: Driver, automation layer, robot

**Action Log**:
The readable record of what the Functional Layer did and why: the Character
Prompts sent, the answers returned, and the actions the Harness took. Points at
the Harness's own session dump rather than copying it.
_Avoid_: Memory log, transcript, audit trail

### Sensing

**Free sensing**:
OS metadata, never pixels. The only sensing tier Fidget ships. Window
geometry, time, idle duration and recent Behaviors need no permission. Window
titles and application names — the frontmost application and a window's owner
alike — need one consent, and it is the same consent for both. The tier's name
is about Capture, which it never does, not about permissions. Exposed via MCP
tools `list_windows` and `describe_screen`. ADR-0031, ADR-0032.
_Avoid_: Ambient sensing, monitoring

**Ambient Capture, On-Demand Capture, Local Gate**:
Dropped. Fidget never takes screenshots, never analyzes screen pixels, and
never embeds OCR or vision models for desktop content awareness. Agents that
need pixel access or desktop control use harness-native computer use (Cursor
Cloud Agents, Codex Computer Use plugin, Hermes computer_use toolset) or attach
an MCP server like cua-driver. ADR-0031.
_Avoid_: Saying these are "upcoming" or "deferred"

### Interaction verbs

**Grab**:
Press and move — the sprite follows the cursor. `grab` is also the optional
Animation drawn while one lasts; a package that declares none draws its `fall`,
and never `hold`, which belongs to the Perch ride. #364.

**Throw**:
Release a Grab with velocity — the sprite travels ballistically until it lands.

**Poke**:
A click on the sprite — provokes a reaction and possibly a line of dialogue.

**Menu**:
Right-click on the sprite, or Control-click on macOS — character switching, settings, quit.

**Summon**:
The deliberate act that opens the Functional Layer.
_Avoid_: Invoke, activate, wake

### Expression

**Speech**:
The line the character says. The session Director proposes it on a wake — Static
never speaks — and an attached Harness proposes it by calling speak.
_Avoid_: talk (the Required Animation), message, utterance

**speak**:
The MCP tool by which a Harness proposes Speech. Until then the session
Director proposes the same Speech without this tool.
_Avoid_: talk, say

**Speech bubble**:
A bubble above the sprite showing Speech, held for reading time (900ms
+ 55ms per character, clamped to 2–8 s). A new line replaces the old one.
While a reply streams, the bubble grows with its Speech and `talk` plays; the
Behavior name line is held back and never shown. Implemented in #119.
_Avoid_: Chat bubble, message, tooltip

**Cue**:
The Shell's acknowledgement that one interaction landed: a procedural visual
over the sprite and a synthesized sound, one pair per interaction — Poke,
Summon, Menu, pickup, drop, and a throw that is the drop played harder. The
Engine names the Cue on the frame; the webview draws and synthesizes it, so no
Character declares one. Do Not Disturb silences the sound and keeps the visual.
A machine that cannot start an audio context does the same. #277, #292.
_Avoid_: Effect, feedback, animation (the Character's art), SFX

**Thinking ellipsis**:
Three animated dots in a bubble above the sprite, shown while a reactive
Director turn is in flight (Poke, Summon, Throw). Appears after 250ms grace,
held ≥600ms once shown. Proactive wakes stay invisible. #119.
_Avoid_: Loading, spinner, progress

**Thinking row**:
A turn's streamed reasoning as a row of the Chat log, titled Thinking and drawn
apart from the reply. Open while the turn thinks, collapsed to its title once
the answer lands, and kept as a reply is kept. Never a line of the Action Log.
Not the Thinking ellipsis, which masks latency and says nothing about what a
model is doing. Either fill can write it: an attached Harness, or a model on the
HTTP fill whose server marks its reasoning apart from its reply. #483, #611,
ADR-0034.
_Avoid_: Reasoning pane, thoughts, chain of thought, transcript, thought strip

**Chat surface**:
The window a Summon opens: where the user types to the attached Harness and
reads the answers too long for a Speech bubble. Belongs to the Character
Instance that was Summoned, and is drawn by Fidget rather than by the
Harness. #17, ADR-0018.
_Avoid_: Chat window, console, terminal, prompt box, Chat UI (its visual
design, not the window)

**Chat UI**:
A named, swappable visual design for the Chat surface — palette, type scale and
shape — defined in `src/chat-ui.css` and applied by a class on the root element
of the Chat surface's own document. Not the surface itself: the surface is the
window, and a Chat UI is one of the designs it can be drawn in. Reaches nothing
the overlay draws by itself — a Character's art never, and the Speech bubble
only the opaque panel tokens in `src/chat-shared.css` that the default Chat UI
and the overlay both import (#441, #456, #545) — and is not the light or dark
theme the operating system supplies. The user picks one for the whole app in
Settings. #355, #348, #1068, ADR-0036.
_Avoid_: Look, theme, skin, style, variant, design

### The published site

**Generated page**:
A page on the published site built at deploy time by a script under `scripts/`
from repository data, so its source is the thing it describes. ADR-0011.
_Avoid_: Dynamic, templated, auto-generated

**Dated page**:
A hand-written proposal on the published site, frozen, carrying its issue number
and approval date so it never claims to be current. ADR-0011.
_Avoid_: Archived, versioned, historical

**Described page**:
The forbidden class: hand-written prose on the published site claiming to
describe shipped behavior, with nothing checking that the two agree. Named so it
can be rejected in review. ADR-0011.
_Avoid_: Documentation page, reference page
