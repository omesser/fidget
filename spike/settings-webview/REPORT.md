# Spike: should the Settings window be a webview?

Worktree `spike-settings-webview`, detached at `09be8958`. Analysis and a
static prototype; no production code changed, no PR.

## Recommendation

**Do it, incrementally, under the conditions in the last section.**

The native call was right when it was made: there was no JS surface in the
tree. ADR-0010 then chose a webview *for chat* "because Settings already priced
the alternative: 1046 and 913 lines of hand-written AppKit and GTK for a form
of checkboxes, and the Windows third is still unwritten". Those two files are
now 2036 and 1559 lines and the Windows third is 2896. The premise ADR-0010
recorded has only got stronger, and the chat surface it produced is the
comparison the owner is now making.

## What is there today (verified in the tree)

| File | Lines | Of which tests |
| --- | ---: | ---: |
| `src-tauri/src/platform/macos/settings_window.rs` | 2036 | 148 |
| `src-tauri/src/platform/windows/settings_window.rs` | 2896 | ~270 |
| `src-tauri/src/platform/x11/settings_window.rs` | 1559 | 0 |
| **three renderers** | **6491** | **~420** |
| `src-tauri/src/settings/form.rs` (form as data) | 2735 | 1373 |
| `src-tauri/src/settings.rs` (view, patch, session, file) | 3877 | 2291 |
| `src-tauri/src/settings/move_drag.rs` | 40 | 22 |
| `scripts/verify-settings-macos.sh` + `scripts/ax-settings.swift` | 615 | AX tree, macOS only |
| `scripts/verify-settings-win.ps1` | 419 | Win32 `FindWindow`/`SendMessage`, **not** UIAutomation |
| Linux verify script | 0 | #630 asks for one |

Growth since ADR-0010 priced it: 1959 → 6491 renderer lines (3.3×).
History: 39 commits touch the renderers, 11 of them `fix(`; 55 touch
`form.rs`/`settings.rs`, 18 of them `fix(`.

The chat window is already a second webview: `main.rs:829` `build_chat` is a
plain `WebviewWindowBuilder` with a title and size, `capabilities/chat.json`
grants it `core:event:allow-listen/unlisten` and nothing else, and it talks to
Rust with `invoke` and `listen` (`src/chat.js:20-23`). A Settings webview is
the same shape with a third capability file.

## The breakage record, classified

Rule: a **renderer** bug is one that exists in a drawing of the form and would
not exist with one interpreter; a **model** bug is in `form.rs`/`settings.rs`
and would exist with any number of renderers.

| Issue | Platform | Class | Why |
| --- | --- | --- | --- |
| #601 clips bottom rows, no scroll, bars bleed | Win32 | renderer | fixed-height layout, no scroll container |
| #674 `Popup` drawn as radio group | GTK | renderer | one renderer mapped Popup differently from the other two |
| #552 section heading/comment never drawn | Win32 | renderer | one renderer skipped a field the other two drew |
| #399 remaining row types | Win32 | renderer | a renderer that did not yet interpret the whole form |
| #400 layout polish + e2e | Win32 | renderer | |
| #642 stray `W` on every help line | AppKit | renderer | `NSButton` with `NSBezelStyle::Disclosure` stretched to field width |
| #625 rows never re-freeze after source change | all three | renderer | `frozen` read at build, never at `draw()`; each renderer fixed separately (#629/#630/#631 checklists) |
| #634 Off pick does not leave the popup | AppKit | unclassified | closed 2026-09-14 with no comment and no commit referencing it; #593 renamed the title in between, so it may have been the verify script's title |
| #629, #630, #631 Settings e2e + fill renderer gaps | all three | cost of three | verification epics, one per renderer |
| #667 verify disclosure against #642 modes | Win32 | cost of three | parity check for a bug another renderer had |

