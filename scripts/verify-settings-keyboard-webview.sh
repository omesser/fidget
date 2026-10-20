#!/usr/bin/env bash
# Keyboard-only Settings sitting: after the tray open, Tab/Space/Enter/Escape only.
# Usage: [FIDGET_VERIFY_BIN=path/to/fidget] ./scripts/verify-settings-keyboard-webview.sh
# Needs Accessibility. Stills land in .verify/. Shares the clipboard sitting's lock.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

bin="${FIDGET_VERIFY_BIN:-$root/target/debug/fidget}"
out="$root/.verify/settings-keyboard-webview-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"

failures=0
lock_held=0
app_pid=""

TABS="Presence Character AI Chat Privacy Development"

info() { printf '\033[36m[INFO]\033[0m %s\n' "$*"; }
pass() { printf '\033[32m[PASS]\033[0m %s\n' "$*"; }
fail() {
  printf '\033[31m[FAIL]\033[0m %s\n' "$*"
  failures=$((failures + 1))
}

# role|title of a focused or dump line. Focus carries a ▸ on a disclosure the
# dump does not, and the value changes with state, so neither is identity.
# Sequences compare by position, so duplicate and empty titles survive.
identity() {
  awk -F'|' '{ t = $2; sub(/^▸ /, "", t); print $1 "|" t }'
}

# The tab stops in dump order, by the focusable column: WebKit gives a
# <pre tabindex="0"> the same AXGroup role as its layout wrappers. The scan
# starts at the AXWebArea because the Tab cycle stays inside the page.
expected_sequence() {
  awk -F'|' '$1 == "AXWebArea" { web = 1 } web && $7 == "true" { print }' "$1" | identity
}

# One full Tab cycle: the first line repeats when focus wraps, and that repeat
# is dropped. A cycle that never wrapped keeps every line, so the comparison
# below sees the truncation.
observed_sequence() {
  awk 'NR == 1 { first = $0 } NR > 1 && $0 == first { exit } { print }' "$1" | identity
}

# The cycle starts wherever Space left focus, so the expected list is rotated
# to that control before the ordered comparison.
check_tab_sequence() {
  local tab="$1" dump="$2" cycle="$3" out="$4"
  local expect="$out/expect-$tab.txt" observed="$out/observed-$tab.txt"
  expected_sequence "$dump" > "$expect"
  observed_sequence "$cycle" > "$observed"
  local start n
  start=$(head -n 1 "$observed")
  n=$(grep -nxF -- "$start" "$expect" | head -n 1 | cut -d: -f1)
  if [ -z "$n" ]; then
    fail "$tab: focus started on '$start', which is not a tabbable control in the dump"
    return 1
  fi
  {
    tail -n +"$n" "$expect"
    head -n "$((n - 1))" "$expect"
  } > "$out/expect-$tab.rotated.txt"
  if diff "$out/expect-$tab.rotated.txt" "$observed" > "$out/sequence-diff-$tab.txt"; then
    pass "$tab: Tab visited all $(wc -l < "$observed" | tr -d ' ') dump controls in dump order"
    return 0
  fi
  fail "$tab: focus sequence differs from dump order (< dump, > observed)"
  cat "$out/sequence-diff-$tab.txt"
  return 1
}

check_tab_still() {
  local tab="$1" png="$2" line="$3"
  if [ -s "$png" ] && [ -n "$line" ] && [ "$line" != "none|||false" ]; then
    pass "$tab: still taken with focus on $(printf '%s' "$line" | cut -d'|' -f1-2)"
    return 0
  fi
  fail "$tab: no still with a focused control (png $([ -s "$png" ] && echo present || echo missing), focused '${line:-}')"
  return 1
}

