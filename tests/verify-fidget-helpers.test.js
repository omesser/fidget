// The verify-fidget launch and cleanup helpers, run as the skill runs them,
// against a stand-in Fidget. Launch returns once the overlay line lands and
// fails within 3 s when it never does; cleanup force-kills only a TERM ignorer.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { tempDir } from "./temp-path.js";

const helpers = path.join(import.meta.dirname, "..", ".agents", "skills", "verify-fidget", "helpers");

function scratch(t, fake) {
  const root = tempDir(t, "verify-helpers-");
  const bin = path.join(root, "fidget");
  fs.writeFileSync(bin, `#!/usr/bin/env bash\n${fake}\n`, { mode: 0o755 });
  return { root, env: { ...process.env, FIDGET_VERIFY_ROOT: root, FIDGET_VERIFY_BIN: bin } };
}

function helper(name, env) {
  const started = Date.now();
  const r = spawnSync("bash", [path.join(helpers, name)], { encoding: "utf8", env });
  return { code: r.status, out: r.stdout + r.stderr, ms: Date.now() - started };
}

function pidOf(root) {
  return Number(fs.readFileSync(path.join(root, "scratch", "pids", "app.pid"), "utf8"));
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

test("launch is ready as soon as the overlay line is written", (t) => {
  const { root, env } = scratch(t, 'sleep 0.3; echo "overlay: overlay-0 covers 1x1 at (0,0)" >&2; exec sleep 30');
  const r = helper("launch.sh", env);
  const pid = pidOf(root);
  process.kill(pid, "SIGKILL");
  assert.equal(r.code, 0, r.out);
  assert.match(r.out, /ready \(overlay line\)/);
  assert.ok(r.ms < 1500, `took ${r.ms} ms`);
});

test("launch fails within 3 s when no overlay line ever comes", (t) => {
  const { root, env } = scratch(t, "exec sleep 30");
  const r = helper("launch.sh", env);
  process.kill(pidOf(root), "SIGKILL");
  assert.notEqual(r.code, 0, r.out);
  assert.ok(r.ms >= 2800 && r.ms < 4500, `took ${r.ms} ms`);
});

test("launch fails at once when Fidget exits during startup", (t) => {
  const { env } = scratch(t, "echo crashed >&2; exit 3");
  const r = helper("launch.sh", env);
  assert.equal(r.code, 1, r.out);
  assert.match(r.out, /process exited/);
  assert.match(r.out, /crashed/);
  assert.ok(r.ms < 1500, `took ${r.ms} ms`);
});

test("cleanup stops a Fidget that honours TERM without a force kill", (t) => {
  const { root, env } = scratch(t, 'echo "overlay: up" >&2; exec sleep 30');
  helper("launch.sh", env);
  const pid = pidOf(root);
  const r = helper("cleanup.sh", env);
  assert.equal(r.code, 0, r.out);
  assert.equal(alive(pid), false, "still running");
  assert.doesNotMatch(r.out, /kill -9/);
  assert.ok(r.ms < 1500, `took ${r.ms} ms`);
});

test("cleanup returns as soon as a Fidget that takes 0.5 s to stop has exited", (t) => {
  const { root, env } = scratch(t, 'trap \'sleep 0.5; kill "$s"; exit 0\' TERM; echo "overlay: up" >&2; sleep 30 & s=$!; wait "$s"');
  helper("launch.sh", env);
  const pid = pidOf(root);
  const r = helper("cleanup.sh", env);
  assert.equal(r.code, 0, r.out);
  assert.equal(alive(pid), false, "still running");
  assert.doesNotMatch(r.out, /kill -9/);
  assert.ok(r.ms >= 450 && r.ms < 1500, `took ${r.ms} ms`);
});

test("cleanup force-kills a Fidget that ignores TERM, after 3 s", (t) => {
  const { root, env } = scratch(t, 'trap "" TERM; echo "overlay: up" >&2; exec sleep 30');
  helper("launch.sh", env);
  const pid = pidOf(root);
  const r = helper("cleanup.sh", env);
  assert.equal(r.code, 0, r.out);
  assert.equal(alive(pid), false, "still running");
  assert.match(r.out, new RegExp(`kill -9 ${pid}`));
  assert.ok(r.ms >= 2800 && r.ms < 4500, `took ${r.ms} ms`);
});

test("cleanup stops Fidget and a child in its own session, promptly", (t) => {
  const { root, env } = scratch(t, 'setsid sleep 30 & echo $! > "$(dirname "$0")/child.pid"; echo "overlay: up" >&2; sleep 30 & wait $!');
  helper("launch.sh", env);
  const pid = pidOf(root);
  const child = Number(fs.readFileSync(path.join(root, "child.pid"), "utf8"));
  const r = helper("cleanup.sh", env);
  const left = alive(child);
  if (left) process.kill(child, "SIGKILL");
  assert.equal(r.code, 0, r.out);
  assert.equal(alive(pid), false, "Fidget still running");
  assert.equal(left, false, "the child was left running");
  assert.doesNotMatch(r.out, /kill -9/);
  assert.ok(r.ms < 1500, `took ${r.ms} ms`);
});

test("cleanup force-kills at once when it has no way to watch the exit", (t) => {
  const { root, env } = scratch(t, 'trap "" TERM; echo "overlay: up" >&2; exec sleep 30');
  helper("launch.sh", env);
  const pid = pidOf(root);
  fs.rmSync(path.join(root, "scratch", "pids", "fidget.lock"), { force: true });
  const r = helper("cleanup.sh", env);
  assert.equal(r.code, 0, r.out);
  assert.equal(alive(pid), false, "still running");
  assert.match(r.out, new RegExp(`kill -9 ${pid}`));
  // macOS still watches the pid through caffeinate, so only Linux is immediate.
  if (process.platform === "linux") assert.ok(r.ms < 1500, `took ${r.ms} ms`);
});

// Fidget exits on TERM but its child ignores TERM and keeps the lock.
// The child reports over a fifo once TERM is ignored, and Fidget is reaped
// right after the TERM, so its pid is gone before the lock wait starts.
test(
  "cleanup waits on the lock and force-kills a child that outlives Fidget",
  { skip: process.platform !== "linux" && "the lock wait needs flock" },
  (t) => {
    const { root, env } = scratch(t, "");
    const script = `
      . "${path.join(helpers, "cleanup.sh")}" > /dev/null # no pids yet: defines stop_pid
      set +e
      mkdir -p "$(dirname "$FIDGET_VERIFY_LOCK")" && mkfifo "${root}/ready"
      exec 9> "$FIDGET_VERIFY_LOCK"; flock -n 9 || exit 90
      bash -c '(trap "" TERM; echo "$BASHPID" > "$0"; exec sleep 30) & exec sleep 30' "${root}/ready" > /dev/null 2>&1 &
      fidget=$!
      exec 9>&-
      exec {r}<> "${root}/ready"; read -r -t 3 child <&"$r" || { pkill -9 -P "$fidget"; kill -9 "$fidget"; exit 91; }
      eval "real_$(declare -f wait_for_exit)"
      wait_for_exit() { wait "$fidget"; real_wait_for_exit "$@"; }
      start=$EPOCHREALTIME
      stop_pid "$fidget"
      echo "stop_pid took $(( (\${EPOCHREALTIME/./} - \${start/./}) / 1000 )) ms"
      if flock -n "$FIDGET_VERIFY_LOCK" true; then echo "lock free"; else echo "lock held"; kill -9 "$child"; fi
    `;
    const started = Date.now();
    const r = spawnSync("bash", ["-c", script], { encoding: "utf8", env, timeout: 10000 });
    const out = r.stdout + r.stderr;
    const ms = Date.now() - started;
    assert.equal(r.status, 0, out);
    assert.match(out, /lock free/, `the child that ignores TERM was left running\n${out}`);
    assert.match(out, /cleanup: kill -9 \d+/);
    assert.ok(ms >= 2800 && ms < 4500, `took ${ms} ms\n${out}`);
  },
);
