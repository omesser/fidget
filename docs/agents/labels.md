# Labels

Apply these when you create an issue, open a pull request, or take one over.
`gh label list` is the tracker. Update this file when you add or remove a label.

Triage names are in `docs/agents/triage-labels.md`. Priority names and pick
order are in `docs/agents/picking-work.md`.

## How to apply

1. Add each label that matches. `gh issue edit <n> --add-label` on an issue,
   `gh pr edit <n> --add-label` on a pull request.
2. Put one of `p0`, `p1`, `p2`, `p3` on an issue.
3. Copy the issue's area and platform labels to the pull request that closes
   it. Add any label the diff matches that the issue lacks.
4. A pull request takes area and platform labels only.
5. Use a name from this file. A skill may still use a name it documents, such
   as `wayfinder:map`. Create a label only when the user asks, and add its row
   here.

Done when every matching label is on the item.

## Issues only

Triage: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`,
`wontfix`.

Priority: `p0`, `p1`, `p2`, `p3`.

| Label | When |
|---|---|
| `question` | Further information is requested. May sit with `needs-info`. |
| `good first issue` | A newcomer can take the issue. |
| `help wanted` | The issue needs extra attention. |
| `umbrella` | The issue links its child issues or pull requests. |
| `Done-when` | Unused. Leave it off. |

## Issues and pull requests

Add `macos`, `linux`, or `windows` for each platform you must verify.

On a pull request, `windows` also runs `cargo test` and the no-console check in
the `windows` job when the diff misses that job's path list in
`.github/workflows/tests.yml`. That list names every Rust file with a `cfg`
that holds on Windows or a `mod` declared under one, and
`tests/windows-gate-paths.test.js` fails when one is missing. Clippy runs in
`windows-clippy` either way. The tracker calls these the Windows compile jobs.

`e2e-windows` runs `.github/workflows/e2e-windows.yml`: every
`scripts/scenarios/ci/*.win.ps1` and the anchor scripts, live on a hosted
runner. It is too heavy for every pull request, so add it when the diff
touches Windows behavior. Remove and re-add it to run again.

`e2e-linux` runs `.github/workflows/e2e-linux.yml`: every
`scripts/scenarios/ci/*.x11.sh`, live on a hosted runner's Xvfb display. Add it
when the diff touches Linux behavior. Remove and re-add it to run again.

`e2e-macos` runs `.github/workflows/e2e-macos.yml`: every macOS leaf in
`scripts/scenarios/ci/`, live on a hosted runner. Add it when the diff touches
macOS behavior. Remove and re-add it to run again.

| Label | When |
|---|---|
| `bug` | A `fix`. Shipped behavior misbehaves. |
| `enhancement` | A `feat`. A new capability. |
| `documentation` | Documentation. |
| `testing` | The change is tests or verification scenarios. Tests inside a behavior change take that behavior's label. |
| `refactoring` | A `refactor`. Structure changes, behavior stays. |
| `performance` | A `perf` change, or CPU, GPU, memory, idle cost, or a benchmark. |
| `maintenance` | Dependency or tooling upkeep. Includes a `build` or `ci` change of that kind, and a dependency bump. |
| `harness` | The user's Harness. Launch, ACP, permissions, its tools. |
| `prompting` | Prompt shape and contents. |
| `local models` | Local model inference. |
| `website` | The GitHub Pages site: the landing page, character gallery, design pages, Fidget Cues, and the scripts that build them. |
| `security` | Secrets, passwords, the keychain, and other security work. |
| `research` | An investigation. |
| `accessibility` | A barrier for people with disabilities. |
| `duplicate` | Close it. Name the surviving item in the close comment. |
| `invalid` | Close it. It is not a real defect or change. |

A `chore` or `revert` takes the row the change matches.
