
// The cue machine. Each interaction has its own visual and sound.
// Frequencies, envelopes, gains, and durations were decided in
// website/cues.html. The Engine decides which cue plays; this obeys.
const MASTER = 0.5;

// How long a Poke stays in flight so a Summon can cancel it. Past
// `input.rs`'s DOUBLE_CLICK_MS no Summon belongs to that Poke, and the
// record is only an element and a gain node.
export const POKE_WINDOW_MS = 400;

let context = null;
// No AudioContext, or one throw, stays silent after the first failure.
// Retrying every cue would be the same error sixty times a second. Visuals still play.
let audioFailed = false;
let audioWarned = false;

function warnAudio(why) {
  if (audioWarned) return;
  audioWarned = true;
  console.warn("fidget: cue audio is unavailable (" + why + "). Visuals still play.");
}

function audio() {
  if (audioFailed) return null;
  try {
    if (!context) {
      const Ctx = window.AudioContext || window.webkitAudioContext;
      if (!Ctx) {
        audioFailed = true;
        warnAudio("no AudioContext");
        return null;
      }
      context = new Ctx();
    }
    // Suspended until the page has been interacted with. Every cue follows a
    // press on the sprite, so the gesture that unlocks audio has already landed.
    if (context.state === "suspended") context.resume();
    return context;
  } catch (err) {
    audioFailed = true;
    warnAudio(err && err.message ? err.message : String(err));
    return null;
  }
}

// A gain node the cue can be cut through. Summon cuts a Poke that is still
// sounding. The cut is a 15 ms ramp so it does not click.
function voice() {
  const ac = audio();
  if (!ac) return null;
  const g = ac.createGain();
  g.gain.value = 0;
  g.connect(ac.destination);
  return {
    ac,
    g,
    cut() {
      const t = ac.currentTime;
      g.gain.cancelScheduledValues(t);
      g.gain.setValueAtTime(g.gain.value, t);
      g.gain.linearRampToValueAtTime(0, t + 0.015);
    },
  };
}

function tone(
  v,
  {
    type = "sine",
    from,
    to = from,
    at = 0,
    attack = 0.004,
    hold = 0,
    decay = 0.14,
    peak = 0.25,
    lowpass = null,
    vibrato = null,
  },
) {
  const { ac, g } = v;
  const t0 = ac.currentTime + at;
  const osc = ac.createOscillator();
  osc.type = type;
  osc.frequency.setValueAtTime(from, t0);
  if (to !== from) osc.frequency.exponentialRampToValueAtTime(to, t0 + attack + hold + decay);
  const end = t0 + attack + hold + decay + 0.02;
  if (vibrato) {
    const lfo = ac.createOscillator();
    lfo.frequency.value = vibrato.rate;
    const depth = ac.createGain();
    depth.gain.value = vibrato.depth;
    lfo.connect(depth);
    depth.connect(osc.frequency);
    lfo.start(t0);
    lfo.stop(end);
  }
  let out = osc;
  if (lowpass) {
    const f = ac.createBiquadFilter();
    f.type = "lowpass";
    f.frequency.value = lowpass;
    osc.connect(f);
    out = f;
  }
  // Per-note envelope on its own gain so two notes in one voice do not fight.
  // The voice gain above them is the one a cancel cuts.
  const env = ac.createGain();
  out.connect(env);
  env.connect(g);
  env.gain.setValueAtTime(0, t0);
  env.gain.linearRampToValueAtTime(peak * MASTER, t0 + attack);
  env.gain.setValueAtTime(peak * MASTER, t0 + attack + hold);
  env.gain.exponentialRampToValueAtTime(0.0005, t0 + attack + hold + decay);
  g.gain.setValueAtTime(1, t0);
  osc.start(t0);
  osc.stop(end);
}

