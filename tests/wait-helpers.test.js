// The shared wait returns as soon as its event shows up and gives up within
// the seconds it is given. It is plain bash with BSD-safe tools, so it runs
// here on Linux too. Each platform helper has to load it.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { test } from "node:test";
import { tempDir } from "./temp-path.js";

const scenarios = path.join(import.meta.dirname, "..", "scripts", "scenarios", "helpers");
const helper = path.join(scenarios, "wait-helpers.sh");

function run(t, body, file = helper) {
  const dir = tempDir(t, "wait-mac-");
  const script = `set -u\n. "${file}"\ndir="${dir}"\nlog="$dir/app.log"; : > "$log"\n${body}`;
  const started = Date.now();
  // Above the longest wait here (10 s overall rest cap). Without this, a
  // stuck wait_for_rest sits until node:test's 60 s.
  const r = spawnSync("bash", ["-c", script], { encoding: "utf8", timeout: 15_000 });
  const ms = Date.now() - started;
  const timedOut = r.error && r.error.code === "ETIMEDOUT";
  return { code: r.status, out: timedOut ? `run() timed out after 15s\n${r.stdout}${r.stderr}` : r.stdout + r.stderr, ms };
}

test("wait_for_trace returns soon after a line lands", (t) => {
  const r = run(t, `: > "$dir/log"; (sleep 0.15; echo "verbs: Poke" >> "$dir/log") & wait_for_trace 2 "$dir/log" '^verbs: .*Poke'`);
  assert.strictEqual(r.code, 0, r.out);
  assert.ok(r.ms < 1000, `took ${r.ms} ms`);
});

test("wait_for_trace sees a line already there without waiting", (t) => {
  const r = run(t, `echo asked > "$dir/marks"; wait_for_marks 2 "$dir/marks" '^asked$'`);
  assert.strictEqual(r.code, 0, r.out);
  assert.ok(r.ms < 500, `took ${r.ms} ms`);
});

test("wait_for_marks gives up within its seconds", (t) => {
  const r = run(t, `: > "$dir/marks"; wait_for_marks 1 "$dir/marks" '^never$'`);
  assert.strictEqual(r.code, 1, r.out);
  assert.ok(r.ms >= 900 && r.ms < 2000, `took ${r.ms} ms`);
});

test("a timed-out wait records a thread dump and the app log tail", (t) => {
  const r = run(t, `sleep 30 & pid=$!
out="$dir"
echo "overlay overlay-0: showThinking instance=inst-1 dnd=off" > "$log"
wait_for_trace 1 "$log" '^never-this$'
status=$?
kill "$pid" 2> /dev/null || true
wait "$pid" 2> /dev/null || true
[ "$status" -eq 1 ]
[ -s "$out/fidget-threads.txt" ]
grep -F "showThinking instance=inst-1" "$out/fidget-log-tail.txt"`);
  assert.strictEqual(r.code, 0, r.out);
  assert.match(r.out, /thread dump in .*fidget-threads\.txt/);
  assert.match(r.out, /app log tail in .*fidget-log-tail\.txt/);
  assert.ok(r.ms < 12000, `took ${r.ms} ms`);
});

test("a wait_until that passes on its last check leaves no thread dump", (t) => {
  const r = run(t, `sleep 30 & pid=$!
out="$dir"
: > "$log"
cat > "$dir/probe.sh" << 'EOF'
c=$(cat "$1" 2>/dev/null || echo 0)
echo $((c + 1)) > "$1"
[ "$c" -ge 1 ]
EOF
chmod +x "$dir/probe.sh"
wait_until 1 "$dir/probe.sh" "$dir/n"
status=$?
kill "$pid" 2> /dev/null || true
wait "$pid" 2> /dev/null || true
[ "$status" -eq 0 ]
[ ! -e "$out/fidget-threads.txt" ]`);
  assert.strictEqual(r.code, 0, r.out);
  assert.doesNotMatch(r.out, /thread dump in/);
});

