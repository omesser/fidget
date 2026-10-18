## Highlights
- **Quick message pill on Windows:** when the pill closes, Fidget gives the keyboard back to the window you were using, and a pill opened by hover stays open.
- **Hover delay setting:** a row on the Development tab in Settings sets how long the pointer rests on the character before the pill opens.
- **Chat:** a failed turn shows its error in Chat, and an ask's title shows as sent, with backticked spans as code.
- **Harness after a cancel:** Fidget no longer kills a Harness that ignores a cancel, and restarts it only if it sends nothing for 2 minutes.
- **Quick message pill on macOS:** the pill can take focus after Chat closes.

## Summary
### Features and behavior changes
- **Quick message pill on Windows:** when the pill closes, Fidget gives the keyboard back to the window you were using, and a pill opened by hover no longer closes on WebView2's activation blur. #1590, #1593
- **Quick message pill on macOS:** the pill can take focus after Chat closes. #1566
- **Hover delay setting:** the Development tab in Settings has a row for how long the pointer rests on the character before the pill opens. #1591
- **Chat:** a failed turn shows its error in Chat and clears the bubble's thinking dots, and an ask's title shows as sent, with backticked spans as code. #1511, #1605
- **Website:** the hero leads with downloads for the visitor's OS, and the landing page has a hero banner and a link to Fidget Cues. #1579, #1527, #1540

### Bug fixes
- **Silent Harness after a cancel:** Fidget no longer kills a Harness that ignores a cancel, and restarts it only if it sends nothing for 2 minutes. #1562
- **Windows drag:** the bubble stays inside the window region while you drag the character. #1569
- **Drop cue:** plays when the character lands, not when you let go. #1517
- **X11 drawing:** the overlay reuses its cached pixmap when only its position changes. #1519

### Testing
- **Event waits:** scenario and verify scripts wait on events instead of fixed sleeps. #1552, #1564, #1571, #1574, #1576, #1587
- **Webview globals:** a pinned oxlint checks the webview for undeclared names. #1596

