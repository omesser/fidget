# Writing

Voice belongs to the
[`developer-voice`](https://github.com/omesser/ai-goodies/tree/main/skills/developer-voice)
skill. Read it there if you do not have it loaded. This file holds the one thing
no skill can know: where writing lands in this repository.

## The pull request title is the line that survives

`main` takes squash merges only. The commit subject is the pull request title
and the body is empty, so nothing else reaches `git log`.

- Write the title as the one-sentence summary a reader should find a year from
  now.
- Branch commit bodies serve the reviewer and the merge discards them. Keep them
  short, or leave them out.
- Reasoning that has to outlive the review goes in a code comment or the pull
  request description. `docs/agents/comments.md` covers the first.

That last point overrides `developer-voice`, which puts the reasoning in the
commit body. Here the body does not survive.

## Pre-commit hooks gate every commit

Run `pre-commit run --files <touched>` or `pre-commit run --all-files` before
every commit and push. Do not use `--no-verify` to skip hooks. The suite
catches trailing whitespace, codespell findings, formatting drift, and clippy
warnings — all of which would fail in CI. Hooks that autofix (trailing
whitespace, formatters) rewrite files in place; stage the fixes and commit
again.

## The title carries its type

Titles follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>[optional scope]: <description>
```

The type is what the reader scanning `git log` wants first — whether a line is a
new capability, a repair, or housekeeping they can skip. Nothing here is
released, so no version is derived from it; the value is legibility.

| Type | For |
|---|---|
| `feat` | A new capability |
| `fix` | A repair to one that misbehaves |
| `docs` | Documentation only, including `CONTEXT.md`, `DESIGN.md` and `docs/SPEC.md` (see `docs/agents/docs.md` for where writing belongs) |
| `test` | Tests and the verification harness, with no change to what ships |
| `refactor` | A change that keeps behavior and alters structure |
| `perf` | A change made for speed or footprint |
| `ci` | Workflows, hooks, and the checks that gate a merge |
| `build` | Dependencies, the Cargo workspace, packaging |
| `chore` | Everything else that touches no behavior |
| `revert` | Undoing a merged change |

Two rules the type table cannot settle:

- **Classify by what the change is for, not by which files it touches.** A fix
  that ships with the documentation correcting it is a `fix`. Tests written for
  behavior landing in the same pull request are part of that `feat`; `test` is
  for a pull request whose product *is* the test.
- **A scope is optional and only earns its place when it narrows something.**
  `engine`, `shell`, `character`, `harness`. Skip it when the description
  already says where the change lives.

The description after the colon keeps the imperative sentence this repository
has always written: `feat(engine): Ride a resized Perch under the same gate`.
Capitalized, no full stop. Conventional Commits does not rule on case, and
matching the existing history matters more than matching other projects.

Mark a breaking change with `!` before the colon — `feat(engine)!: ...` — and
say what breaks in the description.

## The description answers three questions

`.github/pull_request_template.md` asks them: **Why**, **What changed**, and
**How to verify**. Why comes first because a reviewer who does not know the
problem cannot judge the solution — a description that opens with what it added
makes them reconstruct the problem from the diff.

A pull request small enough that all three answers are one sentence each should
give one sentence each. The template is a floor, not a quota.

## A visual change shows itself

When a pull request changes what the product looks like, the description
carries the artifact: the new asset, and the old one beside it when the change
*is* the difference between them. A reviewer who would have to build the branch
to see a corner radius does not build the branch — they approve on the diff,
which is the one thing that cannot show them a corner radius. Motion is no
exception; a contact sheet of frames says more than a sentence about timing.

Upload the image with `gh pr create --attach` or `gh pr comment --attach`, in
`<file>#<alt text>` form. GitHub rewrites a body reference such as
`![alt](./before.png)` to point at the uploaded asset, which outlives the branch
and the squash merge. Do not commit an image only to show it in a pull request;
nothing references it after the merge.

This repository is public, and an upload cannot be taken back. Attach a window
or a region, never a whole display: the rest of the screen is the owner's
desktop. Open the image at full size before uploading it. The
`screencapture-is-cropped` pre-commit hook holds scripts to the same rule.

Check an image in only when it documents an asset that lives here.
`branding/app-icon-preview.png` is the worked example, and it earns its place
next to the icon it previews, not by being in a pull request description.

## An agent signs what it writes on GitHub

An agent that writes on GitHub through a person's account ends every issue,
pull request description, review, comment, and gist with one line. The handle
in the line is the account owner.

A named bot writes its product, then its name in parentheses:

```
_— Grok bot (Coder), on [@omesser](https://github.com/omesser)'s behalf._
```

A subagent writes its product, then `subagent of` and its parent in
parentheses. Use the parent's bot name. If the parent has no bot name, use its
Harness name. Take the parent from the launch message. Bare `Cursor agent` is
wrong for a subagent.

```
_— Cursor agent (subagent of Coder), on [@omesser](https://github.com/omesser)'s behalf._
_— Cursor agent (subagent of Grok Build), on [@omesser](https://github.com/omesser)'s behalf._
```

Harness names are `Cursor`, `Claude Code`, `Grok Build`, `Codex`, `OpenCode`,
`Hermes`, and `Pi`. An attach not on this list uses its command name.

GitHub shows the owner as the author of everything an agent posts. The line
tells a reader which text an agent wrote and which text is the owner's.
`Co-authored-by` covers commits only.

Sign once, at the end of the body. Owner text stays unsigned. If an agent edits
a body the owner wrote, leave it unsigned and explain the edit in a signed
comment.
