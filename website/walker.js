// The landing page's own Character, along the bottom of the viewport. It is
// the playground's Character too, handed over while the playground is in view.
// update() decides and touches no DOM, so node tests it; step() moves it.
import { animationOf, away, character, decodePresses, feet, frameOf, step } from "./playground.js";

// The page origin is shared with other Pages sites, hence the prefix.
const HIDDEN_KEY = "fidget.hidden";

// When it next says a line, in ms: the first soon enough that a reader sees
// one, then a minute or two apart.
const LINES = { first: [6000, 15000], gap: [60000, 120000] };

// When a section counts as taking over the page, and the ratios its observer
// reports at. Most of the screen counts for one taller than the screen, so that
// rule reports every 5%, as its share of the screen grows.
const mostOf = { most: (entry) => entry.intersectionRatio >= 0.6, threshold: [0, 0.6] };
const mostOfScreen = {
  most: (entry) => mostOf.most(entry) || entry.intersectionRect.height >= 0.6 * entry.rootBounds.height,
  threshold: Array.from({ length: 21 }, (_, i) => i / 20),
};

const span = ([low, high], roll) => low + (high - low) * roll;

// phase is who has the Character: page, lent (the playground), away (nobody,
// as the hero's stage shows its own) or hidden (the visitor hid it). view is
// null until the page knows whether the playground is in sight.
export function start({ now, rolls: [side, wait], width, hidden = false, still = false }) {
  return {
    phase: hidden ? "hidden" : "lent",
    sprite: away({ x: side < 0.5 ? 0 : width, facing: side < 0.5 ? 1 : -1 }),
    still,
    view: null,
    speech: null,
    nextLine: now + span(LINES.first, wait),
  };
}

// It walks up in the `margin` beside the content column, or against the edge
// when the margin is narrower than it is, facing the page: never over text.
function inMargin(sprite, left, width, margin, spriteW) {
  const inset = Math.max(margin, spriteW) / 2;
  return { ...sprite, x: left ? inset : width - inset, facing: left ? 1 : -1 };
}

const speaking = (state, now) => state.speech !== null && now < state.speech.until;
const talkable = (sprite) => sprite.mode === "standing" && (sprite.act === "idle" || sprite.act === "walk");

export function animation(state, now) {
  return speaking(state, now) && talkable(state.sprite) ? "talk" : animationOf(state.sprite);
}

function say(state, now, rand, env) {
  const line = env.lines[Math.floor(rand * env.lines.length)];
  const until = now + env.sayMs(line);
  return { ...state, speech: { line, until }, nextLine: until + span(LINES.gap, rand) };
}

// Each tick walks the Character toward where it is wanted. Off the page while the playground
// or the hero stage is in view, on it otherwise. The page hands it over only once it is out of
// sight.
function tick(state, event, env) {
  const { now, rand } = event;
  const move = (sprite, extra) => step(sprite, { now, still: state.still, ...extra }, env.world, env.tuning).state;
  let next = speaking(state, now) ? state : { ...state, speech: null };
  const hold = speaking(next, now) && talkable(next.sprite);
  const out = step(next.sprite, { ...event, still: state.still, hold }, env.world, env.tuning);
  next = { ...next, sprite: out.state };
  if (state.view === null || state.phase === "hidden") return { state: next, cue: out.cue, handoff: null };

  const wanted = state.view.play || state.view.blocked ? "leave" : "enter";
  let handoff = null;
  if (next.phase === "page") {
    next = { ...next, sprite: move(next.sprite, { type: wanted }) };
    if (next.sprite.mode === "away") {
      next = { ...next, phase: state.view.play ? "lent" : "away", speech: null };
      handoff = state.view.play ? "arrive" : null;
    }
  } else if (next.phase === "away" && state.view.play) {
    next = { ...next, phase: "lent" };
    handoff = "arrive";
  } else if (wanted === "enter") {
    const { width, sprite } = env.world;
    const from = next.sprite.mode === "away"
      ? inMargin(next.sprite, next.sprite.x < width / 2, width, env.margin(), sprite.w)
      : next.sprite;
    next = { ...next, phase: "page", sprite: move(from, { type: "enter" }) };
    handoff = state.phase === "lent" ? "depart" : null;
  }
  if (next.phase === "page" && !speaking(next, now) && now >= next.nextLine && talkable(next.sprite)) {
    next = say(next, now, rand, env);
  }
  return { state: next, cue: out.cue, handoff };
}