## What's Changed
* test(verify): Prove macOS Chat perch clicks with command traces by @omesser in https://github.com/omesser/fidget/pull/1512
* fix(site): Make download buttons consistent by moving 'Download for' to a label by @omesser in https://github.com/omesser/fidget/pull/1513
* Show a failed turn in Chat and clear the bubble's thinking by @omesser in https://github.com/omesser/fidget/pull/1511
* Fix shellcheck SC2015 in chat-perch.sh by @omesser in https://github.com/omesser/fidget/pull/1521
* refactor: Rename session log operations to log_* by @omesser in https://github.com/omesser/fidget/pull/1523
* perf(x11): Cache pixmap to skip rebuilds when only position changes by @omesser in https://github.com/omesser/fidget/pull/1519
* fix(scenarios): Script-only fix for sign-in-button scenarios by @omesser in https://github.com/omesser/fidget/pull/1522
* docs: document XWayland native Wayland fullscreen limitation by @omesser in https://github.com/omesser/fidget/pull/1518
* docs: add hero banner to GitHub Pages landing page by @omesser in https://github.com/omesser/fidget/pull/1527
* Pages: assemble the site in one script by @omesser in https://github.com/omesser/fidget/pull/1536
* build(deps): bump agent-client-protocol from 3.1.0 to 3.3.0 by @dependabot[bot] in https://github.com/omesser/fidget/pull/1530
* build(deps): bump serde_json from 1.0.151 to 1.0.154 by @dependabot[bot] in https://github.com/omesser/fidget/pull/1531
* build(deps): bump tauri-plugin-global-shortcut from 2.4.0 to 2.4.1 by @dependabot[bot] in https://github.com/omesser/fidget/pull/1532
* build(deps): bump tauri from 2.12.1 to 2.12.2 by @dependabot[bot] in https://github.com/omesser/fidget/pull/1534
* build(deps): bump tauri-build from 2.7.1 to 2.7.2 by @dependabot[bot] in https://github.com/omesser/fidget/pull/1533
* Play the drop cue on landing, not on release by @omesser in https://github.com/omesser/fidget/pull/1517
* docs(agents): list the website label by @omesser in https://github.com/omesser/fidget/pull/1539
* website: link Fidget Cues from the landing page by @omesser in https://github.com/omesser/fidget/pull/1540
* bench(linux): fix the GPU compositing bench and add uncomposited and picom rows by @omesser in https://github.com/omesser/fidget/pull/1554
* test(scenarios): Wait on events, not fixed sleeps by @omesser in https://github.com/omesser/fidget/pull/1552
* docs: Show Buddy Bot in the README hero by @omesser in https://github.com/omesser/fidget/pull/1537
* test(chat-ui): pin browser color scheme preference in appearance test by @omesser in https://github.com/omesser/fidget/pull/1553
* test(chrome-cdp): cover Chrome cold start in first CDP call timeout by @omesser in https://github.com/omesser/fidget/pull/1558
* test: X11 live proof for thinking-row and chat-header-narrow (2 of 5) by @omesser in https://github.com/omesser/fidget/pull/1556
* fix(site): Clarify the cues playground CTA and Summon chat by @omesser in https://github.com/omesser/fidget/pull/1557
* test(chrome-cdp): wait for Chrome's first reply, then cap each call at 3s by @omesser in https://github.com/omesser/fidget/pull/1559
* feat(core): FIDGET_DATA_DIR override for isolated verifier runs by @omesser in https://github.com/omesser/fidget/pull/1560
* test(verify): Windows chat-perch verifier for #1502 by @omesser in https://github.com/omesser/fidget/pull/1561
* fix(harness): Retire a Harness that goes silent after a cancel by @omesser in https://github.com/omesser/fidget/pull/1562
* docs(site): finish hero banner Soft Open leftovers by @omesser in https://github.com/omesser/fidget/pull/1567
* test(scenarios): Event waits for the remaining Windows and macOS leaves by @omesser in https://github.com/omesser/fidget/pull/1564
* fix(windows): Keep the bubble in the window region while its sprite is dragged by @omesser in https://github.com/omesser/fidget/pull/1569
* test(overlay-rects): Wait on reports and drive frames from virtual time by @omesser in https://github.com/omesser/fidget/pull/1571
* test(verify): Wait on events in the verify launch, cleanup and anchor scripts by @omesser in https://github.com/omesser/fidget/pull/1574
* fix(macos): Quick message pill can be focused after chat closes by @omesser in https://github.com/omesser/fidget/pull/1566
* test(harness): Drive renderer tests over DevTools instead of a time budget by @omesser in https://github.com/omesser/fidget/pull/1576
* fix(wait-helpers): wait on the lock before checking the pid in wait_for_exit by @omesser in https://github.com/omesser/fidget/pull/1586
* feat(site): Lead the hero downloads with the visitor's OS by @omesser in https://github.com/omesser/fidget/pull/1579
* test(scenarios): X11 chat-close-pill-type leaf, and FIDGET_HOVER_DELAY_MS for test scripts by @omesser in https://github.com/omesser/fidget/pull/1580
* test(windows): Scenario waits wake on each line of Fidget's piped stderr by @omesser in https://github.com/omesser/fidget/pull/1587
* X11 chat-close-pill-type scenario: clean up with the shared wait_for_exit by @omesser in https://github.com/omesser/fidget/pull/1592
* feat(windows): Trace the pill-open focus call and add a Windows pill typing scenario by @omesser in https://github.com/omesser/fidget/pull/1583
* Settings: set the quick message hover delay on the Development tab by @omesser in https://github.com/omesser/fidget/pull/1591
* fix(windows): Keep the hover-opened pill through WebView2's activation blur by @omesser in https://github.com/omesser/fidget/pull/1593
* feat(windows): Keyboard hand-back for quick message pill by @omesser in https://github.com/omesser/fidget/pull/1590
* test(harness): Own the probe base href and skip gate in chrome-cdp by @omesser in https://github.com/omesser/fidget/pull/1595
* fix(windows): Notepad cleanup closes its window only with no tab left; drop the stale README line by @omesser in https://github.com/omesser/fidget/pull/1607
* test(globals): Lint the webview for undeclared names with a pinned oxlint by @omesser in https://github.com/omesser/fidget/pull/1596
* fix(chat): Draw an ask's title as sent, with its backticked spans as code by @omesser in https://github.com/omesser/fidget/pull/1605


**Full Changelog**: https://github.com/omesser/fidget/compare/v0.5.1...v0.6.0
