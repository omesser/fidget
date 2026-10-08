# Spike 692: would TypeScript make this webview safer?

The issue asks whether to move `src/` from JavaScript to TypeScript. The decision is not
what a migration would cost to *perform* — a count of compiler errors answers that, and
the answer is boring. The decision is whether we would be **safer afterwards**, and
whether the tooling we would take on is worth it.

This report answers those two.

## Answer

**Keep writing `.js`. Do not add a build step.** The evidence for a transition is weak,
and the tooling price is higher here than in a normal web project.

**If we want static analysis anyway — and there is a decent case for it — `tsc --noEmit`
with `checkJs` is the right tool, used as a checker rather than as a language.** It is a
lighter dependency than a linter and it catches strictly more. That is option 1 in the
issue. Option 2 (generated payload types) is cheap and works; it is worth adding when
something actually reads a large payload. Option 3 (the full port) is the one to refuse.

## 1. Types would have caught none of the bugs this webview has shipped

Nineteen commits in this repository's history fix a bug in `src/*.js`. I read every
diff. Classified by what could have prevented them:

| | Count | |
|---|---|---|
| Needed **type annotations** to catch | **0** | — |
| Caught by **static analysis without types** | **2** | #652, #738 |
| Logic, timing, copy, or a missing feature | 17 | no checker sees these |

The two:

- **#652** (`6f1c112e`) imported `TRUNCATED_MARK` from `bubble.js` after the exporter
  dropped it. The overlay is a `type=module` page, so one missing binding is a
  `SyntaxError`, `start()` never runs, and the Character is invisible.
- **#738** (`b39b37bb`) assigned to an undeclared `loginSaid`. Strict mode makes that a
  `ReferenceError`, so a session reset cleared the log and then died before its own note.

Neither needs a type. One is an unresolved import, the other an undeclared name. Both are
the kind of thing a linter has caught since 2013.

The other seventeen are things no checker reads: a thinking bubble painting over speech,
a bubble drawn on two displays, an audio context that fails on Linux, a permission row
that led with the tool kind and buried the question. Types do not have opinions about
those.

**Both static-analysis classes are already guarded.** `tests/overlay-imports.test.js` was
added *in* the #652 fix commit; `tests/no-undeclared-assignment.test.js` in the #738 one.
They are hand-rolled approximations of two standard rules, and the second says so in its
own header: *"it is not a substitute for a real checker."*

### The honest limit of this evidence

A census of `fix(...)` commits sees only bugs that shipped, were noticed, and were fixed
under that prefix. Bugs a type checker would have stopped someone from *writing* leave no
trace anywhere. So "0 of 19" is evidence about shipped defects, not proof that types are
worthless. It is the best evidence available, and it points one way, but it is not the
whole truth and should not be quoted as if it were.

## 2. What the checkers actually say about today's code

| Tool | Findings on current `src/` | What they are |
|---|---|---|
| `eslint` (`no-undef`, `import/named`, `no-unused-vars`) | **0 errors**, 1 warning | the warning is a real dead variable, `main.js:176` |
| `tsc --noEmit --checkJs`, loose | **29 errors** | 10 untyped `window.__TAURI__`, 16 DOM casts, 3 one-liners |
| `tsc --noEmit --checkJs`, `strict` | **337 errors** | almost all "implicitly has an `any` type" |

The 29 deserve a closer look, because they are the number the issue would have us pay
down. Twenty-six are ceremony: `getElementById` returns `HTMLElement`, so every `.value`,
`.disabled` and `.closest` needs a cast that asserts something the code already knew. A
cast is not a caught bug. It silences the checker and adds a line.

That is the signal-to-noise problem with the loose config: it reports 29 things and
roughly 3 of them are about correctness.

**Both checkers catch both historical bugs.** Measured by reintroducing each:

```
eslint  →  'loginSaid' is not defined                          no-undef
           TRUNCATED_MARK not found in './bubble.js'           import/named
tsc     →  TS2304: Cannot find name 'loginSaid'.
           TS2305: Module '"./bubble.js"' has no exported member 'TRUNCATED_MARK'.
```

So `tsc` is a superset for the classes that have bitten.

## 3. The dependency question, which comes out backwards

Today the repository has **zero npm dependencies**: no `node_modules`, no lockfile, and
CI installs Node but never runs an install. `package.json` is four lines. Any static
analysis ends that, so it is worth knowing what each one costs.