export function update(state, event, env) {
  const unchanged = { state, cue: null, handoff: null };
  if (event.type === "view") return { ...unchanged, state: { ...state, view: { play: event.play, blocked: event.blocked } } };
  if (event.type === "hide") {
    const lend = state.phase === "page" || state.phase === "away";
    const hidden = { ...state, phase: "hidden", speech: null, sprite: away(state.sprite) };
    return { state: hidden, cue: null, handoff: lend ? "arrive" : null };
  }
  if (event.type === "show") return state.phase === "hidden" ? { ...unchanged, state: { ...state, phase: "lent" } } : unchanged;
  if (event.type === "tick") return tick(state, event, env);
  if (state.phase !== "page") return unchanged;
  const out = step(state.sprite, event, env.world, env.tuning);
  let next = { ...state, sprite: out.state };
  if (event.type === "grab") next = { ...next, speech: null };
  // The page has no chat to summon, so a double click is two pokes.
  const cue = out.cue === "summon" ? "poke" : out.cue;
  if (cue === "poke" && !speaking(next, event.now)) next = say(next, event.now, event.rand ?? 0, env);
  return { state: next, cue, handoff: null };
}

// Whether a tick would change nothing: off the page, with nowhere to go.
export function idle(state) {
  if (state.sprite.mode !== "away" || state.phase === "page") return false;
  if (state.phase === "hidden" || state.view === null) return true;
  const { play, blocked } = state.view;
  return state.phase === "lent" ? play || blocked : blocked && !play;
}

function remember(hidden) {
  try {
    if (hidden) localStorage.setItem(HIDDEN_KEY, "1");
    else localStorage.removeItem(HIDDEN_KEY);
  } catch {
    // Storage is off: Hide lasts this page.
  }
}

function hiddenBefore() {
  try {
    return localStorage.getItem(HIDDEN_KEY) === "1";
  } catch {
    return false;
  }
}

