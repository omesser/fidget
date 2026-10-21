#!/usr/bin/env bash
# Scenario: codex-sign-in-link (macOS only, since the sign-in flow is codex-acp's own)
# On screen: launches Fidget as BMO with the codex preset Harness, signed out.
#   You open Chat, sign in through the link Chat shows, and finish in your
#   browser. Two screenshots of the Chat window. Fidget quits when it ends.
# Input: yours, prompted in the terminal: double-click BMO, click "ChatGPT
#   (device code)", click Open, then enter the code in the browser.
# Duration: about 3 min, 20 min at most.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: Chat offers the device-code sign-in; the ask row shows an https
#   link with Open and Decline; after Open, codex opens a session and Chat's
#   mind line names it.
#
# Usage: codex-sign-in-link.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Your own ~/.codex is never read: codex gets a fresh CODEX_HOME that the run
# deletes on exit, sign-in tokens included.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: codex-sign-in-link.sh --go <fidget binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-codex-sign-in-link-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools/npm-cache"
seed_tour_shown "$out/home"
log="$out/app.log"
session_file="$out/home/Library/Application Support/fidget/harness-session.json"
# Outside $out, which outlives the run: this directory ends up holding tokens.
codex_home=$(mktemp -d "${TMPDIR:-/tmp}/fidget-codex-home.XXXXXX")
# File storage, so a sign-in never writes to the login keychain.
echo 'cli_auth_credentials_store = "file"' > "$codex_home/config.toml"

pid=
family() { # <pid>: it and every descendant
  echo "$1"
  for child in $(pgrep -P "$1"); do family "$child"; done
}
cleanup() {
  if [ -n "$pid" ]; then
    # The Harness runs in its own process group, so killing Fidget alone
    # leaves npx, codex-acp and codex app-server behind.
    family "$pid" | xargs kill 2> /dev/null || true
  fi
  rm -rf "$codex_home"
}
trap cleanup EXIT
# A signal has to end the run; cleanup is the EXIT trap's job.
trap 'exit 130' INT TERM

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

[ -x "$tools/window-id" ] || swiftc -O "$root/scripts/scenarios/window-id.swift" -o "$tools/window-id"
[ -x "$tools/ax" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"
build_ax_events

# NO_BROWSER drops codex-acp's browser sign-in, so the device code is the one
# button. The npm cache is shared across runs so npx fetches codex-acp once.
start_ms=$(now_ms)
env HOME="$out/home" \
  CODEX_HOME="$codex_home" NO_BROWSER=1 \
  npm_config_cache="$tools/npm-cache" \
  FIDGET_HARNESS=codex \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!

dump() { # <name>
  "$tools/ax" dump "$pid" BMO > "$out/$1.ax.txt" 2>&1
}
shows() { # <name> <extended regex>: the Chat window's AX dump has a match
  dump "$1" && grep -qE "$2" "$out/$1.ax.txt"
}
shot() { # <name>
  local id
  id=$("$tools/window-id" "$pid" BMO) || fail "$1: no Chat window"
  screencapture -x -o -l "$id" "$out/$1.png"
}

# Every other wait gives up within 3 s. These three wait on a person at the
# keyboard or in the browser, and on npx fetching codex-acp on a cold cache,
# so they keep human-sized limits.
human=120 cold_npx=180 browser_sign_in=600

echo ">>> Double-click BMO to open Chat."
wait_for_ax "$human" "$pid" "$tools/window-id" "$pid" BMO || fail "Chat did not open"
wait_for_ax "$cold_npx" "$pid" shows sign-in '^AXButton\|ChatGPT \(device code\)\|' ||
  fail "Chat offered no device-code sign-in; see $out/sign-in.ax.txt and $log"
echo "ok: Chat offers ChatGPT (device code)"

echo '>>> Click "ChatGPT (device code)" in Chat.'
wait_for_ax "$human" "$pid" shows ask '^AXButton\|Open\|' || fail "no Open button; see $out/ask.ax.txt"
shot ask
grep -qE '^AXButton\|Decline\|' "$out/ask.ax.txt" || fail "the ask row has no Decline; see $out/ask.ax.txt"
link=$(grep -oEm1 'https://[^|[:space:]]+' "$out/ask.ax.txt") || fail "the ask row shows no https link; see $out/ask.ax.txt"
echo "ok: the ask row shows $link with Open and Decline"

echo ">>> Click Open, then sign in and enter the code in the browser."
wait_until "$browser_sign_in" grep -q '"harness":"codex"' "$session_file" || fail "no codex session was saved; see $log"
grep -qE '"session_id":"[^"]+"' "$session_file" || fail "the saved session has no id; see $session_file"
wait_for_ax 3 "$pid" shows after-sign-in 'codex · session [^|]+' ||
  fail "Chat's mind line names no session; see $out/after-sign-in.ax.txt"
shot after-sign-in
echo "ok: $(grep -oEm1 'codex · session [^|]+' "$out/after-sign-in.ax.txt")"
cp "$session_file" "$out/"
echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
