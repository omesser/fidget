// The page character on the landing page. update() is its whole behavior, so
// these drive it with explicit times and rolls, the way the page's frame loop does.

import assert from "node:assert/strict";
import { test } from "node:test";
import { feet } from "../website/playground.js";
import { animation, start, update } from "../website/walker.js";

const TUNING = {
  gravity: 3600, walkSpeed: 120, primitiveMs: 600, pokeCooldownMs: 2500, perchedIdleAfterMs: 4000,
  rideAcceleration: 10000, yankWindowS: 0.1, throwMinSpeed: 80, velocityWindowMs: 60, doubleClickMs: 400,
};
const ENV = {
  world: { width: 1200, height: 800, sprite: { w: 90, h: 90 }, windows: [] },
  tuning: TUNING,
  lines: ["Beep.", "Hello there."],
  sayMs: () => 3000,
  margin: () => 0,
};

function run(state, events, env = ENV) {
  const cues = [];
  const handoffs = [];
  for (const event of events) {
    const out = update(state, event, env);
    state = out.state;
    if (out.cue) cues.push(out.cue);
    if (out.handoff) handoffs.push(out.handoff);
  }
  return { state, cues, handoffs };
}

const ticks = (from, count, dt = 0.05, rand = 0.9) =>
  Array.from({ length: count }, (_, i) => ({ type: "tick", now: from + (i + 1) * dt * 1000, dt, rand }));
const view = (play, blocked = false) => ({ type: "view", play, blocked });
const fresh = (options = {}) => start({ now: 0, rolls: [0.5, 0.5], width: 1200, ...options });
const onPage = () => run(fresh(), [view(false), ...ticks(0, 30)]).state;

test("it enters in the margin beside the content column, on a side the visit rolls, facing the page", () => {
  const wide = { ...ENV, world: { ...ENV.world, width: 1512 }, margin: () => 228 };
  const enter = (side, env) => run(start({ now: 0, rolls: [side, 0.5], width: env.world.width }), [view(false), ...ticks(0, 30)], env).state;
  const left = enter(0.2, wide);
  const right = enter(0.8, wide);
  assert.deepEqual([feet(left.sprite, wide.world), left.sprite.facing], [{ x: 114, y: 800 }, 1]);
  assert.deepEqual([feet(right.sprite, wide.world).x, right.sprite.facing], [1398, -1]);
  const phone = { ...ENV, world: { ...ENV.world, width: 390 }, margin: () => 16 };
  assert.equal(feet(enter(0.2, phone).sprite, phone.world).x, 45, "with no margin to fit in, it enters against the edge");
});

test("the side it first enters on and when it first speaks are separate rolls", () => {
  const soon = start({ now: 0, rolls: [0.2, 0.1], width: 1200 });
  const late = start({ now: 0, rolls: [0.2, 0.9], width: 1200 });
  assert.deepEqual([soon.sprite, late.sprite], [{ mode: "away", x: 0, facing: 1 }, { mode: "away", x: 0, facing: 1 }]);
  assert.deepEqual([soon.nextLine, late.nextLine], [6900, 14100], "the first line comes 6 to 15 s in, by its own roll");
});

test("every return to the page walks up in a margin, on the side nearer where it left", () => {
  const env = { ...ENV, world: { ...ENV.world, width: 1512 }, margin: () => 228 };
  const leftMidPage = { ...start({ now: 0, rolls: [0.2, 0.5], width: 1512 }),
    phase: "lent", sprite: { mode: "away", x: 900, facing: -1 } };
  const back = run(leftMidPage, [view(false), ...ticks(0, 30)], env).state;
  assert.deepEqual([feet(back.sprite, env.world).x, back.sprite.facing], [1398, -1]);
  const leftSide = run({ ...leftMidPage, sprite: { mode: "away", x: 500, facing: 1 } }, [view(false), ...ticks(0, 30)], env).state;
  assert.deepEqual([feet(leftSide.sprite, env.world).x, leftSide.sprite.facing], [114, 1]);
});

test("the playground keeps the character until the page knows where it is scrolled", () => {
  const waiting = run(fresh(), ticks(0, 30));
  assert.deepEqual([waiting.state.phase, waiting.handoffs], ["lent", []]);
});

test("with the playground out of sight, the page takes the character back and it walks up onto the page", () => {
  const back = run(fresh(), [view(false), ...ticks(0, 1)]);
  assert.deepEqual([back.state.phase, back.handoffs, animation(back.state, 50)], ["page", ["depart"], "walk"]);
  const up = run(back.state, ticks(50, 30));
  assert.equal(feet(up.state.sprite, ENV.world).y, 800, "it stands on the bottom edge");
});

