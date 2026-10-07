// The Chat surface: one window per Summoned Character Instance, drawn by
// fidget rather than by whatever answers (ADR-0018). Like the overlay it
// holds no authoritative state; the Shell owns the session behind it.

import { elicitChoices, elicitSays } from "./chat-ask.js";
import { drawAskDetails } from "./chat-ask-row.js";
import { mountChatAppearance } from "./chat-appearance.js";
import {
  canAnswer,
  composerPlaceholder,
  drawInline,
  harnessDisplayName,
  landingCopy,
} from "./chat-connect.js";
import { createNamesNotice } from "./chat-names-hint.js";
import { planSteps } from "./chat-plan.js";
import { applyChatUiClass } from "./chat-ui-class.js";
import { MISSING_ANSWER, createChatTurns } from "./chat-settle.js";
import { createComposerRecall } from "./chat-recall.js";
import { createThinking } from "./chat-thinking.js";
import { stampWhen } from "./chat-stamp.js";
import { mindLine, plainStatus, statusCells } from "./chat-status.js";
import { appendReply, drawReply } from "./markdown.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const chat = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

// The label is `chat-` and the Instance's id, which is how the Shell addresses
// this window. Read from the label because a webview cannot be handed arguments
// at creation, and an event emitted then would reach no listener.
const instance = chat.label.replace(/^chat-/, "");

const log = document.getElementById("log");
const recall = createComposerRecall(log);
// New lines stay under the landing until this Harness can answer.
let holdLogAtTop = false;
const plan = document.getElementById("plan");
const namesHintEl = document.getElementById("names-hint");
const empty = document.getElementById("empty");
const composer = document.getElementById("composer");
const line = document.getElementById("line");
const send = document.getElementById("send");

const promptTab = document.getElementById("prompt");
const promptText = document.getElementById("prompt-text");
const promptSaid = document.getElementById("prompt-said");
const promptSave = document.getElementById("prompt-save");
const promptDiscard = document.getElementById("prompt-discard");
const promptConfirm = document.getElementById("prompt-confirm");
const promptCancel = document.getElementById("prompt-cancel");

const cells = Object.fromEntries(
  ["behavior", "primitive", "animation", "state", "facing", "director", "happened"].map(
    (name) => [name, document.getElementById(`s-${name}`)],
  ),
);

const plainEl = document.getElementById("s-plain");

// The WHO label on the Instance's own turns, filled in once the Shell says who
// this window belongs to.
let them = "";

// Last stamped instant in this window, so a line after midnight can say the
// new day once.
let previousAt = null;

// The last thing the Shell said about the Spatial Layer, and when the proactive
// wake it named falls due. The Shell pushes that deadline once rather than a
// number every second; the arithmetic between costs the frame loop nothing.
let status = null;
let wakeAt = null;

function paint() {
  const left = wakeAt === null ? null : Math.max(0, wakeAt - performance.now());
  const drawn = statusCells(status, left);
  for (const [name, node] of Object.entries(cells)) {
    node.textContent = drawn[name];
  }
  plainEl.textContent = plainStatus(status, left);
}

// Turns waiting on an answer, oldest first. The Shell answers them in the
// order it took them and refuses a line typed while one is still waiting, so
// the oldest row takes the next answer and the newest takes a refusal.
const turns = createChatTurns();

const names = createNamesNotice({
  act: (action) => invoke("names_hint_act", { action }),
});

function paintNames(painted) {
  if (!painted.changed || !namesHintEl) return;
  const view = painted.view;
  namesHintEl.replaceChildren();
  if (view.visible) {
    const heading = document.createElement("h2");
    heading.textContent = view.heading;
    const body = document.createElement("p");
    body.textContent = view.body;
    const actions = document.createElement("div");
    actions.className = "names-hint-actions";
    for (const button of view.buttons) {
      const node = document.createElement("button");
      node.type = "button";
      node.textContent = button.label;
      node.addEventListener("click", () => {
        names
          .press(button.action)
          .then(paintNames)
          .catch((why) => {
            console.error("chat: the notice did not reach the shell:", why);
          });
      });
      actions.append(node);
    }
    namesHintEl.append(heading, body, actions);
  }
  namesHintEl.hidden = !view.visible;
}

function el(cls, tag) {
  const node = document.createElement(tag || "div");
  node.className = cls;
  return node;
}

