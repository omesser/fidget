// The webview draws the Characters and owns none of their state. Each tick the
// Rust side sends every Instance's placement; this keeps the two most recent
// per Instance to interpolate between, and drops an id that stops arriving.

import { arrived, interpolate, onDisplay } from "./interpolate.js";
import {
  createBubbleMachine,
  wrapText,
  placeBubble,
} from "./bubble.js";
import { createCueMachine, cueAnchor, cueIo } from "./cue.js";
import {
  CONNECT_HINT,
  applyQuickMessageGate,
  createQuickMessage,
  crossedDrag,
  placeQuickMessage,
  quickMessageMirror,
} from "./quick-message.js";
import { reportPaintedRects, computeBubblePaintedRect } from "./painted-rects.js";

const stage = document.getElementById("stage");

// Whether this overlay can take a click that is not the art. The Shell answers,
// per platform lane, because it is the side that knows. False until `start`
// asks, which is the safe way to be wrong.
let clickableOffArt = false;

// Every Character's art as data: URLs, keyed by Character name and fetched
// once. Art, not state, and one entry however many Instances draw from it.
let characters = {};

// A press on the pet, measured from pointerdown. The composer never arms
// this: a drag in the field is a selection, and it must not grab the pet.
let petDrag = null;

// Latch for the entire gesture: true from pointerdown on sprite through
// pointerup/cancel. Stays true even after petDrag clears on threshold cross.
let gestureActive = false;

const views = new Map();

function currentDisplayBounds() {
  return { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight };
}