| | Packages | Size |
|---|---|---|
| `typescript` | **1** (zero transitive dependencies) | 23 MB |
| `eslint` + `globals` + `eslint-plugin-import` | **194** (34 direct) | 34 MB |

This is the opposite of the usual intuition. The linter is the heavy supply-chain
commitment; the type checker is one vendor-published package that depends on nothing. If
we are going to break the zero-dependency position for a checker, `tsc` is the cheaper
break *and* it catches more.

## 4. The build step is the expensive part, and here it is worse than usual

`frontendDist` is `"../src"`. The files we read are the files we ship. There is no
frontend build anywhere in this project.

**Verified:** point `frontendDist` at a directory that does not exist and the Rust build
fails at compile time —

```
error: proc macro panicked
    --> src-tauri/src/main.rs:2805:16
     |
2805 |         .build(tauri::generate_context!())
     = help: message: The `frontendDist` configuration is set to `"../dist"`
             but this path doesn't exist
```

So under option 3 a built `dist/` is a **compile-time prerequisite for every `cargo
check`, `cargo build` and `cargo test`** — including pull requests that touch no
JavaScript at all. Every Rust contributor grows an `npm ci && tsc` step. The spike's
earlier draft listed this as unverified; it is now measured, and it is the single
strongest argument against the full port.

Three smaller losses, in descending order of how much they should matter:

- **Tests**: `tests/` is 25 files run by `node --test`. As `.ts` they need Node 22.18+
  for type stripping. CI's `lts/*` is fine; the Node 20 on this machine fails with
  `ERR_UNKNOWN_FILE_EXTENSION`.
- **The vendored renderer**: `src/vendor/marked.esm.js` is byte-for-byte what npm
  published, and `tests/chat-markdown.test.js:500` pins its sha256 against that. The test
  would still pass after bundling — it reads the source file — but it would no longer
  describe what ships. The guarantee quietly stops meaning what it says.
- **Debugging**: What runs in the webview today is what is on disk. After a build it is
  `dist/`, and reading a stack trace needs source maps to be right.

None of these is fatal. Together with the compile-time coupling they are the "high price
in tooling" the issue asks about, and it is real.

## 5. What we would actually gain

Stated fairly, because sections 1 to 4 lean one way:

- **The mechanical class**: A misread field name, wrong case, a renamed field, wrong
  nullability. `probe/misread.js` makes six such mistakes deliberately and `tsc` reports
  all six. This class has not bitten yet, but it is the class that gets more likely as
  the webview grows.
- **Editor help on payloads**: Autocomplete on `opening.`, `payload.`, `placement.` is a
  genuine daily improvement, and it does not require the full port — a generated `.d.ts`
  plus JSDoc gives it.
- **#706**: This is the real argument. Settings moves from a native window into the
  webview, `settings.js` is already 323 of the 2,850 lines here, and it reads the largest
  payload this app has (`FormDescription` with a nine-variant tagged `FormRow`, plus a
  28-field `SettingsView`). A much larger webview changes the arithmetic above.