test("eu-stack's error does not hide a gdb stack", (t) => {
  const r = run(t, `set -e
mkdir -p "$dir/bin"
printf '%s\n' '#!/bin/sh' 'echo "eu-stack: Permission denied" >&2' 'exit 2' > "$dir/bin/eu-stack"
printf '%s\n' '#!/bin/sh' 'echo "GDB STACK"' > "$dir/bin/gdb"
chmod +x "$dir/bin/eu-stack" "$dir/bin/gdb"
sleep 30 & pid=$!
out="$dir"
echo "showThinking" > "$log"
PATH="$dir/bin:$PATH" wait_for_trace 1 "$log" '^never-this$' || true
kill "$pid" 2> /dev/null || true
wait "$pid" 2> /dev/null || true
grep -F "GDB STACK" "$out/fidget-threads.txt"
! grep -F "Permission denied" "$out/fidget-threads.txt"`);
  assert.strictEqual(r.code, 0, r.out);
});

test("skip_thread_dump leaves a timed-out wait alone", (t) => {
  const r = run(t, `sleep 30 & pid=$!
out="$dir"
echo "showThinking" > "$log"
skip_thread_dump=1 wait_for_trace 1 "$log" '^never-this$' || true
kill "$pid" 2> /dev/null || true
wait "$pid" 2> /dev/null || true
[ ! -e "$out/fidget-threads.txt" ]`);
  assert.strictEqual(r.code, 0, r.out);
  assert.doesNotMatch(r.out, /thread dump in/);
});

test("a wait that sees its line leaves no thread dump", (t) => {
  const r = run(t, `sleep 30 & pid=$!
out="$dir"
echo "verbs: Poke" > "$log"
wait_for_trace 2 "$log" '^verbs: .*Poke'
status=$?
kill "$pid" 2> /dev/null || true
wait "$pid" 2> /dev/null || true
[ "$status" -eq 0 ]
[ ! -e "$out/fidget-threads.txt" ]
[ ! -e "$out/fidget-log-tail.txt" ]`);
  assert.strictEqual(r.code, 0, r.out);
  assert.doesNotMatch(r.out, /thread dump in/);
});

test("waits fail at once when Fidget has exited", (t) => {
  const r = run(t, `: > "$dir/log"; sleep 0 & pid=$!; wait "$pid"; wait_for_trace 2 "$dir/log" x`);
  assert.strictEqual(r.code, 1, r.out);
  assert.ok(r.ms < 500, `took ${r.ms} ms`);
});

test("wait_for_file returns soon after the file appears", (t) => {
  const r = run(t, `(sleep 0.15; : > "$dir/f"; echo frame >> "$log") & wait_for_file 2 "$dir/f"`);
  assert.strictEqual(r.code, 0, r.out);
  assert.ok(r.ms < 1000, `took ${r.ms} ms`);
});

test("wait_until checks again when the app log grows", (t) => {
  const r = run(t, `(sleep 0.2; : > "$dir/flag"; echo frame >> "$log") & wait_until 2 test -f "$dir/flag"`);
  assert.strictEqual(r.code, 0, r.out);
  assert.ok(r.ms >= 200 && r.ms < 1000, `took ${r.ms} ms`);
});

test("wait_until gives up within its seconds", (t) => {
  const r = run(t, `wait_until 1 false`);
  assert.strictEqual(r.code, 1, r.out);
  assert.ok(r.ms >= 900 && r.ms < 2000, `took ${r.ms} ms`);
});

test("wait_for_count returns only once the new line lands", (t) => {
  const r = run(t, `echo minimized > "$dir/t"; before=$(line_count "$dir/t" '^minimized$')
(sleep 0.4; echo minimized >> "$dir/t") & wait_for_count 2 "$dir/t" '^minimized$' "$before"`);
  assert.strictEqual(r.code, 0, r.out);
  assert.ok(r.ms >= 400 && r.ms < 1200, `took ${r.ms} ms`);
});

