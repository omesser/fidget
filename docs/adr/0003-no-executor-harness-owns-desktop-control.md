# Fidget ships no Executor; the Harness owns desktop control

fidget does not post synthetic mouse or keyboard events. It ships an MCP server
exposing fidget-side tools — play a Behavior, list windows, describe the screen,
read and write Memory — and attaches a Harness the user supplies. Clicking is the
Harness's job; the character is ours. A future reader will find an app about operating
your computer that deliberately cannot operate your computer, so the reasoning matters.

At the API level, Anthropic's
[computer use tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool)
is reasoning only: the model returns actions and the client executes them. The
[reference implementation](https://github.com/anthropics/anthropic-quickstarts/blob/main/computer-use-demo/README.md)
drives X11 with `xdotool` inside Docker. At the product level this changed on
[23–24 March 2026](https://claude.com/blog/dispatch-and-computer-use): Claude Code and
Claude Cowork perform computer use natively on macOS, with Windows following about ten
days later. The Harness now genuinely brings its own executor, so we do not write one.

## Consequences

It is not portable across Harnesses. Other vendors follow the API pattern — actions out,
client executes — so "BYO Harness" does not imply "any Harness can drive the desktop." A
Harness without an executor can still chat and sense.

Action permissions belong to the Harness, which runs its own consent dialogs. fidget
owns consent for sensing only and must not duplicate them; two dialogs for one click
teaches users to click through both.

A `CGEvent` executor stays on the shelf as the answer if the subscription gate proves
fatal. It is not built on spec.
