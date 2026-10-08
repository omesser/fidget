# Capture drop and the harness computer-use path

Research for fidget Capture architecture, dated 2026-09-22. Anchor: decision
context provided by Oded (Architect).

**Decision:** DROP pixel Capture (both Ambient and On-Demand); KEEP Free sensing
(OS metadata: frontmost app, window geometry, idle). Recommend **cua-driver** as
the docs-primary multi-harness computer-use path when users need desktop control
through a harness. Runner-up: minghinmatthewlam/computer-use-mcp (macOS-only).

Capture never shipped. It was deferred in ADR-0005. Free sensing is implemented
and ships in v1. Computer use for agents belongs outside fidget: in
harness-native capabilities or multi-harness MCP drivers. Capturable (sprite
visible in screen shares) is out of scope for this note.

This note documents the decision path and the multi-harness options so
future work has a starting point when users ask for agent desktop control.

## What Capture versus Free sensing means

**Fact.** ADR-0005 defined three tiers of sensing: Free (OS metadata like
frontmost app and window geometry, no consent), On-Demand Capture (single
screenshot in response to user act, per-act consent), and Ambient Capture
(cadence-bounded asking with mandatory local gate, explicit consent). ADR-0005
deferred Capture entirely, both tiers, while allowing Free sensing to proceed.

**Fact.** fidget ships Free sensing: `list_windows` and `describe_screen`
query window metadata from the OS without touching pixels. CONTEXT.md defines
Free sensing as "OS metadata: frontmost app, window geometry, idle, etc."

**Fact.** ADR-0005's "ask Harness for screenshot" hook is unwired. As of
2026-09-16, the ACP attach has no standard desktop-control capability
(`docs/research/harness-tools-under-acp-probe.md`), so even a consented Capture
would have no path to an attached harness unless the user connects a separate
MCP server for it.

**Inference.** Dropping Capture means fidget never embeds, ships, or owns
screenshot capability. It does not mean agents the user runs can never see
pixels. A user running a CU-capable harness or attaching a computer-use MCP
server can still grant that agent desktop control, just not through fidget's
own code.

## Harness computer-use options

The table below summarizes harness-native computer use and MCP-based paths
available as of 2026-09-22. "Harness-native" means the harness itself provides
desktop tools in its own runtime, without needing an MCP server. "MCP" means the
capability is delivered as an MCP server the user installs.

| Harness / Driver | Computer Use | Source |
|---|---|---|
| Claude Code (interactive) | **harness-native** on macOS, Pro/Max only, requires interactive session | Anthropic docs |
| Claude Code (ACP) | **no**, gated by interactive-session requirement; ACP uses SDK print mode | `harness-tools-under-acp-probe.md` |
| Codex | **harness-native**. Computer Use plugin; host owns capture (macOS/Windows) | https://developers.openai.com/codex/computer-use |
| Cursor Cloud Agents | **harness-native**, computer use in isolated cloud VM | Cursor docs |
| Cursor Local Agents | **MCP attach**, no native CU; user attaches MCP server | Cursor MCP docs |
| Hermes | **harness-native**. `computer_use` toolset routes to cua-driver (MCP stdio) | https://hermes-agent.nousresearch.com/docs/user-guide/features/computer-use |
| OpenCode | **MCP attach**, no native desktop tool listed | OpenCode tools docs |
| ACP spec itself | **no** standard `computer_use` capability defined | agentclientprotocol.com |

**Fact.** No ACP-standard client capability exists for computer use as of ACP
v1. The protocol defines `fs.*`, `terminal.*`, and `elicitation.*`, but no
desktop-control surface. Each harness decides its own path.

**Fact.** Cursor Cloud Agents have harness-native computer use (each agent runs
in an isolated VM with full desktop environment). Cursor local agents have no
native CU and rely on MCP attachment. Codex has harness-native Computer Use as a
plugin (macOS/Windows).

**Fact.** Hermes `computer_use` toolset routes to cua-driver over MCP stdio.
This is a first-party integration where Hermes installs and manages cua-driver
as the backend for its computer_use tools. Note: Hermes ACP mode uses a curated
toolset that may omit computer_use depending on session configuration.

**Inference.** Since no portable ACP computer-use capability exists, the only
multi-harness path for desktop control is an MCP driver the user attaches to
whichever harness they run. That is the gap cua-driver and the
community MCPs fill. Hermes uniquely integrates cua-driver as a first-party
backend.

## MCP driver comparison

Three MCP servers provide Anthropic-style computer-use tools (mouse, keyboard,
screenshots) that work across multiple harnesses. Comparison as of 2026-09-22.

### cua-driver (primary recommendation)

**Fact.** **License:** MIT. **Repository:** https://github.com/trycua/cua
**Platforms:** macOS, Windows, Linux. **Invocation:** `cua-driver mcp` (stdio
transport).

