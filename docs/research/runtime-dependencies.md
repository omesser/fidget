# Runtime dependencies beyond the binary

Research for issue #735 — what fidget needs beyond the compiled binary to run, and whether each dep should be self-contained or declared. Dated 2026-09-23. Motivating bug: #726 (npx missing → silent Codex fail).

## What this note covers

Census of runtime deps beyond the fidget binary. For each: why needed, how detected today, required vs optional, self-contain options, and recommendation (keep external and declare, bundle, or eliminate). Followed by failure UX sketch and follow-up issues.

## Census table

| Dependency | Why Needed | Detection Today | Required vs Optional | Self-Contain Options | Recommendation |
|---|---|---|---|---|---|
| **Node.js + npx** | Launch registry ACP adapters for Claude / Codex / Pi (npx -y @agentclientprotocol/claude-agent-acp@latest, etc.) | `spawn` fails → Chat landing stays visible, surfaces missing launcher in landing message and header (`npx is not installed`). Shipped in #831. | Optional. Required only for Claude/Codex/Pi presets when user picks them. First-party ACP harnesses (hermes, opencode, grok, cursor-agent) need zero Node. | (1) Bundle Node runtime + pinned adapter trees (~50–100MB+, licenses become ours, update cadence ours); (2) prefer/default first-party ACP where available; (3) declare loudly when missing. | **Declare.** Accept external dep for Claude/Codex/Pi. Loud failure shipped (#831). Optional follow-up: spike bundling Node + adapters if install-Node is unacceptable. |
| **Harness CLI on PATH** | First-party ACP: hermes (`hermes acp`), opencode (`opencode acp`), grok (`grok agent stdio`), cursor-agent (`cursor-agent acp`). Binary-distribution registry agents: antigravity-acp (agy_acp_server), goose, copilot, junie, kimi, and 14 more. | Same as npx: `spawn` fails → landing stays visible, missing launcher surfaced. Shipped in #831. | Optional. Required only when user picks that harness. Default Character+StaticDirector needs no harness. | (1) Bundle the CLI binaries (licenses, sizes, update cadence); (2) declare and present install steps; (3) named presets for high-value zero-npx agents; (4) custom argv escape hatch (already exists). | **Declare per preset.** Named presets for hermes/opencode/grok/cursor-agent stay as-is (PATH binary). High-value additions to consider: goose, copilot (#457), antigravity-acp (#604 refresh). Custom argv already covers long tail. Present install steps in Settings/landing when missing. |
| **Platform WebView** | Tauri app shell requires native webview for Chat and Settings windows. | Implicit. On macOS: system WebKit (shipped). On Windows: WebView2 (user-installed or bundled via evergreen bootstrapper). On Linux: WebKitGTK (`libwebkit2gtk-4.1`) must be installed. | Required for Chat / Settings surfaces. Character+overlay alone can run without webview (hypothetically), but product includes Chat. | (1) Bundle/bootstrap on Windows (Tauri does this via WebView2 evergreen); (2) declare on Linux; (3) macOS ships WebKit. | **Declared on Linux only.** README already lists libwebkit2gtk-4.1-dev for build; runtime .deb pulls it as dep. AppImage: user must install libwebkit2gtk. Windows: Tauri WebView2 bootstrapper handles it. macOS: system WebKit. |
| **GStreamer (Linux)** | Cue audio playback on Linux (crate `gstreamer`, gst-plugins-base). macOS uses AVFoundation. Windows uses platform audio APIs. | Graceful. Audio cue plays or silently skips if GStreamer unavailable; no crash, no error surface. | Optional. Fidget works without sound. Cue audio is personality flavor, not functional requirement. | (1) Bundle GStreamer libs; (2) declare in README/install; (3) leave optional / best-effort. | **Declare, keep optional.** README notes GStreamer for Linux audio. No loud failure; silent skip is acceptable for optional audio. |
| **libfuse2 / libfuse2t64 (Linux AppImage)** | AppImage runtime requires FUSE to mount the squashfs payload. | External. User sees "cannot execute" or "FUSE not available" from AppImage runtime, not from fidget. | Required for AppImage distribution only. .deb does not need it. | (1) Migrate to AppImage type 2 with bundled runtime (still needs kernel FUSE or `--appimage-extract`); (2) declare in README; (3) offer .deb as FUSE-free path. | **Declare for AppImage.** README already notes libfuse2 / libfuse2t64. .deb is the FUSE-free alternative. Keep both distributions. |
| **X11 vs Wayland (Linux)** | Desktop environment, not a shippable dep. Feature delta: Wayland restricts overlay positioning and per-window metadata. X11 allows full feature set. | Implicit. Wayland detected at runtime → overlay keeps to screen edges, Perches disabled. X11 → full features. No error; supported Wayland mode. | Environment choice, not a dependency. Both work; Wayland mode is more restricted. | N/A. Desktop environment choice, not a bundling decision. | **Document.** README already says "Under Wayland the sprite keeps to screen edges and loses window Perches — a supported mode, not an error. X11 gets both." No change. |
| **MCP stdio binary (FIDGET_MCP_BIN)** | When BYO harness registers fidget as MCP server via stdio shim, the shim spawns this binary to bridge stdio ↔ loopback HTTP. | Environment variable FIDGET_MCP_BIN. If unset or binary missing, BYO harness stdio path fails. HTTP path (Authorization header) works without it. | Optional. Only needed for BYO harness stdio transport. HTTP transport with bearer token works for all five tested harnesses without stdio binary. | (1) Bundle shim binary beside app; (2) Settings snippet defaults to HTTP path (already preferred); (3) stdio as fallback/advanced. | **Keep external, document HTTP as primary.** Research already concluded HTTP+bearer is the primary BYO path for all harnesses; stdio shim is fallback. No bundling needed; Settings generates HTTP snippets. |

