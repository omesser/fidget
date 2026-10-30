
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

// The lines the bubble draws, and whether the turn ran past them. The flag
// exists because reading truncation off a trailing "…" would treat a line
// that ends in one as truncated.
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

// Decisions apart from the DOM, so node can drive ticks a display never
// reproduces. `event` latches a pulse the renderer would otherwise keep only
// the newest of. Dialogue holds out the indicator; only a silent ending min-holds.
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
  let thinkingPrev = false;
  let wasInterrupted = false;
  // Latches a quick-message send before the Engine flag rises, or `frame`
  // clears thinking. Cleared by dialogue, abandon, hide-all, or the flag falling.
  let aiTurnPending = false;
  // The Engine raised thinking for this turn. The flag falling after that
  // ends a turn that produced no line.
  let aiTurnSeen = false;

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

      // A hidden sprite speaks to nobody. The pulse is consumed, not queued,
      // or the line pops up when the sprite next fades in.
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

      // This overlay lost bubble ownership. A non-owner must not keep thinking
      // from a stale aiTurnStarted.
      if (placement.bubble === false && aiTurnPending) {
        aiTurnPending = false;
        if (thinkingShown) {
          hideThinkingNow();
        }
      }

      if (aiTurnPending && placement.thinking) aiTurnSeen = true;
      else if (aiTurnPending && aiTurnSeen) aiTurnPending = false;

      thinking = Boolean(placement.thinking && placement.visible) || aiTurnPending;
      const thinkingRose = thinking && !thinkingPrev;
      thinkingPrev = thinking;

      if (thinking) {
        if (!thinkingShown && !speechShowing) {
          // Show immediately when thinking rises on an ownership re-entry,
          // or cancelling the ellipsis flickers.
          if (thinkingRose && wasInterrupted && graceTimer === null) {
            thinkingShown = true;
            wasInterrupted = false;
            io.showThinking();
            if (minHoldTimer === null) {
              minHoldTimer = schedule(() => {
                minHoldTimer = null;
                if (!thinking) hideThinkingNow();
              }, THINKING_MIN_HOLD_MS);
            }
          } else if (graceTimer === null) {
            armGrace();
          }
        }
      } else {
        wasInterrupted = false;
        if (graceTimer !== null) {
          cancel(graceTimer);
          graceTimer = null;
        } else if (thinkingShown && minHoldTimer === null) {
          hideThinkingNow();
        }
      }
    },

    // Quick-message accepted a send. Show thinking now, with no grace.
    aiTurnStarted() {
      aiTurnPending = true;
      aiTurnSeen = false;
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

    // chat_send refused, or the gate froze. Abandon the turn this armed.
    aiTurnAbandoned() {
      aiTurnPending = false;
      thinking = false;
      hideThinkingNow();
    },

    // The hide hotkey's instant answer. Nothing may stay or come back.
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

    // Hide the Speech bubble and thinking, but keep the turn. Ownership moved mid-turn,
    // and the new owner must still show the incoming dialogue.
    hideButKeepTurn() {
      const hadThinking = thinking || graceTimer !== null || aiTurnPending;
      hideThinkingNow();
      if (hadThinking) {
        wasInterrupted = true;
      }
      thinkingPrev = false;
      if (speechTimer !== null) {
        cancel(speechTimer);
        speechTimer = null;
      }
      speechShowing = false;
      io.hideSpeech();
    },
  };
}

// Above the head. At the ceiling, when the clamp would cover the face, invert
// and put the bubble under the Character at the same mirrored distance.
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

// The CSS tail hangs below the body, or above it when inverted.
const TAIL_PX = 10;

// Where a readable bubble is drawn, tail included. Windows clips drawing to
// its region, so the tail has to be inside it. Hidden or mid-fade is null.
export function readableBubbleRect(bubble, pos) {
  if (!bubble?.classList.contains("visible")) return null;
  const opacity = parseFloat(bubble.style.opacity);
  if (opacity < 1) return null;
  const y = Math.round(pos.y) - (pos.inverted ? TAIL_PX : 0);
  return [Math.round(pos.x), y, bubble.offsetWidth, bubble.offsetHeight + TAIL_PX];
}