test("the playground coming into view walks the character off the bottom edge, then lends it to the playground", () => {
  const leaving = run(onPage(), [view(true), ...ticks(1500, 5)]);
  assert.deepEqual([leaving.state.phase, leaving.handoffs], ["page", []], "the playground waits for it to leave");
  const gone = run(leaving.state, ticks(1750, 30));
  assert.deepEqual([gone.state.phase, gone.handoffs], ["lent", ["arrive"]]);
  const returned = run(gone.state, [view(false), ...ticks(3500, 1)]);
  assert.deepEqual([returned.state.phase, returned.handoffs], ["page", ["depart"]]);
});

test("a view that flips back before the character is gone turns it around with no handoff", () => {
  const bounced = run(onPage(), [view(true), ...ticks(1500, 3), view(false), ...ticks(1650, 30)]);
  assert.deepEqual([bounced.state.phase, bounced.handoffs, feet(bounced.state.sprite, ENV.world).y], ["page", [], 800]);
});

test("the hero's stage on screen sends the character off without lending it, and it returns once the stage scrolls away", () => {
  const off = run(onPage(), [view(false, true), ...ticks(1500, 30)]);
  assert.deepEqual([off.state.phase, off.handoffs], ["away", []]);
  const lent = run(off.state, [view(true, true), ...ticks(3000, 1)]);
  assert.deepEqual([lent.state.phase, lent.handoffs], ["lent", ["arrive"]], "the playground still gets it when it shows");
  const back = run(off.state, [view(false, false), ...ticks(3000, 30)]);
  assert.deepEqual([back.state.phase, feet(back.state.sprite, ENV.world).y], ["page", 800]);
});

test("hiding lends the character to the playground and stays put; showing takes it back", () => {
  const hidden = run(onPage(), [{ type: "hide" }, ...ticks(1500, 30)]);
  assert.deepEqual([hidden.state.phase, hidden.handoffs, hidden.state.sprite.mode], ["hidden", ["arrive"], "away"]);
  const ignored = run(hidden.state, [{ type: "poke", now: 3000 }, view(false)]);
  assert.deepEqual([ignored.state.phase, ignored.cues], ["hidden", []]);
  const shown = run(hidden.state, [{ type: "show" }, view(false), ...ticks(3000, 1)]);
  assert.deepEqual([shown.state.phase, shown.handoffs], ["page", ["depart"]]);
  assert.equal(fresh({ hidden: true }).phase, "hidden");
});

test("now and then it says a line from its personality, standing still with its talk loop", () => {
  const page = onPage();
  const quiet = run(page, ticks(1500, 1)).state;
  assert.equal(quiet.dialogue, null, "no line before the first one is due");
  const due = quiet.nextLine + 1;
  const talking = run(quiet, [{ type: "tick", now: due, dt: 0.05, rand: 0.6 }]).state;
  assert.equal(talking.dialogue.line, "Hello there.");
  assert.equal(animation(talking, due + 100), "talk");
  const held = run(talking, ticks(due, 20)).state;
  assert.equal(held.sprite.x, talking.sprite.x, "it stays put while it talks");
  const done = run(held, [{ type: "tick", now: due + 3001, dt: 0.05, rand: 0.9 }]).state;
  assert.equal(done.dialogue, null);
  assert.ok(done.nextLine - due >= 60_000 && done.nextLine - due <= 123_001, `the next line is a minute or two away: ${done.nextLine - due}`);
});

test("a poke plays react with the poke ring and says a line, and a double click never summons", () => {
  const poked = run(onPage(), [{ type: "poke", now: 2000 }, { type: "poke", now: 2200 }]);
  assert.deepEqual(poked.cues, ["poke", "poke"]);
  assert.equal(animation(poked.state, 2300), "react");
  assert.ok(ENV.lines.includes(poked.state.dialogue.line));
});

test("a scroll startles it awake, and a throw lands it on the bottom edge", () => {
  const asleep = run(onPage(), ticks(1500, 1, 30)).state;
  const napping = run(asleep, ticks(31_500, 1, 30)).state;
  assert.equal(animation(napping, 61_500), "sleep");
  const startled = run(napping, [{ type: "startle", now: 61_600 }]).state;
  assert.equal(animation(startled, 61_700), "react");
  const thrown = run(onPage(), [
    { type: "grab", now: 2000 }, { type: "drag", x: 640, y: 700, now: 2020 }, { type: "drag", x: 700, y: 600, now: 2040 },
    { type: "release", now: 2040 }, ...ticks(2040, 60, 0.016)]);
  assert.deepEqual([thrown.cues, thrown.state.sprite.on, feet(thrown.state.sprite, ENV.world).y], [["pickup", "throw"], "floor", 800]);
});

test("under reduced motion it stays where it is: no walking on, no wandering, and it still naps and talks on a poke", () => {
  const still = run(fresh({ still: true }), [view(false), ...ticks(0, 1)]);
  assert.equal(animation(still.state, 50), "idle", "it is on the page at once");
  const later = run(still.state, ticks(50, 100, 0.05, 0.9)).state;
  assert.equal(later.sprite.x, still.state.sprite.x);
  const poked = run(later, [{ type: "poke", now: 6000 }]).state;
  assert.ok(poked.dialogue);
});