function add(node) {
  log.append(node);
  log.scrollTop = holdLogAtTop ? 0 : log.scrollHeight;
  return node;
}

function when(at) {
  const instant = typeof at === "number" ? new Date(at) : new Date();
  const stamp = stampWhen(instant, previousAt);
  previousAt = instant;
  const node = el("when", "time");
  node.dateTime = stamp.datetime;
  node.title = stamp.title;
  node.textContent = stamp.label;
  return node;
}

function said(who, text, cls, at) {
  const row = el(`row ${cls}`);
  const cluster = el("who");
  const label = el("who-label");
  label.textContent = who;
  cluster.append(label, when(at));
  // Only what answered gets its Markdown drawn. The user's own turn
  // stays the characters they typed: they wrote punctuation, not a document.
  const body = el(cls === "them" ? "said md" : "said");
  if (cls === "them") {
    drawReply(body, text);
  } else {
    body.textContent = text;
  }
  row.append(cluster, body);
  return add(row);
}

// A turn's answer, opened empty with a blinking caret and appended to as it
// arrives. Today the Shell hands over a finished Wake, so the append runs once;
// with chunks, each lands here and the caret stays until the last.
function opening_answer() {
  const row = said(them, "", "them");
  const caret = el("caret", "span");
  caret.textContent = "▍";
  row.querySelector(".said").append(caret);
  return row;
}

function arrived(row, text) {
  appendReply(row.querySelector(".said"), text);
  log.scrollTop = log.scrollHeight;
}

function settled(row) {
  row.querySelector(".caret")?.remove();
}

// The Harness's thinking, one row per turn in the log (ADR-0034). The Shell
// sends the whole thought so far, so a row is redrawn rather than appended to.
const thinkingRows = new Map();

function drawThinking(view) {
  let row = thinkingRows.get(view.id);
  if (!row) {
    row = el("row thinking");
    const toggle = el("who thinking-toggle", "button");
    toggle.type = "button";
    const label = el("who-label");
    label.textContent = "Thinking";
    toggle.append(label, when(view.at));
    toggle.addEventListener("click", () => thinking.toggle(view.id));
    row.append(toggle, el("said"));
    thinkingRows.set(view.id, row);
    add(row);
    // Under the question it belongs to, above the answer still on its way.
    lowerCaret();
  }
  const body = row.querySelector(".said");
  body.textContent = view.text;
  body.hidden = view.state === "collapsed";
  row.dataset.state = view.state;
  row.querySelector(".thinking-toggle").setAttribute("aria-expanded", String(!body.hidden));
  if (view.state === "streaming") log.scrollTop = log.scrollHeight;
}

const thinking = createThinking(drawThinking);

// The agent's steps, replaced whole on every update because that is how ACP
// sends them (#697). The current step is scrolled to, or a plan longer than
// the cap would leave the reader looking at step one.
function showPlan(steps) {
  plan.replaceChildren(
    ...steps.map((step) => {
      const row = el("step");
      row.dataset.status = step.status;
      row.dataset.priority = step.priority;
      row.textContent = step.text;
      return row;
    }),
  );
  plan.hidden = !steps.length;
  plan.querySelector('[data-status="in_progress"]')?.scrollIntoView({ block: "nearest" });
}

function note(text) {
  const row = el("note");
  row.append(when(), document.createTextNode(text));
  return add(row);
}

// The pieces of the launcher's Harness error landing (#1183), in the log: a
// turn failure arrives mid-conversation, so it cannot take the whole surface.
// The box takes textContent: what a Harness answered is never markup.
function harnessError(said) {
  const row = el("note harness-error");
  const kicker = el("kicker", "p");
  kicker.textContent = "Harness error";
  const title = el("", "h2");
  title.textContent = `${harnessDisplayName(lastOpening)} couldn't answer`;
  const lede = el("", "p");
  lede.textContent = "It sent back an error instead of a reply:";
  const part = el("failure-part", "section");
  const label = el("", "h3");
  label.textContent = "Error output";
  const box = el("failure-box error", "pre");
  box.textContent = said;
  part.append(label, box);
  row.append(when(), kicker, title, lede, part);
  return add(row);
}

// A link in a reply opens in the user's browser. One listener on the log, not
// one per link: every reply redraws its row as chunks arrive. `open_link`
// decides the accepted scheme in Rust; `data-href` is untrusted text.
log.addEventListener("click", (event) => {
  const link = event.target.closest?.(".md-link[data-href]");
  if (!link) {
    return;
  }
  const where = link.dataset.href;
  invoke("open_link", { url: where }).catch((why) => {
    console.error("chat: that link did not open:", why);
    note(`That link did not open: ${why}.`);
  });
});