## Synthesis: what Fidget needs on top of the binary

**Measured from code and registry** (2026-09-23):

Registry: the ACP agent registry index, https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json, `version` field `1.0.0`, fetched 2026-09-23. Documented at https://agentclientprotocol.com/get-started/registry. Every count below is reproducible from that file.

Registry total: 41 agents.
Distribution keys: 19 binary, 22 npx, 2 uvx. Those sum to 43, not 41, because `kilo` and `sigit` each carry both a binary and an npx distribution. Read them as "agents reachable this way", not as a partition.

fidget `launch()` today (src-tauri/src/harness.rs):
- claude, codex, pi: npx adapters (need Node)
- cursor-agent, grok, hermes, opencode: PATH binaries (zero Node)
- custom: split whitespace, user-supplied argv

**Inference:** Three presets need npx. Four need PATH binary (but not npx). Many registry binary agents (goose, copilot, antigravity-acp, junie, kimi, 14 more) have no named fidget preset yet but could.

**Fact:** Registry `distribution.npx` means "install via npx package," not "must run under npx forever." After global install or brew, many agents become PATH binaries (measured: gemini, copilot, grok/opencode already have PATH forms; registry lists npx install flavor). fidget already uses PATH for grok/opencode.

**Inference:** The npx dep is not universal. It is specific to three registry adapters (Claude/Codex/Pi) when the user picks those presets and Node is not installed. First-party ACP harnesses and binary-distribution agents avoid npx entirely.

## Call: self-contained vs declare for each dependency

