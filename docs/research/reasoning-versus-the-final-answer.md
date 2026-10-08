# Telling a model's reasoning from its final answer on the Completer lane

Research for #597, prompted by #541. Question: when a model returns text, can
the HTTP Completer tell the model's reasoning from its final answer, on local
and hosted endpoints alike, from the protocol contracts rather than by
guessing? And can the reply cap be sized from those contracts?

**Answer.** Yes for the part that matters, and the app already receives the
signals it needs and throws them away. Every contract in scope marks a
truncated reply: chat-completions with `finish_reason: "length"`, Responses
with `status: "incomplete"` and `incomplete_details.reason:
"max_output_tokens"`, Anthropic with `stop_reason: "max_tokens"`, ACP with
`stopReason: "max_tokens"`. The app reads none of the four values. It checks
only that `finish_reason` is a string, so a reply that ran out of room and a
reply that finished are the same event to it. Reasoning is marked too, but
unevenly: Responses, Anthropic and ACP mark it in the specification; every
local server in scope marks it by convention in a sibling field of `content`,
under one of two names (`reasoning_content` or `reasoning`); and OpenAI's own
chat-completions specification has no reasoning field at all, so on that
surface the field is a convention the servers agreed on among themselves.
Raw `<think>` tags inside `content` are a per-server accident and cannot be
parsed for reliably. The cap cannot be sized from the contracts, because no
chat-completions contract bounds thinking separately from the answer; the
lever the contracts do offer is `reasoning_effort`, and it is the one lever
that measurably changes the empty-reply rate. The recommendation is one
mechanism with two halves that are the same idea, read the wire's own markers:
refuse a `length` finish with empty content as a reply at all, and route a
marked reasoning delta to the transient thought door ADR-0025 already opened,
never into the reply. A bigger cap is not the fix.

- `file:line` citations are against `66c1bab8`.
- Date: **September 10, 2026**.
- Claims are marked **[spec]** (a published API contract), **[server]**
  (read in a server's source or documentation), **[measured]** (run against
  the oMLX 0.6.4 server at `localhost:8000` on this machine), or
  **[inference]** (my own reasoning about this repository).

Vocabulary is `CONTEXT.md`: Director, Character Prompt, Completer, Harness,
Behavior, Speech, Action Log.

---

## 1. What the code does today

Verified against the tree, not the issue's summary of it.

- `LOCAL_MAX_TOKENS = 512` and `HOSTED_MAX_TOKENS = 80`
  (`src-tauri/src/model.rs:79-80`). The doc comment on the first predicts this
  failure and calls raising the cap "the portable half of that fix", because
  `reasoning_effort` is "not a field every one of these servers accepts, and a
  strict one rejects the whole request over it" (`model.rs:73-78`).
- `max_tokens_for` (`model.rs:504`) returns the user's
  `FIDGET_DIRECTOR_MAX_TOKENS` when it parses to a non-zero number, else the
  constant for local or hosted. The Settings row's placeholder is built at
  `model.rs:531`. A knob exists; nothing tells the user when to turn it.
- `request_body` (`model.rs:1203`) sends `max_tokens` on chat-completions and
  `max_output_tokens` plus `"reasoning": {"effort": "low"}` on Responses
  (`model.rs:1232-1243`). The chat-completions branch sends no effort field.
- `read_event` (`model.rs:1410`) reads `choices[0].delta.content` and, for
  Responses, the `delta` of a `response.output_text.delta` event. Every other
  frame falls to the catch-all arm with `delta: None`. A `reasoning_content`
  or `reasoning` delta is discarded there. A `response.reasoning_summary_text.delta`
  event is discarded there. A `response.incomplete` event is discarded there
  too, which matters below.
- `finished` is `choice["finish_reason"].is_string()` at `model.rs:1418` and
  `model.rs:1432`. The value is never read. `stop` and `length` are one
  signal.
- The test at `model.rs:2229` streams one `reasoning_content` delta and
  `[DONE]` and asserts `Streamed::Complete(String::new())`, under the comment
  "a model that thought its whole budget away did stream". It pins that the
  stream is recognised as a stream. It does not assert anything about the
  reasoning; it asserts the reasoning was dropped.
- An empty `Complete` becomes `Unsent::Failed("streamed reply had no text
  content")` at `model.rs:825`. `retry_settles` (`model.rs:1289`) returns
  `None` for `Failed`, so there is no retry. The wake is lost and nothing
  records that a cap caused it.
