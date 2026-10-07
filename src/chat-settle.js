// What a `chat` payload does to a waiting caret. Its own module because
// chat.js reaches window.__TAURI__ as it loads and cannot be imported outside
// a webview; this can, so it has a test.

export const MISSING_ANSWER = "No answer came back.";

// Keyed by the words `director::happened_cell` writes. The cause leads the
// sentence because "your question was dropped" reads as a bug, where "you
// poked me before that answer landed" reads as what the user just did.
const PREEMPTED_BY = {
  "spoken to": "You asked something else",
  poked: "You poked me",
  thrown: "You threw me",
  summoned: "You summoned me",
  grabbed: "You picked me up",
  perched: "I perched",
  proactive: "My next thought started",
};

export function preemptedNote(cause) {
  const clause = PREEMPTED_BY[cause] ?? "Something else started";
  return `${clause} before that answer landed, so I dropped it.`;
}

export function createChatTurns() {
  const waiting = [];
  let proactivePending = null;

  return {
    typed() {
      const turn = { alreadyHasSpeechAhead: false };
      if (proactivePending && waiting.includes(proactivePending)) {
        const at = waiting.indexOf(proactivePending);
        waiting.splice(at, 1);
        proactivePending = null;
      }
      waiting.push(turn);
      return turn;
    },

    drop(turn) {
      const at = waiting.indexOf(turn);
      if (at >= 0) {
        waiting.splice(at, 1);
      }
      if (turn === proactivePending) {
        proactivePending = null;
      }
    },

    popNewest() {
      const turn = waiting.pop();
      if (turn === proactivePending) {
        proactivePending = null;
      }
      return turn;
    },

    newest() {
      return waiting.at(-1) ?? null;
    },

    clear() {
      waiting.length = 0;
      proactivePending = null;
    },

    settle(payload) {
      if (payload.streaming) {
        const turn = waiting.at(-1) ?? null;
        if (!turn) {
          if (!proactivePending) {
            proactivePending = { alreadyHasSpeechAhead: false, isProactive: true };
            waiting.push(proactivePending);
          }
          proactivePending.alreadyHasSpeechAhead = true;
          return { action: "speech", turn: proactivePending, said: payload.said ?? "" };
        }
        turn.alreadyHasSpeechAhead = true;
        return { action: "speech", turn, said: payload.said };
      }
      const turn = waiting.at(-1) ?? null;
      if (!turn) {
        return { action: "orphan", turn: null, said: payload.said ?? "" };
      }
      waiting.pop();
      if (turn === proactivePending) {
        proactivePending = null;
      }
      if (payload.said) {
        for (const leftover of waiting) {
          leftover.alreadyHasSpeechAhead = true;
        }
        return { action: "speech", turn, said: payload.said, reacting_to: payload.reacting_to };
      }
      if (payload.failure) {
        return { action: "failure", turn, said: payload.failure };
      }
      if (payload.error) {
        return { action: "error", turn, note: payload.error };
      }
      if (payload.superseded_by) {
        return { action: "preempted", turn, note: preemptedNote(payload.superseded_by) };
      }
      if (turn.alreadyHasSpeechAhead) {
        return { action: "silent", turn };
      }
      return { action: "missing", turn, note: MISSING_ANSWER };
    },
  };
}