**Count: 7 of 7 classifiable bugs are renderer bugs; 0 are model bugs.** The
model has bugs too (18 `fix(` commits) but each was fixed once. Of the 11
`fix(` commits touching renderers: 8 renderer-only (#660, #553, #492, #464,
#422, #245, #240, #672's renderer half), 1 in controller logic that is
triplicated (#534), 2 model (#538, #275).

## Is `form.rs` as platform-independent as it looks?

Mostly yes, with one real caveat.

**Yes:** `form::describe()` is pure data — `FormDescription { tabs, operations }`,
`FormTab`, `FormSection`, nine `FormRow` variants and three `CompositeControl`
variants — and every renderer consumes it through the same `describe()` call.
It reads live process state (`harness::driving()`, env vars) so it is rebuilt
per draw; that is exactly what a `settings_snapshot` command would return.
`Popup.options` empty means "renderer fills from `SettingsView.installed`",
which is a reach into the view, not into a platform.

**The caveat:** every renderer imports
`crate::settings::{DirectorDraft, SettingsPatch, SettingsSession, SettingsView}`
and carries its own controller — `director_draft`, `director_staged`,
`do_apply`, `do_cancel`, `do_clear_key`, `fill_shortcut`,
`handle_combobox_change`, `handle_text_change`, `rebuild_instances_list`,
`confirm_wipe`. `form.rs` describes **drawing**; **committing** is written
three times. #534 (untouched Director tab wiped Base URL and Model) was a bug
in that triplicated controller. `settings.rs:2627` even has a test that greps
the three renderer sources for `draw(true)` because none of them can be
constructed in a test. So the honest framing is not "add one interpreter,
delete three" but "add one interpreter **and pull the controller into one
Rust `settings_event` command**, then delete three" — that command is where
the 250–350 new Rust lines below go, and it is the part that becomes testable
for the first time.

## Line delta (estimate)

Out: 6491 renderer + ~40 `platform.rs`/`mod.rs` glue + 1034 scripts + the
source-grep test (~40) ≈ **7600**.

In: `settings.html` ~50, `settings.js` ~350 (the prototype's 155 covers 6 of 9
row types; add `Multiline`, `List` with dismiss, `InspectPath`, tab switching,
focus-refresh, drag), `settings.css` ~250, `capabilities/settings.json` ~15;
Rust: `Serialize` derives ~10, `settings_snapshot` + `settings_event` commands
folding events into `SettingsPatch`/operations ~250–350, window build ~40, a
macOS `raise` ~15; `node --test` ~200. ≈ **1250**.

**Net ≈ −5500 to −6300 lines**, and platform-`cfg`'d Settings code goes from
~6100 lines to ~15 (the raise-above-overlay call, see below).

The range rather than a single figure: the three renderers have diverged more
than a line count shows, so some of what looks duplicated is not, and deleting
it saves less than the arithmetic suggests. Step 1 of the plan measures the
real controller and closes the range.

## Verification

`scripts/ax-settings.swift` exists, in its own words, because "the Settings
window is native AppKit, so the only way to check it from outside was a
screenshot". Every assertion `verify-settings-macos.sh` makes is a string or
boolean off an AX dump: section order, labels, whether the control after a
label is live. The prototype's `controls(tab, view)` returns that same flat
list from the description, and `settings.test.js` makes those assertions in
`node --test` in 45 lines (4 tests, passing). The Windows script asserts less
(window opens, tab index, labels survive a tab switch) and does it with raw
Win32 messages. All 1034 lines become ~200 lines of node tests plus **one**
live smoke per platform (open, dump, pick) that CI still does not run.

## Prototype

`spike/settings-webview/`: `index.html`, `settings.js`, `settings.css`,
`ai-tab.js` (hand-written snapshot of the AI tab in two states),
`settings.test.js`, `shot-modelApi.png`, `shot-harnessDriving.png`.

Run: `node --test spike/settings-webview/`. View: serve the worktree root over
HTTP (Chrome refuses ES modules from `file://`) and open
`spike/settings-webview/index.html?state=harnessDriving`. Screenshots were
taken with headless **Google Chrome (Blink)** at 520×1480, not WKWebView; a
Tauri build was out of scope. Same CSS engine family as WebView2, close to
WebKit for everything used here.

What it taught that reading did not:

1. **It looks like the chat surface at first try:** `chat-ui.css` linked
   unchanged, `settings.css` reads only `var(--chat-*)` tokens (it would pass
   `tests/chat-ui-tokens.test.js`'s rule as written). No literal colour, radius
   or font. Theme consistency is free, and a second `.chat-ui-*` design
   recolours Settings too.
2. **The class namespace is shared, and it bit:** `.frozen` in `chat-ui.css`
   is the Prompt tab's read-only prose box; my `.row.frozen` picked up its
   border and padding. Renamed to `.is-frozen`. A real `settings.css` should
   either import only the token block or prefix its classes.
3. **Frozen and status are one attribute each:** `readonly`, `disabled` and a
   `<p class="status">` — the #625 class of bug cannot exist because a redraw
   is `render()` again from a fresh description; there is no separate
   `apply_enabled_states` pass to forget.
4. **`<details>` is the disclosure #642/#660 were hand-building:** Closed
   reserves no space, the browser owns the toggle, a reader announces it.
5. **`<select>` is the #674 answer:** One dropdown look on three OSes from CSS;
   on macOS WKWebView the opened menu is still AppKit's.
6. **Two "null" bugs in 15 minutes** — `append(null)` prints "null", and the
   snapshot's view keys did not match row ids. Both were visible in a
   screenshot and fixed in one line each. That is the feedback loop the native
   renderers do not have (#629: "screenshots as the only oracle").

## Against — investigated

**What a native window does that a webview cannot.** Settings has no menus.
The title bar comes from Tauri's decorated window, as the chat's does.
Modality is one dialog (Wipe memory: `NSAlert` / `gtk::MessageDialog` /
`MessageBox`) — an in-page `<dialog>` or the existing `confirm()` covers it
without a new plugin. Alt-drag anywhere (#460, `move_drag.rs`) is
`getCurrentWindow().startDragging()` under `core:window:allow-start-dragging`.
Become-key refresh (`windowDidBecomeKey → refresh()`, which re-reads TCC
grants) is a `focus` listener that re-invokes the snapshot. "Hidden, not
rebuilt" (#245) can become "destroyed and rebuilt": the state is refetched
anyway and dropping a typed key on close is the *wanted* behaviour (#279).

**The one thing that stays native:** `raise()` at
`macos/settings_window.rs:1770` puts Settings at `NSStatusWindowLevel` and
calls `activateIgnoringOtherApps`, because the overlay is an `NSPanel` at
`NSFloatingWindowLevel` and a normal window falls behind it. Tauri's
`always_on_top(true)` is *also* floating level, so order among them is not
guaranteed. Keep ~15 lines of macOS `raise` on `window.ns_window()`; the
Windows/X11 overlay is topmost too and needs the same check live.

**Accessibility.** Today: AppKit gives a real AX tree (the verify script proves
it); Win32 common controls give MSAA/UIA for free; GTK3 gives AT-SPI. A
webview: WKWebView exposes the DOM as `AXWebArea` children with proper roles
when the HTML is semantic; WebView2 exposes UIA through Chromium; WebKitGTK
exposes AT-SPI. The prototype uses only `<input>`, `<select>`, `<button>`,
`<details>`, `<label for>` and `role="tab"`, so roles and names exist. **What
changes:** a VoiceOver user navigates a web area (rotor, VO+arrow inside a
group) rather than a native window's controls — a different modality, not a
missing one. `ax-settings.swift dump` would need to descend into the web area;
the AX approach still works. **Not verified live** in a Tauri build; the
condition below covers it. Password fields: WebKit may offer autofill /
"strong password" on `<input type=password>`; `autocomplete="new-password"`
needs a live check.

**Platform integration points.** TCC/consent (`SettingsSession::enable_consent`
→ `consent::enable`), Keychain (`SecretStore` in `settings.rs`), open Memory
file (`open_memory`), wipe (`wipe_memory`), spawn/dismiss, Harness attach — all
are already `SettingsSession` methods in Rust; none live in a renderer. They
become the body of `settings_event` and stay where they are.

One exception, which arrived after this section was first written: **PR #599's
clipboard write is renderer code, three times over** — `NSPasteboard`,
`SetClipboardData`/`CF_TEXT`, and `gtk::Clipboard`, none of them in
`SettingsSession`. In a webview it is one `navigator.clipboard.writeText()`.
That is the only platform integration that moves to JS, and it moves *because*
the renderers each hand-rolled it. It is a fourth triplication, landing while
this spike was being written, and it argues for the change rather than against
it.

**Look and feel on macOS.** This is the real trade. Native today: Aqua
controls, system font, follows light/dark, a standard `NSTabView` — the System
Settings idiom, apart from the bugs. The webview is the chat's dark theme
(`.chat-ui-minimal` sets `color-scheme: dark`) and does not follow the OS
light mode, because the chat already made that choice. Consistent with the
product's own window, inconsistent with the desktop's preferences windows. On
Windows the native window is worse than the prototype today; on Linux the GTK
window is fine but lonely. The owner's stated comparison is against the chat
surface; by that bar the prototype wins. By the "looks like a Mac app" bar it
does not, and nothing in this spike changes that.

**Migration.** `platform::show_settings(session)` is one seam, called from
`main.rs:593`. Incremental path: land the webview complete (all five tabs; a
single tab cannot be split off because tabs share one window) behind a
Development switch or env var, native remains default; verify on three
platforms; flip the default; delete three renderers and three scripts in one
PR. Two implementations alive costs one thing: any form change in that window
lands twice. Keep the window short and freeze native to bug fixes only.

## Conditions and risks I would not accept

1. **Do not start until the controller is pulled into Rust:** Triplicated
   commit logic (`director_draft`, `fill_shortcut`, apply/cancel) becomes one
   `settings_event`; otherwise the webview becomes a fourth copy.
2. **Do not delete the scripts before** `node --test` covers the flat control
   list for every tab in both AI-source states **and** one live open/dump/pick
   smoke per platform exists — the macOS one by extending `ax-settings.swift`
   into the web area.
3. **Keyboard-only operability** is checked before the flag flips: tab order,
   Escape closes, Enter applies, focus visible on every control.
4. **Raise-above-overlay** is verified live on all three platforms; the macOS
   `raise` stays.
5. **Semantic HTML only:** No div-buttons, no custom dropdown widgets; the
   `<select>` stays a `<select>`. This is the accessibility guarantee.
6. **No build step, no bundler, no dependency** (ADR-0010's stance; TypeScript
   is #692 and separate).
7. **Namespace the CSS** or import only tokens — the `.frozen` collision is a
   preview of a class of quiet bugs.
8. The typed API key travels JS → `invoke` → Keychain and is **never logged**
   in the event log the Development tab may grow.

## Not verified

- Anything in a real Tauri/WKWebView/WebView2/WebKitGTK window: AX exposure,
  `<select>` popup behaviour, password autofill, z-order against the overlay.
- Windows and Linux rendering of the prototype (macOS host; the GTK lane does
  not compile here).
- #634's root cause.