// Everything one Instance's sprite needs to be drawn. Per Instance rather than
// shared because two characters speak on their own schedules: one bubble machine
// would hand a line meant for one to whichever drew last.
function createView(id) {
  const sprite = document.createElement("img");
  sprite.className = "sprite";
  sprite.alt = "";
  sprite.dataset.instance = id;

  const bubble = document.createElement("div");
  bubble.className = "bubble";
  bubble.dataset.instance = id;
  const bubbleContent = document.createElement("div");
  bubbleContent.className = "bubble-content";
  const dots = document.createElement("div");
  dots.className = "thinking-dots";
  for (let i = 0; i < 3; i += 1) {
    dots.appendChild(document.createElement("span"));
  }
  // The way out of a line that did not fit. Drawn only when the turn was
  // truncated, and clicked, never implied: opening the Chat surface stays a
  // deliberate act (ADR-0036), so nothing here reacts to the line arriving.
  const more = document.createElement("button");
  more.type = "button";
  more.className = "bubble-more";
  more.textContent = "Open chat";
  // The overlay's own pointer listeners report a press to the Engine, which
  // answers a click on the art with a Poke. This press is on a control, not on
  // the Character, so it stops here.
  const swallow = (event) => event.stopPropagation();
  more.addEventListener("pointerdown", swallow);
  more.addEventListener("pointerup", swallow);
  more.addEventListener("click", (event) => {
    event.stopPropagation();
    window.__TAURI__.core.invoke("overlay_open_chat", { id }).catch((err) => {
      console.error("overlay_open_chat", err);
    });
  });

  bubble.append(bubbleContent, dots, more);

  // The Instance's cues, in a layer of their own so a dismissed character takes
  // any still playing with it. Last of the three, so a cue sharing the sprite's
  // z-index is drawn over the art it marks rather than under it.
  const cueLayer = document.createElement("div");
  cueLayer.className = "cue-layer";
  cueLayer.dataset.instance = id;

  // All three in one call: a sprite and its bubble are stacked by the z-index
  // written every tick, not by append order, which would put the first-seen
  // Instance at the back and send a reappearing id to the front.
  stage.append(bubble, sprite, cueLayer);

  const view = {
    id,
    sprite,
    bubble,
    bubbleContent,
    more,
    cueLayer,
    // Where "Open chat" is, in this overlay's coordinates, or null when it is
    // not drawn. `reportHotspots` sends the set across; see there for why the
    // renderer is the one who has to.
    hotspot: null,
    // Where the bubble is, in overlay coordinates: [x, y, width, height] as integers,
    // or null when not visible. Windows unions this into the input region so the
    // bubble body isn't clipped away by SetWindowRgn.
    paintedRect: null,
    // The two most recent placements and when each arrived. Drawing one sample
    // behind, interpolated, buys continuous motion (see interpolate.js); the
    // hit-test uses the unlagged position, so it leads the screen by one sample.
    //
    // ponytail: every overlay interpolates on its own clock — its own arrival
    // times and its own frame phase — so the two halves of a sprite on a seam
    // can round a point apart while it is moving. At rest they cannot, there
    // being nothing to interpolate. The upgrade is to carry the Engine's own
    // timestamp on the frame and solve for it, which needs a per-webview offset
    // between that clock and `performance.now()`; worth it if README item 19
    // ever shows a shimmer at the seam under a drag.
    previous: null,
    latest: null,
    // The last placement heard, serialized. A resend repeats it byte for byte
    // so a late listener hears `visible`; one already heard has nothing to draw.
    told: null,
    // Which frame and size were last written to the element. Only the transform
    // changes per display frame; writing the same src sixty times a second
    // would ask the loader for art it already has.
    drawn: null,
    // Which Character's art this view is drawing, so a change of art also
    // changes how it is filtered when scaled.
    character: null,
  };

  function spriteRect() {
    return {
      x: view.latest.x,
      y: view.latest.y,
      width: view.latest.width,
      height: view.latest.height,
    };
  }

  // One element in one of two modes makes bubble.js's rule (speech and the
  // indicator never coincide) true of the pixels. Only opacity gates it: an exit
  // timed by `fade_ms`, the Character's presence fade, kept dismissed bubbles up.
  function show(mode) {
    bubble.dataset.mode = mode;
    bubble.classList.add("visible");
    view.quickMachine?.setBubble(true);
    positionBubble(view, spriteRect(), currentDisplayBounds());
    // Place now: idle overlays may not paint after arm(), and a departed
    // bubble must not leave the quick-message at a stale top.
    if (view.quickMachine?.visible) positionQuick(view, spriteRect());
    arm();
  }

  function hide() {
    bubble.classList.remove("visible");
    view.quickMachine?.setBubble(false);
    view.hotspot = null;
    view.paintedRect = null;
    reportAllPaintedRects();
    if (view.quickMachine?.visible) positionQuick(view, spriteRect());
    arm();
  }

  // Anchored when the cue fires rather than followed afterwards: a cue is a
  // burst where the gesture landed, and the sprite it marks may be halfway to
  // the cursor before it fades.
  view.cues = createCueMachine(cueIo(cueLayer, () => cueAnchor(spriteRect())));

  view.bubbles = createBubbleMachine({
    // Speech bubble: measure text, wrap it, show truncation control when needed.
    // The backend sends dialogue text; the frontend measures, wraps, and renders it.
    showSpeech(text, cutOff) {
      const canvas = document.createElement("canvas");
      const ctx = canvas.getContext("2d");
      ctx.font = "14px system-ui, sans-serif";
      const { lines, truncated } = wrapText(text, 260, ctx.measureText.bind(ctx));
      view.bubbleContent.textContent = lines.join("\n");
      more.textContent = "Open chat";
      bubble.removeAttribute("data-ask");
      bubble.toggleAttribute("data-more", truncated && clickableOffArt);
      show("speech");
      window.__TAURI__.core.invoke("overlay_trace_bubble", {
        label: window.__TAURI__.webviewWindow.getCurrentWebviewWindow().label,
        message: `showSpeech instance=${id}`,
      }).catch(() => {});
    },
    // Ask bubble: prompt to open Chat for a question. Shorter text, distinct style.
    // Shown when the backend signals the user needs to answer in Chat.
    showAsk() {
      view.bubbleContent.textContent = clickableOffArt
        ? "Question for you in the "
        : "Question for you in the chat";
      more.textContent = "chat";
      bubble.setAttribute("data-ask", "");
      bubble.toggleAttribute("data-more", clickableOffArt);
      show("speech");
      window.__TAURI__.core.invoke("overlay_trace_bubble", {
        label: window.__TAURI__.webviewWindow.getCurrentWebviewWindow().label,
        message: `showAsk instance=${id}`,
      }).catch(() => {});
    },
    hideSpeech() {
      hide();
      window.__TAURI__.core.invoke("overlay_trace_bubble", {
        label: window.__TAURI__.webviewWindow.getCurrentWebviewWindow().label,
        message: `hideSpeech instance=${id}`,
      }).catch(() => {});
    },
    // Thinking indicator: animated dots, no text. Shown while AI is generating a reply.
    // Smaller than speech bubble, centered differently, no truncation control.
    showThinking() {
      show("thinking");
      window.__TAURI__.core.invoke("overlay_trace_bubble", {
        label: window.__TAURI__.webviewWindow.getCurrentWebviewWindow().label,
        message: `showThinking instance=${id}`,
      }).catch(() => {});
    },
    hideThinking() {
      hide();
      window.__TAURI__.core.invoke("overlay_trace_bubble", {
        label: window.__TAURI__.webviewWindow.getCurrentWebviewWindow().label,
        message: `hideThinking instance=${id}`,
      }).catch(() => {});
    },
  });

  attachQuickMessage(view, id);
  return view;
}

