
import assert from "node:assert/strict";
import { test } from "node:test";

import { applyChatUiClass, applyPlatform, normalizeChatUi } from "../src/chat-ui-class.js";

class TestClassList {
  constructor() {
    this._classes = [];
  }

  add(className) {
    if (!this._classes.includes(className)) {
      this._classes.push(className);
    }
  }

  remove(...classNames) {
    this._classes = this._classes.filter((c) => !classNames.includes(c));
  }

  contains(className) {
    return this._classes.includes(className);
  }

  get length() {
    return this._classes.length;
  }

  toString() {
    return this._classes.join(" ");
  }
}

function createTestRoot() {
  return {
    classList: new TestClassList(),
    get className() {
      return this.classList.toString();
    },
  };
}

test("normalizeChatUi allowlists minimal, terminal, glass", () => {
  assert.equal(normalizeChatUi("minimal"), "minimal");
  assert.equal(normalizeChatUi("terminal"), "terminal");
  assert.equal(normalizeChatUi("glass"), "glass");
});

test("normalizeChatUi maps unknowns to minimal", () => {
  assert.equal(normalizeChatUi("unknown"), "minimal");
  assert.equal(normalizeChatUi("malicious<script>"), "minimal");
  assert.equal(normalizeChatUi(""), "minimal");
  assert.equal(normalizeChatUi(null), "minimal");
  assert.equal(normalizeChatUi(undefined), "minimal");
});

test("applyChatUiClass removes all designs and adds the chosen one", () => {
  const root = createTestRoot();

  applyChatUiClass(root, "minimal");
  assert.ok(root.classList.contains("chat-ui-minimal"), "minimal should be applied");
  assert.ok(!root.classList.contains("chat-ui-terminal"), "terminal should not be present");
  assert.ok(!root.classList.contains("chat-ui-glass"), "glass should not be present");
  assert.equal(root.className, "chat-ui-minimal");

  applyChatUiClass(root, "terminal");
  assert.ok(!root.classList.contains("chat-ui-minimal"), "minimal should be removed");
  assert.ok(root.classList.contains("chat-ui-terminal"), "terminal should be applied");
  assert.ok(!root.classList.contains("chat-ui-glass"), "glass should not be present");
  assert.equal(root.className, "chat-ui-terminal");

  applyChatUiClass(root, "glass");
  assert.ok(!root.classList.contains("chat-ui-minimal"), "minimal should not be present");
  assert.ok(!root.classList.contains("chat-ui-terminal"), "terminal should be removed");
  assert.ok(root.classList.contains("chat-ui-glass"), "glass should be applied");
  assert.equal(root.className, "chat-ui-glass");
});

test("applyChatUiClass normalizes unknown designs to minimal", () => {
  const root = createTestRoot();

  applyChatUiClass(root, "terminal");
  assert.ok(root.classList.contains("chat-ui-terminal"), "terminal should be applied");

  applyChatUiClass(root, "malicious<script>");
  assert.ok(!root.classList.contains("chat-ui-terminal"), "terminal should be removed");
  assert.ok(root.classList.contains("chat-ui-minimal"), "should fall back to minimal");
  assert.ok(!root.classList.contains("chat-ui-malicious<script>"), "malicious class should not be added");
  assert.equal(root.className, "chat-ui-minimal");
});

test("applyChatUiClass handles cold-open scenario", () => {
  const root = createTestRoot();

  applyChatUiClass(root, "glass");
  assert.ok(root.classList.contains("chat-ui-glass"), "glass should be applied on cold-open");
  assert.equal(root.className, "chat-ui-glass", "className should be exactly chat-ui-glass");
  assert.equal(root.classList.length, 1, "only one class should be present");
});

test("applyChatUiClass handles empty/null design gracefully", () => {
  const root = createTestRoot();

  applyChatUiClass(root, "terminal");
  assert.ok(root.classList.contains("chat-ui-terminal"), "terminal should be applied");

  applyChatUiClass(root, null);
  assert.ok(!root.classList.contains("chat-ui-terminal"), "terminal should be removed");
  assert.ok(root.classList.contains("chat-ui-minimal"), "should fall back to minimal");
  assert.equal(root.className, "chat-ui-minimal");
});

test("applyPlatform flags supported operating systems and clears unknown values", () => {
  const root = { dataset: {} };
  for (const flagged of ["linux", "macos", "windows"]) {
    applyPlatform(root, flagged);
    assert.equal(root.dataset.platform, flagged);
  }
  for (const other of ["unknown", "", undefined]) {
    applyPlatform(root, other);
    assert.equal(root.dataset.platform, undefined, String(other));
  }
});