const SOUNDS = {
  poke: (v) => tone(v, { from: 660, to: 600, decay: 0.14, peak: 0.25 }),
  summon: (v) => {
    tone(v, { type: "triangle", from: 523.25, attack: 0.008, decay: 0.13, peak: 0.22 });
    tone(v, { type: "triangle", from: 783.99, attack: 0.008, decay: 0.16, peak: 0.22, at: 0.11 });
  },
  menu: (v) => {
    tone(v, { type: "square", from: 320, attack: 0.002, decay: 0.035, peak: 0.12, lowpass: 1200 });
    tone(v, { type: "square", from: 240, attack: 0.002, decay: 0.035, peak: 0.08, lowpass: 1200, at: 0.06 });
  },
  pickup: (v) => tone(v, { from: 280, to: 720, attack: 0.006, decay: 0.14, peak: 0.2 }),
  drop: (v) => tone(v, { from: 160, to: 55, attack: 0.004, decay: 0.16, peak: 0.3 }),
  throw: (v) => tone(v, { from: 220, to: 55, attack: 0.004, decay: 0.2, peak: 0.48 }),
  // Two voices a few hertz apart, so the "ooo" has a chorus. The possess
  // swell peaks at 0.26 s, when the ghost lands and the thump plays.
  possess: (v) => {
    const ooo = { at: 0.01, attack: 0.21, hold: 0.04, decay: 0.08, peak: 0.12 };
    tone(v, { ...ooo, from: 300, to: 650, vibrato: { rate: 7, depth: 14 } });
    tone(v, { ...ooo, from: 307, to: 663, vibrato: { rate: 7.6, depth: 14 } });
    tone(v, { type: "triangle", from: 330, to: 250, at: 0.26, attack: 0.004, decay: 0.12, peak: 0.08, lowpass: 1200 });
  },
  unpossess: (v) => {
    const ooo = { attack: 0.02, hold: 0.06, decay: 0.18, peak: 0.145 };
    tone(v, { ...ooo, from: 650, to: 300, vibrato: { rate: 7, depth: 14 } });
    tone(v, { ...ooo, from: 663, to: 307, vibrato: { rate: 7.6, depth: 14 } });
  },
};

function el(cls, x, y) {
  const e = document.createElement("div");
  e.setAttribute("class", "cue " + cls);
  e.style.left = x + "px";
  e.style.top = y + "px";
  return e;
}

let cachedPuffInset = {};

export function measurePuffInset(className) {
  if (cachedPuffInset[className]) return cachedPuffInset[className];

  const wrapper = document.createElement("div");
  wrapper.className = "cue";
  wrapper.style.position = "absolute";
  wrapper.style.left = "-9999px";
  wrapper.style.top = "-9999px";
  wrapper.style.visibility = "hidden";

  const puff = el(className, 0, 0);
  wrapper.append(puff);
  const stage = document.getElementById("stage");
  (stage || document.body).append(wrapper);

  void wrapper.offsetHeight;

  const styles = getComputedStyle(puff);
  const baseHeight = parseFloat(styles.height);
  const borderWidth = parseFloat(styles.borderTopWidth || "0");
  const shadowWidth = parseFloat(styles.getPropertyValue("--puff-shadow") || "0");

  const finalScale = styles.getPropertyValue("--puff-final-scale").trim();
  const maxScaleY = parseFloat(finalScale);
  if (!maxScaleY || maxScaleY <= 0) {
    throw new Error(`No --puff-final-scale custom property for ${className}`);
  }

  const inset = ((baseHeight + borderWidth * 2 + shadowWidth * 2) * maxScaleY) / 2;

  cachedPuffInset[className] = inset;
  wrapper.remove();
  return inset;
}

function reap(node, child) {
  // Remove on the last animation to end, so a stagger does not cut its siblings.
  let pending = child ? child.length : 1;
  const done = () => {
    if (--pending <= 0) node.remove();
  };
  (child || [node]).forEach((c) => c.addEventListener("animationend", done, { once: true }));
}

const UP = { x: 0, y: -1 };

// The possession ghost: the frame's silhouette, a white STROKE outside it with
// ink beyond (INK_OUT) and just inside (INK_IN), over a faint FILL. Display px,
// drawn SS times supersampled so the stroke stays smooth.
const PAD = 6;
const SS = 2;
const STROKE = 2;
const INK_OUT = 1.6;
const INK_IN = 1.1;
const FILL = 0.18;
const WHITE = [255, 255, 255];
const INK = [0x24, 0x26, 0x2c];
const OUTLINE_CACHE = 64;

const clamp01 = (v) => Math.min(1, Math.max(0, v));
const REACH = Math.ceil((STROKE + INK_OUT + 1) * SS);
const OFFSETS = [];
for (let dy = -REACH; dy <= REACH; dy++) {
  for (let dx = -REACH; dx <= REACH; dx++) {
    const d = Math.hypot(dx, dy);
    if (d > 0 && d <= REACH) OFFSETS.push(dx, dy, d);
  }
}

