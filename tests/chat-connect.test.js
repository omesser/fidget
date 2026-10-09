import assert from "node:assert/strict";
import { test } from "node:test";

import {
  canAnswer,
  composerPlaceholder,
  inlineSegments,
  landingCopy,
  loginPresentation,
} from "../src/chat-connect.js";

const http = {
  name: "bmo",
  configured: true,
  enabled: true,
};

function npxOpening(name) {
  return {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: name,
    harness: {
      name,
      session: null,
      alive: false,
      login: null,
      missing: "npx",
    },
  };
}

test("HTTP Completer mode can answer when configured and on", () => {
  assert.equal(canAnswer(http), true);
  assert.equal(composerPlaceholder(http), "Ask bmo…");
});

test("a missing npx launcher cannot answer, for every adapter preset", () => {
  for (const name of ["claude", "codex", "pi"]) {
    const opening = npxOpening(name);
    assert.equal(canAnswer(opening), false, name);
    assert.equal(composerPlaceholder(opening), "Nothing can answer yet", name);
    const copy = landingCopy(opening);
    assert.match(copy.title, /needs `npx`/, name);
    assert.match(copy.lede, /`npx` is not installed/, name);
    assert.match(copy.lede, /does not bundle `npx`/, name);
    assert.equal(copy.command, null, name);
  }
});

test("a missing first-party CLI names that binary, not npx", () => {
  for (const name of ["copilot", "cursor-agent", "goose", "grok", "hermes", "opencode"]) {
    const opening = {
      name: "bmo",
      configured: true,
      enabled: true,
      harness_name: name,
      harness: {
        name,
        session: null,
        alive: false,
        login: null,
        missing: name,
      },
    };
    const copy = landingCopy(opening);
    assert.match(copy.title, new RegExp(`needs \`${name}\``), name);
    assert.match(copy.lede, new RegExp(`\`${name}\` is not installed`), name);
    assert.match(copy.lede, new RegExp(`does not bundle \`${name}\``), name);
    assert.doesNotMatch(copy.lede, /`npx`/, name);
  }
});

// The page comes from the Shell's table, which Settings names it from too.
test("a missing launcher names the install page the opening carries, and only that", () => {
  const opening = npxOpening("codex");
  assert.equal(
    landingCopy({ ...opening, harness: { ...opening.harness, install: "https://nodejs.org/" } }).lede,
    "`npx` is not installed. Fidget does not bundle `npx`. Install from https://nodejs.org/. Then press Codex again, or pick a different Harness below.",
  );
  assert.equal(
    landingCopy(opening).lede,
    "`npx` is not installed. Fidget does not bundle `npx`. Then press Codex again, or pick a different Harness below.",
  );
});

test("a named Harness that has not come up stays on the landing", () => {
  const copy = landingCopy({
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "hermes",
    harness: { name: "hermes", session: null, alive: false, login: null },
  });
  assert.equal(copy.title, "Hermes is not running");
  assert.match(copy.lede, /Static weights/);
});

test("a launcher that failed preflight gets the Harness error landing with the probe it ran", () => {
  const opening = {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "claude",
    harness: {
      name: "claude",
      session: null,
      alive: false,
      login: null,
      unhealthy: { command: "npx --version", reason: "timed out after 3.0s", output: "", node_check: "node --version" },
    },
  };
  assert.equal(canAnswer(opening), false);
  const copy = landingCopy(opening);
  assert.equal(copy.kicker, "Harness error");
  assert.equal(copy.title, "Claude Code couldn't start");
  assert.equal(copy.lede, "It timed out after 3.0s, and printed nothing.");
  assert.deepEqual(copy.failure, { command: "npx --version", output: null, nodeCheck: "node --version" });
});

function diedOpening(failed) {
  return {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "codex",
    harness: { name: "codex", session: null, alive: false, login: null, failed },
  };
}

test("a launcher that died at startup names why on the landing", () => {
  const opening = diedOpening({
    command: "npx -y @agentclientprotocol/codex-acp@latest",
    reason: "exited before initialize, signal: 6 (SIGABRT)",
    output: "dyld[0]: Library not loaded: /opt/homebrew/opt/llhttp/lib/libllhttp.9.3.dylib",
    node_check: "node --version",
  });
  assert.equal(canAnswer(opening), false);
  const copy = landingCopy(opening);
  assert.equal(copy.kicker, "Harness error");
  assert.equal(copy.title, "Codex couldn't start");
  assert.equal(copy.lede, "It exited before initialize, signal: 6 (SIGABRT). This is what it printed:");
  assert.deepEqual(copy.failure, {
    command: "npx -y @agentclientprotocol/codex-acp@latest",
    output: "dyld[0]: Library not loaded: /opt/homebrew/opt/llhttp/lib/libllhttp.9.3.dylib",
    nodeCheck: "node --version",
  });
  assert.equal(copy.next, "Fix the error above, then pick it again, or pick a different Harness below.");
});

