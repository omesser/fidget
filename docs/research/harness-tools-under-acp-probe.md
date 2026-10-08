# What a Harness keeps under a Fidget ACP attach, measured

Anchor: `39fd012c` on `main`, 2026-09-16. Every fidget citation below is
read against that tree. Probes ran on macOS 25.6 (Apple silicon), signed in
to claude.ai on a Team plan, in two rounds: 2026-09-16 for the `claude` and
`grok` rows, and 2026-09-17 for `hermes`, `opencode`, `cursor-agent`, `codex`
and the two claims round one left as Inference. Versions on `PATH`, or fetched
by `npx -y ...@latest` at run time:

| Component | Version |
|---|---|
| `@agentclientprotocol/claude-agent-acp` | 0.78.0 |
| `@anthropic-ai/claude-agent-sdk` (bundled by the adapter) | 0.3.270, spawning its own `claude` 2.1.270 |
| `claude` on `PATH` (not what the adapter runs) | 2.1.273 |
| `opencode` | 1.18.30 |
| `hermes` | 0.18.2 |
| `grok` | 1.0.30 |
| `pi-acp` over `pi` | 0.0.33 over 0.85.1 |
| `cursor-agent` | 2026.01.23-916f423 |
| `@agentclientprotocol/codex-acp` over `codex` | 1.12.0 over 0.154.0 |