// RGBA for a W×H alpha map, both in supersampled px. A pixel's nearest one of
// the other kind sits on a boundary, so only boundary pixels splat distances.
// PAD keeps the body further than REACH from the map's edge.
export function outlinePixels(alpha, W, H) {
  const body = (x, y) => x >= 0 && y >= 0 && x < W && y < H && alpha[y * W + x] >= 128;
  const dist = new Float32Array(W * H).fill(REACH + 1);
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const k = body(x, y);
      if (body(x - 1, y) === k && body(x + 1, y) === k && body(x, y - 1) === k && body(x, y + 1) === k) continue;
      for (let j = 0; j < OFFSETS.length; j += 3) {
        const tx = x + OFFSETS[j];
        const ty = y + OFFSETS[j + 1];
        if (tx < 0 || ty < 0 || tx >= W || ty >= H || body(tx, ty) === k) continue;
        const t = ty * W + tx;
        if (OFFSETS[j + 2] < dist[t]) dist[t] = OFFSETS[j + 2];
      }
    }
  }
  const out = new Uint8ClampedArray(W * H * 4);
  for (let i = 0; i < W * H; i++) {
    const d = dist[i] / SS;
    let a;
    let from;
    let to;
    let mix;
    if (alpha[i] >= 128) {
      const edge = clamp01(INK_IN + 0.5 - d);
      a = FILL + (0.9 - FILL) * edge;
      [from, to, mix] = [WHITE, INK, (edge * 0.9) / a];
    } else {
      a = clamp01(STROKE + INK_OUT + 0.5 - d);
      if (a === 0) continue;
      [from, to, mix] = [INK, WHITE, clamp01(STROKE + 0.5 - d) / a];
    }
    for (let c = 0; c < 3; c++) out[i * 4 + c] = Math.round(from[c] + (to[c] - from[c]) * mix);
    out[i * 4 + 3] = Math.round(a * 255);
  }
  return out;
}

const outlines = new Map();

// The sprite's frame as drawn: its layout size, its mirror and its smoothing.
function outlineOf(sprite) {
  const w = sprite.width;
  const h = sprite.height;
  const mirror = Number(sprite.dataset.mirror) === -1 ? -1 : 1;
  const smooth = sprite.style.imageRendering === "auto";
  const key = `${sprite.currentSrc}@${w}x${h}@${mirror}@${smooth}`;
  const hit = outlines.get(key);
  if (hit) {
    outlines.delete(key);
    outlines.set(key, hit);
    return hit;
  }
  const W = (w + 2 * PAD) * SS;
  const H = (h + 2 * PAD) * SS;
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext("2d");
  ctx.imageSmoothingEnabled = smooth;
  if (mirror === -1) {
    ctx.translate(W, 0);
    ctx.scale(-1, 1);
  }
  ctx.drawImage(sprite, PAD * SS, PAD * SS, w * SS, h * SS);
  const image = ctx.getImageData(0, 0, W, H);
  const alpha = new Uint8Array(W * H);
  for (let i = 0; i < alpha.length; i++) alpha[i] = image.data[i * 4 + 3];
  image.data.set(outlinePixels(alpha, W, H));
  ctx.putImageData(image, 0, 0);
  const made = { canvas, w: w + 2 * PAD, h: h + 2 * PAD, smooth };
  outlines.set(key, made);
  if (outlines.size > OUTLINE_CACHE) outlines.delete(outlines.keys().next().value);
  return made;
}

// Each cue's ghost layers and sprite class, how far its keyframed path in
// cue-possess.css reaches above the centre and to the right of it, its whole
// left-to-right `span`, and its largest scale.
const GHOSTS = {
  possess: { layers: ["trail2", "trail", "lead"], sprite: "cue-possess", rise: 165, right: 85, span: [-30, 85], scale: 1 },
  unpossess: { layers: ["rise"], sprite: "cue-release", rise: 150, right: 0, span: [-8, 6], scale: 1.04 },
};

// --dir and --ky bend the path off the overlay's edges. The rise is squashed,
// never flipped below, and the anchor never moves, so the ghost still ends on
// the body. A path that reaches right mirrors when only the left side fits.
export function ghostPath(cue, { cx, cy, gw, gh, stageW }) {
  const { rise, right } = GHOSTS[cue];
  const ky = cy < rise + gh / 2 ? Math.max(0.4, (cy - gh / 2 - 4) / rise) : 1;
  const mirrored = right > 0 && cx + right + gw / 2 > stageW && cx - right - gw / 2 >= 0;
  return { dir: mirrored ? -1 : 1, ky };
}