# Space, Down, Return must leave the same select focused with another value.
check_select_commit() {
  local before="$1" after="$2"
  local before_role after_role before_val after_val
  before_role=$(printf '%s' "$before" | cut -d'|' -f1)
  after_role=$(printf '%s' "$after" | cut -d'|' -f1)
  before_val=$(printf '%s' "$before" | cut -d'|' -f3)
  after_val=$(printf '%s' "$after" | cut -d'|' -f3)
  if [ "$before_role" != "$after_role" ] || [ -z "$after_val" ]; then
    fail "<select> lost focus after Return (before '$before', after '$after')"
    return 1
  fi
  if [ "$before_val" = "$after_val" ]; then
    fail "<select> value did not commit: still '$after_val' after Down and Return"
    return 1
  fi
  pass "<select> committed '$before_val' -> '$after_val'"
  return 0
}

# Inline: shellcheck cannot see that trap calls a function.
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

# An exported key outranks the Keychain store, so no modal prompt takes key
# focus mid-sitting. A placeholder spends no tokens when Enter sets the wake.
log="$out/app.log"
env FIDGET_DIRECTOR_API_KEY=verify-settings-keyboard \
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
"$ax" focus-window "$app_pid" || fail "could not focus the Settings title bar"

still() {
  local name="$1"
  local rect
  rect=$("$ax" frame "$app_pid" 2> /dev/null) || {
    info "no Settings frame for $name"
    return 1
  }
  [ -n "$rect" ] || return 1
  printf '%s\n' "$rect" > "$out/last-rect"
  screencapture -x -o -R "$rect" "$out/${name}.png" 2> /dev/null
  [ -s "$out/${name}.png" ]
}

downscale() {
  local src="$1"
  local dst="$2"
  [ -f "$src" ] || return 0
  sips -Z 420 "$src" --out "$dst" > /dev/null
}

key_owner() {
  osascript -e 'tell application "System Events" to get name of first application process whose frontmost is true' 2> /dev/null
}

# A failed focus read logs who held key focus, so a stolen-focus run reads as
# one at the end instead of as eleven unrelated failures.
focused() {
  "$ax" focused "$app_pid" 2> /dev/null || {
    printf '%s\n' "$(key_owner)" >> "$out/focus-lost.log"
    echo "none|||false"
  }
}

key() { "$ax" key "$app_pid" "$1"; }

# Tab until the focused title equals $1. WebKit prefixes a disclosure with ▸.
tab_until() {
  local want="$1"
  local i=0
  local line="" title=""
  while [ "$i" -lt 48 ]; do
    line=$(focused)
    printf '%s\n' "$line" >> "$out/focus.log"
    title=$(printf '%s\n' "$line" | cut -d'|' -f2)
    title="${title#▸ }"
    [ "$title" = "$want" ] && return 0
    key tab
    i=$((i + 1))
  done
  return 1
}

is_tab_title() {
  local t
  for t in $TABS; do
    [ "$1" = "$t" ] && return 0
  done
  return 1
}

info "Tab order on every Settings tab"
for tab in $TABS; do
  if tab_until "$tab"; then
    key space
    sleep 0.4
    "$ax" dump "$app_pid" > "$out/dump-${tab}.txt" || fail "dump $tab"
    count=0
    shot=0
    first=$(focused)
    printf '%s\n' "$first" > "$out/cycle-${tab}.txt"
    key tab
    j=0
    while [ "$j" -lt 40 ]; do
      line=$(focused)
      printf '%s\n' "$line" >> "$out/cycle-${tab}.txt"
      count=$((count + 1))
      [ "$line" = "$first" ] && [ "$count" -gt 1 ] && break
      # The still shows a ring on the first panel control, past the tab bar.
      title=$(printf '%s\n' "$line" | cut -d'|' -f2)
      if [ "$shot" -eq 0 ] && ! is_tab_title "${title#▸ }"; then
        still "tab-${tab}" && shot=1
        printf '%s\n' "$line" > "$out/still-${tab}-focus.txt"
      fi
      key tab
      j=$((j + 1))
    done
    info "$tab cycle length $count"
  else
    fail "Tab never reached the $tab tab"
  fi
done

