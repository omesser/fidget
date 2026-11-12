// The bindings card and the charge bar. The page paints this. The Shell
// decides whether the sprite is possessed, whether focus has left, and how
// full the bar is.

import { crossedDrag } from "./quick-message.js";

const POSSESSION_HEADING = "Manually drive the fidget on your screen";

const PAUSED_HEADING = "Paused";

const FOCUS_PRESSED_LABEL = "Focused";

const FOCUS_RAISED_LABEL = "Focus";

// Both states name the action, since the visible label is one word.
const FOCUS_ARIA_LABEL = "Click to focus";

// The button carries the Esc hint, so the bindings list does not repeat it.
const RELEASE_LABEL = "Release Possession (Esc)";

// Clear of the screen edge, on the display the sprite stands on. The card
// stays put while the sprite walks, so the background under it stays visible.
const POSSESSION_INSET = 16;

// Digit rows are the list the Shell already mapped, in that order. The card
// only numbers them.
const KEYS = [
  ["← →", "Walk"],
  ["↑ ↓", "Climb"],
  ["↓", "Drop"],
  ["Space", "Jump"],
];

// Walk and Jump are written as labels. An Animation name is a key (`nod-off`).
// A merged digit already joins names with `/`; each name is capitalized.
function animationLabel(name) {
  if (!name) return name;
  return name
    .split("/")
    .map((part) => (part ? part.charAt(0).toUpperCase() + part.slice(1) : part))
    .join("/");
}

function bindingRows(digits) {
  const rows = [...(digits ?? [])].map((name, index) => [
    String(index + 1),
    animationLabel(name),
  ]);
  return [...KEYS, ...rows];
}

// Null while the sprite is not possessed. Paused keeps the card and replaces
// the explanation with the paused line. The whole paused card is the resume
// click, so it has no release button. The focus button shows which it is.
export function possessionCard({ possessed, paused, digits }) {
  if (!possessed) return null;
  const keys = bindingRows(digits);
  if (paused) {
    return {
      paused: true,
      dimmed: true,
      heading: PAUSED_HEADING,
      keys,
      release: null,
      focus: { label: FOCUS_RAISED_LABEL, pressed: false },
    };
  }
  return {
    paused: false,
    dimmed: false,
    heading: POSSESSION_HEADING,
    keys,
    release: RELEASE_LABEL,
    focus: { label: FOCUS_PRESSED_LABEL, pressed: true },
  };
}

// Null hides the bar. A number is the fill, clamped to 0..1.
export function chargeFill(charge) {
  if (charge == null || Number.isNaN(charge)) return null;
  return Math.min(1, Math.max(0, charge));
}

// The whole card takes a press, since its background is the drag handle. The
// bar is only drawn.
export function possessionRects(card, bar) {
  const rects = [];
  if (card) rects.push({ rect: card, clickable: true });
  if (bar) rects.push({ rect: bar, clickable: false });
  return rects;
}

function row(keys, does) {
  const item = document.createElement("li");
  const name = document.createElement("span");
  name.className = "possession-keys";
  name.textContent = keys;
  const what = document.createElement("span");
  what.textContent = does;
  item.append(name, what);
  return item;
}

// One card element keeps its last model. A charge tick must not rebuild it.
const painted = new WeakMap();

function modelStamp(model) {
  if (!model) return "";
  return JSON.stringify([Boolean(model.paused), model.heading, model.keys, model.release, model.focus]);
}

function focusButton({ label, pressed }) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "possession-focus";
  button.setAttribute("aria-pressed", pressed);
  button.setAttribute("aria-label", FOCUS_ARIA_LABEL);
  const icon = document.createElement("span");
  icon.className = "possession-focus-icon";
  const text = document.createElement("span");
  text.className = "possession-focus-label";
  text.textContent = label;
  button.append(icon, text);
  return button;
}

