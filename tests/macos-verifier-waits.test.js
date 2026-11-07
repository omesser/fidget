import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

const scripts = path.join(import.meta.dirname, "..", "scripts");
const contracts = [
  ["verify-overlay.sh", ["wait_for_up", "wait_for_trace", "wait_for_count", "wait_for_rest"]],
  ["verify-settings-webview-select-macos.sh", ["wait_for_up", "wait_for_trace", "wait_for_count"]],
  ["bench-wakeups-macos.sh", ["wait_for_up", "wait_for_trace", "wait_for_count"]],
];

for (const [entrypoint, helpers] of contracts) {
  test(`${entrypoint} waits for log events through the shared helpers`, () => {
    const source = readFileSync(path.join(scripts, entrypoint), "utf8");

    const executableSource = source.replace(/^\s*#.*$/gm, "");
    assert.match(
      executableSource,
      /^\s*(?:source|\.)\s+.*wait-helpers\.mac\.sh/m,
      "loads the shared macOS wait helpers",
    );
    for (const helper of helpers) {
      assert.match(executableSource, new RegExp(`\\b${helper}\\s`), `uses ${helper}`);
    }

    assert.doesNotMatch(executableSource, /\bawait\s*\(\s*\)\s*\{/, "does not define a local await poller");
    assert.doesNotMatch(executableSource, /time\.sleep\(0\.25\)/, "does not poll logs from Python");
    const shellSource = executableSource.replace(
      /<<-?\s*(['"]?)([A-Z_]+)\1[^\n]*\n[\s\S]*?\n\2/g,
      "",
    );
    const loopSource = [...shellSource.matchAll(/\b(?:for|while)\b[\s\S]*?\bdone\b/g)]
      .map(([loop]) => loop)
      .join("\n");
    assert.doesNotMatch(
      loopSource,
      /\b(?:grep|sleep|perl)\b/,
      "does not poll or sleep inside a shell loop",
    );
  });
}

test("bench-wakeups-macos.sh follows a release binary's process log", () => {
  const source = readFileSync(path.join(scripts, "bench-wakeups-macos.sh"), "utf8");
  const executableSource = source.replace(/^\s*#.*$/gm, "");

  assert.match(executableSource, /FIDGET_DATA_DIR=/, "isolates the run's data dir");
  assert.match(
    executableSource,
    /tail -n \+1 -F/,
    "follows process.log from the first line, not the last ten",
  );
  assert.match(
    executableSource,
    /process\.log[\s\S]{0,500}sub\(\/\^\[0-9\]\+ /,
    "strips the epoch prefix process.log puts on every line before the waits read it",
  );
  assert.doesNotMatch(
    executableSource,
    /"\$BINARY"[^\n]*>>?\s*"\$LOG"/,
    "does not wait on the terminal redirect a release binary leaves empty",
  );
});