- A non-empty reply goes to `parse_proposal` (`crates/core/src/director.rs:468`).
  A first line that is not a single identifier is a `ParseError`, and a
  `ParseError` becomes `spoken_or_failed` (`director.rs:281`, `:310`): the
  Engine plays `talk` and the character says the text. Anything a model puts in
  `content` that is not a Behavior name is spoken aloud.

**[inference]** So the lane has one behaviour for reasoning, which is to
discard it, and one behaviour for a truncated reply, which is to treat it as
a completed one. The two together produce #541's 40%: the thinking is
dropped, the empty remainder is read as a whole reply with no text, and the
wake fails without a cause.

---

## 2. What each contract guarantees

### 2.1 OpenAI chat-completions **[spec]**

Read from the published OpenAPI document
(`github.com/openai/openai-openapi`, `openapi.yaml`, version 2.3.0).

- `finish_reason` is `stop` "if the model hit a natural stop point or a
  provided stop sequence", `length` "if the maximum number of tokens
  specified in the request was reached", `content_filter`, or `tool_calls`.
  A truncation is marked, and the mark is part of the contract.
- `max_completion_tokens` is "an upper bound for the number of tokens that
  can be generated for a completion, including visible output tokens and
  reasoning tokens". Thinking and answer share one budget by specification.
- `max_tokens`, the field the app sends, is "deprecated in favor of
  `max_completion_tokens`, and is not compatible with o-series models". On a
  hosted OpenAI reasoning model the app's request shape is the incompatible
  one.
- `reasoning_effort` is a request field: `none`, `minimal`, `low`, `medium`,
  `high`, `xhigh`, `max`, default `medium`, "for reasoning models". The
  specification does not say what a non-reasoning model does with it. The
  repository's own comment records a strict server rejecting the request
  (`model.rs:76-78`).
- `usage.completion_tokens_details.reasoning_tokens` reports how many of the
  completion tokens were reasoning.
- The `ChatCompletionResponseMessage` schema has exactly `content`,
  `refusal`, `tool_calls`, `annotations`, `role`, `function_call`. The
  streaming `ChatCompletionStreamResponseDelta` has `content`,
  `function_call`, `tool_calls`, `role`, `refusal` and logprob fields.
  **There is no reasoning field on either.** OpenAI's hosted reasoning models
  return reasoning tokens counted but never shown on this surface.

So chat-completions specifies the truncation mark and the shared budget, and
specifies nothing about where reasoning text goes. Every `reasoning_content`
the app has ever seen came from a server extending the schema.

### 2.2 OpenAI Responses **[spec]**

- Reasoning is an output item: `{"type": "reasoning", "id", "summary": [...],
  "content": [...], "encrypted_content", "status"}`. `summary` holds
  `summary_text` parts; `content` holds `reasoning_text` parts; the item is
  distinct from the `message` item that carries `output_text`. Reasoning is
  structurally separate from the answer.
- `reasoning.summary` in the request is `auto`, `concise` or `detailed`; the
  raw chain of thought is not returned, only a summary, and only when asked.
- `encrypted_content` is "populated by default for reasoning items returned
  by `POST /v1/responses`", opaque to the client, and exists so a stateless
  caller (`store: false`, which the app sends) can hand the reasoning back on
  the next turn. It is not for reading.
- `max_output_tokens` is "an upper bound for the number of tokens that can
  be generated for a response, including visible output tokens and reasoning
  tokens", minimum 16. One budget again.
- A response that hits the bound has `status: "incomplete"` and
  `incomplete_details.reason: "max_output_tokens"`. Streaming ends with a
  `response.incomplete` event, not `response.completed`.
- Streaming reasoning arrives as `response.reasoning_summary_text.delta` and
  `response.reasoning_text.delta` events, typed apart from
  `response.output_text.delta`.
- `usage.output_tokens_details.reasoning_tokens` is reported.

Responses is the one OpenAI surface where reasoning is a specified,
separately typed object and truncation is a specified status.

### 2.3 Anthropic Messages **[spec]**

Read from `platform.claude.com/docs/en/build-with-claude/thinking`,
`.../streaming` and the Messages API reference.

- Thinking is a content block: `{"type": "thinking", "thinking": "...",
  "signature": "..."}`, arriving before the `text` blocks. "The thinking
  block is still generated content, like the `text` block that follows it,
  but it is separated from the canonical response."
- `signature` is an encrypted copy of the full reasoning for round-tripping.
  `display: "omitted"` returns thinking blocks with an empty `thinking` field
  and a signature only, and is the default on current models; the client
  opts in to text with `display: "summarized"`.