// The rows still offering buttons, by request id. One request reaches every
// open window and only one of them takes the click, so the Shell's settled
// event is what retires the rest.
const asks = new Map();

// A request nothing can answer any more. The buttons go dead, not the row, so
// the log still says what was asked. `option` is the winner on the Shell's word:
// the wire drops every answer after the first, so a local click may have lost.
function retire(request, option) {
  const buttons = asks.get(request);
  if (!buttons) {
    return;
  }
  asks.delete(request);
  for (const button of buttons.querySelectorAll("button")) {
    button.disabled = true;
    if (option && button.dataset.option === option) {
      button.classList.add("chosen");
    }
  }
}

// A permission request, drawn as the options the Harness offered. A click is
// the only answer; a timed-out turn is cancelled by the Shell, not decided
// (ADR-0018). A repeat arrives when a late-opening window is handed unsettled asks.
function asked(ask) {
  if (asks.has(ask.request)) {
    return null;
  }
  const row = el("row ask");
  const label = el("who-label");
  label.textContent = `${them} · asks`;
  const body = el("said");
  drawAskDetails(body, ask);
  const buttons = el("options");
  for (const option of ask.options) {
    const button = el("", "button");
    button.type = "button";
    button.textContent = option.name || option.id;
    button.dataset.option = option.id;
    button.addEventListener("click", () => {
      // Disabled at once so a second click cannot be sent, but nothing is
      // marked chosen until the Shell says what won: see `retire`.
      for (const other of buttons.querySelectorAll("button")) {
        other.disabled = true;
      }
      invoke("permission_answer", { request: ask.request, option: option.id }).catch((why) => {
        console.error("chat: the answer did not reach the Harness:", why);
        note("That answer did not get through.");
      });
    });
    buttons.append(button);
  }
  body.append(buttons);
  row.append(label, body);
  asks.set(ask.request, buttons);
  add(row);
  lowerCaret();
  return row;
}

// The caret of the turn on the wire moves under an ask raised mid-turn, or
// the answer fills a row above the ask and reads as an answer that came
// before the question, with nothing after the user's Yes.
function lowerCaret() {
  const turn = turns.newest();
  if (turn?.them) {
    add(turn.them);
  }
}

// One `elicitation/create` form. Options are the schema's first enum, or for
// a URL form the link itself, drawn in full so the user can check it.
function elicited(form) {
  if (asks.has(form.request)) {
    return null;
  }
  const row = el("row ask");
  const label = el("who-label");
  label.textContent = `${them} · asks`;
  const body = el("said");
  body.textContent = elicitSays(form);
  if (form.url) {
    const link = el("ask-code", "code");
    link.textContent = form.url;
    body.append(link);
  }
  const buttons = el("options");
  for (const choice of elicitChoices(form)) {
    const button = el("", "button");
    button.type = "button";
    button.textContent = choice.name;
    button.dataset.option = choice.value ?? "decline";
    button.addEventListener("click", () => {
      for (const other of buttons.querySelectorAll("button")) {
        other.disabled = true;
      }
      const opened = choice.url ? invoke("open_link", { url: choice.url }) : Promise.resolve();
      opened
        .then(() => invoke("elicitation_answer", { request: form.request, value: choice.value }))
        .catch((why) => {
          console.error("chat: the answer did not reach the Harness:", why);
          note("That answer did not get through.");
        });
    });
    buttons.append(button);
  }
  body.append(buttons);
  row.append(label, body);
  asks.set(form.request, buttons);
  add(row);
  lowerCaret();
  return row;
}

const applyChatAppearance = mountChatAppearance(document.documentElement);

// Whether anything can answer, and what to say when nothing can. Ready is
// `canAnswer`: configured is not enough when the launcher is missing or the
// child never came up (#726). The composer is disabled rather than hidden,
// so it reads as waiting.
// One button per agent method. Rebuilt on every paint so a later opening
// cannot leave a method that is no longer offered.
const retry = document.getElementById("landing-retry");