test("wait_for_count does not count the line already there", (t) => {
  const r = run(t, `echo minimized > "$dir/t"; wait_for_count 1 "$dir/t" '^minimized$' 1`);
  assert.strictEqual(r.code, 1, r.out);
  assert.ok(r.ms >= 900 && r.ms < 2000, `took ${r.ms} ms`);
});

test("wait_for_file gives up within its seconds", (t) => {
  const r = run(t, `wait_for_file 1 "$dir/never"`);
  assert.strictEqual(r.code, 1, r.out);
  assert.ok(r.ms >= 900 && r.ms < 2000, `took ${r.ms} ms`);
});

test("in_state reads the last frame line", (t) => {
  const r = run(t, `printf 'frame: a Idle x\\nframe: b Climb x\\n' > "$dir/l"; in_state "$dir/l" Climb && ! in_state "$dir/l" Idle`);
  assert.strictEqual(r.code, 0, r.out);
});

test("stop_fidget kills Fidget and the processes under it", (t) => {
  const r = run(t, `bash -c 'sleep 30 & wait' & pid=$!; sleep 0.2; child=$(pgrep -P "$pid"); stop_fidget; sleep 0.1
! kill -0 "$pid" 2> /dev/null && ! kill -0 "$child" 2> /dev/null`);
  assert.strictEqual(r.code, 0, r.out);
});

test("stop_fidget kills a tree that ignores TERM within about 3 s", (t) => {
  const r = run(t, `bash -c 'trap "" TERM; sleep 30 & wait' & pid=$!; sleep 0.2; child=$(pgrep -P "$pid"); stop_fidget
! kill -0 "$pid" 2> /dev/null && ! kill -0 "$child" 2> /dev/null`);
  assert.strictEqual(r.code, 0, r.out);
  assert.ok(r.ms >= 2900 && r.ms < 4500, `took ${r.ms} ms`);
});

// The middle bash is not a last simple command, or bash execs it and the
// grandchild's parent pid disappears. set -m stays inside the stand-in, so
// the grandchild's group is not the stand-in's.
const standInTree = `bash -c 'set -m; bash -c "sleep 60 & wait" & wait'`;

test("stop_fidget reaps a grandchild whose process group is not Fidget's", (t) => {
  const r = run(t, `pid= child= grand=
trap 'for p in "$pid" "$child" "$grand"; do [ -n "$p" ] && kill -9 "$p" 2> /dev/null || true; done' EXIT
${standInTree} & pid=$!
sleep 0.3
child=$(pgrep -P "$pid" | head -1)
[ -n "$child" ] || { echo "missing child of $pid"; exit 1; }
grand=$(pgrep -P "$child" | head -1)
[ -n "$grand" ] || { echo "missing grandchild of $child"; exit 1; }
[ "$(ps -o ppid= -p "$grand" | tr -d '[:space:]')" = "$child" ] || { echo "grandchild ppid is $(ps -o ppid= -p "$grand")"; exit 1; }
fpgid=$(ps -o pgid= -p "$pid" | tr -d '[:space:]')
cpgid=$(ps -o pgid= -p "$child" | tr -d '[:space:]')
gpgid=$(ps -o pgid= -p "$grand" | tr -d '[:space:]')
[ "$gpgid" = "$cpgid" ] && [ "$gpgid" != "$fpgid" ] || { echo "pgid fidget=$fpgid child=$cpgid grand=$gpgid"; exit 1; }
stop_fidget
sleep 0.2
alive=
kill -0 "$pid" 2> /dev/null && alive="$alive fidget"
kill -0 "$child" 2> /dev/null && alive="$alive child"
kill -0 "$grand" 2> /dev/null && alive="$alive grand"
[ -z "$alive" ] || { echo "still alive:$alive"; exit 1; }`);
  assert.strictEqual(r.code, 0, r.out);
  assert.ok(r.ms < 2000, `took ${r.ms} ms`);
});

