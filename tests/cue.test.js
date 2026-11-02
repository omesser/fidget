// Bookkeeping and anchor arithmetic can be wrong without a display. What a
// cue looks like and what it sounds like are keyframes and oscillators, and
// are not tested here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { createCueMachine, cueAnchor, cueIo, ghostPath, outlinePixels, POKE_WINDOW_MS } from "../src/cue.js";

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
  for (const name of ["poke", "summon", "menu", "pickup", "drop", "throw", "possess", "unpossess"]) {
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

test("an odd sprite width still centres the cue between its edges", () => {
  const anchor = cueAnchor({ x: 0, y: 0, width: 45, height: 45 });

  assert.equal(anchor.x, 22.5, "the element is translated by -50%, so no rounding is owed here");
  assert.equal(anchor.feetY, 45, "and the feet are the bottom edge, not the middle");
});

// A 20 px square body in a 40 px canvas, in supersampled pixels (two per
// display pixel). Each probe reads one row across the left edge.
test("the outline is a faint body, inked just inside the edge, then white and ink outside", () => {
  const W = 40;
  const alpha = new Uint8Array(W * W);
  for (let y = 10; y < 30; y++) for (let x = 10; x < 30; x++) alpha[y * W + x] = 255;
  const out = outlinePixels(alpha, W, W);
  const px = (x) => Array.from(out.slice((20 * W + x) * 4, (20 * W + x) * 4 + 4));

  assert.deepEqual(px(20), [255, 255, 255, 46], "the body is a faint white fill");
  assert.deepEqual(px(10), [0x24, 0x26, 0x2c, 229], "ink just inside the edge");
  assert.deepEqual(px(9), [255, 255, 255, 255], "white just outside it");
  assert.deepEqual(px(3), [0x24, 0x26, 0x2c, 153], "ink past the white");
  assert.deepEqual(px(0), [0, 0, 0, 0], "and nothing beyond");
});

test("the possess swoop mirrors off a right edge and flattens under a top edge", () => {
  const ghost = { gw: 76, gh: 76, stageW: 1000 };

  assert.deepEqual(ghostPath("possess", { ...ghost, cx: 500, cy: 500 }), { dir: 1, ky: 1 });
  assert.deepEqual(ghostPath("possess", { ...ghost, cx: 900, cy: 500 }), { dir: -1, ky: 1 }, "comes from the left");
  assert.deepEqual(
    ghostPath("possess", { ...ghost, cx: 100, cy: 500, stageW: 200 }),
    { dir: 1, ky: 1 },
    "neither side fits, so the usual side",
  );
  assert.deepEqual(ghostPath("possess", { ...ghost, cx: 500, cy: 141 }), { dir: 1, ky: 0.6 });
  assert.deepEqual(ghostPath("possess", { ...ghost, cx: 500, cy: 40 }), { dir: 1, ky: 0.4 }, "never below 0.4");
  assert.deepEqual(ghostPath("unpossess", { ...ghost, cx: 900, cy: 500 }), { dir: 1, ky: 1 }, "a rise never mirrors");
  assert.deepEqual(ghostPath("unpossess", { ...ghost, cx: 500, cy: 132 }), { dir: 1, ky: 0.6 });
});

// Enough DOM for cueIo's possession visuals: elements that record their
// classes and children, and a 2D context that counts the outline passes.
function fakeDom() {
  const passes = [];
  const element = (tag) => {
    const classes = new Set();
    const node = {
      tag,
      style: { setProperty(name, value) { node.style[name] = String(value); } },
      dataset: {},
      children: [],
      offsetWidth: 0,
      get className() { return [...classes].join(" "); },
      set className(value) { classes.clear(); value.split(" ").filter(Boolean).forEach((c) => classes.add(c)); },
      setAttribute(_name, value) { node.className = value; },
      classList: {
        add: (...c) => c.forEach((x) => classes.add(x)),
        remove: (...c) => c.forEach((x) => classes.delete(x)),
        contains: (c) => classes.has(c),
      },
      append(...added) { node.children.push(...added); },
      addEventListener() {},
      remove() {},
      getContext() {
        return {
          translate() {},
          scale() {},
          drawImage() {},
          putImageData() {},
          getImageData(_x, _y, w, h) {
            passes.push(w);
            return { data: new Uint8ClampedArray(w * h * 4) };
          },
        };
      },
    };
    return node;
  };
  return { passes, element, document: { createElement: element } };
}

function withDom(fn) {
  const previous = globalThis.document;
  const dom = fakeDom();
  globalThis.document = dom.document;
  try {
    return fn(dom);
  } finally {
    if (previous === undefined) delete globalThis.document;
    else globalThis.document = previous;
  }
}

function possessionIo(dom, src) {
  const layer = dom.element("div");
  const sprite = dom.element("img");
  sprite.className = "sprite";
  sprite.currentSrc = src;
  sprite.width = 64;
  sprite.height = 64;
  sprite.dataset.mirror = "-1";
  const io = cueIo(layer, () => ({ x: 400, y: 400, feetX: 400, feetY: 432 }), sprite);
  return { io, layer, sprite };
}

test("possess swoops in three outline ghosts and shakes the sprite", () => {
  withDom((dom) => {
    const { io, layer, sprite } = possessionIo(dom, "data:image/png;base64,possess");
    io.draw("possess");

    const [wrap] = layer.children;
    assert.deepEqual(
      wrap.children.map((g) => g.className),
      ["cue cue-oghost trail2", "cue cue-oghost trail", "cue cue-oghost lead"],
    );
    assert.equal(wrap.children[2].style.width, "76px", "the sprite plus a 6 px pad each side");
    assert.ok(sprite.classList.contains("cue-possess"));

    io.draw("unpossess");
    assert.deepEqual(layer.children[1].children.map((g) => g.className), ["cue cue-oghost rise"]);
    assert.ok(sprite.classList.contains("cue-release"));
    assert.ok(!sprite.classList.contains("cue-possess"), "one sprite cue at a time");
  });
});

test("an outline is drawn once per frame and size, and the cache keeps 64", () => {
  withDom((dom) => {
    const first = possessionIo(dom, "data:frame-0");
    first.io.draw("possess");
    first.io.draw("unpossess");
    assert.equal(dom.passes.length, 1, "the second cue on the same frame reuses it");

    for (let i = 1; i <= 64; i++) possessionIo(dom, `data:frame-${i}`).io.draw("possess");
    dom.passes.length = 0;
    possessionIo(dom, "data:frame-0").io.draw("possess");
    assert.equal(dom.passes.length, 1, "the oldest of 65 was evicted");
  });
});

test("reduced motion keeps the sprite cue and drops the ghost", () => {
  const previous = globalThis.matchMedia;
  globalThis.matchMedia = () => ({ matches: true });
  try {
    withDom((dom) => {
      const { io, layer, sprite } = possessionIo(dom, "data:reduced");
      io.draw("possess");
      assert.deepEqual(layer.children, []);
      assert.ok(sprite.classList.contains("cue-possess"));
    });
  } finally {
    if (previous === undefined) delete globalThis.matchMedia;
    else globalThis.matchMedia = previous;
  }
});

// A fake AudioContext that records which oscillators end up driving another
// oscillator's pitch.
test("the possession sounds are two detuned voices, each with a vibrato", () => {
  const vibratos = [];
  const param = (owner) => ({ owner, value: 0, setValueAtTime() {}, linearRampToValueAtTime() {}, exponentialRampToValueAtTime() {}, cancelScheduledValues() {} });
  const node = () => ({ connect(to) { this.to = to; } });
  class FakeAudio {
    currentTime = 0;
    state = "running";
    destination = {};
    createGain() {
      const g = node();
      g.gain = param(g);
      g.connect = (to) => {
        if (to.owner?.kind === "osc" && g.from?.kind === "osc") vibratos.push(g.from.frequency.value);
      };
      return g;
    }
    createOscillator() {
      const o = { kind: "osc", start() {}, stop() {} };
      o.frequency = param(o);
      o.connect = (to) => (to.from = o);
      return o;
    }
    createBiquadFilter() {
      const f = node();
      f.frequency = param(f);
      return f;
    }
  }
  const previous = globalThis.window;
  globalThis.window = { AudioContext: FakeAudio };
  try {
    const io = cueIo(null, () => ({}));
    io.sound("possess");
    assert.deepEqual(vibratos, [7, 7.6], "possess");
    vibratos.length = 0;
    io.sound("unpossess");
    assert.deepEqual(vibratos, [7, 7.6], "unpossess");
  } finally {
    if (previous === undefined) delete globalThis.window;
    else globalThis.window = previous;
  }
});
