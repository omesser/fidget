#!/usr/bin/env node
// PreToolUse hook: refuses a `git commit` or `git push` that skips the hooks.

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

// Splits a shell line into simple commands of unquoted words. Enough shell to
// keep a quoted commit message one word; not a parser for every construct.
function commands(line) {
  const out = [[]];
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
    } else if (/\s/.test(c) && c !== "\n") {
      end();
    } else if (";&|()\n".includes(c)) {
      end();
      out.push([]);
    } else {
      word = (word ?? "") + c;
    }
  }
  end();
  return out;
}

const GIT_OPTION_WITH_VALUE = new Set(["-C", "-c", "--git-dir", "--work-tree", "--namespace"]);
// `git commit` short options whose value may follow in the same word, as in `-mfix`.
const COMMIT_OPTION_WITH_VALUE = "mFcCtSu";

function skipsHooks(words) {
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
    if (w === "--no-verify") return `git ${sub} --no-verify`;
    if (sub !== "commit" || !/^-[^-]/.test(w)) continue;
    for (let j = 1; j < w.length; j++) {
      if (w[j] === "n") return "git commit -n";
      if (COMMIT_OPTION_WITH_VALUE.includes(w[j])) {
        if (j === w.length - 1 && "mFcCt".includes(w[j])) i++;
        break;
      }
    }
  }
  return null;
}

export function refusal(command) {
  for (const words of commands(command)) {
    const hit = skipsHooks(words);
    if (hit) {
      return `\`${hit}\` skips the git hooks, and docs/agents/writing.md forbids it. Run \`pre-commit run --files <touched>\`, fix what it reports, and commit without the flag.`;
    }
  }
  return null;
}

// Claude Code sends `tool_input`, Grok Build `toolInput`, and Cursor's own
// hooks a bare `command`. Exit 2 means deny to all three.
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const input = JSON.parse(readFileSync(0, "utf8"));
  const reason = refusal(input.tool_input?.command ?? input.toolInput?.command ?? input.command ?? "");
  if (reason) {
    process.stderr.write(`${reason}\n`);
    process.exit(2);
  }
}