// The bystander shares the stand-in's process group. It has to stay alive.
// Signaling that group would kill it and this script's other jobs.
test("stop_fidget does not signal its own process group", (t) => {
  const r = run(t, `pid= child= grand= bystander=
trap 'for p in "$pid" "$child" "$grand" "$bystander"; do [ -n "$p" ] && kill -9 "$p" 2> /dev/null || true; done' EXIT
sleep 60 & bystander=$!
${standInTree} & pid=$!
sleep 0.3
child=$(pgrep -P "$pid" | head -1)
[ -n "$child" ] && [ -n "$bystander" ] || { echo "missing child=$child bystander=$bystander"; exit 1; }
grand=$(pgrep -P "$child" | head -1)
[ -n "$grand" ] || { echo "missing grandchild of $child"; exit 1; }
fpgid=$(ps -o pgid= -p "$pid" | tr -d '[:space:]')
bpgid=$(ps -o pgid= -p "$bystander" | tr -d '[:space:]')
gpgid=$(ps -o pgid= -p "$grand" | tr -d '[:space:]')
cpgid=$(ps -o pgid= -p "$child" | tr -d '[:space:]')
[ "$bpgid" = "$fpgid" ] && [ "$gpgid" = "$cpgid" ] && [ "$gpgid" != "$fpgid" ] || { echo "pgid fidget=$fpgid bystander=$bpgid child=$cpgid grand=$gpgid"; exit 1; }
stop_fidget
sleep 0.2
kill -0 "$bystander" 2> /dev/null || { echo "bystander died"; exit 1; }
kill "$bystander" 2> /dev/null || true
wait "$bystander" 2> /dev/null || true
bystander=`);
  assert.strictEqual(r.code, 0, r.out);
});

for (const leaf of ["mac", "x11"]) {
  test(`wait-helpers.${leaf}.sh loads the shared core`, (t) => {
    const r = run(t, `for f in wait_for_trace wait_for_marks wait_for_file wait_until wait_for_count stop_fidget in_state wait_for_ax_dump answer_permission; do declare -F "$f" > /dev/null || { echo "missing $f"; exit 1; }; done`, path.join(scenarios, `wait-helpers.${leaf}.sh`));
    assert.strictEqual(r.code, 0, r.out);
  });
}

// A child that ignores TERM keeps the inherited lock after the pid it was
// given is gone: the wait holds until its seconds run out.
test(
  "wait_for_exit waits on a held lock even when the pid is already gone",
  { skip: process.platform !== "linux" && "the lock wait needs flock" },
  (t) => {
    const r = run(t, `exec 9> "$dir/lock"; flock -n 9
(trap "" TERM; exec sleep 30) & child=$!
exec 9>&-
sleep 0 & dead=$!; wait "$dead"
wait_for_exit 1 "$dead" "$dir/lock"; rc=$?
kill -9 "$child"
exit "$rc"`);
    assert.strictEqual(r.code, 1, r.out);
    assert.ok(r.ms >= 900 && r.ms < 2000, `took ${r.ms} ms`);
  },
);

test(
  "wait_for_exit returns once the lock is free, with the pid already gone",
  { skip: process.platform !== "linux" && "the lock wait needs flock" },
  (t) => {
    const r = run(t, `: > "$dir/lock"; sleep 0 & dead=$!; wait "$dead"; wait_for_exit 2 "$dead" "$dir/lock"`);
    assert.strictEqual(r.code, 0, r.out);
    assert.ok(r.ms < 500, `took ${r.ms} ms`);
  },
);

test("seed_tour_shown marks the first-run tour as shown", (t) => {
  const r = run(t, `seed_tour_shown "$dir/home"; grep -q '"first_run_tour_shown": true' "$dir/home/Library/Application Support/fidget/settings.json"`, path.join(scenarios, "wait-helpers.mac.sh"));
  assert.strictEqual(r.code, 0, r.out);
});

