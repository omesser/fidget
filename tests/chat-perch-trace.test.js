import { test } from "node:test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { tempDir } from "./temp-path.js";

const script = fileURLToPath(new URL("../scripts/scenarios/manual-only/chat-perch.sh", import.meta.url));

function check(t, trace, firstLine = 2, instance = "character-a") {
  const log = join(tempDir(t, "fidget-perch-trace-"), "app.log");
  writeFileSync(log, trace);
  return spawnSync("bash", [script, "--check-click", log, String(firstLine), instance], {
    encoding: "utf8",
  });
}

function checkMainFit(screen, chat) {
  return spawnSync("bash", [script, "--main-fit", screen, chat], {
    encoding: "utf8",
  });
}

function checkHitTest(t, trace, baseline, x = 420, y = 160, state = "on") {
  const log = join(tempDir(t, "fidget-perch-hit-test-"), "app.log");
  writeFileSync(log, trace);
  return spawnSync("bash", [script, "--check-hit-test", log, String(baseline), String(x), String(y), state], {
    encoding: "utf8",
  });
}

test("Chat opened on another display gets a fitting main-display target", () => {
  const result = checkMainFit("0,0,1920,1080", "2200,100,500,700");
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(result.stdout.trim(), "1420,100");
});

test("Chat already fitting on the main display keeps its position", () => {
  const result = checkMainFit("0,0,1920,1080", "700,200,500,700");
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(result.stdout.trim(), "700,200");
});

test("Chat too large for the main display has no fitting target", () => {
  const result = checkMainFit("0,0,400,300", "700,200,500,700");
  assert.equal(result.status, 1, result.stderr || result.stdout);
});

test("hit-test accepts the expected point and state on any overlay", (t) => {
  const result = checkHitTest(t,
    "hit-test: cursor(420,160) scale 1 -> point(420,160) on overlay 0 miss click-through on\n" +
      "hit-test: cursor(420,160) scale 1 -> point(420,160) on overlay 7 miss click-through on\n",
    1,
  );
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

test("hit-test rejects a line for another point", (t) => {
  const result = checkHitTest(t,
    "hit-test: cursor(420,160) scale 1 -> point(421,160) on overlay 7 miss click-through on\n",
    0,
  );
  assert.equal(result.status, 1, result.stderr || result.stdout);
});

test("hit-test rejects a line for another click-through state", (t) => {
  const result = checkHitTest(t,
    "hit-test: cursor(420,160) scale 1 -> point(420,160) on overlay 7 HIT click-through OFF\n",
    0,
  );
  assert.equal(result.status, 1, result.stderr || result.stdout);
});

test("hit-test does not accept a matching line from before the baseline", (t) => {
  const line = "hit-test: cursor(420,160) scale 1 -> point(420,160) on overlay 7 miss click-through on\n";
  const result = checkHitTest(t, line, 1);
  assert.equal(result.status, 1, result.stderr || result.stdout);
});

test("a new command trace for the clicked Character passes without a desktop", (t) => {
  const result = check(t, "overlay: ready\nbubble: open-chat invoked instance=character-a\n");
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

for (const [name, trace] of [
  ["focus alone", "overlay: ready\nfocused-window=42|BMO\n"],
  ["a command before the click", "bubble: open-chat invoked instance=character-a\noverlay: ready\n"],
  ["another Character", "overlay: ready\nbubble: open-chat invoked instance=character-b\n"],
  ["an instance with the same prefix", "overlay: ready\nbubble: open-chat invoked instance=character-a-extra\n"],
  ["an empty log", ""],
]) {
  test(`${name} cannot prove the bubble click`, (t) => {
    const result = check(t, trace);
    assert.equal(result.status, 1, result.stderr || result.stdout);
  });
}

test("an invalid baseline is refused", (t) => {
  assert.equal(check(t, "bubble: open-chat invoked instance=character-a\n", 0).status, 2);
});

test("without consent the scenario only prints its takeover header", () => {
  const result = spawnSync("bash", [script], { encoding: "utf8" });
  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stdout, /Scenario: chat-perch/);
  assert.match(result.stdout, /Input: synthetic mouse clicks and typing inside Fidget only/);
});
