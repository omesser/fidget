// Which e2e leaves a pull request's changes call for, on one platform.
// Usage: node scripts/e2e-scope.mjs <windows|linux|macos> < changed-paths
// Prints `run=` and `scenarios=` lines for $GITHUB_OUTPUT; an empty
// `scenarios` with `run=true` means every leaf.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { basename } from "node:path";

const platform = process.argv[2];
const suffix = { windows: ".win.ps1", linux: ".x11.sh", macos: ".sh" }[platform];
const ci = "scripts/scenarios/ci/";
const helpers = "scripts/scenarios/helpers/";

// macOS leaves are the `.sh` ones that are not Linux's `.x11.sh`.
const isLeaf = (name) => name.endsWith(suffix) && !(platform === "macos" && name.endsWith(".x11.sh"));

const changed = readFileSync(0, "utf8").split("\n").filter(Boolean);
const leaves = changed
  // A deleted leaf is in the list too, and has nothing to run.
  .filter((path) => path.startsWith(ci) && isLeaf(path) && existsSync(path))
  .map((path) => path.slice(ci.length, -suffix.length));

// Names `file` whole: `wait-helpers.sh` is not named by `wait-helpers.mac.sh`.
const names = (text, file) => new RegExp(`(?<![\\w.-])${file.replace(/[.]/g, "\\.")}(?![\\w-])`).test(text);

// The helpers this platform's leaves load, and the helpers those load in turn.
const sources = readdirSync(ci)
  .filter(isLeaf)
  .map((name) => readFileSync(ci + name, "utf8"));
const used = new Set();
for (let grew = true; grew; ) {
  grew = false;
  for (const name of existsSync(helpers) ? readdirSync(helpers) : []) {
    if (!used.has(name) && sources.some((text) => names(text, name))) {
      used.add(name);
      sources.push(readFileSync(helpers + name, "utf8"));
      grew = true;
    }
  }
}

// A helper can change what any leaf that loads it does, so it runs them all.
const helperChanged = changed.some((path) => path.startsWith(helpers) && used.has(basename(path)));

if (helperChanged) {
  process.stdout.write("run=true\nscenarios=\n");
} else {
  process.stdout.write(`run=${leaves.length > 0}\nscenarios=${leaves.join(" ")}\n`);
}