// The Windows helper blocks on FileSystemWatcher events. pwsh runs it here.
test("sprite_at_rest needs the same spot in all five frames", (t) => {
  const frames = (xs) => xs.map((x) => `echo "frame: 1 Grounded pos(1,1) sprite(${x},20) idle#1" >> "$log"`).join("; ");
  const still = run(t, `${frames([10, 10, 10, 10, 10])}; sprite_at_rest "$log"`);
  assert.strictEqual(still.code, 0, still.out);
  const moved = run(t, `${frames([10, 10, 10, 10, 11])}; sprite_at_rest "$log"`);
  assert.notStrictEqual(moved.code, 0, "a frame in another spot counted as rest");
  const walked = run(t, `${frames([10, 30, 50, 70, 90])}; sprite_at_rest "$log"`);
  assert.notStrictEqual(walked.code, 0, "five spots counted as rest");
});

test("wait_for_rest keeps waiting while the sprite walks, past 3 s", (t) => {
  const walk = 'for i in $(seq 1 16); do echo "frame: 1 Walking pos(1,1) sprite($((i * 10)),20) walk#1" >> "$log"; sleep 0.25; done; for i in 1 2 3 4 5; do echo "frame: 1 Grounded pos(1,1) sprite(160,20) idle#1" >> "$log"; done';
  const r = run(t, `pid=$$; (${walk}) & wait_for_rest "$log"`);
  assert.strictEqual(r.code, 0, r.out);
  assert.ok(r.ms >= 3500 && r.ms < 6000, `took ${r.ms} ms`);
});

test("wait_for_rest gives up 3 s after the last move", (t) => {
  const r = run(t, `pid=$$; echo "frame: 1 Walking pos(1,1) sprite(10,20) walk#1" >> "$log"; (sleep 0.5; echo "frame: 1 Walking pos(1,1) sprite(10,20) walk#1" >> "$log") & wait_for_rest "$log"`);
  assert.notStrictEqual(r.code, 0, r.out);
  assert.ok(r.ms >= 2800 && r.ms < 4500, `took ${r.ms} ms`);
});

test("wait_for_rest gives up at 10 s if the sprite never stops", (t) => {
  const walk = 'i=0; while :; do i=$((i + 1)); echo "frame: 1 Walking pos(1,1) sprite($((i * 10)),20) walk#1" >> "$log"; sleep 0.25; done';
  const r = run(t, `pid=$$; (${walk}) & w=$!; wait_for_rest "$log"; st=$?; kill $w 2>/dev/null; exit $st`);
  assert.notStrictEqual(r.code, 0, r.out);
  assert.match(r.out, /sprite never came to rest within 10s/);
  assert.ok(r.ms >= 9000 && r.ms < 13000, `took ${r.ms} ms`);
});

const pwsh = spawnSync("pwsh", ["-NoProfile", "-Command", "1"], { encoding: "utf8" });
const noPwsh = pwsh.status !== 0;
const winHelper = path.join(scenarios, "wait-helpers.win.ps1");

function ps(t, body) {
  const dir = tempDir(t, "wait-win-");
  const script = `. '${winHelper}'
function Later($c) { $p = New-Object Diagnostics.ProcessStartInfo "bash"; $p.ArgumentList.Add("-c"); $p.ArgumentList.Add("sleep 0.3; $c"); $null = [Diagnostics.Process]::Start($p) }
$W = '${dir}'
$data = Initialize-ScenarioData $W
${body}`;
  // Above the longest wait here (10 s overall rest cap). Without this, a
  // stuck Wait-ForRest sits until node:test's 60 s.
  const r = spawnSync("pwsh", ["-NoProfile", "-Command", script], { encoding: "utf8", timeout: 15_000 });
  const timedOut = r.error && r.error.code === "ETIMEDOUT";
  return {
    code: r.status,
    lines: r.stdout.trim().split("\n").map((l) => l.trim()),
    err: timedOut ? `ps() timed out after 15s\n${r.stderr}` : r.stderr,
  };
}

