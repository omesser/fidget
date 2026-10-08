# Where Writing Belongs

**Litmus:** if changing it next month leaves product architecture the same, it does not belong in an ADR.

## The Map

| Artifact | Holds | Mutable |
|---|---|---|
| `docs/adr/` | One hard-to-reverse decision + why + rejected alternatives | Decision no (supersede); wording yes (compaction) |
| `docs/SPEC.md` | What v1 must do | Until the version cuts |
| `DESIGN.md` | Product shape narrative | Evolves with product |
| Issue / PR | Work, acceptance, verification | Yes |
| `docs/research/` | Investigation, forensics, advisory lists | Yes |
| Code comment | Local why beside the binding | With the code |

## Research is anchored, and does not follow `main`

A `docs/research/` document is dated forensics: it describes the tree the
investigation actually read. Re-pointing its citations at today's code would
make the prose describe code it never saw, so an anchored document does not
track `main`. The number that looks stale is the record.

- **Declare the anchor once, in the preamble, and let it govern the whole
  document.** A note scoped to one section is what sends the next reader to
  `main` with a diff and a rot report (#620).
- **Cite a symbol wherever the prose allows:** A name survives the refactor
  the anchor exists to record; a line number survives nothing. Keep a number
  for a verbatim quote's provenance, and for a spot inside a function too long
  to name.
- **Verify a citation against the anchor, not against your checkout:** A
  refreshed-but-wrong number is the same bug with newer digits.

A finding that has since shipped earns a preamble line saying so. Rewriting
the finding does not — that is a second investigation, and it gets its own
document.

## ADR gates — all three, or skip it

1. **Hard to reverse** — once shipped it cannot be undone cheaply.
2. **Surprising without context** — a reader one year from now will ask why.
3. **Real trade-off** — something was rejected for a reason that still matters.

Skip otherwise. An easy revert, an obvious choice, or no alternative considered means the decision lives elsewhere.

## Do not put in an ADR

- File/module/crate paths, lib selections, feature-flag snapshots
- UI recipes, CSS, tokens, pixel rules, coordinate arithmetic
- API/type/constant names, method signatures
- Smoke diaries, verification dates, build logs
- Post-mortem forensics, issue digests, work blow-by-blow
- How to run/test/reproduce — that is DEVELOPMENT or the README

Mechanism stays fluid. Freeze architecture bets only.

## Editing an ADR

An editorial compaction that leaves the decision unchanged may edit an ADR in
place. A changed decision gets a superseding ADR, and the superseded one is
removed, leaving one `**Supersedes:** [ADR-NNNN](<link pinned to a commit>) (removed).`
line in its successor.

## For agents

Treat ADR as a constraint on architecture, not a checklist of paths or CSS. Contradicting an ADR requires superseding it or explicitly reopening the decision — not rewriting the PR to match last month's implementation snapshot.

An ADR naming a crate path or a UI token two months ago is documenting fluid mechanism, not architecture. If that path or token no longer matches the codebase, the ADR is stale and must be superseded, not followed.
