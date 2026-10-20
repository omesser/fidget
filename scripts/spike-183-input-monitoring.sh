#!/usr/bin/env bash
# Does a mouse-only listen-only CGEventTap prompt for Input Monitoring?
# Usage: scripts/spike-183-input-monitoring.sh <bundle id> [out dir].
# Needs swiftc, passwordless sudo, and Accessibility for the terminal.

set -euo pipefail
cd "$(dirname "$0")/.."

BUNDLE="${1:-}"
# Not under /tmp: LaunchServices will not register a bundle there, and
# tccutil resolves the id through LaunchServices (-10814 otherwise).
OUT="${2:-$HOME/Library/Caches/fidget-spike-183}"
APP="$OUT/Spike183.app"

# A bare `tccutil reset ListenEvent` wipes every app's grant, and
# dev.omesser.fidget is the shipped app, so only a throwaway id is accepted.
case "$BUNDLE" in
  *tccspike*) ;;
  *)
    echo "spike-183: refusing bundle id '$BUNDLE'; it must contain 'tccspike'" >&2
    exit 2
    ;;
esac
[[ "$(uname -s)" == Darwin ]] || {
  echo "spike-183: macOS only" >&2
  exit 2
}

mkdir -p "$OUT" "$APP/Contents/MacOS"
swiftc -O -o "$APP/Contents/MacOS/spike-183-tap" scripts/spike-183-tap.swift
cat > "$APP/Contents/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>$BUNDLE</string>
  <key>CFBundleName</key><string>Spike183</string>
  <key>CFBundleExecutable</key><string>spike-183-tap</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSUIElement</key><true/>
</dict></plist>
EOF
scripts/dev-sign.sh "$APP" > /dev/null
codesign -dv "$APP" 2>&1 | grep "^Identifier=$BUNDLE$" > /dev/null || {
  echo "spike-183: signed identifier is not $BUNDLE" >&2
  exit 1
}

# The alert lives in UserNotificationCenter. Without a window there, this
# prints nothing; with one, its texts and buttons, and Deny is pressed.
dump_alert() {
  osascript - "$1" << 'EOF' 2> /dev/null || true
on run argv
  set report to ""
  tell application "System Events"
    if not (exists process "UserNotificationCenter") then return ""
    tell process "UserNotificationCenter"
      repeat with w in windows
        set report to report & "texts=" & (value of every static text of w as text) & linefeed
        set report to report & "buttons=" & (name of every button of w as text) & linefeed
        if (item 1 of argv) is "deny" then
          repeat with b in buttons of w
            if name of b is in {"Deny", "Don't Allow"} then
              click b
              set report to report & "pressed=" & name of b & linefeed
            end if
          end repeat
        end if
      end repeat
    end tell
  end tell
  return report
end run
EOF
}

sw_vers | tee "$OUT/macos-version.txt"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP"
tccutil reset ListenEvent "$BUNDLE"

# shellcheck disable=SC2024  # the log file is meant to be the user's, not root's
sudo -n log stream --style compact --predicate 'subsystem == "com.apple.TCC"' > "$OUT/tcc.log" 2>&1 &
LOGGER=$!
trap 'sudo -n kill $LOGGER 2>/dev/null; tccutil reset ListenEvent "$BUNDLE" >/dev/null; rm -rf "$APP"' EXIT
sleep 2

for variant in full no-motion; do
  echo "== $variant =="
  rm -f "$OUT/$variant".{out,err,alert,tcc}
  open -n -W --stdout "$OUT/$variant.out" --stderr "$OUT/$variant.err" "$APP" --args "$variant" &
  OPENER=$!
  sleep 2
  dump_alert deny | tee "$OUT/$variant.alert"
  wait "$OPENER"
  cat "$OUT/$variant.out"
  pid="$(sed -n 's/^pid=//p' "$OUT/$variant.out")"
  sleep 1
  # tccd names the spike's own requests msgID=<pid>.<n>; WindowServer's check
  # on its behalf carries the pid inside the attribution instead.
  grep -E "msgID=$pid\.|pid=$pid,|does not allow prompting|PROMPTING" "$OUT/tcc.log" > "$OUT/$variant.tcc" || true
  echo "tcc_lines=$(wc -l < "$OUT/$variant.tcc" | tr -d ' ')"
  { grep -oE "(responsible|requesting|accessing)=\{TCCDProcess: identifier=[^,]+, pid=$pid" "$OUT/$variant.tcc" || true; } | sort -u
  { grep -E "does not allow prompting|PROMPTING" "$OUT/$variant.tcc" || true; } | sed 's/.*\] /tccd: /' | sort -u
  { grep -oE "AUTHREQ_RESULT: msgID=$pid\.[0-9]+, authValue=[0-9]+, authReason=[0-9]+" "$OUT/$variant.tcc" || true; } | sort -u
  if grep -q "^texts=" "$OUT/$variant.alert"; then echo "dialog=yes"; else echo "dialog=no"; fi
done
echo "evidence: $OUT"