test("Windows: Wait-ForMarks returns soon after the line lands", { skip: noPwsh }, (t) => {
  const r = ps(t, `$m = "$W/marks"; Set-Content $m ""; Later "echo asked >> $m"; $t = Get-NowMs; Wait-ForMarks 2 $m '^asked$'; (Get-NowMs) - $t`);
  assert.strictEqual(r.lines[0], "True", r.err);
  assert.ok(Number(r.lines[1]) >= 250 && Number(r.lines[1]) < 1000, `took ${r.lines[1]} ms`);
});

test("Windows: Wait-ForCount ignores the line already there", { skip: noPwsh }, (t) => {
  const r = ps(t, `$m = "$W/marks"; Set-Content $m "spawn"; Wait-ForCount 1 $m '^spawn$' 1; Later "echo spawn >> $m"; Wait-ForCount 2 $m '^spawn$' 1`);
  assert.deepStrictEqual(r.lines.slice(0, 2), ["False", "True"], r.err);
});

test("Windows: Wait-Until re-checks on a trace change and gives up in time", { skip: noPwsh }, (t) => {
  const r = ps(t, `Later "echo x > $W/flag; echo frame >> $data/process.log"; Wait-Until 2 { Test-Path "$W/flag" }; $t = Get-NowMs; Wait-Until 1 { $false }; (Get-NowMs) - $t`);
  assert.deepStrictEqual(r.lines.slice(0, 2), ["True", "False"], r.err);
  assert.ok(Number(r.lines[2]) >= 900 && Number(r.lines[2]) < 2000, `took ${r.lines[2]} ms`);
});

test("Windows: Wait-ForFile sees the file appear", { skip: noPwsh }, (t) => {
  const r = ps(t, `Later "touch $data/process.log"; Wait-ForFile 2 "$data/process.log"`);
  assert.strictEqual(r.lines[0], "True", r.err);
});

test("Windows: Initialize-ScenarioData moves every data folder and seeds the tour", { skip: noPwsh }, (t) => {
  const r = ps(t, `$d = Initialize-ScenarioData $W @{ hide_in_fullscreen = $false }
$env:FIDGET_DATA_DIR -eq $d
$env:APPDATA.StartsWith($W)
$env:WEBVIEW2_USER_DATA_FOLDER.StartsWith($W)
Get-Content -Raw (Join-Path $d "settings.json")`);
  assert.deepStrictEqual(r.lines.slice(0, 3), ["True", "True", "True"], r.err);
  assert.deepStrictEqual(JSON.parse(r.lines[3]), { first_run_tour_shown: true, hide_in_fullscreen: false });
});

test("Windows: Get-SpriteAtRest needs five resting frames in one spot", { skip: noPwsh }, (t) => {
  const r = ps(t, `$t = "$W/p.log"
1..4 | % { Add-Content $t "1 frame: 1 Grounded pos(1,1) sprite(10,20) idle#1 x" }
[bool](Get-SpriteAtRest $t)
Add-Content $t "1 frame: 1 Grounded pos(1,1) sprite(10,20) idle#1 x"
(Get-SpriteAtRest $t).X
Add-Content $t "1 frame: 1 Falling pos(1,1) sprite(10,40) fall#1 x"
[bool](Get-SpriteAtRest $t)`);
  assert.deepStrictEqual(r.lines.slice(0, 3), ["False", "10", "False"], r.err);
});

test("Windows: a process.log write one folder below the watched folder wakes the wait", { skip: noPwsh }, (t) => {
  // The flag is outside the watched folder, so only the process.log write can wake it.
  const flag = path.join(tempDir(t, "wait-flag-"), "flag");
  const r = ps(t, `Later "touch ${flag}; echo frame >> $data/process.log"; $t = Get-NowMs; Wait-Until 3 { Test-Path '${flag}' }; (Get-NowMs) - $t`);
  assert.strictEqual(r.lines[0], "True", r.err);
  assert.ok(Number(r.lines[1]) < 1500, `took ${r.lines[1]} ms: the watcher missed the subfolder`);
});