function paintSignIn(actions, waiting) {
  const host = document.getElementById("landing-sign-in");
  host.replaceChildren();
  for (const action of actions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "sign-in-btn";
    button.textContent = action.label;
    button.addEventListener("click", () => {
      const pending = document.createElement("p");
      pending.className = "hint sign-in-waiting";
      drawInline(pending, waiting);
      button.after(pending);
      button.disabled = true;
      // A Retry during the device flow would race this open for the session.
      retry.disabled = true;
      invoke("sign_in", { instance, methodId: action.id })
        .catch((why) => note(String(why)))
        .finally(() => {
          pending.remove();
          button.disabled = false;
          retry.disabled = false;
        });
    });
    host.append(button);
  }
  host.hidden = actions.length === 0;
}

let lastOpening = null;

function attached(opening) {
  lastOpening = opening;
  const ready = canAnswer(opening);
  const isHttpMode = opening.configured && !opening.harness_name;

  empty.hidden = ready;
  line.disabled = !ready;
  send.disabled = !ready;
  line.placeholder = composerPlaceholder(opening);

  // Apply saved Chat UI design.
  const html = document.documentElement;
  applyChatUiClass(html, opening.chat_ui || "minimal");
  applyChatAppearance(opening.chat_appearance);

  const landing = document.getElementById("landing");
  const httpEmpty = document.getElementById("empty-http");
  const httpOff = document.getElementById("empty-http-off");

  landing.hidden = true;
  httpEmpty.hidden = true;
  httpOff.hidden = true;

  paintSignIn([]);
  document.getElementById("landing-sign-in-label").hidden = true;

  if (ready) {
    holdLogAtTop = false;
    return true;
  }

  if (isHttpMode) {
    if (opening.enabled) {
      httpEmpty.hidden = false;
    } else {
      httpOff.hidden = false;
    }
  } else {
    landing.hidden = false;
    landing.dataset.initializing = opening.harness?.initializing ? "true" : "false";

    const title = document.getElementById("landing-title");
    const lede = document.getElementById("landing-lede");
    const command = document.getElementById("landing-command");
    const hint = document.getElementById("landing-hint");
    const copy = landingCopy(opening);
    paintSignIn(copy.signIn, copy.signInWaiting);
    document.getElementById("landing-buttons").hidden = Boolean(copy.retry);
    retry.hidden = !copy.retry;

    drawInline(title, copy.title);
    drawInline(lede, copy.lede);
    paintFailure(landing, copy);
    const signInLabel = document.getElementById("landing-sign-in-label");
    signInLabel.textContent = copy.signInLabel ?? "";
    signInLabel.hidden = !copy.signInLabel;
    if (copy.command) {
      command.textContent = copy.command;
      command.hidden = false;
      drawInline(hint, copy.hint);
      hint.hidden = false;
    } else {
      command.hidden = true;
      hint.hidden = true;
    }
  }

  return ready;
}

// The failed landing's kicker, boxes and next step. Every other landing hides
// them. The boxes take textContent: what a launcher printed is never markup.
function paintFailure(landing, copy) {
  const failure = copy.failure ?? null;
  landing.classList.toggle("failed", failure !== null);
  const kicker = document.getElementById("landing-kicker");
  kicker.textContent = copy.kicker ?? "";
  kicker.hidden = !copy.kicker;
  document.getElementById("landing-failure").hidden = failure === null;
  for (const [part, text] of [
    ["output", failure?.output],
    ["command", failure?.command],
    ["check", failure?.nodeCheck],
  ]) {
    document.getElementById(`failure-${part}`).textContent = text ?? "";
    document.getElementById(`failure-${part}-part`).hidden = !text;
  }
  const next = document.getElementById("landing-next");
  drawInline(next, copy.next ?? "");
  next.hidden = !copy.next;
}

// Connect button: make that Harness the Completer source. The landing
// and header paint from the opening `ReloadChat` pushes. The click never
// starts the sign-in: the Harness authenticates itself in the user's own
// terminal, and fidget holds no credential.
function connect(harness, label) {
  // Nothing is repainted here: the pick goes through `SettingsSession::apply`,
  // whose `ReloadChat` pushes a full opening to the `chat-opening` listener.
  // A second read from this side would race that push.
  invoke("select_harness", { harness, instance }).catch((why) => {
    // No opening follows a refusal, so the note is the status and has to be seen.
    holdLogAtTop = false;
    console.error(`connect failed:`, why);
    note(`Could not connect to ${label}: ${why}.`);
  });
}

