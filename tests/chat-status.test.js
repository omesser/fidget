// The status bar's arithmetic: what each cell says for one push, and the
// countdown the window runs between pushes. Plus the header's one line about
// which mind answers, which the same module writes.

import assert from "node:assert/strict";
import { test } from "node:test";

import { mindLine, plainStatus, statusCells, untilWake } from "../src/chat-status.js";

// One push, as the Shell serializes it.
const push = {
  behavior: "prowl",
  primitive: "Walk",
  animation: "walk",
  state: "Grounded",
  happened: "poked",
  facing: 1,
  thinking: false,
};

test("a push fills every cell", () => {
  const cells = statusCells(push, 12_000);

  assert.equal(cells.behavior, "prowl");
  assert.equal(cells.primitive, "Walk");
  assert.equal(cells.animation, "walk");
  assert.equal(cells.state, "Grounded");
  assert.equal(cells.happened, "poked");
  assert.equal(cells.facing, "→");
  assert.equal(cells.director, "wake 12s");
});

test("the Engine's own moments draw a dash, not a blank", () => {
  const cells = statusCells({ ...push, behavior: null, primitive: null }, 1000);

  assert.equal(cells.behavior, "—");
  assert.equal(cells.primitive, "—");
});

test("facing left mirrors the arrow", () => {
  assert.equal(statusCells({ ...push, facing: -1 }, 1000).facing, "←");
});

test("a turn on the wire displaces the countdown it would reset anyway", () => {
  assert.equal(statusCells({ ...push, thinking: true }, 5000).director, "thinking");
});

test("no wake coming reads as a dash rather than a number", () => {
  assert.equal(statusCells(push, null).director, "wake —");
});

test("before the first push every cell says so", () => {
  const cells = statusCells(null, null);

  assert.equal(cells.behavior, "—");
  assert.equal(cells.state, "—");
  assert.equal(cells.director, "wake —");
  assert.equal(cells.facing, "");
});

test("the countdown rounds up, and stops at due", () => {
  assert.equal(untilWake(11_400), "12s");
  assert.equal(untilWake(1), "1s");
  assert.equal(untilWake(0), "due");
  // The window keeps counting between pushes, so a deadline goes negative.
  assert.equal(untilWake(-3000), "due");
});

test("the countdown changes unit rather than growing", () => {
  assert.equal(untilWake(59_000), "59s");
  assert.equal(untilWake(60_000), "1m");
  assert.equal(untilWake(119_000), "2m");
  // `Pace::CAP`, the longest wait the ambient arm ever counts down to.
  assert.equal(untilWake(7_200_000), "2h");
});

// The header's line about which mind answers. Branch for branch with
// `settings::harness_state`, so Settings and Chat cannot disagree about it.
const http = { enabled: true, model: "gpt-4o-mini", host: "localhost:8000", harness: null };

test("with no Harness the header names the model and the host", () => {
  assert.equal(mindLine(http), "gpt-4o-mini · localhost:8000");
});

test("an attached Harness is named with the session that proves it is live", () => {
  const opening = {
    ...http,
    harness: {
      name: "hermes",
      session: "8cecc6dc-497f-4899-a827-a4e42fbdc1f2",
      alive: true,
      login: null,
    },
  };

  // The head of the id: a whole UUID leaves the header no room for the name
  // this window belongs to, and the session is here as proof rather than as a
  // value anyone reads back.
  assert.equal(mindLine(opening), "hermes · session 8cecc6dc");
});

// Set and not answering must not read like an attachment that came up.
test("a Harness that never came up says so rather than claiming the turn", () => {
  const opening = {
    ...http,
    harness: { name: "hermes", session: null, alive: false, login: null },
  };

  assert.equal(mindLine(opening), "hermes · not running");
});

test("a launcher that died at startup says it failed, not only that it is down", () => {
  const opening = {
    ...http,
    harness: { name: "codex", session: null, alive: false, login: null, failed: { command: "npx", reason: "exited before initialize", output: "", node_check: null } },
  };

  assert.equal(mindLine(opening), "codex · failed to start");
});

// #949: Apply starts the attach, so the second the handshake takes is a wait
// and not a verdict. `not running` there is the line that read as broken.
test("a Harness still shaking hands says it is starting, not that it is down", () => {
  const opening = {
    ...http,
    harness: {
      name: "hermes",
      session: null,
      alive: false,
      login: null,
      initializing: true,
    },
  };

  assert.equal(mindLine(opening), "hermes · starting");
});

// #726: a missing launcher is not a child that died. Settings already names
// the binary (`harness_state`); the header used to say only `not running`.
test("a missing launcher names the binary rather than a dead child", () => {
  for (const name of ["claude", "codex", "pi"]) {
    const opening = {
      ...http,
      harness: {
        name,
        session: null,
        alive: false,
        login: null,
        missing: "npx",
      },
    };
    assert.equal(
      mindLine(opening),
      `${name} · \`npx\` is not installed`,
      `${name} is an npx adapter, so the header has to name npx`,
    );
  }
});