test("Windows: Get-SpriteAtRest allows 2 px of jitter, not drift", { skip: noPwsh }, (t) => {
  const frame = (x) => `Add-Content $t "1 frame: 1 Grounded pos(1,1) sprite(${x},20) idle#1 x"`;
  const r = ps(t, `$t = "$W/p.log"
${[10, 11, 12, 11, 10].map(frame).join("\n")}
(Get-SpriteAtRest $t).X
${[10, 12, 14, 16, 18].map(frame).join("\n")}
[bool](Get-SpriteAtRest $t)
${[10, 10, 10, 10, 13].map(frame).join("\n")}
[bool](Get-SpriteAtRest $t)
${[10, 10, 10, 10, 210].map(frame).join("\n")}
[bool](Get-SpriteAtRest $t)`);
  assert.deepStrictEqual(r.lines.slice(0, 4), ["10", "False", "False", "False"], r.err);
});

const menuLeaf = path.join(scenarios, "..", "ci", "control-click-menu.win.ps1");

function walkThenRest(t, call) {
  return ps(t, `$t = "$W/p.log"
Set-Content "$W/walk.sh" @'
for i in $(seq 1 16); do echo "1 frame: 1 Grounded pos(1,1) sprite($((i * 10)),20) walk#1 x" >> "$1"; sleep 0.25; done
for i in 1 2 3 4 5; do echo "1 frame: 1 Grounded pos(1,1) sprite(160,20) idle#1 x" >> "$1"; done
'@
$p = New-Object Diagnostics.ProcessStartInfo "bash"
$p.ArgumentList.Add("$W/walk.sh")
$p.ArgumentList.Add($t)
$p.UseShellExecute = $false
$null = [Diagnostics.Process]::Start($p)
${call}`);
}

test("Windows: Wait-ForRest keeps waiting while the sprite walks, past 3 s", { skip: noPwsh }, (t) => {
  const r = walkThenRest(t, `$start = Get-NowMs
$rest = Wait-ForRest $t
$took = (Get-NowMs) - $start
Write-Output $took
Write-Output $(if ($rest) { $rest.X } else { "null" })`);
  assert.strictEqual(r.code, 0, r.err);
  assert.ok(Number(r.lines[0]) >= 3500 && Number(r.lines[0]) < 6000, `took ${r.lines[0]} ms`);
  assert.strictEqual(r.lines[1], "160", r.err);
});

// The leaf's Wait-Settled, not the helper. A walk past 3 s must still rest.
test("Windows: control-click-menu Wait-Settled keeps waiting while the sprite walks, past 3 s", { skip: noPwsh }, (t) => {
  const r = walkThenRest(t, `$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile('${menuLeaf}', [ref]$tokens, [ref]$errors)
$fn = $ast.FindAll({ $args[0] -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $args[0].Name -eq "Wait-Settled" }, $true)
if ($fn.Count -ne 1) { throw "missing Wait-Settled" }
Invoke-Expression $fn[0].Extent.Text
function Fail($m) { throw $m }
$traceFile = $t
$start = Get-NowMs
$rest = Wait-Settled
$took = (Get-NowMs) - $start
Write-Output $took
Write-Output $(if ($rest) { $rest.X } else { "null" })`);
  assert.strictEqual(r.code, 0, r.err);
  assert.ok(Number(r.lines[0]) >= 3500 && Number(r.lines[0]) < 6000, `took ${r.lines[0]} ms: ${r.err}`);
  assert.strictEqual(r.lines[1], "160", r.err);
});