// The page half: one frame loop, which a hidden tab does not run, pointer
// events in, and frames, cues and the bubble out. `play` is the playground's
// handle, or null when it is not mounted.
export function mount(doc, data, { cue, bubble, play, still }) {
  const layer = doc.getElementById("fidget");
  const at = doc.getElementById("fidget-at");
  const sprite = doc.getElementById("fidget-sprite");
  const img = sprite.querySelector("img");
  const balloon = doc.getElementById("fidget-say");
  const back = doc.getElementById("fidget-back");
  const stage = doc.querySelector(".stage");
  const narrow = matchMedia("(max-width: 480px)");
  let worn, me, loops, w, h;
  let playing = null;
  // Each frame, so a pick on the desk shows here at once while on the page,
  // and on the next walk on otherwise: the loop does not run while away.
  const wear = () => {
    const id = character(data.cast, data.hero);
    if (id === worn) return;
    worn = id;
    me = data.cast[id];
    ({ loops } = me);
    [w, h] = me.size;
    playing = null;
    Object.assign(img, { alt: me.name, width: w, height: h });
    img.classList.toggle("pixel", me.pixel);
    sprite.style.width = `${w}px`;
    sprite.style.height = `${h}px`;
    if (!still) Object.values(loops).flatMap((loop) => loop.frames).forEach((src) => { new Image().src = src; });
  };
  wear();
  layer.hidden = false;

  const world = () => ({ width: doc.documentElement.clientWidth, height: innerHeight, sprite: { w, h }, windows: [] });
  const column = doc.querySelector(".wrap");
  const margin = () => column.getBoundingClientRect().left + parseFloat(getComputedStyle(column).paddingLeft);
  const env = () => ({ world: world(), tuning: data.tuning, lines: me.lines, sayMs: bubble.bubbleDuration, margin });
  const feetNow = () => feet(state.sprite, world());
  const io = cue.cueIo(doc.getElementById("fidget-cues"), () => {
    const f = feetNow();
    return { x: f.x, y: f.y - h / 2, feetX: f.x, feetY: f.y };
  });
  // Visual only, as on the desk.
  const cues = cue.createCueMachine({ draw: io.draw, sound: () => () => {} });
  let state = start({ now: performance.now(), rolls: [Math.random(), Math.random()], width: world().width,
    hidden: hiddenBefore(), still });
  back.hidden = state.phase !== "hidden";
  // Frames run only while a tick could change something, so a Character off
  // the page costs nothing; every event that could need one wakes the loop.
  let pending = 0;
  const wake = () => {
    if (!pending && !idle(state)) pending = requestAnimationFrame(frame);
  };
  const apply = (event) => {
    const out = update(state, event, env());
    state = out.state;
    if (out.cue) cues.event({ cue: out.cue, visible: true, sound: false });
    if (out.handoff === "arrive") play?.arrive();
    if (out.handoff === "depart") play?.depart();
    wake();
  };

  // Where the playground, the hero's stage and the twist section are. Without
  // a playground it is never in view. While any of the stage shows, or the twist
  // fills most of the screen, the page's fidget stays off: both show their own.
  const twist = doc.getElementById("twist");
  const view = { play: play ? null : false, stage: stage ? null : false, twist: twist ? null : false };
  const report = () => {
    if (!Object.values(view).includes(null)) apply({ type: "view", play: view.play, blocked: view.stage || view.twist });
  };
  if (stage) {
    new IntersectionObserver(([entry]) => {
      view.stage = entry.isIntersecting;
      report();
    }).observe(stage);
  }
  const takesOver = (target, key, { most, threshold }) => {
    // In once the rule says most shows, out only once none does, so a reader
    // resting at the edge does not bounce it. The first report decides either
    // way, as a reload can restore mid-section.
    new IntersectionObserver(([entry]) => {
      const next = most(entry) || (entry.isIntersecting && view[key] === true);
      if (next === view[key]) return;
      view[key] = next;
      report();
    }, { threshold }).observe(target);
  };
  const desk = doc.getElementById("play-desk");
  if (play && desk) takesOver(desk, "play", mostOf);
  if (twist) takesOver(twist, "twist", mostOfScreen);

  let touched = 0;
  decodePresses(sprite, data.tuning, {
    local: (event) => ({ x: event.clientX, y: event.clientY }),
    feetNow,
    apply,
    grabbable: () => !still && !narrow.matches,
    // A finger has no hover to show the Hide chip, so a press shows it a while.
    onPress: () => {
      at.classList.add("touched");
      clearTimeout(touched);
      touched = setTimeout(() => at.classList.remove("touched"), 4000);
    },
  });
  addEventListener("scroll", () => apply({ type: "startle", now: performance.now() }), { passive: true });

  // Each button hides itself, so focus moves on: Hide to Bring back, and Bring
  // back to Hide once the sprite shows, or to the page when it cannot walk on.
  const hide = doc.getElementById("fidget-hide");
  let refocus = false;
  hide.addEventListener("click", () => {
    apply({ type: "hide" });
    remember(true);
    back.hidden = false;
    back.focus({ preventScroll: true });
  });
  back.addEventListener("click", () => {
    apply({ type: "show" });
    remember(false);
    refocus = !idle(state);
    if (!refocus) back.blur();
    back.hidden = true;
  });

  let since = 0;
  let last = null;
  let shown = null;
  let size = null;
  function draw(now) {
    const gone = state.sprite.mode === "away";
    if (at.hidden !== gone) at.hidden = gone;
    if (refocus && !gone) {
      refocus = false;
      hide.focus({ preventScroll: true });
    }
    const line = state.speech?.line ?? null;
    if (line !== shown) {
      shown = line;
      balloon.classList.toggle("visible", line !== null);
      if (line !== null) {
        balloon.querySelector(".bubble-content").textContent = line;
        size = { width: balloon.offsetWidth, height: balloon.offsetHeight };
      }
    }
    if (gone) return;
    const name = animation(state, now);
    if (name !== playing) {
      playing = name;
      since = now;
    }
    // Reduced motion shows each animation's first frame and does not cycle it.
    const src = frameOf(loops[name], still ? 0 : now - since);
    if (img.getAttribute("src") !== src) img.setAttribute("src", src);
    const f = feetNow();
    // Written only on change, so a Character standing still costs no style work.
    const moved = `translate(${f.x - w / 2}px, ${f.y - h}px)`;
    if (at.style.transform !== moved) at.style.transform = moved;
    const facing = state.sprite.facing < 0 ? "scaleX(-1)" : "";
    if (sprite.style.transform !== facing) sprite.style.transform = facing;
    if (line !== null) {
      const room = world();
      const spot = bubble.placeBubble({ x: f.x - w / 2, y: f.y - h, width: w, height: h }, size,
        { x: 0, y: 0, width: room.width, height: room.height });
      balloon.style.transform = `translate(${spot.x}px, ${spot.y}px)`;
      balloon.style.setProperty("--tail-offset", `${spot.tailOffset}px`);
      balloon.classList.toggle("inverted", spot.inverted);
    }
  }
  function frame(now) {
    pending = 0;
    wear();
    apply({ type: "tick", now, dt: last === null ? 0 : (now - last) / 1000, rand: Math.random() });
    draw(now);
    last = pending ? now : null;
  }
  report();
}