test("a custom launcher line is titled as the Harness, and its path stays in the Command box", () => {
  const line = "/opt/tools/bin/my-agent --acp";
  const opening = {
    ...diedOpening({ command: line, reason: "exited before initialize", output: "boom", node_check: null }),
    harness_name: line,
  };
  opening.harness.name = "/opt/tools/bin/my-agent";
  const copy = landingCopy(opening);
  assert.equal(copy.title, "Harness couldn't start");
  assert.equal(copy.failure.command, line);
  assert.doesNotMatch(copy.lede + copy.next, /my-agent/);
});

test("a launcher that died silently says it printed nothing", () => {
  const copy = landingCopy(
    diedOpening({ command: "hermes acp", reason: "exited before initialize", output: "", node_check: null }),
  );
  assert.equal(copy.lede, "It exited before initialize, and printed nothing.");
  assert.deepEqual(copy.failure, { command: "hermes acp", output: null, nodeCheck: null });
});

test("a failure before the launch has no command to box", () => {
  const copy = landingCopy(
    diedOpening({ command: null, reason: "`codex` did not answer initialize.", output: "", node_check: null }),
  );
  assert.equal(copy.lede, "`codex` did not answer initialize.");
  assert.deepEqual(copy.failure, { command: null, output: null, nodeCheck: null });
});

test("initializing Harness gates chat and shows clear state", () => {
  const opening = {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "hermes",
    harness: {
      name: "hermes",
      session: null,
      alive: false,
      login: null,
      initializing: true,
    },
  };
  assert.equal(canAnswer(opening), false, "chat is gated during initialization");
  assert.equal(composerPlaceholder(opening), "Starting Hermes…");
  const copy = landingCopy(opening);
  assert.equal(copy.title, "Initializing Hermes…");
  assert.match(copy.lede, /starting up/i);
  assert.match(copy.lede, /download/i);
  assert.match(copy.lede, /can take a while/i);
  assert.match(copy.lede, /when the Harness answers/i);
  assert.doesNotMatch(copy.lede, /\d+\s*[–-]\s*\d+\s*seconds/i);
  assert.doesNotMatch(copy.lede, /not running/);
});

test("needs-auth still names the login command", () => {
  const copy = landingCopy({
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "codex",
    login: "codex login",
    harness: {
      name: "codex",
      session: null,
      alive: true,
      login: "codex login",
    },
  });
  assert.equal(copy.title, "Codex needs login");
  assert.equal(copy.lede, "Codex is running but not signed in. Sign in, then press Retry, or pick a different Harness below.");
  assert.equal(copy.command, "codex login");
  assert.equal(copy.signInLabel, null);
  assert.equal(copy.hint, "Run this in a terminal:");
});

// Retry re-asks the Harness already picked, so only the login state offers it.
// The branded buttons stay on every landing (chat-landing-buttons.test.js).
test("only the needs-login landing offers Retry", () => {
  const login = {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "claude",
    login: "claude /login",
    harness: { name: "claude", session: null, alive: true, login: "claude /login" },
  };
  const copy = landingCopy(login);
  assert.equal(copy.retry, true);
  assert.equal(copy.title, "Claude Code needs login");
  assert.equal(copy.lede, "Claude Code is running but not signed in. Sign in, then press Retry, or pick a different Harness below.");
  assert.equal(copy.command, "claude /login");
  assert.deepEqual(copy.signIn, []);

  const pickers = [
    { configured: false, enabled: false },
    { configured: true, enabled: false, harness_name: "codex", harness: { name: "codex", alive: false } },
    npxOpening("codex"),
    { ...login, login: null, harness: { name: "claude", alive: true, login: null, initializing: true } },
    { ...login, login: null, harness: { name: "claude", alive: false, login: null, failed: "died" } },
    { ...login, login: null, harness: { name: "claude", alive: false, login: null } },
  ];
  for (const opening of pickers) {
    assert.equal(Boolean(landingCopy(opening).retry), false, landingCopy(opening).title);
  }
});

