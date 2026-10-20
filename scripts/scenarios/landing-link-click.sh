#!/usr/bin/env bash
# Scenario: landing-link-click (macOS)
# On screen: launches Fidget as BMO with Codex picked and no `npx` on PATH, so
#   no Harness runs. The menu bar icon's menu opens and its Chat… row is
#   pressed, so Chat opens on the "Codex needs `npx`" landing and takes focus.
#   The install link is clicked. Two screenshots. Fidget quits at the end.
# Input: one real click on the menu bar icon, AXPress on Chat…, one real click
#   on the link; no keys. The link goes to a recording `open`, so no browser opens.
# Duration: a few seconds, 15 s at most.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: the landing draws `npx` as code and https://nodejs.org/ as its own
#   element, with no backtick left in the copy; nothing is handed to `open`
#   before the click; the click hands exactly https://nodejs.org/ to `open`;
#   Chat still shows the landing after it, so the webview did not navigate.
#
# Usage: landing-link-click.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: landing-link-click.sh --go <fidget binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-landing-link-click-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$out/bin" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log" opened="$out/opened.txt" url=https://nodejs.org/
: > "$opened"

fail() {
  printf 'FAIL: %s\nevidence in %s\n' "$*" "$out" >&2
  exit 1
}

[ -x "$tools/window-id" ] || swiftc -O "$root/scripts/scenarios/window-id.swift" -o "$tools/window-id"
[ -x "$tools/click-cursor" ] || swiftc -O "$root/scripts/click-cursor.swift" -o "$tools/click-cursor"
[ "$tools/ax" -nt "$root/scripts/ax-settings.swift" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"
build_ax_events

# platform::open_url spawns `open` by name, so this one on PATH takes the hand-off.
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> %q\n' "$opened" > "$out/bin/open"
chmod +x "$out/bin/open"
path="$out/bin:/usr/bin:/bin:/usr/sbin:/sbin"
! PATH=$path command -v npx > /dev/null || fail "npx is on $path, so Codex would launch"

start_ms=$(now_ms)
env HOME="$out/home" PATH="$path" \
  FIDGET_HARNESS=codex \
  FIDGET_DIRECTOR_WAKE_SECS=600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'stop_fidget' EXIT INT TERM

dump() { "$tools/ax" dump "$pid" BMO frames > "$out/$1.ax.txt" 2>&1; }
landing() { dump "$1" && grep -qF '|Codex needs ' "$out/$1.ax.txt"; }
shot() { screencapture -x -o -l "$("$tools/window-id" "$pid" BMO)" "$out/$1.png"; }

# Fidget draws the tray icon before its click handler and its place in the menu
# bar have settled, so an open that lands in that window is lost. The Chat
# window appearing is what each retry waits on.
wait_for_up 3 "$log" || fail "Fidget never wrote its overlay line; see $log"
chat_opened=0
for attempt in 1 2; do
  "$tools/ax" open "$pid" Chat BMO > "$out/open.txt" 2>&1 || true
  if wait_for_ax 2 "$pid" "$tools/window-id" "$pid" BMO; then
    chat_opened=1
    break
  fi
  echo "open: attempt $attempt left Chat closed" >&2
done
[ "$chat_opened" = 1 ] || fail "the menu's Chat row did not open Chat; see $out/open.txt"
wait_for_ax 3 "$pid" landing landing || fail "Chat shows no Codex needs npx landing; see $out/landing.ax.txt"
shot landing
f=$out/landing.ax.txt
! grep -qF '`' "$f" || fail "a backtick is left in the landing copy; see $f"
# The copy marks npx in backticks three times, once in the title and twice in
# the sentence. WebKit folds an inline <code> into the sentence and draws the
# span as an AXCodeStyleGroup, so three spans are what say all three were code.
spans=$(grep -c 'AXCodeStyleGroup' "$f" || true)
[ "$spans" = 3 ] || fail "the landing drew $spans code spans, not the 3 for npx; see $f"
awk -F'|' '$3 ~ /npx is not installed/' "$f" | grep -q . || fail "npx is not in the landing copy; see $f"
row=$(awk -F'|' -v u="$url" '$3 == u' "$f" | head -1)
[ -n "$row" ] || fail "$url is not drawn as its own element; see $f"
read -r x y w h <<< "$(tr , ' ' <<< "${row##*|}")"
echo "ok: the landing draws npx as code and $url apart from the copy, no backtick left"

[ ! -s "$opened" ] || fail "something reached open before the click: $(cat "$opened")"
"$tools/click-cursor" "$((x + w / 2))" "$((y + h / 2))" 1 > "$out/click.txt" 2>&1 || fail "could not click the link; see $out/click.txt"
wait_for_line 3 "$opened" '^https://nodejs\.org/$' || fail "the click handed '$(cat "$opened")' to open, want $url"
echo "ok: the click handed $url to open"

# A webview navigation would replace the landing as the link hands off.
landing after || fail "Chat left the landing after the click, so the webview navigated; see $out/after.ax.txt"
shot after
echo "ok: Chat still shows the landing"
echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
