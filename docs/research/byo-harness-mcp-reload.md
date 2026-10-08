# BYO Harness MCP Reconfiguration and Reload

Research on how each Harness handles MCP server **reconfiguration** and **reload** after the URL or bearer token changes — complementing #580's initial registration research.

## Context

Draft PR #599 implements Settings UI that generates MCP registration snippets for BYO (Bring Your Own) harnesses. The fidget MCP endpoint binds a random port and rotates its bearer token every launch, so users need to **reconfigure** their harness each time fidget restarts.

This research answers:
1. How to **add** an HTTP MCP server with custom `Authorization` bearer (CLI / config)
2. How to **reload** after config changes (without full process restart if possible)
3. Does re-adding an existing server name keep stale URL/token?
4. What should fidget generate: CLI snippet, config fragment, or both?

**Product constraint (Oded)**: Prefer MCP **reconfiguration** methods over harness **launch** CLIs. fidget tells users how to point their running Harness at fidget's MCP, not how to launch the Harness itself.

**Correction, 2026-09-22.** The Pi section below is wrong where it matters, and it is labelled **Fact from docs**, so it reads as settled. Pi ships **no MCP client**. It is deliberately barebones, and MCP arrives through an adapter plugin the user installs, such as `pi-mcp-adapter`. "Pi uses standard `.mcp.json` immediately if present" holds only once that plugin is in place, and `/reload`, `/mcp reconnect` and `/mcp enable/disable` are the plugin's commands rather than stock ones. The word plugin appears nowhere below, which is what made the claim misleading. The finding is left as the investigation recorded it, per `docs/agents/docs.md`; this line governs it. All six other Harnesses were hand-verified on 2026-09-21 and 2026-09-22 against their installed CLIs, and their shapes held.

## Summary Table

| Harness | Add with Auth | Reload Without Restart | Re-add Trap | Generate |
|---------|---------------|------------------------|-------------|----------|
| **Claude Code** | `claude mcp add --header "Authorization: Bearer <token>"` | **No** — must exit and restart session | **Yes** — silently keeps stale | CLI with `remove` then `add` (omit `-s`, local default) |
| **Codex** | CLI: `export` + `codex mcp add --bearer-token-env-var`; Alt: config with `http_headers` | **Yes** — `mcpServer/refresh` command | Unknown | CLI `export` + `codex mcp add` (TOML alt) |
| **Hermes** | CLI: `hermes mcp add --auth header`; Alt: config YAML | **Yes** — `/reload-mcp` | Unknown | CLI `hermes mcp add --auth header` (YAML alt) |
| **OpenCode** | CLI: `opencode mcp add --header`; Alt: flat JSON `type:remote` + `oauth:false` | **No** — restart; `/reload` does not exist (§6) | Unknown | CLI `opencode mcp add --header` (flat JSON alt) |
| **Grok** | `grok mcp add` with `--header`, or config TOML | **Yes** — `/mcps` then press `r` | Unknown | CLI `grok mcp add` (default scope: user) |
| **Pi** | Config: `.mcp.json` or `~/.pi/agent/mcp.json` | **Yes** — `/reload` + `/mcp reconnect` | Unknown | Config snippet + `/reload` |

### Legend
- **Fact**: Verified by documentation or execution
- **Inference**: Reasonable deduction from documented behavior
- **Assumption**: Unverified; needs testing with real CLI

## Per-Harness Findings

### Claude Code

**Add with bearer token**: (**Fact**)
```bash
claude mcp add --transport http \
  --header "Authorization: Bearer <token>" \
  fidget "http://127.0.0.1:<port>/mcp"
```

Config lives in:
- `--scope local` (default): `.claude/config/local.json` in project
- `--scope user`: `~/.claude.json` under `mcpServers`
- `--scope project`: `.mcp.json` in project root

