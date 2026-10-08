# The ACP client uses the official SDK and supported Harnesses are named

**Supersedes:** [ADR-0017](https://github.com/omesser/fidget/blob/3d16d6fc5d9dc8222861a49c05ca68fc4b0053ce/docs/adr/0017-acp-client-over-the-official-sdk-and-supported-harnesses.md) (removed).

## Context

Agent Client Protocol (ACP) is how fidget talks to external agent harnesses.
We need to choose an ACP client implementation and decide which harnesses to
support by name versus requiring users to configure command lines themselves.

Three approaches exist:
1. The official `agent-client-protocol` 2.x SDK maintained by the protocol's
   own repository
2. The `acp-cli` library (pre-1.0, pins older protocol versions, carries CLI
   dependencies)
3. Hand-rolled JSON-RPC (no protocol crate, manually tracking protocol changes)

For harness selection, we could enumerate seventeen agents inherited from CLI
tools, or maintain a focused list of verified harnesses with an escape hatch.

## Decision

fidget is an ACP client built on `agent-client-protocol` 2.x, the SDK Zed
ships and the protocol's own repository maintains.

The product layer owns the policy around the wire: launch table, spawn/respawn
logic and backoff, authentication gate, session file, and what reaches the
Action Log and Chat surface.

The wire is isolated so the frame loop never sees futures (ADR-0004).

The launch table names a focused list of verified harnesses and provides an
escape hatch for any custom command. `HARNESS_PRESETS` is that list. The count
is not what was decided, so it is not recorded here (#777). Harnesses earn
verified standing once a turn has been smoked against them, fresh and resumed.
A name in the table is a command line as much as a label, because a vendor's
bare binary is usually its interactive TUI and its ACP mode a subcommand.

One session per app lifetime, persisted across restarts when the Harness
supports `loadSession`.

## Consequences

The Harness is the Completer for every Instance. ADR-0008's one
conversation holds across wakes for that Instance — Director, Chat, and
tools — not across Character Instances. The Decision's "one session per
app lifetime" is the ACP connection; session ids are per Instance (and
Character identity on retarget). #558.

A permission request is forwarded to the Chat surface of the Instance whose
session asked, or to every open one when no Instance owes it, and answered only
by a click there. A turn that times out sends `cancelled`, which is a
withdrawal, not an answer.

The client hands the session its own tool endpoint, and which endpoint that is
belongs to [ADR-0023](./0023-app-dispatches-its-own-tools.md) and
[ADR-0026](./0026-stdio-mcp-is-a-shim-dialled-from-the-environment.md): either
way the call reaches the live Instances.

A protocol-compatible harness not yet named is reachable through the custom
command and earns a named row once a turn has been smoked against it.
Protocol compatibility alone does not earn the row: Copilot CLI had it from
the start and was named only once smoked (#1016). Antigravity's row names
Google's own ACP server, `agy_acp_server`, because `agy` has no ACP mode
(#604).
Their always-approve or auto-approve flags are never passed by default — the
Chat surface owns permissions.
