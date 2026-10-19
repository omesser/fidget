#!/usr/bin/env bash
# Rank the live bytes in a `malloc_history PID -allBySize` report by the
# innermost frame inside IMAGE, the deepest code of that image on the stack.
# Usage: scripts/heap-top-allocators.sh FILE IMAGE [N]

set -euo pipefail

# BSD awk has no asort, so sort ranks the rows; its leading byte count is cut.
awk -v image="$2" -v top="${3:-20}" '
  /^[0-9]+ calls? for [0-9]+ bytes: / {
    bytes = $4
    total += bytes
    n = split(substr($0, index($0, ": ") + 2), frames, " [|] ")
    tag = " (" image ") "
    symbol = ""
    for (i = n; i > 0 && symbol == ""; i--)
      if (at = index(frames[i], tag)) symbol = substr(frames[i], at + length(tag))
    sub(/[ \t]+$/, "", symbol)
    if (symbol == "") { orphan += bytes; next }
    live[symbol] += bytes
    calls[symbol] += $1
  }
  END {
    printf "top %d allocators in %s, live bytes by innermost frame\n", top, image
    sorter = "sort -rn | head -n " top " | cut -f 2-"
    for (s in live) printf "%d\t%8.0f KB %7d calls  %s\n", live[s], live[s] / 1024, calls[s], s | sorter
    close(sorter)
    printf "%8.0f KB live in total, %.0f KB of it with no %s frame\n", total / 1024, orphan / 1024, image
  }
' "$1"