function positionBubble(view, spriteRect, displayBounds) {
  const bubbleSize = { width: view.bubble.offsetWidth, height: view.bubble.offsetHeight };
  const pos = placeBubble(spriteRect, bubbleSize, displayBounds);
  view.bubble.style.left = `${pos.x}px`;
  view.bubble.style.top = `${pos.y}px`;
  view.bubble.style.setProperty("--tail-offset", `${pos.tailOffset}px`);
  view.bubble.classList.toggle("inverted", pos.inverted);

  // `clientLeft`/`clientTop` are the bubble's 2px ring; without them the rect
  // sits 2px up and left and an 18px target's bottom rows click through. The
  // attribute first spares a layout; zero width catches a CSS-hidden control.
  view.hotspot = view.bubble.hasAttribute("data-more") && view.more.offsetWidth
    ? [
        Math.round(pos.x) + view.bubble.clientLeft + view.more.offsetLeft,
        Math.round(pos.y) + view.bubble.clientTop + view.more.offsetTop,
        view.more.offsetWidth,
        view.more.offsetHeight,
      ]
    : null;

  // Painted rect for Windows input region: the bubble body, not the hotspot control.
  view.paintedRect = computeBubblePaintedRect(view.bubble, pos);
  reportAllPaintedRects();
}

function speechRect(view) {
  if (!view.bubble.classList.contains("visible")) return null;
  if (view.bubble.dataset.mode !== "speech") return null;
  // During fade-out transitions, bubble still has "visible" class but is transparent.
  // Don't reserve space for it while fading.
  const opacity = parseFloat(view.bubble.style.opacity);
  if (opacity < 1 && !isNaN(opacity)) return null;
  // Painted rect is already [x, y, width, height] integers.
  view.paintedRect = [
    parseFloat(view.bubble.style.left) || 0,
    parseFloat(view.bubble.style.top) || 0,
    view.bubble.offsetWidth,
    view.bubble.offsetHeight,
  ];
  return {
    x: parseFloat(view.bubble.style.left) || 0,
    y: parseFloat(view.bubble.style.top) || 0,
    width: view.bubble.offsetWidth,
    height: view.bubble.offsetHeight,
  };
}

function positionQuick(view, spriteRect) {
  const size = { width: view.quick.offsetWidth, height: view.quick.offsetHeight };
  const pos = placeQuickMessage(spriteRect, size, currentDisplayBounds(), speechRect(view));
  view.quick.style.left = `${pos.x}px`;
  view.quick.style.top = `${pos.y}px`;
  view.quick.classList.toggle("inverted", pos.inverted);
  view.quickHotspot =
    clickableOffArt && size.width > 0 && size.height > 0
      ? [Math.round(pos.x), Math.round(pos.y), size.width, size.height]
      : null;
}

let reportedComposing = null;
let reportedQmVisible = null;
let reportedQmState = "";

// Report the full QM state: instance, open, text, focused. Backend uses this
// for pill handoff: when bubble_owner changes and the new owner has QM open,
// it emits qm-handoff to transfer the pill with its state.
function reportQmState(view) {
  const state = {
    instance: view.id,
    open: view.quickMachine.visible,
    text: view.quickField.value,
    focused: document.activeElement === view.quickField,
  };
  const serialized = JSON.stringify(state);
  if (serialized === reportedQmState) return;
  reportedQmState = serialized;
  window.__TAURI__.core.invoke("overlay_qm_state", state).catch((err) => {
    console.error("overlay_qm_state", err);
  });
}

// A newer opening, from the command or from `chat-opening`, wins. The pill
// stays frozen until one says chat can answer.
function paintQuickGate(view, opening) {
  applyQuickMessageGate(view.quickGate, opening);
  // setAvailable skips the paint when the bit did not move. The sentence
  // still can, and the mirror has to grow to fit it.
  syncQuick(view);
}

function refreshQuickGate(view, id) {
  const token = (view.gateToken = (view.gateToken ?? 0) + 1);
  window.__TAURI__.core
    .invoke("chat_opening", { instance: id })
    .then((opening) => {
      if (view.gateToken !== token) return;
      paintQuickGate(view, opening);
    })
    .catch((err) => {
      console.error("chat_opening", err);
      if (view.gateToken !== token) return;
      paintQuickGate(view, null);
    });
}

function noteQuickOpening(opening) {
  for (const view of views.values()) {
    if (!view.quickGate) continue;
    view.gateToken = (view.gateToken ?? 0) + 1;
    paintQuickGate(view, opening);
  }
}