**Fact.** **First-party integration docs** exist for Claude Code, Codex,
Cursor, and OpenCode: https://cua.ai/docs/how-to-guides/driver/connect-your-agent

**Fact.** **Hermes integration:** Hermes' built-in `computer_use` toolset routes
to cua-driver over MCP stdio. Hermes pre-installs cua-driver and manages its
lifecycle. This is a first-party integration:
https://hermes-agent.nousresearch.com/docs/user-guide/features/computer-use

**Fact.** **Accessibility APIs first, screenshots optional:** Uses macOS
Accessibility (AX), Windows UI Automation (UIA), and Linux AT-SPI. Screenshots
are an optional fallback, not the primary interaction mode. Supports permission
modes.

**Fact.** **MCP tools reference:**
https://cua.ai/docs/reference/cua-driver/mcp-tools documents the exposed
computer-use tools.

**Inference.** cua-driver is the broadest multi-harness, multi-OS solution with
first-party docs and Hermes-native integration. Its accessibility-first approach
aligns with lower-overhead desktop control. This is the docs-primary
recommendation.

### domdomegg/computer-use-mcp

**Fact.** **License:** MIT. **Repository:**
https://github.com/domdomegg/computer-use-mcp **Platforms:** OS matrix not
formally documented in README; nut.js dependency suggests cross-platform intent.

**Fact.** **Anthropic-style tools:** Provides `computer` tool matching
Anthropic's interface. Documented for Claude Code, Claude Desktop, Cursor via
npx install-mcp.

**Fact.** **Pixel-centric:** Built on nut.js for mouse/keyboard and screenshot
APIs. Relies on pixel coordinates and screenshots as primary interaction.

**Fact.** **Security note in README:** "Prompt injection: Be wary of prompt
injection attacks." Warns that untrusted content on screen can manipulate agent
actions.

**Inference.** domdomegg/computer-use-mcp is a pixel-first community MCP with
install-mcp convenience. No Hermes first-party integration documented. The
prompt-injection warning is honest but not unique to this driver.

### minghinmatthewlam/computer-use-mcp (runner-up)

**Fact.** **License:** MIT. **Repository:**
https://github.com/minghinmatthewlam/computer-use-mcp **Platforms:** macOS 14+
only. **Accessibility-first:** Uses macOS Accessibility APIs rather than
pixel-centric control.

**Fact.** **Documentation:** Includes connection examples for Claude Code,
Cursor, Codex, and Gemini. Pre-1.0 maturity (version numbers suggest active
development).

**Fact.** **No Hermes-native integration:** Not documented as a first-party
Hermes option.

**Inference.** minghinmatthewlam/computer-use-mcp is macOS-only with
accessibility-first design, making it a strong runner-up for macOS users who
prefer AX over pixels. Its single-platform focus limits broader recommendation.

### Optional footnote: zavora-ai/computer-use-mcp

**Fact.** https://github.com/zavora-ai/computer-use-mcp is another community
MCP. Not selected as primary or runner-up for this comparison. Noted for
completeness.

## User-facing meaning

**The pet keeps its surface interactions:** Perch, fade, window geometry sensing,
idle detection, list_windows, and describe_screen all remain. The sprite still
reacts to the desktop and obeys physics.

**No character content-awareness of pixels:** fidget never analyzes screen content,
never takes screenshots itself, and never embeds OCR or vision models for desktop
sensing. The Local Gate (ADR-0005) is unneeded because nothing asks for Capture.

**Agent CU is still available:** When a user runs a CU-capable harness (e.g.
Cursor Cloud Agent, Codex with Computer Use plugin, Hermes with computer_use
toolset, interactive Claude Code on macOS) or attaches an MCP server like
cua-driver, that agent has desktop control.
The control comes from the harness or MCP server the user chose, not from
fidget.

**Tradeoff:** The character itself never "looks at the screen" in the content sense.
A future where the pet visually reacts to what is on the desktop (noticing a
specific app's content, reading notifications) is not this path. That would
require Capture, which this decision drops.

**Inference.** Users who want agent desktop control attach cua-driver (or
another MCP) to their harness of choice. Users who want only the animated
sprite, sensing, and chat attach no MCP. The decision separates fidget's
role (spatial layer, personality, perch) from the harness's role (functional
layer, execution).

## Do not claim

This note documents a decision and the options above. It **does not** claim:

1. **Fidget ships/embeds/owns screenshots:** It does not. Dropping Capture means
   fidget never takes screenshots, never analyzes pixels, and never bundles
   vision/OCR for desktop content.

2. **Dropping Capture means agents never see pixels:** Agents the user attaches
   can still see pixels if the user runs a CU-capable harness or attaches a
   CU MCP server. The decision is about fidget's own code, not about agent
   capabilities in general.

