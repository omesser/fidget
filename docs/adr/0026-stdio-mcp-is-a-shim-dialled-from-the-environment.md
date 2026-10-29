# The stdio MCP binary is a shim, and it is handed the endpoint in its environment

## Context

ADR-0023 put dispatch inside the running app, which serves MCP on loopback
behind a per-run bearer token. The gate is one bit of the ACP handshake:
`agentCapabilities.mcpCapabilities.http` on `initialize`. A Harness that does
not set it — hermes does not — is given the stdio binary instead. That is a
statement about the handshake and nothing else: hermes is itself an MCP client
that speaks Streamable HTTP and SSE through its own `mcp_servers` config, and
a Harness that starts setting the bit would take ADR-0023's path with no change
here. That binary
answered from stubs: no window sensing, no Instances, no expression. An Expression
there returned success and nothing appeared on screen, which is
indistinguishable from working (#470, #501).

The binary cannot fix that in its own process. The live Instances belong to the
app's frame loop and have no out-of-process form, so a second process can only
ask the first one.

Asking means finding the app's port and its token. The token is 32 fresh bytes
per app run held in memory, and a token another local user can read is that
user's ability to move the character, so it may not reach a log, a trace, or a
file.

## Decision

**The stdio binary holds no state and relays to the app's loopback endpoint.**
It parses each message only far enough to report a failure against it; the
app's answer is passed back untouched. There is no second implementation of the
tools to keep in step, and no stubbed one.

**All three stdio routes relay, the app re-executing itself included.** A
sidecar, a binary named by the environment, and the app binary run as
`--mcp-stdio` are three ways to start one process, and none of them is the
process holding the Instances — the running app is. Letting the third answer
from inside itself would mean a second dispatch path that only one route
reaches, which is the divergence ADR-0023 closed.

**The endpoint is discovered from the process environment, not from a
descriptor file.** The app sets `FIDGET_MCP_URL` and `FIDGET_MCP_TOKEN` on
the MCP server entry it hands the Harness, and the Harness applies them when it
spawns the shim. A file would have to be owner-only, in a directory that is
also owner-only, survive a crash without going stale, and be cleaned up on
exit; the environment needs none of that, and the token still dies with the app
run. The MCP server entry is already the app's own to fill — this sets nothing
of the user's and touches no vendor credential
([ADR-0018](./0018-headless-harness-our-chat-surface.md)).

**A shim with nothing to dial answers an error, never a success.** An unset
variable, a refused connection, a rejected token and an answer too large or too
empty to trust are all one contract: the request is answered with a failure the
Harness can see, and a notification, which gets no response, is reported on
standard error alone. The URL is refused unless it names this machine, because
the token authorises moving the character and a variable naming another host would
post it there.

## Consequences

Every Harness reaches the same dispatch inside the app, whether it dials
loopback itself or is given the shim. There is no second tool surface to keep
in step, and no path that reports success while nothing moves.

A Harness that drops the environment on the MCP servers it spawns gets no
tools, loudly, rather than stub ones quietly. That is the trade the failure
contract buys.

The token remains absent from disk. `FIDGET_MCP_TOKEN` is visible in the
shim's own environment, which on macOS and Linux only the same user can read —
the same protection the Harness's own credential file already relies on.

An app that is not running, or one whose loopback bind failed, hands the shim
nothing — and a Harness that spawns it anyway is told so on every call. That is
the same answer as before for a user with no app open, said out loud.

Reversing this means a descriptor file with its permissions, staleness and
cleanup, or accepting that a Harness which does not advertise the bit has no
working tools.

**For one Harness the endpoint does go in a file, and it is that Harness's own
config (#1020).** `cursor-agent` reads `mcpServers` from neither `session/new`
nor its environment. It loads servers only from an approved `.cursor/mcp.json`,
so the only way to reach it is to write there. Attach merges one entry into
`<cwd>/.cursor/mcp.json` holding the same loopback URL and `Authorization`
header an HTTP-capable Harness is handed directly, then runs `cursor-agent mcp
enable fidget` before spawning `acp`.

This is the reversal named above, taken deliberately and scoped to one Harness.
The costs it predicted are the ones now paid, and they are paid rather than
argued away. Permissions: the file is written 0600, because it holds a live
credential. Staleness: the token is 32 fresh bytes per app run, so a copy that
outlives the app authorises nothing. Cleanup: detach removes the entry, and the
file and directory when attach created them.

What it buys is that `cursor-agent` uses the same transport as every other
Harness, on every platform fidget ships. A unix socket with no token was built
and measured first and would have kept the credential off disk entirely, but it
is `cfg(unix)`, and leaving Windows without tools to avoid a 0600 file in a
directory the user already controls is the worse trade.

What remains is real and is not engineered around: while a session is attached
to a user-set Working directory, a live token sits in that project's
`.cursor/mcp.json`. It is the length of one session, it is owner-only, and it
authorises fidget's tools and nothing else. The shim and the two environment
variables are untouched; `cursor-agent` no longer uses either.
