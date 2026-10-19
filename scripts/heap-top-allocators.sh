#!/usr/bin/env bash
# Rank the live bytes in a `malloc_history PID -allBySize` report by the
# innermost frame inside IMAGE that is not allocator glue: Rust's std and alloc
# are linked into fidget, so its innermost frame is nearly always __rust_alloc.
# Usage: scripts/heap-top-allocators.sh FILE IMAGE [N]

set -euo pipefail

# The glue patterns match demangled names, and the report's Rust symbols are
# mangled. BSD awk has no asort, so sort ranks the rows; its leading count is cut.
c++filt < "$1" | awk -v image="$2" -v top="${3:-20}" '
  /^[0-9]+ calls? for [0-9]+ bytes: / {
    bytes = $4
    n = split(substr($0, index($0, ": ") + 2), frames, " [|] ")
    # Stack logging also records VM regions at their virtual size, gigabytes for
    # a 100 MB process, so only records that end in malloc are heap.
    if (frames[n] !~ / \(libsystem_malloc\.dylib\) /) { regions += bytes; next }
    total += bytes
    tag = " (" image ") "
    symbol = innermost = ""
    for (i = n; i > 0 && symbol == ""; i--) {
      if (!(at = index(frames[i], tag))) continue
      frame = substr(frames[i], at + length(tag))
      sub(/[ \t]+$/, "", frame)
      if (innermost == "") innermost = frame
      if (frame !~ /^(\?\?\?|__rust_|__rdl_|__rg_|<?(alloc|core|std)::)/) symbol = frame
    }
    if (symbol == "") symbol = innermost
    if (symbol == "") { orphan += bytes; next }
    live[symbol] += bytes
    calls[symbol] += $1
  }
  END {
    printf "top %d allocators in %s, live bytes by innermost frame\n", top, image
    sorter = "sort -rn | head -n " top " | cut -f 2-"
    for (s in live) printf "%d\t%8.0f KB %7d calls  %s\n", live[s], live[s] / 1024, calls[s], s | sorter
    close(sorter)
    printf "%8.0f KB live in malloc zones, %.0f KB of it with no %s frame\n", total / 1024, orphan / 1024, image
    printf "%8.0f KB virtual in VM regions outside malloc, not a resident figure\n", regions / 1024
  }
'
