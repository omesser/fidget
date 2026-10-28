#!/usr/bin/env bash
# Usage: follow-sprite.sh LOG HALF_WIDTH HALF_HEIGHT
#
# Keeps the X pointer on the sprite's centre: reads the frame lines that
# FIDGET_TRACE_FRAMES=1 writes to LOG and moves the pointer with xdotool
# once the sprite has moved 8 px, so one call per tick never queues up
# behind a walk. Callers start it under setsid and kill its group.
#
# Bash parses the lines: mawk, Debian's awk, buffers a pipe's input and
# would hold the moves back until the log grows by a block.
set -euo pipefail

log="$1"
half_w="$2"
half_h="$3"
last_x=-1000
last_y=-1000
tail -n +1 -F "$log" 2> /dev/null | while IFS= read -r line; do
  [[ "$line" =~ \ frame:\ .*\ sprite\((-?[0-9]+),(-?[0-9]+)\) ]] || continue
  x=$((BASH_REMATCH[1] + half_w))
  y=$((BASH_REMATCH[2] + half_h))
  if (((x - last_x) ** 2 + (y - last_y) ** 2 >= 64)); then
    xdotool mousemove -- "$x" "$y"
    last_x=$x
    last_y=$y
  fi
done
