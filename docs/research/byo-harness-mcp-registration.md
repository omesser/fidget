# Registering Fidget with a Harness the user runs

> **Archival Note**: This is a historical pre-#599 research snapshot. For current
> BYO harness registration, see the Settings BYO UI in the running application,
> `docs/harness.md` for setup instructions, and
> `docs/research/byo-harness-mcp-reload.md` for reconfiguration/reload guidance.

Research for story 66 — "point any MCP-capable harness at fidget directly" —
and for the Advanced section on the Director settings tab that is meant to serve
it: a harness picker, a generated snippet in a copyable box, and per-harness
instructions under it. Question: for each of the five Harnesses, what exactly
does that box have to contain, and can the user get there by *telling* their
running agent about fidget instead of restarting it with the details?

**Answer.** Every one of the five takes the loopback HTTP endpoint with
`Authorization: Bearer <token>` as an ordinary configured header, so the stdio
shim (ADR-0026) is a fallback here and not a necessity — four of the five were
driven at a replica of `mcp_http.rs`'s gate on this machine and connected, and
none of them sends an `Origin` header, so the DNS-rebinding refusal never fires.
Registration is a config file or a CLI that writes one, read when the harness
process starts; **no harness has a register-an-MCP-server tool the model can
call, so prompting is not a route on any of them**. Only hermes picks up a
change without a restart, and it does that by polling its own config file, not
by being told. The blocking problem is not any harness: it is that fidget has
no surface that reveals the token, so **not one of the procedures below can be
completed by a user today**, and every fix for that trades against the property
`mcp_http.rs` and ADR-0026 both name — 32 fresh bytes a run that never reach
disk, the Action Log, or a trace.

Everything marked *ran* below was executed on this machine against a throwaway
config directory; everything marked *read* is source or shipped documentation.
The two are never mixed in one claim.

The probes were run at `d55beb74`. Every claim this document makes about
fidget's own source was re-checked against `772026cf` before it was
published: `Endpoint` still derives `Clone` alone, and `authorization()` and
`stdio_env()` are still its only two ways out.

## The five, side by side

| Harness | HTTP + bearer header | stdio + env | Registration lives in | Mid-session pickup | Evidence |
|---|---|---|---|---|---|
| Claude Code 2.1.266 | ✅ header sent on every request | ✅ `env` map | `~/.claude.json` (`-s user`), `projects[cwd]` (`-s local`), `.mcp.json` (`-s project`) | ❌ restart; `/mcp` does reconnect/enable/disable only | ran |
| hermes 0.18.2 | ✅ `headers:` map into httpx | ✅ `env:` merged over an allowlist | `~/.hermes/config.yaml`, `mcp_servers:` | ✅ 5s file watcher reloads | ran (watcher read) |
| opencode 1.18.30 | ✅ `headers` into both transports | ✅ `environment` over `process.env` | `./opencode.json` or `~/.config/opencode/opencode.json`, key `mcp` | ❌ "not hot-reloaded… quit and restart" | ran |
| grok 1.0.13 | ✅ `[mcp_servers.x.headers]`, `${VAR}` expanded | ✅ `[mcp_servers.x.env]` | `~/.grok/config.toml` (`--scope user`) | ~ `/mcps` then `r` | ran (refresh read) |
| codex (not installed) | ✅ `http_headers` map, per source | ✅ `env` map, env cleared then rebuilt | `~/.codex/config.toml`, or `-c` overrides per launch | ❌ `/mcp` is list-only | read only |

Two shared facts worth pulling out of the table. First, none of the four
harnesses present here put an `Origin` header on the wire — captured
byte-for-byte on all four — and codex's dynamic-header helper explicitly rejects
a header named `origin` as reserved (read). Second, the `405` on GET that
`mcp_http.rs` returns because there is nothing to stream is treated as "this
server does not stream" and not as a failure: Claude Code and opencode both open
that GET, get a non-2xx, and still report the server connected (ran). hermes's
content-type preflight also tolerates it, because it only judges 2xx responses
(ran). grok never opened a GET at all in the handshake observed here.

The stdio shim works everywhere too — all four spawned a child and delivered
`FIDGET_MCP_URL` and `FIDGET_MCP_TOKEN` intact (ran) — but it costs an
absolute path to a binary the user has to locate, and buys nothing the header
route does not already give. Offer it as the alternative, never as the default.

## Prompting is not a route, on any of them

