#!/usr/bin/env bash
# Click Copy on the Settings webview BYO row and read the pasteboard.
# Usage: [FIDGET_VERIFY_BIN=path/to/fidget] ./scripts/verify-settings-webview-clipboard-macos.sh
# Needs Accessibility. Stills land in .verify/. Holds /tmp/fidget-settings-overlay.lock.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

bin="${FIDGET_VERIFY_BIN:-$root/target/debug/fidget}"
out="$root/.verify/settings-webview-clipboard-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"

failures=0
lock_held=0
app_pid=""

info() { printf '\033[36m[INFO]\033[0m %s\n' "$*"; }
pass() { printf '\033[32m[PASS]\033[0m %s\n' "$*"; }
skip() { printf '\033[33m[SKIP]\033[0m %s\n' "$*"; }
fail() {
  printf '\033[31m[FAIL]\033[0m %s\n' "$*"
  failures=$((failures + 1))
}

trap '[ -n "$app_pid" ] && kill "$app_pid" 2> /dev/null; [ "${lock_held:-0}" -eq 1 ] && rmdir /tmp/fidget-settings-overlay.lock 2> /dev/null' EXIT

[ -x "$bin" ] || {
  fail "no binary at $bin - cargo build -p fidget first, or set FIDGET_VERIFY_BIN"
  exit 1
}

if [ "${FIDGET_OVERLAY_LOCK:-1}" = 1 ]; then
  deadline=$((SECONDS + 1200))
  while ! mkdir /tmp/fidget-settings-overlay.lock 2> /dev/null; do
    info "waiting for /tmp/fidget-settings-overlay.lock"
    if [ "$SECONDS" -ge "$deadline" ]; then
      fail "overlay lock not free after 20 minutes"
      exit 1
    fi
    sleep 30
  done
  lock_held=1
fi

ax="$out/ax-settings"
info "Output: $out"
swiftc -O "$root/scripts/ax-settings.swift" -o "$ax" || {
  fail "could not compile scripts/ax-settings.swift"
  exit 1
}

home="$out/home"
mkdir -p "$home"
link=""
for link in .claude .claude.json .codex .config; do
  [ -e "$HOME/$link" ] && ln -sfn "$HOME/$link" "$home/$link"
done

log="$out/app.log"
env -u FIDGET_DIRECTOR_API_KEY \
  HOME="$home" \
  FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=timber-wolf \
  FIDGET_CHARACTERS="${FIDGET_CHARACTERS:-$root/characters}" \
  "$bin" > "$log" 2>&1 &
app_pid=$!

ready=0
waited=0
while [ "$waited" -lt 40 ]; do
  if grep -qE '^overlay:' "$log" 2> /dev/null; then
    ready=1
    break
  fi
  if ! kill -0 "$app_pid" 2> /dev/null; then
    fail "fidget exited before overlay:"
    tail -40 "$log" || true
    exit 1
  fi
  sleep 0.5
  waited=$((waited + 1))
done
[ "$ready" -eq 1 ] || info "no overlay: line yet; continuing because Settings does not need the sprite"

"$ax" open "$app_pid" || {
  fail "could not open Settings from the tray"
  exit 1
}
"$ax" tab "$app_pid" AI || {
  fail "could not open the AI tab"
  exit 1
}

dump="$out/ai.txt"
"$ax" dump "$app_pid" > "$dump" || {
  fail "could not dump the AI tab"
  exit 1
}

still() {
  local name="$1"
  local rect="${2-}"
  if [ -z "$rect" ]; then
    rect=$("$ax" frame "$app_pid" 2> /dev/null) || {
      info "no Settings frame for $name"
      return 0
    }
  fi
  [ -n "$rect" ] || return 0
  screencapture -x -o -R "$rect" "$out/${name}.png" 2> /dev/null
}

downscale() {
  local src="$1"
  local dst="$2"
  [ -f "$src" ] || return 0
  sips -Z 420 "$src" --out "$dst" > /dev/null
}

