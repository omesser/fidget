# The app dispatches its own tools, and the transport follows

## Context

A Harness reaches fidget's tools over MCP. Making one play a
Behavior means resolving a target against the live Instances and enqueueing a
proposal on the layer that owns them. That layer is owned by the frame loop and
has no representation outside the process.

The published MCP binary was a separate process, so it could not reach any of
that. It answered from a stubbed context: no window sensing, no Instances, no
expression handle. Tool calls returned success and nothing happened on screen,
which is indistinguishable from working.

Two ways to close that gap were available. The app could serve MCP itself, or
the published binary could shrink to a shim that forwards to the running app
over a local channel.

## Decision

**Tools are dispatched inside the running app.** The app serves MCP on loopback
behind a per-run bearer token, and hands that endpoint to any Harness whose
handshake says it can use one.

A Harness that cannot is given the stdio server instead, which relays to the
same endpoint ([ADR-0026](./0026-stdio-mcp-is-a-shim-dialled-from-the-environment.md)).

The shim was rejected as the first move for the same reason the stub failed:
either way the dispatch has to happen inside the app, so serving directly is
the same decision with one less process in it.

## Consequences

Where a tool is dispatched is now an ownership seam rather than an accident of
packaging. Anything that needs the live Instances belongs inside the app, and a
second process can only ask.

The endpoint is reachable only from this machine, authorised per run, and
refused to anything that presents a browser origin or a method other than the
one it answers. A token that could be read from a log or a file would hand a
local process the ability to move the character, so it is held in memory and passed
in a header.

Reversing this means accepting that tool calls cannot reach the Instances, which
is the state this replaced.
