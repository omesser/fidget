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

  return {
    typed() {
      const turn = { alreadyHasSpeechAhead: false };
      waiting.push(turn);
      return turn;
    },

    drop(turn) {
      const at = waiting.indexOf(turn);
      if (at >= 0) {
        waiting.splice(at, 1);
      }
    },

    popNewest() {
      return waiting.pop();
    },

    newest() {
      return waiting.at(-1) ?? null;
    },

    clear() {
      waiting.length = 0;
    },

    settle(payload) {
      const turn = waiting.shift();
      if (!turn) {
        return { action: "orphan", turn: null, said: payload.said ?? "" };
      }
      if (payload.said) {
        for (const leftover of waiting) {
          leftover.alreadyHasSpeechAhead = true;
        }
        return { action: "speech", turn, said: payload.said };
      }
      // The Harness's own words: the diagnosis, drawn under its name.
      if (payload.failure) {
        return { action: "failure", turn, said: payload.failure };
      }
      // The Shell's line already names the Harness (`harness: ...`, `harness
      // not authenticated: ...`); a prefix here stacked a fourth (#991).
      if (payload.error) {
        return { action: "error", turn, note: payload.error };
      }
      // The Shell naming the wake is a fact about this settle, so it is read
      // first. `alreadyHasSpeechAhead` is only a shape heuristic, and all it
      // owes a leftover caret is silence instead of a wrong missing answer.
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