function reportComposing() {
  let id = "";
  for (const [viewId, view] of views) {
    if (view.quickMachine.typing) {
      id = viewId;
      break;
    }
  }
  if (id === reportedComposing) return;
  reportedComposing = id;
  window.__TAURI__.core.invoke("overlay_composing", { instance: id }).catch((err) => {
    console.error("overlay_composing", err);
  });
}

function reportQmVisible() {
  let id = "";
  for (const [viewId, view] of views) {
    if (view.quickMachine.visible) {
      id = viewId;
      break;
    }
  }
  if (id === reportedQmVisible) return;
  reportedQmVisible = id;
  window.__TAURI__.core.invoke("overlay_qm_visible", { instance: id }).catch((err) => {
    console.error("overlay_qm_visible", err);
  });
}

function syncQuick(view) {
  const visible = view.quickMachine.visible;
  view.quick.classList.toggle("visible", visible);
  // A disabled field can still hold the caret from the moment it was ready.
  if (
    (!visible || !view.quickMachine.available) &&
    document.activeElement === view.quickField
  ) {
    view.quickField.blur();
  }
  if (view.quickField.value !== view.quickMachine.text) {
    view.quickField.value = view.quickMachine.text;
  }
  view.quickMirror.textContent = quickMessageMirror(
    view.quickField.value,
    view.quickField.placeholder,
    view.quickMachine.available,
  );
  if (view.quickMachine.takeFocus()) {
    window.__TAURI__.core.invoke("overlay_request_focus").catch((err) => {
      console.error("overlay_request_focus", err);
    });
    view.quickField.focus();
  }
  reportComposing();
  reportQmVisible();
  reportQmState(view);
  if (!visible || !view.latest) {
    view.quickHotspot = null;
    reportHotspots();
    arm();
    return;
  }
  // Force layout before measuring: offsetHeight can be stale if CSS just changed.
  void view.quick.offsetHeight;
  positionQuick(view, {
    x: Math.round(view.latest.x),
    y: Math.round(view.latest.y),
    width: view.latest.width,
    height: view.latest.height,
  });
  reportHotspots();
  arm();
}

// Click-through can drop the pointerleave. The next frame still knows the
// cursor is gone, because :hover is clear once the window ignores it.
// Runs before the pill is up too: a leave during the dwell cancels it.
function notePointerLeft(view) {
  if (!gestureActive) {
    if (view.sprite.matches(":hover")) view.quickMachine.enterSprite();
    else view.quickMachine.leaveSprite();
    if (view.quick.matches(":hover")) view.quickMachine.enterPill();
    else view.quickMachine.leavePill();
  }
}

