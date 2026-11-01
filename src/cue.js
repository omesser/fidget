
// The cue machine. Five interactions, each with its own visual and sound.
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
  { type = "sine", from, to = from, at = 0, attack = 0.004, hold = 0, decay = 0.14, peak = 0.25, lowpass = null },
) {
  const { ac, g } = v;
  const t0 = ac.currentTime + at;
  const osc = ac.createOscillator();
  osc.type = type;
  osc.frequency.setValueAtTime(from, t0);
  if (to !== from) osc.frequency.exponentialRampToValueAtTime(to, t0 + attack + hold + decay);
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
  osc.stop(t0 + attack + hold + decay + 0.02);
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
  possess: (v) => {
    tone(v, { type: "sine", from: 392, to: 523.25, attack: 0.01, decay: 0.18, peak: 0.16 });
    tone(v, { type: "sine", from: 523.25, to: 659.25, attack: 0.01, decay: 0.22, peak: 0.12, at: 0.08 });
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
  possess(stage, c) {
    const r = el("cue-ring possess", c.x, c.y);
    stage.append(r);
    reap(r);
    return [r];
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
// node can drive the bookkeeping without a window or a sound card.
export function cueIo(layer, anchorOf) {
  return {
    draw(name) {
      const nodes = VISUALS[name](layer, anchorOf());
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