This section was carried into `byo-harness-mcp-reload.md` §6 by #743 and is not
repeated here. The short form: no harness has a register-an-MCP-server tool the
model can call, because the server list is assembled from configuration at
process start and the model's tools are fixed for the turn. hermes and opencode
are the two exceptions worth knowing, and neither is a prompt — hermes polls its
own config file, and opencode's `POST /mcp` is an API call needing an address
fidget does not have. Read §6 for the evidence.

The mid-session column of the table above is the same finding per harness.
## What the Settings box has to contain

The generated value in all five cases is the pair (`URL`, raw token). Emit the
token **without** the `Bearer ` prefix: three of the five templates add it
themselves, and a token that arrives pre-prefixed produces `Bearer Bearer …` in
the two that do not.

**Claude Code** — one box, two lines, and the first line is not optional:

```
claude mcp remove -s user fidget 2>/dev/null
claude mcp add -s user --transport http fidget "<URL>" --header "Authorization: Bearer <TOKEN>"
```

Re-adding an existing name is silently ignored — the second `claude mcp add`
printed "MCP server fidget already exists in user config", exited 0, and left
the stale URL and token in place (ran). Since both values change every app run,
the remove is what makes the snippet re-runnable. Do not offer `-s project`: it
writes a checked-in `.mcp.json` and would commit the token.

**hermes** — a YAML fragment, not a command, and this is the first place one box
is not enough:

```yaml
mcp_servers:
  fidget:
    url: "<URL>"
    headers:
      Authorization: "Bearer <TOKEN>"
```

If `mcp_servers:` already exists in their `config.yaml`, only the indented
`fidget:` block is pasted, under it. The box therefore needs an instruction
beside it, and the indentation has to survive the clipboard exactly. `hermes mcp
add` exists but prompts interactively for the token (read), so it is not the
thing to generate.

