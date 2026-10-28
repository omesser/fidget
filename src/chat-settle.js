
// What a `chat` payload does to a waiting caret. Split from chat.js, which
// reads `window.__TAURI__` at import, so node can load this.

export const MISSING_ANSWER = "No answer came back.";

// Keyed by the words `director::happened_cell` writes. The cause leads the
// sentence, because "your question was dropped" reads as a bug and naming
// the poke reads as what the user just did.
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
      // Attach notices are not turn outcomes. Handle them before waiting, or a
      // notice that lands after typed() steals the user's turn.
      if (payload.notice) {
        return { action: "notice", turn: null, note: payload.notice };
      }
      // A `speak` line is its own row. The turn it was said in still owes
      // its reply, so it must not be popped here. The line is speech that
      // turn already said, so a later empty reply stays quiet.
      if (payload.aside) {
        const turn = waiting.at(-1);
        if (turn) {
          turn.alreadyHasSpeechAhead = true;
        }
        return { action: "orphan", turn: null, said: payload.said ?? "" };
      }
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
