# Reminders and scheduling tools

What Fidget can bundle or call so a Character is useful for "remind me at
15:00" and "what is on my calendar" out of the box. The short answer is a
small Fidget-owned reminder tool backed by a file and a Director wake, OS
notifications through one crate, and every real calendar through the MCP
server the user already attaches to their Harness. moadim.io, the example
given, schedules agent sessions rather than reminders, and depends on `tmux`.

Dated 2026-10-06. The Fidget anchor is `aff8f919` on `main`, and every Fidget
citation below is read against that commit. External sources are linked
beside the claim.

---

## What Fidget has today

The seams a reminder feature would plug into are already built. None of them
knows about time yet.

- **Tools:** `list_tools` in `crates/core/src/dispatch.rs` declares eight
  tools: `speak`, `play_behavior`, `list_windows`, `describe_screen`,
  `recall`, `remember`, `list_instances` and `whereabouts`. `README.md`
  ("Harness ↔ MCP") is the only catalog, and the Character Prompt tells the
  model to use the tools it has without naming them (#917). A new tool is a
  new `ToolInfo` entry and a handler, and the test
  `list_tools_returns_exactly_eight_tools` moves to nine.
- **Transport:** `src-tauri/src/mcp_http.rs` serves those tools on loopback
  HTTP behind a per-run bearer token. It is request and response only: the
  module comment says "no sampling, no progress", and `docs/harness.md` ("How
  it works") adds "no notifications, progress, sampling, SSE push, or
  prompts". Every `tools/call` is handed to the frame loop over the `Call`
  channel with a five-second `ANSWER_TIMEOUT`. A reminder that *fires* cannot
  travel this way, because the Harness has to ask first. That matches the MCP
  revision `2026-07-28`, where a server initiates nothing
  ([`docs/research/fidget-harness-two-way.md`](./fidget-harness-two-way.md),
  "MCP as a reverse channel").
- **Wakes:** `crates/core/src/director.rs` is where time already enters.
  `Happened` has seven variants, and the only clock-driven one is
  `Proactive`, which `claim` classes as `Claim::Ambient`. `Pace` spaces
  proactive calls: `Pace::FIRST` is two minutes, and the wait "Grows by
  `model_base.pow(model_power)` after each proactive call, resets when the
  user addresses the character". `follow_up` in
  `crates/core/src/director/prompt.rs` already sends the clock as `HH:MM`
  through `format_clock`. A reminder firing is a new `Happened` variant, and
  `claim` will not compile until it is classed, which is the guard the enum
  exists for.
- **Storage:** Settings are one `settings.json` in the data folder
  (`src-tauri/src/settings.rs`). Memory is one `memory.md` in the same folder
  (`crates/core/src/memory.rs`), read from disk on every `recall`. Both are
  `serde_json` or plain text, with no database and no Tauri store plugin:
  `src-tauri/tauri.conf.json` lists `updater` as the only plugin.
- **Packaging:** `bundle.targets` is `dmg`, `appimage`, `deb` and `nsis`,
  and `bundle.macOS.signingIdentity` is `"-"`, an ad-hoc signature
  ([`docs/research/homebrew-release-shape.md`](./homebrew-release-shape.md)).
  There is no `src-tauri/Info.plist` and no entitlements file, so no usage
  description key is declared today.
- **The Harness side is already leaking:** #1356 (open, p1, bug) reports that
  `src-tauri/src/acp_wire.rs` drops between-turn `session/update` traffic,
  and names the product hole directly: "NL reminders ('remind me at 15:00')
  can be scheduled inside Claude's session, but when they fire under fidget
  attach the user sees nothing"
  ([#1356](https://github.com/omesser/fidget/issues/1356)).

Three seams follow from this: a Fidget tool the Harness calls to *set* a
reminder, a clock-driven Director wake when one *fires*, and the inbound
Harness wake of #1356 for schedulers that live inside the Harness.

## moadim.io

Moadim is "a loop engine for AI agents". The site describes it as
"open-source", running "Claude, Codex, Hermes, or Pi on a schedule, over MCP
and REST", and its JSON-LD lists `operatingSystem` as "macOS, Linux" and the
license as MIT ([moadim.io](https://moadim.io/)). The code is
[`moadim-io/daemon`](https://github.com/moadim-io/daemon), MIT, created
2026-06-10, last pushed 2026-10-03, latest release `v3.2.10` on 2026-09-15
(GitHub API). The crate is [`moadim`](https://crates.io/crates/moadim) 3.2.10.

What it is, from its README
([`README.md`](https://github.com/moadim-io/daemon/blob/main/README.md)):

- A Rust server that schedules **routines**, "a prompt + schedule + agent",
  stored under `~/.config/moadim/routines/`, and exposes them on one port,
  `127.0.0.1:5784`, as a browser UI, a REST API under `/api/v1` with Swagger
  at `/docs`, and an MCP endpoint at `/mcp`.
- The scheduler is in-process, "backed by `tokio-cron-scheduler`", so it
  does not depend on OS cron. `Cargo.toml` also pins `croner = "4"` and
  `rmcp = "3.1.2"` with the `server` feature.
- Every run launches the agent "inside a tmux session" in a throwaway
  workbench under `~/.moadim/workbenches/`. The prerequisites table says
  "Without `tmux`, routine runs silently fail to launch."
- The MCP tools are routine management: `list_routines`, `create_routine`,
  `update_routine`, `delete_routine`, `trigger_routine`, `snooze_routine`,
  `routine_logs`, `create_flag`, `resolve_flag`, `set_power_saving`, and
  server control (`health`, `shutdown`, `restart`).
- Auth: with the default loopback bind and no `MOADIM_API_TOKEN`, "REST/MCP
  accept same-host requests". Setting the token requires `Authorization:
  Bearer` or `X-Moadim-Token` on every request.
- Its own comparison page frames the niche as "a prompt + schedule + coding
  agent that runs unattended in a throwaway workbench, locally, with no cloud
  dependency"
  ([`docs/comparison.md`](https://github.com/moadim-io/daemon/blob/main/docs/comparison.md)).

How it could be called: a Fidget-attached Harness that also has moadim's
`/mcp` registered can create a routine whose prompt says "tell Fidget X".
The routine's agent runs in a fresh tmux session and a fresh workbench, not
in the one ACP session Fidget owns (ADR-0008), so it reaches the sprite only
if that agent has Fidget's loopback URL and per-run token, which is the
bring-your-own registration in `docs/harness.md` ("Pointing a Harness you run
yourself at Fidget") and expires at every app launch.

What I could not verify: Windows support (the README does not mention
Windows, the site says macOS and Linux, and `tmux` is a hard dependency);
whether a routine can be a one-off at a timestamp rather than a cron entry
(the README documents cron schedules and `snooze_routine`, and I did not read
the schema). Verdict: moadim is a scheduler for agent *sessions*. It is not
a reminders or calendar tool, and it brings a Rust build, `tmux`, a second
daemon and a second port for a feature a desktop companion needs in-process.

## OS-native reminders, calendars and timers

### macOS

- **EventKit** is Apple's framework to "Create, view, and edit calendar and
  reminder events"
  ([EventKit](https://developer.apple.com/documentation/eventkit)).
  `requestFullAccessToReminders(completion:)` "Prompts people to grant or
  deny read and write access to reminders" and is available from macOS 14.0
  ([requestFullAccessToReminders](https://developer.apple.com/documentation/eventkit/ekeventstore/requestfullaccesstoreminders(completion:))).
  The app "can't request read-only access to either events or reminders"; the
  choices are write-only for events, or full access
  ([Accessing the event store](https://developer.apple.com/documentation/eventkit/accessing-the-event-store)).
  `NSRemindersFullAccessUsageDescription` "is required if your app uses APIs
  that access the person's reminder data", macOS 14.0 and later
  ([NSRemindersFullAccessUsageDescription](https://developer.apple.com/documentation/bundleresources/information-property-list/nsremindersfullaccessusagedescription)).
  Tauri merges a `src-tauri/Info.plist` into the generated one: "This
  `Info.plist` file is merged with the values generated by the Tauri CLI"
  ([macOS Application Bundle](https://v2.tauri.app/distribute/macos-application-bundle/)).
  Cost: a new plist in the tree, a privacy prompt on first use, and a Rust
  binding (`objc2-event-kit` or a Swift sidecar; neither is in the tree).
  Unverified: how TCC treats an ad-hoc signed bundle across rebuilds. I did
  not find a primary source, so treat grant persistence as unknown.
- **Apple events** (`osascript`, Reminders and Calendar scripting) need
  `NSAppleEventsUsageDescription`, which "is required if your app uses APIs
  that send Apple events"
  ([NSAppleEventsUsageDescription](https://developer.apple.com/documentation/bundleresources/information-property-list/nsappleeventsusagedescription)).
  Same plist and prompt cost, with a shell dependency on top.
- **Shortcuts** run from a shell with `shortcuts run "Name"`, and take input
  with `-i` or `--input-path`
  ([Run shortcuts from the command line](https://support.apple.com/guide/shortcuts-mac/run-shortcuts-from-the-command-line-apd455c82f02/mac)).
  The user has to author the shortcut first, so this is not out of the box.
- **UserNotifications** can schedule a local notification with
  `UNCalendarNotificationTrigger(dateMatching:repeats:)`
  ([Scheduling a notification locally](https://developer.apple.com/documentation/usernotifications/scheduling-a-notification-locally-from-your-app)).
  The Tauri plugin does not use it on desktop; see the crate table.
- **launchd** fires a job on `StartCalendarInterval`, whose "semantics are
  similar to crontab(5)" (`man launchd.plist` on macOS 27.0). `atrun` ships
  with "the Disabled key set to true, so atrun is never invoked" (`man atrun`,
  same machine), so `at` is not a default macOS path.

### Windows

- **Scheduled toasts:** `ScheduledToastNotification` plus
  `ToastNotifier.AddToSchedule` shows a notification "at a later time,
  regardless of whether your app is running at that time". Delivery has "a
  window of 5 minutes"; a machine off longer than that drops it, and "a
  background task with a time trigger" is the recommended alternative
  ([Schedule an app notification](https://learn.microsoft.com/en-us/windows/apps/design/shell/tiles-and-notifications/scheduled-toast)).
  "Apps running with administrator privileges (elevated) cannot send or
  receive app notifications"
  ([App notifications overview](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/)).
  The Tauri plugin's own metadata says Windows support "Only works for
  installed apps. Shows powershell name & icon in development"
  ([plugin `Cargo.toml`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/notification/Cargo.toml)),
  which is the app-identity requirement in practice. Fidget ships NSIS, an
  installed app, so this works for a release build and not for `cargo run`.
- **Task Scheduler:** `schtasks /create /tn MyApp /tr c:\apps\myapp.exe /sc
  once /sd 01/01/2003 /st 00:00` schedules one run; `/sc hourly /mo 5`
  repeats ([schtasks create](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/schtasks-create)).
  It runs a command, not a notification, so Fidget would be its own target.

### Linux

- **Notifications** are the freedesktop Desktop Notifications Specification,
  version 1.3 (2024-08-18), whose stated use cases include "Scheduled alarm"
  ([Desktop Notifications Specification](https://specifications.freedesktop.org/notification-spec/latest/)).
  `notify-send` takes `-u` urgency, `-t` expiry in milliseconds, and `-A`
  actions ([notify-send(1)](https://manpages.debian.org/testing/libnotify-bin/notify-send.1.en.html)).
- **systemd timers:** `systemd-run` "may also be used to create and start a
  transient `.path`, `.socket`, or `.timer` unit"; with `--on-calendar=` "a
  transient path, socket, or timer unit is created alongside the service
  unit" ([systemd-run(1)](https://man7.org/linux/man-pages/man1/systemd-run.1.html)).
  Calendar shorthands include `hourly → *-*-* *:00:00` and `daily → *-*-*
  00:00:00` ([systemd.time(7)](https://man7.org/linux/man-pages/man7/systemd.time.7.html)).
  No systemd on the host means no timer.
- **cron and at:** `crontab(5)` fields are minute, hour, day of month, month,
  day of week, with `@daily` and `@reboot` nicknames
  ([crontab(5)](https://man7.org/linux/man-pages/man5/crontab.5.html)).
  `at` is POSIX, "execute commands at a later time"
  ([at(1p)](https://man7.org/linux/man-pages/man1/at.1p.html)), and needs a
  running `atd`.

Every OS scheduler above runs a *command*. For Fidget that command is Fidget,
so the OS gains nothing over an in-process timer except surviving the app
being closed, which a desktop companion that is not running cannot animate
anyway.

## Rust crates

Versions, licenses and dates are from the crates.io API on 2026-10-06
(latest version's `created_at`). Repository dates are GitHub `pushed_at`.

| Crate | Version, license, last release | What it does | Platforms | Fits where |
|---|---|---|---|---|
| [`notify-rust`](https://crates.io/crates/notify-rust) | 4.18.1, MIT OR Apache-2.0, 2026-09-27 | Desktop notifications. Linux/BSD over D-Bus via `zbus`, macOS via `mac-notification-sys`, Windows via `winrt-notification` ([README](https://github.com/hoodie/notify-rust/blob/main/README.md)) | macOS, Windows, Linux | The one notification dependency, if a reminder should also post an OS notification |
| [`tauri-plugin-notification`](https://crates.io/crates/tauri-plugin-notification) | 2.5.1, Apache-2.0 OR MIT, 2026-10-01 | `NotificationBuilder::schedule` says "Schedule this notification to fire on a later time or a fixed interval" ([docs.rs](https://docs.rs/tauri-plugin-notification/latest/tauri_plugin_notification/struct.NotificationBuilder.html)). On desktop `show()` builds a `notify_rust::Notification` and never reads `schedule` ([`desktop.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/notification/src/desktop.rs)), so a desktop schedule is silently dropped. On macOS it calls `set_application` with the bundle identifier, or `com.apple.Terminal` in dev | Full on all three per its metadata; schedule is mobile | Skip. It is `notify-rust` plus a JS API Fidget does not need |
| [`tauri-plugin-store`](https://crates.io/crates/tauri-plugin-store) | 2.5.0, Apache-2.0 OR MIT, 2026-09-30 | "persistent key-value store" to a file, usable from Rust or the webview ([Store](https://v2.tauri.app/plugin/store/)) | All | Skip. `settings.json` already shows the pattern |
| [`tauri-plugin-sql`](https://crates.io/crates/tauri-plugin-sql), [`rusqlite`](https://crates.io/crates/rusqlite) | 2.5.0 / 0.40.2 MIT, 2026-09-30 / 2026-08-08 | SQLite | All | Skip for a list of reminders |
| [`croner`](https://crates.io/crates/croner) | 4.0.1, MIT, 2026-10-02 | Cron expression parsing and next-occurrence ([croner-rust](https://github.com/hexagon/croner-rust)) | All | Only if recurring reminders use cron syntax |
| [`cron`](https://crates.io/crates/cron) | 0.17.0, MIT OR Apache-2.0, 2026-06-18 | Cron expression parser ([zslayton/cron](https://github.com/zslayton/cron)) | All | Same |
| [`tokio-cron-scheduler`](https://crates.io/crates/tokio-cron-scheduler) | 0.15.1, MIT/Apache-2.0, 2025-10-28 | Cron jobs on Tokio; moadim's scheduler | All | Skip. The frame loop already ticks; a reminder is a deadline compare |
| [`chrono-english`](https://crates.io/crates/chrono-english) | 0.2.1, MIT, 2026-08-26 | "next friday 8pm", "last April 1"; "only a limited set of patterns is supported" ([readme](https://github.com/stevedonovan/chrono-english/blob/master/readme.md)) | All | Skip. The Harness model parses language; the tool takes a timestamp |
| [`human-date-parser`](https://crates.io/crates/human-date-parser) | 0.3.1, MIT, 2025-03-27 | `from_human_time("... at 19:45", now)` ([README](https://github.com/technologicalMayhem/human-date-parser)) | All | Same |
| [`two_timer`](https://crates.io/crates/two_timer) | 2.2.5, MIT, 2023-10-10 | English expressions to a start and end range ("Monday through next Thursday") ([README](https://github.com/dfhoughton/two-timer)) | All | Same |
| [`dateparser`](https://crates.io/crates/dateparser) | 0.3.1, MIT, 2026-03-25 | "commonly used string formats"; absolute, not relative ([README](https://github.com/waltzofpearls/dateparser)) | All | Same |
| [`icalendar`](https://crates.io/crates/icalendar) | 0.17.14, MIT/Apache-2.0, 2026-09-29 | "A builder and parser for rfc5545 iCalendar", with `Todo` ([README](https://github.com/hoodie/icalendar)) | All | Reading a local `.ics` subscription, if that ever ships |
| [`ical`](https://crates.io/crates/ical) | 0.11.0, license listed as non-standard on crates.io, 2024-03-13 | iCal and vCard parser | All | Skip for the license field alone |
| [`libdav`](https://crates.io/crates/libdav) | 0.11.0, ISC, 2026-09-05 | "CalDAV and CardDAV client implementations", service discovery via `CalDavClient::bootstrap_via_service_discovery` ([docs.rs](https://docs.rs/libdav/latest/libdav/)) | All | The crate to pick if Fidget ever speaks CalDAV itself |
| [`caldav`](https://crates.io/crates/caldav) | 0.1.0, MIT, 2018-06-12, no repository | Abandoned | | Skip |

Already in the tree and enough for the minimal shape: `serde_json`, `std`
timers in the frame loop, and the `tokio` runtime features `rt`, `sync` and
`macros` in `src-tauri/Cargo.toml`.

## Calendar standards and the two big services

- **iCalendar (RFC 5545)** defines the to-do component (section 3.6.2), the
  alarm component (3.6.6) and the recurrence rule (3.8.5.3)
  ([RFC 5545](https://www.rfc-editor.org/rfc/rfc5545.txt)). A reminder is a
  `VTODO` with a `VALARM`; a repeat is an `RRULE`.
- **CalDAV (RFC 4791)** creates a calendar with `MKCALENDAR` (5.3.1) and
  queries it with the `calendar-query` REPORT (7.8)
  ([RFC 4791](https://www.rfc-editor.org/rfc/rfc4791.txt)). Apple's iCloud
  and Fastmail speak it, which is why the CalDAV MCP servers below exist.
- **Google Calendar API:** Scopes range from `calendar` ("See, edit, share,
  and permanently delete all the calendars") to `calendar.events`,
  `calendar.events.readonly`, `calendar.freebusy`, and `calendar.app.created`
  ("Make secondary Google calendars, and see, create, change, and delete
  events on them"). "If your public application uses scopes that permit
  access to certain user data, it must complete a verification process"
  ([Calendar API scopes](https://developers.google.com/workspace/calendar/api/auth)).
  Quotas are 10,000 requests per minute per project, 600 per minute per user
  per project, and a daily threshold of 1,000,000 that "You cannot request an
  increase on"
  ([Usage limits](https://developers.google.com/workspace/calendar/api/guides/quota)).
  A desktop app authorizes with the loopback redirect
  (`redirect_uri=http://127.0.0.1:<port>`) and "Incremental authorization is
  not supported for installed apps"
  ([OAuth 2.0 for iOS & Desktop Apps](https://developers.google.com/identity/protocols/oauth2/native-app)).
  A community server's setup notes show the practical cost: a Google Cloud
  project, a "Desktop app" OAuth client, the user added as a test user, and
  "While an app is in test mode the auth tokens will expire after 1 week"
  ([nspady/google-calendar-mcp README](https://github.com/nspady/google-calendar-mcp/blob/main/README.md)).
  Fidget shipping this means Fidget owning a Google Cloud project and the
  verification review.
- **Microsoft Graph:** `Calendars.ReadWrite` delegated "Allows the app to
  create, read, update, and delete events in user calendars", needs no admin
  consent, and "is available for consent in personal Microsoft accounts"
  ([Permissions reference](https://learn.microsoft.com/en-us/graph/permissions-reference)).
  The Outlook service limits each app and mailbox pair to "10,000 API
  requests in a 10-minute period" and "Four concurrent requests"
  ([Throttling limits](https://learn.microsoft.com/en-us/graph/throttling-limits)).
  Same shape as Google: an app registration Fidget owns, a token store, and a
  consent flow.
- **Local `.ics` subscriptions:** A URL to a feed is the lowest-auth calendar
  source. `icalendar` parses it; nothing else is needed. No primary source
  beyond RFC 5545 applies, and I did not evaluate feed refresh semantics.

## MCP servers for reminders and calendars

The reference repository "is dedicated to housing just the small number of
reference servers maintained by the MCP steering group" and points to the
registry for everything else
([modelcontextprotocol/servers](https://github.com/modelcontextprotocol/servers)).
The only time-related reference server is Time, with `get_current_time` and
`convert_time` over IANA zones; it "Requires MCP Python SDK 1.x" and "The
port to v2 is in progress"
([src/time](https://github.com/modelcontextprotocol/servers/tree/main/src/time)).
There is no reference reminders or calendar server.

The registry (`registry.modelcontextprotocol.io/v0/servers`, searched for
`reminder`, `calendar`, `caldav` on 2026-10-06) lists community servers only.
The ones with a repository and a recent update:

| Server | Backend and auth | License, last push | Notes |
|---|---|---|---|
| [`justinhaaheim/apple-reminders-mcp`](https://github.com/justinhaaheim/apple-reminders-mcp) | Swift, EventKit, "macOS 14.0 (Sonoma) or later", built with `swift build -c release`; "macOS will prompt you to grant Reminders access" | README says MIT; GitHub reports no detected license file. Pushed 2026-10-06 | stdio; the user builds it |
| [`FradSer/mcp-server-apple-events`](https://github.com/FradSer/mcp-server-apple-events) | TypeScript over an EventKit backend; Reminders and Calendar, "full CRUD" | MIT, pushed 2026-08-26 | npm, macOS only |
| `io.github.JonathanRReed/apple-mcp-reminders` (registry 1.0.4, 2026-09-04) | Apple Reminders on macOS | Not read | Registry entry only |
| [`nspady/google-calendar-mcp`](https://github.com/nspady/google-calendar-mcp) | Google Calendar API, user-supplied OAuth client, `npx` | MIT, pushed 2026-10-01 | Setup above |
| `io.github.ni-c/caldav-mcp`, `io.github.lukegskw/caldav-mcp`, `io.github.dominik1001/caldav-mcp` | CalDAV; events, and for ni-c "tasks and journal entries"; app passwords | Not read | Registry 0.1.x to 0.9.0, 2026-05 to 2026-09 |
| [`ridafkih/keeper.sh`](https://github.com/ridafkih/keeper.sh) | Calendar sync across Google, Outlook, iCloud, Fastmail and CalDAV, "serves as a global MCP server and API"; hosted at $5 or self-hosted with your own Google and Microsoft sign-in apps | AGPL-3.0, pushed 2026-10-05 | A service, not a sidecar |
| `app.nudgebell/reminders`, `com.reminderit/reminderit` | Hosted; email, WhatsApp, SMS or call | No repository | Paid services |

None of these is something Fidget should ship. They are the "User MCP"
class from
[`docs/research/harness-native-tools-under-acp.md`](./harness-native-tools-under-acp.md):
"the servers the user already added — mail, calendar, issue trackers". The
README already does this for desktop control, pointing at cua-driver instead
of embedding it (`README.md`, "Computer Use").

## Scheduling that already lives in the Harness

- **Claude Code, in-session:** `/loop` and the cron tools "run prompts
  repeatedly, poll for status, or set one-time reminders within a Claude Code
  session". "Tasks only fire while Claude Code is running and idle", they
  expire after seven days, and the minimum interval is one minute
  ([Run prompts on a schedule](https://code.claude.com/docs/en/scheduled-tasks)).
  Under Fidget's attach the adapter is headless and the session is Fidget's,
  so a fire arrives as a between-turn `session/update`, which is the traffic
  #1356 says `acp_wire.rs` drops today.
- **Claude Code Desktop:** Local scheduled tasks "only fire while the app is
  open and your computer is awake"; Desktop "checks the schedule every
  minute" and on wake "starts exactly one catch-up run for the most recently
  missed time"; prompts live in `~/.claude/scheduled-tasks/<task-name>/SKILL.md`
  ([Schedule recurring tasks in Claude Code Desktop](https://code.claude.com/docs/en/desktop-scheduled-tasks)).
  A separate app, separate sessions; it cannot reach Fidget's session.
- **Claude Code Routines:** "Routines are in research preview." They run on
  "Cloud, Anthropic-managed by default", have no local files ("fresh clone"),
  a minimum interval of one hour, and 100 scheduled runs per hour per account
  ([Automate work with routines](https://code.claude.com/docs/en/routines)).
  Wrong layer for a desktop reminder.
- **ACP:** The protocol index lists session updates, prompt lifecycle,
  slash commands and extensibility; nothing named timer or schedule appears
  ([agentclientprotocol.com/llms.txt](https://agentclientprotocol.com/llms.txt)).
  The agent to client direction is `session/update` notifications. A Harness
  that fires a timer can only tell the client; the client decides whether
  that is a wake. That is exactly the gap in #1356.
- **MCP:** No server-initiated direction in revision `2026-07-28`, and Claude
  Code `channels` is a Claude-only research preview
  ([`docs/research/fidget-harness-two-way.md`](./fidget-harness-two-way.md)).
  Fidget cannot push "it is 15:00" to the Harness over MCP.

## Hard constraints, per candidate

| Candidate | Permissions | Background execution | Signing and store | Verdict |
|---|---|---|---|---|
| In-process reminder in Fidget | None | Fires only while Fidget runs. Same limit as Claude Desktop tasks and Fidget's own Behaviors | None | Bundle |
| `notify-rust` OS notification | macOS notification consent on first post; Windows needs an installed app | Posts now; no deferred delivery | None beyond the bundle identifier macOS already has | Bundle, optional per reminder |
| EventKit Reminders | `NSRemindersFullAccessUsageDescription` in a new `Info.plist`, TCC prompt, full access only | Reminders.app delivers; Fidget need not run | Plist merge; TCC under ad-hoc signing unverified | Skip for v1; revisit as "mirror to Apple Reminders" |
| Windows scheduled toast | App identity (installed app) | OS delivers within a 5-minute window; dropped past it | NSIS build only; not `cargo run`; not elevated | Skip for v1 |
| `systemd-run --on-calendar` | None | OS delivers; needs systemd | None | Skip; Linux-only and still runs a command |
| Google Calendar, Microsoft Graph | Fidget-owned OAuth client, verification review, token store | Network, quotas | None | Skip; the user attaches an MCP server |
| CalDAV via `libdav` | App password, URL | Network | None | Skip for v1; cheapest in-binary calendar if ever wanted |
| Community MCP servers | Their own | Their own | Not Fidget's | Call, by documenting them |
| moadim | Loopback, optional token | Separate daemon, `tmux` | Not Fidget's | Skip |

## Recommendations

**Minimal out-of-the-box shape.** Bundle one small thing and call everything
else.

1. **Add a reminder tool pair to `list_tools` in `crates/core/src/dispatch.rs`:**
   `remind` (`message`, `at` as an RFC 3339 timestamp, optional
   `instance_id`) and `list_reminders`, with cancellation folded into
   `list_reminders` output and a `cancel_reminder` only if a Character needs
   it. The Harness model turns "in twenty minutes" into a timestamp, which is
   what it does for every other tool, so no English-date crate ships.
   Persist to one `reminders.json` beside `settings.json` with the
   `serde_json` already in the tree. Nine tools, one file, no plugin.
2. **Fire as a Director wake:** A new `Happened::Reminder(String)` in
   `crates/core/src/director.rs`, classed by `claim` as `Claim::Interaction`
   so it may take an idle call and resets `Pace`, with the reminder text in
   the follow-up. The Static Director fallback speaks the text verbatim in a
   bubble, so a reminder lands even with no Harness attached (ADR-0008 keeps
   the fallback contract). Due-check is a deadline compare in the frame loop,
   next to `session_due`.
3. **Post an OS notification with `notify-rust`** when the reminder fires and
   the display is asleep or Do Not Disturb is on, so the sprite's silence has
   a fallback. One crate, three platforms, no schedule semantics needed
   because Fidget is the scheduler.
4. **Fix #1356** so Harness-side schedulers (Claude `/loop`, cron tools)
   become inbound wakes. That is the second half of "remind me": a user who
   says it to Claude directly should get the same bubble.
5. **Document, do not bundle, calendars:** A README row beside the cua-driver
   paragraph: for Apple Reminders attach an EventKit server, for Google attach
   `nspady/google-calendar-mcp`, for iCloud or Fastmail attach a CalDAV
   server. Same "User MCP" class, same disclaimer.

**Skip.** moadim (agent-session scheduler, `tmux`, no Windows path
verified); `tauri-plugin-notification` (its schedule is mobile-only and its
desktop path is `notify-rust`); EventKit and scheduled toasts in v1 (new
plist, TCC prompt, installed-app-only, and neither exists on Linux); direct
Google or Graph clients (Fidget-owned OAuth apps and a verification review);
cron and NL-date crates (the model and a timestamp cover v1).

**Open questions.**

1. Does a reminder firing count as `Claim::Interaction` (resets `Pace`,
   takes an idle call) or as a new claim class, yes or no on reusing
   `Interaction`?
2. Should a fired reminder with no Harness attached speak the text verbatim
   from the Static Director, yes or no?
3. Should `remind` accept an `every` field (RFC 5545 `RRULE` string parsed by
   `croner` or `icalendar`) in v1, or is one-shot enough, yes or no on
   recurrence?
4. Should the OS notification post on every fire, or only when the sprite
   cannot be seen (display asleep, Do Not Disturb), yes or no on
   always-post?
5. Is "mirror reminders into Apple Reminders" wanted later, which commits the
   bundle to `NSRemindersFullAccessUsageDescription` and a TCC prompt, yes or
   no?
6. Is #1356 a prerequisite for shipping `remind`, or do they land
   independently, yes or no on the dependency?
