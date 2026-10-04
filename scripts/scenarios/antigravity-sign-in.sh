#!/usr/bin/env bash
# Scenario: antigravity-sign-in (macOS only, since the sign-in flow is the server's own)
# On screen: launches Fidget as BMO with the antigravity preset Harness, signed
#   out. You open Chat and sign in with Google in your browser. Two screenshots
#   of the Chat window. Fidget quits, then two headless probes run.
# Input: yours, prompted in the terminal: double-click BMO, click "Log in with
#   Google", then sign in in the browser tab the server opens.
# Duration: about 3 min, 15 min at most.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: Chat offers Log in with Google and names no terminal command; after
#   the browser sign-in, the server opens a session and Chat's mind line names
#   it; `--probe-harness` then ends a turn on a fresh session and again on the
#   resumed one.
#
# Usage: antigravity-sign-in.sh --go <fidget binary> <dir holding agy_acp_server.par>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Your own ~/.gemini is never read: the server gets a fresh GEMINI_HOME that the
# run deletes on exit, sign-in tokens included.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: antigravity-sign-in.sh --go <fidget binary> <server dir>}
server_dir=$(cd "${3:?usage: antigravity-sign-in.sh --go <fidget binary> <server dir>}" && pwd)
[ -x "$server_dir/agy_acp_server.par" ] || {
  echo "no agy_acp_server.par in $server_dir" >&2
  exit 1
}
root=$(cd "$(dirname "$0")/../.." && pwd)
out="${TMPDIR:-/tmp}/fidget-scenario-antigravity-sign-in-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
log="$out/app.log"
session_file="$out/home/Library/Application Support/fidget/harness-session.json"
# Outside $out, which outlives the run: this directory ends up holding tokens.
gemini_home=$(mktemp -d "${TMPDIR:-/tmp}/fidget-gemini-home.XXXXXX")

pid=
family() { # <pid>: it and every descendant
  echo "$1"
  for child in $(pgrep -P "$1"); do family "$child"; done
}
cleanup() {
  if [ -n "$pid" ]; then
    # The Harness runs in its own process group, so killing Fidget alone
    # leaves agy_acp_server and localharness_external behind.
    family "$pid" | xargs kill 2> /dev/null || true
  fi
  rm -rf "$gemini_home"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

[ -x "$tools/window-id" ] || swiftc -O "$root/scripts/scenarios/window-id.swift" -o "$tools/window-id"
[ -x "$tools/ax" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"

run_env=(HOME="$out/home" GEMINI_HOME="$gemini_home" PATH="$server_dir:$PATH"
  FIDGET_HARNESS=antigravity FIDGET_DIRECTOR_API_KEY=x)
env "${run_env[@]}" FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!

wait_for() { # <seconds> <command...>
  local n=$(($1 * 4))
  shift
  until "$@" > /dev/null 2>&1; do
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    n=$((n - 1))
    [ "$n" -gt 0 ] || return 1
    sleep 0.25
  done
}

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

echo ">>> Double-click BMO to open Chat."
wait_for 180 "$tools/window-id" "$pid" BMO || fail "Chat did not open"
wait_for 120 shows sign-in '^AXButton\|Log in with Google\|' ||
  fail "Chat offered no Log in with Google; see $out/sign-in.ax.txt and $log"
shot sign-in
grep -qE '^AXButton\|Log in with Gemini Enterprise\|' "$out/sign-in.ax.txt" ||
  fail "Chat offered no Gemini Enterprise login; see $out/sign-in.ax.txt"
if grep -qE 'run this in a terminal' "$out/sign-in.ax.txt"; then
  fail "Chat names a terminal command Antigravity does not have; see $out/sign-in.ax.txt"
fi
echo "ok: Chat offers Log in with Google and Gemini Enterprise, and no terminal line"

echo '>>> Click "Log in with Google" in Chat, then sign in in the browser tab it opens.'
wait_for 600 grep -q '"harness":"antigravity"' "$session_file" || fail "no antigravity session was saved; see $log"
grep -qE '"session_id":"[^"]+"' "$session_file" || fail "the saved session has no id; see $session_file"
wait_for 30 shows after-sign-in 'antigravity · session [^|]+' ||
  fail "Chat's mind line names no session; see $out/after-sign-in.ax.txt"
shot after-sign-in
echo "ok: $(grep -oEm1 'antigravity · session [^|]+' "$out/after-sign-in.ax.txt")"
cp "$session_file" "$out/"

family "$pid" | xargs kill 2> /dev/null || true
pid=

# The probe keeps its own session file, so the second run resumes the first.
for run in fresh resumed; do
  env "${run_env[@]}" "$bin" --probe-harness > "$out/probe-$run.txt" 2>&1 ||
    fail "the $run probe did not end its turn; see $out/probe-$run.txt"
  echo "ok: $run probe: $(grep -E '^  (session|stop|mcp listed) ' "$out/probe-$run.txt" | tr -s ' ' | paste -sd ';' -)"
done
echo "PASS: evidence in $out"
