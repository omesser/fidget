# Harness Reference

How Fidget talks to a Completer or a Harness at runtime, and the MCP server a Harness calls back into; contributor setup is in [DEVELOPMENT.md](./DEVELOPMENT.md).

## Running with a Completer

With no Director key, Static weights pick idle Behaviors from the Character's manifest. No model, no account, no permission. Connect a Completer for model-driven variety.

### Quick Start

OpenAI, Anthropic, and Ollama use `/v1/chat/completions`. [xAI](https://docs.x.ai/developers/model-capabilities/text/comparison) uses `/v1/responses`, selected by `FIDGET_DIRECTOR_BASE_URL=https://api.x.ai`. A full URL ending in `/chat/completions` or `/responses` is used as-is.

```sh
# OpenAI
cd src-tauri
FIDGET_DIRECTOR_API_KEY="$OPENAI_API_KEY" \
FIDGET_DIRECTOR_BASE_URL=https://api.openai.com \
FIDGET_DIRECTOR_MODEL=gpt-4o-mini \
cargo run

# Anthropic (OpenAI-compatible /v1/chat/completions)
cd src-tauri
FIDGET_DIRECTOR_API_KEY="$ANTHROPIC_API_KEY" \
FIDGET_DIRECTOR_BASE_URL=https://api.anthropic.com \
FIDGET_DIRECTOR_MODEL=claude-haiku-4-5 \
cargo run

# xAI — get a key at https://console.x.ai
cd src-tauri
FIDGET_DIRECTOR_API_KEY="$XAI_API_KEY" \
FIDGET_DIRECTOR_BASE_URL=https://api.x.ai \
FIDGET_DIRECTOR_MODEL=grok-4.6 \
cargo run

# Ollama (local, no key)
cd src-tauri
FIDGET_DIRECTOR_BASE_URL=http://localhost:11434 \
FIDGET_DIRECTOR_MODEL=gemma4 \
cargo run
```

### Director Environment

Switches read the same words as the trace variables. Any other value is a typo: the switch stays as Settings has it, and the launch prints a line naming the variable it ignored. An empty value is no override.

| Variable | What it does |
|---|---|
| `FIDGET_DIRECTOR_API_KEY` | Required for a remote provider. For a local server, set it only when the server requires auth. Empty or unset with a remote URL means Static only. |
| `FIDGET_DIRECTOR_BASE_URL` | Provider origin. Default `https://api.openai.com`. |
| `FIDGET_DIRECTOR_MODEL` | Model name. Leave blank or whitespace to leave the model unset: HTTP omits the `model` field, and a Harness does not set one. A non-empty value is that string on the HTTP body, or `session/set_config_option` on the first option with category `model` when the Harness lists one. |
| `FIDGET_DIRECTOR` | The Director on or off, whatever Settings saved. Off keeps Static even with a key; on still needs a key or a local server. The window and the tray name the variable and disable the toggle. |
| `FIDGET_DIRECTOR_TIMEOUT_SECS` | Timeout for one HTTP Completer request, then Static. Default 30 seconds. Raise it for a cold local server. A Harness turn uses `FIDGET_HARNESS_TURN_TIMEOUT`. |
| `FIDGET_DIRECTOR_MAX_TOKENS` | Ceiling on one HTTP Completer turn. A safeguard against a model that will not stop, not a reply-length budget. Default 1024, or 8192 once the endpoint has been seen to mark its thinking (#606). A value here outranks both. A Harness decides its own reply length. |
| `FIDGET_DIRECTOR_BLANK` | Blank-AI mode, same as Settings → Development → "Blank AI". Empties the built-in Personality Prompt and the app-level instructions (voice rules, behavior list, reply contract) and still sends the Instance Prompt. The Prompt tab marks emptied layers Empty. With no contract the reply is prose, spoken without acting unless the Instance Prompt asks otherwise. The Harness lane keeps a separate session for this mode. |
| `FIDGET_DIRECTOR_REASONING_EFFORT` | Reasoning effort for the Completer in use. Overrides Settings → Development → "Reasoning effort". Leave blank to leave effort unset (not `low`): HTTP omits `reasoning_effort` and `reasoning.effort`, and a Harness does not call `session/set_config_option` for effort. A typed value is sent as written. The picker lists `low`, `medium`, and `high`. Other values are allowed and not checked. On a Harness the value is set on the `thought_level` option, or on `model_config` if that is what the Harness lists, and after the model option when both are set. If neither option exists, Fidget skips the call. If the HTTP host rejects the field, Fidget drops it for the session and `FIDGET_TRACE_DIRECTOR` logs the value. If a Harness rejects the value, Chat shows the error. |
| `FIDGET_HARNESS` | Attach a Harness as the Completer: `claude`, `codex`, `copilot`, `cursor-agent`, `goose`, `grok`, `hermes`, `opencode`, `pi`, `antigravity`, or any command line that speaks ACP on stdio. The Harness signs in on its own; a not-signed-in one is named in Chat with the command that fixes it. Set and empty is the kill switch (Off, whatever Settings saved). Unset falls through to Settings → AI → AI source. ADR-0022. |
| `FIDGET_HARNESS_CWD` | The Harness's project directory: spawn `current_dir` and ACP `session/new` / `session/load` cwd. Empty means the data folder, not `$HOME`. Overrides Settings → Development → "Working directory". The session file and Action Log stay in the data folder. |
| `FIDGET_HARNESS_TURN_TIMEOUT` | How long a Harness `session/prompt` may run before `session/cancel`, excluding time spent waiting on your answer to an ask, and restarted when you answer. Overrides Settings → Development → "Turn timeout, in seconds". Default 120. Read at attach. |
| `FIDGET_HARNESS_AUTH_RETRY_SECS` | How long to leave a not-signed-in Harness before retrying `session/new`. Overrides Settings → Development → "Auth retry, in seconds". Default 60. Read at attach. |
| `FIDGET_MCP_URL`, `FIDGET_MCP_TOKEN` | The app's loopback MCP endpoint and per-run bearer token. The app sets both on the stdio MCP entry it hands the Harness; with neither set the shim fails every call. Set them by hand to point the shim at a running app. Never written to a file or a log. ADR-0026. |
| `FIDGET_MCP_BIN` | The stdio MCP server binary, when it is not beside the app. Overrides Settings → Development → "MCP server binary". Read at attach. See [MCP Server](#mcp-server) for the three stdio routes. |
| `FIDGET_DIRECTOR_WAKE_SECS` | First proactive model-call wait. Overrides Settings → AI → "First wake, in seconds". Default 120. Each proactive call multiplies the wait by the Character's `[director]` `model_base ^ model_power` (default doubling), capped at two hours. Poke and Summon wake immediately. |

### Settings and Keyring

Settings → AI saves base URL, model, and first wake interval, and stores the API key in the OS secret store. Leave model blank to leave it unset: HTTP omits `model`, and a Harness does not set one. Settings → Development saves the Model API timeout and turn ceiling, Blank AI, Reasoning effort, and the Harness turn timeout, auth-retry interval, MCP server binary, and working directory. Leave effort blank to leave it unset on HTTP and on a Harness. A Harness gets the model and the effort through `session/set_config_option`, when Fidget opens a conversation and again when Apply changes either one. That Apply cancels the Harness turn still running. A conversation already open in this run gets the new values at its next wake and keeps its id. One not opened yet this run loads its saved id and gets the values the same way. A value blanked in Settings cannot be set back to the Harness default, so the next wake of an open conversation opens a new one instead. An Apply that changes neither value sets nothing and cancels nothing. See #1430 and #1434.

Apply returns before the Harness answers, because the set goes out at the next wake. A value the Harness does not take therefore reaches Settings later. Three cases count. The Harness refused `session/set_config_option` and gave a reason. The Harness answered success, but its returned option still lists the old value. The session lists no option for that field.

The wake that finds one restores the saved value before it returns, in memory and in `settings.json`. The next wake does not ask again, and a restart does not bring the value back. The value goes back to the model or effort that last worked on this Harness, or to blank, the Harness default, when none has. The conversation is not recorded as opened on the value that did not take.

Settings shows one notice under the field. It names the value you asked for, or says "the Harness default" when that was blank. It then gives the Harness's reason and says what Fidget put back, for example `Model did not apply. You asked for "gpt-5". The Harness refused it: unknown model. Fidget put back "gpt-4o-mini".` When the session has no such option, the reason is `This Harness doesn't offer a reasoning effort setting, so Fidget can't change it.` An open Settings window redraws at once. A closed one reads the notice from the attached Harness the next time it opens. The notice goes when you change the field again, when a later set is confirmed, or when you pick another Harness.

Settings reports a field with no option only when Apply changes it on a running Harness. At open Fidget only logs it and leaves the value alone. One model and effort setting serves every Harness, so restoring it there would undo a value meant for another.

- A working-directory edit respawns the Harness, so process cwd and ACP cwd stay equal. Turn timeout and auth retry land on the next attach.
- Editing the Completer source or HTTP endpoint retargets the running Director with no restart. The session in flight is dropped; a streaming call closes its connection, so the old host stops generating.
- `cargo run` with the env vars unset uses the saved Completer. A field an env var owns shows its value, names the variable, and takes no edit.
- An exported `FIDGET_DIRECTOR_API_KEY` keeps the Keychain out of the launch entirely.
- Settings → Do Not Disturb → Sound mutes cue audio. On by default; off takes effect on the next frame. Do Not Disturb also silences cues and keeps the visual ones (#277). A machine that cannot start an audio context logs one webview console warning and plays the visual cue silently (#292).
- The window moves with a modifier-drag on empty chrome: Command-drag on macOS, Super-drag on Linux, Alt-drag on Windows. Nothing in the UI names this.

**Linux:** the key goes to Secret Service (GNOME Keyring, KWallet), or kernel keyutils when Secret Service is absent. Building the shell needs `libdbus-1-dev`. No packaged secret store is required.

**macOS Keychain ACL:** a saved key's access list names the build that wrote it. An ad-hoc signature names it by a hash that every `cargo build` changes, so a rebuilt app costs two Keychain dialogs. `scripts/dev-sign.sh` signs with a stable identity instead. From the repo root:

```sh
cargo build -p fidget && scripts/dev-sign.sh && ./target/debug/fidget
```

A key saved before the first signed run keeps the old list: clear it in Settings and save it again. Signing also changes the identity macOS grants Accessibility and Screen Recording to, so expect to grant those once more. Released builds are ad-hoc signed too, so updates prompt the same way until there is a Developer ID (#283).

**Accessibility, Screen Recording, and Input Monitoring:** grant them in Settings → What the fidget can see. The pane names the row macOS will show: a `cargo run` from Cursor is listed as Cursor, a packaged build as Fidget. Check the box, then turn that app on in Privacy & Security. Input Monitoring lets the mouse wake an idle sprite, so a poke lands at once instead of up to a second later. Without it the frame loop keeps its idle back-off. The tap starts within about a second of the grant, with no relaunch, and stops when you uncheck the box.

### Local Model Servers

The fidget wakes all day and every Poke is another wake, so a hosted API meters idling, and each wake sends the clock and, with the window-names consent, the frontmost application, its window title and recent applications off the machine. A server of your own removes the meter. On loopback it also keeps that context on the machine; a LAN box still receives it. "Local" means loopback, an RFC1918 or IPv6 unique-local address, or a `.local` name. A local base URL makes `FIDGET_DIRECTOR_API_KEY` optional.

These servers speak `/v1/chat/completions`:

| Server | Base URL | Model name | Auth | Tested |
|---|---|---|---|---|
| [Ollama](https://ollama.com) | `http://localhost:11434` | a tag: `gemma4`, `llama3.2:3b` | none by default | yes — `gemma4:latest`, 9.6 GB, on an Apple-silicon Mac |
| [oMLX](https://github.com/jundot/omlx) | `http://localhost:8000` | a served model id | API key required | yes |
| [llama.cpp](https://github.com/ggml-org/llama.cpp) `llama-server` | `http://localhost:8080` | the gguf path, or `--alias` | optional `--api-key` | no |
| [LM Studio](https://lmstudio.ai) | `http://localhost:1234` | the id shown in its server tab | optional | no |
| [vLLM](https://docs.vllm.ai) | `http://localhost:8000` | the served model id | optional `--api-key` | no |
| [MLX](https://github.com/ml-explore/mlx-examples) `mlx_lm.server` | `http://localhost:8080` | a Hugging Face repo id | none | no |

**Ollama** (no auth):

```sh
ollama pull gemma4
ollama serve

FIDGET_DIRECTOR_BASE_URL=http://localhost:11434 \
FIDGET_DIRECTOR_MODEL=gemma4 \
cargo run
```

**oMLX** (requires API key):

```sh
omlx serve --model mlx-community/Qwen2.5-1.5B-Instruct-4bit --api-key your-key-here

FIDGET_DIRECTOR_API_KEY="$OMLX_API_KEY" \
FIDGET_DIRECTOR_BASE_URL=http://localhost:8000 \
FIDGET_DIRECTOR_MODEL=gemma-4-e2b-it-4bit \
cargo run --bin fidget
```

### Testing Connectivity

`scripts/probe-model.sh` hits the Completer without starting the overlay: GET `/v1/models` (and `/v1/api-key` on xAI), then both POST paths. It reads the same env as `cargo run`, prints status and body, and never prints the key. It also reports whether the configured model is loaded.

```sh
FIDGET_DIRECTOR_API_KEY="$XAI_API_KEY" \
FIDGET_DIRECTOR_BASE_URL=https://api.x.ai \
FIDGET_DIRECTOR_MODEL=grok-4.6 \
scripts/probe-model.sh

# Ollama (no key)
FIDGET_DIRECTOR_BASE_URL=http://localhost:11434 \
FIDGET_DIRECTOR_MODEL=gemma4 \
scripts/probe-model.sh

# oMLX (with key)
FIDGET_DIRECTOR_API_KEY="$OMLX_API_KEY" \
FIDGET_DIRECTOR_BASE_URL=http://localhost:8000 \
FIDGET_DIRECTOR_MODEL=gemma-4-e2b-it-4bit \
scripts/probe-model.sh
```

The app runs the same check once at startup, in the background, and logs a miss:

```
director: http://localhost:11439 unreachable: Connection refused; staying on StaticDirector until it answers
director: http://localhost:11434 model "llama3.2" is not served; it has gemma4:latest
```

Neither line stops anything. A failed wake already falls back to Static.

`scripts/probe-harness.sh` does the same for a Harness. It serves Fidget's MCP endpoint, spawns the Harness, prints what `initialize` advertised, runs one fixed prompt, and reports whether the reply parsed as a Behavior proposal and whether the Harness fetched the tool list. No overlay, and no credential printed: a Harness that is not signed in comes back as the command to run in your own terminal.

```sh
FIDGET_HARNESS=hermes scripts/probe-harness.sh
```

```
probe-harness
  harness      hermes
  command      hermes acp
  dir          /Users/you/Library/Application Support/fidget/probe
  timeout      turn 20s, attach 20s

attach
  agent        hermes-agent
  loadSession  true
  mcp http     false
  mcp          /path/fidget --mcp-stdio
  authMethods  custom runtime credentials, Configure Hermes provider
  session      33f5d650-5476-40c6-876b-cb04f14bfc27

turn
  prompt       Reply with exactly this one line and nothing else: Wave | Hello from the probe.
  stop         end_turn
  reply        Wave | Hello from the probe.
  proposal     Wave | Hello from the probe.
  mcp listed   yes, 1 tools/list request(s)
```

- `mcp` is what the session was handed. `mcp http` is why: `hermes` advertises no HTTP MCP on ACP `initialize`, so it gets the stdio server, which relays to the app (ADR-0026). A Harness that advertises HTTP MCP shows a `http://127.0.0.1:…/mcp` URL instead, never the token. The probe binds that listener itself before it attaches. A failed bind is reported in capitals under `mcp`, and then nothing it reports about tools holds.
- `FIDGET_PROBE_THEN_CONFIGURE` adds an Apply and a second turn after the first one succeeds. It takes space-separated `model=<id>` and `effort=<level>`, either or both, for example `FIDGET_PROBE_THEN_CONFIGURE='model=<id> effort=high'`. An unknown key, a word without `=`, a repeated key, or an empty value exits 2 without asking. The `apply` block prints the model and effort in force. Its `session` line says whether the conversation kept its id and what reached the wire. A value counts as set only when the Harness answered `session/set_config_option` with success and the returned options list that option at the requested value. The line reads one of these:
  - `(kept, values set on it)` means every value was confirmed.
  - `(kept, NOT CONFIRMED: … was answered without the new value)` means the Harness answered, but the option still reads the old value.
  - `(kept, NOT SET: the Harness advertises no … option)` means the session listed no option, so nothing was sent. `cursor-agent` lists only `modes`.
  - `(kept, NO SET SENT: …)` means nothing changed on the conversation, for example because an exported variable won.

  The probe exits 1 on the last three, because none proved a set. An exported `FIDGET_DIRECTOR_MODEL` or `FIDGET_DIRECTOR_REASONING_EFFORT` wins over the Apply, so leave unset the one you change.
- `mcp listed` is whether the Harness asked for `tools/list`. A tool it calls is answered through the app's `dispatch` against an empty desktop and no Instances, so `speak` fails and `list_windows` is empty. Each call prints as `mcp call`.
- Exit code: 2 means never asked (nothing configured, no binary, not signed in), 1 means asked and not answered, 0 means `end_turn`, a completed turn.
- The `probe` folder keeps the session file and the Action Log away from a real install. Memory is not isolated: a `remember` during a probe writes the real `memory.md`.

Dated transcripts belong on the issue that ran the probe. Update the README's [Harness Support](../README.md#harness-support) table when a row's command or user-visible session behavior changes, not when a probe is re-run.

### Provider Details

**Cursor API:** `CURSOR_API_KEY` is for the Cloud Agents API and SDKs, not a Completer. `https://api.cursor.com` has no `/v1/chat/completions`; a POST there is a 404 and Static takes over.

**xAI keys:** a 403 is xAI refusing the key; a bad body is a 400. Keys are granted per endpoint in [console.x.ai](https://console.x.ai), and `/v1/responses` and `/v1/chat/completions` are separate ACLs. A team that requires mTLS wants `https://mtls.api.x.ai`. The Completer retries chat-completions if Responses returns 403 or 404.

**Streaming:** the Completer asks for `stream: true`. The Behavior name is the first line, so the fidget can start moving before the dialogue arrives. Closing a streaming connection also stops the generation, which a whole-reply request does not. A server that rejects the field, or ignores it, still works: the parser handles both shapes.

### Proactive model calls

Session calls stay quiet while the main display is asleep. Settings can turn the Director off, or keep it on with proactive calls disabled.

A Character that should back off faster or slower than doubling says so:

```toml
[director]
model_base = 3
model_power = 1
```

### Reply Contract Measurements

`measure_the_reply_contract_failure_rate` in `src-tauri/src/model.rs` measures how often a model breaks the reply contract. It is `#[ignore]`d and runs against a live server; its doc comment has the command and the `FIDGET_BENCH_*` knobs. The last published results are in the [README before #135](https://github.com/omesser/fidget/blob/f08c6edffaa4cbc310164a594fe4be889e22f9de/README.md#L438).

## Harness Support

The README's [Harness Support](../README.md#harness-support) table names each Harness, its command and its standing. This section holds the reference behind it.
Named rows are smoked with `scripts/probe-harness.sh` (see [Testing Connectivity](#testing-connectivity)); the run itself lives on the issue that did it.

What each named Harness keeps under an ACP attach, measured in the [tool-class probe](./research/harness-tools-under-acp-probe.md):

| Harness | Under ACP attach |
|---|---|
| `claude` | Keeps shell, web search and fetch, filesystem, and user-scope MCP plus claude.ai connectors, loads local and project MCP only when `cwd` matches, and does not list `AskUserQuestion`. |
| `codex` | Keeps shell, web search and fetch, filesystem, the user's own MCP servers, and `request_user_input`. |
| `copilot` | Keeps shell (`bash`, with `read_bash`, `stop_bash` and `list_bash`), filesystem (`view`, `create`, `edit`, `grep`, `glob`), `web_fetch` and no web-search tool, its subagent set (`task`, `parallel`, `search_code_subagent`, `read_agent`, `list_agents`, `write_agent`), `skill`, `sql` and `session_store_sql`, and five tools from its bundled `github-mcp-server`, and lists no ask-user tool. It takes Fidget's own MCP over HTTP, and lists Fidget's eight tools alongside its own, under an `fidget-` prefix. |
| `cursor-agent` | Keeps shell, web search and fetch, filesystem, and the user's own MCP servers, and lists no ask-user tool. |
| `grok` | Keeps shell, web search and fetch, filesystem, and `ask_user_question`, and the user's own MCP servers were empty on a machine with none configured, and project scope keys off `cwd` per vendor docs. |
| `goose` | Lists eighteen tools of its own: `shell`, the `developer` filesystem set (`edit`, `write`, `load`, `tree`, `read_image`), `analyze`, `delegate`, `load_skill`, and its `apps__`, `todo__` and `extensionmanager__` built-in extensions. No web tool, neither search nor fetch, and no ask-user tool. It takes Fidget's own MCP over HTTP, and lists Fidget's eight tools alongside its own, under an `fidget__` prefix. |
| `opencode` | Keeps shell, web fetch, filesystem, and the user's own MCP servers, and lists no web-search tool and no ask-user tool. |
| `hermes` | Keeps shell, web search and extract, filesystem, and the user's own MCP servers via the vendor mcp subcommand that the probe did not exercise, lists no ask-user tool, and lists browser tools that the start-up CDP check marks unavailable. |
| `pi` | Keeps its own `read`, `bash`, `edit`, and `write` tools, has no web tool, and on `initialize` has `http` and `sse` both false. Handed Fidget's stdio server on `session/new`, it never asked for the tool list: `pi-acp` stores those servers and does not pass them to `pi` (#1019), which reads the project `.mcp.json` the app writes on Apply and the probe does not. |

No Harness brings desktop control to an ACP session Fidget opens.

`scripts/probe-harness.sh` reports under `mcp listed` whether the Harness
fetched Fidget's tool list (#984). Goose, Copilot, Codex, and Antigravity
fetched it on a stock run; Pi did not. The prefixes above come from the tool-class probe's own
client.

Session handling:

| Harness | Fresh session | Resumed session | `loadSession` | MCP transport † | Auth methods ‡ |
|---|---|---|---|---|---|
| `claude` | yes | yes | yes | http | none advertised when signed in |
| `codex` | yes | yes | yes | http | two: API Key, ChatGPT |
| `copilot` | yes | yes | yes | http | one: Log in with Copilot CLI |
| `cursor-agent` | yes | no | no | http, through its own config | one: `cursor_login` |
| `grok` | yes | yes | yes | http | three: xai.api_key, cached_token, Grok |
| `goose` | yes | yes | yes | http | one: Configure Provider |
| `opencode` | yes | yes | yes | http | Login with opencode |
| `hermes` | yes | yes, after the reopen | yes | stdio | two: custom runtime credentials, Configure Hermes provider |
| `pi` | yes | yes | yes | none | `pi_terminal_login` |
| `antigravity` | yes | yes | yes | http | four: Log in with Google, Log in with Gemini Enterprise, Gemini API key, Gemini Enterprise Agent Platform. The last two need a key or cloud project Fidget never sends, so they are not buttons |
| anything else | unverified | unverified | unverified | unverified | unverified |

- † What `initialize` advertised: `claude`, `codex`, `opencode`, `grok`, `goose`, `copilot` and `antigravity` set `agentCapabilities.mcpCapabilities.http` (the running app hands the loopback URL); `hermes` and `pi` omit it and get the stdio binary that relays to the same endpoint (ADR-0023, ADR-0026). `cursor-agent` omits it and ignores `mcpServers`, so Fidget writes the endpoint into `<cwd>/.cursor/mcp.json` instead ([details](#how-cursor-agent-is-reached)). `opencode`, `grok`, `copilot` and `antigravity` also advertise `sse`, which nothing here reads.
- ‡ `authMethods` is what is *available*, not what is outstanding — an empty list is no proof a login is unnecessary. Only `session/new` answering `-32000` is (ADR-0022).

### Finding programs on `PATH`

launchd starts a Finder or Dock launch of Fidget.app with `/usr/bin:/bin:/usr/sbin:/sbin`, and a Linux desktop file often skips `~/.bashrc`. Neither has Homebrew, nvm, volta, or `~/.local/bin` (#1436). So at startup, before it starts any Harness, Fidget runs your `$SHELL -l -i` once and merges the `PATH` it prints into its own. Then it stops that shell and anything its rc files left running. Every Harness, `npx`, and version probe inherits the result.

- **Order:** folders the launcher lists before its first system folder (`/usr/local/bin`, `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`) stay first. The shell's folders come next, then the rest of the launcher's, each once. A login shell rebuilds `PATH` system first (Debian's `/etc/profile`, macOS `path_helper`), so without this a script's own folder would lose to `/usr/bin`. launchd's `PATH` starts with `/usr/bin`, so a Finder launch gets the shell's `PATH` first.
- **Started from a terminal:** Fidget keeps the terminal's `PATH` and runs no shell.
- **Shell slow or broken:** after 5 seconds Fidget kills the shell. It then adds whichever of `/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin`, `~/.cargo/bin`, `~/.volta/bin`, `~/.bun/bin`, `~/.local/share/mise/shims`, and `~/.asdf/shims` exist.
- **Only `PATH`:** other variables in your rc files do not reach Fidget. A provider key exported there would switch a signed-in Harness to API billing, and a `FIDGET_DIRECTOR_*` export would take over its Settings row.
- **Windows:** not affected, because a Windows app reads `PATH` from the registry.

The process log beside Memory has one `path:` line that names which case ran and the `PATH` in force.

`fidget --probe-harness` (`scripts/probe-harness.sh`) returns before this step and keeps the caller's `PATH`, so it does not show what a Finder launch finds.

### Inbound Wake Contract

A Harness can work between Fidget's `session/prompt` calls: Claude Code `/loop` and `CronCreate` fires, Hermes scheduled reminders, and any background agent output arrive as ACP `session/update` notifications while no Fidget turn is open. This is an **inbound wake** — the Harness starting a turn on the one session rather than answering one Fidget asked for (ADR-0008).

#### What counts as inbound wake

Between-turn `session/update` carrying agent text that is flushed to the user. Only agent speech triggers an inbound wake and resets Pace; between-turn thought and tool activity are visible (#1370) but do not count as a wake. Not attribution metadata: usage and latency stay internal.

**Load replay is not a wake:** `session/load` replays the conversation as updates before it answers. Fidget keeps the whole replay as `Replayed` entries, in order, and hands them to that Instance's Chat, above everything else in its log, under "Earlier in this session." (#1393). Each prompt Fidget sent is a folded **Prompt** row. When the user typed a line into it, that line is also the user's row above the folded frame, read back out of the frame after `they said:` (#1435). Thinking, from `agent_thought_chunk` or a reply's `<think>` block, is a folded **Thinking** row (ADR-0034), as live thinking is once its turn lands. A turn's agent messages are one reply row with the Instance's name. A new `messageId` inside that turn is a paragraph break, the way a live turn joins them, and a user prompt starts the next reply. A replay with no user prompt keeps each `messageId` as its own reply. Its Behavior line is read against the Behaviors the Character declares, so the row shows the dialogue as a live reply does, and a reply that names only a Behavior draws no row (#1435). A tool call is a folded row titled with its title and last status. It lists each location as a path and a line when the Harness sent one. Then it lists each piece of content. Text appears as sent. A diff appears as its path, its `(+added/-removed)` line counts and a note that the full diff is not drawn. Only `\n` ends a line, so the counts match `git diff --numstat`. Lines that the old and new text share at the start and the end are skipped. If more than 10,000 lines remain on either side (`MAX_DIFF_LINES` in `tool_content.rs`), Fidget does not diff them. It counts every remaining old line as removed and every remaining new line as added, and the row reads `(+added/-removed, approximate)`. A path, a terminal id and text drop control characters, and a path or an id also loses its line breaks. A terminal appears as `Terminal <id>`, and Fidget draws no live terminal (#1449). Any other block appears as the mark from ADR-0028, and a resource link in it opens. A later update to the call replaces its locations and content whole. A plan is drawn as steps in the log, never into the live plan above the composer. Live Chat draws no row for prompts or tool calls, so those two shapes exist only for replay. No row carries a time, because the replay has none. A new `messageId` inside a turn is a new paragraph in that reply, not another row. With no user prompt in the replay, each `messageId` is its own reply. Restoring never marks the Instance `addressed`, never touches Pace, and never forwards an ask. `session/load` replays updates, not `session/request_permission`, so a replayed permission is its tool call row and has no buttons. Each session id restores once per run, so a respawn that loads the same id again draws nothing new. A load that fails and falls back to `session/new` drops its replay, because the session that answers never said it.

**One reader:** `hear` in `acp_wire.rs` reads each `session/update` once into a `Heard` value. A live turn and a between-turn message pass it to `show`, which raises `Event`s. A load replay passes it to `Restore::take`, which collects the `Replayed` entries. The two sinks differ only in what they do with an update. A live turn joins all its agent messages and sends tool calls as deltas. A replay joins one turn's agent messages into one reply the same way a live turn does, with a paragraph break where `messageId` changes, and folds a tool call into one row. The next user prompt starts the next reply. With no user prompt, each `messageId` is its own reply.

**Non-text content in a message or thought chunk:** An image, audio clip, resource link or embedded resource in an `agent_message_chunk`, `agent_thought_chunk` or `user_message_chunk` becomes one Markdown line in the reply, the Thinking row or the replayed Prompt, as a paragraph of its own: `[image image/png]`, `[audio audio/wav]`, `[resource file:///notes.txt, text/plain, 5 bytes]` (a blob's size is an estimate, `~N bytes`). A resource link to an `http`, `https` or `mailto` URL is a link, `[name](url)`, and a click opens it through `open_link` like any link in a reply. A link to any other scheme, such as `file:`, is text: `[link name file:///x]`. The data is never drawn, only named. Every field in a mark is escaped, so a name cannot close its own label or forge a link. A link whose name reads as a URL on another host is followed by its real target as text. In a Thinking or Prompt row only a link written as `[text](url)` is live, never a bare URL, `www.` host or email. A tool call's content uses the same mark in its replayed row, described under **Load replay is not a wake**.

While a turn is open, one muted line under the answer says `Running: <title>` for the latest call whose status is `in_progress`, and `Waiting` when none is. A call with no title reads `Running: tool`. After `STALL_MS` (30 seconds) with no Harness update the line says `No news for 30s`. The next update puts the line back. A typed turn starts clean. An unprompted row keeps calls that arrived before the row existed. The line is removed when the turn ends. Live Chat still draws no tool-call row (#1446).

#### Interaction with ADR-0008 (one session)

Inbound wakes are still the one Director session per Character Instance. A Harness fire is a turn the Harness started in the same conversation Fidget prompts, not a second session. Inbound wakes are not user-addressed reactive touches and not Fidget's proactive exponential-backoff timer. They participate in Pace on the reactive path: the inbound wake marks the Instance `addressed`, and the following Director wake calls `pace.after_reactive()`, resetting the exponential backoff to the first wait interval.

#### Visibility

- **Chat surface:** Agent text accumulated in between-turn updates is emitted as a Chat row when a flush boundary arrives, labeled `unprompted (AI)`: the row reads `Name · unprompted (AI)`, in the same row form as `unprompted` and `when poked`.
- **Bubble and Behaviors:** The inbound wake marks the Instance `addressed`, triggering a Director wake. Speech reaches the bubble and Behaviors the same way a Poke or chat wake does — through the Director call that follows.
- **Director wake and Pace participation:** An inbound wake behaves like a reactive wake: it marks the Instance `addressed`, and the Director wake that follows calls `pace.after_reactive()`, resetting the exponential backoff to the first wait interval. This keeps cron-scheduled or `/loop`-driven speech from leaving the character silent for the full proactive interval.

#### Flush boundaries

ACP v1 has no end-of-turn notification from the Harness, so accumulated between-turn agent text and thought are flushed when a session transition arrives. The flush boundaries are (#1383):

- **Ask:** A between-turn `session/request_permission` flushes the session's accumulated `Inbound` before the ask is held, giving each Harness fire a coherent wake. The Thinking row is closed (empty thought event) if one was open.
- **Form:** A between-turn `elicitation/create` with session scope flushes the same way.
- **Prompt:** A Fidget `session/prompt` flushes and drops the session's `Inbound`, ending its Thinking row if open. The Harness's own turn is over; Fidget's turn begins.
- **Close:** A Fidget `session/close` flushes and drops the `Inbound` the same way Prompt does.
- **New message:** A between-turn agent chunk whose `messageId` differs from the one held flushes the session's `Inbound` first, so two fires in a row are two wakes (#1435). A chunk with no `messageId` never starts a wake, so a Harness that sends none keeps one wake until another boundary.

Multiple Harness fires before Fidget's next prompt previously piled into one undifferentiated accumulator; flushing on each ask or form boundary fixes that (ponytail fix, #1383).

#### Attribution

Each session belongs to the Instance it was opened for, from the moment it opens, so work between turns has an owner before that session's first prompt (#1395).

- **Asks:** An ask or form stores the session that asked. `awaiting_user` resolves that session through the owners map, so an ask that arrives during open still belongs to its Instance once the open finishes. It holds that Instance's wakes, not those of the Instance whose turn is running. A form no session scopes, such as a sign-in link, and an ask on a session this child never opened, are owed by whichever Instance holds the turn.
- **Updates:** During a Fidget turn, an update, ask, or session-scoped form for another session is held as that session's between-turn work. It never joins the running turn's reply.
- **Chat:** An ask, form, or plan is drawn only in the Chat of the Instance that owns it (#1422). An ask or form is named when it arrives, by the same rule as above. One that arrives during open, before the owners map has the session, goes to the turn holder's Chat, and a wake's open runs inside that wake's own turn. A Chat that opens later replays only its own open asks and forms. When the owner's Chat is not on screen, Fidget opens it once, and opens it again only after that owner's last ask or form settles. Another Instance's row opens that Instance's Chat. A form nobody owes, such as a sign-in link before any turn, is drawn in every Chat so whichever is open can answer it.

#### Out of scope

- **Live Harness scheduled-fire prove:** Unit and fake-ACP proof shipped (#1370, #1383); proving with a real Harness cron is optional follow-on.
- **Memory scheduling:** Harness cron is session-scoped, jittery, and not portable across Directors. A systematic parse-and-act path for reminders committed to Memory (due-at / remind facts) is future work and does not block this contract.

### Setting up `pi`

Needs a global `pi` on `PATH`: `brew install pi-coding-agent` (Homebrew pins Node in the shebang). `npx`, `node`, and `pi` must resolve in the app's environment; a Finder-launched build finds them on your login shell's `PATH` (see [Finding programs on `PATH`](#finding-programs-on-path)). An unconfigured Pi may pick up an ambient provider key from the environment; configuring `~/.pi/agent/` (e.g. `omlx launch pi`) wins. An npm-global `pi` can shadow the keg: `npm uninstall -g @earendil-works/pi-coding-agent`, then `brew link pi-coding-agent`.

### Setting up `antigravity`

Unzip the `antigravity-acp` archive from the [ACP registry](https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json) and put its folder on `PATH`, keeping `localharness_external` beside the server. Sign in with Chat's Log in with Google or Gemini Enterprise button; the server opens your browser, and it has no terminal login. Google's [FAQ](https://antigravity.google/docs/faq) says third-party software using an Antigravity login violates its Terms of Service and may get the account suspended.

## MCP Server

Two transports, not to be conflated:

1. **ACP** (Fidget ↔ Harness): always stdio. Fidget spawns the Harness and prompts it over newline-delimited JSON-RPC.
2. **MCP** (Harness → Fidget): the Harness calls back so `speak`, sensing, and Memory reach the fidget.

The README lists the [tools and resources](../README.md#harness--mcp).

### How it works

Dispatch lives in the running app (ADR-0023). `src-tauri/src/mcp_http.rs` serves the tools on `http://127.0.0.1:<random-port>/mcp` behind a per-run bearer token: 32 fresh bytes in memory, never on disk or in a log. It is thread-per-request with no async runtime, request/response only: no notifications, progress, sampling, SSE push, or prompts. If the bind fails at launch, the app runs without MCP (ADR-0023). The bind is loopback only, because the token authorizes moving the fidget (ADR-0023). The denylist applies to `list_windows`, `describe_screen`, and `fidget://windows`: it filters password managers and redacts password fields.

The ACP `initialize` bit `agentCapabilities.mcpCapabilities.http` decides the route:

- **Advertised** → the Harness gets the loopback URL and token directly.
- **Absent or false** → the Harness gets a stdio MCP server entry to spawn (ADR-0026). That binary is a stateless relay: it posts every JSON-RPC message to the app's endpoint, using `FIDGET_MCP_URL` and `FIDGET_MCP_TOKEN` from its environment. It is found as `FIDGET_MCP_BIN`, else a `fidget-mcp` sidecar beside the app, else the app binary itself (`fidget --mcp-stdio`).
- **`cursor-agent`** ignores `mcpServers` entirely and loads servers only from an approved `.cursor/mcp.json` (#1020). It gets the loopback URL and token through that file.

On ACP attach, Fidget pre-approves only its own MCP tools where the Harness supports a scoped startup policy: Claude through session options, Codex and OpenCode through inline config, Copilot and Grok through launch flags, and Cursor through its project CLI permissions. Other tools keep the Harness's normal approval policy, and Fidget forwards any permission request it receives to Chat unchanged. Hermes permits ordinary MCP tool calls without a per-call prompt in the installed build. For Goose, Pi, and Antigravity, Fidget has not verified a scoped startup rule and leaves their permission flow to the Harness.

Codex gets the Fidget HTTP endpoint in its `CODEX_CONFIG` session override under a process-specific `fidget_attached_<pid>` server name, with `default_tools_approval_mode: "approve"`. This keeps an existing `mcp_servers.fidget` entry separate. The bearer token is passed through `FIDGET_MCP_TOKEN`, not embedded in the JSON. Fidget leaves `mcpServers` empty for Codex sessions using this override: codex-acp replaces the entire `mcp_servers` override when ACP also supplies a server, which would discard the approval rule. If the inherited override already uses the process-specific name, Fidget falls back to normal ACP registration and permission flow.

### Elicitation

`initialize` declares both elicitation modes, `form` and `url`, to every Harness. With `url` declared, a Harness can hand Chat a link rather than open a browser itself. codex-acp offers its device-code sign-in only then. codex-acp and claude-agent-acp send an MCP server's OAuth link the same way; without `url` that server stays signed out.

It is not narrowed per Harness. A Harness already runs code as the user, so a link lets it do nothing new. What the link adds is a gate: Chat draws the URL in full, as `open_link` will open it, and nothing opens until the user clicks Open. `open_link` refuses any scheme but `http`, `https` and `mailto`.

Two kinds of link open Chat: one that arrives during Fidget's own `authenticate`, and one scoped to a tool call. A tool call's link blocks its turn, so it behaves like a permission ask: Chat opens for it unless Do Not Disturb is on, and then it waits in Chat as an ask does. Any other link, such as an MCP server's after `session/new`, waits in Chat: the next Chat to open draws it, and nothing takes focus. A link scoped to the session and no tool call belongs to the session, so the turn it lands in does not cancel it.

`elicitation/complete` retires a held link's row, as an answer does, and answers the request with `cancel`. The user finished the flow somewhere else and made no choice in Chat, which is what `cancel` means; `decline` would say they refused. An `elicitationId` no held link carries is ignored.

### How `cursor-agent` is reached

`src-tauri/src/cursor_mcp.rs`. `.cursor/mcp.json` is the only place to define a server (`cursor-agent mcp` has no `add`), and `cursor-agent mcp enable` is the only way to approve one. Approvals are read once per `cursor-agent` process, so before spawning `cursor-agent acp`, attach:

1. Merges `{"url": …, "headers": {"Authorization": "Bearer …"}}` under `mcpServers."fidget"` in `<cwd>/.cursor/mcp.json`, beside existing servers. A file that does not parse is left alone and the attach continues without tools.
2. `chmod 600` the file, because it holds a live credential. Windows has no mode bits here, so the file keeps the project directory's ACL.
3. Adds `Mcp(fidget:*)` to `permissions.allow` in `<cwd>/.cursor/cli.json` unless it is already there, preserving other project permissions. Writes a missing `permissions.deny` as `[]`, because cursor-agent rejects the object without it, and leaves an existing deny list as it is. Cursor's CLI uses this rule to allow calls to the Fidget server without prompting; a matching `permissions.deny` still wins.
4. Runs `cursor-agent mcp enable fidget` in that directory (~380ms).

URL and token are new every app run, so each attach rewrites and re-approves. Within one run the entry is unchanged and a re-attach costs only the spawn.

Cursor appends each approval to `~/.cursor/projects/<slug>/mcp-approvals.json` and never prunes: about 31 bytes per app run. Detach does not call `cursor-agent mcp disable`, because that blocks the server from ever loading again.

Detach removes the `fidget` entry from `mcp.json`, and the file too if attach created it and nothing else is left in it. The `Mcp(fidget:*)` allow in `cli.json` stays, because it holds no credential and the next attach would add it again, so `.cursor/` stays as well. While attached, the token sits in the working directory's `.cursor/mcp.json`, owner-only, dead after the app run. What the project's VCS does with an untracked `.cursor/` is the project's business.

### Pointing a Harness you run yourself at Fidget

An attached Harness needs no setup. A Harness you launch yourself gets nothing forwarded, so register the endpoint by hand, once per app launch:

1. Start Fidget and open Settings.
2. Under **BYO - Point existing Harness at Fidget**, pick the Harness.
3. Copy the generated command (or JSON fragment, for Harnesses with no `mcp add`) and run or paste it as the instructions say. Hermes prompts for the token, so its row has a separate Copy for it.
4. Reload or restart the Harness session. Claude Code and OpenCode read MCP config only at session start.

The port is OS-assigned and the token is minted in memory at every launch (ADR-0018), so no document can carry the command, and a stale entry is a dead one. The generator (`byo_registration` in `src-tauri/src/settings.rs`) always hands out the loopback URL and token; the stdio relay plays no part here. The box is empty when the loopback bind failed.

| Harness | What the generator emits | Where the entry lands | Standing |
|---|---|---|---|
| `claude` | `claude mcp remove` then `claude mcp add --transport http …` | `~/.claude.json`, keyed by working directory (local scope) | **verified by hand** |
| `codex` | `export FIDGET_MCP_TOKEN=…` then `codex mcp add --url … --bearer-token-env-var` | `~/.codex/config.toml`, token stays in the environment | **verified by hand** |
| `cursor-agent` | `mcpServers` JSON fragment, then `cursor-agent mcp enable fidget` | `.cursor/mcp.json` in the project | **verified by hand** |
| `grok` | `grok mcp add … --transport http` with a header | `~/.grok/config.toml`, token in the file | **verified by hand** |
| `hermes` | `hermes mcp add --url … --auth header`, token pasted at its prompt | `~/.hermes/config.yaml`, token in `~/.hermes/.env` | **verified by hand** |
| `opencode` | `opencode mcp add --url … --header "Authorization=Bearer …"` | `~/.config/opencode/opencode.json`, token in the file | **verified by hand** |
| `pi` | `mcpServers` JSON fragment for `.mcp.json` or `~/.pi/agent/mcp.json` | the project, or the agent directory | **verified by hand** ‡ |
| `copilot` | `copilot mcp remove` then `copilot mcp add --transport http …` with a header | `~/.copilot/mcp-config.json`, token in the file | **config generated, unverified** |

**Verified by hand** means the box's output was run as given and that Harness's `speak` was recorded in the bubble on a real Mac (2026-09-21/22). **Config generated, unverified** means the shape was checked against the CLI (`copilot` 1.0.88; `remove` comes first because a second `add` fails) but no `speak` is recorded (#1016). A Harness the popup does not list (`custom`) gets the bare URL and token.

‡ `pi` has no MCP client of its own; it needs an adapter plugin such as `pi-mcp-adapter` (verified on pi 0.85.1 with pi-mcp-adapter 2.36.0). The adapter connects lazily: a headless `pi -p` run must call `mcp({"connect": "fidget"})` first, and an interactive session wants `/mcp connect` or `/mcp reconnect fidget`. The tool is reached through Pi's `mcp` proxy as `fidget_speak`. Whether an attached `pi-acp` session lists Fidget's tools is unmeasured (#984).

**Traps:**

- Every registration dies with the app. Under `opencode` a dead entry hangs `opencode run` at startup for minutes with no error; remove the entry or pass `--pure`.
- `opencode` has no `mcp remove`. Edit `~/.config/opencode/opencode.json` by hand.
- `hermes mcp remove` leaves `MCP_FIDGET_API_KEY` in `~/.hermes/.env`, so the next `hermes mcp add` skips the token prompt, reuses the dead token, and fails with `401 Unauthorized`. Delete that line before re-adding. Its `mcp add` and `mcp remove` also rewrite `config.yaml`, stripping comments and re-indenting.
- `codex` needs the `export` in the same shell that launches `codex`. Sourcing it through a pipe leaves the tool unregistered.
- `pi` needs its adapter told to connect. The fragment carries no `"lifecycle": "eager"`, so the server stays disconnected until asked.

### Not served

- **Pixels:** `describe_screen` is window metadata only; Fidget takes no screenshots and runs no OCR or vision ([ADR-0031](./adr/0031-drop-capture-tiers.md)). Agents that need pixels use Harness-native computer use or an MCP server like cua-driver.
- **Input events:** No click, type, or mouse tools (ADR-0003). The Harness owns desktop control.
- **Non-loopback MCP** (ADR-0023).

Whether a Harness sets `mcpCapabilities.http` is the Harness's decision. Hermes speaks HTTP MCP as a client through its own `mcp_servers` config, a different axis from the ACP bit; a Hermes that set the bit would take the loopback path with no change here.