**Locked decisions (Oded, 2026-09-23):** Node/npx and Harness CLIs declared, not bundled. WebView as recommended (Linux declared). GStreamer optional. libfuse for AppImage only (prefer type 2 with bundled runtime when we ship that path). X11/Wayland is environment, not dependency. MCP stdio binary: do not bundle; challenge need; optional download at best. Failure UX: sticky inline (copy-pasteable), not toast. Named presets: goose yes (#930), copilot not needed, antigravity+others V2.

### Node.js + npx

**Call: Declare. LOCKED (Oded, 2026-09-23).**

Accept external dep for Claude/Codex/Pi presets. Install Node (https://nodejs.org/), ensure npx on PATH. Loud failure shipped (#831). Not bundling. README lists Node as requirement for harnesses that need it.

**Reasoning:** (1) Three of seven named presets need it; four do not. (2) Bundling Node is 50–100MB+, licenses, and update cadence. (3) Many users already have Node. (4) Preferring first-party ACP (hermes, opencode, grok, cursor-agent) over npx adapters is product/UX, not shipping Node.

**npx alternatives spike (V2 deferred):** First-party Claude/Codex ACP binaries if/when Anthropic/OpenAI ship them to replace Zed npm adapters. Good future spike; not V1.

**Do not:** Vendor acpx. Wrong layer (measured: acpx still calls npx for Claude/Codex/Pi adapters; does not remove Node dep). See architect comment 2026-09-17.

**Issue #928 closed not planned.**

### Harness CLIs on PATH

**Call: Declare per preset. LOCKED (Oded, 2026-09-23).**

Not bundling. Graceful fail if missing. README lists PATH + authenticated as requirements per harness. Present install steps in Settings/landing when selected harness CLI is missing.

Named presets stay as-is: hermes, opencode, grok, cursor-agent. High-value new named preset: goose (#930). Copilot not needed for now (not prevalent enough). Antigravity+others V2. Custom argv already exists as escape hatch for long tail (kimi, junie, kiro, iflow, stakpak, etc.).

**Reasoning:** Each is a separate product. Bundling 5+ harness binaries for "pick one" is larger than declaring "install the one you pick." Registry already has 19 binary agents; we cannot bundle all. Loud failure shipped (#831).

### Platform WebView

**Call: Declared on Linux only. LOCKED (Oded, 2026-09-23).**

As recommended. Graceful fail if missing. README documents Tauri WebView requirement: Windows handled by Tauri WebView2 bootstrapper, Linux declared, macOS nothing extra.

macOS: system WebKit (shipped).
Windows: Tauri WebView2 evergreen bootstrapper handles it (user-installed or bundled).
Linux: libwebkit2gtk-4.1 (runtime) must be installed. README already notes libwebkit2gtk-4.1-dev for build; .deb package pulls runtime dep automatically. AppImage: user must install libwebkit2gtk-4.1 (or libwebkit2gtk-4.1-0 on older distros).

**Reasoning:** Tauri chose WebView2 bootstrapper strategy for Windows; we inherit that. macOS WebKit is system. Linux is the only platform where the dep is external and user-installed, but it is standard desktop library.

**No change needed.** Already documented.

### GStreamer (Linux audio)

**Call: Declare, keep optional. LOCKED (Oded, 2026-09-23).**

README lists as requirement for Linux audio cues. Nice-to-have. Silent skip is acceptable for optional audio. Fidget works without sound.

**Reasoning:** Audio is personality flavor, not functional requirement. Graceful degradation (silent skip) is better UX than loud failure for an optional feature. Many users have GStreamer installed (common desktop dep).

**No change needed.** Already documented in DEVELOPMENT.md.

### libfuse2 / libfuse2t64 (Linux AppImage)

**Call: Declare for AppImage. LOCKED (Oded, 2026-09-23).**

AppImage distribution only. When we ship AppImage, prefer type 2 with bundled runtime. README already notes libfuse2 / libfuse2t64. .deb is the FUSE-free alternative.

**Reasoning:** AppImage runtime requires FUSE to mount squashfs. Two distributions (AppImage + .deb) cover both "user wants portable bundle" and "user wants native package manager." AppImage type 2 with bundled runtime reduces external FUSE dep (still needs kernel FUSE or `--appimage-extract` fallback).

**No change needed for current distributions.** .deb and tarball releases do not need FUSE. AppImage path future work.

### MCP stdio binary (FIDGET_MCP_BIN)

**Call: Do not bundle; challenge need; optional download at best. LOCKED (Oded, 2026-09-23).**

BYO harness registration via HTTP + bearer token is the primary path for all five tested harnesses (claude, hermes, opencode, grok, codex). Stdio shim is fallback/advanced. No bundling with main binary.

**Reasoning:** Research (docs/research/byo-harness-mcp-registration.md) concluded HTTP path works for all, stdio costs "absolute path to a binary the user has to locate, and buys nothing the header route does not already give." Settings snippets default to HTTP. Challenge whether stdio path is still needed.

**No change needed.** Already documented and implemented. HTTP primary.

### uv / uvx (for registry uvx agents)

**Call: Out of scope for V1.**

Registry has 2 uvx agents, `fast-agent` 0.10.1 and `minion-code` 0.1.44. Same "declare the runner" problem as npx, different toolchain (Python uv instead of Node). No named fidget presets for uvx agents yet. Custom argv escape hatch already covers them if user has uv installed.

**Reasoning:** Same as npx: bundling a package runner for 2 agents is heavier than declaring "install uv if you want fast-agent." Defer until user demand or named preset for one of them.

**No follow-up issue.** Revisit when/if uvx agent becomes a named preset candidate.

## Failure UX when declared dep is missing

### What shipped (#831)

When spawn of selected harness fails (npx or any PATH binary):

1. Chat landing stays visible. Does not dismiss into dead chat.
2. Landing message surfaces the missing launcher: `` `npx` is not installed `` for Claude/Codex/Pi, or `` `<binary>` is not installed `` for first-party ACP.
3. Chat header shows `<harness> - not running (missing <tool>)`.
4. Retry works. After user installs the dep, pressing Connect button again succeeds.

This covers npx-adapter presets (claude, codex, pi) and first-party ACP presets (hermes, opencode, grok, cursor-agent).

### Locked failure UX (Oded, 2026-09-23)

**Sticky inline error.** NOT a toast or snackbar. Reason: user can copy-paste the message into search/AI. Keep visible until fixed or intentional user action (Retry, change harness, explicit dismiss). Issue #950 (sticky inline missing-dep message).

Landing stays visible (#831 already shipped). Issue #949 (Harness Apply/Connect + chat "initializing…").

### Remaining gaps

**Install URLs per preset (#929).** Landing message says `` `npx` is not installed `` but does not link to https://nodejs.org/. First-party ACP messages say `` `hermes` is not installed `` but do not link to install pages.

**Grouping presets by dep (#929).** Settings/landing do not yet group zero-Node presets (hermes, opencode, grok, cursor-agent) separately from npx presets (claude, codex, pi).

### Optional deps (no loud failure)

**GStreamer (Linux audio):** Silent skip is acceptable. No error surface. README already documents.

**WebView on Linux:** If libwebkit2gtk missing, app fails to start (Tauri/GTK error, not fidget-specific). No special handling needed beyond .deb dependency and README note for AppImage users.

**FUSE for AppImage:** AppImage runtime handles the error ("FUSE not available"). README already documents. .deb is FUSE-free alternative.

## Named preset candidates (zero-npx, not yet in Fidget)

**Measured:** Registry has 19 binary-distribution agents. fidget has named presets for 4 of them (cursor-agent, opencode, grok via PATH form, hermes). The rest require custom argv.

**Locked decisions (Oded, 2026-09-23):**

1. **goose** (`goose acp`) — YES. Block/open-source, strong "local agent" candidate. Issue #930 filed.
2. **copilot** (`copilot --acp --stdio`) — NO. Not prevalent enough for now. Issue #457 closed without named row.
3. **antigravity-acp** (`agy_acp_server.par` / `.exe`) — V2. Google's official ACP server for Antigravity. Registry v1.1.1 binary on dl.google.com. Updates #604 (premise "agy has no ACP" now stale; official binary adapter exists). Not V1.
4. **junie, kimi, others** — V2 TBD. Not V1.

**Do not add:** `gemini --acp` (Gemini CLI is sunset; Google path is Antigravity). Noted in architect correction 2026-09-17.

## Follow-up issues filed

Based on Done-when checklist, decisions (Oded, 2026-09-23), and this research:

1. **#929: Settings/landing copy: Distinguish first-party ACP vs npx-adapter presets**: When presenting harness options, clarify which need Node (Claude/Codex/Pi) vs which are standalone CLIs (hermes/opencode/grok/cursor-agent). Helps user choose zero-dep option when possible. Includes install URLs per preset in missing-launcher messages.

2. **#930: Named preset for Goose**: Add `launch()` row + landing button for goose (zero-npx, open-source local agent). Includes smoke test via probe-harness.sh. LOCKED decision: proceed.

3. **#948: ADR: Runtime dependency philosophy**: Document "self-contain vs declare" framework for future deps. Criteria: size, licenses, update cadence, user install base, required vs optional. Filed; not written in this PR.

4. **#949: Harness Apply/Connect + chat "initializing…"**: Chat surface UX when harness is starting. Filed.

5. **#950: Sticky inline missing-dep message**: Sticky inline error (copy-pasteable), not toast. Keep visible until fixed or intentional user action (Retry, change harness, explicit dismiss). Filed.

**Issue #928 closed not planned.** Node bundling spike not needed. Decision: declare Node, not bundle.

**Not filed (V1 decisions):**
- Copilot named preset: NO (not prevalent enough).
- Antigravity-acp, junie, kimi named presets: V2 TBD. Issue #604 already exists for antigravity; updated with registry binary finding in comments.
- npx alternatives spike (first-party Claude/Codex ACP binaries): V2 deferred. Good future spike; not V1.

## Resolved decisions (Oded, 2026-09-23)

Formerly open questions. All answered and locked.

**Q1. Node bundling decision:**
**RESOLVED: Declare, not bundle.** Not willing to ship Node runtime + adapter trees (~50–100MB+, licenses, update cadence). Accept "install Node" + loud error (#831) as V1 path for Claude/Codex/Pi. README lists Node as requirement for harnesses that need it. Issue #928 closed not planned.

**Q2. Named preset prioritization:**
**RESOLVED:**
- **goose:** YES. Issue #930 filed.
- **copilot:** NO. Not prevalent enough for now.
- **antigravity-acp, junie, kimi:** V2 TBD. Not V1.

**Q3. Failure UX details:**
**RESOLVED: Sticky inline error (copy-pasteable), not toast.** Landing stays visible (#831). Keep error visible until fixed or intentional user action (Retry, change harness, explicit dismiss). Issues #949 (harness initializing), #950 (sticky inline missing-dep).

**Q4. npx alternatives (first-party Claude/Codex ACP binaries):**
**RESOLVED: V2 deferred.** Good future spike if/when Anthropic/OpenAI ship first-party ACP binaries to replace Zed npm adapters. Not V1 work.

## Claims and evidence labels

**Measured** (from code/registry/live tests):
- Registry v1.0.0 (https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json, fetched 2026-09-23) has 41 agents, with 19 binary, 22 npx and 2 uvx distribution keys. `kilo` and `sigit` carry two each, so the keys outnumber the agents.
- fidget launch() in src-tauri/src/harness.rs: 3 npx presets (claude/codex/pi), 4 PATH presets (cursor-agent/grok/hermes/opencode), 1 custom.
- #726 motivating bug (closed by #831): npx missing caused silent dead chat. #831 shipped loud failure (landing stays, missing launcher surfaced).
- Architect comments dated 2026-09-17 on #735 (npx vs acpx, deep pass of registry agents, Gemini sunset correction).
- antigravity-acp v1.1.1 in registry with binary distribution (agy_acp_server.par/.exe on dl.google.com).

**Inferred** (from code patterns/docs):
- Registry npx means "install via npx," not "must run under npx forever." Many become PATH binaries after install (grok, opencode, gemini, copilot observed).
- Vendoring acpx does not remove npx dep (acpx still spawns npx for Claude/Codex/Pi adapters per docs).
- Bundling Node is 50–100MB+ (typical Node runtime + node_modules for 3 packages).
- WebView2 bootstrapper on Windows is Tauri's choice; fidget inherits that strategy.

**Guess** (stated assumptions):
- User install base for Node is "many users already have it" (no hard data; common dev tool).
- GStreamer is "common desktop dep" on Linux (no hard data; ships with many distros for media playback).
- First-party Claude/Codex ACP binaries may exist someday (Anthropic/OpenAI have not announced them; Zed npm adapters are current path).

## Summary

fidget runtime deps beyond the binary: Node+npx (for 3 presets), Harness CLIs on PATH (per preset chosen), platform WebView (Linux declares, macOS/Windows self-contain), GStreamer (Linux optional audio), libfuse (Linux AppImage). Each assessed for self-contain vs declare.

**Decisions locked (Oded, 2026-09-23):** Declare Node+npx and Harness CLIs. Not bundling. Loud failure shipped (#831). README lists as requirements. Prefer first-party ACP (zero-npx) in Settings/landing copy (#929). Named preset for goose (#930). Copilot not needed; antigravity+others V2. Platform WebView / GStreamer / FUSE as recommended; no change. Failure UX: sticky inline (copy-pasteable), not toast (#950). MCP stdio binary: do not bundle; challenge need.

Self-contained where sane (Windows WebView2 bootstrapper, macOS system WebKit). Declared where bundling costs more than clarity (Node for 3 agents, per-harness CLIs). Silent failures eliminated by #831.

Follow-up issues: #929 (Settings copy), #930 (Goose preset), #948 (ADR runtime dep philosophy), #949 (harness initializing), #950 (sticky inline). Issue #928 closed not planned.

---

Dated: 2026-09-23. Anchor: Issue #735, architect comments 2026-09-17, motivating bug #726 (closed by #831). Decisions locked by Oded 2026-09-23. All claims labeled Fact/Measured/Inferred/Guess. URLs verified 2026-09-23 (registry.json resolves, antigravity-acp v1.1.1 present). Do-not-claim: this note does not claim fidget should bundle all 19 registry binary agents, or that acpx is the npx fix (explicitly rejected), or that Gemini CLI is a V1 path (sunset, per architect correction). Follow-up issues filed: #929, #930, #948, #949, #950. Issue #928 closed not planned.
