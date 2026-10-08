# No generic import on-ramp for convention-less ecosystems

`scripts/import-pet.py` gets one adapter per source ecosystem, and an adapter
requires a machine-readable convention that names poses — petscodex's atlas
rows, Shimeji-ee's `actions.xml`. An ecosystem without one gets no import path.
There is no generic fallback that takes an undifferentiated pile of PNGs and
produces a Character Package.

#121 proposed that fallback: number every frame onto a contact sheet, emit a
skeleton Character Manifest, and let a human or an agent fill the Required
Animation Set in from the sheet. #147 built it and it was driven end to end.
The flow completes. It also produces a Character that is wrong.

Measured against the source pack's own `actions.xml` — which the flow never
reads — three of nine Animations were filled from art meaning something else:
`land` from pull-up-over-an-edge frames, `sleep` from a Tripping frame, `react`
from three ceiling-cling frames that read as nothing. The bad `react` passed
`character::load` clean. Nothing caught it but rendering the package and
looking at the sprite.

That is the cost. A failure that validates is more expensive than no path at
all, because it surfaces only when someone opens the Character, and by then it
sits in a manifest that looks authored. A pile of PNGs carries no pose
semantics, so no tool reading that input can check its own answer;
categorization is a guess the flow presents as an import. An adapter knows what
each frame means, and when it is wrong it is wrong the same way on every run.

The Required Animation Set sharpens this. Nine slots, and no real pack has nine
distinct poses, so substitution is mandatory on every import — a judgment the
generic flow neither explains nor records.

A source gets a real adapter, or it gets nothing.

## Considered Options

- **The contact-sheet worksheet as built (#147):** Rejected on its own
  evidence. Beyond the mis-mapping: the sheet renders 96px thumbnails of 128px
  art, too small to tell poses apart — the agent driving it wrote a throwaway
  script to re-render frames at 3x before it could categorize anything. The
  review bar requires `walk` to head right; the mode mirrors nothing and offers
  no flag, so left-facing art cannot pass without mirroring every PNG by hand
  first. Pointing `-o` at the worksheet directory reports success while
  deleting the contact sheet and the frame index.
- **A higher-resolution contact sheet:** Fixes legibility and nothing else. The
  three wrong Animations were wrong about pose *meaning*, not about pixels — a
  bigger thumbnail of a ceiling-cling frame is still a frame whose purpose is
  unrecoverable without the pack's convention.
- **Hand-categorization with no tool:** Already possible: write the Character
  Manifest, point `character::load` at it. It needs no mode in the importer,
  and it does not dress a guess up as an import.

## Consequences

Supporting a new ecosystem means adapter work — reading its convention and
teaching the importer its pose semantics. That is the price, and it is paid
once per ecosystem rather than once per pack.

#121 is closed unbuilt and #147 is closed unmerged. The `--format frames` mode
is not in the tool; the importer keeps the two adapters it has. #112's promise
of a contact-sheet on-ramp is withdrawn, and its body says so.

Reversing this needs new evidence that a generic flow can fail loudly — that a
mis-categorized Animation can be rejected rather than validated. #147's branch
stays for reference.
