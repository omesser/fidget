# The npx presets stay on npx and the latest adapter, and Fidget probes the launcher first

**Amends:** [ADR-0035](./0035-declare-runtime-deps-and-fail-loudly.md). Its
decision to declare Node.js and `npx` stands. This ADR settles how the `npx`
presets launch, and what Fidget owes the user for that choice.

## Context

The `claude`, `codex` and `pi` presets run their ACP adapters through
`npx -y <adapter>@latest`. [Running the npx presets without npx](../research/harness-without-npx.md)
(#1147) weighed every way off `npx`:

- None of the three adapters ships a native binary today.
- A cold first run downloads the adapter and a second copy of its CLI: 12 to
  18 s for claude and codex on the machine measured, and under a second once
  cached.
- A Node sidecar is 122 MB on disk against an 11 MB installer, and needs
  re-signing for notarization.
- Bun or Deno compiled adapters are 62 to 107 MB each.
- A pinned, integrity-checked fetch run by the user's own `node` is small, and
  it can point each adapter at the CLI the user already has.

The research recommended that last option.

## Decision

The owner decided the V1 shape on 2026-09-29:

**The `npx` presets keep launching through `npx`, on the latest adapter.**
Fidget does not bundle Node.js, does not fetch or cache adapter packages
itself, and does not pin adapter versions.

**Why not pin.** Harness CLIs, their ACP adapters, Node.js and `npx` all
release very often. A pin makes Fidget own those versions: someone has to
notice each release, test it, and ship a Fidget update. And a user who updates
their own CLI can land on a combination Fidget never tested. The goal is to
take version management, and even version awareness, off both the user and
Fidget as far as possible. Riding `@latest` leaves the versions with their
owners.

**Fidget pays for that in the user experience instead:**

- A first run may take as long as the download. Nothing on the attach path
  gives up on a healthy cold start, and a slow start never reads as a failure.
- While a Harness initializes, nothing freezes. Chat says which Harness is
  starting, and that a first run may be downloading it.
- Before spawning a Harness, Fidget runs a cheap version probe of its launcher:
  `npx --version` for an `npx` preset, `<binary> --version` for the others. A
  launcher that is missing gets ADR-0035's sticky error. A launcher that is
  present but unhealthy gets its own sticky state, under the same rules.

## Rejected

- **Pinned, integrity-checked adapter fetch**: It removes the cold `npx`
  resolve and the silent upstream update. It is rejected for the version burden
  above: Fidget would own an adapter release train, and a compatibility matrix
  against whichever CLI version each user has installed. #1147 also checked the
  pinned adapters only as far as `initialize`, never a full session.
- **Bundling Node.js as a sidecar**: It fails ADR-0035's size test.
- **Compiled adapters (Bun or Deno)**: They are larger still. Bun also links
  LGPL-2 code statically.
- **`bunx` or `pnpm dlx`**: They swap one runtime the user must install for
  another, and change nothing else.

## Consequences

Each `npx` preset takes an upstream adapter release the next time it starts. A
breaking release reaches users with no Fidget change to hold it back. The
upside is that the adapter keeps pace with the CLI it drives, and nobody at
Fidget has to ship an update for it.

The first run of each `npx` preset costs the download, once per machine. The
follow-ups make that cost visible and survivable: #1160 (waits sized for a cold
start), #1161 (a clear, responsive starting state) and #1162 (the launcher
probe and its unhealthy state).

Pinning or bundling is a V2 question, with the same trigger as ADR-0035: a
first-party ACP binary for Claude or Codex, or a breaking `@latest` release
that a pin would have stopped.