test("Windows: Wait-ForRest keeps the rest it already saw when a walk frame lands", { skip: noPwsh }, (t) => {
  const r = ps(t, `$t = "$W/p.log"
1..4 | % { Add-Content $t "1 frame: 1 Grounded pos(1,1) sprite(160,20) idle#1 x" }
$orig = (Get-Command Get-SpriteAtRest).ScriptBlock
function Get-SpriteAtRest([string]$TraceFile) {
    $at = & $orig $TraceFile
    if ($at -and -not $script:walkAfterRest) {
        $script:walkAfterRest = $true
        Add-Content $TraceFile "1 frame: 1 Grounded pos(1,1) sprite(200,20) walk#1 x"
    }
    return $at
}
Later "echo '1 frame: 1 Grounded pos(1,1) sprite(160,20) idle#1 x' >> $t"
$start = Get-NowMs
$rest = Wait-ForRest $t
$took = (Get-NowMs) - $start
Write-Output $(if ($rest) { $rest.X } else { "null" })
Write-Output $took`);
  assert.strictEqual(r.code, 0, r.err);
  assert.strictEqual(r.lines[0], "160", `re-read after rest: ${r.lines[0]} ${r.err}`);
  assert.ok(Number(r.lines[1]) < 2000, `took ${r.lines[1]} ms`);
});

test("Windows: Wait-ForRest gives up 3 s after the last move", { skip: noPwsh }, (t) => {
  const r = ps(t, `$t = "$W/p.log"
Add-Content $t "1 frame: 1 Grounded pos(1,1) sprite(10,20) walk#1 x"
Later "echo '1 frame: 1 Grounded pos(1,1) sprite(20,20) walk#1 x' >> $t"
$start = Get-NowMs
$rest = Wait-ForRest $t
$took = (Get-NowMs) - $start
Write-Output ([bool]$rest)
Write-Output $took`);
  assert.strictEqual(r.code, 0, r.err);
  assert.strictEqual(r.lines[0], "False", r.err);
  assert.ok(Number(r.lines[1]) >= 2800 && Number(r.lines[1]) < 4500, `took ${r.lines[1]} ms`);
});

test("Windows: Wait-ForRest gives up at 10 s if the sprite never stops", { skip: noPwsh }, (t) => {
  const r = ps(t, `$t = "$W/p.log"
Set-Content "$W/walk.sh" @'
i=0; while :; do i=$((i + 1)); echo "1 frame: 1 Grounded pos(1,1) sprite($((i * 10)),20) walk#1 x" >> "$1"; sleep 0.25; done
'@
$p = New-Object Diagnostics.ProcessStartInfo "bash"
$p.ArgumentList.Add("$W/walk.sh")
$p.ArgumentList.Add($t)
$p.UseShellExecute = $false
$w = [Diagnostics.Process]::Start($p)
$start = Get-NowMs
$rest = Wait-ForRest $t
$took = (Get-NowMs) - $start
if (-not $w.HasExited) { $w.Kill() }
Write-Output $(if ($rest) { $rest.X } else { "null" })
Write-Output $took`);
  assert.strictEqual(r.code, 0, r.err);
  assert.match(r.err, /sprite never came to rest within 10s/);
  assert.strictEqual(r.lines[0], "null", r.err);
  assert.ok(Number(r.lines[1]) >= 9000 && Number(r.lines[1]) < 13000, `took ${r.lines[1]} ms`);
});

test("Windows: Invoke-AxWin splats a hashtable of ax-window-win.ps1 parameters", { skip: noPwsh }, (t) => {
  // A stub stands in for ax-window-win.ps1, so this checks the binding only.
  const r = ps(t, `$stub = "$W/ax.ps1"
Set-Content $stub 'param([Parameter(Position = 0)]$Command, [int]$ProcessId, $Title, $Arg3) "$Command $ProcessId $Title $Arg3"; exit 0'
$script:AxScript = $stub
Invoke-AxWin "$W/o.txt" @{ Command = "dump"; ProcessId = 42; Title = "BMO"; Arg3 = "frames" }
(Get-Content "$W/o.txt").Trim()`);
  assert.deepStrictEqual(r.lines.slice(0, 2), ["True", "dump 42 BMO frames"], r.err);
});