**opencode** — a JSON object, and the second place one box is not enough:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "fidget": {
      "type": "remote",
      "url": "<URL>",
      "enabled": true,
      "oauth": false,
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

A user with an existing `opencode.json` must merge the `mcp` key rather than
replace the file, and a malformed result is not a degraded config but a refused
startup — unknown top-level keys raise `ConfigInvalidError` (read). `"oauth":
false` is worth emitting and is the reason to prefer the JSON over the one-line
`opencode mcp add`, which cannot express it: `mcp_http.rs` answers a bad token
with a bare 401 and no `WWW-Authenticate`, and opencode's SDK only enters OAuth
discovery when that header is present (read), so the field turns an
already-unlikely "run `opencode mcp auth`" toast into an impossible one.

**grok** — one box, one line, no remove needed; `grok mcp add` is documented as
"Add or update", so re-running it overwrites in place (ran):

```
grok mcp add fidget "<URL>" --transport http --header "Authorization: Bearer <TOKEN>"
```

**codex** — one box, and the launch-flag form is the right default because it
writes nothing:

```
codex -c 'mcp_servers.fidget.url="<URL>"' -c 'mcp_servers.fidget.http_headers.Authorization="Bearer <TOKEN>"'
```

The `~/.codex/config.toml` equivalent is `url` plus `http_headers = {
Authorization = "Bearer <TOKEN>" }` under `[mcp_servers.fidget]`. Do not emit
`bearer_token = "…"`: codex rejects a literal token field on both transports
(read).

### Where one box is not enough

Three cases, and they are all instructions rather than second snippets:

1. **The file-fragment harnesses:** hermes and opencode paste into a file that
   may already exist, so the box's content is a fragment to merge and the copy
   needs to say so. The command harnesses (Claude Code, grok, codex) do not.
2. **The restart line:** Claude Code, opencode and codex read config at process
   start, so the snippet is inert until the harness restarts. hermes reloads
   within five seconds; grok needs `/mcps` then `r` in a live session.
3. **The verify line:** Each has a cheap non-interactive check that connects and
   reports, which is the best thing to put under the box: `claude mcp list`,
   `hermes mcp test fidget`, `opencode mcp list`, `grok mcp doctor fidget`,
   `codex mcp get fidget`. The first four were run here; they print a
   connected/failed line and a tool count without starting a session.

And one line every harness needs: the snippet is valid for **this app run only**.
Both halves of it change when fidget restarts, so the box is something the user
comes back to, not a one-time setup step. For four of the five that also means a
harness restart each time. Only hermes makes it a paste.

## The token problem

None of the above is reachable today, because fidget does not tell anyone the
token.

`Endpoint` in `src-tauri/src/mcp_http.rs` implements neither `Debug` nor
`Serialize`, and the token leaves it through exactly two methods — `authorization()`
and `stdio_env()` — both called only from `harness.rs`, and only to compose the
`mcpServers` entry for a Harness fidget spawned itself. There is no Tauri
command, no settings row, and no CLI flag that prints it. The startup line does
print the endpoint *URL*, and only when a Harness is attached; in the BYO case
nothing is attached, so the user has neither half of the pair. That is the design
working as ADR-0023 and ADR-0026 intended for the spawned case, and it is exactly
what story 66 cannot live with.

A second process cannot solve this: `fidget --print-mcp-endpoint` would be a
different process with a different token, and making it ask the running app means
an unauthenticated loopback route that hands out the credential — strictly worse
than any option below. The reveal has to come from the running app's own UI.

The options, and what each costs:

1. **Reveal it in the generated snippet, in Settings:** Cheapest, and it is what
   the Advanced section already assumes. The token then lives in the clipboard,
   and for four of five harnesses in a config file the user owns. ADR-0010's
   rule 7 — do not log, print, or fingerprint a credential — governs a *Harness's*
   credential; this is fidget's own, and 0010's extension to it in ADR-0026 is
   by analogy, so this is a decision available without amending 0010. What it
   does retire is ADR-0026's consequence "The token remains absent from disk",
   for the BYO path only. Say it in the UI in one line: fidget keeps the token
   out of its own files, and this procedure puts it in yours.
2. **Reveal it, and steer away from the shared-file scopes:** The same as 1 plus
   refusing to generate `claude mcp add -s project` or grok's `--scope project`,
   both of which write a committable file. Costs nothing and removes the only
   failure mode here that leaves the token somewhere the user did not expect.
3. **A stable token on disk, so the snippet survives restarts:** The largest UX
   win — no re-paste, no harness restart — and it reverses ADR-0026's rejection of
   a descriptor file on its own terms: permissions, staleness after a crash, and
   cleanup on exit, none of which the environment needed. It also only pays off
   fully alongside a fixed port. Not a settings change; a new ADR.
4. **A fixed port with a per-run token:** Halves the churn, since only one half
   of the snippet moves, and leaves the "dies with the app run" property intact.
   The cost is a well-known loopback port to collide with and to squat on, which
   is a worse listener than an ephemeral one for something whose only defence
   past the bind is the token.
5. **Do not reveal it, and decline story 66:** Free, honest, and available: the
   attached-Harness path already reaches every tool, and this only closes the door
   for a user who wants their own agent.

The lazy reading of this list is 1 plus 2 — the Advanced section as designed, with
a sentence about the file and no project-scope snippets — and 4 held in reserve
for when the per-run re-paste is what people actually complain about. 3 should not
arrive as an implementation detail of a settings pane.

## What is not verified

**codex is entirely unverified.** It is not installed here (`which codex` finds
nothing, `~/.codex` does not exist), so every codex claim in this document is read
from `openai/codex@main` and OpenAI's docs. The one that matters is whether its
rmcp streamable-HTTP client completes a handshake against a JSON-only server that
answers GET with 405 and notifications with an empty 202. That shape is
spec-legal and the other four accept it, but codex has not been shown to. Two
smaller unknowns: whether a dotted `-c` override carrying a capitalized header
name round-trips through the TOML parser, and whether `Op::RefreshMcpServers`
re-reads config from disk — moot, since nothing user-facing triggers it. One
borrowed machine with codex on it settles all three in a minute: run the snippet
above, then `/mcp verbose`.

Beyond codex:

- **Nothing was tested against the running app:** Every probe here drove a
  faithful replica of `answer()` in `mcp_http.rs` — Origin to 403, wrong or
  absent bearer to 401, non-POST to 405, 202 on notifications — because the
  research worktree is unbuilt. A single smoke test against the real binary is
  cheap and should precede shipping the Settings copy.
- **No harness was observed calling a tool:** Handshake and `tools/list` were
  verified end to end on four harnesses; an actual `speak` reaching an Instance
  was not, in any of them.
- **The mid-session claims are the softest part of the table:** hermes's 5-second
  watcher and `/reload-mcp`, grok's `/mcps` refresh, and Claude Code's `/mcp`
  argument list are all read — from source, shipped docs, and embedded strings
  respectively — and no interactive session was started for any harness. Whether
  Claude Code's `/mcp reconnect` picks up a config file edited mid-session is a
  genuine unknown; the copy should say "restart claude" rather than promise it.
- **Claude Code's project-scope approval flow** ("New MCP server found in this
  project") was read from binary strings, not exercised, and does not apply to
  the `user`/`local` scopes the snippet should use.
- **grok in a real session** may open the GET stream its `doctor` handshake did
  not, which `mcp_http.rs` answers 405. Its client handles that per the comment
  there, and the other three harnesses prove the shape, but it was not observed.