This note answers #668 alongside
[`harness-native-tools-under-acp.md`](./harness-native-tools-under-acp.md)
(#669), which did the desk research from adapter source and vendor docs at
anchor `e070216` and left five items open because it ran nothing. This note
runs them and records what came back. Every measurement agrees with that
note's Facts. The one number it got wrong had changed on `main` between its
anchor and this one. The contradictions section below names it.

Round two closed every open Harness row, turned two Inferences into Facts, and
**corrected one Inference that was wrong**: advertising `elicitation.form`
does not switch a tool on. It changes how a question that arrives anyway is
drawn. Answer 6 and ranked option 2 carry the correction and the measurement
behind it. Round two also added `codex` and `cursor-agent`, two named rows in
`harness.rs` that round one's matrix did not cover at all.

Round three (2026-09-18, #786) reran the computer-use probe on the same
Mac. `npx -y @agentclientprotocol/claude-agent-acp@latest` was 0.79.0,
spawning bundled `claude` 2.1.274. ACP `_auth/status_update` reported
`account.plan` as `team` and then `Claude Team`. `~/.claude.json`
`oauthAccount.organizationType` is `claude_team` and `seatTier` is
`team_tier_1`. The Team run could not isolate the plan gate from the
interactive-session gate, but vendor documentation now resolves the
remaining question: computer use requires an interactive Claude Code
session, while the ACP adapter launches the SDK's stream-json print path.
Claim 11 is therefore a documented Inference, not an Assumption. The
fixture run is in the round-three section below.

## How claims are marked

**Fact** is something read off a live ACP session, a shipped file, or a
vendor document quoted here. **Inference** is a deduction from Facts.
**Assumption** is a claim nothing here checked. Predictions and unseen
causes are Assumptions.

## Answers to the eight spike questions

1. **What tool list does ACP expose for `claude`?** Fact, measured off the
   Agent SDK's `init` message: 33 built-in tools, the whole `claude_code`
   preset. `Task`, `Bash`, `Read`, `Write`, `Edit`, `NotebookEdit`, `LSP`,
   `Skill`, `ToolSearch`, `WebSearch`, `WebFetch`, `Monitor`, `Workflow`,
   `EnterWorktree`, plan mode, cron and scheduling tools, MCP resource
   tools. The list also carries every tool of every MCP server the SDK
   connected. Two things are absent: `AskUserQuestion`, and anything named `computer-use`.
2. **Computer use under ACP**: Fact, from Anthropic's page: "Computer use is a
   research preview on macOS that requires a Pro or Max plan. It is not
   available on Team or Enterprise plans. It requires an interactive session,
   so it is not available in non-interactive mode with the `-p` flag."
   Fact, from `sdk.mjs` 0.3.270: the SDK launches `claude` with
   `--output-format stream-json --input-format stream-json
   --permission-prompt-tool stdio`, the print-mode class. Fact, measured: the
   session's `mcp_servers` list carries no `computer-use` entry, with or
   without the undocumented `ALLOW_ANT_COMPUTER_USE_MCP=1` in the child's
   environment. Inference: the adapter does not satisfy the vendor's
   interactive-session requirement, so a Pro or Max plan would not make this
   ACP path eligible. The blocker is Anthropic's documented session mode,
   not fidget's client capabilities.
3. **Web search and fetch**: Fact, measured: `WebSearch` and `WebFetch` are in
   the tool list in every Claude probe. Fact, measured: Grok's session lists
   `web_search`, `web_fetch`, `open_page` and the `x_*` search tools. Fact,
   measured in round two: hermes lists `web_search` and `web_extract`;
   cursor-agent lists `WebSearch` and `WebFetch`; codex lists `web__run`;
   opencode lists `webfetch` and **no** web-search tool, which matches the
   vendor's "websearch is conditional". Every Harness measured here reaches the
   web except pi.
4. **User's existing MCP servers**: Fact, measured: with `cwd` set to
   fidget's data folder the session connected the user-scope plugin server
   (`plugin:context-mode:context-mode`, `connected`) and listed five claude.ai
   connectors (`needs-auth`). With `cwd` set to `$HOME`, where the user once
   ran `claude mcp add` at the default local scope, one more server appeared
   (`caveman-shrink`, `failed`, a broken command but loaded). The built-in
   tool list was byte-identical across both runs. Fact, from the Agent SDK
   docs: "`~/.claude.json` global config: Always read", and "The `cwd` option
   determines where the SDK looks for project-level inputs." Inference: the
   only MCP loss under attach is the local and project scope. Its cause is
   the `cwd` fidget chooses (`fidget_core::memory::data_dir()`, passed
   at `harness.rs:1714`). ACP, the adapter, and the one MCP entry fidget
   hands over on `session/new` play no part in it.
5. **Permissions**: Fact, measured: the tools are in the list. A native tool
   call that does nothing under attach is therefore a permission problem or
   a turn-budget problem, not a missing tool. Fact, at anchor:
   `harness::TURN_TIMEOUT` defaults to 120 seconds
   (`an_unset_timeout_gives_a_harness_turn_minutes_not_the_http_hop`,
   `harness.rs:71`), since #695 (#690). #669's "20s default turn budget" was
   `model::TIMEOUT` at its anchor `e070216` and no longer applies to a
   Harness turn. Not measured: whether Chat answers a
   `session/request_permission` inside that budget in practice.
6. **Client capabilities**: Fact, from the ACP initialization page: the
   client capabilities are `fs.readTextFile`, `fs.writeTextFile`,
   `terminal`, `elicitation`, and session config options. `fs/*` and
   `terminal/*` are methods the agent calls on the client. No client
   capability names computer use. Fact, from `acp-agent.js` 0.78.0:
   `AskUserQuestion` is routed only when
   `this.clientCapabilities?.elicitation?.form` is set, which matches the
   tool's absence in every probe run with `clientCapabilities: {}`.
   Inference: advertising `fs` or `terminal` switches on no agent-side tool.
   **Corrected in round two.** The first round inferred that advertising
   `elicitation.form` switches one tool on. It does not. Fact, measured with
   `clientCapabilities: {"elicitation":{"form":true}}`: `init.tools` is the same
   33 names as with `{}`, and `AskUserQuestion` is in neither. Fact, from
   `acp-agent.js` 0.78.0 at the gate: *"AskUserQuestion is surfaced to us as a
   normal permission check (the SDK routes it through `canUseTool` whenever a
   callback is registered, rather than the interactive dialog). Present it as an
   ACP form elicitation."* The capability changes how a question that already
   arrived is **rendered**, from a plain permission request into a form. It adds
   nothing to the advertised tool list. Option 2 below is re-ranked on that.
7. **Other Harnesses**: See the matrix. Round one measured Grok by
   self-report and could not reach opencode or hermes, because both default to
   a local model server that was not running (`http://127.0.0.1:8000/v1`).
   **Round two reached both**, and added the two named rows the matrix never
   had. Fact, measured: hermes lists **27 tools** through its own `/tools` slash
   command over ACP, which needs no provider at all. opencode lists **10 tools**
   when its model is overridden to `xai/grok-4.6`, a provider this machine has a
   key for. cursor-agent lists **19** and codex **22**, both against their own
   signed-in accounts. pi lists **4**: `read`, `bash`, `edit`, `write`. Its
   local model server was the thing that was down; once it was up, pi answered.
   The `pi v0.85.1` that two earlier probes recorded is a session banner pi
   prints before the turn, not its answer.
8. **Escape hatches**: Ranked below. The evidence rules out two of the issue's
   five. There is no `claude-agent-acp --cli` mode (the adapter's `cli`
   handling is argument parsing for `--version`), and passing user MCP
   config through `session/new` is unnecessary because the SDK already
   reads `~/.claude.json` and merges.

## The load-bearing claims, labelled

| # | Claim | Label | Source |
|---|---|---|---|
| 1 | The `claude` row runs Zed's adapter over the Agent SDK, which spawns its own bundled `claude` in stream-json print mode | Fact | `harness.rs:144`; `sdk.mjs` 0.3.270 argv; `init.claude_code_version` = 2.1.270 while `PATH` has 2.1.273 |
| 2 | fidget attaches with `cwd` = `data_dir()` and advertises no client capabilities beyond `clientInfo` | Fact | `harness.rs:1714`; `acp_wire.rs:461`; `crates/core/src/memory.rs:51` |
| 3 | The attach keeps 33 built-in tools including `WebSearch`, `WebFetch`, `Bash`, `Task`, `Skill` | Fact | measured, `init.tools`, two `cwd` values, identical |
| 4 | `AskUserQuestion` is absent from `init.tools` under `clientCapabilities: {}` **and stays absent under `elicitation.form`**. The capability re-renders a question that arrives as a permission check; it adds no tool | Fact, both halves | measured both capability sets, 33 names identical; `acp-agent.js` 0.78.0 line 5263 comment and the `canUseTool` gate at 5267 |
| 5 | User-scope MCP servers and claude.ai connectors load regardless of `cwd` | Fact | measured, `init.mcp_servers` in both runs; Agent SDK docs "Always read" |
| 6 | Local-scope MCP servers load only when `cwd` is the directory they were added in | Fact | measured, `caveman-shrink` present only with `cwd=$HOME`; MCP docs scope table |
| 7 | Project-scope `.mcp.json` loads relative to `cwd` | **Fact** | measured in round two against a fixture directory holding only a `.mcp.json`; `probe-project-scope` appeared in `init.mcp_servers` |
| 8 | Computer use needs macOS, Pro or Max (not Team or Enterprise), claude.ai auth, and an interactive session | Fact | code.claude.com/docs/en/computer-use |
| 9 | The computer-use opt-in is `enabledMcpServers`, recorded per project in `~/.claude.json` | Fact | code.claude.com/docs/en/mcp, "an opt-in list for built-in servers that default to off, such as `computer-use`" |
| 10 | No `computer-use` server appears in an ACP session on this machine, with or without `ALLOW_ANT_COMPUTER_USE_MCP=1` | Fact | measured, `init.mcp_servers`; round three: still absent with `enabledMcpServers: ["computer-use"]` on a throwaway fixture `cwd` |
| 11 | On a Pro or Max machine with the opt-in recorded for fidget's `cwd`, the interactive gate would still keep `computer-use` out of an ACP session | **Inference** | Anthropic's computer-use docs require an interactive Claude Code session; the CLI reference defines `-p` as the SDK/non-interactive path; the ACP adapter launches stream-json print mode. Round three confirmed the Team plan but was not needed to establish this session-mode conclusion |
| 12 | The Harness turn budget is 120 seconds by default | Fact | `harness.rs` tests for #690 at anchor |
| 13 | ACP has no computer-use client capability; `fs/*` and `terminal/*` are methods the agent calls on the client | Fact | agentclientprotocol.com/protocol/initialization |
| 14 | Grok's ACP session carries web, shell, file, subagent, scheduler and media tools | Fact about the self-report, Inference about the actual list | measured `AGENT_TEXT`, 33 names |
| 15 | opencode and hermes carry their own toolsets under a fidget attach | **Fact** | measured in round two: hermes 27 tools via `/tools`, opencode 10 tools on `xai/grok-4.6` |
| 15a | cursor-agent carries 19 tools under attach, web and shell included | Fact | measured, self-report: `Shell`, `WebSearch`, `WebFetch`, `Task`, `ListMcpResources`, `FetchMcpResource`, `GenerateImage` among them |
| 15b | codex carries 22 tools under attach, web and shell included | Fact | measured, self-report: `web__run`, `exec`, `exec_command`, `apply_patch`, `request_user_input`, the `collaboration.*` subagent set, `list_mcp_resources` |
| 15c | codex and cursor-agent are named rows in `harness.rs` that this note's matrix did not cover | Fact | `harness.rs:146,153` at anchor; `HARNESS_PRESETS` has seven entries |
| 16 | Hermes' browser tools are **listed but gated**: all ten `browser_*` names appear in `/tools` while the start-up CDP check marks them unavailable for the turn | Fact, refined in round two | measured in one run: `Available tools (27)` includes `browser_navigate` etc., and stderr carries `check_fn _browser_cdp_check returned False; dependent tools will be unavailable this turn`. A tool list is therefore not a capability list on this Harness |
| 17 | pi's own tools are intact under `pi-acp`: it lists `read`, `bash`, `edit`, `write`, which is its whole documented set | **Fact** | measured twice with different prompts, identical both times; `pi --help` opens "pi - AI coding assistant with read, bash, edit, write tools" |
| 17b | A probe that returns only `pi v0.85.1` means pi's provider is down, not that pi withholds its tools | Fact | the banner precedes every turn; the same probe returned the tool list once the local model server was running |
| 17a | `pi-acp` advertises no MCP capability at all | Fact | measured `initialize`: `mcpCapabilities: {"http":false,"sse":false}`. `README.md:159`'s "Chat-only (no MCP)" is right about the transport, whatever it implies about pi's own tools |

## The matrix

Cells describe what the agent has inside a session fidget opened, with
fidget's current `cwd` and empty client capabilities. Bold marks a
measured cell.

| Harness | Computer use (user's desktop) | Shell | Web search / fetch | User's own MCP servers | Filesystem / edit | Ask the user a question |
|---|---|---|---|---|---|---|
| `claude` | **no** (Pro/Max, macOS, claude.ai auth, and interactive-session requirements; ACP uses the SDK print path) | **yes** | **yes** | **conditional**: user scope and claude.ai connectors yes; local and project scope only when `cwd` matches | **yes** | **no** without `elicitation.form` |
| `opencode` | no (none exists) | **yes** (`bash`) | **fetch only** (`webfetch`; no websearch tool in the list) | **yes** (`mcpCapabilities: {http, sse}`; the user's own agents and commands loaded) | **yes** (`edit`, `write`, `read`, `glob`, `grep`) | **no** (none listed) |
| `hermes` | no (browser automation instead, opt-in) | **yes** (`terminal`, `process`, `execute_code`) | **yes** (`web_search`, `web_extract`); **browser tools listed but unavailable** — the start-up CDP check fails and gates all ten | **yes** (vendor `mcp` subcommand; not exercised) | **yes** (`read_file`, `write_file`, `patch`, `search_files`) | **no** (none listed) |
| `cursor-agent` | no | **yes** (`Shell`) | **yes** (`WebSearch`, `WebFetch`) | **yes** (`ListMcpResources`, `FetchMcpResource`) | **yes** (`Read`, `Write`, `StrReplace`, `Glob`, `Grep`, `Delete`) | **no** (none listed) |
| `codex` | no | **yes** (`exec`, `exec_command`, `write_stdin`) | **yes** (`web__run`) | **yes** (`list_mcp_resources`, `read_mcp_resource`, `mcpCapabilities: {http}`) | **yes** (`apply_patch`) | **yes** (`request_user_input` listed) |
| `grok` | no (Grok Bot runs on a cloud computer) | **yes** (`run_terminal_command`) | **yes** (`web_search`, `web_fetch`, `open_page`) | conditional: `_x.ai/mcp/servers_updated` came back empty on a machine with none configured; project scope keys off `cwd` per vendor docs | **yes** | **yes** (`ask_user_question` listed) |
| `pi` | no (none exists) | **yes** (`bash`) | **no** (no web tool in the list, matching the vendor) | **no** (`mcpCapabilities: {http: false, sse: false}`) | **yes** (`read`, `edit`, `write`) | **no** (none listed) |

Inference, unchanged from #669 and now with all seven rows measured: no
Harness brings desktop control to an ACP session fidget opens. ADR-0003's "not portable across Harnesses" understates it. Desktop
control is available under no Harness on this path.

Two things the second round changed in the reading of this table. Shell, web
and filesystem are **not** the differentiators they looked like after round
one. Shell and filesystem are universal across all seven. Web is the only one
of the three that varies, and it splits two ways rather than being a spectrum:
five Harnesses have both search and fetch, opencode has fetch only, pi has
neither. What varies most is asking the user a question: only `grok` and
`codex` list a tool for it, and neither needs a client capability to do so. And a listed tool is not a
callable tool, which hermes' ten gated `browser_*` names prove in the one run
where both the list and the gate were captured.

## Ranked options

Ordered by tools recovered per unit of architectural cost.

### 1. Attach in a directory the user's own configuration keys on

Fact: this recovers local-scope MCP servers (measured). Inference: it
recovers project-scope `.mcp.json`, Grok's project `.grok/config.toml`
walk, and opencode's project config, all of which key off `cwd` per vendor
docs. The candidate directory is a user-chosen project folder, or `$HOME`
as a default that matches where `claude mcp add` lands when run outside a
project. ADR-0003 is untouched. ADR-0018 is untouched, since it already
accepts that the Harness runs its own tools headless. ADR-0023 is
untouched, since the MCP entry fidget hands over on `session/new` merges
with whatever `cwd` brings in rather than replacing it. Cost: the session file and Action Log
currently live beside `cwd` in the data folder (`harness.rs:1226` comment),
so the two paths have to be separated first.

### 2. Advertise `elicitation.form`

**Re-ranked in round two on a corrected premise.** Round one ranked this second
because it read as the one client capability that switches a tool on. Measured,
it switches none: `init.tools` is byte-identical with and without it, and
`AskUserQuestion` is in neither list (claim 4). What it actually buys is the
rendering of a question that arrives anyway. Without it the adapter sends a
plain `session/request_permission`; with it, an ACP form.

That is still worth having, and it is still cheap, but it is a Chat polish item
rather than a tool-recovery one. Grok and codex both list their own ask tool
regardless of any capability. ADR-0018 gives fidget the chat surface, so a
multiple-choice prompt drawn by Chat is inside the decision. ADR-0003 and
ADR-0023 are untouched. Cost: a form renderer in Chat.

The tool-recovery win that round one attributed to this option does not exist.
Option 1 is now the only option in this list that recovers tools.

### 3. Say what survives, per row, in the README

Fact: `README.md:159` reads "Chat-only (no MCP)" for `pi`, which is true of
fidget's MCP and reads as a claim about pi's own tools. The Harness
Support tables carry no tool-class column at all. No ADR consequence.

### 4. The adapter's `_meta.claudeCode.options` hatch

Fact: `session/new` `_meta.claudeCode.options` reaches the SDK's `Options`
(`acp-agent.d.ts:637`), and `emitRawSDKMessages` beside it is what these
probes used. Nothing measured here needs it. The preset is already whole
and the MCP merge already happens. Its only remaining use would be
`options.env`, which can carry a provider key into a Harness fidget
spawns, an ADR-0010 hazard. Keep it for diagnostics such as the probe and
out of the product.

### 5. Host an interactive session for a full-tool lane (#508)

Inference: an interactive `claude` on a Pro or Max plan is the documented
shape for computer use, but ACP cannot provide that session mode. It is the
only route to that cell in the matrix and it costs the most. ADR-0018 forbids it
outright ("We never launch, embed, or wrap the Harness's TUI"), so it needs
a supersession. ADR-0023 is strained. An interactive CLI takes MCP from its
own config, so fidget's endpoint would have to be registered rather than
handed over on `session/new`. ADR-0003 is the one it serves. Not filed
here. #508 already carries the question, and this note adds that the
payoff is one tool class on one Harness on one plan tier.

### 6. Rejected: a Fidget Executor

Out of scope by the issue and by ADR-0003. Listed so the ranking is
exhaustive.

## Recommendation

Option 1 ranks first. It claims that changing `cwd` restores the user's
local and project MCP servers to the attach. The computer-use cell is
closed by the vendor's documented session-mode requirement: changing `cwd`
or recording `enabledMcpServers: ["computer-use"]` cannot turn the ACP
SDK print path into an interactive Claude Code session. #508 remains the
route that would require a separate architectural decision.

No further Pro or Max probe is required for this question. The vendor docs
define the eligibility boundary, and the ACP adapter's launch mode is already
recorded above.

Any machine on any plan can rerun the cheap half. Run the same command
twice with two `cwd` values, one of them a directory where
`claude mcp add <name> ...` was run at the default scope, and diff
`init.mcp_servers`.

`scripts/probe-harness.sh` is not a substitute. It prints the handshake
and the turn outcome, not the agent's tool list, and ACP has no method that
returns one. The probe client used for every measurement in this note is
below. It matches fidget's client on the points that matter: empty
client capabilities, `mcpServers: []`, and no answer to
`session/request_permission`.

```js
import { spawn } from "node:child_process";

const [cwd, secs, prompt, dashdash, ...argv] = process.argv.slice(2);
if (dashdash !== "--") throw new Error("usage: cwd secs prompt -- argv...");
const child = spawn(argv[0], argv.slice(1), { cwd, stdio: ["pipe", "pipe", "pipe"], env: process.env });
let id = 0;
const send = (method, params) =>
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: id++, method, params }) + "\n");
const pending = {};
const request = (method, params) =>
  new Promise((resolve) => { pending[id] = resolve; send(method, params); });

let text = "";
let buf = "";
const stderr = [];
child.stderr.on("data", (d) => stderr.push(String(d)));
child.stdout.on("data", (d) => {
  buf += d;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i);
    buf = buf.slice(i + 1);
    if (!line.trim()) continue;
    let m;
    try { m = JSON.parse(line); } catch { console.log("non-json:", line.slice(0, 200)); continue; }
    if (m.id !== undefined && pending[m.id]) { pending[m.id](m); delete pending[m.id]; continue; }
    if (m.method === "_claude/sdkMessage") {
      const s = m.params?.message ?? m.params;
      if (s?.type === "system" && s?.subtype === "init") {
        console.log("INIT tools:", JSON.stringify(s.tools));
        console.log("INIT mcp_servers:", JSON.stringify(s.mcp_servers));
        for (const k of Object.keys(s)) if (!["tools", "mcp_servers"].includes(k)) console.log("INIT", k, "=", JSON.stringify(s[k]).slice(0, 200));
      }
      continue;
    }
    if (m.method === "session/update") {
      const u = m.params.update;
      if (u.sessionUpdate === "agent_message_chunk" && u.content?.type === "text") text += u.content.text;
      else if (u.sessionUpdate === "tool_call") console.log("TOOL_CALL", u.title, u.kind);
      else if (u.sessionUpdate === "available_commands_update") console.log("COMMANDS", (u.availableCommands || []).map((c) => c.name).join(","));
      else if (u.sessionUpdate !== "agent_thought_chunk") console.log("UPDATE", u.sessionUpdate);
      continue;
    }
    if (m.method === "session/request_permission") {
      console.log("PERMISSION_REQUEST", JSON.stringify(m.params.toolCall?.title), JSON.stringify(m.params.options?.map((o) => o.kind)));
      continue;
    }
    if (m.method) {
      console.log("AGENT_REQ", m.method, JSON.stringify(m.params).slice(0, 200));
      if (m.id !== undefined) child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: m.id, error: { code: -32601, message: "Method not found" } }) + "\n");
    }
  }
});

const init = await request("initialize", { protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: "probe-668", version: "0" } });
console.log("INITIALIZE agentCapabilities:", JSON.stringify(init.result?.agentCapabilities), "agentInfo:", JSON.stringify(init.result?.agentInfo));
console.log("INITIALIZE authMethods:", JSON.stringify((init.result?.authMethods || []).map((a) => a.id)));
const meta = process.env.PROBE_META ? JSON.parse(process.env.PROBE_META) : undefined;
const ses = await request("session/new", { cwd, mcpServers: [], ...(meta ? { _meta: meta } : {}) });
if (ses.error) {
  console.log("SESSION_NEW error:", JSON.stringify(ses.error).slice(0, 400));
} else {
  console.log("SESSION_NEW ok, modes:", JSON.stringify(ses.result?.modes?.availableModes?.map((x) => x.id)));
  const p = request("session/prompt", { sessionId: ses.result.sessionId, prompt: [{ type: "text", text: prompt }] });
  const timeout = new Promise((r) => setTimeout(() => r({ timeout: true }), Number(secs) * 1000));
  const done = await Promise.race([p, timeout]);
  console.log("PROMPT result:", JSON.stringify(done.result ?? done.error ?? done).slice(0, 300));
}
console.log("AGENT_TEXT:\n" + text);
const err = stderr.join("");
if (err.trim()) console.log("STDERR (first 1200):\n" + err.slice(0, 1200));
child.kill();
process.exit(0);
```

## Where this contradicts what the repository already says

- **ADR-0003, Consequences**: "The capability is a research preview gated
  behind a Pro or Max subscription" names one of four gates. Anthropic's
  page adds Team and Enterprise as excluded, claude.ai auth as required, and
  an interactive session as required. The fourth is the one no subscription
  clears and the one that bears on fidget's attach path. The decision
  still holds on its other grounds. The consequence is incomplete.
- **`README.md:159`**: "Chat-only (no MCP)" on the `pi` row. See option 3.
- **`harness-native-tools-under-acp.md`, claim 14**: The 20-second turn
  budget was true at its anchor `e070216` and is 120 seconds at this one
  (`harness::TURN_TIMEOUT`, #695 for #690). That note now carries a
  "since shipped" preamble line saying so, added in the same PR as this
  note per `docs/agents/docs.md`.
- **`docs/research/fidget-harness-two-way.md:14`**: "a local agent that keeps
  its own tools, permissions, and memory" holds for the tool preset and for
  user-scope configuration. It does not hold for computer use, and it holds
  for local-scope configuration only with a `cwd` change.

## Round two: how the remaining rows were measured

Same probe client, same machine, 2026-09-17. Three techniques closed the rows
round one left open.

**A provider-free tool list.** hermes advertises a `tools` slash command in
`available_commands_update`. Sending `/tools` as the prompt returns the registry
without a single model call, so the dead local provider stops mattering:

```sh
node acp-probe.mjs "$PWD" 90 '/tools' -- hermes acp
```

**A provider override.** opencode's configured model is `omlx/gemma-4-e2b-it-4bit`
against the same dead server, but this machine has `XAI_API_KEY` exported.
Overriding the model in a copy of the config reaches a live provider without
touching the user's own file:

```sh
python3 -c "import json;d=json.load(open('$HOME/.config/opencode/opencode.json'));d['model']='xai/grok-4.6';json.dump(d,open('/tmp/oc.json','w'))"
OPENCODE_CONFIG=/tmp/oc.json node acp-probe.mjs "$PWD" 150 '<tool-list prompt>' -- opencode acp
```

**A fixture directory for claim 7.** A directory containing nothing but a
`.mcp.json` naming one server, passed as `cwd`:

```sh
mkdir -p /tmp/projscope && printf '{"mcpServers":{"probe-project-scope":{"command":"node","args":["-e","setInterval(()=>{},1e9)"]}}}' > /tmp/projscope/.mcp.json
PROBE_META='{"claudeCode":{"emitRawSDKMessages":[{"type":"system","subtype":"init"}]}}' \
  node acp-probe.mjs /tmp/projscope 90 'Reply with the single word ok.' \
  -- npx -y @agentclientprotocol/claude-agent-acp@latest
```

`init.mcp_servers` came back carrying `probe-project-scope`, alongside the
user-scope and connector entries. That is claim 7, measured.

**Reading past pi's banner.** pi prints a session banner (`pi v0.85.1`, the
loaded context file, then every skill path) before the turn's own text. Two
earlier probes recorded that banner as pi's whole answer, which read as a
Harness that would not name its tools. It was the local model server being
down: the banner is emitted regardless, so the turn produced nothing after it.
With the server up, the same probe returns the list on the last line. Read the
end of `AGENT_TEXT`, not the start:

```sh
node acp-probe.mjs "$PWD" 180 '<tool-list prompt>' -- npx -y pi-acp@latest \
  | sed -n '/^AGENT_TEXT:/,$p' | tail -1
```

`read,bash,edit,write`, twice with different prompts, and `pi --help` opens
with the same four. **A probe that returns only a banner is evidence about the
provider, not about the Harness.**

For the `elicitation.form` probe, one line of the client changes:

```diff
-clientCapabilities: {}
+clientCapabilities: process.env.PROBE_CAPS ? JSON.parse(process.env.PROBE_CAPS) : {}
```

then `PROBE_CAPS='{"elicitation":{"form":true}}'`. Diff `INIT tools` against a
run without it. They are identical, which is claim 4.

## Round three: this machine is still Team (#786)

Same probe client, same Mac, 2026-09-18. The adapter was
`@agentclientprotocol/claude-agent-acp@0.79.0`. `session/new` used
`_meta.claudeCode.emitRawSDKMessages: true` (boolean). The 0.78.0 filter
array in the recipe above does not forward `_claude/sdkMessage` on this
adapter version.

The opt-in was `enabledMcpServers: ["computer-use"]` under
`projects["/private/tmp/fidget-786-probe/fixture"]` in `~/.claude.json`,
and under the `/tmp/...` symlink path. That directory was created for
this probe and removed afterwards. It was not the user's home project
and not fidget's data folder.

ACP `_auth/status_update` on the live session (email omitted):

```json
{"authStatus":{"kind":"account","label":"Claude Team","account":{"plan":"team","organization":"Prisma Photonics"}}}
```

A later update used `"plan":"Claude Team"` with the same label.
`oauthAccount.userRateLimitTier` is `default_claude_max_5x`. That is a
rate-limit label, not a consumer Max plan. The ACP account object and
`organizationType` are the plan fields, and both say Team.

`init.mcp_servers` from the resolved-cwd run, session
`2b70ef07-7414-468f-9011-9078aa69a521`, `claude_code_version` 2.1.274,
`apiKeySource` `none`:

```json
[{"name":"plugin:context-mode:context-mode","status":"connected","source":"plugin"},{"name":"claude.ai Claude Docs","status":"connected","source":"claudeai"},{"name":"claude.ai Gmail","status":"needs-auth","source":"claudeai"},{"name":"claude.ai Google Drive","status":"needs-auth","source":"claudeai"},{"name":"claude.ai Slack","status":"needs-auth","source":"claudeai"},{"name":"claude.ai Atlassian Rovo","status":"needs-auth","source":"claudeai"},{"name":"claude.ai Canva","status":"needs-auth","source":"claudeai"}]
```

No `computer-use` name. No `mcp__computer-use__*` tool. Claim 10 still
holds with the per-project opt-in recorded. Claim 11 is now an Inference
from the vendor's interactive-session requirement and the adapter's
stream-json print path. The Team result is consistent with that conclusion
but is not presented as a Pro or Max observation.

## Open questions

- Claim 11 is resolved as an Inference from Anthropic's documented
  interactive-session requirement and the ACP adapter's SDK print path.
- hermes' and opencode's lists are self-reported through a model turn, except
  hermes' `/tools`, which is the adapter's own registry and therefore stronger
  evidence. cursor-agent's and codex's are self-reported. Only the `claude` row
  is read off a structured `init` message. Self-report can omit.
- Whether hermes' `browser_*` tools become callable after `--setup-browser` and
  a passing CDP check. The list does not change; the gate does.
- Whether the claude.ai connectors that showed `needs-auth` here would show
  `connected` for a user who authorized them in the CLI. Nothing here
  exercised that.
- Whether Chat answers a permission request inside 120 seconds in practice
  for a `Bash` or `WebFetch` call under attach. That is a different spike.