test("needs-login offers agent sign-in beside the terminal command", () => {
  const opening = {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "codex",
    login: "codex login",
    sign_in: [
      { id: "chatgpt", label: "ChatGPT" },
      { id: "apikey", label: "API Key" },
    ],
    harness: {
      name: "codex",
      session: null,
      alive: true,
      login: "codex login",
    },
  };
  assert.deepEqual(loginPresentation(opening), {
    command: "codex login",
    actions: [
      { id: "chatgpt", label: "ChatGPT" },
      { id: "apikey", label: "API Key" },
    ],
  });
  const copy = landingCopy(opening);
  assert.equal(copy.retry, true);
  assert.equal(copy.command, "codex login");
  assert.equal(copy.signInLabel, "Login using:");
  assert.equal(copy.hint, "Or run this in a terminal:");
  assert.equal(
    copy.signInWaiting,
    "Finish signing in in your browser. Any code on that page came from Codex, so continue only if you just clicked this button.",
  );
  assert.deepEqual(copy.signIn, [
    { id: "chatgpt", label: "ChatGPT" },
    { id: "apikey", label: "API Key" },
  ]);
  assert.equal(canAnswer(opening), false);

  const bare = { ...opening };
  delete bare.sign_in;
  assert.deepEqual(loginPresentation(bare).actions, []);
  const bareCopy = landingCopy(bare);
  assert.equal(bareCopy.command, "codex login");
  assert.equal(bareCopy.signInLabel, null);
  assert.equal(bareCopy.hint, "Run this in a terminal:");
  assert.equal(bareCopy.signInWaiting, null);
  assert.deepEqual(bareCopy.signIn, []);

  assert.deepEqual(
    loginPresentation({
      configured: true,
      enabled: true,
      login: "codex login",
      kind: "agent",
    }).actions,
    [],
  );
  assert.deepEqual(landingCopy({ configured: false, enabled: false }).signIn, []);
});

test("Antigravity signs in from its buttons and names no terminal command", () => {
  const opening = {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "antigravity",
    login: "Log in with Google from Chat",
    sign_in: [
      { id: "oauth-personal", label: "Log in with Google" },
      { id: "oauth-business", label: "Log in with Gemini Enterprise" },
    ],
    harness: {
      name: "antigravity",
      session: null,
      alive: true,
      login: "Log in with Google from Chat",
    },
  };
  const copy = landingCopy(opening);
  assert.equal(copy.title, "Antigravity needs login");
  assert.equal(copy.lede, "Antigravity is running but not signed in. Sign in, or pick a different Harness below.");
  assert.equal(copy.retry, true);
  assert.equal(copy.signInLabel, "Login using:");
  assert.equal(copy.command, null);
  assert.deepEqual(
    copy.signIn.map((action) => action.id),
    ["oauth-personal", "oauth-business"],
  );
});

test("unconfigured and switched-off copy is unchanged", () => {
  assert.equal(
    landingCopy({ configured: false, enabled: false }).title,
    "Connect a Harness to get started",
  );
  assert.equal(
    landingCopy({
      configured: true,
      enabled: false,
      harness_name: "codex",
      harness: { name: "codex", alive: false, missing: "npx" },
    }).title,
    "Chat is switched off",
  );
});

test("a backticked command becomes a code segment without its backticks", () => {
  assert.deepEqual(inlineSegments("Run `node --version` in a terminal."), [
    { kind: "text", text: "Run " },
    { kind: "code", text: "node --version" },
    { kind: "text", text: " in a terminal." },
  ]);
});

test("a URL becomes a link, leaving the full stop after it as text", () => {
  assert.deepEqual(inlineSegments("Install from https://nodejs.org/. Then retry."), [
    { kind: "text", text: "Install from " },
    { kind: "link", text: "https://nodejs.org/" },
    { kind: "text", text: ". Then retry." },
  ]);
  assert.deepEqual(inlineSegments("see (http://x.ai/docs),"), [
    { kind: "text", text: "see (" },
    { kind: "link", text: "http://x.ai/docs" },
    { kind: "text", text: ")," },
  ]);
});

test("code and a link in one sentence each keep their kind", () => {
  const copy = landingCopy({
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "codex",
    harness: { name: "codex", session: null, alive: false, login: null, missing: "npx", install: "https://nodejs.org/" },
  });
  const kinds = inlineSegments(copy.lede).filter((s) => s.kind !== "text");
  assert.deepEqual(kinds, [
    { kind: "code", text: "npx" },
    { kind: "code", text: "npx" },
    { kind: "link", text: "https://nodejs.org/" },
  ]);
});

test("a URL inside backticks stays code, not a link", () => {
  assert.deepEqual(inlineSegments("`curl https://x.ai/`"), [
    { kind: "code", text: "curl https://x.ai/" },
  ]);
});

test("an unmatched backtick reads as itself", () => {
  assert.deepEqual(inlineSegments("run `a` then `b"), [
    { kind: "text", text: "run " },
    { kind: "code", text: "a" },
    { kind: "text", text: " then `b" },
  ]);
});

test("markup and other schemes stay text", () => {
  assert.deepEqual(inlineSegments('<img src=x onerror="alert(1)"> javascript:alert(1)'), [
    { kind: "text", text: '<img src=x onerror="alert(1)"> javascript:alert(1)' },
  ]);
  assert.deepEqual(inlineSegments(null), []);
});
