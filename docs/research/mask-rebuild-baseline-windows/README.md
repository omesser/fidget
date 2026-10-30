# Windows mask-rebuild baseline evidence (#428)

The debug summaries were measured on DESKTOP-UQIE144 at tip `a38ba3b48bb43ed4b909cb26866b6b1af97dbcd1` (PR #981 instrumentation) on 2026-09-24.

## Files

- `scenarios-release.txt` — 2026-10-30 release runs, every #428 scenario (supersedes the debug summaries)
- `environment.txt` — host/OS/display/binary configuration
- `idle-summary.txt` — cursor away, 0 rebuilds / 12s
- `walk-summary.txt` — walk-only subset under cursor
- `fast-summary.txt` — BMO react under cursor
- `large-summary.txt` — Black Mage @3x under cursor
- `walk-mask-rebuild-sample.log` — sample `mask_rebuild:` log lines from walk bout
- `fast-mask-rebuild-sample.log` — sample `mask_rebuild:` log lines from fast/react
- `large-mask-rebuild-sample.log` — sample `mask_rebuild:` log lines from Black Mage

Full TRACE logs remain on the measurement workstation.
