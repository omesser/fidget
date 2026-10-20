#!/usr/bin/env bash
# Prove that native Wayland fullscreen apps don't appear in XWayland's window list.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*"; }

fail() {
  log_error "$@"
  exit 1
}

for tool in mutter xprop xterm gtk4-demo; do
  command -v "$tool" > /dev/null || fail "$tool not found (install mutter, x11-utils, xterm, gtk-4-examples)"
done

cd "$WORKSPACE_ROOT"

STAMP=$(date +%Y%m%d-%H%M%S)
OUT=".verify/fullscreen-wayland-native-xwayland-$STAMP"
mkdir -p "$OUT"
MUTTER_LOG="$OUT/mutter.log"
NATIVE_LOG="$OUT/native.log"
XTERM_LOG="$OUT/xterm.log"

MUTTER_PID=""
NATIVE_PID=""
XTERM_PID=""

# shellcheck disable=SC2317,SC2329 # invoked via trap
cleanup() {
  log_info "Cleaning up..."
  for pid in "$XTERM_PID" "$NATIVE_PID" "$MUTTER_PID"; do
    if [ -n "$pid" ]; then
      kill "$pid" 2> /dev/null || true
    fi
  done
}

trap cleanup EXIT

XDG_RUNTIME_DIR="/tmp/mutter-test"
export XDG_RUNTIME_DIR
mkdir -p "$XDG_RUNTIME_DIR"
rm -f "$XDG_RUNTIME_DIR"/.mutter-Xwaylandauth.* 2> /dev/null || true
WAYLAND_SOCKET="mutter-test-${STAMP}"
export WAYLAND_DISPLAY="$WAYLAND_SOCKET"

log_info "Starting mutter headless with XWayland..."
mutter --headless --wayland --virtual-monitor 1920x1080 --wayland-display="$WAYLAND_SOCKET" > "$MUTTER_LOG" 2>&1 &
MUTTER_PID=$!

log_info "Waiting for mutter XWayland display line..."
deadline=$(($(date +%s) + 3))
while [ "$(date +%s)" -lt "$deadline" ]; do
  if ! ps -p "$MUTTER_PID" > /dev/null 2>&1; then
    cat "$MUTTER_LOG"
    fail "Mutter failed to start"
  fi

  if grep -qE "Using public X11 display :[0-9]+" "$MUTTER_LOG" 2> /dev/null; then
    DISPLAY=$(grep -oE "Using public X11 display :[0-9]+" "$MUTTER_LOG" | head -1 | sed 's/.*:\([0-9]\+\)/:\1/')
    export DISPLAY
    log_info "Found XWayland at $DISPLAY"
    break
  fi
  sleep 0.2
done

[ -n "${DISPLAY:-}" ] || fail "XWayland display not found in mutter log"

XAUTHORITY_FILE=$(find "$XDG_RUNTIME_DIR" -name ".mutter-Xwaylandauth.*" 2> /dev/null | head -1 || true)
if [ -n "$XAUTHORITY_FILE" ]; then
  export XAUTHORITY="$XAUTHORITY_FILE"
  log_info "Using XAUTHORITY=$XAUTHORITY"
fi

log_info "Waiting to connect to $DISPLAY..."
deadline=$(($(date +%s) + 3))
while [ "$(date +%s)" -lt "$deadline" ]; do
  if xprop -root > /dev/null 2>&1; then
    log_info "Connected to $DISPLAY"
    break
  fi
  sleep 0.2
done

if ! xprop -root > /dev/null 2>&1; then
  fail "Cannot connect to $DISPLAY"
fi

count_windows() {
  { xprop -root _NET_CLIENT_LIST 2> /dev/null | grep -oE '0x[0-9a-fA-F]+' || true; } | wc -l
}

dump_windows() {
  local label="$1"
  printf "\n${GREEN}=== %s ===${NC}\n" "$label"
  local list
  list=$(xprop -root _NET_CLIENT_LIST 2> /dev/null | grep -oE '0x[0-9a-fA-F]+' || true)

  if [ -z "$list" ]; then
    echo "  (no windows)"
    return
  fi

  echo "$list" | while IFS= read -r wid; do
    local class name
    class=$(xprop -id "$wid" WM_CLASS 2> /dev/null | sed -n 's/.*= "\([^"]*\)".*/\1/p' || echo "?")
    name=$(xprop -id "$wid" _NET_WM_NAME WM_NAME 2> /dev/null | sed -n 's/.*= "\([^"]*\)".*/\1/p' | head -1 || echo "?")
    printf "  %s  WM_CLASS=%-15s WM_NAME=\"%s\"\n" "$wid" "$class" "$name"
  done

  local count
  count=$(echo "$list" | wc -l)
  printf "  ${YELLOW}Total: %d${NC}\n" "$count"
}

