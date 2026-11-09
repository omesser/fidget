import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { tempDir } from "./temp-path.js";

const script = join(import.meta.dirname, "..", "scripts", "bump-crate-versions.sh");
// Probed outside the repository, where the fixtures run: inside it,
// rust-toolchain.toml finds a toolchain the fixtures wouldn't have.
const skip = spawnSync("cargo", ["--version"], { cwd: tmpdir() }).status === 0 ? false : "cargo is not usable";

// No dependencies, so cargo never reaches for the registry. The metadata table
// is a `version` key the bump has to leave alone.
function workspace(t, version) {
  const dir = tempDir(t, "crate-versions-");
  writeFileSync(join(dir, "Cargo.toml"), `[workspace]
resolver = "2"
members = ["member"]

[workspace.metadata.decoy]
version = "9.9.9"

[workspace.package]
version = "${version}"
edition = "2021"
`);
  mkdirSync(join(dir, "member", "src"), { recursive: true });
  writeFileSync(join(dir, "member", "src", "lib.rs"), "");
  writeFileSync(join(dir, "member", "Cargo.toml"), `[package]
name = "member"
version.workspace = true
edition.workspace = true
`);
  execFileSync("cargo", ["generate-lockfile", "--quiet"], { cwd: dir });
  return dir;
}

const manifest = (dir) => readFileSync(join(dir, "Cargo.toml"), "utf8");
const lock = (dir) => readFileSync(join(dir, "Cargo.lock"), "utf8");
const bump = (dir, tag) => spawnSync("bash", [script, tag], { cwd: dir, encoding: "utf8" });

test("a newer Release moves the workspace and its lockfile", { skip }, (t) => {
  const dir = workspace(t, "0.7.0");
  const run = bump(dir, "v0.8.0");
  assert.equal(run.status, 0, run.stderr);
  assert.match(manifest(dir), /\[workspace\.package\]\nversion = "0\.8\.0"\n/);
  assert.match(manifest(dir), /\[workspace\.metadata\.decoy\]\nversion = "9\.9\.9"\n/);
  assert.match(lock(dir), /name = "member"\nversion = "0\.8\.0"\n/);
});

test("versions compare as numbers, not as text", { skip }, (t) => {
  const dir = workspace(t, "0.9.0");
  assert.equal(bump(dir, "v0.10.0").status, 0);
  assert.match(manifest(dir), /\[workspace\.package\]\nversion = "0\.10\.0"\n/);
});

for (const tag of ["v0.7.0", "v0.6.4"]) {
  test(`${tag} does not move crates already at 0.7.0`, { skip }, (t) => {
    const dir = workspace(t, "0.7.0");
    const before = [manifest(dir), lock(dir)];
    const run = bump(dir, tag);
    assert.equal(run.status, 0, run.stderr);
    assert.deepEqual([manifest(dir), lock(dir)], before);
  });
}

for (const tag of ["", "v0.8.0-rc.1", "0.8", "nightly"]) {
  test(`'${tag}' is refused as a stable Release tag`, { skip }, (t) => {
    const dir = workspace(t, "0.7.0");
    const before = manifest(dir);
    assert.equal(bump(dir, tag).status, 2);
    assert.equal(manifest(dir), before);
  });
}
