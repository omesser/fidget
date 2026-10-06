# Scenarios

End-to-end scenarios that drive the real app and assert on what it shows. Each
one launches Fidget, so each run is a GUI takeover. Read
[`docs/agents/gui-takeover.md`](../../docs/agents/gui-takeover.md) before you
run one: set up, post the prompt, and wait for a go-ahead for that run.

## Run one

Print the scenario's header. That is setup, not the takeover:

```sh
cargo run -p fidget-verify -- scenario thinking-row
```

Post the header as the takeover prompt. Once you have the go-ahead:

```sh
cargo run -p fidget-verify -- scenario thinking-row --go
```

`--go` builds `target/debug/fidget`, and the test binary when the scenario's
`Usage` line names one, then runs the script with them. The test binary's
`harness::tests::fake_acp_agent` is the fixture Harness. Arguments after `--`
go to the script after the binaries, as in `scenario hero-gif --go --
--harness claude`. The exit code is `fidget-verify`'s: 0 passed, 1 failed, 2
skipped or printed the header, 3 a tool error.

`chat-header-narrow.sh` takes the same two binaries. It resizes Chat to 420,
360 and 320 points and checks that the header keeps one row and never scrolls
sideways.

`chat-level.sh` takes the same two binaries. It checks that Chat floats at
layer 25 only while focused, and that minimizing Chat, focused or not, logs the
change that lets the quick-message pill back.

`question-bubble.sh` takes the same two binaries. Its fixture asks a permission
question on the first turn, then the scenario pokes the sprite at the position
the frame trace logs. It checks that no question cue shows before the Poke, and
that after it the bubble reads "Question for you in the" with a `chat` link
button.

`poke-mid-climb.sh` takes only the app binary and runs no Harness. It throws
the sprite at a display's side edge with a real drag
(`scripts/scenarios/throw-sprite.swift`), then clicks it until one click lands
as a Poke while it climbs. From the frame trace, it checks that the Poke
starts `react` over, that the sprite stays Climbing at one position in one
climb frame for the first 2.3 s of the 2.5 s cooldown, and that it climbs on
by 3.5 s. Set `FIDGET_SCENARIO_TRACE` to a saved app log to run only the
check, as `fixtures/poke-mid-climb-trace.txt` does in `crates/verify`.

`launcher-dies-at-startup.sh` takes the same two binaries. Its fixture aborts
on the first launch, the way `npx` does over a broken Node. It opens Chat from
the menu bar icon's Chat… row, checks that at 420 and 320 points the Harness
error landing's boxes end inside the window and Error output starts above the
composer, then presses Codex and checks that the Harness launches again at
once. The landing's copy and the capture are unit-tested, not checked here.
The menu bar icon takes one real click; Chat… and Codex go through AXPress,
with a click at the control's centre only if AXPress is refused.

`landing-link-click.sh` takes only the app binary: it picks Codex with no
`npx` on `PATH`, so no Harness runs. It opens Chat from the menu bar icon,
checks that the "Codex needs `npx`" landing draws `npx` and the install link as
their own elements, then clicks the link. A recording `open` on `PATH` takes
the hand-off, so the check is that exactly `https://nodejs.org/` reached it and
that Chat still shows the landing. No browser opens.

`hero-gif.sh` takes the same two binaries and records the display for 45 s
while you throw, poke, double-click and chat with the Character for the README
hero. Its `--crop` pass re-encodes the saved recording to an MP4 beside it,
or an animated WebP with `--webp`, and launches nothing. The README embeds the
MP4 as a `user-attachments` video. `--harness claude` runs the real
Claude Code adapter: the private `HOME` links your `~/.claude`,
`~/.claude.json`, `~/.npm` and `~/Library/Keychains`; Fidget's settings and
memory still land under the private `HOME`. Keep Chat open from the
double-click on: the first-run gesture tour bubble fires 25 s after launch
unless Chat is open.

## Windows and X11

`thinking-row`, `chat-header-narrow`, `landing-link-click`,
`launcher-dies-at-startup`, `sign-in-button`, `question-bubble` and
`control-click-menu` each have `.x11.sh` and `.win.ps1` leaves.
`fidget-verify scenario <name>` prints the leaf for this host. With `--go` it
runs that leaf, or skips when this host has none.

These stay macOS only on purpose:

- `chat-level` asserts Chat's macOS window level. Windows has only a topmost
  flag, and X11 leaves stacking to the window manager. Its minimize check could
  still get its own leaf.
- `hero-gif` records the README video, and the README needs one recording, not
  one per OS.
- `codex-sign-in-link` and `antigravity-sign-in` check a sign-in flow that is
  the Harness's own and the same on every OS. A port would need a person at
  the browser on each OS to recheck only how Chat draws it.