**Reload mechanism**: (**Fact**)
- **No native reload without restart:** Must exit session and start new one.
- Config files are read at session startup only.
- `/mcp reconnect` only re-establishes connections for already-configured servers; does **not** pick up new servers or changed URLs/tokens.
- Feature requests exist (#34893, #46426) but not implemented as of 2026-09.

**Re-add trap**: (**Fact**)
```bash
# First add
claude mcp add --transport http --scope user \
  --header "Authorization: Bearer token1" \
  fidget "http://127.0.0.1:58819/mcp"

# fidget restarts, new port and token
# Re-add with same name
claude mcp add --transport http --scope user \
  --header "Authorization: Bearer token2" \
  fidget "http://127.0.0.1:61234/mcp"

# Result: EXIT 0, prints "Server already exists"
# Config KEEPS OLD VALUES (port 58819, token1)
# Must remove first to actually update
```

Issue #580 confirmed this trap: Claude Code silently ignores re-add of an existing name, keeping the stale entry.

**What to generate**: (**Fact** per #599)
```bash
# Remove stale entry, then add fresh one (omit -s to use local default)
claude mcp remove fidget 2>/dev/null
claude mcp add --transport http fidget \
  "http://127.0.0.1:<port>/mcp" \
  --header "Authorization: Bearer <token>"
```

Instructions: "Exit your Claude session and start a new one (`claude`) to connect. Add `-s user` only if you want the same entry in every project (optional)."

---

### Codex

**Add with bearer token**: (**Fact** per #599)

PRIMARY: CLI with environment variable
```bash
export FIDGET_MCP_TOKEN='<token>'
codex mcp add fidget --url 'http://127.0.0.1:<port>/mcp' --bearer-token-env-var FIDGET_MCP_TOKEN
```

ALTERNATIVE: Config `~/.codex/config.toml`
```toml
[mcp_servers.fidget]
url = "http://127.0.0.1:<port>/mcp"
http_headers = { "Authorization" = "Bearer <token>" }
```

**Reload mechanism**: (**Fact** from source)
- **Yes** — `mcpServer/refresh` command triggers hot reload
- Reloads config for active threads, preserves thread-local overrides
- Pull request #8957 added this feature
- Best-effort refresh available; strict mode fails loudly

**Re-add trap**: (**Assumption**)
Unknown. Config file editing would naturally overwrite existing values, but behavior after `mcpServer/refresh` is not documented for URL/token changes specifically.

**What to generate**: (**Fact** per #599)
CLI with environment variable (PRIMARY):
```bash
export FIDGET_MCP_TOKEN='<token>'
codex mcp add fidget --url 'http://127.0.0.1:<port>/mcp' --bearer-token-env-var FIDGET_MCP_TOKEN
```

Instructions: "Run both lines in a terminal where Codex will inherit the environment, then run `mcpServer/refresh` from your Codex session. If `mcpServer/refresh` is unavailable, restart your Codex session. Alternatively, add or update `[mcp_servers.fidget]` in `~/.codex/config.toml` with `http_headers` (not `headers`)."

**Note**: Codex uses `http_headers`, NOT `headers` (which is silently ignored per #599).

---

### Hermes

**Add with bearer token**: (**Fact** per #599)

PRIMARY: CLI with interactive token prompt
```bash
hermes mcp add fidget --url 'http://127.0.0.1:<port>/mcp' --auth header
```

ALTERNATIVE: Config `~/.hermes/config.yaml`
```yaml
mcp_servers:
  fidget:
    url: "http://127.0.0.1:<port>/mcp"
    headers:
      Authorization: "Bearer <token>"
```

**Reload mechanism**: (**Fact**)
- **Yes** — `/reload-mcp` command
- Also: auto-reload watcher detects config changes (30s timeout for interactive flows)
- Pull request #1474 added auto-reload
- Known issue #14716: failed reloads may not retry until config changes again

**Re-add trap**: (**Assumption**)
Unknown. YAML file editing would overwrite existing values. No trap documented similar to Claude's silent ignore.

**What to generate**: (**Fact** per #599)
CLI with interactive token prompt (PRIMARY):
```bash
hermes mcp add fidget --url 'http://127.0.0.1:<port>/mcp' --auth header
```

Instructions: "Run it in a terminal, then paste the raw token (no `Bearer` prefix) at the interactive prompt. This stores `MCP_FIDGET_API_KEY` in `~/.hermes/.env` and adds the header `Bearer ${MCP_FIDGET_API_KEY}`. Then run `/reload-mcp` in your Hermes session. Alternatively, add or update `fidget:` under `mcp_servers:` in `~/.hermes/config.yaml` with YAML format above; keep indentation exact."

---

### OpenCode

**Add with bearer token**: (**Fact** per #599)

PRIMARY: CLI
```bash
opencode mcp add fidget --url 'http://127.0.0.1:<port>/mcp' --header "Authorization=Bearer <token>"
```

ALTERNATIVE: Flat JSON in `opencode.jsonc` (NOT nested `mcp.servers`)
```jsonc
{
  "mcp": {
    "fidget": {
      "type": "remote",
      "url": "http://127.0.0.1:<port>/mcp",
      "oauth": false,
      "headers": {
        "Authorization": "Bearer <token>"
      }
    }
  }
}
```

**Reload mechanism**: (**Fact**)
- **Yes** — `/reload` command
- Reloads entire config (global + project), disposes/recreates instances including MCP
- Issue #6719 implemented this in 2026
- Also: runtime API `PUT /api/mcp/{server}` can add/replace servers

**Re-add trap**: (**Assumption**)
Unknown. JSON file editing would overwrite existing values. No documented trap.

**What to generate**: (**Fact** per #599)
CLI (PRIMARY):
```bash
opencode mcp add fidget --url 'http://127.0.0.1:<port>/mcp' --header "Authorization=Bearer <token>"
```

Instructions: "Run it in a terminal, then restart OpenCode. If OpenCode tries OAuth, set `\"oauth\": false` in the config. Alternatively, merge flat `mcp.fidget` content into `opencode.jsonc` (NOT nested `mcp.servers.<name>` shape); use `type: \"remote\"` and `oauth: false`."

---

### Grok

**Add with bearer token**: (**Fact** from docs)

CLI:
```bash
grok mcp add --transport http \
  --header "Authorization: Bearer <token>" \
  fidget "http://127.0.0.1:<port>/mcp"
```

Config: `~/.grok/config.toml` or `.grok/config.toml` (project-scoped)
```toml
[mcp_servers.fidget]
url = "http://127.0.0.1:<port>/mcp"
headers = { "Authorization" = "Bearer <token>" }
```

**Reload mechanism**: (**Fact**)
- **Yes** — `/mcps` opens MCP management modal
  - Press `r` to refresh config changes
  - Press `Space` to toggle server on/off
  - Press `i` to authenticate OAuth
  - Press `a`/`x` to add/remove
- Also: CLI `grok mcp enable/disable <name>`
- Reload reconnects existing servers and picks up new ones

**Re-add trap**: (**Assumption**)
Unknown. Config editing or `grok mcp add` (if it overwrites) would replace values. No documented trap like Claude's.

**What to generate**: (**Fact** per #599)
CLI (overwrites in place, no remove needed):
```bash
grok mcp add fidget "http://127.0.0.1:<port>/mcp" --transport http \
  --header "Authorization: Bearer <token>"
```

Instructions: "Run it in a terminal; `grok mcp add` overwrites in place, so re-running after a relaunch is enough. Default scope is user (all projects); use `--scope project` for current project only. Then in your live Grok session, run `/mcps` and press `r` to reload."

Config alternative (if preferred):
```toml
[mcp_servers.fidget]
url = "http://127.0.0.1:<port>/mcp"
headers = { "Authorization" = "Bearer <token>" }
```

---

### Pi

**Add with bearer token**: (**Fact** from docs)

Config: `.mcp.json` (project) or `~/.pi/agent/mcp.json` (agent dir)
```json
{
  "mcpServers": {
    "fidget": {
      "url": "http://127.0.0.1:<port>/mcp",
      "headers": {
        "Authorization": "Bearer <token>"
      }
    }
  }
}
```

Pi uses standard `.mcp.json` immediately if present.

**Reload mechanism**: (**Fact**)
- **Yes** — `/reload` picks up config changes
- **Also** — `/mcp reconnect <server>` or `/mcp reconnect` (all) refreshes tool metadata
- `/mcp enable/disable <server>` toggles without editing config
- Changes take effect on next `/reload`

**Re-add trap**: (**Assumption**)
Unknown. JSON file editing would overwrite existing values. No documented trap.

**What to generate**: (**Inference**)
JSON config fragment:
```json
{
  "mcpServers": {
    "fidget": {
      "url": "http://127.0.0.1:<port>/mcp",
      "headers": {
        "Authorization": "Bearer <token>"
      }
    }
  }
}
```

Instructions: "Add or update this in `.mcp.json` (project) or `~/.pi/agent/mcp.json`, then run `/reload` followed by `/mcp reconnect fidget` in your Pi session."

---

## Cross-Cutting Findings

### 1. All Support Bearer Token in Config

**Fact**: Every harness supports custom `Authorization` headers, either via:
- CLI flag: `--header "Authorization: Bearer <token>"`
- Config field: `headers` map/object with `Authorization` key

This confirms #580's finding: the stdio shim is never required for bearer auth.

### 2. Reload Support Varies Widely

| Has Native Reload | Harness | Method |
|-------------------|---------|--------|
| ✅ Yes | Hermes | `/reload-mcp` |
| ✅ Yes | Grok | `/mcps` + `r` |
| ✅ Yes | Pi | `/reload` + `/mcp reconnect` |
| ✅ Yes | Codex | `mcpServer/refresh` |
| ❌ **No** | **Claude Code** | Must restart session |
| ❌ **No** | **OpenCode** | Must restart; see §6 |

**Fact**: Four of six harnesses support mid-session reload. Claude Code and OpenCode both require a restart; OpenCode's entry was corrected in §6 after a live run.

### 3. The Claude Code Re-Add Trap

**Fact** (#580): `claude mcp add` with an existing name exits 0 but **silently keeps the old URL and token**. This is unique to Claude Code; other harnesses either:
- Overwrite via config file edit (Hermes, OpenCode, Grok, Pi, Codex)
- Or behavior is unknown but no trap documented

**Implication for fidget**: Generated snippet for Claude Code **must** include `claude mcp remove` first.

### 4. Config File Locations

| Harness | User/Global Config | Project Config |
|---------|-------------------|----------------|
| Claude Code | `~/.claude.json` | `.mcp.json` or `.claude/config/local.json` |
| Codex | `~/.codex/config.toml` | — |
| Hermes | `~/.hermes/config.yaml` | — |
| OpenCode | `~/.config/opencode/opencode.jsonc` | `opencode.jsonc` |
| Grok | `~/.grok/config.toml` | `.grok/config.toml` |
| Pi | `~/.pi/agent/mcp.json` | `.mcp.json` or `.pi/mcp.json` |

### 5. Session Lifecycle vs Process Lifecycle

**Key distinction**:
- **Claude Code session** = one `claude` invocation. Exiting and re-running `claude` is a "restart." No window/app to close.
- **Other harnesses**: Similar — terminal-based sessions where "restart" means exit and re-invoke the command.

This means "restart" is less disruptive than it sounds: the user types the command again, doesn't lose unsaved work in an app, and conversation history may be preserved depending on the harness.

### 6. Prompting Is Not a Route

**Fact**: No harness has a register-an-MCP-server tool the model can call, so a
user cannot hand their running agent the URL and token in conversation. The
reason is the same every time: the MCP server list is assembled from
configuration when the process starts, and the tools the model can see are fixed
for the turn.

The model-facing tool surfaces say so directly. Claude Code's in-session `/mcp`
takes `[reconnect <server>|enable|disable [<server>|all]]` and nothing else (read
from the binary). OpenCode's builtin tools are `bash, edit, glob, grep, list,
patch, read, task, todowrite, webfetch, write`, with no MCP verb among them
(read). Grok's only MCP-facing tools are `search_tool` and `use_tool`, over
servers that are *already* enabled (read). Codex's `/mcp [verbose]` only lists
(read).

A prompt can of course make the agent shell out to `claude mcp add` or `grok mcp
add`. That writes the same file the user would have pasted into and still needs
the restart or the manual refresh afterwards — a longer path to the same place,
with a live agent editing config as the extra risk. Tell the user to paste one
line instead.

Two exceptions are worth knowing, and neither changes the recommendation.

- **Hermes needs no restart, and not because anyone told it anything:** It stats
  `config.yaml` every 5 seconds, and when the `mcp_servers` section specifically
  differs it disconnects, re-reads, reconnects, and refreshes the agent's tool
  list (read, in `cli.py`). So for Hermes the whole procedure is "save the file",
  mid-session included. `/reload-mcp` forces the same thing and warns that it
  invalidates the provider prompt cache.
- **OpenCode has a genuinely dynamic path, and it is an API call rather than a
  prompt.** `POST /mcp` (`mcp.add`) on a running OpenCode server registers a
  server in memory, taking the same headers and environment (read). It does not
  persist, and it needs that server's address, which fidget does not have in
  the BYO scenario. Not a fallback; recorded so nobody rediscovers it as one.

**Implication for fidget**: the Settings surface generates a snippet the user
pastes. There is no version of this where fidget talks the harness into
registering itself, so nothing should be designed on the assumption that one
arrives later.

#### OpenCode does not reload, and the `/reload` in §2 was never shipped

The §2 table originally credited OpenCode with `/reload`, while the earlier
registration pass recorded the opposite from a live run. Settled by running it
again on OpenCode 1.18.30, and the registration pass was right.

**Fact** (ran, 2026-09-16): a headless `opencode serve` was started against a
throwaway config holding one MCP server, `alpha`. A second server, `beta`, was
then written into that same config file with the process left running. Three
seconds later `GET /mcp` still reported only `alpha`, and `GET /config` still
returned an `mcp` object with `alpha` alone in it. The configuration is read
once at start and cached; a file the user edits mid-session is not seen.

**Fact** (read): `sst/opencode#6719`, cited in this document's own sources, is
titled *"[FEATURE]: slash command for reload"* and is **open**. It is a request
for `/reload`, not documentation of it. That is where the ✅ came from.

**Fact** (ran, 2026-09-16): `POST /mcp` (`mcp.add`) does work, exactly as §6
describes. Posting `gamma` to a running server registered it immediately and
OpenCode attempted the connection, with no restart and no config write. The
server list also carries `POST /mcp/{name}/connect` and
`POST /mcp/{name}/disconnect`, so a registered server can be reconnected in
place. None of this rescues the BYO case: every route needs the address of the
user's own OpenCode server, which fidget does not have.

**Implication for fidget**: generate OpenCode's snippet with a restart
instruction, alongside Claude Code's. Two of six, not one.

Grok's refresh remains read from its shipped documentation rather than
exercised, which matches the `~` this research already gives it.

---

## Recommendations for PR #599 Settings UI

### 1. Generate Appropriate Snippet Per Harness

| Harness | Generate | Format |
|---------|----------|--------|
| Claude Code | CLI with `remove` + `add` (omit `-s`) | Bash |
| Codex | CLI with `export` + `codex mcp add --bearer-token-env-var` | Bash (TOML alt) |
| Hermes | CLI `hermes mcp add --auth header` (interactive token) | Bash (YAML alt) |
| OpenCode | CLI `opencode mcp add --header` | Bash (flat JSON alt) |
| Grok | CLI `grok mcp add` (default scope: user) | Bash (TOML alt) |
| Pi | Config JSON fragment | JSON |

**Rationale** (per #599 Spec):
- CLI is PRIMARY for Claude, Codex, Hermes, OpenCode, Grok (matches actual CLI availability and product constraint to prefer reconfiguration over launch)
- Config snippets are ALTERNATIVES for Codex/Hermes/OpenCode (when CLI unavailable or user prefers file editing)
- Pi has no CLI, remains JSON fragment only

### 2. Include Reload Instructions Per Harness

| Harness | Reload Instruction |
|---------|-------------------|
| Claude Code | "Exit your Claude session and start a new one with `claude`." |
| Codex | "Run `mcpServer/refresh` from your Codex session." (or "Restart your Codex session" if refresh unavailable) |
| Hermes | "Run `/reload-mcp` in your Hermes session." |
| OpenCode | "Restart OpenCode." |
| Grok | "Run `/mcps`, then press `r` to reload." |
| Pi | "Run `/reload`, then `/mcp reconnect fidget`." |

### 3. Tooltip: "Token changes every launch"

**Fact**: fidget's MCP endpoint binds `127.0.0.1:0` (random port) and generates a fresh 32-byte token per run. Decided by @omesser on #577: surface this in a tooltip.

Suggested text: "The URL and token change every time fidget launches. Re-run this command/snippet each time."

### 4. Claude Code Snippet Must Remove First (and Omit Scope)

**Critical**: Because `claude mcp add` silently keeps stale values when the name exists, the generated snippet **must** remove first:

```bash
claude mcp remove fidget 2>/dev/null
claude mcp add --transport http fidget \
  "http://127.0.0.1:<port>/mcp" \
  --header "Authorization: Bearer <token>"
```

The `2>/dev/null` suppresses "not found" errors when the entry doesn't exist yet.

**Scope**: Omit `-s` to use local default (project-scoped). `-s user` is optional only if the user wants the same entry in every project (#599).

### 5. Scope Selection (Claude Code)

Claude Code has three scopes: `local` (project-only, default when omitted), `user` (all projects), `project` (shared with teammates).

**Recommendation per #599**: Omit `-s` to use **local** default. Reasoning:
- `local` (default): Project-scoped, appropriate for BYO scenarios where token is per-run
- `user`: Only when user explicitly wants the same entry in every project (optional, mentioned in instructions)
- `project`: Deliberately excluded — writes checked-in `.mcp.json`, which would commit the per-run token

If scope selector is added to UI, offer `local` (default) and `user` (optional); `project` is inappropriate for per-run credentials.

### 6. CLI-First with Config Alternative

Per #599 Spec, most harnesses show **CLI as PRIMARY**, config as **ALTERNATIVE**:

**CLI snippets** (Claude, Codex, Hermes, OpenCode, Grok):
1. Command line ready to run in terminal
2. Instructions for reload mechanism
3. Alternative config format noted in instructions

**Config snippets** (Pi only):
1. The **minimal addition** — just the `fidget` server entry
2. Path where it goes: "`.mcp.json` (project) or `~/.pi/agent/mcp.json`"
3. Merge instructions: "Add or replace the `fidget` entry."

Example CLI-first (Codex):
```bash
export FIDGET_MCP_TOKEN='<token>'
codex mcp add fidget --url 'http://127.0.0.1:<port>/mcp' --bearer-token-env-var FIDGET_MCP_TOKEN
```
Instructions: "Run both lines... Alternatively, add `[mcp_servers.fidget]` in `~/.codex/config.toml` with `http_headers`..."

### 7. Consider "Custom" Fallback

For harnesses not in the picker (future-proofing), or if a user runs a different CLI:

**Generate**: Generic HTTP MCP registration shape
```
URL: http://127.0.0.1:<port>/mcp
Authorization: Bearer <token>
```

Instructions: "Configure your harness to connect to this HTTP MCP server with the Authorization header."

---

## Comparison to #580 (Initial Registration Research)

#580 answered: "How to **initially register** an MCP server with bearer auth?"

This research answers: "How to **reconfigure** after the URL/token changes?"

Key difference:
- **Registration** (one-time): Which config file, what syntax, does it work at all?
- **Reconfiguration** (every fidget launch): Can you reload without restart? Do you need to remove first?

Findings that changed:
- **Reload is possible** on 5/6 harnesses (only Claude Code requires restart)
- **Claude Code re-add trap confirmed**: Must remove before add
- **Config snippets are viable** for most harnesses (not just CLI)

---

## Open Questions (For Future Testing)

1. **Codex `mcpServer/refresh`**: Does it pick up changed URL/token for an existing server name, or does it have a trap like Claude Code?
2. **Hermes auto-reload**: What happens if reload fails (issue #14716)? Does manual `/reload-mcp` recover?
3. **Grok `grok mcp add` re-add**: Does it overwrite or ignore like Claude Code?
4. **Pi `/mcp reconnect` necessity**: Is `/reload` alone sufficient, or is `/mcp reconnect` required for tool list refresh?
5. **All harnesses**: If the fidget process dies while the harness is connected, how do they recover? Auto-reconnect vs manual?

These are answerable with execution once harness CLIs are installed.

---

## References

### Documentation Sources

- **Claude Code MCP docs**: https://code.claude.com/docs/en/mcp
- **Claude Code MCP quickstart**: https://code.claude.com/docs/en/mcp-quickstart
- **Claude Code issue #34893**: [Feature Request] Add `/restart` command to reload MCPs
- **Claude Code issue #46426**: Feature request: hot-reload MCP servers without restarting
- **Hermes MCP docs**: https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp
- **Hermes config reference**: https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference
- **Hermes PR #1474**: fix: auto-reload MCP tools when mcp_servers config changes
- **Hermes issue #14716**: MCP config watcher records failed reloads as applied
- **OpenCode MCP docs**: https://opencode.ai/v2/docs/mcp-servers/
- **OpenCode migrate V1**: https://opencode.ai/v2/docs/migrate-v1/
- **OpenCode issue #6719**: [FEATURE] slash command for reload
- **Grok MCP docs**: https://docs.x.ai/build/features/mcp-servers
- **Grok commands cheat sheet**: https://toolsbase.dev/en/reference/grok-build-commands
- **Codex source**: `codex-rs/app-server/src/mcp_refresh.rs`
- **Codex PR #8957**: feat: hot reload mcp servers
- **Codex issue #21055**: Preserve session MCP config on refresh
- **Pi MCP adapter**: https://pi.dev/packages/pi-mcp-adapter
- **Pi commands docs**: https://mintlify.wiki/nicobailon/pi-mcp-adapter/usage/commands

### Related Fidget Issues/PRs

- **#577**: Surface the MCP endpoint so a Harness you run yourself can reach fidget
- **#580**: spike(harness): How each Harness registers a BYO MCP server
- **#599**: feat(settings): Generate the MCP registration for a Harness you run yourself (draft PR)
- **ADR-0010**: Credential rules, loopback-only MCP, token stays out of logs
- **ADR-0023**: Dispatch inside the running app, loopback HTTP MCP
- **ADR-0026**: stdio MCP binary is a relay shim

---

## Change Log

- **2026-09-12**: Initial research document created. Covers six harnesses (Claude Code, Codex, Hermes, OpenCode, Grok, Pi). Documented reload mechanisms, re-add trap (Claude Code), and config locations. Recommendations for PR #599 Settings UI.
- **2026-09-16**: Added §6, Prompting Is Not a Route, carried over from an earlier registration pass (branch `research/byo-harness-mcp`) that never reached a pull request. That pass covered five harnesses — Claude Code, Hermes, OpenCode, Grok and Codex — and did not examine Pi, so §6 says nothing about Pi. It also contradicted this document on OpenCode's mid-session reload.
- **2026-09-16**: Settled that contradiction by running OpenCode 1.18.30 again. It does not re-read its config mid-session, and the `/reload` this document credited it with is an open feature request (`sst/opencode#6719`), not a shipped command. Corrected the summary table, the reload table, the per-harness instruction, and the #599 recommendation. Confirmed `POST /mcp` in-memory registration at the same time.

_— Cursor agent (Coder), on [@omesser](https://github.com/omesser)'s behalf._
