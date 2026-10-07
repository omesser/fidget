// Hover-to-compose above a Character. The overlay owns the DOM; this decides
// when the composer is up and whether a press still belongs to the pet.

import { canAnswer, composerPlaceholder } from "./chat-connect.js";

export const CONNECT_PROMPT = "Connect an AI to talk to me";
// Shorter than the prompt, so the hint and Open chat fit on one line.
export const CONNECT_HINT = "No AI connected yet.";

// Every unavailable state is fixed in Chat's landing, so the pill links there.
// A Harness still starting clears on its own and keeps Chat's sentence.
export function quickMessageConnects(opening) {
  return !canAnswer(opening) && !opening?.harness?.initializing;
}

export function quickMessagePrompt(opening) {
  if (canAnswer(opening)) return "talk to me";
  if (quickMessageConnects(opening)) return CONNECT_PROMPT;
  return composerPlaceholder(opening);
}

// `field` and `send` are the overlay's controls. `machine` is this module.
// One opening, the same one Chat paints from.
// `link` is absent where the overlay cannot take a click off the art.
export function applyQuickMessageGate({ field, send, link, machine }, opening) {
  const ready = canAnswer(opening);
  // Enable before the machine claims the caret. focus() on a still-disabled
  // field is dropped, and the pill would thaw with nowhere to type.
  field.disabled = !ready;
  send.disabled = !ready;
  field.placeholder = quickMessagePrompt(opening);
  field.setAttribute("aria-label", ready ? "Quick message" : field.placeholder);
  if (link) link.hidden = !quickMessageConnects(opening);
  // A disabled Send beside the link would read as a second, dead control.
  send.hidden = Boolean(link) && !link.hidden;
  machine.setAvailable(ready);
}

// The mirror is what gives the pill its width. An empty draft mirrors a
// hairline, which clips the unavailable sentence under overflow: hidden.
export function quickMessageMirror(text, prompt, available) {
  if (text) return `${text}\u200b`;
  if (!available && prompt) return `${prompt}\u200b`;
  return "\u200b";
}

export const HOVER_DELAY_MS = 1500;

// Auto-hide: 3s continuous away from both sprite and pill. Re-entering resets.
export const AUTO_HIDE_DELAY_MS = 3000;

// While Speech or thinking is up the Character has the floor, so an empty pill
// the pointer left gives way. This is only long enough to cross onto the pill.
export const BUBBLE_YIELD_MS = 1000;

// A click that stays put is a poke. Past this, the same press is a drag.
export const DRAG_DISMISS_PX = 4;

export function crossedDrag(dx, dy) {
  return dx * dx + dy * dy >= DRAG_DISMISS_PX * DRAG_DISMISS_PX;
}

