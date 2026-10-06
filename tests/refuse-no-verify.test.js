import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { refusal } from "../scripts/refuse-no-verify.mjs";

test("a commit that skips hooks is refused", () => {
  assert.match(refusal("git commit --amend --no-verify"), /--no-verify/);
});

test("every spelling that skips hooks is refused", () => {
  for (const [command, flag] of [
    ["git commit -n -m wip", "git commit -n"],
    ["git commit -anm wip", "git commit -n"],
    ["git -C ../wt -c user.name=x commit --no-verify", "git commit --no-verify"],
    ["cargo fmt && GIT_EDITOR=true /usr/bin/git commit --amend -n", "git commit -n"],
    ["git push --no-verify origin HEAD", "git push --no-verify"],
    ["git commit -m 'ok' ; git push --no-verify", "git push --no-verify"],
  ]) {
    assert.match(refusal(command) ?? "", new RegExp(`^\`${flag}\``), command);
  }
});

test("the hook exits 2 with the reason for every harness's input shape", () => {
  const hook = fileURLToPath(new URL("../scripts/refuse-no-verify.mjs", import.meta.url));
  const run = (input) => spawnSync(process.execPath, [hook], { input: JSON.stringify(input) });
  for (const input of [
    { tool_input: { command: "git commit --no-verify" } },
    { toolInput: { command: "git commit --no-verify" } },
    { command: "git commit --no-verify" },
  ]) {
    const { status, stderr } = run(input);
    assert.equal(status, 2, JSON.stringify(input));
    assert.match(String(stderr), /^`git commit --no-verify` skips/);
  }
  assert.equal(run({ tool_input: { command: "git commit -m ok" } }).status, 0);
});

test("-n elsewhere and the flag inside a message pass", () => {
  for (const command of [
    "git log -n 5",
    "sudo -n true",
    "git push -n origin HEAD",
    `git commit -m "never use --no-verify or -n"`,
    "git commit -m 'drop -n' -s",
    `git commit -m "$(cat <<'EOF'\nSkip --no-verify.\n\ngit commit --no-verify is banned.\nEOF\n)"`,
    "echo git commit --no-verify",
    "git commit -mnope",
  ]) {
    assert.equal(refusal(command), null, command);
  }
});
