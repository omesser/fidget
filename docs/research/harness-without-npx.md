# Running the Claude, Codex and Pi presets without `npx`

Anchor: `8fdd3a77` on `main`, 2026-09-28. Every Fidget citation below is read
against that tree. Measurements ran the same day on macOS 25.6 (Apple silicon),
Node.js 20.19.2 and npm 11.16.0 from nvm, Bun 1.4.2, Deno 2.9.7. Nothing signed
in, no Fidget ran, no GUI opened. The ACP probe sent `initialize` only, so no
session started and no Harness CLI was spawned by an adapter.

This note answers the owner's question "removing the need for npx or bundling
it or some alternative for it doing the same thing". It builds on
[ADR-0035](../adr/0035-declare-runtime-deps-and-fail-loudly.md), the
[runtime dependency census](./runtime-dependencies.md) (#735) and the closed
spike #928, which asked the bundling question and was closed on the owner's
call before it measured anything. Versions read:

| Package | Version | License | Source |
|---|---|---|---|
| `@agentclientprotocol/claude-agent-acp` | 0.82.0 | Apache-2.0 | npm registry, `npm view` |
| `@anthropic-ai/claude-agent-sdk` (its dependency) | 0.3.280, bundling `claude` 2.1.280 | "© Anthropic PBC. All rights reserved" | the tarball's `LICENSE.md`, `manifest.json` |
| `@agentclientprotocol/codex-acp` | 2.0.0 | Apache-2.0 | npm registry |
| `@openai/codex` (its dependency) | 0.158.0 | Apache-2.0 | npm registry |
| `pi-acp` | 0.0.34 | MIT | npm registry |
| `pi` (`earendil-works/pi`) | 0.87.1 | | GitHub release `v0.87.1` |
| Node.js | v24.21.0 (Active LTS, "Krypton") | Node.js license, 2,946-line `LICENSE` | https://nodejs.org/dist/index.json |

Claims are labelled **Measured** (ran it here), **Source** (read in a
package's code, docs or release assets, cited) or **Inferred**.

---

## What Fidget runs today

`launch` (`src-tauri/src/harness.rs:101`) maps three presets to `npx`:

- `claude` → `npx -y @agentclientprotocol/claude-agent-acp@latest` (harness.rs:108-111)
- `codex` → `npx -y @agentclientprotocol/codex-acp@latest` (harness.rs:112-115)
- `pi` → `npx -y pi-acp@latest` (harness.rs:133), and `named_login` sends Pi's sign-in through `npx` too (harness.rs:2056)

The comment above them says `@latest` is load-bearing: "npx serves the first
cache it built, and the adapter bundles the Claude Code it was built against"
(harness.rs:105-107). `install_page` names Node.js for `npx`
(harness.rs:1999-2001). `exited` adds the `node --version` hint
(harness.rs:1965-1975). `attach_timeout` gives `initialize` ten seconds because
"`npx` starting Zed's adapter cold is slower than any reply" (harness.rs:700-704).

## What an adapter actually is

The three packages are not one shape. The shape decides what each option costs.

- **claude-agent-acp is JS over a 217 MB native `claude`:** Source: its
  `dist/acp-agent.js` `claudeCliPath` resolves
  `@anthropic-ai/claude-agent-sdk-<platform>-<arch>/claude`, an
  `optionalDependencies` entry of the SDK. The SDK's `manifest.json` lists that
  binary at 217,254,576 bytes on darwin-arm64 and pins it to `claude` 2.1.280.
  `CLAUDE_CODE_EXECUTABLE` overrides the lookup (acp-agent.js:626-629, 6592).
- **codex-acp is one bundled JS file over a 331 MB native `codex`:** Source:
  its tarball holds a single `dist/index.js` (1,506,033 bytes). It spawns
  `node @openai/codex/bin/codex.js app-server`, or `$CODEX_PATH app-server`
  when that is set (`startCodexConnection`, dist/index.js:27303-27311).
  `@openai/codex` pulls `@openai/codex-darwin-arm64` at 331,047,086 bytes
  unpacked. The README documents `CODEX_PATH`.
- **pi-acp is JS over the user's `pi`:** Source: `getPiCommand` spawns `pi`
  (or `pi.cmd`) from `PATH` (pi-acp dist/index.js:61-66). Nothing native ships
  with it. `pi` itself ships as Bun-compiled binaries, 30 to 45 MB per platform,
  on its GitHub release (`scripts/build-binaries.sh:131-133` runs
  `bun build --compile`).

**The user already has the native CLI.** Fidget tells a `claude` user to run
`claude /login` and a `codex` user to run `codex login` (`named_login`), and
pi-acp cannot work without `pi` on `PATH`. So the 217 MB and 331 MB copies that
`npx` downloads are a second install of a program the user already has.
**Inferred** from the login table and the adapters' own override variables.

## Option 5 first: what `npx` costs today

**Measured**, each package in its own empty `npm_config_cache`:

| Adapter | Cold first run | Warm run | Download (`_cacache`) | Installed (`_npx/<hash>`) |
|---|---|---|---|---|
| claude-agent-acp 0.82.0 | 12.5 to 14.5 s | 0.6 to 0.9 s | 135 MB | 263 MB |
| codex-acp 2.0.0 | 14.3 to 17.6 s | 0.6 s | 152 MB | 346 MB |
| pi-acp 0.34 | 1.3 s | 0.6 s | 5.4 MB | 7.1 MB |

- **Cold start beats the attach timeout only on a fast link:** The claude and
  codex cold runs sit above `attach_timeout`'s ten seconds on this machine's
  connection. **Inferred**: a slower link makes the first attach time out. An
  npx cache hit is fast; the ten seconds was sized for the cold case.
- **Offline works once cached:** `npm_config_offline=true` served both the
  pinned spec and `@latest` from cache in 0.76 s. **Measured.** npm documents
  `offline` as "Any packages not locally cached will result in an error"
  (npm/cli v11.16.0 `docs/lib/content/commands/npm-exec.md`, "A note on caching").
- **The cache is npm's, and it only grows:** npx installs "to a folder in the
  npm cache" (npm-exec.md, line 23). Each spec resolution gets its own
  `_npx/<hash>` directory. This Mac's `~/.npm/_npx` holds 30 of them, 1.1 GB.
  **Measured.** Every `@latest` bump of claude-agent-acp is another 135 MB
  download and a new directory. **Inferred.**
- **`@latest` means the registry picks the code:** claude-agent-acp published 15
  stable versions in the 30 days to 2026-09-28, codex-acp 8, pi-acp 1.
  **Measured** from `npm view <pkg> time`. Transitive ranges float too:
  codex-acp asks for `@openai/codex` `^0.158.0`, and npx has no lockfile to hold
  it. **Source**, npm registry metadata.
- **`engines` does not stop an old Node:** claude-agent-acp declares
  `node >=22`. On Node 20.19.2 npm printed `EBADENGINE` and the adapter still
  answered `initialize`. **Measured.** Whether a full session works on 20 is
  untested.

The failure modes the user sees are the ones ADR-0035 already covers: `npx`
missing, Node broken, a timeout on a cold first attach, and a registry that is
unreachable before the first cache.

## Option 1: adapters shipping native binaries

**None of the three does today.**

- **codex-acp did, and stopped:** `zed-industries/codex-acp` was a Rust
  adapter. Its last release, v0.16.0 on 2026-06-08, carried eight per-platform
  archives (darwin, linux gnu and musl, windows, on x86_64 and aarch64). The
  repository is archived. **Source**, GitHub releases API. Its successor,
  `agentclientprotocol/codex-acp`, is TypeScript, and its releases v1.13.1 and
  v2.0.0 carry no assets. **Source**, GitHub releases API.
- **claude-agent-acp and pi-acp publish npm packages only:** Their latest
  GitHub releases (v0.82.0, v0.0.34) carry no assets. **Source.**
- **The ACP registry lists all three as `npx` only:** `claude-acp` 0.82.0,
  `codex-acp` 1.13.1 and `pi-acp` 0.0.34, each with a single `distribution.npx`.
  **Source**, https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json,
  `version` 1.0.0, fetched 2026-09-28.
- **No first-party ACP mode exists yet:** `claude --help` (2.1.283),
  `codex --help` (0.157.1) and `pi --help` (0.87.1) name no ACP mode. Codex has
  `app-server`, which codex-acp wraps, and pi has `--mode rpc`, which pi-acp
  wraps. **Measured.** Requests are open upstream:
  [openai/codex#30052](https://github.com/openai/codex/issues/30052) and
  [anthropics/claude-code#24411](https://github.com/anthropics/claude-code/issues/24411).

**Agree with ADR-0035.** Its named reopening trigger, a first-party ACP binary
for Claude or Codex, is still the one that would drop Node for free. It has not
happened.

## Option 2: Fidget fetches and caches the package itself

This removes `npx`, not Node. It is the only option that changes nothing the
user has to install.

What it looks like per adapter:

- **codex, one file, no npm:** **Measured:** codex-acp 2.0.0's `dist/index.js`,
  copied alone into an empty directory and run as
  `CODEX_PATH=$(command -v codex) node codex-acp.mjs`, answered `initialize`
  with `agentInfo` and the `api-key` and `chat-gpt` auth methods. Without
  `CODEX_PATH`, and without the optional native package, it answered with error
  1001, "Codex process has exited with code 1". So Fidget would fetch one
  270 KB tarball, check it, extract one file, and run it with the user's `node`.
- **claude and pi: a pinned lockfile and `npm ci`:** Their `dist/` is not
  bundled, so they need a dependency tree. **Measured**: a cold
  `npm install --omit=optional` of claude-agent-acp 0.82.0 took 5.7 s, fetched
  37 MB and installed 56 MB, against npx's 14.5 s, 135 MB and 263 MB. The
  lockfile it wrote holds 113 `integrity` fields. pi-acp: 1.2 s, 5.4 MB, 7.1 MB.
  With `CLAUDE_CODE_EXECUTABLE` pointing at the user's `claude`, the 217 MB
  binary is never needed. That still uses the user's `npm`, which ships with
  Node, so the user installs nothing new.

What it costs Fidget:

- **Fidget owns the version bumps:** 15 claude-agent-acp releases in 30 days is
  a bump every two days. A pin that nobody bumps goes stale the same way
  `harness.rs:105-107` warns about. **Inferred.** A Renovate or Dependabot rule
  on a committed lockfile is the usual answer.
- **Version skew moves:** Pointing `CLAUDE_CODE_EXECUTABLE` at the user's
  `claude` removes the drift the harness.rs comment is about, because
  `claude update` then updates what the adapter runs. It adds a new one: the
  SDK is built against `claude` 2.1.280 (its `manifest.json`) and this Mac runs
  2.1.283. The same holds for codex-acp's `^0.158.0` against a local `codex`
  0.157.1. Whether a newer or older CLI speaks the adapter's protocol is
  untested here. **Inferred.** It needs a full session probe before it ships.
- **A cache directory and its cleanup:** Fidget's own cache replaces
  `~/.npm/_npx`, so Fidget can delete old versions. That is code Fidget writes.
- **The registry is still a network dependency on first use,** the same as
  `npx`.

This is mechanism, not a change to the decision. ADR-0035 says "Node.js and
`npx`" are declared. Fidget running `node` directly still declares Node.js; the
install page and the `node --version` hint stay. **Inferred** from the ADR text.

## Option 3: bundling a Node runtime as a Tauri sidecar

- **Size:** Node v24.21.0 downloads at 27.4 MB (darwin-arm64 `.tar.xz`),
  29.2 MB (darwin-x64), 31.9 MB (linux-x64 `.tar.xz`) and 37.6 MB (win-x64
  `.zip`). **Source**, `Content-Length` on nodejs.org/dist. The darwin-arm64
  `node` binary is 122,129,232 bytes on disk. **Measured.** Fidget's own
  v0.0.1-dev DMG is 11.1 MB, the NSIS installer 8.3 MB, the `.deb` 12.3 MB.
  **Source**, the `v0.0.1-dev` release assets. A sidecar Node roughly triples
  the download and multiplies the installed app about ten times on macOS.
- **The census understated the tree and overstated the runtime:** ADR-0035
  and the census put "a Node runtime with three adapter trees" at 50 to 100 MB.
  **Disagree, in both directions.** Done the way `npx` does it, the trees alone
  are 616 MB installed, because of the bundled `claude` and `codex` binaries.
  Done with the override variables above, they are about 65 MB, and Node is the
  largest piece. **Measured**, the tables above.
- **Signing and notarization:** Tauri bundles a sidecar through `externalBin`,
  one file per target triple, `-aarch64-apple-darwin` and so on (tauri-docs v2,
  `develop/sidecar.mdx`, lines 10-35). Node's official macOS binary is signed
  by the Node.js team (TeamIdentifier `HX7739G8FX`) with, among others,
  `allow-jit`, `allow-unsigned-executable-memory` and `get-task-allow`.
  **Measured** with `codesign -d --entitlements`. Apple's notarization guide
  says to avoid `get-task-allow` ("Resolving common notarization issues",
  developer.apple.com). So Fidget would re-sign Node with its own identity and a
  trimmed entitlement set. Fidget is ad-hoc signed today (`signingIdentity: "-"`,
  `src-tauri/tauri.conf.json`), and notarization is a follow-up per the comment
  in `.github/workflows/release.yml`. The sidecar makes that follow-up harder.
- **Update cadence:** Node 24 is Active LTS to 2026-10-20 and Maintenance to
  2028-04-30 (nodejs/Release `README.md`). Node security releases become
  Fidget releases. **Inferred.**
- **License:** Node's `LICENSE` is 2,946 lines of bundled third-party notices
  that Fidget would ship. **Measured**, line count. Bundling the adapters'
  native `claude` would also mean redistributing a binary under "All rights
  reserved" terms (`@anthropic-ai/claude-agent-sdk` `LICENSE.md`). With
  `CLAUDE_CODE_EXECUTABLE`, Fidget ships none of it.

### Node's Single Executable Applications

- **Node 24's SEA runs CommonJS only:** "The single executable application
  feature currently only supports running a single embedded script using the
  CommonJS module system" (nodejs/node v24.21.0
  `doc/api/single-executable-applications.md`, lines 31-32). All three adapters
  ship ESM (`import` statements after the shebang in each `dist/index.js`), so each needs a
  bundler pass to CommonJS first. **Source.**
- **Node 26 adds ESM and `--build-sea`:** v26.10.0's copy of the same doc names
  both. It is "Current", not LTS, until 2026-10-28. **Source.**
- **The feature is "Stability: 1.1 - Active development"** in both versions.
- **Each SEA is a full `node` binary**, so about 120 MB per adapter, or one
  binary that dispatches on `argv` to all three. macOS needs
  `codesign --remove-signature` before injection and a re-sign after (the doc's
  steps 5 and 7). **Source.** An SEA costs everything a sidecar Node costs, plus
  a bundler step per adapter release. It saves nothing on size.

## Option 4: alternative runtimes

### `bun build --compile` and `deno compile`

**Measured**, adapters installed with `--omit=optional`, then compiled on this Mac:

| Adapter | Bun 1.4.2 binary | `initialize` under Bun | Deno 2.9.7 binary | `initialize` under Deno |
|---|---|---|---|---|
| claude-agent-acp 0.82.0 | 64.9 MB | OK, 0.10 s | 107.0 MB | OK, 1.35 s |
| codex-acp 2.0.0 | 63.7 MB | OK with `CODEX_PATH`, 0.28 s; without it, "Cannot find module '@openai/codex/bin/codex.js' from '/$bunfs/root/…'" | 82.2 MB | OK with `CODEX_PATH`, 1.00 s |
| pi-acp 0.0.34 | 62.5 MB | OK, 0.03 s, but `agentInfo.version` reads `0.0.0` | 73.3 MB | OK, 0.90 s |

- **The ACP SDK and `child_process` load under both:** Every row answered
  `initialize`. That does not prove a session: `session/new` is where each
  adapter spawns `claude`, `codex app-server` or `pi`. **Inferred** risk.
- **Native binary lookup breaks inside a compiled binary:** codex-acp resolves
  `@openai/codex` through `createRequire` and spawns `process.execPath` on it
  (dist/index.js:27309-27310). Inside a Bun binary `process.execPath` is the
  adapter itself. **Measured** failure above. claude-agent-acp's
  `claudeCliPath` resolves through `createRequire` the same way, so it needs
  `CLAUDE_CODE_EXECUTABLE` too. **Inferred** from acp-agent.js:626-661.
- **Each binary carries its own runtime,** so three adapters are about 190 MB
  under Bun and 260 MB under Deno. **Measured**, the sum of the table. One
  Bun binary that dispatches on `argv` would be about 65 MB. **Inferred.**
- **Signing:** Bun documents re-signing its executables with an
  `allow-jit` entitlements plist (oven-sh/bun `bun-v1.4.2`
  `docs/bundler/executables.mdx`, "Code signing on macOS", lines 1048-1084).
  That is the same work as Node's.
- **License:** Bun is MIT but "statically links JavaScriptCore (and WebKit)
  which is LGPL-2 licensed", with the relinking obligation that brings
  (oven-sh/bun `bun-v1.4.2` `LICENSE.md`). Deno is MIT (GitHub license API).
- **Precedent:** `pi` itself ships as `bun build --compile` binaries, and so,
  going by the SDK's `extractFromBunfs.js`, does the `claude` binary. So Bun
  compile works for these authors' own CLIs. **Source** for pi, **Inferred**
  for claude.

### `bunx` and `pnpm dlx`

- **They swap one declared runtime for another:** `bunx` needs Bun and
  `pnpm dlx` needs pnpm, which runs on Node. **Source.**
- **`bunx` still runs these adapters on Node:** "If an executable is marked with
  `#!/usr/bin/env node`, Bun spins up a `node` process to execute the file"
  unless `--bun` is passed (oven-sh/bun `bun-v1.4.2` `docs/pm/bunx.mdx`,
  line 61). All three `dist/index.js` files start with that shebang. **Source.**
- **`bunx` installs every optional dependency:** claude-agent-acp's own comment
  says "bunx hydrates every optional dep" (acp-agent.js:637-638), which is both
  libc variants of the 225 MB binary on Linux.

Neither changes anything for the user except which runtime they install.

## Option 6: supply chain and update risk

| Path | Who picks the code that runs | Integrity check | Provenance |
|---|---|---|---|
| `npx …@latest` (today) | The registry, at every attach | npm checks each tarball's `sha512` against the packument it just fetched, so it proves the download, not the choice | claude-agent-acp, codex-acp and `@openai/codex` carry SLSA provenance attestations. `pi-acp` and `@anthropic-ai/claude-agent-sdk` carry none. **Source**, `npm view … dist.attestations` |
| Fidget-managed pin (option 2) | Fidget, at release | The `sha512` in a lockfile Fidget commits (113 entries for claude-agent-acp), or one pinned hash for codex-acp's single tarball | Same packages, now checkable at bump time |
| Sidecar Node or SEA (option 3) | Fidget, at release | Fidget's code signature over the bundle | Node's own release signatures, then Fidget's |
| Bun or Deno binary (option 4) | Fidget, at release | Fidget's code signature | The Bun or Deno release, plus the adapter's |

- **`pi-acp` is the weakest link today:** It is a single maintainer's package
  (`svkozak/pi-acp`) with no provenance, and `@latest` runs whatever that
  account publishes next. **Source.** A pin removes that exposure. Every pin
  also removes silent upstream fixes, which is the cost.
- **Auto-update:** Only `npx @latest` updates without a Fidget release. Every
  other option ties adapter updates to Fidget releases, or to a Fidget-run
  download of a new pin.

## Where this note agrees and disagrees with the earlier work

- **Agree:** Node is declared, and removing Node means shipping a JavaScript
  runtime. None of options 3 and 4 is small by ADR-0035's first question, and
  all of them make Fidget follow a release cadence it does not follow today.
- **Agree:** the reopening trigger is a first-party ACP mode. None exists yet.
- **Disagree on size:** The 50 to 100 MB estimate in ADR-0035 and the census
  (both labelled it Inferred) is low for the trees `npx` installs (616 MB) and
  high for trees without the bundled CLIs (about 65 MB).
- **Disagree on `@latest` being load-bearing:** The harness.rs comment is right
  about the cause: the adapter's bundled `claude` drifts. But the adapter
  exposes `CLAUDE_CODE_EXECUTABLE`, and codex-acp exposes `CODEX_PATH`. Pointing
  them at the user's own CLI fixes that drift at the root and makes a pin safe.
- **Update to the census:** "Zed's adapters" are now the `agentclientprotocol`
  organisation's. The Rust codex-acp with native binaries is archived.
- **#928's option (c), "bundle adapters-only and require Node", is close to
  option 2.** It never got measured. Fetching on first use rather than bundling
  keeps it out of the installer.

## Comparison

Costs are per preset. "Node" means the user installs Node.js.

| Option | User installs | Fidget ships | Cold first attach | Removes `npx` | Removes Node | ADR-0035 |
|---|---|---|---|---|---|---|
| Today: `npx …@latest` | Node, plus the CLI to log in | Nothing | claude 12 to 15 s, codex 14 to 18 s, pi 1.3 s | No | No | Is the decision |
| 1. Native adapter binary | The CLI | Nothing, or a download | Unknown | Yes | Yes | Its named trigger |
| 2. Fidget-managed pin, run with `node` | Node, plus the CLI | Pins, a cache and a bump rule | claude about 6 s, codex one 270 KB file, pi 1.2 s | Yes | No | Unchanged |
| 3. Sidecar Node or SEA | The CLI | 27 to 38 MB download, about 120 MB on disk per platform, re-signed | Option 2's fetch, or none if the adapters are bundled | Yes | Yes | Supersedes |
| 4a. Bun or Deno compiled adapter | The CLI | 62 to 107 MB per adapter, per platform, re-signed | None | Yes | Yes | Supersedes |
| 4b. `bunx` or `pnpm dlx` | Bun or pnpm, plus the CLI | Nothing | Same as `npx` | Yes | Swaps Node for Bun or pnpm | Supersedes for no gain |

## Recommendation

**No option beats "declare Node" for any of the three presets. Keep
ADR-0035.** Removing Node costs a runtime of 60 to 120 MB per platform, a
re-signing step and a second release train. That is five to ten times the
size of the app today.

**Do replace `npx` with a Fidget-managed pin, run by the user's `node`.** It
stays inside ADR-0035, because Node is still what is declared. Per preset:

- **codex:** fetch codex-acp's tarball at a pinned version and `sha512`, run its
  one `dist/index.js` with `node`, and set `CODEX_PATH` to the user's `codex`.
  This is the biggest win: no npm, no 331 MB download.
- **claude:** `npm ci` against a committed lockfile with `--omit=optional`, and
  set `CLAUDE_CODE_EXECUTABLE` to the user's `claude`. 37 MB instead of 135 MB,
  and `claude update` then updates what the adapter runs.
- **pi:** `npm ci` against a committed lockfile. The gain is the pin, not the
  size, and the sign-in line in `named_login` stops needing `npx` too.

Before any of that ships, one full-session probe per preset should confirm the
user's own CLI version works under the pinned adapter. That is the one
**Inferred** risk the recommendation rests on.

If the owner wants zero-Node for one preset anyway, the smallest path is one
Bun-compiled binary for that adapter, about 65 MB, with `CODEX_PATH` or
`CLAUDE_CODE_EXECUTABLE` set. That supersedes ADR-0035. This note does not
write that ADR.
