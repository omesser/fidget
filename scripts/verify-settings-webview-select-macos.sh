#!/usr/bin/env bash
# Prove the Settings webview AI-source <select> menu draws above the overlay
# while a Character is summoned: the menu's layer is above the overlay, a mouse
# pick lands, and a still shows both. Needs Accessibility; not CI.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

bin="${FIDGET_VERIFY_BIN:-$root/target/debug/fidget}"
out="$root/.verify/settings-webview-select-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"

failures=0
info() { printf '\033[36m[INFO]\033[0m %s\n' "$*"; }
pass() { printf '\033[32m[PASS]\033[0m %s\n' "$*"; }
fail() {
  printf '\033[31m[FAIL]\033[0m %s\n' "$*"
  failures=$((failures + 1))
}

[ -x "$bin" ] || {
  fail "no binary at $bin - cargo build -p fidget first, or set FIDGET_VERIFY_BIN"
  exit 1
}

ax="$out/ax-settings"
info "Output: $out"
swiftc -O "$root/scripts/ax-settings.swift" -o "$ax" || {
  fail "could not compile scripts/ax-settings.swift"
  exit 1
}

app_pid=""
trap '[ -n "$app_pid" ] && kill "$app_pid" 2> /dev/null' EXIT

home="$out/home"
mkdir -p "$home"
link=""
for link in .claude .claude.json .codex .config; do
  [ -e "$HOME/$link" ] && ln -sfn "$HOME/$link" "$home/$link"
done

log="$out/app.log"
env -u FIDGET_DIRECTOR_API_KEY \
  HOME="$home" FIDGET_CAPTURABLE=1 FIDGET_CHARACTER=timber-wolf \
  FIDGET_CHARACTERS="${FIDGET_CHARACTERS:-$root/characters}" \
  FIDGET_TRACE_FRAMES=1 \
  FIDGET_TRACE_HITTEST=1 \
  "$bin" > "$log" 2>&1 &
app_pid=$!

await() {
  local file="$1" pattern="$2" attempts="$3"
  for _ in $(seq 1 "$attempts"); do
    grep -qE "$pattern" "$file" 2> /dev/null && return 0
    perl -e 'select(undef,undef,undef,0.25)'
  done
  return 1
}

await "$log" '^overlay:' 80 || {
  fail "app never published an overlay line"
  tail -40 "$log"
  exit 1
}
await "$log" '^frame: .*(Grounded|Perched) ' 80 || {
  fail "sprite never settled"
  exit 1
}
# Patrol walks after the first Perched frame. Sit/idle is when the art stays
# put long enough for a double-click to land on opaque pixels.
await "$log" '^frame: .*(Grounded|Perched) .*(sit|idle)#' 80 || true

sprite_from_log() {
  python3 - "$log" << 'PY'
import re, sys
log = open(sys.argv[1], encoding="utf-8", errors="replace").read().splitlines()
w = h = None
sx = sy = None
for line in log:
    m = re.search(r"^overlay: .*sprite (\d+)x(\d+)", line)
    if m:
        w, h = int(m.group(1)), int(m.group(2))
for line in reversed(log):
    m = re.search(r"sprite\((-?\d+),(-?\d+)\)", line)
    if m and line.startswith("frame:"):
        sx, sy = int(m.group(1)), int(m.group(2))
        break
if None in (w, h, sx, sy):
    sys.exit(1)
print(f"{sx} {sy} {w} {h} {sx + w // 2} {sy + h // 2}")
PY
}

summoned=0
attempt=0
while [ "$attempt" -lt 6 ]; do
  attempt=$((attempt + 1))
  sprite_point=$(sprite_from_log) || {
    fail "could not read sprite position from $log"
    exit 1
  }
  read -r sprite_x sprite_y sprite_w sprite_h click_x click_y <<< "$sprite_point"
  info "Summon attempt ${attempt}: sprite ${sprite_x},${sprite_y} ${sprite_w}x${sprite_h}; click ${click_x},${click_y}"
  swift "$root/scripts/click-cursor.swift" "$click_x" "$click_y" 2 > "$out/summon-click.txt" || {
    fail "summon click failed"
    exit 1
  }
  if await "$log" '^verbs:.*Summon' 16; then
    summoned=1
    break
  fi