The boundary-drift argument the issue leads with, though, does not survive contact with
the history: 31 commits changed one of the seven structs the webview listens to, and 30
changed a `src/*.js` file in the same commit. The one that did not (#322) changed
internals, not the wire. "Rust changed, JavaScript forgot" has not happened here, because
the same person changes both sides at once.

Nor would types have caught the two incidents the issue names. #654 changed a return type
from `Result<(), String>` to `Result<String, String>`; JavaScript that ignores a return is
fine either way. #659/#678 is a field that does not exist on `ChatHarness` — and a checker
cannot flag a field nobody reads, because types describe what is there, not what should
be.

## 6. Recommendation

1. **Do not port to TypeScript, and do not add a build**: Zero of nineteen shipped bugs
   needed types; the compile-time coupling in section 4 is a standing tax on every Rust
   change.
2. **If we add a checker, make it `tsc --noEmit` with `checkJs`** — one zero-dependency
   package, catches both classes that have bitten, and unlike a linter it can also read
   the payloads later. Expect to spend the 26 DOM casts to buy the 3 real findings, and
   decide whether that trade is worth it with eyes open. Running it non-blocking for a
   while is a cheap way to find out.
3. **Take option 2 when something needs it**: `ts-rs-proto/` proves generation works
   (section 7). The first real customer is #706's settings payload, not `chat.js`.
4. **Revisit after #706 lands**: That is the change that could move this answer, and it
   is the only one that should.

The status quo is not "no safety". It is 203 tests, and a habit visible in #691 and #694
of pulling logic out of `chat.js` into a small module precisely so it can be tested. That
habit has caught more than a type checker would have.

## 7. Option 2 works, and here is what it costs

`ts-rs-proto/` is a standalone crate that copies the real payload shapes and derives
`ts_rs::TS` only under `#[cfg(test)]`, so the shipped binary carries none of it.

- A test writes `generated/payloads.d.ts` under `REFRESH_BINDINGS=1` and otherwise
  compares the committed file to a fresh render — the same shape as #729's fixture test
  in `form.rs:1606`. Renaming `alive` to `missing` fails it with the refresh command in
  the message; restoring passes. Measured both ways.
- Cold build 6.9 s, warm 0.2 s. Six dev-only crates join the tree: `ts-rs`,
  `ts-rs-macros`, `thiserror`, `thiserror-impl`, `termcolor`, `syn 2`. Nothing at runtime.
- Lifetimes, `Option`, `Vec`, `BTreeMap`, `&'static str` and `#[serde(tag = "type")]` all
  render correctly. The last matters: `FormRow` uses it, and that is what #706 will read.
- Adding the generated types on top of the loose config takes 29 findings to 19, and **no
  new boundary error appears**. Today's JavaScript reads the payloads correctly.

Two gotchas, both measured and both fixed in the prototype:

- `u64` renders as `bigint` while the JSON wire carries a number, so `chat.js:661` becomes
  a false error. `#[cfg_attr(test, ts(type = "number | null"))]` on the two `Option<u64>`
  fields settles it.
- The generator must end the file with **exactly one newline**. ts-rs leaves a trailing
  blank line, `end-of-file-fixer` trims it on commit, and the next `cargo test` then fails
  against a fixture the lint itself rewrote. Any generated file this repository commits
  has the same constraint.

Commands are not generated. `tauri-boundary.d.ts` hand-writes the 8 event names and 15
command names and types `window.__TAURI__`, which is what removes the 10 `__TAURI__`
errors. `tauri-specta` would generate that half too, but it is a runtime dependency and a
registration step, and 15 lines did not earn it. Not prototyped.

## Not verified

- The `loginSaid` crash in the running app (read from module semantics and git history;
  the fix shipped in #738).
- `ts-rs` against the real `src-tauri` crate. The prototype copies the shapes into a
  standalone crate to avoid a cold Tauri build.
- `tauri-specta`.
- `npx` availability on the Windows runner, if a checker ever became a pre-commit hook.
- Whether a bundler can preserve the vendored `marked` bytes. Section 4 assumes it cannot.

## Files in this directory

- `tsconfig.option1.json`, `tsconfig.option1-loose.json` — option 1, strict and loose.
- `tsc.option1.strict.txt` (337), `tsc.option1.loose.txt` (30) — the original runs, made
  before #738 fixed `loginSaid`. Against `main` at `32a8d28d` the loose run is 29: that
  one line is gone and the other 29 are unchanged. Named `.txt` because the root
  `.gitignore` skips `*.log`.
- `marked.esm.d.ts.stub` — copy to `src/vendor/marked.esm.d.ts` to reproduce. Without it
  `tsc` follows the import into the vendored file and reports 273 errors there;
  `exclude` does not stop it, but a sibling `.d.ts` does.
- `tauri-boundary.d.ts`, `tsconfig.option2.json`, `tsc.option2.txt` (20) — option 2.
- `probe/misread.js`, `tsconfig.probe.json` — six deliberate misreads, six errors.
- `ts-rs-proto/` — the generator. `REFRESH_BINDINGS=1 cargo test` to regenerate.

## How to reproduce

```sh
# The type check against current main (expect 29)
cp spike/typescript-692/marked.esm.d.ts.stub src/vendor/marked.esm.d.ts
npx -y -p typescript@5 tsc --noEmit -p spike/typescript-692/tsconfig.option1-loose.json
rm src/vendor/marked.esm.d.ts

# The generator and its stale check (16 tests)
cd spike/typescript-692/ts-rs-proto && cargo test

# The compile-time coupling: set frontendDist to a missing path, then
cargo check -p fidget      # proc macro panicked, generate_context!

# The bug census
git log --no-merges --pretty='%h|%s' -- 'src/*.js' | grep -iE '\|fix'
```