BASELINE=$(count_windows)
dump_windows "Baseline (mutter)"

log_info "Starting native Wayland client (gtk4-demo, GDK_BACKEND=wayland)..."
WAYLAND_DISPLAY="$WAYLAND_SOCKET" WAYLAND_DEBUG=1 GDK_BACKEND=wayland GSK_RENDERER=cairo gtk4-demo > "$NATIVE_LOG" 2>&1 &
NATIVE_PID=$!

log_info "Waiting for native client wl_surface.attach + commit..."
deadline=$(($(date +%s) + 3))
commit_found=false
while [ "$(date +%s)" -lt "$deadline" ]; do
  if ! ps -p "$NATIVE_PID" > /dev/null 2>&1; then
    log_error "Native client exited early. Log tail:"
    tail -20 "$NATIVE_LOG"
    fail "Native client failed"
  fi

  if grep -qE " -> wl_surface[@#][0-9]+\\.attach\\(wl_buffer[@#]" "$NATIVE_LOG" 2> /dev/null; then
    surface_with_attach=$(grep -E " -> wl_surface[@#][0-9]+\\.attach" "$NATIVE_LOG" | grep -oE "wl_surface[@#][0-9]+" | head -1)
    if [ -n "$surface_with_attach" ]; then
      attach_line=$(grep -n " -> $surface_with_attach\\.attach" "$NATIVE_LOG" 2> /dev/null | head -1 | cut -d: -f1)
      if [ -n "$attach_line" ]; then
        commit_after=$(grep -n " -> $surface_with_attach\\.commit" "$NATIVE_LOG" 2> /dev/null | awk -F: -v attach="$attach_line" '$1 > attach {print $1; exit}')
        if [ -n "$commit_after" ]; then
          log_info "Native client attached buffer and committed"
          commit_found=true
          break
        fi
      fi
    fi
  fi
  sleep 0.1
done

if [ "$commit_found" = false ]; then
  log_error "Native client never attached+committed. Log tail:"
  tail -20 "$NATIVE_LOG"
  fail "Native client deadline expired"
fi

AFTER_NATIVE=$(count_windows)
dump_windows "With native Wayland client (gtk4-demo)"

if [ "$AFTER_NATIVE" -gt "$BASELINE" ]; then
  log_error "UNEXPECTED: Window count increased from $BASELINE to $AFTER_NATIVE"
  log_error "Native Wayland app appeared in _NET_CLIENT_LIST (should not happen)"
  exit 1
fi

log_info "Native client did NOT appear in _NET_CLIENT_LIST (expected)"

log_info "Starting XWayland client (xterm)..."
xterm -e cat > "$XTERM_LOG" 2>&1 &
XTERM_PID=$!

log_info "Waiting for xterm in _NET_CLIENT_LIST..."
deadline=$(($(date +%s) + 3))
xterm_found=false
while [ "$(date +%s)" -lt "$deadline" ]; do
  list=$(xprop -root _NET_CLIENT_LIST 2> /dev/null | grep -oE '0x[0-9a-fA-F]+' || true)
  if [ -n "$list" ]; then
    for wid in $list; do
      class=$(xprop -id "$wid" WM_CLASS 2> /dev/null | sed -n 's/.*= "\([^"]*\)".*/\1/p' || echo "")
      if [ "$class" = "xterm" ]; then
        log_info "xterm detected at window $wid"
        xterm_found=true
        break 2
      fi
    done
  fi
  sleep 0.3
done

if [ "$xterm_found" = false ]; then
  fail "xterm did not appear in _NET_CLIENT_LIST"
fi

AFTER_XTERM=$(count_windows)
dump_windows "With XWayland client (xterm)"

if [ "$AFTER_XTERM" -le "$AFTER_NATIVE" ]; then
  log_error "FAIL: xterm did not increase window count"
  exit 1
fi

log_info "xterm appeared in _NET_CLIENT_LIST (expected)"

echo ""
log_info "${GREEN}CONFIRMED:${NC} Native Wayland apps do NOT appear in XWayland's _NET_CLIENT_LIST"
log_info "XWayland clients DO appear in _NET_CLIENT_LIST"
log_info "Baseline: $BASELINE, +native: $AFTER_NATIVE, +xterm: $AFTER_XTERM"
