#!/usr/bin/env bash
# Scenario: hero-gif (macOS only, since the README needs one recording, not one per OS)
# On screen: launches Fidget as one Character (default Buddy Bot) with a Harness
#   and records the main display until the last beat, 90 s at most. The terminal cues five beats: throw
#   the sprite at a window's top edge, poke it, double-click it (Chat opens; keep
#   it open, or the first-run tour bubble lands 25 s after launch), type a
#   question, Enter, and throw it again once the reply lands as a speech bubble
#   and in Chat: Claude Code's words under --harness claude, "Hello" from the fixture.
# Input: yours, at the mouse and keyboard. Each cue waits for the last action's reply. None sent.
# Duration: about 90 s, 3 min at most. Grants: Screen Recording, Accessibility.
# Asserts: the recording exists and ffprobe reads it. The look is yours to judge
#   from the contact sheet and the full-frame GIF in the evidence directory.
#
# Usage: hero-gif.sh --go <fidget binary> <fidget test binary> [--harness fixture|claude|grok] [--character <id>]
#        hero-gif.sh --crop x:y:w:h [--webp] <recording.mp4> [<from s> [<length s>]]
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# --harness claude links ~/.claude, ~/.claude.json, ~/.npm and ~/Library/Keychains
#   into the private HOME: sign-in and warm npx carry over, Fidget's data stays isolated.
# --crop re-encodes a saved recording to <recording>-hero.mp4, launching nothing;
#   --webp writes a 960 px looping <recording>-hero.webp. x:y:w:h is in recording pixels.
set -euo pipefail

ffmpeg=/opt/homebrew/bin/ffmpeg
root=$(cd "$(dirname "$0")/../.." && pwd)
record=90

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# Two passes: 128 colours drawn from what moves, ordered dither. Under 3 MB
# for a 15 s loop needs 15 fps and a crop, not the whole display.
to_gif() { # <in> <out> <crop filter or empty> <from s> <length s>
  local vf="fps=15,${3}scale=800:-1:flags=lanczos"
  "$ffmpeg" -y -v error -ss "$4" -t "$5" -i "$1" \
    -vf "$vf,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
    -loop 0 "$2"
  sized "$2"
}

# du rounds by allocated blocks and misread a fresh file, so print bytes.
sized() { echo "$1: $(($(stat -f %z "$1") / 1024)) KB"; }

to_mp4() { # <in> <out> <crop filter> <from s> <length s>
  "$ffmpeg" -y -v error -ss "$4" -t "$5" -i "$1" \
    -vf "fps=30,${3}scale=trunc(iw/2)*2:trunc(ih/2)*2,setsar=1" \
    -c:v libx264 -crf 23 -preset slow -pix_fmt yuv420p -movflags +faststart "$2"
  sized "$2"
}

# Homebrew ffmpeg has no WebP encoder, so go through a 256-colour GIF and gif2webp.
# A full-detail GIF of a window with live graphs runs near 10 MB; 960 px at 10 fps
# as WebP is about 3 MB for 40 s.
to_webp() { # <in> <out> <crop filter> <from s> <length s>
  local gif="${2%.webp}.gif"
  "$ffmpeg" -y -v error -ss "$4" -t "$5" -i "$1" \
    -vf "fps=10,${3}scale=960:-2:flags=lanczos,split[a][b];[a]palettegen=max_colors=256:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle" \
    -loop 0 "$gif"
  /opt/homebrew/bin/gif2webp -q 75 -m 6 -mixed "$gif" -o "$2" > /dev/null 2>&1
  rm -f "$gif"
  sized "$2"
}

case "${1:-}" in
  --crop)
    crop_usage="usage: hero-gif.sh --crop x:y:w:h [--webp] <recording.mp4> [<from s> [<length s>]]"
    crop=${2:?$crop_usage}
    shift 2
    encode=to_mp4 ext=mp4
    if [ "${1:-}" = --webp ]; then
      encode=to_webp ext=webp
      shift
    fi
    rec=${1:?$crop_usage}
    IFS=: read -r x y w h <<< "$crop"
    "$encode" "$rec" "${rec%.*}-hero.$ext" "crop=$w:$h:$x:$y," "${2:-0}" "${3:-15}"
    exit 0
    ;;
  --go) ;;
  *)
    sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0"
    exit 2
    ;;
esac
shift

usage="usage: hero-gif.sh --go <fidget binary> <fidget test binary> [--harness fixture|claude|grok] [--character <id>]"
harness_kind=fixture character=buddy-bot
bin=${1:?$usage}
test_bin=${2:?$usage}
shift 2
while [ $# -gt 0 ]; do
  case "$1" in
    --harness) harness_kind=${2:?$usage} ;;
    --character) character=${2:?$usage} ;;
    *) fail "$usage" ;;
  esac
  shift 2
done
# Buddy Bot draws 90 px square and Trump 108 px at 1x, so a crop tuned for
# one is loose or tight on the other.
[ -d "$root/characters/$character" ] || fail "no such character: $root/characters/$character"
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-hero-gif-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home"
home="$out/home"
log="$out/app.log" marks="$out/harness.log" rec="$out/recording.mp4"
: > "$marks"

# avfoundation numbers the displays after the cameras, so look the index up.
# ffmpeg exits non-zero after listing, so the pipeline must not trip pipefail.
screen=$({ "$ffmpeg" -hide_banner -f avfoundation -list_devices true -i "" 2>&1 || true; } |
  sed -n "s/.*\[\([0-9]*\)\] Capture screen ${FIDGET_HERO_DISPLAY:-0}\$/\1/p")
