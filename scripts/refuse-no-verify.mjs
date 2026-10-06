#!/usr/bin/env node
// PreToolUse hook: refuses a `git commit` or `git push` that skips the hooks.

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

// Splits a shell line into simple commands of unquoted words. Enough shell to
// keep a quoted commit message one word and drop heredoc bodies; not a parser
// for every construct.
function commands(line) {
  const out = [[]];
  const heredocs = [];
  let word = null;
  let quote = null;
  const end = () => {
    if (word !== null) out.at(-1).push(word);
    word = null;
  };
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === quote) quote = null;
      else if (c === "\\" && quote === '"') word += line[++i] ?? "";
      else word += c;
    } else if (c === "'" || c === '"') {
      quote = c;
      word ??= "";
    } else if (c === "\\") {
      word = (word ?? "") + (line[++i] ?? "");
    } else if (line.startsWith("<<", i) && line[i + 2] !== "<") {
      end();
      const [match, dash, delimiter] = line.slice(i).match(/^<<(-?)\s*([^\s;&|()<>]*)/);
      heredocs.push({ end: delimiter.replace(/["'\\]/g, ""), tabs: dash === "-" });
      i += match.length - 1;
    } else if (/\s/.test(c) && c !== "\n") {
      end();
    } else if (";&|()\n".includes(c)) {
      end();
      out.push([]);
      if (c === "\n") i = skipBodies(line, i + 1, heredocs.splice(0)) - 1;
    } else {
      word = (word ?? "") + c;
    }
  }
  end();
  return out;
}

// Returns where the line resumes after each heredoc body, in order.
function skipBodies(line, at, heredocs) {
  for (const { end, tabs } of heredocs) {
    while (at < line.length) {
      const stop = line.indexOf("\n", at) + 1 || line.length;
      const text = line.slice(at, stop).replace(/\n$/, "");
      at = stop;
      if ((tabs ? text.replace(/^\t+/, "") : text) === end) break;
    }
  }
  return at;
}

const GIT_OPTION_WITH_VALUE = new Set(["-C", "-c", "--git-dir", "--work-tree", "--namespace"]);
// `git commit` short options that take a value, stuck on as in `-mfix` or as the
// next word. `-S` and `-u` take theirs stuck on only.
const COMMIT_SHORT_WITH_VALUE = "mFcCt";
const COMMIT_SHORT_WITH_STUCK_VALUE = "Su";
const COMMIT_LONG_WITH_VALUE = new Set([
  "--message", "--file", "--author", "--date", "--template", "--reuse-message",
  "--reedit-message", "--fixup", "--squash", "--trailer", "--cleanup", "--pathspec-from-file",
]);

// git accepts any unambiguous prefix of a long option; `--no-ver` is ambiguous with `--no-verbose`.
const isNoVerify = (w) => w.length >= "--no-veri".length && "--no-verify".startsWith(w);

function hookSkip(words) {
  let i = 0;
  while (/^\w+=/.test(words[i] ?? "")) i++;
  if (words[i]?.split("/").pop() !== "git") return null;
  for (i++; words[i]?.startsWith("-"); i++) {
    if (GIT_OPTION_WITH_VALUE.has(words[i])) i++;
  }
  const sub = words[i];
  if (sub !== "commit" && sub !== "push") return null;
  for (i++; i < words.length && words[i] !== "--"; i++) {
    const w = words[i];
    if (isNoVerify(w)) return `git ${sub} --no-verify`;
    if (sub !== "commit") continue;
    if (COMMIT_LONG_WITH_VALUE.has(w)) i++;
    if (!/^-[^-]/.test(w)) continue;
    for (let j = 1; j < w.length; j++) {
      if (w[j] === "n") return "git commit -n";
      if (COMMIT_SHORT_WITH_STUCK_VALUE.includes(w[j])) break;
      if (COMMIT_SHORT_WITH_VALUE.includes(w[j])) {
        if (j === w.length - 1) i++;
        break;
      }
    }
  }
  return null;
}

export function refusal(command) {
  for (const words of commands(command)) {
    const hit = hookSkip(words);
    if (hit) {
      return `\`${hit}\` skips the git hooks, and docs/agents/writing.md forbids it. Run \`pre-commit run --files <touched>\`, fix what it reports, and commit without the flag.`;
    }
  }
  return null;
}

// Claude Code sends `tool_input` and Grok Build, reading the same settings,
// `toolInput`. Exit 2 means deny to both.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const input = JSON.parse(readFileSync(0, "utf8"));
  const reason = refusal(input.tool_input?.command ?? input.toolInput?.command ?? "");
  if (reason) {
    process.stderr.write(`${reason}\n`);
    process.exit(2);
  }
}
