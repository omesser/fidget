#!/usr/bin/env bash
# Run one ACP turn against the configured Harness, no sprite; never prints a
# credential. Usage: FIDGET_HARNESS=hermes scripts/probe-harness.sh (also reads
# FIDGET_HARNESS_CWD, FIDGET_MCP_BIN, and FIDGET_PROBE_THEN_CONFIGURE for an
# Apply and a second turn: space-separated model=<id> and effort=<level>, either
# or both, e.g. FIDGET_PROBE_THEN_CONFIGURE='effort=high'). Exit 2: never asked
# or a bad FIDGET_PROBE_THEN_CONFIGURE, 1: unanswered, 0: end_turn.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

exec cargo run -q -p fidget -- --probe-harness