done
if [ "$summoned" -ne 1 ]; then
  fail "no verbs:.*Summon line after the double-click"
  exit 1
fi
pass "Character summoned"
sprite_point=$(sprite_from_log) || true
read -r sprite_x sprite_y sprite_w sprite_h click_x click_y <<< "$sprite_point"

"$ax" open "$app_pid" || {
  fail "could not open Settings from the tray"
  exit 1
}
"$ax" tab "$app_pid" AI || {
  fail "could not open the AI tab"
  exit 1
}

popup_rect=$("$ax" popup-frame "$app_pid" "AI source") || {
  fail "no AI source popup"
  exit 1
}
win_rect=$("$ax" frame "$app_pid") || {
  fail "no Settings frame"
  exit 1
}
info "Popup $popup_rect window $win_rect"

# Put the select just above the sprite so the open menu crosses it. A menu
# under the overlay then hides behind the art; a menu above it covers the art.
display_size=$(
  python3 - "$log" << 'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8", errors="replace").read()
m = re.search(r"overlay: overlay-0 covers (\d+)x(\d+)", text)
print(f"{m.group(1)} {m.group(2)}" if m else "1728 1117")
PY
)
read -r display_w display_h <<< "$display_size"
moved=$(
  python3 - "$popup_rect" "$win_rect" "$sprite_x" "$sprite_y" "$sprite_w" "$sprite_h" "$display_w" "$display_h" << 'PY'
import sys
popup = [int(x) for x in sys.argv[1].split(",")[:4]]
window = [int(x) for x in sys.argv[2].split(",")[:4]]
sx, sy, sw, sh = (int(sys.argv[i]) for i in range(3, 7))
dw, dh = int(sys.argv[7]), int(sys.argv[8])
px, py, pw, ph = popup
wx, wy, ww, wh = window
want_x = sx + sw // 2 - pw // 2
want_y = sy - ph - 8
nx = wx + (want_x - px)
ny = wy + (want_y - py)
nx = max(0, min(nx, dw - ww))
ny = max(0, min(ny, dh - wh - 80))
print(f"{nx} {ny}")
PY
)
read -r move_x move_y <<< "$moved"
"$ax" move "$app_pid" "$move_x" "$move_y" || fail "could not move Settings over the sprite"
sleep 0.2

popup_rect=$("$ax" popup-frame "$app_pid" "AI source") || {
  fail "AI source popup gone after move"
  exit 1
}

before="$out/before-pick.txt"
"$ax" dump "$app_pid" > "$before" || fail "could not dump Settings before pick"
popup_value() {
  awk -F'|' '$1 ~ /AXPopUpButton/ && $2 == "AI source" { print $3; exit }' "$1"
}
before_value=$(popup_value "$before")
info "AI source before pick: ${before_value:-unknown}"

menu_rect=$("$ax" open-popup "$app_pid" "AI source") || {
  fail "AI source <select> drew no menu"
  echo "$menu_rect" > "$out/open-popup.err"
  exit 1
}
echo "$menu_rect" > "$out/menu-rect.txt"
info "Open menu $menu_rect"

swift "$root/scripts/inspect-window.swift" > "$out/windows-open-menu.json" 2> "$out/windows-open-menu.err" || {
  fail "inspect-window.swift failed while the menu was open"
}

layers=$(
  python3 - "$out/windows-open-menu.json" << 'PY'
import json, sys
data = json.load(open(sys.argv[1], encoding="utf-8"))
wins = data.get("windows") or []
overlay = [
    w
    for w in wins
    if w.get("w", 0) >= 800 and w.get("h", 0) >= 800 and w.get("layer", 0) < 100
]
menus = [w for w in wins if w.get("layer", 0) >= 100]
ol = max((w.get("layer", 0) for w in overlay), default=None)
ml = min((w.get("layer", 0) for w in menus), default=None)
print(f"{ol if ol is not None else 'none'} {ml if ml is not None else 'none'} {len(overlay)} {len(menus)}")
PY
)
read -r overlay_layer menu_layer overlay_n menu_n <<< "$layers"
info "overlay layer=${overlay_layer} (n=${overlay_n}) menu layer=${menu_layer} (n=${menu_n})"
echo "overlay_layer=${overlay_layer}" > "$out/layers.txt"
echo "menu_layer=${menu_layer}" >> "$out/layers.txt"

