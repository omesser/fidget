
// What the status bar draws from one push and the clock, and which mind
// answers. Split from chat.js, which reads `window.__TAURI__` at import,
// so node can load this.
import { t } from "./i18n.js";

// What a cell says when there is nothing to say. A blank cell reads as a
// bar that broke.
const NONE = "—";

// Milliseconds until the next proactive wake, in the largest unit that still
// counts. Rounded up, and `due` past zero. Three characters fit the bar.
export function untilWake(ms) {
  if (ms === null || ms === undefined) {
    return NONE;
  }
  const secs = Math.ceil(ms / 1000);
  if (secs <= 0) {
    return "due";
  }
  if (secs < 60) {
    return `${secs}s`;
  }
  const mins = Math.ceil(secs / 60);
  return mins < 60 ? `${mins}m` : `${Math.ceil(mins / 60)}h`;
}

// One push, plus how much of its countdown is left, as each cell's text.
// `status` is null until the first arrives, and every cell says so.
export function statusCells(status, msLeft) {
  return {
    behavior: status?.behavior ?? NONE,
    primitive: status?.primitive ?? NONE,
    animation: status?.animation ?? NONE,
    state: status?.state ?? NONE,
    // Nothing rather than an arrow before the first push. A guessed arrow is a lie.
    facing: status ? (status.facing < 0 ? "←" : "→") : "",
    // One fact, not both. This cell held the widest pair, and a wake resets
    // the countdown, so under a turn on the wire it is always the full wait.
    director: status?.thinking ? t("thinking") : t("wake {when}", { when: untilWake(status ? msLeft : null) }),
    happened: status?.happened ?? NONE,
  };
}

// What the fidget is doing right now, without Primitive or State vocabulary.
export function plainStatus(status, msLeft) {
  if (!status) {
    return t("Starting up…");
  }
  if (status.thinking) {
    return t("Thinking…");
  }

  let parts = [];
  const activity = humanizeActivity(status);
  if (activity) {
    parts.push(activity);
  } else {
    parts.push(t("Idle"));
  }

  const cue = humanizeHappened(status.happened);
  if (cue) {
    parts.push(cue);
  }

  const wake = untilWake(msLeft);
  if (wake !== "due" && wake !== NONE && !cue) {
    parts.push(t("next thought in {wake}", { wake }));
  }

  return parts.join(" · ");
}

function humanizeActivity(status) {
  if (status.behavior) {
    const readable = humanizeBehavior(status.behavior);
    if (readable) {
      return readable;
    }
  }

  if (status.animation) {
    const readable = humanizeAnimation(status.animation);
    if (readable) {
      return readable;
    }
  }

  if (status.primitive && status.primitive !== NONE) {
    const readable = humanizePrimitive(status.primitive);
    if (readable) {
      return readable;
    }
  }

  return null;
}

function humanizeBehavior(behavior) {
  if (!behavior || behavior === NONE) {
    return null;
  }

  return cleanName(behavior);
}

function humanizeAnimation(animation) {
  if (!animation || animation === NONE) {
    return null;
  }

  return cleanName(animation);
}

function humanizePrimitive(primitive) {
  if (!primitive || primitive === NONE) {
    return null;
  }

  return cleanName(primitive);
}

function humanizeHappened(happened) {
  if (!happened || happened === NONE) {
    return null;
  }

  const map = {
    poked: "just poked",
    summoned: "just summoned",
    thrown: "just thrown",
    grabbed: "just grabbed",
    dropped: "just dropped",
    "spoken to": "just spoken to",
  };

  return map[happened] ? t(map[happened]) : t("just {happened}", { happened });
}

function cleanName(raw) {
  if (!raw) {
    return null;
  }
  const cleaned = raw.replace(/_/g, " ").replace(/-/g, " ");
  return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
}

// Which mind answers, beside who you are talking to. A statement, never a
// control. An install the user can still make is `missing`, not `not running`.
export function mindLine(opening) {
  if (!opening) {
    return "";
  }
  if (!opening.enabled) {
    return t("static weights");
  }
  const harness = opening.harness;
  if (!harness) {
    // Nothing rather than a bare separator. Empty strings mean the Shell could
    // not read its own inspect, and the dot is punctuation pretending to be a fact.
    return opening.model && opening.host ? `${opening.model} · ${opening.host}` : "";
  }
  if (harness.login) {
    return t("{name} · not signed in — `{login}`", { name: harness.name, login: harness.login });
  }
  if (harness.missing) {
    return t("{name} · `{missing}` is not installed", { name: harness.name, missing: harness.missing });
  }
  if (harness.unhealthy) {
    return t("{name} · unhealthy", { name: harness.name });
  }
  // Before `not running`. Over the handshake that names a state the user
  // would have to act on, when the only thing to do is wait.
  if (harness.initializing) {
    return t("{name} · starting", { name: harness.name });
  }
  if (harness.failed) {
    return t("{name} · failed to start", { name: harness.name });
  }
  if (!harness.alive) {
    return t("{name} · not running", { name: harness.name });
  }
  if (!harness.session) {
    return t("{name} · no session yet", { name: harness.name });
  }
  // The head of the id. A full UUID pushes the Instance name off a 420-point
  // header, and Settings draws the id in full, so the head cannot disagree.
  return t("{name} · session {id}", { name: harness.name, id: harness.session.slice(0, 8) });
}
