# Soft+Hard Review Gate

When to post this gate: an agent reviewing a pull request applies Soft+Hard to assess whether a change is safe to merge. Post as a GitHub pull request review with event `COMMENT`, not `APPROVE` or `REQUEST_CHANGES` when posting as the repo owner account.

## Axes

Three axes combine into one merge verdict.

**Soft (Standards).** Does the code follow this repo's documented standards, including the smell baseline from the code-review skill? Documented violations fail Soft. Baseline smells are judgment calls; cite them with the smell name and the hunk, not as hard violations. A documented repo standard always overrides the baseline.

**Spec (Done-when).** Does the code faithfully implement what the originating issue or spec asked for? Missing requirements, scope creep, or wrong implementation fail Spec. When code appears Done but live verification is still OPEN, mark Spec as `Pass (provisional)` rather than Pass. A provisional pass does not block the gate.

**Hard (Blast radius).** What could this break outside the diff, and is the one safety fact proven? A change is safe when the one fact it depends on is verified to step 4 or 5 from blast-radius (ran a script or reproduced in the app, not just pointed at a line). Hard Pass with no findings = "none" (no breakage outside the diff). List unproven safety facts or real risks as Hard Fail.

## Format

Every Soft+Hard gate comment follows this structure.

**Header** (required). One line. Tip SHA, then **Cleared** or **Not cleared**.

```
## Soft+Hard — tip `<sha>` — **Cleared**
```

or

```
## Soft+Hard — tip `<sha>` — **Not cleared**
```

**Summary table** (required). Soft / Spec / Hard / Total, each Pass or Fail. Spec may read `Pass (provisional)` when code Done-when holds but live prove is still OPEN.

```
| Axis | Verdict |
|---|---|
| Soft | Pass |
| Spec | Pass |
| Hard | Pass |
| **Total** | **Cleared** |
```

**Total** is Cleared only when Soft=Pass AND Hard=Pass AND Spec≠Fail. Soft Fail or Hard Fail or Spec Fail → Not cleared.

**Sections** (required). One section per axis, with heading `### Soft — Pass|Fail`, `### Spec — Pass|Fail|Pass (provisional)`, `### Hard — Pass|Fail`. Each section holds findings as bullets. OPEN findings in Soft are labeled `Soft-FIX`. Hard findings are labeled `Hard-FIX` when they block.

## Standing rules

- Do not wait for CI or list "make it green" as a gate condition. CI is orthogonal.
- Do not include local runner paths or artifacts (tip SHA, PASS/FAIL, and what was checked only).
- Agents never merge. Oded merges.
- Sign off per `docs/agents/writing.md` at the end.
- After posting a new Soft+Hard gate review on a PR, hide/minimize all prior Soft+Hard gate reviews from the same reviewer on that PR (GitHub: minimize as Outdated) so only a single up-to-date Soft+Hard review remains visible. Do not leave stacked obsolete Cleared/Not cleared gates.

## Skeleton

Copy-paste and fill.

```
## Soft+Hard — tip `<sha>` — **[Cleared | Not cleared]**

| Axis | Verdict |
|---|---|
| Soft | [Pass | Fail] |
| Spec | [Pass | Pass (provisional) | Fail] |
| Hard | [Pass | Fail] |
| **Total** | **[Cleared | Not cleared]** |

### Soft — [Pass | Fail]

[No findings | findings as bullets with smell names or standard citations]

### Spec — [Pass | Pass (provisional) | Fail]

[No findings | missing/wrong/creep as bullets with spec quotes]

### Hard — [Pass | Fail]

[none | findings as bullets with real risks and proof level]

_— [Bot name], on [@omesser](https://github.com/omesser)'s behalf._
```

## Example: Cleared

```
## Soft+Hard — tip `a1b2c3d` — **Cleared**

| Axis | Verdict |
|---|---|
| Soft | Pass |
| Spec | Pass |
| Hard | Pass |
| **Total** | **Cleared** |

### Soft — Pass

No standards violations. No baseline smells.

### Spec — Pass

Issue #123 asked for perch position persistence. Implemented: saves position on unmount, restores on mount. Verified manually.

### Hard — Pass

Only touches perch position logic. Callers unchanged. Ran integration test `tests/perch_position.rs` against the branch; persisted across restart as expected. Safety fact (state writes idempotent) proven at step 4.

_— Cursor agent (by Architect), on [@omesser](https://github.com/omesser)'s behalf._
```

## Example: Not cleared

```
## Soft+Hard — tip `e4f5g6h` — **Not cleared**

| Axis | Verdict |
|---|---|
| Soft | Fail |
| Spec | Pass (provisional) |
| Hard | Fail |
| **Total** | **Not cleared** |

### Soft — Fail

- **Soft-FIX** Mysterious Name: `process_thing` in `engine.rs:45`. What thing? Rename to `apply_gesture_to_sprite` or similar.
- Possible Speculative Generality (judgment call): `future_hook` parameter added but not used. Issue #456 asks only for current behavior; remove the hook or document why it's needed now.

### Spec — Pass (provisional)

Issue #456 Done-when: gesture moves sprite, no clip. Code reads correct; in-app verification not yet run.

### Hard — Fail

- **Hard-FIX** `apply_gesture_to_sprite` calls `update_position` on every frame. That writes to the same JSON backup file concurrently when two sprites move. File corruption risk (high likelihood, loses user state). Safety fact unproven.
- Checked: no other callers modified. Cleared.

_— Cursor agent (by Architect), on [@omesser](https://github.com/omesser)'s behalf._
```