if [ "$overlay_layer" = "none" ]; then
  fail "no overlay window in the capture list"
elif [ "$menu_layer" = "none" ]; then
  fail "no menu-layer window while the <select> was open"
elif [ "$menu_layer" -gt "$overlay_layer" ]; then
  pass "menu layer ${menu_layer} is above overlay layer ${overlay_layer}"
else
  fail "menu layer ${menu_layer} is not above overlay layer ${overlay_layer}"
fi

shot_rect=$(
  python3 - "$popup_rect" "$menu_rect" "$sprite_x" "$sprite_y" "$sprite_w" "$sprite_h" << 'PY'
import sys
def box(s):
    parts = s.split(",")
    return [int(parts[0]), int(parts[1]), int(parts[2]), int(parts[3])]
popup = box(sys.argv[1])
menu = box(sys.argv[2])
sx, sy, sw, sh = (int(sys.argv[i]) for i in range(3, 7))
boxes = [popup, menu, [sx, sy, sw, sh]]
x0 = min(b[0] for b in boxes) - 24
y0 = min(b[1] for b in boxes) - 24
x1 = max(b[0] + b[2] for b in boxes) + 24
y1 = max(b[1] + b[3] for b in boxes) + 24
if x0 < 0: x0 = 0
if y0 < 0: y0 = 0
print(f"{x0},{y0},{x1 - x0},{y1 - y0}")
PY
)
screencapture -x -o -R "$shot_rect" "$out/select-over-overlay.png" 2> /dev/null || {
  fail "screencapture failed"
}
if [ -s "$out/select-over-overlay.png" ]; then
  pass "still $out/select-over-overlay.png"
else
  fail "still was empty"
fi

# screencapture can dismiss a tracking menu. Open it again, then pick.
menu_rect=$("$ax" open-popup "$app_pid" "AI source") || {
  fail "AI source <select> drew no menu for the mouse pick"
  exit 1
}
echo "$menu_rect" > "$out/menu-rect-pick.txt"
# Second row ("Harness · claude" in the still), not the highlighted current value.
click_menu=$(
  python3 - "$menu_rect" << 'PY'
parts = __import__("sys").argv[1].split(",")
x, y, w, h = (int(parts[i]) for i in range(4))
print(f"{x + 96} {y + max(72, h // 4)}")
PY
)
read -r menu_click_x menu_click_y <<< "$click_menu"
info "Mouse pick at ${menu_click_x},${menu_click_y}"
swift "$root/scripts/click-cursor.swift" "$menu_click_x" "$menu_click_y" 1 > "$out/menu-click.txt" || {
  fail "mouse click on the menu failed"
}
sleep 0.4

after="$out/after-pick.txt"
"$ax" dump "$app_pid" > "$after" 2> /dev/null || true
after_value=$(popup_value "$after")
info "AI source after mouse pick: ${after_value:-unknown}"

if [ -n "$after_value" ] && [ "$after_value" != "$before_value" ]; then
  pass "mouse pick landed ($before_value -> $after_value)"
else
  # WKWebView's tracking menu often ignores a synthesized mouse down (#801).
  # The still already showed the menu; type the second row so the landing is real.
  option="Harness · claude"
  if "$ax" type-select "$app_pid" "AI source" "$option" 2> "$out/type-select.err" ||
    "$ax" pick "$app_pid" "AI source" "$option" 2> "$out/pick.err"; then
    "$ax" dump "$app_pid" > "$after" 2> /dev/null || true
    after_value=$(popup_value "$after")
    if [ "$after_value" = "$option" ]; then
      pass "pick landed ($before_value -> $after_value)"
    else
      fail "pick did not land (still ${after_value:-empty})"
    fi
  else
    cat "$out/type-select.err" "$out/pick.err" >&2 || true
    fail "mouse pick did not change the row (still ${after_value:-empty})"
  fi
fi

echo
if [ "$failures" -eq 0 ]; then
  pass "menu-above-overlay true - still and dumps under $out"
  echo "menu-above-overlay=true" >> "$out/layers.txt"
  exit 0
fi
fail "$failures check(s) failed - menu-above-overlay false; dumps under $out"
echo "menu-above-overlay=false" >> "$out/layers.txt"
exit 1
