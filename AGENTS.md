# Fidget

## Agent skills

### Issue tracker

Issues live in GitHub Issues on `omesser/fidget`, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage roles, each label string equal to its name. See `docs/agents/triage-labels.md`.

### Picking work

`Todo` on the project board, highest priority first, matching the OS
you can build. See `docs/agents/picking-work.md`.

### Vendored skill sets

pstack and Matt Pocock's engineering set are vendored into `.agents/skills/`,
each skill a directory holding a `SKILL.md`. `.agents/pstack/SKILLS.md` and
`.agents/mattpocock/SKILLS.md` index them, one line each. These are the skills
behind `/implement`, `/research` and `/tdd`.

None of them is on by default. Apply one when the user names it, by reading that
skill's `SKILL.md` and following it. `poteto-mode` is sticky once entered — it
stays on across turns — so never enter it unprompted.

`docs/agents/vendored-skills.md` records where a vendored skill's opinions
collide with this repository's rules, and this repository's rules win.
`.agents/README.md` covers where the bytes live and how the syncs work.

### Domain docs

Single-context. Vocabulary is `CONTEXT.md`. v1 scope and requirements are
`docs/SPEC.md`. Decisions live in `docs/adr/` at the repo root. See
`docs/agents/domain.md`.

Before editing `docs/research/`, an ADR, `docs/SPEC.md`, `DESIGN.md` or
`CONTEXT.md`, check where that writing belongs. Research is anchored to the
tree it read and does not follow `main`. See `docs/agents/docs.md`.

## Taking over the GUI

Ask before launching the app, focusing a window, or sending input: say what
happens on screen and for how long, then wait for a go-ahead for that run. Only
the agent's own private VM is exempt. See `docs/agents/gui-takeover.md`.

## Comments

Comments say why, not what, and earn their length. See `docs/agents/comments.md`.

## Writing

Squash merges make the pull request title the only line that reaches `git log`.
An agent signs every comment it writes on GitHub. See `docs/agents/writing.md`.