tab_order=PASS
focus_ring=PASS
for tab in $TABS; do
  if [ ! -s "$out/cycle-${tab}.txt" ] || [ ! -s "$out/dump-${tab}.txt" ]; then
    fail "$tab: no cycle or dump recorded"
    tab_order=FAIL
  else
    check_tab_sequence "$tab" "$out/dump-${tab}.txt" "$out/cycle-${tab}.txt" "$out" || tab_order=FAIL
  fi
  if check_tab_still "$tab" "$out/tab-${tab}.png" "$(cat "$out/still-${tab}-focus.txt" 2> /dev/null)"; then
    downscale "$out/tab-${tab}.png" "$out/01-tab-${tab}.png"
  else
    focus_ring=FAIL
  fi
done
if [ "$tab_order" = PASS ]; then
  pass "Tab reaches every control in DOM order on every Settings tab"
else
  fail "Tab order sitting failed"
fi
if [ "$focus_ring" = PASS ]; then
  pass "Focus ring still taken on every Settings tab"
else
  fail "Focus ring stills incomplete"
fi

# <select> Space, Down, Return. Character popup is the first <select>.
select_ok=FAIL
if tab_until "Character"; then
  key space
  sleep 0.3
  s=0
  while [ "$s" -lt 24 ]; do
    line=$(focused)
    echo "$line" | cut -d'|' -f1 | grep -Eq 'AXPopUpButton|AXComboBox' && break
    key tab
    s=$((s + 1))
  done
  select_before=$(focused)
  printf '%s\n' "$select_before" > "$out/select-before.txt"
  menus_before=$("$ax" menus "$app_pid")
  still "select-closed"
  key space
  sleep 0.35
  menus_after=$("$ax" menus "$app_pid")
  still "select-open"
  downscale "$out/select-open.png" "$out/05-select-space.png"
  info "select menus before=$menus_before after=$menus_after focused=$select_before"
  key down
  sleep 0.2
  still "select-arrow"
  key return
  sleep 0.4
  select_after=$(focused)
  printf '%s\n' "$select_after" > "$out/select-after.txt"
  select_ok=PASS
  if [ "${menus_after:-0}" -gt "${menus_before:-0}" ]; then
    pass "<select> opened on Space (menu windows $menus_before -> $menus_after)"
  else
    fail "<select> Space did not add a menu-layer window ($menus_before -> $menus_after)"
    select_ok=FAIL
  fi
  check_select_commit "$select_before" "$select_after" || select_ok=FAIL
else
  fail "could not reach Character for the <select> sitting"
fi

# <details> Enter. Presence "What is this?" is the first disclosure.
details_ok=FAIL
if tab_until "Presence"; then
  key space
  sleep 0.3
  if tab_until "What is this?"; then
    before=$(focused)
    still "details-closed"
    printf '%s\n' "$before" > "$out/details-before-focus.txt"
    key return
    sleep 0.3
    after=$(focused)
    still "details-open"
    downscale "$out/details-open.png" "$out/06-details-enter.png"
    printf '%s\n' "$after" > "$out/details-after-focus.txt"
    before_val=$(printf '%s\n' "$before" | cut -d'|' -f3)
    after_val=$(printf '%s\n' "$after" | cut -d'|' -f3)
    if [ "$before_val" != "$after_val" ]; then
      pass "<details> toggled on Enter ($before_val -> $after_val)"
      details_ok=PASS
    else
      fail "<details> AX value stayed $after_val after Enter"
    fi
  else
    fail "Tab never reached a What is this? disclosure"
  fi
else
  fail "could not reach Presence for the <details> sitting"
fi

