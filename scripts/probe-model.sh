#!/usr/bin/env bash
# Usage: scripts/probe-model.sh. Hits the Director Completer with the overlay's
# env and HTTP client, printing only the key's length and last four. With a local
# base URL the key is optional.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

exec cargo run -q -p fidget -- --probe-model
