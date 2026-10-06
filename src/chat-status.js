// What the Chat surface's status bar draws, from one push and the clock, and
// the header's line about which mind answers. Its own module because chat.js
// reaches window.__TAURI__ as it loads; this can be imported, so it has a test.

// What a cell says when there is nothing to say — the dash the Shell's
// `engine:` trace writes. A blank cell reads as a bar that broke.
const NONE = "—";

// Milliseconds until the next proactive wake, in the largest unit that still
// counts. Rounded up, and `due` past zero: a wake the Engine has not got to
// yet is due, not late. Three characters is what the bar's width budget fits.
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

// One push, plus how much of its countdown is left, as the text of each cell.
// `status` is null until the first arrives, and every cell says so.
export function statusCells(status, msLeft) {
  return {
    behavior: status?.behavior ?? NONE,
    primitive: status?.primitive ?? NONE,
    animation: status?.animation ?? NONE,
    state: status?.state ?? NONE,
    // Nothing rather than an arrow before the first push: a sprite nothing has
    // reported on faces no way, and a guessed arrow is a lie.
    facing: status ? (status.facing < 0 ? "←" : "→") : "",
    // One fact, not both: this cell held the widest pair on the line. It costs
    // nothing — a wake resets the ambient pace as it starts, so the countdown
    // under a turn on the wire is always the full wait over again.
    director: status?.thinking ? "thinking" : `wake ${untilWake(status ? msLeft : null)}`,
    happened: status?.happened ?? NONE,
  };
}

// Plain-language status for first-time readers: what the fidget is doing right
// now, without Primitive/State vocabulary.
export function plainStatus(status, msLeft) {
  if (!status) {
    return "Starting up…";
  }
  if (status.thinking) {
    return "Thinking…";
  }

  let parts = [];
  const activity = humanizeActivity(status);
  if (activity) {
    parts.push(activity);
  } else {
    parts.push("Idle");
  }

  const cue = humanizeHappened(status.happened);
  if (cue) {
    parts.push(cue);
  }

  const wake = untilWake(msLeft);
  if (wake !== "due" && wake !== NONE && !cue) {
    parts.push(`next thought in ${wake}`);
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

  return map[happened] || `just ${happened}`;
}

function cleanName(raw) {
  if (!raw) {
    return null;
  }
  const cleaned = raw.replace(/_/g, " ").replace(/-/g, " ");
  return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
}

// Which mind answers this window, for the header beside who you are talking
// to. A statement and never a control. The branches follow
// `settings::harness_state`'s order, including the missing-CLI state #659
// added there. Chat used to drop `missing` and say `not running` for an
// install the user could still make (#726).
export function mindLine(opening) {
  if (!opening) {
    return "";
  }
  if (!opening.enabled) {
    return "static weights";
  }
  const harness = opening.harness;
  if (!harness) {
    // Nothing rather than a bare separator: the Shell sends empty strings only
    // when it could not read its own inspect, and ` · ` is punctuation
    // pretending to be a fact.
    return opening.model && opening.host ? `${opening.model} · ${opening.host}` : "";
  }
  if (harness.login) {
    return `${harness.name} · not signed in — \`${harness.login}\``;
  }
  if (harness.missing) {
    return `${harness.name} · \`${harness.missing}\` is not installed`;
  }
  if (harness.unhealthy) {
    return `${harness.name} · unhealthy`;
  }
  // Before `not running`, which over the handshake names a state the user
  // would have to act on when the only thing to do is wait (#949).
  if (harness.initializing) {
    return `${harness.name} · starting`;
  }
  if (harness.failed) {
    return `${harness.name} · failed to start`;
  }
  if (!harness.alive) {
    return `${harness.name} · not running`;
  }
  if (!harness.session) {
    return `${harness.name} · no session yet`;
  }
  // The head of the id, not the whole of it: a full UUID pushes the Instance's
  // own name off a 420-point header, and Settings draws it in full, so the head
  // cannot disagree with it.
  return `${harness.name} · session ${harness.session.slice(0, 8)}`;
}
