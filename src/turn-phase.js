// Silence for this long replaces the phase line until the next update.
// Tests read this, not a copied number of seconds.
export const STALL_MS = 30_000;

// ACP `in_progress` is the call doing work. `pending` has not started.
const RUNNING = new Set(["in_progress"]);

export function idlePhase() {
  return { open: false, calls: {}, order: [], lastAt: null };
}

export function reducePhase(state, event) {
  if (event.type === "close") {
    return idlePhase();
  }
  if (event.type === "open") {
    // A typed turn starts clean. An unprompted row keeps calls that arrived
    // before it existed.
    const base = event.fresh ? idlePhase() : state;
    return { ...base, open: true, lastAt: event.at };
  }
  if (event.type === "news") {
    return state.open ? { ...state, lastAt: event.at } : state;
  }
  if (event.type !== "tool") {
    return state;
  }
  const prev = state.calls[event.id];
  const title = event.title ?? prev?.title ?? null;
  const status = event.status ?? prev?.status ?? null;
  return {
    ...state,
    calls: { ...state.calls, [event.id]: { title, status } },
    order: state.order.includes(event.id) ? state.order : [...state.order, event.id],
    lastAt: event.at,
  };
}

export function phaseLine(state, now) {
  if (!state.open || state.lastAt == null) {
    return "";
  }
  if (now - state.lastAt >= STALL_MS) {
    return `No news for ${STALL_MS / 1000}s`;
  }
  let running = null;
  for (const id of state.order) {
    const call = state.calls[id];
    if (RUNNING.has(call.status)) {
      running = call;
    }
  }
  if (running) {
    return `Running: ${running.title || "tool"}`;
  }
  return "Waiting";
}