function attachQuickMessage(view, id) {
  const quick = document.createElement("div");
  quick.className = "bubble quick-message";
  quick.dataset.instance = id;

  const row = document.createElement("div");
  row.className = "quick-message-row";
  const mirror = document.createElement("div");
  mirror.className = "quick-message-mirror";
  mirror.setAttribute("aria-hidden", "true");
  const field = document.createElement("textarea");
  field.className = "quick-message-field";
  field.rows = 1;
  // Same bound the Chat composer declares, which is CHAT_LIMIT.
  field.maxLength = 16000;
  field.disabled = true;
  field.autocomplete = "off";
  field.setAttribute("aria-label", "Quick message");
  // Only where a click off the art lands. Elsewhere the hint stays text.
  // A muted hint whose last words are the control, as the ask bubble does.
  let link = null;
  if (clickableOffArt) {
    link = document.createElement("span");
    link.className = "quick-message-connect";
    link.hidden = true;
    const openChat = document.createElement("button");
    openChat.type = "button";
    openChat.className = "quick-message-open";
    openChat.textContent = "Open chat";
    const swallow = (event) => event.stopPropagation();
    openChat.addEventListener("pointerdown", swallow);
    openChat.addEventListener("pointerup", swallow);
    openChat.addEventListener("click", (event) => {
      event.stopPropagation();
      view.quickMachine.summon();
      window.__TAURI__.core.invoke("overlay_open_chat", { id }).catch((err) => {
        console.error("overlay_open_chat", err);
      });
    });
    link.append(`${CONNECT_HINT} `, openChat);
  }
  const send = document.createElement("button");
  send.type = "button";
  send.className = "quick-message-send";
  send.setAttribute("aria-label", "Send");
  send.disabled = true;
  const icon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  icon.setAttribute("viewBox", "0 0 12 12");
  icon.setAttribute("aria-hidden", "true");
  const point = document.createElementNS("http://www.w3.org/2000/svg", "path");
  point.setAttribute("d", "M2.2 1.4 L10.2 6 L2.2 10.6 L2.2 7.1 L6.5 6 L2.2 4.9 Z");
  point.setAttribute("fill", "#14171e");
  icon.append(point);
  send.append(icon);
  // Before the field, so the sibling rule in main.css can hide the
  // placeholder that would otherwise draw under the link.
  row.append(mirror, ...(link ? [link] : []), field);
  quick.append(row, send);
  stage.append(quick);

  view.quick = quick;
  view.quickField = field;
  view.quickMirror = mirror;

  let machine;
  machine = createQuickMessage({
    schedule: (fn, ms) => window.setTimeout(fn, ms),
    clear: (timer) => window.clearTimeout(timer),
    // Frozen until the first opening. The fetch below is what thaws it.
    available: false,
    onChange() {
      syncQuick(view);
    },
    send(text) {
      const token = (view.gateToken = (view.gateToken ?? 0) + 1);
      window.__TAURI__.core
        .invoke("chat_opening", { instance: id })
        .then((opening) => {
          // A newer opening already painted. Trust that one: this fetch is a
          // stale picture, and the line still goes if that picture can answer.
          if (view.gateToken === token) paintQuickGate(view, opening);
          if (!machine.available) {
            view.bubbles.aiTurnAbandoned();
            machine.restore(text);
            return;
          }
          // Start AI turn immediately. Backend filters thinking/dialogue by bubble
          // ownership; non-owner overlays clear aiTurnPending when placement.bubble=false.
          view.bubbles.aiTurnStarted();
          return window.__TAURI__.core.invoke("chat_send", { instance: id, text, echo: true });
        })
        .catch((err) => {
          view.bubbles.aiTurnAbandoned();
          if (machine.available) machine.restore(text);
          console.error("chat_send", err);
        });
    },
  });
  view.quickMachine = machine;
  view.quickGate = { field, send, link, machine };
  view.quickMirror.textContent = "\u200b";
  paintQuickGate(view, null);
  refreshQuickGate(view, id);

  view.sprite.addEventListener("pointerenter", () => {
    machine.enterSprite();
    refreshQuickGate(view, id);
  });
  view.sprite.addEventListener("pointerleave", () => machine.leaveSprite());
  quick.addEventListener("pointerenter", () => machine.enterPill());
  quick.addEventListener("pointerleave", () => machine.leavePill());
  field.addEventListener("focus", () => machine.focus());
  field.addEventListener("blur", () => machine.blur());
  field.addEventListener("input", () => {
    machine.setText(field.value);
    syncQuick(view);
  });
  field.addEventListener("keydown", (event) => {
    const handled = machine.keydown(event.key, {
      shiftKey: event.shiftKey,
      composing: event.isComposing,
    });
    if (!handled) return;
    event.preventDefault();
    event.stopPropagation();
  });
  send.addEventListener("click", (event) => {
    event.stopPropagation();
    machine.submit();
  });
}

// Tell the Rust side where this overlay wants a click. Clicks pass through
// wherever the art is not, and "Open chat" sits outside the art; only the
// renderer knows where, because the bubble is sized by text measured here.
//
// ponytail: sent whenever the rectangle changes, which while a truncated line
// is up and the Character is walking is once a frame. Under a hundred bytes an
// invoke and only while such a bubble is on screen; if that ever shows up in a
// profile, the upgrade is to send the control's offset from the sprite once per
// line and let the frame loop follow the sprite itself.
let reportedHotspots = "";

function reportHotspots() {
  const rects = [];
  for (const view of views.values()) {
    if (view.hotspot) rects.push(view.hotspot);
    if (view.quickHotspot) rects.push(view.quickHotspot);
  }
  const serialized = JSON.stringify(rects);
  if (serialized === reportedHotspots) return;
  reportedHotspots = serialized;
  window.__TAURI__.core.invoke("overlay_hotspots", { rects }).catch((err) => {
    console.error("overlay_hotspots", err);
  });
}

// An Instance that stopped arriving was dismissed. Its elements go with it.
// Dispose while it is still mapped: the composing report scans views, so a
// gone Instance must not leave its caret held.
function removeView(id) {
  const view = views.get(id);
  if (!view) return;
  view.bubbles.hideAllNow();
  view.quickMachine.dispose();
  view.sprite.remove();
  view.bubble.remove();
  view.quick.remove();
  view.cueLayer.remove();
  views.delete(id);
  // After the view is gone, so a scan cannot still name it. dispose already
  // dropped the caret; this is the report that clears a stale id.
  reportComposing();
  reportQmVisible();
}