// The background is the drag handle. Only a press that never became a drag
// resumes a paused card, so moving it leaves the keyboard where it is. A press
// here is not a press on the art, so it must not poke.
export function wirePossessionCard(card, invoke, drag) {
  const report = (command) => (err) => console.error(command, err);
  const focus = () => invoke("overlay_request_focus").catch(report("overlay_request_focus"));
  let press = null;
  card.addEventListener("pointerdown", (event) => {
    event.stopPropagation();
    if (event.target.closest(".possession-focus")) {
      focus();
      return;
    }
    if (event.button > 0 || event.target.closest("button")) return;
    press = { id: event.pointerId, x: event.clientX, y: event.clientY, dragging: false };
    card.setPointerCapture?.(event.pointerId);
  });
  card.addEventListener("pointermove", (event) => {
    if (!press || event.pointerId !== press.id) return;
    const dx = event.clientX - press.x;
    const dy = event.clientY - press.y;
    if (!press.dragging && !crossedDrag(dx, dy)) return;
    press.dragging = true;
    drag.move(dx, dy);
  });
  const end = (event, clicked) => {
    if (!press || event.pointerId !== press.id) return;
    const { dragging } = press;
    press = null;
    if (dragging) drag.drop();
    else if (clicked && card.classList.contains("paused")) focus();
  };
  card.addEventListener("pointerup", (event) => {
    event.stopPropagation();
    end(event, true);
  });
  // A drop that never arrives would leave the whole display taking clicks.
  for (const type of ["pointercancel", "lostpointercapture"]) {
    card.addEventListener(type, (event) => end(event, false));
  }
  // The release button presses Esc, so Possession ends on the path the key takes.
  card.addEventListener("click", (event) => {
    event.stopPropagation();
    if (!event.target.closest(".possession-release")) return;
    for (const down of [true, false]) {
      invoke("overlay_possession_key", { key: "escape", down }).catch(report("overlay_possession_key"));
    }
  });
}

// Paint `model` into `card`. `bar` is the track. `fill` is from `chargeFill`.
// Returns whether the card's nodes were rebuilt.
export function paintPossession(card, bar, model, fill) {
  const stamp = modelStamp(model);
  const rebuilt = painted.get(card) !== stamp;
  if (rebuilt) {
    painted.set(card, stamp);
    card.classList.toggle("visible", Boolean(model));
    card.classList.toggle("paused", Boolean(model?.paused));
    card.replaceChildren();
    if (model) {
      const heading = document.createElement("p");
      heading.className = "possession-heading";
      heading.textContent = model.heading;
      card.append(heading);
      if (!model.paused) {
        const list = document.createElement("ul");
        for (const [keys, does] of model.keys) list.append(row(keys, does));
        card.append(list);
      }
      const footer = document.createElement("div");
      footer.className = "possession-footer";
      if (model.release) {
        const release = document.createElement("button");
        release.type = "button";
        release.className = "possession-release";
        release.textContent = model.release;
        footer.append(release);
      }
      footer.append(focusButton(model.focus));
      card.append(footer);
    }
  }
  const shown = fill != null && model && !model.paused;
  bar.classList.toggle("visible", shown);
  const inner = bar.querySelector(".possession-charge-fill");
  if (inner) inner.style.width = shown ? `${fill * 100}%` : "0";
  return rebuilt;
}

// The usable area when the shell has named one. The overlay frame includes
// the menu bar, and a card inset from that frame covers the Apple menu.
export function possessionBounds(work, frame) {
  if (work && work.width > 0 && work.height > 0) return work;
  return frame;
}

// Top-left of `bounds`, the usable area of the display the sprite is on, or
// `at` once the card was dragged there. Where the sprite stands on that display
// does not move the card.
export function placePossession(cardSize, barHeight, gap, bounds, at) {
  const x = Math.round(at?.x ?? bounds.x + POSSESSION_INSET);
  const y = Math.round(at?.y ?? bounds.y + POSSESSION_INSET);
  return {
    card: { x, y },
    bar: {
      x,
      y: Math.round(y + cardSize.height + gap),
      width: cardSize.width,
      height: barHeight,
    },
  };
}

// Where a drag of (`dx`, `dy`) from `from` drops a card of `size`, kept whole
// inside `bounds`.
export function draggedTo(from, dx, dy, size, bounds) {
  const clamp = (value, low, span) => Math.min(Math.max(value, low), low + Math.max(0, span));
  return {
    x: clamp(from.x + dx, bounds.x, bounds.width - size.width),
    y: clamp(from.y + dy, bounds.y, bounds.height - size.height),
  };
}