X11 reads the Chat window through AT-SPI (`scripts/ax-window-linux.py`) and
needs `python3-pyatspi` plus `DISPLAY`. Windows reads it through UI Automation
(`scripts/ax-window-win.ps1`). Dump lines are `role|name`. Pass `frames` to
append `|x,y,w,h`. `size` resizes the window and prints `x,y,w,h`.

`thinking-row` asserts the same open and collapsed Thinking-row shapes as
macOS. `chat-header-narrow` resizes Chat to 420, 360 and 320 and asserts one
header row with no sideways scroll.

`landing-link-click`, `launcher-dies-at-startup` and `sign-in-button` open
Chat from the tray's Chat… row. X11 clicks it over the tray's dbusmenu
(`ax-window-linux.py tray`), so it needs a StatusNotifierWatcher and skips
without one. Windows clicks the taskbar icon and invokes the row
(`ax-window-win.ps1 tray`), so the icon must not sit in the hidden-icons
overflow. On X11 a recording `xdg-open` on `PATH` takes the URL, as `open`
does on macOS. Windows hands it to `ShellExecuteW`, so its leaves do not
record the URL and a real browser opens.

`question-bubble` and `control-click-menu` find the sprite from the frame
trace, as on macOS. `question-bubble` pokes it with a real click and dumps the
overlay window, titled `Fidget`. X11 and Windows have no Control-click, so
`control-click-menu` right-clicks the sprite instead. `ax-window-linux.py menu`
and `ax-window-win.ps1 menu` send that click, print the open menu's items and
press Escape.

To check an assertion without a desktop, point the leaf at fixture dumps and
skip the launch:

```sh
FIDGET_SCENARIO_AX_OPEN=scripts/scenarios/fixtures/thinking-row-open.txt \
FIDGET_SCENARIO_AX_DONE=scripts/scenarios/fixtures/thinking-row-done.txt \
scripts/scenarios/thinking-row.x11.sh --go /bin/true /bin/true
```

```sh
FIDGET_SCENARIO_AX_420=scripts/scenarios/fixtures/chat-header-narrow-420.txt \
FIDGET_SCENARIO_AX_360=scripts/scenarios/fixtures/chat-header-narrow-360.txt \
FIDGET_SCENARIO_AX_320=scripts/scenarios/fixtures/chat-header-narrow-320.txt \
scripts/scenarios/chat-header-narrow.x11.sh --go /bin/true /bin/true
```

Those runs do not prove the live window. A live `--go` still needs the
go-ahead in `docs/agents/gui-takeover.md`.

## codex-sign-in-link

`codex-sign-in-link.sh` checks the sign-in link against the real codex-acp,
because the URL elicitation under test is codex-acp's own. It needs a ChatGPT
account, the network, and you at the keyboard: the terminal prompts each click.
It takes only the app binary:

```sh
cargo run -p fidget-verify -- scenario codex-sign-in-link --go
```

codex runs against a fresh `CODEX_HOME` with file credential storage, so your
`~/.codex` and your keychain stay untouched. The run deletes that directory on
exit, and the tokens with it.

## antigravity-sign-in

`antigravity-sign-in.sh` checks Chat's Google sign-in against Google's real ACP
server, because the browser flow under test is the server's own. It needs a
Google account, the network, and you at the keyboard. It takes the app binary
and the folder you unzipped the `antigravity-acp` registry archive into:

```sh
cargo run -p fidget-verify -- scenario antigravity-sign-in --go -- ~/agy-acp
```

The server runs against a fresh `GEMINI_HOME`, so your `~/.gemini` stays
untouched. The run deletes that directory on exit, and the tokens with it.
After the sign-in it quits Fidget and runs `--probe-harness` twice, fresh and
resumed, against the same login.

## The header

The comment block at the top of each scenario, one field per line:

| Field | Says |
|---|---|
| `Scenario` | Name and platform |
| `On screen` | What appears, what takes focus, what gets captured |
| `Input` | Clicks or keys sent, or `none` |
| `Duration` | Expected time, and the maximum |
| `Grants` | macOS permissions the terminal needs |
| `Asserts` | What makes the run fail |

## Rules

- Isolated `HOME` under the evidence directory, so a run never reads or writes
  the owner's settings.
- A fixture Harness from `src-tauri/src/harness.rs`, never a real one and never
  a stub agent script. The two sign-in scenarios above and `hero-gif.sh
  --harness claude` are the exceptions.
- Kill what the run started on exit, and nothing else.
- Assert and exit non-zero. A count printed for someone to read is not a check.

## Evidence

Each run writes to `$TMPDIR/fidget-scenario-<name>-<timestamp>/`, outside the
repository: the app log, the fixture Harness log, screenshots, and the
Accessibility dump each assertion read. The last line of output names the
directory.