function drawView(view, now) {
  const { latest } = view;
  const at = view.previous ? interpolate(view.previous, latest, now) : latest;
  const spriteX = Math.round(at.x);
  const spriteY = Math.round(at.y);

  // Whole pixels, so an integer-scaled sprite is not resampled onto a fractional
  // grid by the compositor (ADR-0006). `mirror` comes from the Shell, not the
  // heading: a Character that draws its own left strip is sent as authored.
  view.sprite.style.transform = `translate(${spriteX}px, ${spriteY}px) scaleX(${latest.mirror})`;

  // Carried on every frame rather than announced on change: a change announced
  // while this file was still fetching its art is a change nobody heard.
  // Rewriting the same values restarts no transition; the hotkey sends zero.
  view.sprite.style.transition = `opacity ${latest.fade_ms}ms linear`;
  view.sprite.style.opacity = latest.visible ? "1" : "0";

  // Character presence uses fade_ms; speech/thinking keep the CSS fade so a
  // line does not inherit a 0ms or multi-second presence transition.
  if (!latest.visible) {
    view.bubble.style.transition = `opacity ${latest.fade_ms}ms linear`;
    view.bubble.style.opacity = "0";
    // A control nobody can see is not one to click, and a fading bubble is
    // still `.visible` — so this is cleared here as well as in `hide`.
    view.hotspot = null;
    view.paintedRect = null;
    reportAllPaintedRects();
    if (view.quickMachine.visible) view.quickMachine.dismiss();
    if (latest.fade_ms === 0) {
      view.bubbles.hideAllNow();
    }
  } else {
    view.bubble.style.transition = "";
    view.bubble.style.opacity = "";
  }

  // Bubble decisions before placement so this frame's show/hide is what
  // speechRect and the pill offset see (show/hide also place when idle).
  view.bubbles.frame(latest);

  const spriteOnDisplay = spriteX >= 0 && spriteX < window.innerWidth &&
                          spriteY >= 0 && spriteY < window.innerHeight;

  if (latest.visible && spriteOnDisplay) {
    const rect = { x: spriteX, y: spriteY, width: latest.width, height: latest.height };
    if (view.bubble.classList.contains("visible")) {
      positionBubble(view, rect, currentDisplayBounds());
    }
    if (view.quickMachine.visible) {
      positionQuick(view, rect);
    }
  }

  const placement = `${latest.character} ${latest.animation}#${latest.frame_index} ${latest.width}x${latest.height}`;
  if (placement === view.drawn) {
    return;
  }
  view.drawn = placement;

  const art = characters[latest.character];
  if (view.character !== latest.character) {
    view.character = latest.character;
    // The Character Manifest's render_mode: smooth art asks the compositor to
    // filter when scaling, where pixel art (main.css's default) must not.
    view.sprite.style.imageRendering = art?.smooth ? "auto" : "";
  }

  const src = art?.art[latest.animation]?.[latest.frame_index];
  if (src) {
    view.sprite.src = src;
  }
  view.sprite.style.width = `${latest.width}px`;
  view.sprite.style.height = `${latest.height}px`;
  view.sprite.dataset.animation = latest.animation;
  view.sprite.dataset.frameIndex = latest.frame_index;
  view.sprite.style.visibility = "visible";
}

// Armed rather than looping. `draw` used to re-arm itself at the top of every
// frame, so the overlay asked for a display frame at panel refresh forever,
// whatever the sprite was doing. The asking is what costs, and not in this
// process: WebKit runs a CVDisplayLink per display in the host, for as long as
// a page wants frames, and #741 measured those threads at half an idle character's
// wakeups. Every arrival that says something new arms this again, so a placement
// is still drawn the frame after it lands, and a resend of one is not.
//
// Arming is per display too. Every overlay hears about every Instance in its
// own coordinates, so a Character on a seam stays whole, which also means a
// placement landing here says nothing about whether this display has work (#764).
let armed = false;

// A pixel would cover what two displays at different scale factors round apart
// at a seam. Eight costs nothing: a sprite that close to the edge straddles it
// and arms both overlays anyway.
const SEAM_MARGIN = 8;

function reportAllPaintedRects() {
  reportPaintedRects(views, window.__TAURI__.core.invoke);
}


function needsFrame(view) {
  return onDisplay(view.previous, view.latest, currentDisplayBounds(), SEAM_MARGIN);
}

function arm() {
  if (armed) return;
  armed = true;
  requestAnimationFrame(draw);
}

function draw(now) {
  armed = false;
  for (const view of views.values()) {
    if (!view.latest) continue;
    drawView(view, now);
    if (!arrived(view.previous, view.latest, now) && needsFrame(view)) {
      arm();
    }
  }
  reportHotspots();
  if (cadence) noteCadence(now);
}

// Null unless FIDGET_TRACE_CADENCE is on. Each display frame as Unix ms, with
// whether it asked for the next one and the two arrivals it drew between, for
// scripts/frame-cadence.mjs. Sent once a second, and only after a frame.
let cadence = null;