- Streaming: `thinking_delta` events inside `content_block_delta`, then one
  `signature_delta`, then the block closes. With `display: "omitted"` no
  `thinking_delta` is emitted at all.
- "Thinking tokens count toward `max_tokens`." `budget_tokens` must be at
  least 1,024 and less than `max_tokens`; "the budget is a target rather than
  a strict cap"; `max_tokens` "remains the hard ceiling on total output".
- `stop_reason` is `end_turn`, `max_tokens` ("we exceeded the requested
  `max_tokens` or the model's maximum"), `stop_sequence`, `tool_use`,
  `pause_turn`, or `refusal`.

Anthropic specifies both halves: reasoning is a typed block, truncation is a
typed stop reason. Its budget floor of 1,024 is above the app's whole local
cap, so its thinking budget cannot be borrowed as a number, only as a shape.

### 2.4 The `reasoning_content` convention on local servers **[server]**

None of these servers publishes a contract. Each documents what it emits, and
they disagree on the field name. The table is what each server does when
serving a reasoning model over `/v1/chat/completions`, read from its docs or
source at the dates below.

| Server | Reasoning field | Populated when | Budget lever accepted on the request | `finish_reason` on cap | Source |
|---|---|---|---|---|---|
| vLLM (docs dated 2026-08-03) | `reasoning` on message and delta. "`reasoning` used to be called `reasoning_content`", and a client reading the old name "could silently read an empty `reasoning_content`, even when `reasoning` is populated." | Only with `--reasoning-parser <name>` at serve time. Without it, thinking stays in `content`. | `reasoning_effort`; `thinking_token_budget` (with `--reasoning-config`); `include_reasoning: false` to drop it from the reply. | `length` | `docs/features/reasoning_outputs.md`; `vllm/entrypoints/openai/chat_completion/protocol.py:72` ("vLLM-specific fields that are not in OpenAI spec"), `:245`, `:259`, `:540-556` (accepts `reasoning_content` on input and renames it). |
| llama.cpp server | `reasoning_content` on message and delta (`reasoning_content_delta`). | `--reasoning-format` default `auto`, which resolves to `deepseek` and moves thoughts out of `content`; `none` "leaves thoughts unparsed in `message.content`"; `deepseek-legacy` does both. | `--reasoning-budget N` and `--reasoning-effort LEVEL` are server flags; `reasoning_format` is also a per-request field. | `length` unless the stop was EOS or a stop word (`tools/server/server-task.cpp:415-424`). | `tools/server/README.md` (`--reasoning-format`, "The server supports parsing and returning reasoning via the `reasoning_content` field, similar to Deepseek API"); `common/chat.h:85`, `:131`, `:289-291`. |
| Ollama (OpenAI-compatible surface) | `reasoning` on message and delta, copied from the native `message.thinking`. | Whenever the model is a thinking model and `think` is on; a stream splits a mixed chunk into a reasoning chunk and a content chunk. | `reasoning_effort` and `reasoning: {effort}` are mapped to the native `think` level. gpt-oss "cannot be fully disabled". | `length` (`DoneReasonLength` → `"length"`, `llm/server.go:251-265`). | `openai/openai.go:35`, `:46`, `:294`, `:344-372`, `:536-557`, `:712-718`; `docs.ollama.com/capabilities/thinking`; `docs.ollama.com/api/openai-compatibility`. |
| oMLX 0.6.4 | `reasoning_content` on message and delta. | Always separated when the server recognises the model's thinking markers. For gpt-oss the Harmony `analysis` channel is rewritten to `<think>…</think>` (`omlx/adapter/output_parser.py:155`) and then split by regex (`omlx/api/thinking.py:145`, `extract_thinking`). A `<think>` opened by the prompt is tracked so a body with no opening tag still splits. | `reasoning_effort` (forwarded to the chat template) and `thinking_budget` (per request, `omlx/api/openai_models.py:321-323`). Unknown fields are ignored. | `length` (`omlx/request.py:46`, `omlx/models/llm.py:182`). | Source at commit `d7c63c27`, 2026-09-10; `/openapi.json` on the running server. |
| LM Studio | `reasoning_content` for DeepSeek R1 (0.3.9, opt-in in App Settings); `reasoning` for gpt-oss (0.3.23, "aligning with o3-mini"). Two names on one server, by model. | By model family. | `reasoning.effort` on its Responses surface for gpt-oss. | Not documented on the pages read. | `lmstudio.ai/docs/developer/api-changelog`, entries 0.3.9 and 0.3.23. |
| SGLang | `reasoning_content` on message and delta. | Only with `--reasoning-parser` at serve time and `separate_reasoning: true` on the request. | Model-specific. | Not read. | `docs.sglang.ai/advanced_features/separate_reasoning`; `python/sglang/srt/parser/reasoning_parser.py` (`BaseReasoningFormatDetector`, `stream_reasoning`, `force_reasoning`). |

Three things follow.

1. **[server]** Every server in scope emits the specified `length` value when
   its cap is hit. That half is uniform.
2. **[server]** The reasoning field has two names in current use.
   `reasoning_content` (DeepSeek's API, llama.cpp, oMLX, SGLang, LM Studio
   for R1) and `reasoning` (vLLM current, Ollama, LM Studio for gpt-oss). A
   client that reads one name misses the other. vLLM says so in its own
   migration warning.
3. **[server]** Whether the field is populated at all is a server-side
   setting on vLLM and SGLang and a model-detection outcome on the others.
   The same model can arrive marked on one server and unmarked on another.

### 2.5 Raw `<think>` tags in `content` **[server] [measured]**

A `<think>` tag in `content` is what a client sees when the server did not
separate reasoning: llama.cpp with `--reasoning-format none`, vLLM or SGLang
without a parser, a server that does not recognise a new model's markers, or
a body the server could not parse. oMLX's `extract_thinking` docstring lists
the shapes it handles and the one it gives up on: "`<think>everything…`" with
no close tag is classified as content, "so the response is never empty"
(`omlx/api/thinking.py:145-170`).

That last rule was measured (§6). A Qwen3.5 reply truncated mid-thought
arrives from oMLX with the whole thought in `content` and no tag anywhere in
it, because the template opened the tag in the prompt and the model never
reached the close. A client-side tag parser sees prose.

### 2.6 Sorting the conventions

| Signal | Chat-completions | Responses | Anthropic | ACP | Standing |
|---|---|---|---|---|---|
| Truncation mark | `finish_reason: "length"` | `status: "incomplete"`, `incomplete_details.reason: "max_output_tokens"`, `response.incomplete` | `stop_reason: "max_tokens"` | `stopReason: "max_tokens"` | **Specified on all four.** |
| Reasoning marked apart from the answer | none in the spec | `reasoning` output item; `response.reasoning_*` events | `thinking` block; `thinking_delta` | `agent_thought_chunk` | Specified on three. |
| `reasoning_content` / `reasoning` sibling field | emitted by vLLM, llama.cpp, Ollama, oMLX, LM Studio, SGLang | n/a | n/a | n/a | **De-facto convention**, two names, server-gated. |
| `<think>` inside `content` | when a server does not or cannot separate | n/a | n/a | n/a | **Per-server accident.** |
| Thinking budget separate from the answer | `reasoning_effort` (spec, reasoning models only); `thinking_budget` / `thinking_token_budget` (oMLX, vLLM only) | `reasoning.effort` | `budget_tokens` ≥ 1,024 | none | Effort is specified where reasoning models are; a token budget is not portable. |

---

## 3. What `finish_reason: "length"` with empty content means

**[spec] [server]** On every surface in scope it means one thing: the
generation hit the requested cap before the model emitted an end-of-turn
token. It is specified by OpenAI and emitted with the same value by vLLM,
llama.cpp, Ollama and oMLX (§2.4), so it is reliably distinguishable from
`stop`. The app just never compares the string.

Empty content beside it narrows the cause to two:

1. **The model spent the cap thinking:** Structurally marked on the servers
   that separate reasoning: `reasoning_content` or `reasoning` is non-empty
   and `content` is empty or absent. **[measured]** oMLX with gpt-oss-20b at a
   48-token cap: the message has `role` and `reasoning_content` and no
   `content` key at all; `finish_reason: "length"`; `usage.completion_tokens:
   48`. On Responses the same request returns `status: "incomplete"`,
   `incomplete_details.reason: "max_output_tokens"`, and
   `output_tokens_details.reasoning_tokens: 45` of 48.
2. **The model wrote nothing:** Rare, and indistinguishable from case 1 on a
   server that does not separate reasoning. On such a server the thought is
   in `content` instead, so `content` is not empty and the case does not
   arise; the failure is different and worse (§4).

**[inference]** A `length` finish with empty content is therefore never a
reply, on any server. It is a cap that was too small for this model's
thinking on this prompt. The right response is to say so, in the Action Log
and to the user who owns the knob, and to spend no Speech on it.

What the app does with the Responses form today is worth naming.
`read_event` has an arm for `response.completed` and none for
`response.incomplete`, so an oMLX or hosted Responses truncation ends the body
with no recognised marker and is classified `Streamed::Cut` (`model.rs:1320`, `:1363`),
"the stream ended mid-reply". `retry_settles` says a `Cut` is worth one retry
without `stream` (`model.rs:1292`). The whole-body retry then reads
`output_text`, finds it empty, walks `output` and finds a `reasoning` item
with no `content` array and a `message` item with empty text, and fails with
"model reply had no text content". Two requests, both spent on thinking, to
learn what the first `response.incomplete` event already said.

---

## 4. Markable and capturable, or only heuristic?

**Per endpoint, from the contracts:**

| Endpoint | Reasoning capture | Truncation | Depend on it? |
|---|---|---|---|
| Responses (OpenAI, xAI, oMLX, LM Studio) | Typed item and typed events. | Typed status. | Yes. **[spec]**, and **[measured]** on oMLX. |
| Anthropic Messages (and oMLX's `/v1/messages`) | Typed block; empty text under `display: "omitted"`. | Typed stop reason. | Yes. **[spec]**, and **[measured]** on oMLX: a 48-token gpt-oss reply is one `thinking` block and `stop_reason: "max_tokens"`. |
| chat-completions on a server that separates reasoning | Sibling field, one of two names. | `length`. | Yes for the mark, if the client reads both names. **[server]** |
| chat-completions on a server that does not separate | Thought is in `content`, tagged or untagged. | `length`. | No. Only a heuristic, and the heuristic has a measured hole. |
| ACP | `agent_thought_chunk`. | `stopReason: "max_tokens"`. | Yes. **[spec]** |

**How the heuristic fails, and what it costs.** A tag parser needs a tag.
**[measured]** Qwen3.5-9B on oMLX, whole-body, 48-token cap: `content` is
`"Thinking Process:\n\n1.  **Analyze the Request:** ..."`, `finish_reason:
"length"`, no `reasoning_content`, no `<think>`. The same request streamed
arrives as sixteen `reasoning_content` deltas and one `content` delta. The
same tokens are reasoning on one wire shape and answer on the other, from
one server. Fed to `parse_proposal`, `"Thinking Process:"` fails the
identifier test and the character speaks the model's chain of thought from a
Speech bubble. A thought whose first word happens to be a single token
(`Okay`, `Hmm`) parses as a Behavior name instead, and the Engine rejects it
as undeclared. Neither outcome is recoverable after the fact: once thinking
is in `content` there is no marker left to split on. The gpt-oss model card,
via the Harmony format guide, says the analysis channel "does not adhere to
the same safety standards as final messages" and should not be shown to
users; a heuristic that misses puts exactly that text in the bubble.

**[inference]** The honest boundary is: read what is marked, refuse what is
truncated, and do not try to un-mix a `content` that a server already mixed.
The mixed case is the server's setting to fix (`--reasoning-format`,
`--reasoning-parser`), and the app can name it in the Action Log when a
`length` finish arrives with prose that parsed as nothing.

---

## 5. How harnesses handle it, and what the Completer lane can borrow

**[spec]** ACP's `session/update` carries `agent_message_chunk` for the
answer and `agent_thought_chunk`, "a chunk of the agent's internal reasoning
being streamed", as a distinct discriminator. A turn ends with a `StopReason`:
`end_turn`, `max_tokens` ("the turn ended because the agent reached the
maximum number of tokens"), `max_turn_requests`, `refusal`, `cancelled`.

**In this repository** (`src-tauri/src/acp_wire.rs`):

- `SessionUpdate::AgentThoughtChunk` appends to a per-turn `thought` buffer
  and emits `Event::Thought(line)`, "never into `said`" (`acp_wire.rs:750-757`).
  `harness.rs:1169` forwards it to the Chat surface and logs nothing. That is
  ADR-0025: transient status, replaced by the next thought, cleared at the
  end of the turn.
- The turn's `stop_reason` is matched: `EndTurn => Ok(said)`, anything else
  `=> Err(TurnError::Stopped(name))` (`acp_wire.rs:651-652`). **A Harness turn
  that hit `max_tokens` is already refused as a reply on that lane.** The
  Completer lane accepts the same condition as a finished reply.

**Why the asymmetry exists.** **[inference]** The Harness lane sits on a
protocol whose designers were building for TUIs that draw thinking, so the
discriminator is in the schema and the client was handed both halves. The
Completer lane sits on chat-completions, whose designer never exposed
reasoning text on that surface at all (§2.1), so every local server that
wanted to show thinking invented a field, and two names survived. The
Harness lane also benefits from the adapters upstream: `claude-agent-acp`
turns an Anthropic `thinking_delta` into an `agent_thought_chunk` (issue #483
found the mapping at `acp-agent.js:3242`), so the typed Anthropic block
becomes the typed ACP chunk with no guessing anywhere in the chain.

**What to borrow.** Not the code; the two rules the ACP path already
follows, both of which are contract-driven rather than heuristic:

1. A stop that is not `end_turn` is an error with a name, not a reply.
2. Thinking leaves by its own door and never joins the text a turn returns.

ADR-0025's Decision section states rule 2 for any lane: "Thinking is never
part of the answer. It leaves the wire on its own event and never joins the
text a turn returns." The Completer lane is not exempt from it; it has simply
had nothing to route.

---

## 6. Measured on oMLX

All runs against oMLX 0.6.4 at `http://localhost:8000` on this machine,
2026-09-10, models as loaded by the server. Scripts are throwaway and not in
the repository. Prompts are noted per table. "Toy prompt" is a four-Behavior
roster and one sentence of instruction; "real prompt" is the cat package's
`personality.txt` and roster assembled exactly as
`crates/core/src/director/prompt.rs:10-65` assembles the opening turn, with
`what just happened: poked`.

### 6.1 Wire shapes, gpt-oss-20b-MXFP4-Q8, toy prompt, one request each

| Request | What came back |
|---|---|
| chat-completions, stream, `max_tokens: 48` | 12 frames: 8 `reasoning_content` deltas, 1 `content` delta (`"\n"`), `finish_reason: "length"`, `[DONE]`. |
| chat-completions, whole, `max_tokens: 48` | `message: {role, reasoning_content}`; **no `content` key**; `finish_reason: "length"`; `completion_tokens: 48`. |
| chat-completions, whole, unknown field `fidget_bogus_field: 1` | HTTP 200. oMLX ignores unknown fields. |
| chat-completions, stream, `max_tokens: 512`, `reasoning_effort: "low"` | `finish_reason: "stop"`, 103 chars of reasoning, answer `prowl` plus a line. |
| chat-completions, stream, `max_tokens: 512`, `thinking_budget: 64` | `finish_reason: "stop"`, reasoning cut at 260 chars, answer `prowl` plus a line. |
| `/v1/messages`, `max_tokens: 48` | `content: [{type: "thinking"}]` only, `stop_reason: "max_tokens"`. |
| `/v1/responses`, `max_output_tokens: 48`, `reasoning: {effort: "low"}` | `status: "incomplete"`, `incomplete_details: {reason: "max_output_tokens"}`, `output: [reasoning item with 1 summary part, message item with empty output_text]`, `output_tokens_details.reasoning_tokens: 45`. |
| `/v1/responses`, stream, `max_output_tokens: 48` | 22 events including 8 `response.reasoning_summary_text.delta`, 1 `response.output_text.delta`, ending in `response.incomplete`. No `[DONE]`. |

### 6.2 The same server, a different model: Qwen3.5-9B-8bit, toy prompt

| Request | What came back |
|---|---|
| chat-completions, stream, `max_tokens: 48` | 16 `reasoning_content` deltas, 1 `content` delta; `finish_reason: "length"`. |
| chat-completions, whole, `max_tokens: 48` | `message: {role, content}` where `content` is the thought (`"Thinking Process:\n\n1.  **Analyze the Request:** ..."`); **no `reasoning_content`; no `<think>` tag**; `finish_reason: "length"`. |
| chat-completions, stream, `max_tokens: 512` | 171 `reasoning_content` deltas, 1,846 chars of thought, `finish_reason: "length"`. The model did not finish thinking in 512 tokens either. |
| chat-completions, stream, `max_tokens: 512`, `thinking_budget: 64` | Reasoning closed at 217 chars, then 150 `content` deltas that continue the thought in prose; `finish_reason: "length"`. The budget forced the close tag and the model kept thinking in `content`. |
| `/v1/messages`, `max_tokens: 48` | `content: [{type: "text"}]` holding the thought; `stop_reason: "max_tokens"`. |

Two measured findings the docs do not promise. The streaming and whole-body
paths of one server classify the same truncated thought differently. And a
server-side thinking budget is only as good as the model's obedience to a
forced close tag; on this model it moved the thinking into the answer.

### 6.3 Empty-reply rate by lever, gpt-oss-20b-MXFP4-Q8, toy prompt, 10 runs each, non-streaming

| Config | `finish_reason: "length"` | Empty `content` | Valid first line | Completion tokens, median (max) |
|---|---|---|---|---|
| `max_tokens: 512` | 0 | 0 | 10 | 209.5 (319) |
| `max_tokens: 512`, `reasoning_effort: "low"` | 0 | 0 | 10 | 63.5 (113) |
| `max_tokens: 512`, `thinking_budget: 128` | 0 | 0 | 10 | 139.5 (152) |
| `max_tokens: 80` | 9 | 9 | 1 | 80 (80) |
| `max_tokens: 80`, `reasoning_effort: "low"` | 2 | 0 | 10 | 54.5 (80) |

The toy prompt is short and the model finished under 512 every time, which
#541 did not see with the real prompt. The rows to read are the 80-token
ones: the hosted cap, applied to this model, fails nine times in ten, and
`reasoning_effort: "low"` alone takes that to zero empty replies with every
first line valid. The two `length` finishes under `low` had a valid Behavior
name and a cut spoken line.

### 6.4 Empty-reply rate by lever, gpt-oss-20b-MXFP4-Q8, real prompt, 20 runs each, non-streaming

| Config | `finish_reason: "length"` | Empty `content` | Valid first line | Completion tokens, median (max) | Reasoning chars, median |
|---|---|---|---|---|---|
| `max_tokens: 512` | **7** | **7** | 13 | 458 (512) | 1,662 |
| `max_tokens: 512`, `reasoning_effort: "low"` | 0 | 0 | 19 | 82 (176) | 163 |
| `max_tokens: 512`, `thinking_budget: 128` | 0 | 0 | 20 | 173.5 (190) | 524 |

The first row reproduces #541 from outside the app: 7 of 20 against its 6 of
20, median 458 tokens against its 479. The cap is not the variable; the
prompt is. The real Character Prompt makes this model think about 1,700
characters at its default effort, and 512 tokens holds that about two times
in three. `reasoning_effort: "low"` cuts the thinking by ten and the failure
to zero in twenty. oMLX's `thinking_budget` also took it to zero here, on
this model; §6.2 shows the same field misfiring on Qwen3.5, which is why the
recommendation does not lean on it.

---

## 7. Recommendation

**Read the wire's own markers. Refuse a `length` finish with empty content as
a reply, and route a marked reasoning delta to the ADR-0025 thought door.
Leave the cap where it is.** One mechanism, two halves, both contract-driven,
no heuristic.

What it is, concretely, on the Completer lane:

1. `read_event` reads the value of `finish_reason` and the `response.incomplete`
   event. A `length` or `max_output_tokens` end with no text is a new
   `Unsent` variant, say `Truncated`, that names the cap in force. It is
   never a reply, never Speech, and it is logged with the cap and the model
   so the Action Log answers "why did the character go quiet" with "512 tokens
   was not enough for gpt-oss-20b at its default effort". `retry_settles`
   returns `None` for it: the same question at the same cap gets the same
   nothing, as the test at `model.rs:2229` already argues for the empty case.
2. `read_event` reads `delta.reasoning_content` and `delta.reasoning`, the
   two names §2.4 found in use, and `response.reasoning_summary_text.delta`,
   and yields them as a thought rather than as text. The Completer forwards a
   thought the way the Harness does (`Forwarded::Thought`), and the Chat
   surface draws it where ADR-0025 says: one line above the composer,
   replaced by the next, cleared at the end of the turn, kept by nothing.
3. Nothing is done about a thought that arrived in `content`. It parses as
   it parses today. When a `length` finish arrives with prose that produced
   no Behavior, the Action Log line names the likely cause so the user knows
   which server setting to change.

**Why this and not the others.**

- *A bigger cap.* #541's median was 479 of 512 on the real prompt; Qwen3.5
  did not finish at 512 in §6.2. No number is safe for the next model,
  because no chat-completions contract bounds thinking apart from the
  answer (§2.6). The maintainer ruled tuning out, and the contracts agree
  with him: a cap is the wrong knob for this failure. The knob the user
  already has (`FIDGET_DIRECTOR_MAX_TOKENS`) stays, and the new `Truncated`
  line tells them when to turn it.
- *Separate budgets for thinking and answer.* Not portable. `thinking_budget`
  is oMLX's field, `thinking_token_budget` is vLLM's, llama.cpp takes it as
  a server flag only, Ollama and OpenAI chat-completions have none, and
  Anthropic's floor is 1,024. It worked on gpt-oss (§6.4, zero in twenty)
  and misfired on the second model measured (§6.2, Qwen3.5 kept thinking in
  `content` after the forced close), so it is a per-server, per-model lever,
  not a contract.
- *A streaming parser that routes thinking away from the reply.* This is
  half 2, restricted to what is marked. A parser that also splits `<think>`
  out of `content` is the heuristic §4 shows failing without a tag to find,
  and the mixed case is a server setting.
- *A capability probe per endpoint.* It would tell us which name the server
  uses and whether it separates reasoning. Reading both names on every frame
  costs one string compare and needs no probe. Probing for
  `reasoning_effort` acceptance is a different question, below.
- *Refusing an empty `length` finish as a reply.* This is half 1. On its own
  it stops the garbage parse and the wasted whole-body retry, and it makes
  the failure visible. It does not by itself lower the failure rate, which is
  why it is paired with the lever that does.

**The lever.** §6.3 and §6.4 measure `reasoning_effort: "low"` as the one
request field that changes the empty-reply rate, and §2.4 finds it accepted
by every local server in scope (vLLM, llama.cpp via template, Ollama mapped
to `think`, oMLX forwarded to the template, LM Studio on Responses) and
specified by OpenAI for reasoning models. The open hazard is the one the
constant's comment recorded: a strict server, or a hosted non-reasoning
model, may reject the whole request over the field. The repository already
has the pattern for exactly this. `Unsent::NotStreamable` retries once
without `stream` and remembers the answer per host (`model.rs:1260-1295`).
The same one-retry-and-remember shape fits `reasoning_effort`. That is a
follow-up with its own measurement against a hosted endpoint, not this
spike's to decide, and the recommendation above stands without it.

**What it costs.**

- A new `Unsent` variant, the two `read_event` arms, the `response.incomplete`
  arm, and tests for each shape in §6.1. Small, in `model.rs`.
- A thought path from the Completer to the Chat surface. The Harness lane
  has `Forwarded::Thought`; the Completer's replies travel a different route
  to the Shell, and giving them a thought event is the plumbing cost of half
  2. It is the larger of the two halves and can land second.
- One Action Log line kind for a truncated wake, and the words for it.

**What it breaks.**

- A `length` finish with a valid first line and a cut second line, which
  today is accepted and spoken with half a sentence, becomes `Truncated` and
  is not spoken. §6.3 measured that at two in ten under an 80-token cap with
  `low` effort, and zero in ten at 512. #302 already refused a cut stream for
  the same half-sentence reason; this makes the two ends of the wire agree.

  **Overruled when it shipped (#610, #614).** A `length` finish that wrote
  text is refused as a *turn* — never a success, never retried, the cap named
  in the Action Log — and still shown: the Behavior is played and the words
  are said. We are the ones who cut the model off, so what it managed to write
  is not thrown away. The reply is written down as `[response truncated]`
  under the words, in the session the next turn is built from and in the Chat
  history, so a reader and the model both see where it stopped; what the character
  speaks stays the model's own words. The empty case is unchanged, and is
  silence.
- Any server that marks a completed reply with a `finish_reason` other than
  `stop` or `length` (`tool_calls`, `content_filter`) is unchanged, since
  only `length` is refused.
- The test at `model.rs:2229` changes meaning: the reasoning-only stream
  still proves the body was a stream, and now also proves the thought was
  routed and the finish was read.

**Does it need an ADR?** No new one. ADR-0025 already decides what a thought
is and where it goes, for any lane, and its Decision paragraph does not name
the Harness. Half 1 is mechanism: a truncation is an error with a name, which
the ACP path already does at `acp_wire.rs:652` without an ADR. If the
maintainer wants the Completer lane named under ADR-0025's Consequences, that
is a one-line addition to an existing ADR, not a decision to record.

---

## What this could not settle

- Hosted OpenAI behaviour for `reasoning_effort` on a non-reasoning model,
  and for the deprecated `max_tokens` on an o-series model, were read from
  the specification and not measured. No hosted key was spent on this spike.
- LM Studio and SGLang were read, not run. Neither is installed here.
- The 40% in #541 was measured with the app's benchmark against the real
  prompt in a session with the app's history; §6.4 is a fresh single-turn
  request each time, so the two baselines are comparable in shape and not in
  exact number.
- Whether an oMLX `thinking_budget` misfires on gpt-oss the way it did on
  Qwen3.5 was not tested beyond the one run in §6.1, which behaved.
- `docs/research/director-in-flight-and-latency.md` §2.5 and item 4 of its
  recommendations cited `model.rs` line numbers that had moved; #613 replaced
  them with symbol names. The argument there still holds and this document
  supersedes its lines on the effort lever.