for (const btn of document.querySelectorAll(".connect-btn")) {
  // The button sits in the log, so the focus a click gives it scrolls the
  // landing status off the top.
  btn.addEventListener("mousedown", (event) => {
    event.preventDefault();
  });
  btn.addEventListener("click", () => {
    holdLogAtTop = true;
    log.scrollTop = 0;
    connect(btn.dataset.harness, btn.querySelector(".connect-label").textContent);
  });
}

// Retry is the same pick on the Harness already picked. It re-asks for a
// session, which is how a login run in a terminal is proved.
retry.addEventListener("click", () => {
  connect(lastOpening.harness_name, harnessDisplayName(lastOpening));
});

const settingsBtn = document.getElementById("settings-btn");
if (settingsBtn) {
  settingsBtn.addEventListener("click", () => {
    invoke("show_settings").catch((err) => {
      console.error("Failed to open Settings:", err);
    });
  });
}


// Which tab is showing. The conversation and the prompt behind it are the two
// things this window holds, and they do not fit one above the other at 420
// points (`main.rs`).
function showTab(name) {
  const prompt = name === "prompt";
  log.hidden = prompt;
  composer.hidden = prompt;
  promptTab.hidden = !prompt;
  for (const [id, on] of [
    ["tab-chat", !prompt],
    ["tab-prompt", prompt],
  ]) {
    const tab = document.getElementById(id);
    tab.setAttribute("aria-selected", String(on));
  }
  if (prompt) {
    promptText.focus();
  }
}

document.getElementById("tab-chat").addEventListener("click", () => showTab("chat"));
document.getElementById("tab-prompt").addEventListener("click", () => showTab("prompt"));

// The Instance Prompt as the Shell last told us it stands. An opening pushed
// while the user is mid-sentence must not take the sentence: only text that
// still matches what was saved is replaced.
let savedPrompt = "";

// Whether the Save button is asking for confirmation rather than offering to
// save. Saving throws the session away, so the second click does it, and never
// a keystroke, which would wipe the conversation mid-sentence (ADR-0012).
function askingToSave(asking) {
  promptSave.hidden = asking;
  promptConfirm.hidden = !asking;
  promptCancel.hidden = !asking;
  // Discard is a dirty-pair twin of Save, not of Cancel: Cancel aborts the
  // confirm, Discard would restore the field mid-ask and compete with it.
  promptDiscard.hidden = asking;
  syncPromptActions();
}

function promptDirty() {
  return promptText.value !== savedPrompt;
}

// Clean Save used to open the confirm and wipe the session for no change.
// Disabled until the field differs from what the Shell last saved.
function syncPromptActions() {
  const confirming = !promptConfirm.hidden;
  const dirty = promptDirty();
  promptSave.disabled = confirming || !dirty;
  promptDiscard.disabled = confirming || !dirty;
}

function fillFrozen(id, text) {
  const el = document.getElementById(id);
  const written = (text ?? "").trim();
  el.classList.toggle("is-empty", !written);
  // "Empty", not a collapsed box: Blank AI empties a layer rather than hiding it.
  el.textContent = written || "Empty";
}

function showPrompt(opening) {
  fillFrozen("instructions", opening.instructions);
  fillFrozen("personality", opening.personality);
  // Said before it is hit as well as in the refusal after: the Shell owns the
  // number, so the tab reads it rather than restating it.
  document.getElementById("prompt-limit").textContent = opening.prompt_limit;
  if (promptText.value === savedPrompt) {
    promptText.value = opening.instance_prompt;
  }
  savedPrompt = opening.instance_prompt;
  syncPromptActions();
}

promptText.addEventListener("input", syncPromptActions);
promptText.addEventListener("change", syncPromptActions);

promptSave.addEventListener("click", () => {
  if (!promptDirty()) {
    return;
  }
  promptSaid.textContent = "";
  askingToSave(true);
});

promptDiscard.addEventListener("click", () => {
  promptText.value = savedPrompt;
  promptSaid.textContent = "";
  askingToSave(false);
});

promptCancel.addEventListener("click", () => askingToSave(false));

