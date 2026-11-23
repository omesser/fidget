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

// Whole names only: `wait-helpers.mac.sh` does not mention `wait-helpers.sh`.
const escape = (text) => text.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
const mentions = (text, file) => new RegExp(`(?<![\\w.-])${escape(file)}(?![\\w-])`).test(text);

// Leaves load helpers, and tools such as `scripts/ax-settings.swift` too.
const files = (dir) => (existsSync(dir) ? readdirSync(dir, { withFileTypes: true }) : [])
  .filter((entry) => entry.isFile())
  .map((entry) => dir + entry.name);
const candidates = [...files(helpers), ...files("scripts/")];

// What this platform's leaves load, and what those files load in turn.
const sources = readdirSync(ci)
  .filter(isLeaf)
  .map((name) => readFileSync(ci + name, "utf8"));
const loaded = new Set();
for (let grew = true; grew; ) {
  grew = false;
  for (const path of candidates) {
    if (!loaded.has(path) && sources.some((text) => mentions(text, basename(path)))) {
      loaded.add(path);
      sources.push(readFileSync(path, "utf8"));
      grew = true;
    }
  }
}

// A loaded file can change what any leaf that loads it does, so it runs them all.
const helperChanged = changed.some((path) => loaded.has(path));

if (helperChanged) {
  process.stdout.write("run=true\nscenarios=\n");
} else {
  process.stdout.write(`run=${leaves.length > 0}\nscenarios=${leaves.join(" ")}\n`);
}