[ -n "$screen" ] || fail "no avfoundation screen device; grant Screen Recording to the terminal"

case "$harness_kind" in
  fixture)
    # FIDGET_HARNESS splits on whitespace, so no path in it may hold a space.
    harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=hero count=$marks"
    [ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"
    ;;
  claude)
    harness=claude
    command -v npx > /dev/null || fail "npx is not on PATH; the claude Harness runs on Node"
    # claude-agent-acp needs Node 22 and, on an older one, hangs instead of attaching.
    node_major=$(node -p 'process.versions.node.split(".")[0]')
    [ "$node_major" -ge 22 ] || fail "node $(node -v) is on PATH; the claude Harness needs 22 or newer"
    # The login sits in the login keychain, not under ~/.claude, and `security`
    # falls back to `$HOME/Library/Keychains/login.keychain-db` for its search
    # list, so CLAUDE_CONFIG_DIR alone would still read no keychain.
    [ -e "$HOME/.claude.json" ] || fail "no $HOME/.claude.json; sign in with \`claude /login\` first"
    mkdir -p "$out/home/Library"
    for entry in .claude .claude.json .npm Library/Keychains; do
      [ -e "$HOME/$entry" ] || fail "no $HOME/$entry"
      ln -s "$HOME/$entry" "$out/home/$entry"
    done
    ;;
  grok)
    # grok signs in from its own files under the real HOME and needs no Node.
    harness=grok home=$HOME
    command -v grok > /dev/null || fail "grok is not on PATH"
    ;;
  *) fail "$usage" ;;
esac

env HOME="$home" \
  FIDGET_HARNESS="$harness" FIDGET_TRACE_DIRECTOR=1 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER="$character" FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true; pkill -f "count=$marks" || true' EXIT

sleep 4
kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
# A real Harness opens with its own turn. Record once that reply lands, or a
# question typed on cue queues behind it and misses the take.
if [ "$harness_kind" != fixture ]; then
  echo ">>> warming up: waiting for $harness_kind's first reply before recording (60 s at most)"
  for _ in $(seq 60); do
    grep -q 'Static fallback' "$log" && fail "the Harness failed its first turn; see $log"
    grep -q '^harness: reply ' "$log" && break
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    sleep 1
  done
fi
"$ffmpeg" -y -v error -f avfoundation -capture_cursor 1 -framerate 30 -i "$screen:none" \
  -t "$record" -vf 'crop=trunc(iw/2)*2:trunc(ih/2)*2' -c:v libx264 -preset ultrafast -crf 18 -pix_fmt yuv420p \
  "$rec" > "$out/ffmpeg.log" 2>&1 &
ffpid=$!
t0=$SECONDS

now() { echo $((SECONDS - t0)); }
cue() { # <text>
  echo ">>> $(now)s  $1"
}
# Every action makes the sprite think and then talk, and a newer action cancels
# the reply in flight. So each cue waits for the action's trigger, the Harness
# reply after it, and a beat to read it. Log lines are matched past `heard`.
heard=$(wc -l < "$log")
wait_for() { # <ERE>: true once a log line past `heard` matches; moves `heard` to it
  local n
  while [ "$(now)" -lt $((record - 3)) ]; do
    # StaticDirector's bubbles look like replies; a take on them is no take.
    if tail -n +"$((heard + 1))" "$log" | grep -q 'Static fallback'; then
      kill -INT "$ffpid" 2> /dev/null || true
      fail "the Harness failed a turn and StaticDirector spoke instead; see $log"
    fi
    n=$(tail -n +"$((heard + 1))" "$log" | grep -n -m1 -E "$1" | cut -d: -f1)
    if [ -n "$n" ]; then
      heard=$((heard + n))
      return 0
    fi
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    sleep 0.5
  done
  return 1
}
beat() { # <trigger> <next cue>
  if wait_for "happened=$1" && wait_for '^harness: reply '; then
    sleep 2
    cue "$2"
  else
    cue "no $1 reply by $((record - 3)) s. Hands off."
    return 1
  fi
}

cue "recording. Throw it at the window's top edge, then wait for it to speak."
# A missed beat has already said so; `|| true` only keeps `set -e` from ending the take.
# shellcheck disable=SC2015
beat Perch "it spoke. Poke it once, move the mouse off, and wait." &&
  beat Poke "it spoke. Double-click it. Chat opens; leave it open and wait." &&
  beat Summon "it spoke. Type in Chat: What's in the news today?  Then press Enter." &&
  beat 'Chat\(' "it answered. Throw it once more, anywhere." &&
  beat Perch "done. Hands off while the recording closes." || true
# SIGINT makes ffmpeg finish the file; it exits 255 for that, so judge the file.
kill -INT "$ffpid" 2> /dev/null || true
wait "$ffpid" || true

[ -s "$rec" ] || fail "no recording at $rec; see $out/ffmpeg.log"
secs=$(/opt/homebrew/bin/ffprobe -v error -show_entries format=duration -of csv=p=0 "$rec")
[ -n "$secs" ] || fail "ffprobe cannot read $rec; see $out/ffmpeg.log"

"$ffmpeg" -y -v error -i "$rec" -vf "fps=45/$secs,scale=480:-1,tile=5x9" -frames:v 1 "$out/contact-sheet.png"
to_gif "$rec" "$out/full-frame.gif" "" 0 "$secs"
echo "PASS: evidence in $out"
echo "next: $0 --crop x:y:w:h $rec [<from s> [<length s>]]"