// Everywhere the ghost is drawn in flight, as [x, y, width, height]: its
// path's reach plus its frame turned any way. Windows draws only there.
export function ghostFlight(cue, { cx, cy, gw, gh }, { dir, ky }) {
  const { rise, span, scale } = GHOSTS[cue];
  const r = (Math.hypot(gw, gh) / 2) * scale;
  const xs = span.map((dx) => cx + dx * dir);
  const left = Math.floor(Math.min(...xs) - r);
  const top = Math.floor(cy - rise * ky - r);
  const right = Math.ceil(Math.max(...xs) + r);
  const bottom = Math.ceil(cy + r);
  return [left, top, right - left, bottom - top];
}

function ghost(outline, c, cls, path) {
  const g = document.createElement("canvas");
  g.setAttribute("class", "cue cue-oghost " + cls);
  g.width = outline.canvas.width;
  g.height = outline.canvas.height;
  g.getContext("2d").drawImage(outline.canvas, 0, 0);
  g.style.left = c.x + "px";
  g.style.top = c.y + "px";
  g.style.width = outline.w + "px";
  g.style.height = outline.h + "px";
  g.style.imageRendering = outline.smooth ? "auto" : "pixelated";
  g.style.setProperty("--dir", path.dir);
  g.style.setProperty("--ky", path.ky);
  return g;
}

// Restarts the class's animation even when it is the one already on. The
// shake and the dim end apart, so the class goes when neither still runs.
function shake(sprite, cls) {
  const all = Object.values(GHOSTS).map((g) => g.sprite);
  sprite.classList.remove(...all);
  void sprite.offsetWidth;
  sprite.classList.add(cls);
  sprite.onanimationend = () => {
    const running = sprite.getAnimations().some((a) => a.animationName && a.playState !== "finished");
    if (!running) sprite.classList.remove(...all);
  };
}

const reducedMotion = () => globalThis.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

function possession(stage, c, sprite, cue) {
  shake(sprite, GHOSTS[cue].sprite);
  if (reducedMotion()) return [];
  const outline = outlineOf(sprite);
  // The overlay's cue layer is zero-sized, so its stage is the window. The
  // cues page draws into a desk narrower than that.
  const stageW = stage.clientWidth || (globalThis.innerWidth ?? Infinity);
  const at = { cx: c.x, cy: c.y, gw: outline.w, gh: outline.h };
  const path = ghostPath(cue, { ...at, stageW });
  const ghosts = GHOSTS[cue].layers.map((cls) => ghost(outline, c, cls, path));
  const wrapper = document.createElement("div");
  wrapper.className = "cue";
  wrapper.flight = ghostFlight(cue, at, path);
  wrapper.append(...ghosts);
  stage.append(wrapper);
  reap(wrapper, ghosts);
  return [wrapper];
}

// The puff sits on the contact, pushed off the surface along its normal by
// its own half-thickness, and lies along it. A landing with no contact is a
// floor under the feet.
export function landingPuff(stage, c, className) {
  const normal = c.ripple_normal ?? UP;
  const inset = measurePuffInset(className);
  const x = (c.ripple_x ?? c.feetX) + normal.x * inset;
  const y = (c.ripple_y ?? c.feetY) + normal.y * inset;
  const turn = (Math.atan2(normal.y, normal.x) * 180) / Math.PI + 90;
  // A translate, not left and top. Chrome rounds those to 1/64px, and the
  // inset is what holds the puff flush against a wall.
  const wrapper = el("", 0, 0);
  wrapper.style.transform = `translate(${x}px, ${y}px) rotate(${turn}deg)`;
  wrapper.append(el(className, 0, 0));
  stage.append(wrapper);
  reap(wrapper);
  return [wrapper];
}

