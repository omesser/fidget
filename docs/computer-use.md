# Computer Use

Fidget serves no input events ([ADR-0003](./adr/0003-no-executor-harness-owns-desktop-control.md)) and takes no screenshots ([ADR-0031](./adr/0031-drop-capture-tiers.md)). An agent that needs to see and drive the desktop gets that from its Harness, or from a computer-use MCP server the user attaches, never through Fidget.

[cua-driver](https://github.com/trycua/cua) is the portable option: MIT, macOS, Windows and Linux, attached over stdio MCP. This page covers installing it, the permission each OS grants, and the config each Harness in [Harness Support](../README.md#harness-support) reads. Fidget does not ship, download, start or update cua-driver, and never picks its permission mode.

The commands and flags below were read from [cua.ai](https://cua.ai/docs/cua-driver) on 2026-11-01. Upstream moves faster than this page; the pages under [Upstream](#upstream) win when they disagree.

## Install

Run every step in the desktop session you want the agent to drive, with no administrator password. The installer picks the right path for the platform.

### macOS

Needs macOS 14 or later.

```sh
/bin/bash -c "$(curl -fsSL https://cua.ai/driver/install.sh)"
cua-driver permissions grant
cua-driver permissions status
```

`permissions grant` starts the installed `CuaDriver.app` by its exact path, so macOS attributes the two prompts to that copy of the app and the grant sticks to the driver. Neither prompt grants anything by itself: both offer **Open System Settings**, and the grant is the toggle you switch on there. macOS does not always show both in one pass, so run `permissions grant` again if `status` reports one missing.

### Windows

Needs Windows 10 or 11, and an interactive desktop session: sign-in at the console or over RDP, not a Session 0 SSH shell.

```powershell
irm https://cua.ai/driver/install.ps1 | iex
cua-driver autostart kick
```

The installer tries to register a logon task, and `kick` starts it now. If registration was skipped or failed, run `cua-driver serve` in a terminal in the desktop session and leave it open.

### Linux

Needs x86_64, an X11 or XWayland session, and AT-SPI 2. Native Wayland is opt-in per compositor, so treat X11 and XWayland as the supported path.

```sh
/bin/bash -c "$(curl -fsSL https://cua.ai/driver/install.sh)"
cua-driver serve
```

Run `serve` in a terminal inside the desktop session, so the driver can reach that session's display and accessibility bus, and leave it open. On a minimal Debian or Ubuntu image, `sudo apt install libxi6 at-spi2-core` first. On GNOME, also install the bundled WinRects helper and log out and back in once so Shell loads it:

```sh
~/.cua-driver/packages/current/wayland-helper/install.sh
gnome-extensions info winrects@cua
```

### Check the driver before wiring it up

```sh
cua-driver status
cua-driver doctor
cua-driver call list_apps
```

`status` says whether a daemon is reachable, `doctor` prints per-check probes and warnings, and `list_apps` names GUI apps the driver can see. An empty list means the driver cannot reach the desktop, and `doctor` names the permission or display problem behind it. Read the whole `doctor` report: a zero exit code does not prove the desktop is ready.

## Permissions

| OS | What it needs | Where |
|---|---|---|
| macOS | Accessibility, and Screen Recording (macOS 15 and later names that row **Screen & System Audio Recording**), both for `CuaDriver.app` | System Settings → Privacy & Security. CuaDriver gets rows of its own: Fidget's own Accessibility grant is a different row and does not cover it. |
| Windows | An interactive desktop session, and no TCC-style grant | A Medium-integrity agent cannot drive an elevated (High-integrity) window; that is UIPI, not a driver setting. An SSH shell is Session 0, which has no desktop: drive from the console or RDP session, or follow upstream's [Windows over SSH](https://cua.ai/docs/cua-driver/guides/vms-and-remote#windows-over-ssh) recipe, which uses the autostart task. |
| Linux | The daemon running inside the desktop session, with `DISPLAY` for X11 or the Wayland session present, and AT-SPI 2 on | A `serve` started from a TTY with no display reaches nothing, and a `doctor` report full of display errors is that. |

A changed macOS grant is only picked up after a full relaunch of `CuaDriver.app`. If the toggles look on and the driver still refuses, see [Troubleshoot](https://cua.ai/docs/cua-driver/guides/troubleshoot) for the stale-grant reset.

## Risk

cua-driver drives the real desktop as you do: the apps you are signed in to, your mail, your browser profiles, your files. It is not a sandbox, and it is not Fidget's.

- **The process that starts the driver picks the permission mode:** On macOS, `cua-driver mcp` proxies to `CuaDriver.app`, whose launch flags decide. On Windows and Linux, the MCP process owns its own runtime, so the client config's `env` block sets `CUA_DRIVER_PERMISSION_MODE`. The default, `standard`, allows observation and input to every app with no prompt.
- **A shell export may not reach it:** Fidget carries only `PATH` over from your login shell ([harness.md](./harness.md#finding-programs-on-path)), so a `CUA_DRIVER_PERMISSION_MODE` exported in `.zshrc` does not reach a Harness Fidget launched from Finder. Put it in the Harness config's `env` block, or on the daemon's launch flags.
- **Fidget is not in this decision:** It does not start the driver, choose a mode or write a capability manifest. [Set permissions](https://cua.ai/docs/cua-driver/guides/permissions) is the upstream page for the three modes, the capability manifest and the policy files.

## Hook it into your Harness

### Where the config has to live

Fidget starts the Harness itself, in its own data folder, and that folder is the Harness's project scope ([ADR-0030](./adr/0030-harness-cwd-is-project-scope.md)). A project-scoped file, such as `.mcp.json`, a repository `.cursor/mcp.json`, or a project `.grok/config.toml`, is looked up from there and usually misses. User-scope config loads wherever the Harness starts, so every entry below is user scope. To use a project's own config instead, name that folder in Settings → Development → "Working directory", or in `FIDGET_HARNESS_CWD`.

| OS | Fidget's data folder |
|---|---|
| macOS | `~/Library/Application Support/fidget` |
| Linux | `~/.local/share/fidget` |
| Windows | `%APPDATA%\fidget` |

A Harness reads MCP config when a session starts. Start a new session after an edit; one already open keeps the tool list it began with.

`cua-driver mcp-config --client <client>` prints the entry for most of the clients below, with the absolute path of the installed binary, and installs cua-driver's agent skill for that client. It is worth running first and comparing shapes.

### `claude`

```sh
claude mcp add --transport stdio --scope user cua-driver -- cua-driver mcp
```

`--scope user` matters. The default is `local`, which is keyed to the folder you ran the command in, and Fidget attaches from its data folder. `claude mcp list` then shows the entry wherever the Harness starts.

### `codex`

```sh
codex mcp add cua-driver -- cua-driver mcp
```

That writes user scope in `~/.codex/config.toml`, which loads alongside the `fidget_attached_<pid>` server Fidget adds for itself. Codex's own approval policy still applies to every call.

### `copilot`

Merge into `~/.copilot/mcp-config.json`:

```json
{
  "mcpServers": {
    "cua-driver": {
      "type": "local",
      "command": "cua-driver",
      "args": ["mcp"],
      "tools": ["*"]
    }
  }
}
```

Upstream has no `mcp-config` preset for Copilot CLI; this is the shape its own docs give.

### `cursor-agent`

Merge into `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "cua-driver": {
      "command": "cua-driver",
      "args": ["mcp"]
    }
  }
}
```

Cursor records an approval per project, so approve it in the folder Fidget attaches from:

```sh
cd "$HOME/Library/Application Support/fidget"   # Linux: ~/.local/share/fidget
cursor-agent mcp enable cua-driver
```

Fidget writes its own `fidget` entry into `.cursor/mcp.json` inside that same folder at attach, and removes it at detach. An entry in the user-scope file is not touched by that.

### `grok`

```sh
grok mcp add cua-driver -- cua-driver mcp
```

`grok mcp add` writes `~/.grok/config.toml` unless `--scope project` is passed, and `grok mcp doctor cua-driver` checks the result.

### `goose`

Goose calls an MCP server an extension and reads it from `~/.config/goose/config.yaml` (`%APPDATA%\Block\goose\config\config.yaml` on Windows). `goose configure` → Add extension → Command-line writes this for you; by hand, it goes under `extensions:`:

```yaml
extensions:
  cua-driver:
    type: stdio
    name: cua-driver
    cmd: cua-driver
    args: ["mcp"]
    enabled: true
    timeout: 300
```

### `opencode`

Merge into `~/.config/opencode/opencode.json`:

```json
{
  "mcp": {
    "cua-driver": {
      "type": "local",
      "command": ["cua-driver", "mcp"],
      "enabled": true
    }
  }
}
```

OpenCode also reads a project `opencode.json` from the folder it starts in, which under Fidget is the data folder. The global file is the one that always loads.

### `antigravity`

Merge into `~/.gemini/config/mcp_config.json`:

```json
{
  "mcpServers": {
    "cua-driver": {
      "command": "cua-driver",
      "args": ["mcp"]
    }
  }
}
```

`cua-driver mcp-config --client antigravity` prints the same entry. Fidget attaches Antigravity's `agy_acp_server`, and whether that server reads this file is unverified here; `/mcp list` in the Antigravity CLI is where Antigravity itself reports what it loaded.

### `hermes`

Hermes has cua-driver built in. Its `computer_use` toolset speaks MCP over stdio to a driver binary the Hermes installer pins, so do not add a second raw MCP server. Enable the toolset, then check that it landed where the `hermes acp` session Fidget starts will read it:

```sh
hermes tools enable computer_use --platform cli
hermes computer-use status
```

`computer_use` should now be among the enabled toolsets in `~/.hermes/config.yaml`. Upstream enables it for the CLI with a `-t computer_use` flag, which Fidget cannot pass to `hermes acp`, so that list is the place to add it if the session shows no `computer_use` tool in Chat. `hermes computer-use doctor` prints a per-check matrix and names the binary and the grant behind a failure, and `hermes computer-use install` repairs a missing driver.

### `pi`

Pi has no MCP client, so there is nothing to register. cua-driver ships an agent skill instead, and Pi's `bash` tool runs the driver one call at a time:

```sh
cua-driver skills install
cua-driver skills status
cua-driver call list_apps
```

`cua-driver mcp-config --client pi` prints Pi's setup. The skill teaches the tool order and the snapshot rules; without it, the model has to be told to run `cua-driver call`.

### Anything else

A Harness that reads a standard `mcpServers` block takes the generic entry, which `cua-driver mcp-config` prints with the installed path filled in:

```json
{
  "mcpServers": {
    "cua-driver": {
      "command": "cua-driver",
      "args": ["mcp"]
    }
  }
}
```

## Check it works

Open Chat on a Character Instance whose Harness you configured, and send:

> Call the cua-driver `list_apps` tool and name one app it returns. Do not answer from memory.

Chat draws the call as a folded row that names it, and the reply names a real app. A model that says it has no such tool means the config did not load: check the scope and the folder under [Where the config has to live](#where-the-config-has-to-live), then start a new session. `cua-driver call list_apps` in a terminal is the other half of the check, and separates a driver that cannot see the desktop from a Harness that cannot see the driver.

## Other drivers

One line each, no steps:

- **Windows fallback:** [Windows-MCP](https://github.com/CursorTouch/Windows-MCP) is the alternative for Windows users.
- **Wayland:** Linux users on Wayland can try [computer-use-linux](https://github.com/agent-sh/computer-use-linux).

The [Capture decision note](./research/capture-drop-and-harness-cu-path.md) has the per-Harness table and the other drivers surveyed.

## Upstream

- [Quickstart](https://cua.ai/docs/cua-driver/quickstart): install per OS, the first run, and the macOS permission flow in full.
- [Connect your agent](https://cua.ai/docs/cua-driver/guides/connect-your-agent): every supported client, the skill pack, and `mcp-config`.
- [Set permissions](https://cua.ai/docs/cua-driver/guides/permissions): the three modes, the capability manifest, and the policy files.
- [MCP tools](https://cua.ai/docs/cua-driver/reference/mcp-tools): what the server exposes, tool by tool.
- [Platform support](https://cua.ai/docs/cua-driver/concepts/platform-support): Linux window systems, browser routes, and the known limits.
- [Troubleshoot](https://cua.ai/docs/cua-driver/guides/troubleshoot): stale macOS grants, empty results, and per-app limits.
- [Operate](https://cua.ai/docs/cua-driver/guides/operate): keeping the daemon running, updates, and pinning a permission mode.
