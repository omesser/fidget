// Connect-landing copy and the ready gate. chat.js reaches window.__TAURI__
// as it loads and cannot be imported outside a webview; this can, so it has
// a test. One module so attached() and the composer cannot disagree about
// whether a named Harness can answer (#726).

const DISPLAY_NAMES = {
  claude: "Claude Code",
  codex: "Codex",
  copilot: "GitHub Copilot",
  "cursor-agent": "Cursor",
  goose: "Goose",
  grok: "Grok",
  opencode: "OpenCode",
  hermes: "Hermes",
  pi: "Pi",
  antigravity: "Antigravity",
};

// Harnesses with no login command to run. Their buttons are the only way in.
const BUTTONS_ONLY = new Set(["antigravity"]);

export function harnessDisplayName(opening) {
  const key = opening?.harness_name || opening?.harness?.name;
  return DISPLAY_NAMES[key] || key || "The Harness";
}

// No login means no actions, even if `sign_in` is still on the opening.
// `kind` is not a button. The shell already decided which methods are.
export function loginPresentation(opening) {
  if (!opening?.configured || !opening.enabled || !opening.login) {
    return { command: null, actions: [] };
  }
  const raw = Array.isArray(opening.sign_in) ? opening.sign_in : [];
  const actions = [];
  for (const item of raw) {
    if (!item || typeof item.id !== "string" || item.id === "") {
      continue;
    }
    if (typeof item.label !== "string" || item.label === "") {
      continue;
    }
    actions.push({ id: item.id, label: item.label });
  }
  return { command: opening.login, actions };
}

// Configured is not ready. A named Harness whose child never came up, or
// whose launcher is missing, must not enable Ask {name} the way a live
// session does. HTTP Completer mode has no harness object.
// Initializing also gates: ACP handshake is in progress and turns would fail.
export function canAnswer(opening) {
  if (!opening?.configured || !opening.enabled || opening.login) {
    return false;
  }
  const harness = opening.harness;
  if (!harness) {
    return true;
  }
  return harness.alive && !harness.missing && !harness.unhealthy && !harness.initializing;
}

export function composerPlaceholder(opening) {
  if (canAnswer(opening)) {
    return `Ask ${opening.name}…`;
  }
  // The composer is the one surface a user types into, so a wait that ends on
  // its own says so rather than reading as a dead end (#949).
  if (opening?.harness?.initializing) {
    return `Starting ${harnessDisplayName(opening)}…`;
  }
  return "Nothing can answer yet";
}

