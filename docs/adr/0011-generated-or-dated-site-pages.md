# Every page on the published site is Generated or Dated

**Status:** Accepted, amended by
[ADR-0038](./0038-a-generated-landing-page-owns-the-site-root.md): a Generated
landing page owns the site root, and the design directory moves to
`/design.html`. The two classes and the allowlist stand.

Every page on `omesser.github.io/fidget` is one of two classes. Nothing else
may exist.

- A **Generated page** is built at deploy time by a script under `scripts/` from
  repository data, so its source is the thing it describes and malformed input
  fails the build.
- A **Dated page** is a hand-written proposal, frozen, carrying its issue number
  and approval date, so it never claims to be current.

The forbidden third class is the **Described page**: hand-written prose claiming
to describe shipped behavior. It shares no link with the code that a build can
check, so the two agree until one of them changes and nothing then tells the
reader which one they are holding.

## The site is a review surface

One design question at a time, argued for @omesser and whoever is arguing it.
The README stays the front door: two front doors drift, and the README wins
because that is where a contributor already is.

A page earns its place when the thing it documents is timed, audible, or
operated, and github.com can only give you a still or a paragraph.
`docs/design/cues.html` plays a 660 Hz tone falling to 600 over a 140 ms decay
(`src/cue.js:111`); you cannot hear a table.

## The pages

The site is flat. A page exists when a design question arrives that has to be
heard or operated before it can be answered.

The gallery is Generated and shows every Character this project can publish.
That is not the same as every installed Character: a package whose art carries
terms that do not cover publishing it is left off entirely, and `WITHHELD` in
`scripts/make-character-gallery.py` is the list. Leaving it unmentioned is
deliberate — naming a package the project is unsure it may publish is its own
kind of publishing, and the manifest already records the position for anyone
reading the repository. It is also the one page meant to be found in
search: proposal pages carry `<meta name="robots" content="noindex">` so a
superseded proposal does not become the canonical result for the thing it
proposed.

Each index entry names its page's class, and each page repeats it in its own
header — `Generated from characters/ at deploy`, or `Proposal, approved in
#277`. A page reached from search arrives without the index.

There is no version tree. One file per design question, revised in place; git
holds the history and the deployment is built from the commit, so a superseded
proposal stops being reachable. One exception, default off: freeze a copy under
its own name if a merged issue links a proposal that is later replaced.

## The allowlist changes its unit

`.github/workflows/pages.yml` names every file it copies, and that list is the
security model. #289's gallery needs a few hundred PNGs, too many to name as
reviewed lines, so the workflow names every file *and* every generated tree, one
line per generator, and a guard after assembly fails the build if `_site` holds
anything but `.html`, `.css`, `.js`, `.png` or `.svg`.

The guarantee moves from a reviewed line in a diff to a reviewed script, taken
on purpose. What the allowlist exists to prevent is `docs/research/` and
`docs/agents/` getting crawlable URLs, and every one of those files is `.md`. A
generator enumerates and copies named files, never a source directory wholesale.

## Considered Options

- **A real landing page** — what the app is, screenshots, install links. Two
  front doors that drift.
- **Mirroring `docs/SPEC.md`, the ADRs and `CONTEXT.md`**: github.com renders
  them, and Jekyll does not run here, so a mirror needs a build step in order to
  be worse.
- **A downloads page**: GitHub Releases is that page and it updates itself.
- **A Perch and physics sandbox**: A second implementation of `crates/core`
  physics in JavaScript, ungeneratable from the Rust and guaranteed to drift.
  The best-looking idea here and the clearest reject.
- **A Behavior and Primitive explorer**: It is a table, and tables are
  Markdown's home turf.
- **An in-browser wasm demo**: Its own project, and the payoff misrepresents a
  thing whose whole point is sitting on top of your other windows.
- **A version archive**: Machinery for a problem two pages do not have.

## Consequences

The relabel exit is taken: `cues.html` is the #277 Dated proposal (`noindex`, a
visible class line, index `.cls`). It no longer claims to be the spec the Shell
implements. `src/cue.js` still ports that frozen page, so the two can still
drift — the copy-the-Shell exit stays open if a later change needs the live
keyframes instead of the proposal.

Reversing this means allowing a Described page, and accepting that each one
drifts from the code it describes with nothing to catch it.