3. **Security parity across drivers:** This note lists drivers and their
   documented features. It does not audit security models, sandboxing, or
   permission enforcement. Each driver's security is its own concern.

4. **Community MCPs are Hermes-integrated like cua-driver:** Only cua-driver is
   documented as a first-party Hermes integration. The others (domdomegg,
   minghin) are community MCPs that work via standard MCP attachment.

5. **minghin is multi-OS:** It is macOS 14+ only. The note calls it runner-up
   for macOS users specifically.

6. **domdomegg OS matrix is formally documented:** The README does not state
   supported OSes explicitly. The nut.js dependency suggests cross-platform
   intent, but this is inference, not vendor claim.

7. **fidget recommends one driver for all users:** The note recommends
   cua-driver as docs-primary for breadth and first-party integrations. Users
   choose their own MCP servers. The note informs; it does not enforce.

8. **ACP will gain a standard computer-use capability:** ACP v1 has none. Future
   versions may add one, but this note makes no prediction.

9. **Cursor local agents will gain native CU:** They rely on MCP attachment as
   of this writing. The note does not predict product roadmap changes.

10. **This decision is reversible:** Architecturally it is, but the decision
    to drop Capture is Oded's (Architect) and is recorded here as context for
    future work, not as a proposal open for re-litigation in this PR.

## Suggested follow-ups (out of scope for this PR)

The following are **next steps** this note identifies, but does not implement:

1. **Supersede or amend ADR-0005 Capture tiers:** ADR-0005 deferred Capture; this
   decision drops it. An ADR update should make that explicit.

2. **Scrub consent copy that says "Capture when it ships."** If any UI strings,
   comments, or docs say "Capture coming soon," remove or rephrase them to
   reflect the drop.

3. **Separate issue if user-facing CU guidance is wanted:** If fidget's README
   or docs should point users to cua-driver or explain how to attach a CU MCP,
   file that as a separate docs issue. This PR is research only.

4. **Evaluate zavora-ai and any other late-emerging MCPs:** Community MCP
   options change. The comparison here is 2026-09-22; future notes can
   revisit if new cross-platform or better-integrated options emerge.

## Gaps and open questions

**Assumption.** domdomegg/computer-use-mcp OS support is inferred from nut.js,
not stated by the vendor. Verification: check nut.js platform matrix or test the
MCP on Windows/Linux.

**Assumption.** minghin and domdomegg are not Hermes-integrated at the
first-party level. Verification: search Hermes docs/repo for references to these
MCP servers.

**Fact.** cua-driver MCP tools are documented at the URL cited. Verified
2026-09-22: https://cua.ai/docs/reference/cua-driver/mcp-tools resolves.

**Fact.** Hermes computer_use routing to cua-driver is first-party. Verified
2026-09-22: https://hermes-agent.nousresearch.com/docs/user-guide/features/computer-use
documents the integration and states "The built-in `computer_use` toolset is the
recommended Hermes integration. It speaks MCP over stdio to `cua-driver`".

**Fact.** zavora-ai/computer-use-mcp repository exists. Verified 2026-09-22:
https://github.com/zavora-ai/computer-use-mcp resolves (200).

**Fact.** Codex Computer Use plugin exists. Verified 2026-09-22:
https://developers.openai.com/codex/computer-use documents "Computer Use plugin"
for ChatGPT/Codex on macOS and Windows where host grants permissions.

**Fact.** Cursor Cloud Agents have harness-native computer use (isolated VM with
full desktop environment). Verified 2026-09-22: cursor.com/docs/cloud-agent and
cursor.com/blog/agent-computer-use.

**Fact.** ACP v1 has no computer_use client capability. Verified 2026-09-22:
https://agentclientprotocol.com/protocol/v1/initialization lists `fs.*`,
`terminal.*`, `elicitation.*` but no desktop-control capability.

## Summary

fidget **keeps Free sensing** (window metadata, idle, frontmost app) and
**drops Capture** (pixel analysis, screenshots, OCR/vision). Computer use for
agents lives **outside fidget** in harness-native capabilities or MCP servers.

**Recommended MCP path:** cua-driver (primary) for multi-OS, multi-harness,
accessibility-first desktop control with Hermes first-party integration.
minghinmatthewlam/computer-use-mcp (runner-up) for macOS-only
accessibility-first alternative.

Users who want agent desktop control attach cua-driver to their harness of
choice. The character keeps perch, fade, sensing, and personality. The harness or
MCP owns execution. The roles stay separated per ADR-0003.

---

Dated: 2026-09-22. Anchor: Oded / Architect decision context. All claims
labeled Fact/Inference/Assumption. URLs verified live 2026-09-22. Do-not-claim
list complete. Gaps named. Follow-ups listed as out-of-scope next steps.