function noteCadence(now) {
  // ponytail: the first Instance only, which is all the bench launches.
  const view = views.values().next().value;
  if (!view?.latest) return;
  const origin = performance.timeOrigin;
  cadence.push([
    origin + now,
    armed,
    view.previous ? origin + view.previous.at : null,
    origin + view.latest.at,
  ]);
  if (cadence.length === 1) setTimeout(flushCadence, 1000);
}

function flushCadence() {
  const frames = cadence;
  cadence = [];
  window.__TAURI__.core.invoke("overlay_cadence", { frames }).catch((err) => {
    console.error("overlay_cadence", err);
  });
}

async function start() {
  characters = (await window.__TAURI__.core.invoke("character")).characters;
  // Caught, because the overlay is worth more than the control: an unanswered
  // question leaves `clickableOffArt` false, where an uncaught reject would
  // abort the rest of `start` and there would be no sprite at all.
  try {
    clickableOffArt = await window.__TAURI__.core.invoke(
      "overlay_hit_tests_hotspots",
    );
  } catch (err) {
    console.error("overlay_hit_tests_hotspots", err);
  }
  try {
    if (await window.__TAURI__.core.invoke("overlay_traces_cadence")) cadence = [];
  } catch (err) {
    console.error("overlay_traces_cadence", err);
  }

  // One overlay per display, each told every sprite in its own coordinates. A
  // listener with no target is an `Any` listener that tauri hands every emit,
  // so without this each display would draw whichever overlay's frame came last.
  const overlay = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

  // Same event Chat's composer freezes on. Addressed here too, because
  // emit_to the Chat label does not reach this window, and Chat may not
  // be open when the harness settles.
  await window.__TAURI__.event.listen(
    "chat-opening",
    ({ payload }) => {
      noteQuickOpening(payload);
    },
    { target: overlay.label },
  );

  await window.__TAURI__.event.listen(
    "frame",
    ({ payload }) => {
      const seen = new Set();

      payload.sprites.forEach((sprite, index) => {
        seen.add(sprite.id);
        let view = views.get(sprite.id);
        if (!view) {
          view = createView(sprite.id);
          views.set(sprite.id, view);
        }

        // Stacked in the order sent, last in front, which is what
        // `input::press_target` picks too. A bubble sits above its own sprite so
        // a line clamped onto the head near a display's top stays readable.
        view.bubble.style.zIndex = `${index * 2 + 1}`;
        view.quick.style.zIndex = `${index * 2 + 2}`;
        view.sprite.style.zIndex = `${index * 2}`;
        view.cueLayer.style.zIndex = `${index * 2}`;

        // One overlay owns each Instance's bubble (`bubble_owner`), and the
        // Shell sends the line, the indicator and the cue to that one only.
        // Speech hides on its reading timer, not on the next frame, so the
        // overlay that just lost ownership drops its bubble once, on the change.
        // However, thinking and pending AI turns must survive ownership changes:
        // a character crossing a seam mid-turn would drop the reply otherwise.
        if (!sprite.bubble && view.latest?.bubble !== false) view.bubbles.hideButKeepTurn();

        const placement = {
          ...sprite,
          visible: payload.visible,
          fade_ms: payload.fade_ms,
          // Whether a cue may be heard as well as seen. Settings and Do Not
          // Disturb decide; this only obeys.
          sound: payload.sound,
        };
        // One sprite's share of what the Shell compares before sending, so its
        // resend of an unchanged Placement is unchanged here too.
        const told = JSON.stringify(placement);
        const changed = told !== view.told;
        view.told = told;
        view.previous = view.latest;
        view.latest = { ...placement, at: performance.now() };
        // Dialogue rides one tick and `latest` keeps only the newest placement,
        // so an Engine outpacing the display would lose pulses before `draw`
        // read them. The machine latches the pulse here, where every delivery is seen.
        view.bubbles.event(sprite);
        // A cue is the same shape of pulse, latched in the same place. It reads
        // `latest` rather than `sprite` for the two answers that belong to the
        // desktop: whether the Character is on screen, and whether it may be heard.
        view.cues.event(view.latest);
        view.quickMachine.setChatOpen(sprite.chatting);
        notePointerLeft(view);

        if (changed && needsFrame(view)) arm();
      });

      // An id that stopped arriving was dismissed, so its elements go. The Rust
      // side must never drop a live Instance from the list to mean anything
      // else: a sprite taken away here loses its bubble and its interpolation.
      for (const id of [...views.keys()]) {
        if (!seen.has(id)) {
          removeView(id);
        }
      }
    },
    { target: overlay.label },
  );

  // Pill handoff: when bubble_owner changes and this overlay is the new owner
  // with QM already open, reopen the pill with the transferred state.
  await window.__TAURI__.event.listen(
    "qm-handoff",
    ({ payload }) => {
      const view = views.get(payload.instance);
      if (!view) return;

      // Old owner closes its pill without clearing the shared state.
      // New owner reopens with the text and focus state.
      if (payload.open) {
        view.quickMachine.setText(payload.text);
        view.quickMachine.show();
        if (payload.focused) {
          view.quickMachine.requestFocus();
        }
        syncQuick(view);

        if (window.__TAURI__ && window.__TAURI__.core) {
          window.__TAURI__.core.invoke("overlay_request_focus").catch((err) => {
            console.error("overlay_request_focus after handoff", err);
          });
        }
      }
    },
    { target: overlay.label },
  );

  // Pill dismiss: when bubble_owner changes and this overlay is the old owner,
  // dismiss the local pill DOM without clearing backend state (new owner has it).
  await window.__TAURI__.event.listen(
    "qm-dismiss",
    ({ payload }) => {
      const view = views.get(payload.instance);
      if (view && view.quickMachine.visible) {
        view.quickMachine.hide();
      }
    },
    { target: overlay.label },
  );

  arm();

  // Received only while click-through is off, over the art; the Rust side's
  // button poll has been seen to miss that press. Last write wins: two in-flight
  // invokes can finish out of order, and an up before its down sticks the latch.
  const reporter = (command) => {
    let inflight = false;
    let queued = null;
    const send = (down) => {
      inflight = true;
      window.__TAURI__.core
        .invoke(command, { down })
        .catch((err) => {
          console.error(command, err);
        })
        .finally(() => {
          inflight = false;
          if (queued !== null) {
            const next = queued;
            queued = null;
            send(next);
          }
        });
    };
    return (down) => {
      if (inflight) {
        queued = down;
        return;
      }
      send(down);
    };
  };
  const reportPrimary = reporter("overlay_primary");
  const reportSecondary = reporter("overlay_secondary");
  // The webview's own menu is a browser menu. Ours is native and opened
  // from Verb::Menu once the latch above is seen.
  document.addEventListener("contextmenu", (event) => {
    event.preventDefault();
  });
  petDrag = null;
  gestureActive = false;
  document.addEventListener("pointerdown", (event) => {
    const composer = event.target.closest?.(".quick-message");
    const sprite = event.target.closest?.(".sprite");
    if (!composer && !sprite) {
      for (const view of views.values()) view.quickMachine.outside();
      petDrag = null;
      gestureActive = false;
      return;
    }
    const where = composer ? "composer" : "character";
    let reachPet = views.size === 0;
    let owner = null;
    for (const view of views.values()) {
      const hit = (composer && view.quick === composer) || (sprite && view.sprite === sprite);
      if (!hit) continue;
      owner = view;
      // A press on the composer must not capture the pointer or the pet gets the Poke.
      view.quickMachine.press(where, () => {
        reachPet = true;
      });
      break;
    }
    if (where === "character" && event.button === 0 && owner) {
      petDrag = { x: event.clientX, y: event.clientY, id: event.pointerId };
      gestureActive = true;
    } else {
      petDrag = null;
      gestureActive = false;
    }
    if (!reachPet) return;
    if (event.button === 0) {
      event.target.setPointerCapture?.(event.pointerId);
      reportPrimary(true);
    } else if (event.button === 2) {
      reportSecondary(true);
    }
  });
  document.addEventListener("pointermove", (event) => {
    if (!petDrag || event.pointerId !== petDrag.id) return;
    if (!crossedDrag(event.clientX - petDrag.x, event.clientY - petDrag.y)) return;
    petDrag = null;
    for (const view of views.values()) view.quickMachine.drag();
  });
  document.addEventListener("dblclick", (event) => {
    if (!event.target.closest?.(".sprite")) return;
    // Summon opens Chat. The composer yields so the two are not up together.
    for (const view of views.values()) view.quickMachine.summon();
  });
  window.addEventListener("blur", () => {
    for (const view of views.values()) view.quickMachine.outside();
  });
  document.addEventListener("pointerup", (event) => {
    if (event.button === 0) {
      petDrag = null;
      gestureActive = false;
      reportPrimary(false);
    } else if (event.button === 2) {
      reportSecondary(false);
    }
  });
  document.addEventListener("pointercancel", () => {
    petDrag = null;
    gestureActive = false;
    reportPrimary(false);
    reportSecondary(false);
  });
}

start().catch((err) => {
  // No art or no frames means nothing to draw and nothing to hit-test, so say
  // so loudly rather than showing an empty overlay that looks like a hung app.
  console.error("fidget could not draw the Character:", err);
});
