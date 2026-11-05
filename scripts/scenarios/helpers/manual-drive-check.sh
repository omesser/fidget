# shellcheck shell=bash
# Shared by the macOS and X11 leaves. The Windows leaf keeps the same messages.
# A full check wants Escape in the trace and k or K in the letter file.
# pause: the click-away has landed and Escape has not been pressed yet.

# The focus button, else the Paused heading. The heading sits against the
# quick-message field, so a click on its centre can miss the card.
card_center() { # <dump>
  awk -F'|' '
    {
      n = split($NF, a, ",")
      if (n != 4 || a[3] + 0 <= 0 || a[4] + 0 <= 0) next
      if (index($0, "Click to focus")) {
        bx = a[1] + a[3] / 2
        by = a[2] + a[4] / 2
        button = 1
      } else if (!heading && index($0, "Paused")) {
        hx = a[1] + a[3] / 2
        hy = a[2] + a[4] / 2
        heading = 1
      }
    }
    END {
      if (button) printf "%d %d\n", bx, by
      else if (heading) printf "%d %d\n", hx, hy
    }
  ' "$1"
}

check_manual_drive() { # <trace> <letter file> [pause]
  local trace=$1 letter=$2 mode=${3:-done}
  if ! awk -v mode="$mode" '
    function miss(m) { print m > "/dev/stderr"; exit 1 }
    {
      line = $0
      sub(/\r$/, "", line)
      if (line ~ /^[0-9]+ /) {
        rest = substr(line, index(line, " ") + 1)
        if (rest ~ /^(frame:|menu:|possession:|overlay )/) line = rest
      }
      if (!on) {
        if (line ~ /^menu: possession on$/) on = 1
        next
      }
      # The release plays react. That motion is after the pause this check owns.
      if (escaped) next
      if (line ~ /^possession: escape$/) { escaped = 1; next }
      if (line !~ /^frame: /) next
      if (!match(line, /sprite\(-?[0-9]+,-?[0-9]+\)/)) next
      token = substr(line, RSTART + 7, RLENGTH - 8)
      split(token, xy, ",")
      n++
      xs[n] = xy[1] + 0
      fall[n] = (line ~ / Falling /)
      walk[n] = (line ~ / walk#/)
    }
    END {
      if (!on) miss("possession never turned on")
      if (mode != "pause" && !escaped) miss("Escape did not release possession")
      if (n < 1) miss("the held Right did not walk the sprite")
      for (i = 1; i <= n; i++) if (fall[i]) miss("the pause launched a jump")
      origin = xs[1]
      lastwalk = 0
      maxx = origin
      for (i = 1; i <= n; i++) if (walk[i]) {
        lastwalk = i
        if (xs[i] > maxx) maxx = xs[i]
      }
      if (!lastwalk || maxx - origin < 8) miss("the held Right did not walk the sprite")
      if (n - lastwalk < 5) miss("the sprite kept walking after the other window took focus")
      spot = xs[lastwalk + 1]
      for (i = lastwalk + 1; i <= n; i++) {
        if (walk[i] || xs[i] - spot > 2 || spot - xs[i] > 2)
          miss("the sprite kept walking after the other window took focus")
      }
      exit 0
    }
  ' "$trace"; then
    return 1
  fi
  if [ "$mode" = pause ]; then
    echo "ok: Right walked the sprite and the pause held the jump"
    return 0
  fi
  if ! grep -q '[Kk]' "$letter"; then
    echo "the letter did not reach the other window" >&2
    return 1
  fi
  echo "ok: Right walked the sprite, the pause held, Escape released it, and the letter landed"
}
