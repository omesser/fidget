#!/usr/bin/env bash
# Usage: scripts/heap-top-allocators.sh FILE IMAGE [N]. Ranks live bytes in a
# `malloc_history PID -allBySize` report by innermost frame inside IMAGE; Rust's
# std and alloc are linked in, so that frame is usually `__rust_alloc`.

set -euo pipefail

# Glue patterns match demangled names, and the report's Rust symbols are mangled.
# GNU c++filt keeps v0 crate hashes, which sed drops. BSD awk has no asort, so
# sort ranks the rows and cut drops its count.
c++filt < "$1" | sed -E 's/\[[0-9a-f]{16}\]//g' | awk -v image="$2" -v top="${3:-20}" '
  /^[0-9]+ calls? for [0-9]+ bytes: / {
    bytes = $4
    n = split(substr($0, index($0, ": ") + 2), frames, " [|] ")
    # Stack logging records VM regions at virtual size, gigabytes for a 100 MB
    # process, so only records ending in malloc are heap.
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
