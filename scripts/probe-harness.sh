#!/usr/bin/env bash
# Run one ACP turn against the configured Harness, no sprite. It never prints a
# credential. Usage: FIDGET_HARNESS=hermes scripts/probe-harness.sh. It also
# reads FIDGET_HARNESS_CWD and FIDGET_MCP_BIN.
# FIDGET_PROBE_THEN_CONFIGURE adds an Apply and a second turn. It takes
# space-separated model=<id> and effort=<level>, either or both, for example
# FIDGET_PROBE_THEN_CONFIGURE='effort=high'.
# Exit codes: 2 when nothing was asked or the variable is bad, 1 when the Harness
# did not answer, 0 for end_turn.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

exec cargo run -q -p fidget -- --probe-harness