// Strings the landing paints. Facts come from the opening; this file owns
// the sentences so Chat and the tests cannot drift.
export function landingCopy(opening) {
  const name = harnessDisplayName(opening);
  const harness = opening?.harness;
  const missing = harness?.missing;

  // Buttons are the first tier and the command the second. With no button,
  // the command is the only path, so it drops the "Or".
  if (opening?.configured && opening.enabled && opening.login) {
    const signIn = loginPresentation(opening).actions;
    const buttons = signIn.length > 0;
    const terminal = !BUTTONS_ONLY.has(opening.harness_name);
    return {
      title: `${name} needs login`,
      lede: terminal
        ? `${name} is running but not signed in. Sign in, then press Retry.`
        : `${name} is running but not signed in.`,
      // Retry re-asks the Harness already picked. The buttons stay for a
      // different one (#1458).
      retry: true,
      command: terminal ? opening.login : null,
      signInLabel: buttons ? "Login using:" : null,
      hint: buttons ? "Or run this in a terminal:" : "Run this in a terminal:",
      // The Harness opens the browser itself and may prefill a code there that
      // never reaches us (grok's inline flow). The click is what the user vouches for.
      signInWaiting: buttons
        ? `Finish signing in in your browser. Any code on that page came from ${name}, so continue only if you just clicked this button.`
        : null,
      signIn,
    };
  }

  if (!opening?.configured) {
    return {
      title: "Connect a Harness to get started",
      lede: "Choose an agent runtime to power this chat. Each signs in on its own — no credentials stored here.",
      command: null,
      signInLabel: null,
      hint: null,
      signIn: [],
    };
  }

  if (!opening.enabled) {
    return {
      title: "Chat is switched off",
      lede: "Turn AI back on in Settings, or connect a Harness below.",
      command: null,
      signInLabel: null,
      hint: null,
      signIn: [],
    };
  }

  if (missing) {
    const installHint = harness.install ? ` Install from ${harness.install}.` : "";
    return {
      title: `${name} needs \`${missing}\``,
      lede: `\`${missing}\` is not installed. Fidget does not bundle \`${missing}\`.${installHint} Then press ${name} again, or pick a different Harness below.`,
      command: null,
      signInLabel: null,
      hint: null,
      signIn: [],
    };
  }

  if (harness?.initializing) {
    return {
      title: `Initializing ${name}…`,
      lede: `${name} is starting up. First run may download and can take a while. Chat will be ready when the Harness answers.`,
      command: null,
      signInLabel: null,
      hint: null,
      signIn: [],
    };
  }

  // The launcher died before it answered, or preflight refused it. Either way
  // the Shell hands over the command, why, what it printed, and for `npx` the
  // Node.js check. Only the reason is prose; the rest is raw text for the boxes.
  // A custom line's name is its whole path, which the Command box already shows.
  const failed = harness?.failed ?? harness?.unhealthy;
  if (failed) {
    const preset = Object.hasOwn(DISPLAY_NAMES, opening.harness_name ?? "");
    const { command, reason, output, node_check: nodeCheck } = failed;
    const why = String(reason ?? "").replace(/\.$/, "");
    let lede;
    if (!command) {
      lede = `${why}.`;
    } else if (output) {
      lede = `It ${why}. This is what it printed:`;
    } else {
      lede = `It ${why}, and printed nothing.`;
    }
    return {
      kicker: "Harness error",
      title: `${preset ? name : "Harness"} couldn't start`,
      lede,
      failure: { command: command || null, output: output || null, nodeCheck: nodeCheck || null },
      next: "Fix the error above, then pick it again, or pick a different Harness below.",
      command: null,
      signInLabel: null,
      hint: null,
      signIn: [],
    };
  }

  if (harness && !harness.alive) {
    return {
      title: `${name} is not running`,
      lede: `${name} is set but has not come up. Static weights answer until it does. Pick a different Harness below.`,
      command: null,
      signInLabel: null,
      hint: null,
      signIn: [],
    };
  }

  return {
    title: "Chat is switched off",
    lede: "Turn AI back on in Settings, or connect a Harness below.",
    command: null,
    signInLabel: null,
    hint: null,
    signIn: [],
  };
}

// A bare URL, and the punctuation that ends the sentence around it rather than
// the URL. `http` too: the scheme check is `platform::open_url`'s, in Rust.
const URL_IN_TEXT = /https?:\/\/[^\s<>"'`]+/g;
const URL_TAIL = /[.,;:!?)\]]+$/;

// A landing sentence as text, code and link segments: backticks mark a
// command, a bare URL a link. Not Markdown, which would eat the backslashes
// and underscores of a path the Shell names in a failure.
export function inlineSegments(text) {
  const segments = [];
  const push = (kind, piece) => {
    const last = segments.at(-1);
    if (kind === "text" && last?.kind === "text") {
      last.text += piece;
    } else if (piece !== "") {
      segments.push({ kind, text: piece });
    }
  };
  const parts = String(text ?? "").split("`");
  // With an odd number of backticks the last one has no partner and stays.
  const closed = parts.length % 2 === 1 ? parts.length : parts.length - 1;
  parts.forEach((part, i) => {
    if (i % 2 === 1 && i < closed) {
      push("code", part);
      return;
    }
    const plain = i % 2 === 1 ? `\`${part}` : part;
    let from = 0;
    for (const found of plain.matchAll(URL_IN_TEXT)) {
      const url = found[0].replace(URL_TAIL, "");
      push("text", plain.slice(from, found.index));
      push("link", url);
      from = found.index + url.length;
    }
    push("text", plain.slice(from));
  });
  return segments;
}

// Draw `text` into `parent` as its segments, through `textContent` only: the
// Shell's failure sentence is untrusted. A link is a reply link (`.md-link`),
// so the log's click listener hands it to `open_link` rather than navigating.
export function drawInline(parent, text, doc = globalThis.document) {
  parent.replaceChildren(
    ...inlineSegments(text).map(({ kind, text: piece }) => {
      if (kind === "text") {
        return doc.createTextNode(piece);
      }
      const node = doc.createElement(kind === "code" ? "code" : "span");
      node.textContent = piece;
      if (kind === "link") {
        node.className = "md-link";
        node.dataset.href = piece;
        node.title = piece;
      }
      return node;
    }),
  );
}
