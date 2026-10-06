# The poke-mid-climb trace check, shared by the macOS and X11 leaves.
# poke-mid-climb.win.ps1 carries a PowerShell copy; keep the messages identical.
# Usage: python3 poke-mid-climb-check.py <app log>
import re, sys

frame = re.compile(r"^frame: (\d+) (\w+) pos\((-?\d+),(-?\d+)\) \S+ (\S+)#(\d+) ")
frames, poke_at, state = [], None, None
for line in open(sys.argv[1], errors="replace"):
    m = frame.match(line)
    if m:
        at, st, x, y, animation, index = m.groups()
        frames.append((int(at), st, int(x), int(y), animation, int(index)))
        state = st
    elif poke_at is None and line.startswith("verbs:") and "Poke" in line and state == "Climbing":
        poke_at = len(frames)


def miss(message):
    print(message, file=sys.stderr)
    sys.exit(1)


if poke_at is None:
    miss("no Poke landed on a climbing sprite")
after = frames[poke_at:]
# POKE_COOLDOWN_MS is 2500; the windows leave a tick either side of it.
span = after[-1][0] - after[0][0] if after else 0
if span < 2300:
    miss(f"the trace stops {span / 1000:.1f} s after the Poke")
paused = [f for f in after if f[0] - after[0][0] < 2300]
resumed = [f for f in after if 2600 <= f[0] - after[0][0] <= 3500]
if any(f[1] != "Climbing" for f in paused):
    miss("the sprite left the wall during the pause")
if len({(f[2], f[3]) for f in paused}) != 1:
    miss("the sprite moved during the pause")
if after[0][4:] != ("react", 0):
    miss("the Poke did not start react over")
if len({f[5] for f in paused if f[4] == "climb"}) > 1:
    miss("the sprite climbed in place during the pause")
if not any(f[1] == "Climbing" and f[3] < paused[0][3] for f in resumed):
    miss("the sprite did not climb on after the cooldown")
print(f"ok: paused at {paused[0][2:4]} for {paused[-1][0] - paused[0][0]} ms in one climb pose, then climbed on")