test("a missing first-party CLI names that binary, not npx", () => {
  const opening = {
    ...http,
    harness: {
      name: "cursor-agent",
      session: null,
      alive: false,
      login: null,
      missing: "cursor-agent",
    },
  };
  assert.equal(
    mindLine(opening),
    "cursor-agent · `cursor-agent` is not installed",
  );
});

test("not signed in outranks the session, and names the login command", () => {
  const opening = {
    ...http,
    harness: { name: "hermes", session: "655092d4", alive: true, login: "hermes login" },
  };

  assert.equal(mindLine(opening), "hermes · not signed in — `hermes login`");
});

test("attached before a session opens claims no session", () => {
  const opening = {
    ...http,
    harness: { name: "hermes", session: null, alive: true, login: null },
  };

  assert.equal(mindLine(opening), "hermes · no session yet");
});

test("switched off there is no mind to name", () => {
  assert.equal(mindLine({ ...http, enabled: false }), "static weights");
  assert.equal(
    mindLine({
      ...http,
      enabled: false,
      harness: { name: "hermes", session: "655092d4", alive: true, login: null },
    }),
    "static weights",
  );
});

test("the header says nothing before the first opening arrives", () => {
  assert.equal(mindLine(null), "");
});

// The Shell sends empty strings only when it could not read its own inspect.
test("an endpoint the Shell could not read draws nothing, not a separator", () => {
  assert.equal(mindLine({ ...http, model: "", host: "" }), "");
});

// Plain-language status that connects to the visual character's behavior and
// actions, not only chat state.
test("plain status says Thinking when a turn is on the wire", () => {
  assert.equal(plainStatus({ ...push, thinking: true }, 12_000), "Thinking…");
});

test("plain status reflects behavior as human-readable activity", () => {
  assert.equal(
    plainStatus({ ...push, behavior: "prowl", happened: null }, 24_000),
    "Prowl · next thought in 24s",
  );
  assert.equal(plainStatus({ ...push, behavior: "nap", happened: null }, 60_000), "Nap · next thought in 1m");
  assert.equal(plainStatus({ ...push, behavior: "supervise", happened: null }, 0), "Supervise");
});

test("plain status falls back to animation when behavior is null", () => {
  assert.equal(
    plainStatus({ ...push, behavior: null, animation: "walk", happened: null }, 30_000),
    "Walk · next thought in 30s",
  );
  assert.equal(
    plainStatus({ ...push, behavior: null, animation: "fall", happened: null }, 5000),
    "Fall · next thought in 5s",
  );
  assert.equal(plainStatus({ ...push, behavior: null, animation: "sit", happened: null }, null), "Sit");
});

test("plain status falls back to primitive when behavior and animation are null", () => {
  assert.equal(
    plainStatus({ ...push, behavior: null, animation: null, primitive: "Sleep", happened: null }, 120_000),
    "Sleep · next thought in 2m",
  );
  assert.equal(
    plainStatus({ ...push, behavior: null, animation: null, primitive: "Walk", happened: null }, 0),
    "Walk",
  );
});

test("plain status appends happened cue when present", () => {
  assert.equal(plainStatus({ ...push, behavior: "sit", happened: "poked" }, 30_000), "Sit · just poked");
  assert.equal(
    plainStatus({ ...push, behavior: "prowl", happened: "spoken to" }, 60_000),
    "Prowl · just spoken to",
  );
  assert.equal(plainStatus({ ...push, behavior: "greet", happened: "summoned" }, null), "Greet · just summoned");
});

test("plain status omits wake countdown when happened cue is present", () => {
  assert.equal(plainStatus({ ...push, behavior: "walk", happened: "thrown" }, 24_000), "Walk · just thrown");
  assert.equal(plainStatus({ ...push, behavior: "inspect", happened: "poked" }, 90_000), "Inspect · just poked");
});

test("plain status humanizes unknown behaviors by cleaning the name", () => {
  assert.equal(
    plainStatus({ ...push, behavior: "custom_behavior", happened: null }, 30_000),
    "Custom behavior · next thought in 30s",
  );
  assert.equal(plainStatus({ ...push, behavior: "test-name", happened: null }, 0), "Test name");
});

test("plain status cleans behavior names generically", () => {
  assert.equal(
    plainStatus({ ...push, behavior: "fidget", happened: null }, 15_000),
    "Fidget · next thought in 15s",
  );
  assert.equal(
    plainStatus({ ...push, behavior: "meditate", happened: null }, 120_000),
    "Meditate · next thought in 2m",
  );
  assert.equal(
    plainStatus({ ...push, behavior: "power_down", happened: null }, 60_000),
    "Power down · next thought in 1m",
  );
});

test("plain status says Idle when behavior/animation/primitive are all null", () => {
  assert.equal(
    plainStatus({ ...push, behavior: null, animation: null, primitive: null, happened: null }, 24_000),
    "Idle · next thought in 24s",
  );
  assert.equal(
    plainStatus({ ...push, behavior: null, animation: null, primitive: null, happened: null }, 0),
    "Idle",
  );
  assert.equal(
    plainStatus({ ...push, behavior: null, animation: null, primitive: "—", happened: null }, 30_000),
    "Idle · next thought in 30s",
  );
});

test("plain status says Starting up before the first push", () => {
  assert.equal(plainStatus(null, null), "Starting up…");
});
