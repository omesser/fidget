#!/usr/bin/env bash
# Usage: FIDGET_HARNESS=hermes scripts/probe-harness.sh, plus FIDGET_HARNESS_CWD
# and FIDGET_MCP_BIN. Runs one ACP turn, no sprite, and never prints a credential.
# Exit codes: 2 nothing asked or bad variable, 1 no answer, 0 end_turn.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

exec cargo run -q -p fidget -- --probe-harness
