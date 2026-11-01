// Bookkeeping and anchor arithmetic can be wrong without a display. What a
// cue looks like and what it sounds like are keyframes and oscillators, and
// are not tested here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { createCueMachine, cueAnchor, cueIo, POKE_WINDOW_MS } from "../src/cue.js";

// The io a cue machine draws and sounds through, recording what it was asked
// for and handing back the stops the machine is expected to call.
function machineHarness() {
  const calls = [];
  const timers = new Map();
  let now = 0;
  let nextId = 1;

  const machine = createCueMachine({
    draw(name) {
      calls.push(`draw:${name}`);
      return () => calls.push(`undraw:${name}`);
    },
    sound(name) {
      calls.push(`sound:${name}`);
      return () => calls.push(`cut:${name}`);
    },
    schedule(fn, ms) {
      const id = nextId++;
      timers.set(id, { fn, at: now + ms });
      return id;
    },
    cancel(id) {
      timers.delete(id);
    },
  });

  const advance = (ms) => {
    now += ms;
    for (const [id, timer] of [...timers.entries()].sort((a, b) => a[1].at - b[1].at)) {
      if (timer.at <= now) {
        timers.delete(id);
        timer.fn();
      }
    }
  };

  const placement = (cue, overrides) => ({ cue, visible: true, sound: true, ...overrides });
  return { machine, calls, advance, placement };
}

test("each cue is drawn and sounded once, on the tick that carries it", () => {
  for (const name of ["poke", "summon", "menu", "pickup", "drop", "throw", "possess"]) {
    // A machine each, so the Poke the previous cue left in flight is not the
    // thing the next one is measured against. The cancel has its own test.
    const { machine, calls, placement } = machineHarness();

    machine.event(placement(name));
    assert.deepEqual(calls, [`draw:${name}`, `sound:${name}`], name);

    calls.length = 0;
    machine.event(placement(null));
    assert.deepEqual(calls, [], `${name} rides one tick only`);
  }
});

// A double-click is two releases. The first emits a Poke before anything
// can know a second is coming, so the Poke cue is always part-played when the
// Summon lands.
test("a Summon cancels a Poke still in flight, visual and sound both", () => {
  const { machine, calls, placement } = machineHarness();

  machine.event(placement("poke"));
  calls.length = 0;
  machine.event(placement("summon"));

  assert.deepEqual(calls, ["undraw:poke", "cut:poke", "draw:summon", "sound:summon"]);
});

test("a Summon on its own cancels nothing", () => {
  const { machine, calls, placement } = machineHarness();

  machine.event(placement("summon"));
  assert.deepEqual(calls, ["draw:summon", "sound:summon"]);
});

test("a Poke past the double-click window is no longer in flight", () => {
  const { machine, calls, placement, advance } = machineHarness();

  machine.event(placement("poke"));
  advance(POKE_WINDOW_MS);
  calls.length = 0;
  machine.event(placement("summon"));

  assert.deepEqual(calls, ["draw:summon", "sound:summon"], "an unrelated double-click");
});

// The record holds an element and a gain node, so the newest Poke has to
// replace the older one rather than joining it.
test("only the newest Poke is the one a Summon cancels", () => {
  const { machine, calls, placement, advance } = machineHarness();

  machine.event(placement("poke"));
  advance(POKE_WINDOW_MS / 2);
  machine.event(placement("poke"));
  calls.length = 0;

  // The first Poke's forget timer would have fired by now had it survived, and
  // taken the second Poke's record with it.
  advance(POKE_WINDOW_MS / 2);
  machine.event(placement("summon"));
  assert.deepEqual(calls, ["undraw:poke", "cut:poke", "draw:summon", "sound:summon"]);
});

// Do Not Disturb is quiet, not gone: a visual cue cannot embarrass anyone in a
// meeting, and it is folded into this one flag.
test("sound gates the audio only, and the visual still plays", () => {
  const { machine, calls, placement } = machineHarness();

  machine.event(placement("drop", { sound: false }));
  assert.deepEqual(calls, ["draw:drop"]);

  // And a silent Poke is still cancellable, or a muted double-click would show
  // two cues where an unmuted one shows one.
  calls.length = 0;
  machine.event(placement("poke", { sound: false }));
  machine.event(placement("summon", { sound: false }));
  assert.deepEqual(calls, ["draw:poke", "undraw:poke", "draw:summon"]);
});

test("a sound that throws still leaves the visual", () => {
  const calls = [];
  const machine = createCueMachine({
    draw(name) {
      calls.push(`draw:${name}`);
      return () => calls.push(`undraw:${name}`);
    },
    sound() {
      throw new Error("no AudioContext");
    },
  });

  machine.event({ cue: "poke", visible: true, sound: true });
  assert.deepEqual(calls, ["draw:poke"]);
});

test("a hidden Character produces no cue of either kind", () => {
  const { machine, calls, placement } = machineHarness();

  machine.event(placement("menu", { visible: false }));
  assert.deepEqual(calls, []);
});

test("the click cues anchor to the sprite's centre, the drag cues to its feet, a landing to where it touched", () => {
  const anchor = cueAnchor({ x: 100, y: 200, width: 126, height: 128, ripple_x: 0, ripple_y: 300, ripple_normal: { x: 1, y: 0 } });

  assert.deepEqual(anchor, {
    x: 163,
    y: 264,
    feetX: 163,
    feetY: 328,
    ripple_x: 0,
    ripple_y: 300,
    ripple_normal: { x: 1, y: 0 },
  });
});

// The frame listener draws through cueIo. A name with no visual throws there
// and takes the rest of the frame with it.
test("a possess cue draws a ring instead of throwing", () => {
  const nodes = [];
  const previous = globalThis.document;
  globalThis.document = {
    createElement() {
      const node = {
        style: {},
        className: "",
        setAttribute(_name, value) {
          node.className = value;
        },
        append() {},
        addEventListener() {},
        remove() {},
      };
      return node;
    },
  };
  try {
    const io = cueIo(
      {
        append(...added) {
          nodes.push(...added);
        },
      },
      () => ({ x: 10, y: 20, feetX: 10, feetY: 40 }),
    );
    const stop = io.draw("possess");
    assert.equal(nodes.length, 1);
    assert.match(nodes[0].className, /\bpossess\b/);
    stop();
  } finally {
    if (previous === undefined) delete globalThis.document;
    else globalThis.document = previous;
  }
});

test("an odd sprite width still centres the cue between its edges", () => {
  const anchor = cueAnchor({ x: 0, y: 0, width: 45, height: 45 });

  assert.equal(anchor.x, 22.5, "the element is translated by -50%, so no rounding is owed here");
  assert.equal(anchor.feetY, 45, "and the feet are the bottom edge, not the middle");
});
