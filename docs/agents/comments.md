# Comments

A comment says why. It stays short. It earns every line.

The default ceiling is 3 lines per block. A public item keeps a one-line summary, so `cargo doc` and an IDE hover still say what the item is. A Rust `//!` module header is exempt. It documents the module. A `///` item doc is not exempt. A block a script prints as its takeover prompt is exempt too. It is operator output, and `docs/agents/gui-takeover.md` has the agent read a scenario's screen, input and duration out of it.

Code comments are not audit logs or decision logs. Keep a longer record only when the implementation is weird and the record is what makes that implementation defensible.

## Decide in this order

1. Delete a block that restates what the code does.
2. Delete a sentence that narrates how the code came to be. The pull request description holds that history.
3. Keep a remaining why in at most 3 lines, in the present tense.
4. Keep a bare issue or ADR number only when the code is not understandable without opening it. When the sentence already states the fact, drop the number.
5. Keep the one-line summary on a public item.

Stop when you do not understand the comment. Leave it alone.

A remaining why may run past 3 lines in four cases.

- The implementation is weird, and the comment is what makes it defensible.
- The block is a `ponytail:` marker. It names the ceiling, the cost, and the condition that changes it.
- The block names a bound that looks arbitrary.
- The block names a rejected alternative a reader would otherwise try again.

If a comment exists because the code is unclear, fix the code in a different pull request. A comment edit stays comment-only.

## What earns the words

`PERSONALITY_LIMIT` in `crates/core/src/character.rs` is the bound. The first draft ran five lines:

```rust
/// How long a Personality Prompt may be, in characters.
///
/// The prompt is untrusted text that goes into every Character Prompt the
/// Director sends, so an unbounded one spends a user's tokens and buries the
/// sensing context under prose. Generous enough for a paragraph of personality.
```

The comment that ships is three lines.

```rust
/// How long a Personality Prompt may be, in characters. Untrusted text in every
/// Character Prompt the Director sends, so an unbounded one spends tokens and
/// buries sensing under prose. Generous enough for a paragraph of personality.
pub(crate) const PERSONALITY_LIMIT: usize = 2000;
```

Without those lines, 2000 is a number the next reader is free to change.

The Dock note on `a_walk_into_the_dock_climbs_onto_it_rather_than_behind_it` in `crates/core/src/engine.rs` is the constraint. Three lines. The fact is in the comment, so the block cites no issue.

```rust
/// The Dock is the one thing on screen drawn in front of the sprite, so a
/// walk that carries on under it puts the sprite where nobody can see or
/// grab it. Its side is a wall.
```

## What does not

A comment that restates the code does not earn its length. From a one-line getter this repository once shipped:

```rust
/// Every Animation's frames as `data:` URLs, for the webview to draw from.
///
/// Handed over once. The webview picks a frame out of this by the name and
/// index each `Frame` carries, so it still holds no authoritative state —
/// only the art, which is not state.
pub fn art(&self) -> &BTreeMap<String, Vec<String>> {
```

The module doc above it already said why the frames are `data:` URLs, and `src/main.js` already says the webview owns no state. The first line is the whole comment.

```rust
/// Every Animation's frames as `data:` URLs, for the webview to draw from.
```

Delete the restatement.

## History stays out of the comment

Leave out how the bug was found and what the code used to do. A squash merge keeps the pull request title and drops the commit body, so that history does not reach `git log`. Put it in the pull request description when a reviewer needs it.

A comment that stops the next reader putting the bug back is the exception. The Dock note above is that shape. It says where the sprite must not end up, and why.

## `ponytail:` comments

A deliberate shortcut with a known ceiling names the ceiling and the upgrade path. From `backup_path` in `crates/core/src/memory.rs`.

```rust
/// ponytail: seconds since the epoch rather than a civil timestamp. It sorts
/// correctly and costs no date library; swap it for an ISO stamp if one ever
/// arrives for another reason. Two wipes in the same second share a name, and
/// the later one wins.
```

The ceiling, the cost, and the condition under which it changes. Without all three it is a shortcut nobody can price. The summary above the marker is part of the same block. On `backup_path` that summary is two lines, and the marker adds four. The marker is the exception that keeps the block.

## The lint

The bar on this page is 3 lines. `pre-commit` runs [ast-grep](https://ast-grep.github.io/) and rejects a comment block over 20 lines (`scripts/ast-grep/rules/comment-blocks.yml`). The rule file says the 20 is what the tree passes today, and that tightening it is a separate pull request. A green lint does not mean a block meets the 3-line bar. Module docs, printed takeover headers and `ponytail:` markers sit over 3 lines on purpose.

Each language has its own rule in that file. Adding a language means adding the rule.

Covered languages on ast-grep 0.45.3: Rust, JavaScript, TypeScript, Python, Bash, Swift, CSS, YAML, HTML.

**PowerShell (`.ps1`) is intentionally ungated.** No off-the-shelf linter provides a comment-block ceiling. PSScriptAnalyzer in this repository gates Error and ParseError severity only. A custom Measure rule would be ours to own.

**TOML (`.toml`) is ungated.** #813 replaced the Python hook, and ast-grep does not parse TOML. `customLanguages` can wire `@ast-grep/lang-toml` without a per-platform build. That wire-up is a separate change.

Do not assume a `.ps1` or `.toml` file is ceiling-checked.