const VISUALS = {
  poke(stage, c) {
    const r = el("cue-ring", c.x, c.y);
    stage.append(r);
    reap(r);
    return [r];
  },
  summon(stage, c) {
    const a = el("cue-ring in", c.x, c.y);
    const b = el("cue-ring in second", c.x, c.y);
    stage.append(a, b);
    reap(a);
    reap(b);
    return [a, b];
  },
  menu(stage, c) {
    // Whole pixels. A ring this thin smears when its centre sits between them.
    const x = Math.round(c.x);
    const y = Math.round(c.y);
    const fill = el("cue-wedge", x, y);
    const edge = el("cue-wedge-edge", x, y);
    stage.append(fill, edge);
    // Advance the sweep by hand. A custom property cannot be keyframed
    // without @property, and this stays portable to GTK WebKit.
    const t0 = performance.now();
    const sweepMs = 240;
    const ease = (p) => 1 - Math.pow(1 - p, 3);
    const step = (now) => {
      const p = Math.min(1, (now - t0) / sweepMs);
      const deg = (ease(p) * 360).toFixed(1) + "deg";
      fill.style.setProperty("--sweep", deg);
      edge.style.setProperty("--sweep", deg);
      if (p < 1) requestAnimationFrame(step);
      else {
        fill.classList.add("fading");
        edge.classList.add("fading");
        reap(fill);
        reap(edge);
      }
    };
    requestAnimationFrame(step);
    return [fill, edge];
  },
  pickup(stage, c) {
    const ds = [-30, -10, 10, 30].map((dx, i) => {
      const d = el("cue-dash", c.feetX + dx, c.feetY - 12);
      d.style.animationDelay = i * 25 + "ms";
      return d;
    });
    const g = document.createElement("div");
    g.className = "cue";
    g.append(...ds);
    stage.append(g);
    reap(g, ds);
    return [g];
  },
  drop(stage, c) {
    return landingPuff(stage, c, "cue-puff");
  },
  throw(stage, c) {
    return landingPuff(stage, c, "cue-puff hard");
  },
  possess(stage, c, sprite) {
    return possession(stage, c, sprite, "possess");
  },
  unpossess(stage, c, sprite) {
    return possession(stage, c, sprite, "unpossess");
  },
};

// Click cues hang at the centre, because the click landed on the art. Pickup
// and drop hang at the feet, the contact the gesture broke and made. Overlay
// coordinates, the same space the bubble is placed in.
export function cueAnchor(sprite) {
  return {
    x: sprite.x + sprite.width / 2,
    y: sprite.y + sprite.height / 2,
    feetX: sprite.x + sprite.width / 2,
    feetY: sprite.y + sprite.height,
    ripple_x: sprite.ripple_x,
    ripple_y: sprite.ripple_y,
    ripple_normal: sprite.ripple_normal,
  };
}

// DOM and Web Audio for one Instance's cue layer. Apart from the machine so
// node can drive the bookkeeping without a window or a sound card. `sprite` is
// the Instance's `<img>`, which the possession cues outline and shake.
export function cueIo(layer, anchorOf, sprite) {
  return {
    draw(name) {
      const nodes = VISUALS[name](layer, anchorOf(), sprite);
      return () => nodes.forEach((node) => node.remove());
    },
    sound(name) {
      // Swallowed. Silence is the mute machine, and a throw here would kill
      // the frame listener and the visual with it.
      try {
        const v = voice();
        if (!v) return () => {};
        SOUNDS[name](v);
        return () => v.cut();
      } catch (err) {
        warnAudio(err && err.message ? err.message : String(err));
        return () => {};
      }
    },
  };
}

// Decisions apart from the pixels and the oscillators. A cue rides one
// Engine tick, like `dialogue`, so it is latched where every delivery is
// seen. A double-click is two releases, and its Summon cancels the Poke.
export function createCueMachine(io) {
  const schedule = io.schedule ?? ((fn, ms) => setTimeout(fn, ms));
  const cancel = io.cancel ?? ((id) => clearTimeout(id));

  // At most one. Only a Poke is cancelled, and only the newest can still be sounding.
  let poke = null;

  function forget() {
    if (poke === null) return;
    cancel(poke.timer);
    poke = null;
  }

  return {
    // `cue` names the cue or is null. `visible` is whether the Character is
    // on screen. `sound` is whether it may be heard as well as seen.
    event(placement) {
      const name = placement.cue;
      // A hidden Character produces no cue. A sound with nothing on screen
      // to explain it is worse than silence.
      if (!name || !placement.visible) return;

      if (name === "summon" && poke) {
        poke.stop();
        forget();
      }

      const stopVisual = io.draw(name);
      // Sound is gated and the visual is not. Do Not Disturb and the Settings
      // mute fold into this flag. A cue that cannot be heard still has to be
      // seen, or a muted character stops answering.
      let cutSound = () => {};
      if (placement.sound) {
        try {
          cutSound = io.sound(name) || (() => {});
        } catch {
          // Mute machine: silence, once, rather than taking the overlay with it.
        }
      }

      if (name === "poke") {
        // A second Poke replaces the record. The older one is past cancelling
        // by the time a Summon could arrive for this one.
        forget();
        poke = {
          stop: () => {
            stopVisual();
            cutSound();
          },
          timer: schedule(forget, POKE_WINDOW_MS),
        };
      }
    },
  };
}
