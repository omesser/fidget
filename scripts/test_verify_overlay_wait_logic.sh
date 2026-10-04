#!/usr/bin/env bash
# Test the still-frame and hit-test wait logic in verify-overlay.sh to ensure
# they read from the tail of the log rather than finding the first occurrence.

set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

echo "Test 1: Still-frame detection polls the tail, not the first occurrence"

cat > "$TEMP_DIR/app.log" << 'EOF'
character: Test Character
overlay: overlay-0 covers 1920x1080 at (0,0)
sprite 176x160
frame: 10 Falling sprite(500,200)
frame: 11 Falling sprite(500,300)
frame: 12 Falling sprite(500,400)
frame: 13 Perched sprite(906,639) land#0
frame: 14 Perched sprite(906,639) land#0
frame: 15 Perched sprite(906,639) land#0
frame: 16 Perched sprite(906,639) land#0
frame: 100 Perched sprite(800,500)
frame: 101 Perched sprite(800,500)
EOF

python3 - "$TEMP_DIR" << 'PY'
import re, sys, time
out = sys.argv[1]

def read_tail_frames(log_path, n=10):
    with open(log_path) as f:
        lines = f.readlines()
    return [
        (m[1], m[2])
        for line in lines
        if (m := re.match(r"^frame: .* sprite\((-?\d+),(-?\d+)\)", line))
    ][-n:]

log_path = f"{out}/app.log"
for _ in range(40):
    frames = read_tail_frames(log_path, n=5)
    if len(frames) >= 3 and all(f == frames[0] for f in frames):
        pos = frames[0]
        break
    time.sleep(0.25)
else:
    log = open(log_path).read()
    at = re.findall(r"^frame: .* sprite\((-?\d+),(-?\d+)\)", log, re.M)
    if not at:
        sys.exit(1)
    pos = at[-1]

print(f"{pos[0]},{pos[1]}")
PY

RESULT=$(
  python3 - "$TEMP_DIR" << 'PY'
import re, sys, time
out = sys.argv[1]

def read_tail_frames(log_path, n=10):
    with open(log_path) as f:
        lines = f.readlines()
    return [
        (m[1], m[2])
        for line in lines
        if (m := re.match(r"^frame: .* sprite\((-?\d+),(-?\d+)\)", line))
    ][-n:]

log_path = f"{out}/app.log"
for _ in range(40):
    frames = read_tail_frames(log_path, n=5)
    if len(frames) >= 3 and all(f == frames[0] for f in frames):
        pos = frames[0]
        break
    time.sleep(0.25)
else:
    log = open(log_path).read()
    at = re.findall(r"^frame: .* sprite\((-?\d+),(-?\d+)\)", log, re.M)
    if not at:
        sys.exit(1)
    pos = at[-1]

print(f"{pos[0]},{pos[1]}")
PY
)

if [ "$RESULT" = "800,500" ]; then
  echo "  PASS  detected the last still sprite (800,500), not the first (906,639)"
else
  echo "  FAIL  got $RESULT, expected 800,500"
  exit 1
fi

echo ""
echo "Test 2: Still-frame detection handles moving sprite gracefully"

cat > "$TEMP_DIR/moving.log" << 'EOF'
sprite 176x160
frame: 1 Falling sprite(100,100)
frame: 2 Falling sprite(100,200)
frame: 3 Falling sprite(100,300)
frame: 4 Falling sprite(100,400)
frame: 5 Falling sprite(100,500)
EOF

RESULT=$(
  python3 - "$TEMP_DIR/moving.log" << 'PY'
import re, sys, time
out = sys.argv[1]

def read_tail_frames(log_path, n=10):
    with open(log_path) as f:
        lines = f.readlines()
    return [
        (m[1], m[2])
        for line in lines
        if (m := re.match(r"^frame: .* sprite\((-?\d+),(-?\d+)\)", line))
    ][-n:]

log_path = out
max_attempts = 2
for _ in range(max_attempts):
    frames = read_tail_frames(log_path, n=5)
    if len(frames) >= 3 and all(f == frames[0] for f in frames):
        pos = frames[0]
        break
    time.sleep(0.25)
else:
    log = open(log_path).read()
    at = re.findall(r"^frame: .* sprite\((-?\d+),(-?\d+)\)", log, re.M)
    if not at:
        sys.exit(1)
    pos = at[-1]

print(f"{pos[0]},{pos[1]}")
PY
)

if [ "$RESULT" = "100,500" ]; then
  echo "  PASS  fell back to last frame when sprite never stood still"
else
  echo "  FAIL  got $RESULT, expected 100,500"
  exit 1
fi

echo ""
echo "Test 3: Hit-test wait logic matches cursor position before reading decision"

cat > "$TEMP_DIR/hittest1.log" << 'EOF'
hit-test: frame#10 cursor(100,100) -> HIT via alpha=255
hit-test: frame#50 cursor(200,200) -> miss via alpha=0
hit-test: frame#90 cursor(300,300) -> HIT via alpha=200
EOF

line=""
for _ in $(seq 1 40); do
  line=$(grep 'hit-test:' "$TEMP_DIR/hittest1.log" | tail -1)
  if echo "$line" | grep -q "cursor(300,300)"; then
    break
  fi
  sleep 0.01
done

if echo "$line" | grep -q "cursor(300,300)" && echo "$line" | grep -q "HIT"; then
  echo "  PASS  finds cursor(300,300) with HIT decision"
else
  echo "  FAIL  got: $line"
  exit 1
fi

cat > "$TEMP_DIR/hittest2.log" << 'EOF'
hit-test: frame#10 cursor(100,100) -> HIT via alpha=255
hit-test: frame#50 cursor(200,200) -> miss via alpha=0
EOF

line=""
for _ in $(seq 1 40); do
  line=$(grep 'hit-test:' "$TEMP_DIR/hittest2.log" | tail -1)
  if echo "$line" | grep -q "cursor(200,200)"; then
    break
  fi
  sleep 0.01
done

if echo "$line" | grep -q "cursor(200,200)" && echo "$line" | grep -q "miss"; then
  echo "  PASS  finds cursor(200,200) with miss decision"
else
  echo "  FAIL  got: $line"
  exit 1
fi

echo ""
echo "Test 4: Hit-test wait handles stale lines correctly"

cat > "$TEMP_DIR/stale.log" << 'EOF'
hit-test: frame#10 cursor(100,100) -> HIT via alpha=255
hit-test: frame#20 cursor(100,100) -> HIT via alpha=255
EOF

cat >> "$TEMP_DIR/stale.log" << 'EOF'
hit-test: frame#120 cursor(500,600) -> miss via alpha=0
EOF

line=""
for _ in $(seq 1 40); do
  line=$(grep 'hit-test:' "$TEMP_DIR/stale.log" | tail -1)
  if echo "$line" | grep -q "cursor(500,600)"; then
    break
  fi
  sleep 0.01
done

if echo "$line" | grep -q "cursor(500,600)" && echo "$line" | grep -q "miss"; then
  echo "  PASS  waits past stale cursor(100,100) lines to find cursor(500,600)"
else
  echo "  FAIL  got: $line"
  exit 1
fi

echo ""
echo "All wait logic tests passed."