# The InspectBlock is a <pre>; WebKit publishes it as AXStaticText whose
# value is the snippet. Unescape the dump's one-line \\n encoding.
snippet=$(awk -F'|' '$3 ~ /mcp add/ { gsub(/\\n/, "\n", $3); print $3; exit }' "$dump")
if [ -z "$snippet" ]; then
  fail "AI dump has no BYO snippet (no line matching mcp add)"
  head -80 "$dump"
  exit 1
fi
printf '%s\n' "$snippet" > "$out/snippet.txt"
info "snippet: $snippet"

sentinel="SENTINEL-855-$(date +%s)"
printf '%s' "$sentinel" | pbcopy
before=$(pbpaste)
printf '%s\n' "$before" > "$out/paste-before.txt"

button_rect=$("$ax" press-button "$app_pid" Copy 1) || {
  fail "could not press the first Copy button"
  exit 1
}
printf '%s\n' "$button_rect" > "$out/copy-button-rect.txt"
sleep 0.6
got=$(pbpaste)
printf '%s\n' "$got" > "$out/paste-after.txt"

still "01-ai-after-copy"
if [ -n "$button_rect" ]; then
  still "01-copy-button" "$button_rect"
fi

downscale "$out/01-ai-after-copy.png" "$out/01-ai-after-copy-sm.png"
if [ -f "$out/01-copy-button.png" ]; then
  downscale "$out/01-copy-button.png" "$out/01-copy-button-sm.png"
fi

if [ "$got" = "$snippet" ]; then
  pass "first Copy wrote the BYO snippet to the pasteboard"
elif [ "$got" = "$sentinel" ] || [ "$got" = "$before" ]; then
  fail "first Copy left the pasteboard unchanged (still $got)"
else
  fail "first Copy wrote something other than the snippet"
  info "wanted: $snippet"
  info "got:    $got"
fi

# The token InspectBlock is a preformatted group between the two Copy
# buttons. Claude leaves AXEmptyGroup there, so the second Copy is a
# visible no-op and must not wipe the snippet we just proved.
token=$(awk -F'|' '
  $1 ~ /AXButton/ && $2 == "Copy" { copies++; next }
  copies == 1 && $1 ~ /AXPreformatted/ { pre = 1 }
  copies == 1 && pre && $1 ~ /AXStaticText/ && length($3) > 0 {
    gsub(/\\n/, "\n", $3)
    print $3
    exit
  }
' "$dump")

copy_count=$(awk -F'|' '$1 ~ /AXButton/ && $2 == "Copy" { n++ } END { print n+0 }' "$dump")
printf 'copy_buttons=%s\ntoken=%s\n' "$copy_count" "$token" > "$out/token-row.txt"

if [ -n "$token" ] && [ "$copy_count" -ge 2 ]; then
  sentinel2="SENTINEL-855-token-$(date +%s)"
  printf '%s' "$sentinel2" | pbcopy
  "$ax" press-button "$app_pid" Copy 2 || fail "could not press the second Copy button"
  sleep 0.6
  got2=$(pbpaste)
  printf '%s\n' "$got2" > "$out/paste-token.txt"
  still "02-after-token-copy"
  if [ "$got2" = "$token" ]; then
    pass "Hermes token Copy wrote the token to the pasteboard"
  else
    fail "Hermes token Copy did not write the token (got ${got2})"
  fi
else
  skip "Hermes token Copy: token row empty or hidden (default BYO harness is not Hermes)"
fi

echo
if [ "$failures" -eq 0 ]; then
  pass "clipboard sitting passed - dumps and stills under $out"
  printf '%s\n' "$out" > "$out/OUTDIR"
  exit 0
fi
fail "$failures check(s) failed - dumps and stills under $out"
printf '%s\n' "$out" > "$out/OUTDIR"
exit 1