export function createQuickMessage({ schedule, clear, send, onChange, available = true, onDragDismiss }) {
  let visible = false;
  let text = "";
  let focused = false;
  let claimFocus = false;
  let disposed = false;
  let hoverTimer = null;
  let autoHideTimer = null;
  let overSprite = false;
  let overPill = false;
  // The overlay re-reports the hover every tick, so without this the dwell
  // re-arms under the cursor that just opened Chat.
  let yielded = false;
  // Chat is the composer while it is up. A level, told by every frame.
  let chatOpen = false;
  let bubbleUp = false;
  let ready = available;

  function changed() {
    if (!disposed) onChange?.();
  }

  function cancelHover() {
    if (hoverTimer === null) return;
    clear(hoverTimer);
    hoverTimer = null;
  }

  function cancelAutoHide() {
    if (autoHideTimer === null) return;
    clear(autoHideTimer);
    autoHideTimer = null;
  }

  function hasText() {
    return text.trim().length > 0;
  }

  function startAutoHideIfNeeded() {
    if (disposed || !visible || hasText() || focused || overSprite || overPill) return;
    if (autoHideTimer !== null) return;
    autoHideTimer = schedule(() => {
      autoHideTimer = null;
      if (disposed || !visible || hasText() || focused || overSprite || overPill) return;
      hide();
    }, bubbleUp ? BUBBLE_YIELD_MS : AUTO_HIDE_DELAY_MS);
  }

  function hide() {
    cancelHover();
    cancelAutoHide();
    const was = visible;
    visible = false;
    focused = false;
    claimFocus = false;
    if (was) changed();
  }

  function hideWithoutReport() {
    cancelHover();
    cancelAutoHide();
    visible = false;
    focused = false;
    claimFocus = false;
  }

  function show() {
    if (visible || disposed) return;
    visible = true;
    // The caret is the typing hold. A frozen pill is a status, so claiming
    // it would stop the walk for a field that takes nothing.
    if (ready) {
      focused = true;
      claimFocus = true;
    }
    changed();
  }

  function dismissOpen() {
    // A click away also abandons a dwell that has not opened yet, so the
    // pill cannot appear after the pointer has already gone.
    cancelHover();
    if (!visible) return;
    text = "";
    hide();
  }

  function dismissOpenWithDragLatch() {
    cancelHover();
    if (!visible) return;
    // Set drag latch BEFORE clearing state
    if (onDragDismiss) onDragDismiss();
    text = "";
    hide();
  }

  function submit() {
    const line = text.trim();
    if (!ready || !visible || !line) return false;
    text = "";
    send(line);
    hide();
    return true;
  }

  return {
    get visible() {
      return visible;
    },
    get text() {
      return text;
    },
    get available() {
      return ready;
    },
    get typing() {
      return visible && focused && ready;
    },
    takeFocus() {
      if (!claimFocus) return false;
      claimFocus = false;
      return true;
    },
    enterSprite() {
      if (disposed) return;
      overSprite = true;
      cancelAutoHide();
      if (yielded || chatOpen || visible || hoverTimer !== null) return;
      hoverTimer = schedule(() => {
        hoverTimer = null;
        if (disposed || !overSprite || chatOpen) return;
        show();
      }, HOVER_DELAY_MS);
    },
    leaveSprite() {
      overSprite = false;
      yielded = false;
      cancelHover();
      startAutoHideIfNeeded();
    },
    enterPill() {
      if (disposed) return;
      overPill = true;
      cancelAutoHide();
    },
    leavePill() {
      overPill = false;
      startAutoHideIfNeeded();
    },
    // A pill already up stays: it may hold a draft, and auto-hide takes an empty one.
    setChatOpen(open) {
      chatOpen = Boolean(open);
      if (chatOpen) cancelHover();
    },
    setBubble(up) {
      if (up === bubbleUp) return;
      bubbleUp = up;
      if (!up) return;
      cancelAutoHide();
      startAutoHideIfNeeded();
    },
    setAvailable(next) {
      if (disposed || next === ready) return;
      ready = next;
      if (!ready) {
        // The placeholder is the only place this pill can say why. A draft
        // would hide that sentence.
        text = "";
        focused = false;
        claimFocus = false;
      } else if (visible) {
        focused = true;
        claimFocus = true;
      }
      changed();
    },
    setText(value) {
      if (!ready) return;
      text = value;
      if (hasText()) {
        cancelAutoHide();
      } else {
        startAutoHideIfNeeded();
      }
    },
    focus() {
      if (!ready || focused) return;
      focused = true;
      if (visible) changed();
    },
    blur() {
      if (!focused) return;
      focused = false;
      startAutoHideIfNeeded();
      if (visible) changed();
    },
    keydown(key, mods = {}) {
      if (!visible || !ready) return false;
      if (key === "Enter" && !mods.shiftKey && !mods.composing) return submit();
      return false;
    },
    // A press on the composer is text, not a Poke. The character still is.
    press(where, report) {
      if (where === "composer") return;
      report();
    },
    outside: dismissOpen,
    drag() {
      // Always set drag latch when dragging so handoff can fire on ownership change.
      // Typed/focused pills don't dismiss locally (they follow), but empty unfocused pills do.
      if (visible && onDragDismiss) onDragDismiss();
      
      if (!hasText() && !focused) {
        text = "";
        hide();
      }
    },
    summon() {
      yielded = true;
      dismissOpen();
    },
    submit,
    restore(value) {
      if (disposed) return;
      visible = true;
      if (!ready) {
        // A refused send had already hidden the pill. Bring it back so the
        // unavailable sentence is on screen, not the line that could not go.
        text = "";
        focused = false;
        claimFocus = false;
        changed();
        return;
      }
      text = value;
      focused = true;
      claimFocus = true;
      changed();
    },
    dismiss() {
      text = "";
      hide();
    },
    hideWithoutReport,
    dispose() {
      // Tell the overlay while this Instance is still mapped. The composing
      // report scans views, and a removed one must not leave its caret held.
      cancelHover();
      cancelAutoHide();
      text = "";
      visible = false;
      focused = false;
      claimFocus = false;
      if (!disposed) onChange?.();
      disposed = true;
    },
  };
}

// Same seat as Speech. When that bubble is already there, step clear of it
// so a hover does not cover the line the Character is saying.
export function placeQuickMessage(spriteRect, size, bounds, speechRect) {
  const spriteCenterX = spriteRect.x + spriteRect.width / 2;
  const gap = 10;

  let x = spriteCenterX - size.width / 2;
  let y = spriteRect.y - size.height - gap;

  const wouldClampToTop = y < bounds.y;
  const clampedY = bounds.y;
  const wouldCoverSprite = wouldClampToTop && (clampedY + size.height > spriteRect.y);

  if (wouldCoverSprite) {
    y = spriteRect.y + spriteRect.height + gap;
  }

  if (speechRect) {
    const overlaps =
      x < speechRect.x + speechRect.width &&
      x + size.width > speechRect.x &&
      y < speechRect.y + speechRect.height &&
      y + size.height > speechRect.y;
    if (overlaps) {
      y = speechRect.y - size.height - gap;
      if (y < bounds.y) y = speechRect.y + speechRect.height + gap;
    }
  }

  const inverted = y > spriteRect.y + spriteRect.height / 2;
  const tailOffset = spriteCenterX - (x + size.width / 2);
  return { x, y, tailOffset, inverted };
}
