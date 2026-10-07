// The bubble's arithmetic and decisions. main.js owns the DOM that draws them.

const MIN_DURATION_MS = 2000;
const MAX_DURATION_MS = 8000;
const BASE_DURATION_MS = 900;
const MS_PER_CHAR = 55;
const MAX_LINES = 6;

export function bubbleDuration(text) {
  const duration = BASE_DURATION_MS + text.length * MS_PER_CHAR;
  return Math.max(MIN_DURATION_MS, Math.min(MAX_DURATION_MS, duration));
}

// The lines the bubble draws, and whether the turn ran past them. The flag is
// why an object comes back: a truncated turn puts a control in the bubble, and
// reading it off the trailing "…" would call a line that ends in one truncated.
export function wrapText(text, maxWidth, measureFn) {
  const lines = [];
  let truncated = false;

  for (const paragraph of text.split("\n")) {
    if (lines.length >= MAX_LINES) {
      truncated = true;
      break;
    }

    let currentLine = "";
    for (const word of paragraph.split(" ")) {
      if (lines.length >= MAX_LINES) {
        truncated = true;
        break;
      }

      const testLine = currentLine ? `${currentLine} ${word}` : word;
      if (measureFn(testLine).width > maxWidth && currentLine) {
        lines.push(currentLine);
        currentLine = word;
      } else {
        currentLine = testLine;
      }
    }

    if (currentLine) {
      if (lines.length < MAX_LINES) {
        lines.push(currentLine);
      } else {
        truncated = true;
      }
    }
  }

  if (truncated) {
    lines[MAX_LINES - 1] = lines[MAX_LINES - 1].trimEnd() + "…";
  }

  return { lines, truncated };
}

export const THINKING_GRACE_MS = 250;
export const THINKING_MIN_HOLD_MS = 600;