promptConfirm.addEventListener("click", () => {
  askingToSave(false);
  // Refused by the Shell rather than cut here: the bound is one number, in one
  // place, and the words that did not fit are still in the box to be cut by
  // the person who wrote them.
  invoke("chat_prompt", { instance, text: promptText.value })
    .then(() => {
      savedPrompt = promptText.value.trim();
      promptText.value = savedPrompt;
      promptSaid.textContent = "Saved.";
      // The log is cleared by `chat-session`: saving reopens the session, and
      // that event is the one place a replacement is drawn (ADR-0012).
      syncPromptActions();
    })
    .catch((why) => {
      promptSaid.textContent = String(why);
      syncPromptActions();
    });
});

function showWho(opening) {
  them = opening.name;
  document.getElementById("name").textContent = opening.name;
  document.getElementById("character").textContent = opening.character;
  // Refilled on every opening, not only the first: the Completer source can
  // move, and a mode label that keeps the mode it opened with is a lie.
  drawInline(document.getElementById("mind-text"), mindLine(opening));
  for (const node of document.querySelectorAll(".i-name")) {
    node.textContent = opening.name;
  }
  for (const node of document.querySelectorAll(".i-character")) {
    node.textContent = opening.character;
  }
  if (!line.disabled) {
    line.placeholder = composerPlaceholder(opening);
  }
}

// A textarea does not submit on Enter. Enter still sends, the whole muscle
// memory of this window; Shift+Enter types the newline. `isComposing` is the
// IME's Enter accepting a candidate, and sending there would cut the word.
line.addEventListener("keydown", (event) => {
  if (recall.consume(event, line)) {
    return;
  }
  if (event.key !== "Enter" || event.shiftKey || event.isComposing) {
    return;
  }
  // Otherwise the newline lands in the field as well as sending the turn.
  event.preventDefault();
  composer.requestSubmit();
});

composer.addEventListener("submit", (event) => {
  event.preventDefault();
  const text = line.value.trim();
  if (!text) {
    return;
  }

  // Asked again on every send rather than subscribed to: a Completer attached
  // in Settings while this window is open has to reach it, and nothing else
  // here needs to know the moment it changes.
  invoke("chat_opening", { instance })
    .then((opening) => {
      // The header too, not only whether anything can answer: this opening is
      // where a Harness that came up after the window opened is noticed, and a
      // header still saying `not running` over a live session is a lie.
      showWho(opening);
      if (!attached(opening)) {
        return;
      }
      line.value = "";
      thinking.landed();
      const turn = turns.typed();
      turn.you = said("You", text, "you");
      recall.release();
      turn.them = opening_answer();
      return invoke("chat_send", { instance, text }).catch((why) => {
        drop(turn);
        throw why;
      });
    })
    .catch((why) => {
      console.error("chat: the line did not reach the frame loop:", why);
      note("That did not get through.");
    });
});

// A line that will never be answered takes its rows with it, rather than
// leaving the user looking at a question in the log that nothing is working on.
function drop(turn) {
  turns.drop(turn);
  turn.you.remove();
  turn.them.remove();
}

// The session behind this window was replaced, and the Shell says why. The rows
// go: a transcript above the composer claims that what answers next has read
// it. A note in their place, or the log reads as the app losing the conversation.
function newSession(why) {
  // Keeping `empty` is not tidiness: the empty-state panel is a child of the
  // log, and `attached()` reaches into it by id on every opening. Sweeping it
  // out with the rows leaves that lookup dereferencing null.
  log.replaceChildren(empty);
  recall.release();
  thinking.clear();
  thinkingRows.clear();
  // Not a child of the log, so replacing the rows above does not clear it.
  showPlan([]);
  turns.clear();
  asks.clear();
  // A boundary is where a stamp should say the hour again rather than count
  // minutes from a line that is no longer on screen.
  previousAt = null;
  note(`New session — ${why}. Nothing said earlier is in it.`);
}

