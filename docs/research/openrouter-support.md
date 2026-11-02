# OpenRouter as a first-class AI source

Anchor: `faa34b9f` on `main`, 2026-10-18. Every Fidget citation below is read
against that tree. The spike is #1614.

Since #2080 the HTTP Completer runs Fidget's tools, so the reason given under
"Not needed" for skipping `reasoning_details` passback no longer holds.
OpenRouter says reasoning blocks must be preserved during tool use
([reasoning tokens](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens.md)).
Passback is still not built. It is an open point for reasoning models in a
tool round, and nothing has run against OpenRouter to show what goes wrong.

This note answers one question: what does it take to make
[OpenRouter](https://openrouter.ai) a first-class AI source in Fidget? It
covers the Model API, which is the HTTP Completer in `src-tauri/src/model.rs`,
the Settings AI tab, the Chat landing, the docs, and the Harness presets in
`harness::launch`.

Claims carry one of three labels:

- **Measured** means it ran here on 2026-10-18. That covers unauthenticated
  requests to `openrouter.ai`, the brand SVGs fetched from
  [openrouter.ai/brand](https://openrouter.ai/brand), and `ori --help` on the
  installed `ori` 0.15.7-alpha.
- **Source** means it was read in Fidget's code or on an OpenRouter docs page,
  and the page is linked.
- **Inferred** means it was reasoned from the other two and nothing ran.

No OpenRouter key was available. Nothing authenticated ran, so no streamed
turn, reasoning delta, or mid-stream error was observed on the wire. Fidget
did not run.

---

## Summary

A user can reach OpenRouter today by typing its URL into Base URL. That is
not support. First class means the user finds OpenRouter without knowing its
URL, enters a key in a place made for it, gets a working first launch, and
reads about it in the README. That takes code in five places:

1. **Settings:** an OpenRouter row in the Base URL picker under Model API. It
   fills Base URL, and fills a blank Model with `openrouter/auto`.
2. **Chat landing:** an OpenRouter button with OpenRouter's glyph and accent.
   It switches the source to Model API on OpenRouter and opens Settings on the
   API key row.
3. **Key storage:** one keychain account per hosted endpoint, so picking
   OpenRouter never sends a stored OpenAI key to `openrouter.ai`.
4. **Wire fixes:** three gaps in the HTTP Completer. The worst one logs a
   false failure on every launch.
5. **Docs:** the README, its attribution section, and `docs/harness.md`.

"Sign in with OpenRouter" (OAuth PKCE) comes after these, as decided. Fidget
should not launch the `ori` CLI. The plan is the [ordered slices](#plan)
below.

## Where OpenRouter belongs in Settings

**Recommendation: a row in the Base URL picker, not an AI source entry.** The
Chat button below is what makes it visible.

The AI source picker chooses a Completer. `harness_options` lists Model API,
then one `Harness · <name>` per `HARNESS_PRESETS` entry, then
`Harness · Custom` (Source). Each one is a different process or protocol.
OpenRouter is neither. It is one more OpenAI-compatible HTTP endpoint, served
by the same HTTP Completer as OpenAI and Ollama. An "OpenRouter" source would
be a second name for Model API with Base URL fixed.

The AI source entry also costs more (Source):

- The picker writes `TextField::Harness`. `harness_rows`,
  `harness_command_line`, and `harness_choice` map every value that is not a
  preset or `custom` to a Custom command line. An `openrouter` value needs its
  own arm in all three.
- #1427 disables the section whose source the picker does not name. An
  OpenRouter source would have to serve the Model / API section too.
- `select_harness_blocking` refuses any name not in `HARNESS_PRESETS`.

A Base URL row is one `ENDPOINTS` entry. `endpoint_title` already round-trips
it:

```rust
("Hosted", "OpenRouter", "https://openrouter.ai/api"),
```

The row is the host plus `/api`, because `completions_url` adds `/v1` and the
path. `endpoint_title` matches the trimmed base exactly, so a user who types
`https://openrouter.ai/api/v1` rests on Custom. That is the current behavior
for `https://api.openai.com/v1` too.

The row should do one thing no current row does: fill a blank Model.
`openrouter/auto` ("Auto Router") is in the catalog (Measured), so a user who
picks the row and pastes a key gets a reply without choosing among 471 slugs.
A Model the user already typed stays.

The settings page is drawn from the form description. The
`tests/fixtures/settings-snapshot-modelApi.json` fixture lists the picker's
options and is checked against `form.rs` (Source). The row has to update it.

## The API key

**Today (Source).** The API key row (`DIRECTOR_API_KEY_ID`) stores one secret
under the keychain account `director-api-key` in `secrets.rs`. Every Base URL
shares it. A grep at the anchor found no code that ties the key to the Base
URL.

**What breaks (Inferred).** A user with an OpenAI key saved who picks
OpenRouter sends `Authorization: Bearer <OpenAI key>` to `openrouter.ai`. The
request fails with 401, and a secret has gone to a third party. This is true
of any Base URL change today. A first-class picker row makes it a one-click
path.

**Fix.** Key the account by hosted endpoint, for example
`director-api-key:openrouter.ai`. Local rows need no key (`is_local`). A
Custom URL keeps the shared account. The key row's placeholder then says
which endpoint the key is for. The shared account is not migrated: the
owner's call is no backcompat before v1. The cost is that anyone on the
OpenAI, Anthropic, or xAI rows pastes their key again once after the upgrade.
The rejected alternative reads `director-api-key` once as a fallback.

**Where to get a key.** The key row's help line names OpenRouter's key page
when OpenRouter is picked. The URL is read off OpenRouter's docs when the
slice is built, not here.

## The Chat landing button

**What exists (Source).** `src/chat.html` draws one `.connect-btn` per
Harness, each with an inline SVG mark and a `data-harness` name. Each
Harness has a `--<name>-accent` in `src/chat-ui.css`, most with a
light-theme value too. The accent colors the button's border, its hover
fill, and its icon through `currentColor`. A click calls `select_harness`, and
a "More options in Settings" button calls `show_ai_source`, which opens
Settings through `Reveal::AiSource`.

**The glyph (Measured, Source).** OpenRouter's brand page offers a glyph "for
avatars, favicons, small sizes" as SVG, in four colorways. Its one rule is
"Please don't stretch, recolor, or remix the marks", and it states no license
([brand](https://openrouter.ai/brand)). Each glyph SVG is one path on a
`0 0 1024 730` viewBox with one fill:

| Colorway | Fill | Brand page says |
|---|---|---|
| Grape | `#7624F4` | light backgrounds |
| Volt | `#C8FF00` | dark backgrounds |
| Ink | `#03080A` | light backgrounds |
| Cloud | `#FCFCFE` | dark backgrounds |

Inline the path with `fill="currentColor"` and set `--openrouter-accent` to
Volt on the dark Chat ground and Grape on the light one. The glyph then only
ever shows in an official colorway, so it is not recolored (Inferred). Keep
the viewBox. Changing it to square would stretch the mark. The README's
"Prior Art and Attribution" section names where each mark comes from, so it
gains an OpenRouter line.

**What a press does.** `select_harness` cannot serve it, because OpenRouter
is not a preset. A new command does three things, in one `SettingsSession`
apply:

1. Set the source to Model API (`TextField::Harness` empty).
2. Set Base URL to the OpenRouter row, and Model to `openrouter/auto` if
   blank.
3. Open Settings on the API key row, through a new `Reveal::ApiKey` beside
   `Reveal::AiSource`. Focus goes to the field.

Until a key is saved, Chat shows its existing `empty-http` state, "Nothing to
answer with" (Source). Once sign-in exists, step 3 starts sign-in instead.

**Copy that turns false.** The landing title is "Connect a Harness to get
started" and the lede says each runtime signs in on its own with "no
credentials stored here" (Source). With an OpenRouter button, both are wrong:
OpenRouter is not a Harness, and its key is stored in the keychain. That copy
is the owner's. The slice proposes wording and leaves the choice to him.

**Header (Source).** With Model API in force, the Chat header's mind line is
`<model> · <host>`, so it already reads `openrouter/auto · openrouter.ai`. No
change.

## Model API: what already works

**URL join (Source).** `completions_url` appends `/v1/chat/completions` to a
base that does not end in `/v1`. It appends only `/chat/completions` to a base
that does. Both `https://openrouter.ai/api` and `https://openrouter.ai/api/v1`
therefore resolve to `https://openrouter.ai/api/v1/chat/completions`, which is
OpenRouter's chat path ([API overview](https://openrouter.ai/docs/api-reference/overview)).
`host_is_xai` is false for `openrouter.ai`, so the Responses path is never
chosen.

**Auth (Source, Measured).** `Endpoint::headers` sends
`Authorization: Bearer <key>`. That is OpenRouter's scheme
([authentication](https://openrouter.ai/docs/api_reference/authentication.md)).
`is_local` is false for `openrouter.ai`, so `endpoint_from` refuses to build an
`Endpoint` without a key. This matches every hosted row. Without a key, chat
answers 401 with `{"error":{"message":"No cookie auth credentials found","code":401}}`
(Measured). `status_error` already reports a non-2xx status with its body.

**Streaming (Source).** OpenRouter holds the connection open with
`: OPENROUTER PROCESSING` comment lines. It ends the stream with `data: [DONE]`
([streaming](https://openrouter.ai/docs/api-reference/streaming)). `read_frames`
skips every line that does not start with `data:` and stops on `[DONE]`.

**Reasoning (Source, Measured).** OpenRouter streams thinking as
`delta.reasoning`
([reasoning tokens](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens.md)).
`read_event` already reads `chunk["reasoning"]` as a thought. A thinking model
therefore fills the Thinking row and earns `THINK_CEILING` on the next turn,
as on vLLM and Ollama.

**Unsupported parameters (Source).** OpenRouter ignores a parameter the model
does not support, rather than refusing it (API overview). So no 400 ever names
one, and the `Field` fallbacks in `Endpoint::post` never fire on OpenRouter.

**Reasoning effort (Measured).** Fidget sends effort as a top-level
`reasoning_effort`, while OpenRouter documents `reasoning: {effort}`. Of 471
listed models, 209 name `reasoning_effort` in `supported_parameters` and 341
name `reasoning` (`GET /api/v1/models`). On a model that takes only
`reasoning`, the effort is dropped without a word (Inferred). This is
acceptable for v1, because a blank effort is the default.

**Turn ceiling (Measured, Source).** Of 471 models, 458 list `max_tokens` and
79 list `max_completion_tokens`. The `Field::Cap` rename is not needed. On the
13 models that list neither, no ceiling applies (Inferred).

The ceiling has a gap that is not OpenRouter's. `Endpoint::wire_budget` sends
`TURN_CEILING` (1024) until a turn on the host has marked its thinking, and
only then `THINK_CEILING` (8192). On OpenRouter, the host is one URL for every
model. Two cases follow (Inferred):

- The first turn of a reasoning model goes out at 1024 and can end as
  "spent all 1024 tokens thinking". This is the same first-turn cost as on any
  other host.
- A model whose provider does not stream its reasoning as text never marks.
  OpenRouter returns some reasoning only as encrypted `reasoning_details`
  (reasoning tokens docs). Such a model stays at 1024 on every turn, while its
  hidden reasoning still counts against the 1024.

Both cases already have a lever: `FIDGET_DIRECTOR_MAX_TOKENS` and its Settings
row. A keyed probe on one encrypted-reasoning model would show whether the
second case is real.

**Usage chunk (Source).** On chat completions, the last chunk before `[DONE]`
carries `usage` and one choice with empty content (streaming docs).
`read_event` takes its empty `delta.content` as no text. The reply is
unchanged.

## Model API: what breaks

### 1. The `/v1/models` preflight asks the wrong URL

`spawn_preflight` and `run_probe` build the models URL as
`format!("{origin}/v1/models")`, where `origin` is scheme, host, and port only.
The base path is lost.

| URL | Answer (Measured) |
|---|---|
| `https://openrouter.ai/v1/models` (what Fidget asks) | 404, `text/html` |
| `https://openrouter.ai/api/v1/models` (the real path) | 200, `application/json`, 471 models |

`preflight_verdict` turns the 404 into
`director: https://openrouter.ai /v1/models answered 404; staying on StaticDirector until it answers`.
The message is false. The preflight is diagnostic only, and the next wake
still goes out, so the Director works. But the first line the user reads says
it does not. `run_probe`, behind `scripts/probe-model.sh`, asks the same URL
and prints `/v1/models answered 404`.

Fix the root cause rather than special-casing OpenRouter. Derive the models
URL from the inference URL. Replace a trailing `/chat/completions` or
`/responses` with `/models`. This gives `/v1/models` on every host that works
today. It also gives `/api/v1/models` on OpenRouter, and the right path on any
other proxy mounted under a prefix (Inferred).

### 2. A wrong slug floods the log

When the model is not served, `preflight_verdict` joins every served id into
one message. OpenRouter serves 471 ids, and the joined list is 12,173
characters (Measured). OpenRouter slugs also look alike.
`anthropic/claude-haiku-5.5` and `anthropic/claude-haiku-5.5:batch` are both
listed. A user who typed
`claude-haiku-5.5` without the vendor prefix gets the whole catalog. Cap the
list, for example at the ten closest ids, or name only the count.

### 3. A 200 that carries an error reads as a reply

OpenRouter returns the HTTP error status only for invalid requests and missing
credits. Any other failure after routing starts comes back as status 200, with
the error in the body or in an SSE event
([errors](https://openrouter.ai/docs/api_reference/errors-and-debugging.md)).
A mid-stream error looks like this
([limits](https://openrouter.ai/docs/api_reference/limits.md)):

```json
{"error":{"code":429,"message":"Rate limit exceeded"},"choices":[{"index":0,"delta":{"content":""},"finish_reason":"error"}]}
```

`read_event` reads any `finish_reason` as `finished`. It flags only `length` as
truncated. The turn therefore ends as `Streamed::Complete` (Source):

- With no text before the error, `reply_from` returns
  `streamed reply had no text content`. The wake fails, and the Action Log
  loses OpenRouter's message (Inferred).
- With text before the error, the partial text is spoken as if whole. Its
  first line is parsed as a Behavior (Inferred).

The in-band case that matters for a desktop pet is a 429 on a `:free` model.
Missing credits are not in-band. OpenRouter answers them with an HTTP 402,
which `status_error` already reports (errors). Free models allow 20 requests
per minute, and 50 per day on an account that has bought less than 10 credits
(limits). Proactive model calls from one Character Instance can reach the
daily cap in a working day (Inferred).

The fix runs through three places. `Event` gains the error message.
`read_frames` stops on it and returns it, which needs a new `Streamed` variant.
`reply_from` turns that variant into `Unsent::Failed` that carries
`error.message`. `content_from_body` does the same for the non-streaming
answer. Any OpenAI-compatible host that reports errors in-band benefits.

### 4. Not needed

- **Attribution headers:** `HTTP-Referer` and `X-OpenRouter-Title` only list
  the app on OpenRouter's rankings
  ([app attribution](https://openrouter.ai/docs/app-attribution.md)). Fidget
  sends `User-Agent: fidget`. Skip them until someone asks to be listed.
- **`reasoning_details` passback:** OpenRouter asks callers to send it back for
  encrypted reasoning in tool loops. The HTTP Completer runs no tools. Its
  session stores only answer text.
- **Provider routing, `models` fallbacks, and `:nitro`:** Each one is a request
  field Fidget has no Settings row for. A slug suffix such as `:nitro` already
  works through the Model field (Inferred).

## Sign in with OpenRouter

Today the user creates a key on openrouter.ai and pastes it. OpenRouter also
has OAuth PKCE
([OAuth](https://openrouter.ai/docs/guides/overview/auth/oauth.md)). A headless
variant shows a code for the user to paste, so it needs no callback server.
The code is single-use and lasts 10 minutes. `POST /api/v1/auth/keys`
exchanges it for a key, and Fidget stores that key in OpenRouter's keychain
account like a pasted key. It then becomes what the Chat button does. It is
the last slice, as decided.

## Docs

Each place that changes, and the slice it rides with:

| Place | Change | Slice |
|---|---|---|
| `README.md`, "Get It", the Model API paragraph | It names the presets Base URL fills. OpenRouter joins the list. | 6 |
| `README.md`, "Prior Art and Attribution" | Where the OpenRouter glyph comes from and whose trademark it is. | 6 |
| `src/chat.html` and `landingCopy` in `src/chat-connect.js`, landing title and lede | Stop saying "Harness" and "no credentials stored here" when OpenRouter is offered. | 6 |
| `docs/harness.md`, "Quick Start" | An `# OpenRouter` block beside OpenAI, Anthropic, xAI, and Ollama. | 5 |
| `docs/harness.md`, "Provider Details" | The in-band error, the free-model limits, and `openrouter/auto`. | 5 |
| `docs/harness.md`, a new section | The Claude and Codex recipes. | 7 |

README and landing copy is the owner's voice. Each slice proposes the line in
its description and leaves the wording to him. Whether OpenRouter earns a
line in "What It Does" is his call too.

## Harness presets

### Fidget's side

`Launch::command` builds the Harness child with the inherited environment. Its
doc comment says no provider key goes in, "because one overrides a
subscription login with no prompt" (Source). `docs/harness.md` adds that a
Finder-launched Fidget takes only `PATH` from the login shell (Source). A
`export ANTHROPIC_BASE_URL=...` in `.zshrc` therefore reaches the Harness only
when Fidget starts from a terminal (Inferred).

Each Harness needs its provider set in its own config, such as the user's
`~/.codex/config.toml`. Fidget does not read or write that config (Source).

Fidget does write one thing a Harness reads as a model. The Harness Model row
goes out through `session/set_config_option`. For Codex,
`sync_codex_project_config` also writes `model` and `model_reasoning_effort`
into a project `.codex/config.toml` under Fidget's data directory (Source).
Under OpenRouter, that value has to be an OpenRouter slug. Whether each
adapter accepts a slug it did not list is unverified.

### Per preset

| Preset | Route to OpenRouter | Status |
|---|---|---|
| `claude` | Claude Code with `ANTHROPIC_BASE_URL=https://openrouter.ai/api`, `ANTHROPIC_AUTH_TOKEN=<key>`, and `ANTHROPIC_API_KEY=""`, through OpenRouter's Anthropic-compatible `/api/v1/messages` ([Claude Code guide](https://openrouter.ai/docs/guides/guides/claude-code-integration.md)). The Agent SDK under `claude-agent-acp` uses the same variables ([Agent SDK guide](https://openrouter.ai/docs/guides/community/anthropic-agent-sdk)). | Endpoint answers 401 in Anthropic error shape without a key (Measured). OpenRouter guarantees this only with Anthropic as the provider (Source). |
| `codex` | `model_provider = "openrouter"` and a `[model_providers.openrouter]` table with `base_url = "https://openrouter.ai/api/v1"` in `~/.codex/config.toml` ([Codex guide](https://openrouter.ai/docs/guides/guides/codex-cli.md)) | `/api/v1/responses` exists and answers 401 without a key (Measured). It is stateless and refuses `store: true` or `previous_response_id` with 400 ([Responses](https://openrouter.ai/docs/api_reference/responses/overview.md)). Whether `codex-acp` sends either field is unverified. |
| `pi` | Its own config in `~/.pi/agent/`. `docs/harness.md` also says that an unconfigured Pi may pick up an ambient provider key from the environment. | The ambient-key route is Source. That Pi takes OpenRouter is Inferred. |
| `opencode`, `goose`, `hermes` | Each has its own provider config | Inferred from each project's provider support. Not read this session. |
| `copilot`, `cursor-agent`, `grok`, `antigravity` | None. Each runs on its vendor's account | Out of scope (Inferred). |

For `claude`, the variables have to reach the adapter. Claude Code's
`settings.json` takes an `env` block, which works however Fidget was started.
That changes every Claude Code session on the machine, not only Fidget's
(Inferred). The recipe has to say so.

### Why the Harness half stays a recipe

A launch preset, Settings row, or environment injection that points a Harness
at OpenRouter would put a provider key into the child. That undoes the
no-provider-key rule in `Launch::command`, which exists so that a user's
subscription login is never replaced without a prompt. The recipes go in
`docs/harness.md` after one keyed run, with the transcript on the issue, as
`docs/harness.md` asks of every probe.

### The `ori` CLI

`ori` is OpenRouter's CLI. Its "Ori Harness" mode is documented as "Run your
existing agent CLI on OpenRouter". It signs in with OAuth PKCE and launches
"the real agent CLI on your machine" for Claude Code, Codex, Hermes,
OpenCode, Pi, and others
([Ori Harness](https://openrouter.ai/docs/guides/ori/harness)). An untracked
`.ori/` folder in the owner's local checkout, not in the repo, holds its
state. Its one session record names `"harness": "ori"` and this spike's
prompt (Measured).

**Fidget should not launch it.** The evidence (Measured, on 0.15.7-alpha):

- `ori claude` starts Claude Code itself, interactive or with `-p`, and
  injects settings into that launch. Fidget's `claude` preset runs
  `claude-agent-acp`, and every Harness child has to speak ACP on stdio.
  Neither `ori --help` nor `ori help harness` names ACP or a stdio agent mode.
- `ori help login` says this release keeps keys and OIDC tokens in plaintext
  under `~/.ori/auth/plaintext`. Fidget reading that store would be reading
  another app's credentials.

`ori` is still the easiest way for a user to run Claude or Codex on OpenRouter
in their own terminal, so the recipe section names it. If a later `ori` speaks
ACP, it becomes a Harness preset candidate, and this section is out of date.

## Plan

Each slice is one pull request. Slices 1 to 3 have no dependencies and can go
in any order.

1. **`fix(model): Derive the models URL from the inference URL`.** Gap 1, in
   `spawn_preflight` and `run_probe`. Verified by a unit test for
   `https://openrouter.ai/api` next to the existing `completions_url` cases,
   and `scripts/probe-model.sh` against `https://openrouter.ai/api` printing
   no 404.
2. **`fix(model): Cap the served-model list in the preflight verdict`.** Gap 2,
   in `preflight_verdict`. Verified by a unit test with a 471-id list that
   asserts the message length.
3. **`fix(model): Read an in-band error as a failed turn`.** Gap 3, in
   `read_event`, `read_frames`, `reply_from`, and `content_from_body`.
   Verified by canned SSE bytes through `read_stream`, as the existing stream
   tests do, asserting `Unsent::Failed` carries "Rate limit exceeded".
4. **`feat(settings): Keep one API key per hosted endpoint`.** `secrets.rs`
   accounts, the API key row's read and write, and its placeholder. No
   migration, as the owner's call before v1: anyone on the OpenAI, Anthropic,
   or xAI rows pastes their key again once after the upgrade. Verified by
   `MemoryStore` tests that an OpenAI key and an OpenRouter key coexist and
   that switching Base URL reads the other one.
5. **`feat(settings): Add OpenRouter to the Base URL picker`.** The `ENDPOINTS`
   row, the blank-Model fill with `openrouter/auto`, the key row's help line,
   the `settings-snapshot-modelApi.json` fixture, and the `docs/harness.md`
   Quick Start and Provider Details lines. Verified by `form.rs` tests for
   `endpoint_title` and the fill, `node --test tests/`, and the settings
   verify script's AX dump showing the row. Depends on 1, so the first launch
   after a pick logs no false failure, and on 4, so the pick sends no stored
   OpenAI key.
6. **`feat(chat): Offer OpenRouter on the Chat landing`.** The glyph and
   `--openrouter-accent` in `src/chat.html` and `src/chat-ui.css`, the new
   select command in `main.rs`, `Reveal::ApiKey` in `form.rs`, the landing
   copy in `src/chat.html` and `landingCopy` in `src/chat-connect.js`, and the
   README lines. Verified by a `form.rs` test that the aim lands
   on the key row the form draws, like
   `the_ai_source_aim_lands_on_the_picker_the_form_draws`, and a cropped
   render of the landing in both themes on the PR. Depends on 5.
7. **`docs(harness): Run Claude and Codex on OpenRouter`.** A keyed probe,
   then the recipes, naming `ori` as the terminal route. Verified by the probe
   transcript on the issue. Needs a funded OpenRouter key. Independent of 1
   to 6.
8. **`feat(settings): Sign in with OpenRouter`.** OAuth PKCE in its headless
   form. The Chat button and the key row start it. Verified by a keyed
   end-to-end sign-in, with the exchange tested against a fixture. Depends on
   4 and 6.

## Decisions

The owner decided these on 2026-10-18.

1. OpenRouter gets a row in the Base URL picker once slice 5 is built and
   tested. Not before.
2. `docs/harness.md` carries the Claude and Codex recipes once slice 7 has
   run against a real key. Not before.
3. "Sign in with OpenRouter" is wanted, but not yet. Slice 8 waits until the
   work reaches it.
4. OpenRouter is a first-class citizen: a Settings entry that fills the
   endpoint and takes the key, a Chat landing button with its icon, and
   README and docs coverage. This comes from the owner's PR comment, which
   is newer than 1 to 3. It widens the scope and does not reverse them.

## Open questions

1. Does OpenRouter go in the Base URL picker under Model API, as recommended,
   rather than as its own entry in the AI source picker?
2. Should each hosted endpoint keep its own API key (slice 4), so picking
   OpenRouter never sends a stored OpenAI key to `openrouter.ai`? The cost:
   anyone on the OpenAI, Anthropic, or xAI rows pastes their key again once
   after the upgrade, since the shared account is not migrated. The rejected
   alternative reads `director-api-key` once as a fallback.
3. Should picking OpenRouter fill a blank Model with `openrouter/auto`?