// The bubble decisions, apart from the DOM, so node can drive the machine
// through tick orderings a display never reproduces on demand. Three rules:
// `dialogue` rides one Engine tick and the renderer keeps only the newest
// placement, so `event` latches the pulse per delivery. A response ends the
// thinking indicator the same frame it shows; the min-hold is for silent
// endings only. Speech wins over the indicator for its whole reading window,
// and a turn still in flight starts its grace when the line hides.
export function createBubbleMachine(io) {
  const schedule = io.schedule ?? ((fn, ms) => setTimeout(fn, ms));
  const cancel = io.cancel ?? ((id) => clearTimeout(id));

  let pendingDialogue = null;
  let pendingAsk = false;
  let speechTimer = null;
  let speechShowing = false;
  let graceTimer = null;
  let minHoldTimer = null;
  let thinkingShown = false;
  let thinking = false;
  // Latched when a quick-message send starts the AI turn; cleared by dialogue, abandon, or hide-all.
  // Without it, frame() would clear thinking before the Engine raises the flag.
  let aiTurnPending = false;

  function hideThinkingNow() {
    if (graceTimer !== null) {
      cancel(graceTimer);
      graceTimer = null;
    }
    if (minHoldTimer !== null) {
      cancel(minHoldTimer);
      minHoldTimer = null;
    }
    if (thinkingShown) {
      thinkingShown = false;
      io.hideThinking();
    }
  }

  function armGrace() {
    graceTimer = schedule(() => {
      graceTimer = null;
      if (!thinking || speechShowing) return;
      thinkingShown = true;
      io.showThinking();
      minHoldTimer = schedule(() => {
        minHoldTimer = null;
        if (!thinking) hideThinkingNow();
      }, THINKING_MIN_HOLD_MS);
    }, THINKING_GRACE_MS);
  }

  return {
    // Every delivered placement, straight from the event listener.
    event(placement) {
      if (placement.dialogue) pendingDialogue = placement.dialogue;
      if (placement.asking) pendingAsk = true;
    },

    // The newest placement, once per drawn frame.
    frame(placement) {
      const dialogue = pendingDialogue;
      const ask = pendingAsk && !dialogue;
      pendingDialogue = null;
      pendingAsk = false;

      // A hidden sprite speaks to nobody; the pulse is consumed, not queued,
      // or the line would pop up whenever the sprite next fades in.
      if ((dialogue || ask) && placement.visible) {
        aiTurnPending = false;
        hideThinkingNow();
        if (speechTimer !== null) cancel(speechTimer);
        speechShowing = true;
        if (ask) io.showAsk();
        else io.showSpeech(dialogue);
        // The pointer to Chat carries a control, so it gets the longest window.
        speechTimer = schedule(() => {
          speechTimer = null;
          speechShowing = false;
          io.hideSpeech();
          // Only now may a turn still in flight surface its indicator.
          if (thinking && graceTimer === null && !thinkingShown) armGrace();
        }, ask ? MAX_DURATION_MS : bubbleDuration(dialogue));
      }

      thinking = Boolean(placement.thinking && placement.visible) || aiTurnPending;
      if (thinking) {
        if (!thinkingShown && graceTimer === null && !speechShowing) {
          armGrace();
        }
      } else if (graceTimer !== null) {
        cancel(graceTimer);
        graceTimer = null;
      } else if (thinkingShown && minHoldTimer === null) {
        hideThinkingNow();
      }
    },

    // Quick-message accepted a send: the AI turn starts; show thinking now, no grace.
    // Cleared by dialogue/ask, aiTurnAbandoned, or hideAllNow — not by Engine thinking:false.
    aiTurnStarted() {
      aiTurnPending = true;
      thinking = true;
      if (speechTimer !== null) {
        cancel(speechTimer);
        speechTimer = null;
      }
      if (speechShowing) {
        speechShowing = false;
        io.hideSpeech();
      }
      if (graceTimer !== null) {
        cancel(graceTimer);
        graceTimer = null;
      }
      if (!thinkingShown) {
        thinkingShown = true;
        io.showThinking();
      }
      if (minHoldTimer === null) {
        minHoldTimer = schedule(() => {
          minHoldTimer = null;
          if (!thinking) hideThinkingNow();
        }, THINKING_MIN_HOLD_MS);
      }
    },

    // chat_send refused or the gate froze: abandon the AI turn we armed.
    aiTurnAbandoned() {
      aiTurnPending = false;
      thinking = false;
      hideThinkingNow();
    },

    // The hide hotkey's instant answer: nothing may stay or come back.
    hideAllNow() {
      aiTurnPending = false;
      hideThinkingNow();
      if (speechTimer !== null) {
        cancel(speechTimer);
        speechTimer = null;
      }
      speechShowing = false;
      pendingDialogue = null;
      pendingAsk = false;
      io.hideSpeech();
    },

    // Hide speech and thinking but preserve aiTurnPending and pendingDialogue.
    // Used when bubble ownership changes mid-turn: the old owner hides its
    // bubbles, but the new owner must still show the incoming dialogue.
    hideButKeepTurn() {
      hideThinkingNow();
      if (speechTimer !== null) {
        cancel(speechTimer);
        speechTimer = null;
      }
      speechShowing = false;
      io.hideSpeech();
    },
  };
}

// The bubble sits above the head. At the ceiling, when the clamp
// would cover the Character's face, invert: put the bubble under the Character
// at the same mirrored vertical distance.
export function placeBubble(spriteRect, bubbleSize, displayBounds) {
  const spriteCenterX = spriteRect.x + spriteRect.width / 2;
  const gap = 10;

  let x = spriteCenterX - bubbleSize.width / 2;
  let y = spriteRect.y - bubbleSize.height - gap;

  const wouldClampToTop = y < displayBounds.y;
  const clampedY = displayBounds.y;
  const wouldCoverSprite = wouldClampToTop && (clampedY + bubbleSize.height > spriteRect.y);

  if (wouldCoverSprite) {
    y = spriteRect.y + spriteRect.height + gap;
  }

  x = Math.max(displayBounds.x, Math.min(x, displayBounds.x + displayBounds.width - bubbleSize.width));
  y = Math.max(displayBounds.y, Math.min(y, displayBounds.y + displayBounds.height - bubbleSize.height));

  const tailOffset = spriteCenterX - (x + bubbleSize.width / 2);

  return { x, y, tailOffset, inverted: wouldCoverSprite };
}