# Enter stages, Apply saves: AI tab, First wake field. Replace 120 with 90, Return, then Apply.
# A junk string looks like it typed (the field shows it) then reverts on blur
# because the row only stores a number.
enter_ok=FAIL
if tab_until "AI"; then
  key space
  sleep 0.4
  if tab_until "First wake, in seconds"; then
    # Caret lands at the start; End then backspace clears 120 without Cmd-A,
    # which WKWebView did not take as select-all.
    key end
    key delete
    key delete
    key delete
    "$ax" type "$app_pid" "90"
    sleep 0.2
    still "enter-before"
    downscale "$out/enter-before.png" "$out/04-enter-applies.png"
    key return
    sleep 0.5
    still "enter-after"
    "$ax" dump "$app_pid" > "$out/enter-dump.txt"
    settings_json="$home/Library/Application Support/fidget/settings.json"
    # AX keeps the placeholder 120 on this row. The file is what Apply wrote.
    # AI-tab edits are staged until Apply, so Enter alone must not write it.
    if grep -Fq '"director_wake_secs": "90"' "$settings_json" 2> /dev/null; then
      fail "Enter wrote director_wake_secs 90 before Apply"
    elif tab_until "Apply"; then
      key space
      sleep 0.5
      if grep -Fq '"director_wake_secs": "90"' "$settings_json"; then
        pass "Enter staged First wake = 90 and Apply wrote it to settings.json"
        enter_ok=PASS
      else
        fail "Apply did not write director_wake_secs 90"
        grep director_wake_secs "$settings_json" || true
      fi
    else
      fail "Tab never reached Apply"
    fi
  else
    fail "Tab never reached First wake, in seconds"
  fi
else
  fail "could not reach AI for the Enter-applies sitting"
fi

# Escape last: it closes the window.
still "escape-before"
downscale "$out/escape-before.png" "$out/03-escape-before.png"
key escape
sleep 0.5
escape_ok=FAIL
if "$ax" frame "$app_pid" > /dev/null 2>&1; then
  fail "Escape left the Settings window open"
  still "escape-still-open"
else
  pass "Escape closed the Settings window"
  escape_ok=PASS
  if [ -f "$out/last-rect" ]; then
    screencapture -x -o -R "$(cat "$out/last-rect")" "$out/escape-after.png" 2> /dev/null || true
    downscale "$out/escape-after.png" "$out/03-escape.png"
  elif [ -f "$out/03-escape-before.png" ]; then
    cp "$out/03-escape-before.png" "$out/03-escape.png"
  fi
fi

five_stills=""
for tab in $TABS; do
  five_stills="$five_stills ![tab-${tab}](./01-tab-${tab}.png)"
done
table="$out/TABLE.md"
{
  echo '| Behaviour | Result | Still |'
  echo '| --- | --- | --- |'
  echo "| Tab reaches every control in DOM order on every Settings tab | $tab_order |${five_stills} |"
  echo "| Focus ring visible on each | $focus_ring |${five_stills} |"
  echo "| Escape closes | $escape_ok | ![escape](./03-escape.png) |"
  echo "| Enter stages, Apply saves | $enter_ok | ![enter-applies](./04-enter-applies.png) |"
  echo "| \`<select>\` opens on Space, picks with arrows, commits | $select_ok | ![select-space](./05-select-space.png) |"
  echo "| \`<details>\` toggles on Enter | $details_ok | ![details-enter](./06-details-enter.png) |"
} > "$table"
cat "$table"

echo
echo "$out" > "$out/../settings-keyboard-webview-latest"
if [ "$failures" -eq 0 ]; then
  pass "all checks passed - stills under $out"
  exit 0
fi
fail "$failures check(s) failed - dumps and stills under $out"
if [ -s "$out/focus-lost.log" ]; then
  lost=$(wc -l < "$out/focus-lost.log" | tr -d ' ')
  owners=$(sort "$out/focus-lost.log" | uniq -c | sort -rn | awk '{ $1 = $1 ":"; print }' | tr '\n' ' ')
  info "$lost focus reads failed while key focus sat on: $owners"
  info "another window took key focus during the sitting; rerun with nothing else raising windows before trusting these failures"
else
  info "every focus read answered, so these failures are not a stolen-focus artefact"
fi
exit 1
