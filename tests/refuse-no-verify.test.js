import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { test } from "node:test";

const hook = join(import.meta.dirname, "..", "scripts", "refuse-no-verify.sh");
// macOS `/bin/bash` is 3.2, the oldest the hook has to run on.
const bash = process.platform === "darwin" ? "/bin/bash" : "bash";

const run = (command, key = "tool_input") =>
  spawnSync(bash, [hook], {
    input: JSON.stringify({ tool_name: "Bash", [key]: { command, description: "run it" } }),
  });

const refused = (command, key) => {
  const { status, stderr } = run(command, key);
  return status === 2 ? String(stderr).split(" skips")[0] : status;
};

test("every spelling that skips hooks is refused", () => {
  for (const [command, flag] of [
    ["git commit --amend --no-verify", "`git commit --no-verify`"],
    ["git commit -n -m wip", "`git commit -n`"],
    ["git commit -anm wip", "`git commit -n`"],
    ["git commit --no-veri", "`git commit --no-verify`"],
    ["git -C ../wt -c user.name=x commit --no-verify", "`git commit --no-verify`"],
    ["cargo fmt && GIT_EDITOR=true /usr/bin/git commit --amend -n", "`git commit -n`"],
    ["git push --no-verify origin HEAD", "`git push --no-verify`"],
    ["git commit -m 'ok' ; git push --no-verify", "`git push --no-verify`"],
    ["true | git commit -n", "`git commit -n`"],
    ["git status\ngit commit -n", "`git commit -n`"],
  ]) {
    assert.equal(refused(command), flag, command);
  }
});

test("-n elsewhere and the flag inside a message pass", () => {
  for (const command of [
    "git log -n 5",
    "sudo -n true",
    "git push -n origin HEAD",
    `git commit -m "never use --no-verify or -n"`,
    "git commit -m 'drop -n' -s",
    `git commit -m "say \\"-n\\" twice" --message "-n is fine"`,
    `git commit -m "$(cat <<'EOF'\nSkip --no-verify.\n\ngit commit --no-verify is banned, don't.\nEOF\n)"`,
    "echo git commit --no-verify",
    "git commit -mnope",
    "git commit -m\"x\"",
  ]) {
    assert.equal(refused(command), 0, command);
  }
});

test("Grok Build's toolInput shape is read too", () => {
  assert.equal(refused("git commit --no-verify", "toolInput"), "`git commit --no-verify`");
});

test("an unquoted heredoc body is read as commands, a known false positive", () => {
  assert.equal(
    refused("cat > notes.md <<EOF\ngit push --no-verify skips hooks\nEOF"),
    "`git push --no-verify`",
  );
});