async function start() {
  // Addressed to this window's label. Not optional: a listener registered with
  // no target is an `Any` listener that hears every emit, so two Chat surfaces
  // would each render the other's answers.
  await listen(
    "chat",
    ({ payload }) => {
      if (payload.you) {
        said("You", payload.said ?? "", "you", payload.at);
        return;
      }
      if (payload.thought) {
        thinking.kept(payload.said ?? "", payload.at);
        return;
      }
      if (payload.busy) {
        const refused = turns.popNewest();
        if (refused) {
          drop(refused);
        }
        note("Still answering the last one — ask again when it lands.");
        return;
      }
      // Whatever the turn said, its thinking is over.
      thinking.landed();
      if (payload.reacting_to) {
        // A line the user did not type, in the log as well as the bubble so the
        // conversation has one place to be read (ADR-0018). Labelled with what
        // drew it out, or it reads as the answer to whatever is above it.
        said(`${them} · ${payload.reacting_to}`, payload.said, "them", payload.at);
        return;
      }
      const outcome = turns.settle(payload);
      if (outcome.action === "orphan") {
        // An answer with no question in this window: the Instance was asked
        // somewhere else, or this window opened after the line was sent.
        said(them, outcome.said, "them", payload.at);
        return;
      }
      const turn = outcome.turn;
      settled(turn.them);
      if (outcome.action === "speech") {
        arrived(turn.them, outcome.said);
      } else if (outcome.action === "failure") {
        turn.them.remove();
        harnessError(outcome.said);
      } else if (outcome.action === "error") {
        // The Harness answered with an error. Static weights took the turn
        // either way, so the row looks like the one below; the error is the
        // only part the user can act on (ADR-0008).
        turn.them.remove();
        note(outcome.note);
      } else if (outcome.action === "preempted") {
        // The question stays in the log, and this is what closes it: a log
        // that simply stops reads as one still waiting for an answer.
        turn.them.remove();
        note(outcome.note);
      } else if (outcome.action === "silent") {
        turn.them.remove();
      } else {
        // No line: the call failed and static weights took over, silent by
        // contract, or Do Not Disturb refused. Said out loud, because a log
        // that stops is indistinguishable from one still waiting.
        turn.them.remove();
        note(MISSING_ANSWER);
      }
    },
    { target: chat.label },
  );

  await listen(
    "chat-thought",
    ({ payload }) => {
      thinking.thought(payload);
    },
    { target: chat.label },
  );

  await listen(
    "chat-plan",
    ({ payload }) => {
      showPlan(planSteps(payload));
    },
    { target: chat.label },
  );

  await listen(
    "chat-permission",
    ({ payload }) => {
      asked(payload);
    },
    { target: chat.label },
  );

  await listen(
    "chat-elicitation",
    ({ payload }) => {
      elicited(payload);
    },
    { target: chat.label },
  );

  await listen(
    "chat-permission-settled",
    ({ payload }) => {
      retire(payload.request, payload.option);
    },
    { target: chat.label },
  );

  await listen(
    "chat-status",
    ({ payload }) => {
      status = payload;
      const ms = payload.wake_ms ?? null;
      wakeAt = ms === null ? null : performance.now() + ms;
      paint();
    },
    { target: chat.label },
  );

  await listen(
    "chat-session",
    ({ payload }) => {
      newSession(payload);
    },
    { target: chat.label },
  );

  await listen(
    "names-hint",
    ({ payload }) => {
      paintNames(names.receive(payload));
    },
    { target: chat.label },
  );

  // Full opening, not only name and Character: a Director or Completer-source
  // change has to re-run `attached()` on a window that is already listening.
  await listen(
    "chat-opening",
    ({ payload }) => {
      showWho(payload);
      attached(payload);
      showPrompt(payload);
    },
    { target: chat.label },
  );

  // Chat UI selection changed: swap the root class.
  await listen(
    "chat-ui",
    ({ payload }) => {
      applyChatUiClass(document.documentElement, payload);
    },
    { target: chat.label },
  );

  await listen(
    "chat-appearance",
    ({ payload }) => {
      applyChatAppearance(payload);
    },
    { target: chat.label },
  );

  // Both listeners are up, so the state as it stands can be asked for. The bar
  // is pushed on change and a window opened between two of them would sit at
  // dashes until the sprite next did something different.
  invoke("chat_ready", { instance }).catch((why) => {
    console.error("chat: the status bar could not ask for a first push:", why);
  });

  const opening = await invoke("chat_opening", { instance });

  // Apply Chat UI design immediately to prevent FOUC.
  applyChatUiClass(document.documentElement, opening.chat_ui || "minimal");
  applyChatAppearance(opening.chat_appearance);

  showWho(opening);
  attached(opening);
  showPrompt(opening);
  line.focus();
}

// Only the countdown moves between pushes, and it moves once a second.
paint();
setInterval(paint, 1000);

start().catch((why) => {
  // Not knowing who this window belongs to or whether anything can answer
  // makes it a field that takes lines nobody reads, so say so rather than
  // showing an empty log.
  console.error("fidget could not open the Chat surface:", why);
  note("This window could not reach Fidget.");
});
