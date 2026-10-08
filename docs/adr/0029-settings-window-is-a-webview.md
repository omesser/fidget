# Settings is one webview on the Chat surface's tokens

**Status:** Accepted.

## Context

Settings was three native renderers of one form description: AppKit, Win32 and
GTK 3. No ADR recorded that choice. It was made when the tree had no JavaScript
surface at all, and native was the only credible option then.

The Chat surface then chose a webview (ADR-0018), partly because Settings had
already priced the native alternative. The native renderers kept growing, and
each needed its own verify scripts, because a native window cannot be asserted
on from `node --test`.

Every classifiable Settings bug over two months was a renderer bug, and none was
in the form model. #625 had to be fixed in each renderer. Verification was one
epic per renderer (#629, #630, #631).

The form model described drawing but not committing. Each renderer carried its
own copy of the controller (draft, staged, apply, cancel, shortcut fill), and
#534 was a bug in one copy.

The Chat surface was already a webview, themed by the Chat UI token seam
([ADR-0036](./0036-chat-ui-is-an-app-wide-design-over-one-token-seam.md)),
tested with `node --test`, and the window the product's others are judged
against.

## Decision

Settings is a webview window on the same stack as Chat: raw ES modules, no
bundler, and only the Chat UI's tokens for colour, type and shape.

The form stays data. The Shell serialises the form description and the
settings a tab reads to the page, on open and on focus. The page renders from
that snapshot and nothing else, and every redraw is a full render, so a frozen
row cannot be read at build and forgotten at draw.

Committing moves out of the renderers into Rust, once. The page sends row
events by id, and Rust folds them into the settings patch and session. Consent
prompts, the Keychain, opening and wiping Memory, spawn and dismiss stay in
Rust. None of them reaches JavaScript.

The page uses semantic HTML only, so the platform webview exposes it to
Accessibility. The same flat list of controls the native AX dump read is
produced in JavaScript, so the AX assertions become node tests.

The three native renderers and their verify scripts are deleted.

## Considered Options

- **Keep three native renderers and fix them:** The bugs are fixable one at a
  time. Rejected: fixes and verification are per renderer, and each new row
  type lands three times. The cost is not one bug, it is the multiplier.
- **Native on macOS, webview elsewhere:** Keeps Aqua where the owner lives and
  fixes Windows where it is worst. Rejected: it keeps the multiplier at two and
  makes one product look like two across machines.
- **A webview in a native shell (native tab bar, web panes):** Rejected: the
  tab bar is the cheapest part to draw, and the join is another platform seam.

## Consequences

- One interpreter, one controller, one place a display bug can be.
  Platform-specific Settings code shrinks to the macOS call that raises the
  window above the overlay panel.
- Settings looks like Chat, because it reads the same tokens. On macOS it
  stops looking like a System Settings pane. That is accepted.
- Accessibility changes modality, a web area rather than native controls, but
  not presence, provided the HTML stays semantic.
- The Chat surface's stance on the stack holds: no build step, no dependency.
