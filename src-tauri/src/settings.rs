//! The user's standing choices, as a file they own.
//!
//! Settings is how #18 reaches the Director, hide rules, Memory, and launch
//! without finding the sprite. The document is JSON so a hand-edit is a text
//! editor, the same deal Memory already makes. Missing keys take their
//! defaults, so an older file keeps working when a field is added.

pub mod controller;
pub mod form;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
#[cfg(test)]
use std::thread;
#[cfg(test)]
use std::time::{Duration, Instant};

use fidget_core::memory::MemoryManifest;
use fidget_core::roster::InstanceSpec;
use fidget_core::visibility::HideRules;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::consent::{self, CapabilityId, ConsentRow};
use crate::dev_flags;
use crate::model::{self, DirectorInspect, DirectorSettings};
use crate::secrets::{SecretStore, DIRECTOR_API_KEY};

/// One running character, as settings lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRow {
    pub id: String,
    pub name: String,
    pub character: String,
    /// This Instance's own layer of the Character Prompt (ADR-0012). Not shown
    /// in the Settings window: these rows are also how `chat_opening` reads the
    /// roster, and the Prompt tab is where the text is read and written.
    pub prompt: String,
}

/// Light or dark for Chat, or follow the computer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatAppearance {
    #[default]
    System,
    Light,
    Dark,
}

impl ChatAppearance {
    pub fn title(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn from_title(title: &str) -> Self {
        match title.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

impl<'de> Deserialize<'de> for ChatAppearance {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ChatAppearanceVisitor)
    }
}

struct ChatAppearanceVisitor;

impl<'de> Visitor<'de> for ChatAppearanceVisitor {
    type Value = ChatAppearance;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(ChatAppearance::from_title(value))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        self.visit_str(&value)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
        Ok(ChatAppearance::System)
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
        Ok(ChatAppearance::System)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
        Ok(ChatAppearance::System)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
        Ok(ChatAppearance::System)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(ChatAppearance::System)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(ChatAppearance::System)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        Deserialize::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(ChatAppearance::System)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(ChatAppearance::System)
    }
}

/// What the settings window shows. Built from the live file and roster so the
/// window holds no copy that could drift.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SettingsView {
    pub director_enabled: bool,
    pub proactive_wakes: bool,
    pub do_not_disturb: bool,
    pub sound: bool,
    pub hidden: bool,
    pub hide_in_fullscreen: bool,
    pub launch_at_login: bool,
    pub hide_hotkey: String,
    pub excluded_applications: Vec<String>,
    pub character: String,
    pub memory_path: String,
    pub last_payload: Option<String>,
    pub installed: Vec<String>,
    pub instances: Vec<InstanceRow>,
    pub director_base_url: String,
    pub director_model: String,
    /// Whether a key is stored — never the raw secret itself.
    pub api_key_set: bool,
    pub api_key_fingerprint: String,
    /// Non-empty when the last store read failed. Distinct from unset.
    pub api_key_error: String,
    /// The Completer source popup's title in force: Off, a preset name, or
    /// Custom. The exported variable outranks the file, as on the endpoint
    /// rows (#272). The command line beside it is a `development_texts` row.
    pub harness: String,
    /// What the attachment is doing. Not attached, attached but not signed in,
    /// or attached.
    pub harness_state: String,
    /// The registration box's Harness picker, snippet and instructions (#577).
    /// The picker is the file's alone - no variable owns it, because nothing
    /// but this box reads it.
    pub byo_harness: String,
    pub byo_snippet: String,
    pub byo_steps: String,
    /// The raw token for Hermes interactive paste. Empty when snippet already
    /// embeds the token (Claude, Codex, Grok, OpenCode, Pi).
    pub byo_token: String,
    /// The Development rows, by form row id: the value in force, which is the
    /// exported variable's where it owns the row and the file's otherwise.
    ///
    /// Keyed rather than one named field per row. Six switches would be six
    /// fields here, six bindings in each window, and a seventh row would need
    /// all of that again before it did anything (#273).
    pub development_switches: HashMap<String, bool>,
    pub development_texts: HashMap<String, String>,
    /// Live OS grants, not a file field. The window rereads them on become-key.
    pub consent: Vec<ConsentRow>,
    /// The name Privacy & Security will show for this process.
    pub consent_listed_as: String,
    /// Which Chat UI design is selected: minimal, terminal, or glass.
    pub chat_ui: String,
    pub chat_appearance: ChatAppearance,
}

/// One row's value, in the three shapes the page draws.
///
/// Untagged, so a checkbox reads a bare `true` and a text row a bare string,
/// which is what `src/settings.js` indexes out of `values`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RowValue {
    Bool(bool),
    Text(String),
    /// The Instances list. Objects rather than printed lines, because Dismiss
    /// has to name an Instance by its id and a line throws the id away (#875).
    Instances(Vec<InstanceRow>),
}

/// The Development switches, by row id, as the window must draw them.
///
/// Through `dev_flags`, which is the one place that decides what an exported
/// variable does to a switch.
fn development_switches(settings: &Settings) -> HashMap<String, bool> {
    HashMap::from([
        (
            form::TRACE_FRAMES_ID.to_string(),
            dev_flags::TRACE_FRAMES.in_force(settings.trace_frames),
        ),
        (
            form::TRACE_HITTEST_ID.to_string(),
            dev_flags::TRACE_HITTEST.in_force(settings.trace_hittest),
        ),
        (
            form::TRACE_DIRECTOR_ID.to_string(),
            dev_flags::TRACE_DIRECTOR.in_force(settings.trace_director),
        ),
        (
            form::TRACE_ENGINE_ID.to_string(),
            dev_flags::TRACE_ENGINE.in_force(settings.trace_engine),
        ),
        (
            form::DIRECTOR_BLANK_ID.to_string(),
            dev_flags::DIRECTOR_BLANK.in_force(settings.director_blank),
        ),
        (form::PI_PROJECT_MCP_ID.to_string(), settings.pi_project_mcp),
        (
            form::CAPTURABLE_ID.to_string(),
            dev_flags::CAPTURABLE.in_force(settings.capturable),
        ),
    ])
}

/// Every text row a variable can own, by row id, with the same precedence.
///
/// Both windows fill such a row from this map by id, so the tab it is drawn on
/// does not matter: the wake interval and the Harness command line are the
/// AI tab's. Numbers were all it held when #273 landed it; the name is
/// the tab those first rows sat on, not a type.
fn development_texts(settings: &Settings) -> HashMap<String, String> {
    HashMap::from([
        (
            form::HARNESS_COMMAND_ID.to_string(),
            harness_in_force(settings).1,
        ),
        (
            form::DIRECTOR_TIMEOUT_SECS_ID.to_string(),
            limit_in_force::<u64>(model::TIMEOUT_SECS, &settings.director_timeout_secs),
        ),
        (
            form::DIRECTOR_MAX_TOKENS_ID.to_string(),
            limit_in_force::<u32>(model::MAX_TOKENS, &settings.director_max_tokens),
        ),
        (
            form::DIRECTOR_WAKE_SECS_ID.to_string(),
            limit_in_force::<u64>(model::WAKE_SECS, &settings.director_wake_secs),
        ),
        (
            form::HARNESS_AUTH_RETRY_SECS_ID.to_string(),
            limit_in_force::<u64>(
                crate::harness::AUTH_RETRY_SECS,
                &settings.harness_auth_retry_secs,
            ),
        ),
        (
            form::HARNESS_TURN_TIMEOUT_SECS_ID.to_string(),
            limit_in_force::<u64>(
                crate::harness::TURN_TIMEOUT_SECS,
                &settings.harness_turn_timeout_secs,
            ),
        ),
        // Not a limit either: the app cannot know which values the user's
        // host takes, so whatever was typed is shown back (#638).
        (
            form::DIRECTOR_REASONING_EFFORT_ID.to_string(),
            model::env_or_file(model::REASONING_EFFORT, &settings.director_reasoning_effort),
        ),
        // Not a limit: any path the user typed is shown back verbatim, because
        // a path this machine has not got yet is still the one to keep.
        (
            form::MCP_BIN_ID.to_string(),
            model::env_or_file(crate::harness::MCP_BIN, &settings.mcp_bin),
        ),
        (
            form::HARNESS_CWD_ID.to_string(),
            model::env_or_file(crate::harness::CWD, &settings.harness_cwd),
        ),
    ])
}

/// One Completer limit as the window must show it, in the type the read site
/// parses it as.
///
/// A value `dev_flags::seed` cannot use — blank, non-numeric, out of range, or
/// zero, which it calls unset — shows blank, so the row's placeholder names
/// the default that is in force instead. Showing it verbatim would name a
/// timeout `model::timeout_for` never reaches, and a frozen row offers no way
/// to correct that.
///
/// Blank is also what a blur over the row commits, so clicking into an
/// unusable value and out again saves the emptiness. Nothing is lost:
/// everything it can discard is a value `seed` already treats as unset, and a
/// variable that owns the row freezes it before it can be clicked.
fn limit_in_force<T>(var: &str, file: &str) -> String
where
    T: std::str::FromStr + fmt::Display + Default + PartialEq,
{
    match model::env_or_file(var, file).trim().parse::<T>() {
        Ok(value) if value != T::default() => value.to_string(),
        _ => String::new(),
    }
}

/// The Completer source rows as the window must draw them: the popup title in
/// force and the command line beside it.
///
/// `FIDGET_HARNESS` outranks the file for the reason it outranks the
/// endpoint rows — `harness::from_settings` gives it the last word, and a
/// window that printed the file's value would name a Harness the app never
/// spawns (#272). Its grammar carries a custom command line in the value
/// itself, so the file's `harness_command` has nothing to add there.
fn harness_in_force(settings: &Settings) -> (String, String) {
    // `std::env::var` rather than `model::env_override`: exported-and-empty is
    // Off here, not a fall-through, and the window has to print what
    // `harness::from_settings` will act on (#452).
    match std::env::var(crate::harness::VAR) {
        Ok(exported) => form::harness_rows(&exported, ""),
        Err(_) => form::harness_rows(&settings.harness, &settings.harness_command),
    }
}

/// What the Completer source row says about the attachment.
///
/// Not attached, attached but not signed in, or attached and answering.
/// Not signed in names the login command for the user's own terminal.
/// fidget holds no credential (ADR-0018), asks for none, and runs no login.
fn harness_state(harness: Option<&crate::harness::HarnessInspect>) -> String {
    match harness {
        None => "Not attached. The HTTP endpoint below is the AI brain.".to_string(),
        Some(attached) => match &attached.login {
            Some(login) => format!(
                "{} attached but not authenticated. Run `{login}` in a terminal - \
                 Fidget never asks for it.",
                attached.name
            ),
            // Configured and not answering is its own state, and the one that
            // must not read as attached: a Harness this machine has not got
            // leaves the handle in place while the Director runs on Static,
            // and the HTTP rows below are live because of it (#452).
            //
            // Live is not the same as in force, which is what this line used
            // to imply and #469 corrected. `completer_from` hands the Director
            // the handle for as long as one exists — a dead session falling
            // through to the HTTP Completer is the second mind ADR-0008
            // refuses. What #500 changed is only how long that lasts: the
            // retry is the Session's own, and the row above moves the handle
            // now rather than at the next launch, so the wait is bounded by
            // the user rather than by a relaunch.
            // #659 splits the first of those: a machine that never had the
            // CLI is waiting for an install, not for an answer, and an errno
            // is not a sentence that says so.
            None if !attached.alive => match &attached.missing {
                Some(command) => format!(
                    "`{command}` is not installed, so {} is not running and the fidget runs on \
                     static weights. Fidget does not bundle `{command}` - install it{}, or switch AI \
                     source to Model API to use an HTTP endpoint.",
                    attached.name,
                    crate::harness::install_page(command)
                        .map(|(_, url)| format!(" from {url}"))
                        .unwrap_or_default(),
                ),
                // #949: Apply starts the attach itself, and the handshake
                // takes a second or two. "Set but not running" over that
                // second reads as a Harness that never will be, which is what
                // sent the user looking for the switch that starts it.
                None if attached.initializing => format!(
                    "{} is starting. Chat waits until it answers, and the AI runs on static \
                     weights meanwhile.",
                    attached.name
                ),
                None if attached.unhealthy.is_some() => format!(
                    "{} is unhealthy: {} The fidget runs on static weights until it is fixed.",
                    attached.name,
                    attached
                        .unhealthy
                        .as_ref()
                        .map(crate::harness::LaunchFailure::sentence)
                        .unwrap_or_default()
                ),
                None if attached.failed.is_some() => format!(
                    "{} failed to start: {} The fidget runs on static weights until it answers.",
                    attached.name,
                    attached
                        .failed
                        .as_ref()
                        .map(crate::harness::LaunchFailure::sentence)
                        .unwrap_or_default()
                ),
                None => format!(
                    "{} is set but not running, so the fidget runs on static weights until it \
                     answers. It stays the AI brain while it is set; you may switch AI source at any \
                     time. Apply takes effect at once.",
                    attached.name
                ),
            },
            None => match &attached.session_id {
                Some(id) => format!("{} attached, session {id}.", attached.name),
                None => format!("{} attached; no session opened yet.", attached.name),
            },
        },
    }
}

/// What to paste to point a Harness the user runs themselves at this app's
/// MCP server, and what to do with it, as (snippet, instructions, raw_token).
///
/// Pure, because the eight registration shapes are the whole of what can be
/// wrong here: each was checked against the installed CLI in #580, #636 and
/// #1016, and a typo in one fails at the Harness rather than anywhere this
/// code can see.
///
/// The token goes in raw, without `Bearer `. Six of the eight templates add
/// that prefix themselves, so a pre-prefixed token reads `Bearer Bearer` in
/// the other two.
///
/// The third element (raw_token) is empty when the snippet already embeds the
/// token. For Hermes (interactive paste), it holds the raw token for the separate
/// copy field.
///
/// A name the popup cannot offer - blank, `custom`, a hand-edited file - gets
/// the pair on its own, because the pair is all any of the eight templates is
/// made of.
fn byo_registration(harness: &str, url: &str, token: &str) -> (String, String, String) {
    match harness {
        // The remove is not optional: Claude Code answers a re-add of an
        // existing name by keeping the old URL and token and exiting 0 (#580,
        // #644), and both of ours change every launch. Scope is omitted so it
        // defaults to local. `-s project` is deliberately absent - it writes a
        // checked-in `.mcp.json`, which would commit the token.
        "claude" => (
            format!(
                "claude mcp remove fidget 2>/dev/null\n\
                 claude mcp add --transport http fidget \"{url}\" \
                 --header \"Authorization: Bearer {token}\""
            ),
            "Run both lines in a terminal, then exit your Claude session and start a \
             new one with `claude`. The remove is not optional: re-adding a name Claude \
             Code already knows keeps the old URL and token and reports success. Add `-s \
             user` only if you want the same entry in every project. `claude mcp list` \
             says whether it connected."
                .to_string(),
            String::new(), // Token already in snippet
        ),
        "codex" => (
            format!(
                "export FIDGET_MCP_TOKEN='{token}'\n\
                 codex mcp add fidget --url '{url}' --bearer-token-env-var FIDGET_MCP_TOKEN"
            ),
            "Run both lines in a terminal where Codex will inherit the environment, then \
             run `mcpServer/refresh` from your Codex session to pick up the change. If \
             `mcpServer/refresh` is unavailable, exit your Codex session and open a new \
             one. Alternatively, add or update `[mcp_servers.fidget]` in \
             `~/.codex/config.toml` with `url = \"{url}\"` and `http_headers = {{ \
             \"Authorization\" = \"Bearer {token}\" }}`."
                .to_string(),
            String::new(), // Token already in snippet (export)
        ),
        // The remove is not optional: `copilot mcp add` refuses a name it
        // already holds (exit 1, "already exists. To update it, remove it
        // first"), and both the URL and the token change every launch. Whether
        // a running session re-reads `mcp-config.json` was not measured, so
        // the steps ask for a new one.
        "copilot" => (
            format!(
                "copilot mcp remove fidget 2>/dev/null\n\
                 copilot mcp add --transport http fidget \"{url}\" \
                 --header \"Authorization: Bearer {token}\""
            ),
            "Run both lines in a terminal, then start a new `copilot` session. The \
             remove is not optional: `copilot mcp add` refuses a name it already \
             holds. This writes `~/.copilot/mcp-config.json` (user scope). \
             `copilot mcp list` says whether it is registered."
                .to_string(),
            String::new(), // Token already in snippet
        ),
        "cursor-agent" => (
            format!(
                "{{\n\
                 \x20 \"mcpServers\": {{\n\
                 \x20   \"fidget\": {{\n\
                 \x20     \"url\": \"{url}\",\n\
                 \x20     \"headers\": {{\n\
                 \x20       \"Authorization\": \"Bearer {token}\"\n\
                 \x20     }}\n\
                 \x20   }}\n\
                 \x20 }}\n\
                 }}"
            ),
            "A fragment for `.cursor/mcp.json` (project) or `~/.cursor/mcp.json`, \
             not a command. Add or update the `fidget` entry under `mcpServers`, \
             then run `cursor-agent mcp enable fidget` in a terminal and start a \
             new session. `cursor-agent mcp list` reports."
                .to_string(),
            String::new(),
        ),
        "pi" => (
            format!(
                "{{\n\
                 \x20 \"mcpServers\": {{\n\
                 \x20   \"fidget\": {{\n\
                 \x20     \"url\": \"{url}\",\n\
                 \x20     \"lifecycle\": \"eager\",\n\
                 \x20     \"headers\": {{\n\
                 \x20       \"Authorization\": \"Bearer {token}\"\n\
                 \x20     }}\n\
                 \x20   }}\n\
                 \x20 }}\n\
                 }}"
            ),
            "A fragment for `.mcp.json` (project) or `~/.pi/agent/mcp.json`, not a \
             command. Add or update the `fidget` entry under `mcpServers`. Then run \
             `/reload` followed by `/mcp reconnect fidget` in your Pi session."
                .to_string(),
            String::new(),
        ),
        "grok" => (
            format!(
                "grok mcp add fidget \"{url}\" --transport http \
                 --header \"Authorization: Bearer {token}\""
            ),
            "Run it in a terminal; `grok mcp add` overwrites in place, so re-running it \
             after a relaunch is enough. It defaults to user scope (`~/.grok/config.toml`). \
             Add `--scope project` only if you want project-local config (careful: shareable \
             config should not commit the token). Then in your live Grok session, run `/mcps` \
             and press `r` to reload. `grok mcp doctor fidget` reports."
                .to_string(),
            String::new(), // Token already in snippet
        ),
        "hermes" => (
            format!("hermes mcp add fidget --url '{url}' --auth header"),
            format!(
                "Run it in a terminal, then paste the raw token (no `Bearer` prefix) at the \
                 interactive prompt. This stores `MCP_FIDGET_API_KEY` in `~/.hermes/.env` \
                 and adds the header `Bearer ${{MCP_FIDGET_API_KEY}}`. Then run `/reload-mcp` \
                 in your Hermes session. Alternatively, add or update `fidget:` under \
                 `mcp_servers:` in `~/.hermes/config.yaml` with `url: \"{url}\"` and `headers:` \
                 → `Authorization: \"Bearer <token>\"`; keep the indentation exactly. \
                 `hermes mcp test fidget` reports."
            ),
            token.to_string(), // Show token separately for interactive paste
        ),
        // Restart, not `/reload`: OpenCode reads its configuration once at
        // start and caches it. A server written into the config of a running
        // session never appears — `GET /mcp` and `GET /config` both still
        // report the old set. `/reload` was in these steps because
        // `sst/opencode#6719` was read as a shipped command; it is an open
        // feature request. Telling the user to run it leaves them talking to a
        // session that still holds the previous run's port and token, with
        // nothing to show that anything failed.
        "opencode" => (
            format!(
                "opencode mcp add fidget --url '{url}' --header \"Authorization=Bearer {token}\""
            ),
            "Run it in a terminal, then restart your OpenCode session. OpenCode does not \
             re-read MCP mid-session. If OpenCode tries OAuth, set `\"oauth\": false` in \
             the config. Alternatively, merge this flat `mcp.fidget` content into \
             `opencode.jsonc` (not nested `mcp.servers`): `{{ \"mcp\": {{ \"fidget\": \
             {{ \"type\": \"remote\", \"url\": \"{url}\", \"oauth\": false, \"headers\": \
             {{ \"Authorization\": \"Bearer {token}\" }} }} }} }}`. `opencode mcp list` \
             reports."
                .to_string(),
            String::new(), // Token already in snippet
        ),
        _ => (
            format!("URL:   {url}\nToken: {token}"),
            format!(
                "No generated snippet for {harness:?}. Point the Harness at that URL over \
                 Streamable HTTP and have it send `Authorization: Bearer <token>`."
            ),
            String::new(),
        ),
    }
}

/// The Advanced box's two rows for the Harness the user picked, as
/// (snippet, instructions).
///
/// Empty when the loopback server did not bind this run, which is the only
/// way there is nothing to show: `main` serves it before anything can open
/// this window and regardless of whether a Harness is attached, so a BYO user
/// with none has an endpoint like everyone else.
/// The registration box's Harness, resting on the first name the picker
/// offers so the box is never blank on a first visit.
pub fn byo_harness_in_force(settings: &Settings) -> String {
    match settings.byo_harness.trim() {
        "" => form::HARNESS_PRESETS[0].to_string(),
        picked => picked.to_string(),
    }
}

pub fn byo_rows(harness: &str) -> (String, String, String) {
    match crate::mcp_http::endpoint() {
        Some(endpoint) => {
            let (url, token) = endpoint.registration();
            byo_registration(harness, &url, &token)
        }
        None => (
            String::new(),
            "The loopback MCP server did not start this run, so there is nothing to \
             register. Restarting Fidget is the fix; stderr says why it failed."
                .to_string(),
            String::new(),
        ),
    }
}

impl SettingsView {
    pub fn from_parts(
        settings: &Settings,
        memory_path: &Path,
        last_payload: Option<String>,
        installed: Vec<String>,
        instances: Vec<InstanceRow>,
        api_key: (bool, String, String),
        harness: Option<crate::harness::HarnessInspect>,
    ) -> Self {
        let (api_key_set, api_key_fingerprint, api_key_error) = api_key;
        let (byo_snippet, byo_steps, byo_token) = byo_rows(&byo_harness_in_force(settings));
        Self {
            // The value in force, as the Development rows show theirs: an
            // exported switch reads as it exported, however the file has it.
            director_enabled: model::director_in_force(settings.director_enabled),
            proactive_wakes: settings.proactive_wakes,
            do_not_disturb: settings.do_not_disturb,
            sound: settings.sound,
            hidden: settings.hidden,
            hide_in_fullscreen: settings.hide_in_fullscreen,
            launch_at_login: settings.launch_at_login,
            hide_hotkey: display_hotkey(&settings.hide_hotkey),
            excluded_applications: settings.excluded_applications.clone(),
            character: settings.character.clone(),
            memory_path: memory_path.display().to_string(),
            last_payload,
            installed,
            instances,
            // The resolved endpoint, not the file's: an exported variable
            // outranks the file in `model::resolve`, and a window that printed
            // the file value would name a host the Director never calls (#272).
            director_base_url: model::env_or_file(model::BASE_URL, &settings.director_base_url),
            director_model: model::env_or_file(model::MODEL, &settings.director_model),
            api_key_set,
            api_key_fingerprint,
            api_key_error,
            harness: harness_in_force(settings).0,
            harness_state: harness_state(harness.as_ref()),
            byo_harness: byo_harness_in_force(settings),
            byo_snippet,
            byo_steps,
            byo_token,
            development_switches: development_switches(settings),
            development_texts: development_texts(settings),
            consent: consent::rows(|id| settings.wants_consent(id)),
            consent_listed_as: String::new(),
            chat_ui: settings.chat_ui.clone(),
            chat_appearance: settings.chat_appearance,
        }
    }

    /// Every value the page draws, by the form row id it asks for.
    ///
    /// The page indexes `values` by row id and the fields above are named for
    /// the file they came from. This is the one place the two vocabularies
    /// are reconciled, and `form.rs`'s fixture test pins the key set against
    /// `describe()`, so a row added without a value fails a test rather than
    /// drawing blank. #875.
    ///
    pub fn row_values(&self) -> BTreeMap<String, RowValue> {
        let text = |value: &str| RowValue::Text(value.to_string());
        let popup = |id: &str| RowValue::Text(self.popup_value(id).unwrap_or_default());
        let mut values = BTreeMap::from([
            (
                form::DND_ID.to_string(),
                RowValue::Bool(self.do_not_disturb),
            ),
            (form::SOUND_ID.to_string(), RowValue::Bool(self.sound)),
            (form::HIDDEN_ID.to_string(), RowValue::Bool(self.hidden)),
            (
                form::FULLSCREEN_ID.to_string(),
                RowValue::Bool(self.hide_in_fullscreen),
            ),
            (
                form::LAUNCH_ID.to_string(),
                RowValue::Bool(self.launch_at_login),
            ),
            (form::HOTKEY_ID.to_string(), text(&self.hide_hotkey)),
            (form::CHARACTER_ID.to_string(), text(&self.character)),
            (
                form::INSTANCES_ID.to_string(),
                RowValue::Instances(self.instances.clone()),
            ),
            (form::NEW_NAME_ID.to_string(), text("")),
            // New starts on the Character in force (#875).
            (form::NEW_CHARACTER_ID.to_string(), text(&self.character)),
            (
                form::DIRECTOR_ID.to_string(),
                RowValue::Bool(self.director_enabled),
            ),
            (
                form::PROACTIVE_ID.to_string(),
                RowValue::Bool(self.proactive_wakes),
            ),
            (form::HARNESS_ID.to_string(), text(&self.harness)),
            (
                form::HARNESS_STATE_ID.to_string(),
                text(&self.harness_state),
            ),
            (form::BYO_HARNESS_ID.to_string(), text(&self.byo_harness)),
            (form::BYO_SNIPPET_ID.to_string(), text(&self.byo_snippet)),
            (form::BYO_TOKEN_ID.to_string(), text(&self.byo_token)),
            (form::BYO_STEPS_ID.to_string(), text(&self.byo_steps)),
            (
                form::DIRECTOR_BASE_URL_PICK_ID.to_string(),
                popup(form::DIRECTOR_BASE_URL_PICK_ID),
            ),
            (
                form::DIRECTOR_BASE_URL_ID.to_string(),
                text(&self.director_base_url),
            ),
            (
                form::DIRECTOR_MODEL_ID.to_string(),
                text(&self.director_model),
            ),
            (
                form::HARNESS_MODEL_ID.to_string(),
                text(&self.director_model),
            ),
            (
                form::PAYLOAD_ID.to_string(),
                text(self.last_payload.as_deref().unwrap_or_default()),
            ),
            (
                form::DIRECTOR_REASONING_EFFORT_PICK_ID.to_string(),
                popup(form::DIRECTOR_REASONING_EFFORT_PICK_ID),
            ),
            (
                form::EXCLUDED_ID.to_string(),
                RowValue::Text(self.excluded_text()),
            ),
            (form::MEMORY_PATH_ID.to_string(), text(&self.memory_path)),
        ]);
        // These three are keyed by row id already, which is how one window
        // filled a tab's worth of rows from a single map (#273).
        values.extend(
            self.development_switches
                .iter()
                .map(|(id, on)| (id.clone(), RowValue::Bool(*on))),
        );
        values.extend(
            self.development_texts
                .iter()
                .map(|(id, value)| (id.clone(), RowValue::Text(value.clone()))),
        );
        values.extend(
            self.consent
                .iter()
                .map(|row| (row.row_id(), RowValue::Bool(row.granted))),
        );
        values.insert(
            "chat_ui".to_string(),
            text(&form::chat_ui_title(&self.chat_ui)),
        );
        values.insert(
            "chat_appearance".to_string(),
            text(self.chat_appearance.title()),
        );
        values
    }

    /// One name per line, the same text the excluded-applications field edits.
    pub fn excluded_text(&self) -> String {
        self.excluded_applications.join("\n")
    }

    /// What the key field shows when it is empty. The placeholder, not the
    /// value: a fingerprint sitting in the field would be committed as a key
    /// on the next blur.
    pub fn api_key_placeholder(&self) -> String {
        if model::env_override(model::API_KEY).is_some() {
            // The variable's key is the one `resolve` hands the Completer, so
            // the stored fingerprint would name a key nothing uses (#272).
            "Overridden by env".to_string()
        } else if !self.api_key_error.is_empty() {
            format!("Unavailable: {}", self.api_key_error)
        } else if self.api_key_set {
            format!("Set: {}", self.api_key_fingerprint)
        } else {
            "Not set".into()
        }
    }

    /// What the popup with this id shows right now, or `None` for one whose
    /// value the form does not hold.
    ///
    /// A pick already showing this value is a no-op. On the source popup a
    /// restated Custom command line would otherwise collapse to the bare word
    /// `custom` (#452).
    ///
    /// The last two are shortcut pickers, which rest on the title for what
    /// the field below them holds rather than on a value of their own (#670).
    pub fn popup_value(&self, id: &str) -> Option<String> {
        match id {
            form::CHARACTER_ID => Some(self.character.clone()),
            form::HARNESS_ID => Some(self.harness.clone()),
            form::BYO_HARNESS_ID => Some(self.byo_harness.clone()),
            form::DIRECTOR_BASE_URL_PICK_ID => Some(form::endpoint_title(&self.director_base_url)),
            form::DIRECTOR_REASONING_EFFORT_PICK_ID => Some(form::effort_title(
                self.development_texts
                    .get(form::DIRECTOR_REASONING_EFFORT_ID)
                    .map(String::as_str)
                    .unwrap_or_default(),
            )),
            "chat_appearance" => Some(self.chat_appearance.title().to_string()),
            _ => None,
        }
    }

    /// The Instance a Dismiss press names.
    ///
    /// `None` for an id the roster no longer carries: the page draws from a
    /// snapshot, and a fidget can go while that list is on screen. #875.
    pub fn instance(&self, id: &str) -> Option<&InstanceRow> {
        self.instances.iter().find(|row| row.id == id)
    }

    /// Whether Clear key has anything to clear. A key the store would not
    /// hand over counts: an unreadable one can still be wiped.
    pub fn clear_key_enabled(&self) -> bool {
        self.api_key_set || !self.api_key_error.is_empty()
    }
}

/// Work the settings window asks the frame loop to do.
#[derive(Clone, Debug)]
pub enum SettingsOp {
    Spawn {
        character: String,
        name: String,
    },
    Dismiss {
        id: String,
    },
    SwitchAll {
        character: String,
    },
    /// Completer target changed. Resolved off the frame thread so the loop
    /// never reads Keychain. Drops in-flight session history (ADR-0008).
    Retarget {
        settings: DirectorSettings,
        enabled: bool,
        proactive_allowed: bool,
        configured: bool,
    },
    /// Director switch or Completer source changed: already-open Chat
    /// surfaces must re-run `attached()` from a full opening. Not a second
    /// session (ADR-0008). #473.
    ReloadChat,
    /// Throw every live Instance's conversation away and open a fresh one.
    ///
    /// Not a Retarget: nothing about what answers moved, so the Completer is
    /// rebuilt from the settings the loop already holds (#679).
    NewSession,
    /// Chat UI selection changed: swap the root class on every chat surface.
    ChatUIChanged {
        chat_ui: String,
    },
    ChatAppearanceChanged {
        chat_appearance: ChatAppearance,
    },
}

/// Whether what a secure field holds is a key somebody typed.
///
/// Both windows leave that field blank on refresh, so an empty one is an
/// untouched one and a blur over it is not an edit. Empty reaching
/// `write_director_key` deletes the stored key, which is what Clear key is
/// for and never what tabbing past the field should mean.
fn key_was_typed(text: &str) -> bool {
    model::trim_key(text).is_some()
}

/// Write the Director API key to the secret store, never to the settings file.
///
/// Empty after the same trim env keys use is delete: a quoted blank must not
/// become a Bearer of quotes.
pub fn write_director_key(store: &dyn SecretStore, patch: &SettingsPatch) -> Result<(), String> {
    match patch.completer.director_api_key.as_deref() {
        None => Ok(()),
        Some(value) => match model::trim_key(value) {
            Some(key) => store.set(DIRECTOR_API_KEY, &key),
            None => store.delete(DIRECTOR_API_KEY),
        },
    }
}

/// Env, then the file, then the store. A store read error is `Err`, not Unset:
/// treating it as no key would drop a remote Completer to Static on Retarget.
///
/// The store is read only when this process could use what it says, because
/// the read is a Keychain dialog on macOS (#283) and one nobody can act on is
/// the whole of #290. Two answers are already elsewhere: an exported key
/// outranks the store, and an attached Harness is the Completer for every
/// Instance (ADR-0008), so the HTTP settings are never consulted. Call after
/// `harness::attach`, or a Harness launch reads a key it will never send.
pub fn director_settings(
    settings: &Settings,
    secrets: &dyn SecretStore,
) -> Result<DirectorSettings, String> {
    resolve_director(settings, secrets, crate::harness::attached().is_some())
}

/// The test seam for `director_settings`. `harness::attach` holds its Session
/// in a process-wide slot for the app's lifetime, so a test that attached
/// one would decide the answer for every other test in the binary.
fn resolve_director(
    settings: &Settings,
    secrets: &dyn SecretStore,
    harness_attached: bool,
) -> Result<DirectorSettings, String> {
    let stored = if model::env_owns_key() || harness_attached {
        None
    } else {
        secrets.get(DIRECTOR_API_KEY)?
    };
    Ok(model::resolve(
        &settings.director_base_url,
        &settings.director_model,
        stored.as_deref(),
    ))
}

/// Fold a patch into `settings` and put the live development flags back in
/// step with it.
///
/// A development switch is live state as well as a file field, so a patch that
/// only reached the file is the relaunch #273 reports. Both `apply` paths go
/// through here, so the test seam cannot pass while the seeding is gone.
fn apply_and_seed(settings: &mut Settings, patch: SettingsPatch) {
    settings.apply(patch);
    dev_flags::seed(settings);
}

/// The one settings file whose next save stalls. Every test that writes
/// settings goes through `save`, so a bare flag let a parallel test's save take
/// the stall and leave the waiting test deadlocked on the settings lock.
#[cfg(test)]
static SAVE_STALL: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(test)]
static SAVE_STALLING: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static KEY_STALL: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static KEY_STALLING: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static RETARGET_STALL: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static RETARGET_STALLING: AtomicBool = AtomicBool::new(false);

/// One writer of `settings.json`. Not the settings mutex: the frame loop takes
/// that every tick, and two flushes must not overwrite a newer edit with an
/// older snapshot.
fn settings_file() -> &'static Mutex<()> {
    static FILE: Mutex<()> = Mutex::new(());
    &FILE
}

/// Clone the live settings and write them. The caller must not hold `settings`.
pub(crate) fn flush_settings(settings: &Mutex<Settings>, path: &Path) -> io::Result<()> {
    let _file = settings_file()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let snapshot = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    snapshot.save(path)
}

/// Write an owned snapshot. Startup uses this before the settings mutex exists.
pub(crate) fn write_settings_file(settings: &Settings, path: &Path) -> io::Result<()> {
    let _file = settings_file()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    settings.save(path)
}

/// Clone under the file lock, after the settings guard is gone. The frame loop
/// takes `settings` every tick, so the write cannot sit inside that guard.
fn save_after_release(settings: &Arc<Mutex<Settings>>, path: &Path) -> Result<(), String> {
    flush_settings(settings, path).map_err(|error| error.to_string())
}

/// Keychain write, after a settings lock that is not held across the store.
/// A dialog or a slow store must not stall the frame loop.
fn write_key_off_the_settings_lock(
    settings: &Arc<Mutex<Settings>>,
    store: &dyn SecretStore,
    patch: &SettingsPatch,
) -> Result<(), String> {
    drop(settings.lock().map_err(|error| error.to_string())?);
    #[cfg(test)]
    if KEY_STALL.swap(false, Ordering::SeqCst) {
        KEY_STALLING.store(true, Ordering::SeqCst);
        thread::sleep(Duration::from_secs(2));
        KEY_STALLING.store(false, Ordering::SeqCst);
    }
    write_director_key(store, patch)
}

/// Keychain read for Retarget, same rule as the write: look, then let go,
/// then ask the store.
fn retarget_off_the_settings_lock(
    settings: &Arc<Mutex<Settings>>,
    snapshot: &Settings,
    store: &dyn SecretStore,
) -> Result<SettingsOp, String> {
    drop(settings.lock().map_err(|error| error.to_string())?);
    #[cfg(test)]
    if RETARGET_STALL.swap(false, Ordering::SeqCst) {
        RETARGET_STALLING.store(true, Ordering::SeqCst);
        thread::sleep(Duration::from_secs(2));
        RETARGET_STALLING.store(false, Ordering::SeqCst);
    }
    retarget_payload(snapshot, store)
}

/// Write the key first so a store error cannot leave a URL in memory that
/// was never saved or sent as Retarget.
///
/// The test seam for `SettingsSession::apply`, without the lock, the file and
/// the ops channel.
#[cfg(test)]
fn apply_with_store(
    settings: &mut Settings,
    store: &dyn SecretStore,
    patch: SettingsPatch,
) -> Result<(), String> {
    write_director_key(store, &patch)?;
    apply_and_seed(settings, patch);
    Ok(())
}

/// `Some` on the key always retargets: we cannot compare a secret to the
/// file. URL and model retarget only when the value actually changed — both
/// windows commit on every blur, an untouched field included.
fn completer_retargets(settings: &Settings, patch: &SettingsPatch) -> bool {
    patch.completer.director_api_key.is_some()
        || patch
            .completer
            .director_base_url
            .as_ref()
            .is_some_and(|url| url != &settings.director_base_url)
        || patch
            .completer
            .director_model
            .as_ref()
            .is_some_and(|model| model != &settings.director_model)
        // The timeout and the turn ceiling are baked into the Endpoint by
        // `model::endpoint_from`, so a change to either only reaches the
        // Director through a rebuild.
        || patch
            .completer
            .director_timeout_secs
            .as_ref()
            .is_some_and(|secs| secs != &settings.director_timeout_secs)
        || patch
            .completer
            .harness_turn_timeout_secs
            .as_ref()
            .is_some_and(|secs| secs != &settings.harness_turn_timeout_secs)
        || patch
            .completer
            .director_max_tokens
            .as_ref()
            .is_some_and(|cap| cap != &settings.director_max_tokens)
        // So is the reasoning effort, and the rebuild is also what puts
        // `Endpoint::takes_effort` back to optimistic, which is the whole of
        // that flag's reset path (#638).
        || patch
            .completer
            .director_reasoning_effort
            .as_ref()
            .is_some_and(|effort| effort != &settings.director_reasoning_effort)
        // Blank-AI mode decides the opening turn, and a session that has had
        // its opening cannot be given another one — so the toggle has to
        // rebuild the Director rather than change what the next follow-up
        // rides on. Without this the mode would lie: half a session shaped by
        // a Character, half not, and no way to tell which reply was which
        // (#657).
        || patch
            .completer
            .director_blank
            .is_some_and(|blank| blank != settings.director_blank)
        // Every Completer source change retargets since #500, Off and a
        // different Harness alike: `harness::retarget` has moved the handle by
        // the time the payload is built, and `completer_from` reads it.
        //
        // The wake interval reaches a running Director the same way: the rebuild
        // is where `model::config_from` reads it (#262).
        || patch
            .completer
            .director_wake_secs
            .as_ref()
            .is_some_and(|secs| secs != &settings.director_wake_secs)
        || harness_retargets(settings, patch)
}

/// Whether the Harness this process would spawn is not the one it has. #500.
///
/// Identity is `Target` (Launch plus resolved cwd), not Launch alone (#782).
/// The saved row alone cannot answer it: `FIDGET_HARNESS` owns the row, so
/// clearing the file under an export changes the setting and nothing else. And
/// two rows can name one Harness — the `hermes` preset and a custom
/// `hermes acp` join to the same `Launch` — which is a pick that must not kill
/// a child and open it again.
fn harness_retargets(settings: &Settings, patch: &SettingsPatch) -> bool {
    let source_changed = harness_source_changed(settings, patch);
    let cwd_raw_changed = patch
        .completer
        .harness_cwd
        .as_ref()
        .is_some_and(|cwd| cwd != &settings.harness_cwd);
    if !source_changed && !cwd_raw_changed {
        return false;
    }
    let mut next = settings.clone();
    next.apply(patch.clone());
    crate::harness::Target::from_settings(next.harness_source().as_deref(), &next.harness_cwd)
        != crate::harness::Target::from_settings(
            settings.harness_source().as_deref(),
            &settings.harness_cwd,
        )
}

/// Whether an already-open Chat surface must hear a new opening.
///
/// Director on/off never retargets. Every Completer source change does since
/// #500. All of them still change what `chat_opening` would say, and the
/// window only asked once. #473.
///
/// A Completer retarget joins them since #474: the header names the model and
/// the host, so an endpoint edit moves what an open window is drawing. The
/// predicate is `completer_retargets` whole rather than its two endpoint terms,
/// because a key or a timeout change re-pushes an opening that reads the same,
/// and one redundant event is cheaper than a second rule to keep in step.
fn chat_surface_reloads(settings: &Settings, patch: &SettingsPatch) -> bool {
    // A new session empties the transcript the surface is showing, and the
    // opening it draws over that is one the window only asked for once (#679).
    patch.new_session
        || patch
            .completer
            .director_enabled
            .is_some_and(|on| on != settings.director_enabled)
        || harness_source_changed(settings, patch)
        || completer_retargets(settings, patch)
}

fn harness_source_changed(settings: &Settings, patch: &SettingsPatch) -> bool {
    if patch.completer.harness.is_none() && patch.completer.harness_command.is_none() {
        return false;
    }
    let mut next = settings.clone();
    next.apply(patch.clone());
    next.harness_source() != settings.harness_source()
}

/// The AI tab's batched rows as the page drew them at Apply, by row id and in
/// the shapes `row_values` hands the page. A row it did not send reads as live.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct DraftRows {
    #[serde(flatten)]
    pub values: BTreeMap<String, RowValue>,
    /// Clear key draws a blank field, and a blank key field is an untouched
    /// one, so the staged delete crosses as its own flag.
    #[serde(default)]
    pub clear_key: bool,
}

/// What the page drew, read against the description it drew from.
pub struct AiDraft<'a> {
    pub rows: DraftRows,
    pub description: &'a form::FormDescription,
}

impl<'a> AiDraft<'a> {
    /// The AI tab as live state draws it: nothing staged.
    pub fn live(description: &'a form::FormDescription) -> Self {
        Self {
            rows: DraftRows::default(),
            description,
        }
    }

    /// The changed AI values Apply sends in one patch, or `None` when clean.
    ///
    /// Only a batched row applies here, and a frozen one never does:
    /// `model::resolve` gives the exported variable the last word (#272). A
    /// blank key is refused by `set_text`, so a typed key beats a staged clear.
    pub fn patch(&self, view: &SettingsView) -> Option<SettingsPatch> {
        let live = view.row_values();
        let description = self.description;
        let mut patch = SettingsPatch::default();
        for (id, value) in &self.rows.values {
            if !description.batched(id) || description.frozen(id) || live.get(id) == Some(value) {
                continue;
            }
            match value {
                RowValue::Bool(on) => {
                    if let Some(field) = description.bool_write(id) {
                        patch.set_bool(field, *on);
                    }
                }
                RowValue::Text(text) => {
                    if let Some(field) = description.text_write(id) {
                        patch.set_text(field, text);
                    }
                }
                RowValue::Instances(_) => {}
            }
        }
        // Nothing stored is nothing to delete, so that clear is not an edit.
        if self.rows.clear_key
            && patch.completer.director_api_key.is_none()
            && !description.frozen(form::DIRECTOR_API_KEY_ID)
            && view.clear_key_enabled()
        {
            patch.completer.director_api_key = Some(String::new());
        }
        (patch != SettingsPatch::default()).then_some(patch)
    }
}

#[cfg(test)]
impl<'a> AiDraft<'a> {
    /// The draft for `rows`, parsed the way Apply's payload is.
    pub fn drawn(description: &'a form::FormDescription, rows: serde_json::Value) -> Self {
        Self {
            rows: serde_json::from_value(rows).expect("the page's draft"),
            description,
        }
    }
}

/// Fingerprint plus an error string: a get `Err` is not Unset. Log like the
/// other store call sites so a locked Keychain is visible.
fn stored_key_status(store: &dyn SecretStore) -> (bool, String, String) {
    match store.get(DIRECTOR_API_KEY) {
        Ok(Some(key)) => (true, model::key_fingerprint(&key), String::new()),
        Ok(None) => (false, String::new(), String::new()),
        Err(why) => {
            eprintln!("director: secret store: {why}");
            (false, String::new(), why)
        }
    }
}

/// Resolve Director settings on the settings thread. The frame loop only applies them.
fn retarget_payload(settings: &Settings, store: &dyn SecretStore) -> Result<SettingsOp, String> {
    let director = director_settings(settings, store)?;
    let mut cfg = model::config_from(&director);
    cfg.apply_switch(settings.director_enabled);
    Ok(SettingsOp::Retarget {
        settings: director,
        enabled: cfg.enabled,
        proactive_allowed: settings.proactive_wakes,
        configured: cfg.configured,
    })
}

fn chat_appearance_op(before: ChatAppearance, patch: &SettingsPatch) -> Option<SettingsOp> {
    patch
        .chat_appearance
        .filter(|&next| next != before)
        .map(|chat_appearance| SettingsOp::ChatAppearanceChanged { chat_appearance })
}

/// Everything Settings needs to read and write.
pub struct SettingsSession {
    pub settings: Arc<Mutex<Settings>>,
    pub path: PathBuf,
    pub memory_path: PathBuf,
    pub rules: Arc<Mutex<HideRules>>,
    pub inspect: Arc<Mutex<DirectorInspect>>,
    pub instances: Arc<Mutex<Vec<InstanceRow>>>,
    pub installed: Vec<String>,
    pub ops: mpsc::Sender<SettingsOp>,
    pub app: AppHandle,
    pub on_rebind: fn(&AppHandle, &str),
    pub secrets: Arc<dyn SecretStore>,
    /// Last successful store fingerprint. Become-key must not hit Keychain
    /// every focus; a failed read is not cached, so unlocking can recover.
    pub key_cache: Mutex<Option<(bool, String)>>,
}

impl SettingsSession {
    pub fn view(&self) -> SettingsView {
        let settings = self
            .settings
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
        let instances = self
            .instances
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
        // The attachment is re-read here rather than taken as `inspect` last
        // recorded it, the way `chat_opening` does: a login the user ran
        // mid-session moves the Harness out of the not-authenticated state,
        // and nothing pushes that (#436).
        let (last_payload, harness) = match self.inspect.lock() {
            Ok(mut inspect) => {
                inspect.harness = crate::harness::attached().map(|session| session.inspect());
                (inspect.last_payload.clone(), inspect.harness.clone())
            }
            Err(_) => (None, None),
        };
        let mut view = SettingsView::from_parts(
            &settings,
            &self.memory_path,
            last_payload,
            self.installed.clone(),
            instances,
            self.key_status_for_view(),
            harness,
        );
        view.consent_listed_as = consent::process_listed_as();
        view
    }

    /// Flip on: persist intent, then the system prompt if the OS has not
    /// granted yet. Flip off: persist intent and stop using the grant; the
    /// OS grant stays until Privacy & Security revokes it.
    pub fn enable_consent(&self, id: CapabilityId) {
        consent::enable(id, consent::live());
    }

    pub fn apply(&self, patch: SettingsPatch) -> Result<(), String> {
        let switching = patch.roster.character.clone();
        let rebind = patch.presence.hide_hotkey.clone();
        let new_session = patch.new_session;
        write_key_off_the_settings_lock(&self.settings, self.secrets.as_ref(), &patch)?;
        if let Some(raw) = patch.completer.director_api_key.as_deref() {
            self.remember_written_key(raw);
        }
        #[cfg(not(target_os = "linux"))]
        let prompt_ax = patch.use_accessibility == Some(true);
        let prompt_wt = patch.use_window_names == Some(true);
        #[cfg(target_os = "macos")]
        let prompt_im = patch.use_input_monitoring == Some(true);
        let next = {
            let settings = self.settings.lock().map_err(|error| error.to_string())?;
            let mut next = settings.clone();
            next.apply(patch.clone());
            next
        };
        // Pi's project file is disk, and the frame loop takes `settings` every tick.
        sync_pi_project_mcp(&next)?;
        let mut settings = self.settings.lock().map_err(|error| error.to_string())?;
        let retarget = completer_retargets(&settings, &patch);
        let move_harness = harness_retargets(&settings, &patch);
        let reload_chat = chat_surface_reloads(&settings, &patch);
        let chat_ui_changed = patch
            .chat_ui
            .as_ref()
            .is_some_and(|ui| *ui != settings.chat_ui);
        let new_chat_ui = patch.chat_ui.clone();
        let appearance_op = chat_appearance_op(settings.chat_appearance, &patch);
        // Seeded before `retarget_payload`, which rebuilds the Endpoint from
        // the live timeout and turn ceiling.
        apply_and_seed(&mut settings, patch);
        #[cfg(not(target_os = "linux"))]
        consent::set_wanted(CapabilityId::Accessibility, settings.use_accessibility);
        consent::set_wanted(CapabilityId::WindowNames, settings.use_window_names);
        if let Ok(mut rules) = self.rules.lock() {
            rules.set_away(settings.hidden);
            rules.set_hide_in_fullscreen(settings.hide_in_fullscreen);
        }
        let snapshot = settings.clone();
        drop(settings);
        // The frame loop takes `settings` every tick. The file write stays
        // off that lock; `flush_settings` is what serializes the writers.
        save_after_release(&self.settings, &self.path)?;

        if let Some(name) = switching {
            let _ = self.ops.send(SettingsOp::SwitchAll { character: name });
        }
        // Before Retarget: `director_settings` skips the keychain while a
        // handle exists, and `completer_from` prefers that handle.
        let mut dropped_harness = false;
        if move_harness {
            crate::harness::retarget(
                crate::harness::Target::from_settings(
                    snapshot.harness_source().as_deref(),
                    &snapshot.harness_cwd,
                ),
                model::director_in_force(snapshot.director_enabled),
            );
            dropped_harness = crate::harness::attached().is_none();
        }
        if retarget {
            match retarget_off_the_settings_lock(&self.settings, &snapshot, self.secrets.as_ref()) {
                Ok(op) => {
                    let _ = self.ops.send(op);
                }
                Err(why) => {
                    eprintln!("director: secret store: {why}");
                    if dropped_harness {
                        let director = model::resolve(
                            &snapshot.director_base_url,
                            &snapshot.director_model,
                            None,
                        );
                        let mut cfg = model::config_from(&director);
                        cfg.apply_switch(snapshot.director_enabled);
                        let _ = self.ops.send(SettingsOp::Retarget {
                            settings: director,
                            enabled: cfg.enabled,
                            proactive_allowed: snapshot.proactive_wakes,
                            configured: cfg.configured,
                        });
                    }
                }
            }
        }
        // After Retarget, so a patch that did both leaves the new Completer in
        // place before the conversation on it is opened.
        if new_session {
            let _ = self.ops.send(SettingsOp::NewSession);
        }
        if reload_chat {
            let _ = self.ops.send(SettingsOp::ReloadChat);
        }
        if chat_ui_changed {
            if let Some(ui) = new_chat_ui {
                let _ = self.ops.send(SettingsOp::ChatUIChanged { chat_ui: ui });
            }
        }
        if let Some(op) = appearance_op {
            let _ = self.ops.send(op);
        }
        if let Some(spec) = rebind {
            (self.on_rebind)(&self.app, &spec);
        }
        #[cfg(not(target_os = "linux"))]
        if prompt_ax {
            self.enable_consent(CapabilityId::Accessibility);
        }
        if prompt_wt {
            self.enable_consent(CapabilityId::WindowNames);
        }
        #[cfg(target_os = "macos")]
        if prompt_im {
            self.enable_consent(CapabilityId::InputMonitoring);
        }
        Ok(())
    }

    fn remember_written_key(&self, raw: &str) {
        let cached = match model::trim_key(raw) {
            Some(key) => Some((true, model::key_fingerprint(&key))),
            None => Some((false, String::new())),
        };
        if let Ok(mut cache) = self.key_cache.lock() {
            *cache = cached;
        }
    }

    fn key_status_for_view(&self) -> (bool, String, String) {
        if let Ok(cache) = self.key_cache.lock() {
            if let Some((set, fingerprint)) = cache.as_ref() {
                return (*set, fingerprint.clone(), String::new());
            }
        }
        let (set, fingerprint, error) = stored_key_status(self.secrets.as_ref());
        if error.is_empty() {
            if let Ok(mut cache) = self.key_cache.lock() {
                *cache = Some((set, fingerprint.clone()));
            }
        }
        (set, fingerprint, error)
    }

    /// Hand the Memory file to the desktop's opener, in Rust rather than in
    /// the page: #706 keeps the file system out of JavaScript.
    pub fn open_memory(&self) -> Result<(), String> {
        crate::platform::open_path(&self.memory_path)
    }

    pub fn wipe_memory(&self) -> Result<(), String> {
        MemoryManifest::new(&self.memory_path)
            .wipe()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// The loop pushes `settings-refresh` on the tick that runs the op, so
    /// the window redraws from the roster rather than from a guess.
    pub fn spawn(&self, character: String, name: String) {
        let _ = self.ops.send(SettingsOp::Spawn { character, name });
    }

    pub fn dismiss(&self, id: String) {
        let _ = self.ops.send(SettingsOp::Dismiss { id });
    }
}

/// One Apply. A field left `None` is not a change, in a group or beside one.
/// Spawn and Dismiss stay `SettingsOp`s.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SettingsPatch {
    pub roster: RosterPatch,
    pub completer: CompleterPatch,
    pub presence: PresencePatch,
    pub do_not_disturb: Option<bool>,
    pub sound: Option<bool>,
    pub launch_at_login: Option<bool>,
    pub trace_frames: Option<bool>,
    pub trace_hittest: Option<bool>,
    pub trace_director: Option<bool>,
    pub trace_engine: Option<bool>,
    pub trace_bubble: Option<bool>,
    pub capturable: Option<bool>,
    #[serde(default)]
    pub use_accessibility: Option<bool>,
    #[serde(default)]
    pub use_window_names: Option<bool>,
    pub use_input_monitoring: Option<bool>,
    /// Throw the conversation in flight away and open a fresh one on the same
    /// Completer. Not a file field either: a session boundary is a moment, not
    /// a setting, and nothing about it survives the restart (#679).
    #[serde(default)]
    pub new_session: bool,
    pub chat_ui: Option<String>,
    pub chat_appearance: Option<ChatAppearance>,
}

/// The Character every Instance switches to.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct RosterPatch {
    pub character: Option<String>,
}

/// HTTP knobs, Harness launch, and the proactive switch, in one Apply.
#[derive(Clone, Default, Deserialize, PartialEq)]
pub struct CompleterPatch {
    pub director_enabled: Option<bool>,
    pub proactive_wakes: Option<bool>,
    pub director_base_url: Option<String>,
    pub director_model: Option<String>,
    pub director_timeout_secs: Option<String>,
    pub director_max_tokens: Option<String>,
    pub director_reasoning_effort: Option<String>,
    pub director_wake_secs: Option<String>,
    /// Present so callers can write the store; `Settings::apply` ignores it
    /// because the key is not a file field.
    pub director_api_key: Option<String>,
    pub director_blank: Option<bool>,
    pub harness: Option<String>,
    pub harness_command: Option<String>,
    pub harness_auth_retry_secs: Option<String>,
    pub harness_turn_timeout_secs: Option<String>,
    pub harness_cwd: Option<String>,
    pub mcp_bin: Option<String>,
    pub byo_harness: Option<String>,
    #[serde(default)]
    pub pi_project_mcp: Option<bool>,
}

/// Whether the sprite is shown, and which applications hide it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct PresencePatch {
    pub hidden: Option<bool>,
    pub hide_in_fullscreen: Option<bool>,
    pub hide_hotkey: Option<String>,
    pub excluded_applications: Option<Vec<String>>,
}

/// A boolean field of `SettingsPatch`, as the form row writing it names it.
///
/// A name rather than a `&str` so the row and the setter cannot disagree: with
/// a string key, a row could name a field no setter knew, and that compiled
/// clean and shipped a checkbox that wrote nothing (#273).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum BoolField {
    DirectorEnabled,
    ProactiveWakes,
    DoNotDisturb,
    Sound,
    Hidden,
    HideInFullscreen,
    LaunchAtLogin,
    TraceFrames,
    TraceHittest,
    TraceDirector,
    TraceEngine,
    DirectorBlank,
    /// Apply writes the project `.mcp.json` when Pi is the Harness. Default on.
    PiProjectMcp,
    /// macOS and Windows support capture exclusion via platform APIs. Linux
    /// has no exclusion API (ADR-0024) but the setting and UI row are present
    /// for consistency. The patch field itself is not gated: the file carries
    /// it anywhere.
    Capturable,
    // The consent rows. All three platforms offer WindowNames; Accessibility
    // is macOS and Windows. The patch fields are not gated; the file carries them. #250.
    #[cfg(not(target_os = "linux"))]
    UseAccessibility,
    UseWindowNames,
    /// The idle event tap, which macOS alone has a grant to ask for (#721).
    #[cfg(target_os = "macos")]
    UseInputMonitoring,
}

/// A text field of `SettingsPatch`, as the form row writing it names it.
///
/// Typed for the reason `BoolField` is. No `hide_hotkey`: the row showing it is
/// an `InspectBlock` that writes nothing, because a text field is not a key
/// recorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TextField {
    Character,
    DirectorBaseUrl,
    DirectorModel,
    DirectorTimeoutSecs,
    DirectorMaxTokens,
    DirectorReasoningEffort,
    DirectorWakeSecs,
    DirectorApiKey,
    /// The Completer source popup. Written as a title and stored as the value
    /// `FIDGET_HARNESS` would take; `form::harness_choice` is the one place
    /// that translates.
    Harness,
    HarnessCommand,
    HarnessAuthRetrySecs,
    HarnessTurnTimeoutSecs,
    /// Which Harness the registration box is written for. A view preference
    /// and nothing else: no launch reads it (#577).
    ByoHarness,
    McpBin,
    HarnessCwd,
    ExcludedApplications,
    ChatUI,
    ChatAppearance,
}

impl SettingsPatch {
    /// Write the boolean field a checkbox declares.
    ///
    /// The one place that turns a row's field into a patch, and the one place
    /// the boolean field list is spelled: a `BoolField` this match does not
    /// cover is a compile error.
    pub fn set_bool(&mut self, field: BoolField, value: bool) {
        match field {
            BoolField::DirectorEnabled => self.completer.director_enabled = Some(value),
            BoolField::ProactiveWakes => self.completer.proactive_wakes = Some(value),
            BoolField::DoNotDisturb => self.do_not_disturb = Some(value),
            BoolField::Sound => self.sound = Some(value),
            BoolField::Hidden => self.presence.hidden = Some(value),
            BoolField::HideInFullscreen => self.presence.hide_in_fullscreen = Some(value),
            BoolField::LaunchAtLogin => self.launch_at_login = Some(value),
            BoolField::TraceFrames => self.trace_frames = Some(value),
            BoolField::TraceHittest => self.trace_hittest = Some(value),
            BoolField::TraceDirector => self.trace_director = Some(value),
            BoolField::TraceEngine => self.trace_engine = Some(value),
            BoolField::DirectorBlank => self.completer.director_blank = Some(value),
            BoolField::PiProjectMcp => self.completer.pi_project_mcp = Some(value),
            BoolField::Capturable => self.capturable = Some(value),
            #[cfg(not(target_os = "linux"))]
            BoolField::UseAccessibility => self.use_accessibility = Some(value),
            BoolField::UseWindowNames => self.use_window_names = Some(value),
            #[cfg(target_os = "macos")]
            BoolField::UseInputMonitoring => self.use_input_monitoring = Some(value),
        }
    }

    /// Write the text field a row declares, and say whether it took the value.
    ///
    /// False only for a blank API key: both windows leave that field blank on
    /// refresh, so a blur over an untouched one is not an edit, and blank
    /// reaching the store is what Clear key means. `key_was_typed` is the whole
    /// of that test.
    pub fn set_text(&mut self, field: TextField, value: &str) -> bool {
        match field {
            TextField::Character => self.roster.character = Some(value.to_string()),
            TextField::DirectorBaseUrl => {
                self.completer.director_base_url = Some(value.to_string())
            }
            TextField::DirectorModel => self.completer.director_model = Some(value.to_string()),
            TextField::DirectorTimeoutSecs => {
                self.completer.director_timeout_secs = Some(value.to_string())
            }
            TextField::DirectorMaxTokens => {
                self.completer.director_max_tokens = Some(value.to_string())
            }
            // Trimmed, not validated: a stray space around `high` is a typo,
            // but `high` itself is only the user's host's business (#638).
            TextField::DirectorReasoningEffort => {
                self.completer.director_reasoning_effort = Some(value.trim().to_string())
            }
            TextField::DirectorWakeSecs => {
                self.completer.director_wake_secs = Some(value.to_string())
            }
            // The popup hands over its title; the file keeps the value
            // `harness::launch` reads, so Off is blank and Custom is `custom`.
            TextField::Harness => self.completer.harness = Some(form::harness_choice(value)),
            TextField::HarnessCommand => {
                self.completer.harness_command = Some(value.trim().to_string())
            }
            TextField::HarnessAuthRetrySecs => {
                self.completer.harness_auth_retry_secs = Some(value.to_string())
            }
            TextField::HarnessTurnTimeoutSecs => {
                self.completer.harness_turn_timeout_secs = Some(value.to_string())
            }
            // Trimmed like the command line beside it: a path pasted out of a
            // terminal carries the space that follows it.
            TextField::ByoHarness => self.completer.byo_harness = Some(value.trim().to_string()),
            TextField::McpBin => self.completer.mcp_bin = Some(value.trim().to_string()),
            TextField::HarnessCwd => self.completer.harness_cwd = Some(value.trim().to_string()),
            TextField::DirectorApiKey if key_was_typed(value) => {
                self.completer.director_api_key = Some(value.to_string())
            }
            TextField::DirectorApiKey => return false,
            // One name per line, the shape both windows' multiline field holds.
            TextField::ExcludedApplications => {
                self.presence.excluded_applications =
                    Some(value.lines().map(|line| line.trim().to_string()).collect())
            }
            TextField::ChatUI => self.chat_ui = Some(form::chat_ui_choice(value)),
            TextField::ChatAppearance => {
                self.chat_appearance = Some(ChatAppearance::from_title(value))
            }
        }
        true
    }
}

/// A custom command line as a log may carry it: the program, and how many
/// arguments followed.
///
/// The key beside it is fingerprinted. This one is not. A Harness credential
/// is not logged, printed, or fingerprinted, and a flag on a command line is
/// somewhere a token can sit. The program name is a binary, so it is the one
/// word that cannot be one.
fn command_line_debug(line: &str) -> String {
    let mut words = line.split_whitespace();
    match words.next() {
        None => String::new(),
        Some(program) => format!("{program} +{} arg(s)", words.count()),
    }
}

impl fmt::Debug for CompleterPatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompleterPatch")
            .field("director_enabled", &self.director_enabled)
            .field("proactive_wakes", &self.proactive_wakes)
            .field("director_base_url", &self.director_base_url)
            .field("director_model", &self.director_model)
            .field("director_timeout_secs", &self.director_timeout_secs)
            .field("director_max_tokens", &self.director_max_tokens)
            .field("director_reasoning_effort", &self.director_reasoning_effort)
            .field("director_wake_secs", &self.director_wake_secs)
            .field(
                "director_api_key",
                &self.director_api_key.as_deref().map(model::key_fingerprint),
            )
            .field("director_blank", &self.director_blank)
            .field("harness", &self.harness)
            .field(
                "harness_command",
                &self.harness_command.as_deref().map(command_line_debug),
            )
            .field("harness_auth_retry_secs", &self.harness_auth_retry_secs)
            .field("harness_turn_timeout_secs", &self.harness_turn_timeout_secs)
            .field("harness_cwd", &self.harness_cwd)
            .field("mcp_bin", &self.mcp_bin)
            .field("byo_harness", &self.byo_harness)
            .field("pi_project_mcp", &self.pi_project_mcp)
            .finish()
    }
}

impl Settings {
    /// Whether a frame may make a sound. Do Not Disturb is quiet but not
    /// gone (#84), so it takes the audio cue and leaves the visual one; the
    /// webview is told the answer and never works it out itself (#277).
    pub fn sound_allowed(&self) -> bool {
        self.sound && !self.do_not_disturb
    }

    pub fn apply(&mut self, patch: SettingsPatch) {
        if let Some(value) = patch.completer.director_enabled {
            self.director_enabled = value;
        }
        if let Some(value) = patch.completer.proactive_wakes {
            self.proactive_wakes = value;
        }
        if let Some(value) = patch.do_not_disturb {
            if value != self.do_not_disturb && crate::dev_flags::TRACE_BUBBLE.is_on() {
                eprintln!("settings: dnd={}", if value { "on" } else { "off" });
            }
            self.do_not_disturb = value;
        }
        if let Some(value) = patch.sound {
            self.sound = value;
        }
        if let Some(value) = patch.presence.hidden {
            self.hidden = value;
        }
        if let Some(value) = patch.presence.hide_in_fullscreen {
            self.hide_in_fullscreen = value;
        }
        if let Some(value) = patch.presence.hide_hotkey {
            self.hide_hotkey = value;
        }
        if let Some(value) = patch.launch_at_login {
            self.launch_at_login = value;
        }
        if let Some(value) = patch.presence.excluded_applications {
            self.excluded_applications = value;
        }
        if let Some(value) = patch.roster.character {
            self.character = value;
        }
        if let Some(value) = patch.completer.director_base_url {
            self.director_base_url = value;
        }
        if let Some(value) = patch.completer.director_model {
            self.director_model = value;
        }
        if let Some(value) = patch.completer.director_timeout_secs {
            self.director_timeout_secs = value;
        }
        if let Some(value) = patch.completer.director_max_tokens {
            self.director_max_tokens = value;
        }
        if let Some(value) = patch.completer.director_reasoning_effort {
            self.director_reasoning_effort = value;
        }
        if let Some(value) = patch.completer.director_wake_secs {
            self.director_wake_secs = value;
        }
        if let Some(value) = patch.completer.harness {
            // A command line the source field carries — hand-edited there, or
            // read there under the variable's own grammar — is drawn in the
            // command-line row, which writes the *other* field. Move it before
            // the pick lands, so choosing the Custom entry already on screen
            // is not a silent Off (#452).
            if let Some(line) = form::harness_command_line(&self.harness) {
                if self.harness_command.trim().is_empty() {
                    self.harness_command = line.to_string();
                }
            }
            self.harness = value;
        }
        if let Some(value) = patch.completer.harness_command {
            self.harness_command = value;
        }
        if let Some(value) = patch.completer.harness_auth_retry_secs {
            self.harness_auth_retry_secs = value;
        }
        if let Some(value) = patch.completer.harness_turn_timeout_secs {
            self.harness_turn_timeout_secs = value;
        }
        if let Some(value) = patch.completer.byo_harness {
            self.byo_harness = value;
        }
        if let Some(value) = patch.completer.mcp_bin {
            self.mcp_bin = value;
        }
        if let Some(value) = patch.completer.harness_cwd {
            self.harness_cwd = value;
        }
        if let Some(value) = patch.trace_frames {
            self.trace_frames = value;
        }
        if let Some(value) = patch.trace_hittest {
            self.trace_hittest = value;
        }
        if let Some(value) = patch.trace_director {
            self.trace_director = value;
        }
        if let Some(value) = patch.trace_engine {
            self.trace_engine = value;
        }
        if let Some(value) = patch.trace_bubble {
            self.trace_bubble = value;
        }
        if let Some(value) = patch.completer.director_blank {
            self.director_blank = value;
        }
        if let Some(value) = patch.completer.pi_project_mcp {
            self.pi_project_mcp = value;
        }
        if let Some(value) = patch.capturable {
            self.capturable = value;
        }
        if let Some(value) = patch.use_accessibility {
            self.use_accessibility = value;
        }
        if let Some(value) = patch.use_window_names {
            self.use_window_names = value;
        }
        if let Some(value) = patch.use_input_monitoring {
            self.use_input_monitoring = value;
        }
        if let Some(value) = patch.chat_ui {
            self.chat_ui = value;
        }
        if let Some(value) = patch.chat_appearance {
            self.chat_appearance = value;
        }
        // director_api_key is intentionally ignored: the key lives in the
        // secret store, never in the JSON document.
    }

    /// What `harness::from_settings` reads out of the two source rows: the
    /// command line under `custom`, the preset name otherwise, and `None` for
    /// Off.
    ///
    /// One grammar with `FIDGET_HARNESS`, so `harness::launch` parses both.
    /// A blank command line under `custom` is Off rather than a spawn of nothing.
    pub fn harness_source(&self) -> Option<String> {
        match self.harness.trim() {
            "" => None,
            form::HARNESS_CUSTOM_VALUE => {
                Some(self.harness_command.trim().to_string()).filter(|line| !line.is_empty())
            }
            preset => Some(preset.to_string()),
        }
    }

    pub fn wants_consent(&self, id: CapabilityId) -> bool {
        match id {
            #[cfg(not(target_os = "linux"))]
            CapabilityId::Accessibility => self.use_accessibility,
            CapabilityId::WindowNames => self.use_window_names,
            #[cfg(target_os = "macos")]
            CapabilityId::InputMonitoring => self.use_input_monitoring,
        }
    }
}

/// The hide hotkey shipped until the user binds another.
///
/// Three modifiers, because a global shortcut is taken from every application
/// on the machine and B alone belongs to most of them. One canonical spelling
/// that `parse_hotkey` reads on every OS; what a user reads is
/// `display_hotkey`, in the words that OS gives the keys (#194).
pub const DEFAULT_HIDE_HOTKEY: &str = "Control-Option-Command-B";

fn default_pi_project_mcp() -> bool {
    true
}

/// The project file for an Apply that is about to attach Pi, when the hatch
/// is on. A missing endpoint or a file we cannot edit fails the Apply before
/// the Harness moves.
fn sync_pi_project_mcp(settings: &Settings) -> Result<(), String> {
    let launch = crate::harness::from_settings(settings.harness_source().as_deref());
    let pi = launch.as_ref().is_some_and(|launch| launch.name == "pi");
    if !pi || !settings.pi_project_mcp {
        return Ok(());
    }
    if crate::mcp_http::endpoint().is_none() {
        return Err(
            "Pi's project file was left alone: this launch has no MCP endpoint".to_string(),
        );
    }
    let dir = crate::harness::attach_dir(&settings.harness_cwd)?;
    crate::pi_mcp::sync_project_file(&dir)
}

/// Everything settings owns. Defaults are the v1 first-run answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Session Director on. Off leaves Static weights running the life.
    pub director_enabled: bool,
    /// Proactive session wakes. Off keeps the Director for Poke and Summon.
    /// Older files still spell the key `ambient_wakes`.
    #[serde(alias = "ambient_wakes")]
    pub proactive_wakes: bool,
    /// Quiet: on screen, not starting things. Persists so a restart stays quiet.
    pub do_not_disturb: bool,
    /// The cues a gesture plays are heard, not only seen (#277).
    pub sound: bool,
    /// Off screen, same flag the hotkey flips.
    pub hidden: bool,
    /// Fade away when a fullscreen application is frontmost.
    pub hide_in_fullscreen: bool,
    /// One canonical spec, in any platform's words. Read with `parse_hotkey`
    /// and shown to a user with `display_hotkey`, never raw (#194).
    ///
    /// A string rather than the `Hotkey` it parses into, which would otherwise
    /// be the honest type: `load` turns any parse failure into whole-file
    /// defaults, so a struct-shaped field meeting a string in an installed
    /// `settings.json` would silently reset every other setting with it.
    /// Persist the struct only behind a deserializer that accepts both shapes.
    pub hide_hotkey: String,
    pub launch_at_login: bool,
    pub excluded_applications: Vec<String>,
    /// Last chosen Character Package. Empty means the loader's default.
    pub character: String,
    /// Instances to spawn on launch. Empty means the one character first-run runs.
    pub instances: Vec<InstanceSpec>,
    /// Empty means unset — Completer resolution falls through to env then defaults.
    pub director_base_url: String,
    /// Empty means unset — Completer resolution falls through to env then defaults.
    pub director_model: String,
    /// Model API timeout, in seconds. Empty means unset, as on the two above.
    /// A Harness turn is `harness_turn_timeout_secs` (#690).
    pub director_timeout_secs: String,
    /// Turn ceiling, in tokens. Empty means unset, as on the two above.
    pub director_max_tokens: String,
    /// How hard to ask the Completer to think. Empty means unset: the Model
    /// API omits the field, and a Harness does not call `session/set_config_option`.
    pub director_reasoning_effort: String,
    /// First ambient wait, in seconds. Empty means unset, and leaves
    /// `Pace::FIRST`. The Character's `model_base` and `model_power` grow the
    /// wait from here; this is only where it starts (#262).
    pub director_wake_secs: String,
    /// Which Harness is the Completer, in the values `FIDGET_HARNESS` takes:
    /// empty for none, a preset name (`claude`, `codex`, `copilot`,
    /// `cursor-agent`, `goose`, `grok`, `hermes`, `opencode`, `pi`), or
    /// `custom`, which defers to `harness_command`. The variable outranks it,
    /// and either way `harness::retarget` reaches the attachment now (#500).
    pub harness: String,
    /// The command line `custom` runs, split on whitespace as the variable's
    /// own value is. Kept when a preset is picked, so coming back to Custom
    /// does not lose what was typed.
    pub harness_command: String,
    /// How long an unauthenticated Harness is left alone before `session/new`
    /// is tried again, in seconds. Empty means unset, and leaves the 60 in
    /// `harness::AUTH_RETRY`. Read when a Session is built, so a change lands
    /// on the next attach (#447).
    pub harness_auth_retry_secs: String,
    /// How long a Harness `session/prompt` may run, in seconds. Empty means
    /// unset, and leaves `harness::TURN_TIMEOUT`. Not the Model API field
    /// (#690).
    pub harness_turn_timeout_secs: String,
    /// Which Harness the registration box on the AI tab is written for. A
    /// view preference: the box is something a BYO user comes back to every
    /// launch, so the pick is worth keeping. Blank rests on the first name
    /// `form::HARNESS_PRESETS` lists, and nothing else reads it (#577).
    pub byo_harness: String,
    /// Where the stdio MCP server binary is. Empty means beside the app, then
    /// the app binary's own `--mcp-stdio` (#166). For power users and CI,
    /// which is why it is a Development row and not a Director one.
    pub mcp_bin: String,
    /// ACP cwd / spawn dir. Empty is the data folder. Session file and Action
    /// Log stay in the data folder (#782).
    pub harness_cwd: String,
    /// Apply writes the project `.mcp.json` when the attached Harness is Pi.
    /// On, including for a file that predates the field.
    #[serde(default = "default_pi_project_mcp")]
    pub pi_project_mcp: bool,
    /// Development switches. Off is the shipped answer for all of them; see
    /// `dev_flags`, which holds the live value each read site loads.
    pub trace_frames: bool,
    pub trace_hittest: bool,
    pub trace_director: bool,
    pub trace_engine: bool,
    pub trace_bubble: bool,
    /// Blank-AI mode: built-in prompt layers emptied, Instance Prompt kept
    /// (#657, #680).
    pub director_blank: bool,
    /// Appear in screenshots and screen shares. True (default) means the fidget
    /// is capturable; false excludes it. macOS and Windows read it; the field
    /// is unconditional so the document round-trips on every platform.
    pub capturable: bool,
    /// Use Accessibility where the OS has granted it. Off does not revoke TCC.
    pub use_accessibility: bool,
    /// Use the window-names consent where the OS has granted it: titles and
    /// owning application alike, one consent for the pair (ADR-0032). macOS
    /// uses TCC Screen Recording; Linux uses xdg-desktop-portal ScreenCast
    /// (Wayland). Off does not revoke the grant while running.
    /// Serde aliases preserve compatibility with old settings files, so a
    /// grant given under an earlier name is carried over rather than reset.
    #[serde(
        default,
        alias = "use_screen_recording",
        alias = "use_portal_screencast",
        alias = "use_window_titles"
    )]
    pub use_window_names: bool,
    /// Listen for mouse events so the frame loop can sleep while the desktop is
    /// idle (#721). macOS alone acts on it; the field is unconditional so the
    /// document round-trips on every platform.
    #[serde(default)]
    pub use_input_monitoring: bool,
    /// Whether the first-run gesture tour has been shown. Once only, persisted
    /// per-app rather than per-Instance: a second fidget spawned later sees
    /// this flag set.
    pub first_run_tour_shown: bool,
    /// The user dismissed the window-names notice. One dismissal is for good.
    /// No form row: the notice's own button writes it, and a file from before
    /// the field existed must still parse.
    #[serde(default)]
    pub names_hint_dismissed: bool,
    /// Which Chat UI design is selected: minimal, terminal, or glass.
    pub chat_ui: String,
    #[serde(default)]
    pub chat_appearance: ChatAppearance,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            director_enabled: true,
            proactive_wakes: true,
            do_not_disturb: false,
            sound: true,
            hidden: false,
            hide_in_fullscreen: true,
            hide_hotkey: DEFAULT_HIDE_HOTKEY.to_string(),
            launch_at_login: false,
            excluded_applications: Vec::new(),
            character: String::new(),
            instances: Vec::new(),
            director_base_url: String::new(),
            director_model: String::new(),
            director_timeout_secs: String::new(),
            director_max_tokens: String::new(),
            director_reasoning_effort: String::new(),
            director_wake_secs: String::new(),
            harness: String::new(),
            harness_command: String::new(),
            harness_auth_retry_secs: String::new(),
            harness_turn_timeout_secs: String::new(),
            byo_harness: String::new(),
            mcp_bin: String::new(),
            harness_cwd: String::new(),
            pi_project_mcp: true,
            trace_frames: false,
            trace_hittest: false,
            trace_director: false,
            trace_engine: false,
            trace_bubble: false,
            director_blank: false,
            capturable: true,
            use_accessibility: false,
            use_window_names: false,
            use_input_monitoring: false,
            first_run_tour_shown: false,
            names_hint_dismissed: false,
            chat_ui: "minimal".to_string(),
            chat_appearance: ChatAppearance::System,
        }
    }
}

impl Settings {
    /// Read the document at `path`. A missing file is first-run defaults.
    ///
    /// A file that cannot be parsed is also defaults rather than a refused
    /// launch: a typo in a hand-edit must not cost the fidget, the same
    /// degradation Memory already chose.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(_) => Self::default(),
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        }
    }

    /// First launch may write under an app-data dir that does not exist yet.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        #[cfg(test)]
        if SAVE_STALL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take_if(|stalled| stalled == path)
            .is_some()
        {
            SAVE_STALLING.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_secs(2));
            SAVE_STALLING.store(false, Ordering::SeqCst);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, text)
    }
}

/// Where settings lives beside Memory, so both are in one folder the user owns.
pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

/// The modifiers and key a hide-hotkey string names.
///
/// Parsed here rather than by the shortcut plugin so a bad binding is a
/// settings problem, not a plugin one, and the default can take over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub control: bool,
    pub option: bool,
    pub shift: bool,
    pub command: bool,
    pub key: char,
}

/// Read `Control-Option-Command-B` into parts. Unknown tokens refuse the
/// whole string so a typo cannot silently drop a modifier.
///
/// Each alias set is one key under every OS's name for it, `Win` included, so
/// that everything `Hotkey::display` prints is something this reads back.
pub fn parse_hotkey(spec: &str) -> Option<Hotkey> {
    let mut hotkey = Hotkey {
        control: false,
        option: false,
        shift: false,
        command: false,
        key: '\0',
    };
    let mut key = None;
    for token in spec
        .split('-')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token {
            "Control" | "Ctrl" => hotkey.control = true,
            "Option" | "Alt" => hotkey.option = true,
            "Shift" => hotkey.shift = true,
            "Command" | "Super" | "Meta" | "Win" => hotkey.command = true,
            one if one.len() == 1 => {
                let letter = one.chars().next()?.to_ascii_uppercase();
                if !letter.is_ascii_alphabetic() {
                    return None;
                }
                if key.is_some() {
                    return None;
                }
                key = Some(letter);
            }
            _ => return None,
        }
    }
    hotkey.key = key?;
    Some(hotkey)
}

/// How the keyboard in front of a user names the modifier keys.
///
/// The chord is the same three modifiers on every OS — the plugin registers
/// one binding — so this is a spelling, not a second hotkey. Taking the words
/// as an argument is what lets a Mac test assert what Linux would read (#194).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierWords {
    Mac,
    Linux,
    Windows,
}

impl ModifierWords {
    /// The words this build's OS uses. X11 and Wayland both say Super.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    /// Control, Option and Command under these words. Shift is Shift
    /// everywhere, so it is not in the table.
    fn names(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Mac => ("Control", "Option", "Command"),
            Self::Linux => ("Ctrl", "Alt", "Super"),
            Self::Windows => ("Ctrl", "Alt", "Win"),
        }
    }
}

impl Hotkey {
    /// Print the chord in `words`, e.g. `Control-Option-Command-B` on a Mac
    /// and `Ctrl-Alt-Super-B` on Linux.
    ///
    /// Every spelling it prints is one `parse_hotkey` reads back, because the
    /// settings hotkey field shows this string and takes it again on rebind.
    pub fn display(&self, words: ModifierWords) -> String {
        let (control, option, command) = words.names();
        let mut parts = Vec::with_capacity(5);
        if self.control {
            parts.push(control);
        }
        if self.option {
            parts.push(option);
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.command {
            parts.push(command);
        }
        let letter = self.key.to_ascii_uppercase().to_string();
        parts.push(&letter);
        parts.join("-")
    }
}

/// The hotkey `spec` names, in the words of the OS this build runs on.
///
/// A hand-edited or older file may name the keys in any platform's words, so
/// the stored string is parsed rather than printed. An unreadable one falls
/// back to the default, the same binding the shell registers for it.
pub fn display_hotkey(spec: &str) -> String {
    parse_hotkey(spec)
        .or_else(|| parse_hotkey(DEFAULT_HIDE_HOTKEY))
        .map(|hotkey| hotkey.display(ModifierWords::current()))
        .unwrap_or_default()
}

/// Flip Go-away and keep `Settings.hidden` on the same flag, so a restart or
/// a later patch cannot undo a hotkey hide the menu already persisted.
pub fn toggle_away(rules: &mut HideRules, settings: &mut Settings) {
    rules.toggle();
    settings.hidden = rules.is_away();
}

/// The shortcut plugin's `Code` name for a letter, e.g. `KeyH`.
///
/// Letters only: `parse_hotkey` already refuses anything else, and a name
/// the plugin cannot parse must not silently become `KeyB`.
pub fn key_code_name(key: char) -> Option<String> {
    if key.is_ascii_alphabetic() {
        Some(format!("Key{}", key.to_ascii_uppercase()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completer;
    use crate::secrets::{MemoryStore, SecretStore, DIRECTOR_API_KEY};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> PathBuf {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "fidget-settings-{n}-{:?}.json",
            std::thread::current().id()
        ))
    }

    /// A missing file is first-run, not an error: that is how every new user starts.
    #[test]
    fn a_missing_file_is_first_run_defaults() {
        let path = std::env::temp_dir().join("fidget-settings-does-not-exist.json");
        let _ = fs::remove_file(&path);

        assert_eq!(Settings::load(&path), Settings::default());
        assert!(Settings::default().director_enabled);
        assert!(Settings::default().proactive_wakes);
        assert!(Settings::default().hide_in_fullscreen);
        assert!(Settings::default().sound);
        assert!(!Settings::default().do_not_disturb);
        assert!(!Settings::default().hidden);
        assert!(!Settings::default().launch_at_login);
        assert_eq!(Settings::default().hide_hotkey, DEFAULT_HIDE_HOTKEY);
    }

    /// What the user set is what the next launch reads. That is the whole file.
    #[test]
    fn a_saved_document_round_trips() {
        let path = temp_path();
        let _ = fs::remove_file(&path);

        let settings = Settings {
            director_enabled: false,
            proactive_wakes: false,
            do_not_disturb: true,
            sound: false,
            hidden: true,
            hide_in_fullscreen: false,
            hide_hotkey: "Control-Shift-H".to_string(),
            launch_at_login: true,
            excluded_applications: vec!["1Password".to_string(), "Keychain Access".to_string()],
            character: "nim".to_string(),
            instances: vec![InstanceSpec::fresh("bmo", "Beemo")],
            director_base_url: "https://api.x.ai".into(),
            director_model: "grok-4.6".into(),
            director_timeout_secs: "45".into(),
            director_max_tokens: "300".into(),
            director_reasoning_effort: "high".into(),
            director_wake_secs: "300".into(),
            harness: "custom".into(),
            harness_command: "opencode acp".into(),
            harness_auth_retry_secs: "5".into(),
            harness_turn_timeout_secs: "90".into(),
            byo_harness: "hermes".into(),
            mcp_bin: "/opt/fidget-mcp".into(),
            harness_cwd: String::new(),
            pi_project_mcp: true,
            trace_frames: true,
            trace_hittest: true,
            trace_director: true,
            trace_engine: true,
            trace_bubble: true,
            director_blank: true,
            capturable: true,
            chat_ui: "minimal".into(),
            chat_appearance: ChatAppearance::System,
            use_accessibility: true,
            use_window_names: false,
            use_input_monitoring: true,
            first_run_tour_shown: false,
            names_hint_dismissed: false,
        };
        settings.save(&path).expect("save");

        assert_eq!(Settings::load(&path), settings);
        let _ = fs::remove_file(&path);
    }

    /// Do Not Disturb is quiet but not gone (#84): it takes the sound and
    /// leaves the visual cue, and it never turns the sound back on (#277).
    #[test]
    fn sound_is_allowed_only_when_on_and_not_disturbing() {
        let mut settings = Settings::default();
        assert!(settings.sound_allowed());
        settings.do_not_disturb = true;
        assert!(!settings.sound_allowed());
        settings.sound = false;
        assert!(!settings.sound_allowed());
        settings.do_not_disturb = false;
        assert!(!settings.sound_allowed());
    }

    /// The window toggles one field at a time, and the mute has to land
    /// without a restart, so the patch is the whole path (#277).
    #[test]
    fn a_patch_can_mute_and_unmute() {
        let mut settings = Settings::default();
        settings.apply(SettingsPatch {
            sound: Some(false),
            ..SettingsPatch::default()
        });
        assert!(!settings.sound);
        settings.apply(SettingsPatch {
            sound: Some(true),
            ..SettingsPatch::default()
        });
        assert!(settings.sound);
    }

    /// #975 renamed the field with the consent it stores. A rename that read
    /// the old document as a default would revoke a grant the user gave
    /// without telling them, which is worse than asking again on purpose, so
    /// the alias carries it over. `use_screen_recording` is the same rename
    /// one step further back.
    #[test]
    fn a_grant_stored_under_an_older_field_name_is_still_a_grant() {
        for old_name in [
            "use_window_titles",
            "use_screen_recording",
            "use_portal_screencast",
        ] {
            let path = temp_path();
            fs::write(&path, format!(r#"{{"{old_name}":true}}"#)).expect("write");
            assert!(
                Settings::load(&path).use_window_names,
                "{old_name} has to load as the window-names grant"
            );
            let _ = fs::remove_file(&path);
        }
    }

    #[test]
    fn a_saved_light_appearance_loads_as_light() {
        let path = temp_path();
        let settings = Settings {
            chat_appearance: ChatAppearance::Light,
            ..Settings::default()
        };
        settings.save(&path).expect("save");
        assert_eq!(Settings::load(&path).chat_appearance, ChatAppearance::Light);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_light_token_in_either_case_loads_as_light() {
        for json in [
            r#"{"chat_appearance":"light"}"#,
            r#"{"chat_appearance":"Light"}"#,
        ] {
            let path = temp_path();
            fs::write(&path, json).expect("write");
            assert_eq!(
                Settings::load(&path).chat_appearance,
                ChatAppearance::Light,
                "{json}"
            );
            let _ = fs::remove_file(&path);
        }
    }

    #[test]
    fn a_bad_chat_appearance_token_loads_as_system_and_keeps_a_sibling() {
        for json in [
            r#"{"director_enabled":false,"chat_appearance":"sepia"}"#,
            r#"{"director_enabled":false,"chat_appearance":null}"#,
            r#"{"director_enabled":false,"chat_appearance":1}"#,
        ] {
            let path = temp_path();
            fs::write(&path, json).expect("write");
            let settings = Settings::load(&path);
            assert_eq!(settings.chat_appearance, ChatAppearance::System, "{json}");
            assert!(!settings.director_enabled, "{json}");
            let _ = fs::remove_file(&path);
        }
    }

    #[test]
    fn applying_appearance_sends_the_op_once() {
        let settings = Settings::default();
        let mut patch = SettingsPatch::default();
        patch.set_text(TextField::ChatAppearance, "Light");
        match chat_appearance_op(settings.chat_appearance, &patch) {
            Some(SettingsOp::ChatAppearanceChanged { chat_appearance }) => {
                assert_eq!(chat_appearance, ChatAppearance::Light);
            }
            other => panic!("expected ChatAppearanceChanged, got {other:?}"),
        }
        let mut next = settings.clone();
        next.apply(patch.clone());
        assert_eq!(next.chat_appearance, ChatAppearance::Light);
        assert!(
            chat_appearance_op(next.chat_appearance, &patch).is_none(),
            "the same value must not send again"
        );
    }

    #[test]
    fn the_saved_document_does_not_carry_an_api_key() {
        let path = temp_path();
        let settings = Settings {
            director_base_url: "https://api.x.ai".to_string(),
            director_model: "grok-4.6".to_string(),
            ..Settings::default()
        };
        settings.save(&path).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(!text.contains("api_key"), "{text}");
        assert!(!text.contains("sk-"), "{text}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn the_settings_view_never_holds_the_raw_key() {
        let settings = Settings {
            director_base_url: "https://api.x.ai".to_string(),
            director_model: "grok-4.6".to_string(),
            ..Settings::default()
        };
        // The endpoint the view prints is the resolved one, so a var exported
        // in the developer's shell would otherwise decide these two.
        model::tests::with_env(None, None, None, || {
            let view = SettingsView::from_parts(
                &settings,
                Path::new("/tmp/fidget/memory.md"),
                Some("You are Nim.".to_string()),
                vec!["nim".to_string()],
                Vec::new(),
                (true, "len=12 last=key1".to_string(), String::new()),
                None,
            );
            assert_eq!(view.director_base_url, "https://api.x.ai");
            assert_eq!(view.director_model, "grok-4.6");
            assert!(view.api_key_set);
            assert_eq!(view.api_key_fingerprint, "len=12 last=key1");
            assert_eq!(view.api_key_placeholder(), "Set: len=12 last=key1");
            let dump = format!("{view:?}");
            assert!(!dump.contains("sk-"), "{dump}");
        });
    }

    #[test]
    fn the_key_placeholder_is_not_set_when_unset() {
        model::tests::with_env(None, None, None, || {
            let view = SettingsView::from_parts(
                &Settings::default(),
                Path::new("/tmp/fidget/memory.md"),
                None,
                Vec::new(),
                Vec::new(),
                (false, String::new(), String::new()),
                None,
            );
            assert_eq!(view.api_key_placeholder(), "Not set");
            assert!(!view.clear_key_enabled());
        });
    }

    #[test]
    fn the_key_placeholder_is_unavailable_when_the_store_cannot_be_read() {
        model::tests::with_env(None, None, None, || {
            let view = SettingsView::from_parts(
                &Settings::default(),
                Path::new("/tmp/fidget/memory.md"),
                None,
                Vec::new(),
                Vec::new(),
                (false, String::new(), "keychain locked".into()),
                None,
            );
            assert!(!view.api_key_set);
            assert_eq!(view.api_key_placeholder(), "Unavailable: keychain locked");
            assert!(
                view.clear_key_enabled(),
                "Clear stays offered so a key we could not read can still be wiped"
            );
            assert_ne!(
                view.api_key_placeholder(),
                "Not set",
                "a locked store must not look like no key"
            );
        });
    }

    #[test]
    fn apply_ignores_the_api_key_patch_on_the_file() {
        let mut settings = Settings::default();
        settings.apply(SettingsPatch {
            completer: CompleterPatch {
                director_base_url: Some("https://api.x.ai".to_string()),
                director_model: Some("grok-4.6".to_string()),
                director_api_key: Some("sk-should-not-land".to_string()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        });
        assert_eq!(settings.director_base_url, "https://api.x.ai");
        assert_eq!(settings.director_model, "grok-4.6");
        let path = temp_path();
        settings.save(&path).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(!text.contains("sk-should-not-land"), "{text}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn settings_patch_debug_omits_the_raw_key() {
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_api_key: Some("sk-super-secret-key".into()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        let dump = format!("{patch:?}");
        assert!(
            !dump.contains("sk-super-secret-key"),
            "Debug must not echo the key: {dump}"
        );
        assert!(
            dump.contains(&model::key_fingerprint("sk-super-secret-key")),
            "Debug should name the fingerprint: {dump}"
        );
    }

    /// Proactive model calls are their own switch. Turning them off must not turn the
    /// Director off, or Poke would go silent with idle life.
    #[test]
    fn proactive_wakes_can_be_off_while_the_director_stays_on() {
        let settings = Settings {
            director_enabled: true,
            proactive_wakes: false,
            ..Settings::default()
        };

        assert!(settings.director_enabled);
        assert!(!settings.proactive_wakes);
    }

    /// A hand-edit that drops a key, or an older file, must not refuse to load.
    #[test]
    fn a_partial_document_fills_missing_keys_from_defaults() {
        let path = temp_path();
        fs::write(&path, r#"{"director_enabled":false}"#).expect("write");

        let settings = Settings::load(&path);
        assert!(!settings.director_enabled);
        assert!(settings.proactive_wakes, "unset proactive stays on");
        assert!(
            settings.sound,
            "a file from before the setting stays audible"
        );
        assert!(settings.hide_in_fullscreen);
        assert!(
            settings.harness_cwd.is_empty(),
            "a file from before the row is empty, which is the data folder"
        );
        assert!(settings.director_base_url.is_empty());
        assert!(settings.director_model.is_empty());
        assert!(settings.pi_project_mcp);
        assert_eq!(settings.chat_appearance, ChatAppearance::System);
        let _ = fs::remove_file(&path);
    }

    /// Garbage is first-run rather than a crash. The character staying up is the
    /// product; the file can be rewritten the next time something is toggled.
    #[test]
    fn a_corrupt_document_degrades_to_defaults() {
        let path = temp_path();
        fs::write(&path, "not json {").expect("write");

        assert_eq!(Settings::load(&path), Settings::default());
        let _ = fs::remove_file(&path);
    }

    /// A file from before the rename. The old key off must not load as on.
    #[test]
    fn an_old_ambient_wakes_key_still_loads_as_off() {
        let path = temp_path();
        fs::write(&path, r#"{"ambient_wakes":false}"#).expect("write");
        let settings = Settings::load(&path);
        assert!(!settings.proactive_wakes);
        settings.save(&path).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("\"proactive_wakes\": false"), "{text}");
        assert!(!text.contains("ambient_wakes"), "{text}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn settings_sit_beside_memory_in_the_data_dir() {
        assert_eq!(
            settings_path(Path::new("/tmp/fidget")),
            PathBuf::from("/tmp/fidget/settings.json")
        );
    }

    #[test]
    fn the_default_hide_hotkey_parses() {
        assert_eq!(
            parse_hotkey(DEFAULT_HIDE_HOTKEY),
            Some(Hotkey {
                control: true,
                option: true,
                shift: false,
                command: true,
                key: 'B',
            })
        );
    }

    /// A hotkey hide that does not write `hidden` comes back on restart, and a
    /// later settings patch of something else would overwrite HideRules with
    /// the stale flag. The menu already persists; the hotkey must too.
    #[test]
    fn hiding_from_the_hotkey_is_what_the_next_launch_reads() {
        let mut rules = HideRules::default();
        let mut settings = Settings::default();
        assert!(!settings.hidden);

        toggle_away(&mut rules, &mut settings);
        assert!(rules.is_away());
        assert!(
            settings.hidden,
            "hotkey hide must set the same flag the menu writes"
        );

        let path = temp_path();
        settings.save(&path).expect("save");
        let loaded = Settings::load(&path);
        let mut restarted = HideRules::default();
        restarted.set_away(loaded.hidden);
        assert!(restarted.is_away());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_rebound_hotkey_names_its_own_key_not_the_shipped_b() {
        assert_eq!(parse_hotkey("Control-Shift-H").map(|h| h.key), Some('H'));
        assert_eq!(key_code_name('H').as_deref(), Some("KeyH"));
        assert_eq!(key_code_name('B').as_deref(), Some("KeyB"));
        assert_eq!(key_code_name('1'), None);
    }

    /// The window reads this snapshot, not the file, so a field that does not
    /// appear here is a field the user cannot see.
    #[test]
    fn the_settings_view_is_what_the_window_shows() {
        let settings = Settings {
            director_enabled: false,
            proactive_wakes: false,
            do_not_disturb: true,
            sound: false,
            hidden: true,
            hide_in_fullscreen: false,
            hide_hotkey: "Control-Shift-H".to_string(),
            launch_at_login: true,
            excluded_applications: vec!["1Password".to_string(), "Keychain Access".to_string()],
            character: "nim".to_string(),
            instances: Vec::new(),
            director_base_url: String::new(),
            director_model: String::new(),
            director_timeout_secs: String::new(),
            director_max_tokens: String::new(),
            director_reasoning_effort: String::new(),
            director_wake_secs: String::new(),
            harness: String::new(),
            harness_command: String::new(),
            harness_auth_retry_secs: String::new(),
            harness_turn_timeout_secs: String::new(),
            byo_harness: String::new(),
            mcp_bin: String::new(),
            harness_cwd: String::new(),
            pi_project_mcp: true,
            trace_frames: false,
            trace_hittest: false,
            trace_director: false,
            trace_engine: false,
            trace_bubble: false,
            director_blank: false,
            capturable: true,
            chat_ui: "minimal".into(),
            chat_appearance: ChatAppearance::System,
            use_accessibility: true,
            use_window_names: false,
            use_input_monitoring: false,
            first_run_tour_shown: false,
            names_hint_dismissed: false,
        };
        let view = SettingsView::from_parts(
            &settings,
            Path::new("/tmp/fidget/memory.md"),
            Some("You are Nim.".to_string()),
            vec!["bmo".to_string(), "nim".to_string()],
            vec![InstanceRow {
                id: "1".to_string(),
                name: "Nim".to_string(),
                character: "nim".to_string(),
                prompt: String::new(),
            }],
            (false, String::new(), String::new()),
            None,
        );
        assert!(!view.director_enabled);
        assert!(!view.proactive_wakes);
        assert!(view.do_not_disturb);
        assert!(!view.sound);
        assert!(view.hidden);
        assert!(!view.hide_in_fullscreen);
        assert_eq!(view.hide_hotkey, display_hotkey("Control-Shift-H"));
        assert_eq!(view.excluded_text(), "1Password\nKeychain Access");
        assert_eq!(view.character, "nim");
        assert_eq!(view.memory_path, "/tmp/fidget/memory.md");
        assert_eq!(view.last_payload.as_deref(), Some("You are Nim."));
        assert_eq!(view.installed, ["bmo", "nim"]);
        assert_eq!(view.instances[0].name, "Nim");
        assert_eq!(view.instances[0].character, "nim");
        assert!(!view.api_key_set);
        #[cfg(not(target_os = "linux"))]
        {
            #[cfg(target_os = "macos")]
            assert_eq!(
                view.consent.iter().map(|row| row.title).collect::<Vec<_>>(),
                [
                    "Accessibility",
                    "Window and application names",
                    "Input Monitoring"
                ]
            );
            #[cfg(target_os = "windows")]
            {
                assert_eq!(
                    view.consent.iter().map(|row| row.title).collect::<Vec<_>>(),
                    ["Accessibility", "Window and Application Names"]
                );
                assert!(!view.consent[1].granted);
            }
            assert!(
                view.consent[0].granted,
                "the checkbox follows settings intent, not the OS grant"
            );
            #[cfg(target_os = "macos")]
            assert!(!view.consent[1].granted);
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(view.consent.len(), 1);
            assert_eq!(view.consent[0].title, "Window and application names");
            assert!(!view.consent[0].granted);
        }
        #[cfg(target_os = "macos")]
        {
            let intro = consent::pane_intro("Cursor");
            assert!(
                intro.contains("Cursor"),
                "the pane has to name the TCC row, got {:?}",
                intro
            );
        }
    }

    /// The document field the native checkbox writes. `SettingsSession::apply`
    /// and launch seed `consent::wanted` from it; the tap follows that, and
    /// `a_cleared_setting_starts_no_tap` is the spawn-side half. Off is the
    /// shipped state: decision 9 does not let a first run ask (#721).
    #[test]
    #[cfg(target_os = "macos")]
    fn unchecking_input_monitoring_clears_store_intent() {
        let mut settings = Settings::default();
        assert!(!settings.wants_consent(CapabilityId::InputMonitoring));

        settings.apply(SettingsPatch {
            use_input_monitoring: Some(true),
            ..SettingsPatch::default()
        });
        assert!(settings.use_input_monitoring);
        assert!(settings.wants_consent(CapabilityId::InputMonitoring));

        settings.apply(SettingsPatch {
            use_input_monitoring: Some(false),
            ..SettingsPatch::default()
        });
        assert!(
            !settings.wants_consent(CapabilityId::InputMonitoring),
            "unchecking has to stop the tap even though macOS keeps the grant"
        );
    }

    #[test]
    fn unchecking_consent_stops_using_it_without_a_file_grant() {
        let mut settings = Settings::default();
        #[cfg(not(target_os = "linux"))]
        {
            settings.apply(SettingsPatch {
                use_accessibility: Some(true),
                ..SettingsPatch::default()
            });
            assert!(settings.use_accessibility);
            settings.apply(SettingsPatch {
                use_accessibility: Some(false),
                ..SettingsPatch::default()
            });
            assert!(!settings.use_accessibility);
        }
        #[cfg(target_os = "linux")]
        {
            settings.apply(SettingsPatch {
                use_window_names: Some(true),
                ..SettingsPatch::default()
            });
            assert!(settings.use_window_names);
            settings.apply(SettingsPatch {
                use_window_names: Some(false),
                ..SettingsPatch::default()
            });
            assert!(!settings.use_window_names);
        }
        #[cfg(target_os = "windows")]
        {
            settings.apply(SettingsPatch {
                use_window_names: Some(true),
                ..SettingsPatch::default()
            });
            assert!(settings.use_window_names);
            settings.apply(SettingsPatch {
                use_window_names: Some(false),
                ..SettingsPatch::default()
            });
            assert!(!settings.use_window_names);
        }
        let view = SettingsView::from_parts(
            &settings,
            Path::new("/tmp/memory.md"),
            None,
            Vec::new(),
            Vec::new(),
            (false, String::new(), String::new()),
            None,
        );
        assert!(
            !view.consent[0].granted,
            "unchecking has to show off even if the OS still holds the grant"
        );

        // Consent rows must appear in row_values by their row_id.
        let values = view.row_values();
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(
                values.get(form::CONSENT_ACCESSIBILITY_ID),
                Some(&RowValue::Bool(settings.use_accessibility)),
                "Accessibility checkbox value must match settings.use_accessibility"
            );
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                values.get(form::CONSENT_WINDOW_NAMES_ID),
                Some(&RowValue::Bool(settings.use_window_names)),
                "The window-names checkbox value must match settings.use_window_names"
            );
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                values.get(form::CONSENT_SCREEN_RECORDING_ID),
                Some(&RowValue::Bool(settings.use_window_names)),
                "Screen Recording checkbox value must match settings.use_window_names"
            );
            assert_eq!(
                values.get(form::CONSENT_INPUT_MONITORING_ID),
                Some(&RowValue::Bool(settings.use_input_monitoring)),
                "Input Monitoring checkbox value must match settings.use_input_monitoring"
            );
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                values.get(form::CONSENT_PORTAL_SCREENCAST_ID),
                Some(&RowValue::Bool(settings.use_window_names)),
                "The Linux window-names checkbox value must match settings.use_window_names"
            );
        }
    }

    /// The Instances list is this view. After a dismiss the window must
    /// redraw from a view that no longer carries the gone row.
    #[test]
    fn a_dismissed_instance_is_gone_from_the_settings_list() {
        let settings = Settings::default();
        let remaining = vec![InstanceRow {
            id: "trump".to_string(),
            name: "Trump".to_string(),
            character: "Trump".to_string(),
            prompt: String::new(),
        }];
        let view = SettingsView::from_parts(
            &settings,
            Path::new("/tmp/memory.md"),
            None,
            vec!["Trump".to_string(), "Cat".to_string()],
            remaining,
            (false, String::new(), String::new()),
            None,
        );
        assert_eq!(
            view.instances
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            ["Trump"],
            "Cat must not remain after it was dismissed"
        );
    }

    #[test]
    fn write_director_key_sets_the_store_and_not_the_file() {
        let store = MemoryStore::new();
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_api_key: Some("sk-from-settings".to_string()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        write_director_key(&store, &patch).unwrap();
        assert_eq!(
            store.get(DIRECTOR_API_KEY).unwrap().as_deref(),
            Some("sk-from-settings")
        );
    }

    #[test]
    fn write_director_key_clears_on_empty() {
        let store = MemoryStore::new();
        store.set(DIRECTOR_API_KEY, "sk-from-settings").unwrap();
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_api_key: Some(String::new()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        write_director_key(&store, &patch).unwrap();
        assert_eq!(store.get(DIRECTOR_API_KEY).unwrap(), None);
    }

    #[test]
    fn write_director_key_fails_loudly_when_store_set_fails() {
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_api_key: Some("sk-new-key".into()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        let result = write_director_key(&FailingStore, &patch);
        assert!(
            result.is_err(),
            "a store set error must fail loudly, not succeed silently"
        );
    }

    #[test]
    fn write_director_key_none_leaves_the_store() {
        let store = MemoryStore::new();
        store.set(DIRECTOR_API_KEY, "sk-from-settings").unwrap();
        write_director_key(&store, &SettingsPatch::default()).unwrap();
        assert_eq!(
            store.get(DIRECTOR_API_KEY).unwrap().as_deref(),
            Some("sk-from-settings")
        );
    }

    struct FailingStore;

    impl SecretStore for FailingStore {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Err("keychain locked".into())
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            Err("keychain locked".into())
        }
        fn delete(&self, _: &str) -> Result<(), String> {
            Err("keychain locked".into())
        }
    }

    #[test]
    fn a_store_get_error_is_not_an_unset_remote_key() {
        let settings = Settings {
            director_base_url: "https://api.openai.com".into(),
            director_model: "gpt-4o-mini".into(),
            ..Settings::default()
        };
        // A key exported in the developer's shell would resolve this remote
        // as configured and take the precondition with it.
        model::tests::with_env(None, None, None, || {
            assert!(
                director_settings(&settings, &FailingStore).is_err(),
                "a store error must not resolve as no key"
            );
            let unset = model::resolve(&settings.director_base_url, &settings.director_model, None);
            assert!(
                !model::config_from(&unset).configured,
                "precondition: unset remote is Static"
            );
        });
    }

    /// A store that counts its reads. Each one is a Keychain dialog on macOS,
    /// so the count is the assertion in #290.
    struct CountingStore {
        inner: MemoryStore,
        reads: AtomicUsize,
    }

    impl CountingStore {
        fn with_key(key: &str) -> Self {
            let inner = MemoryStore::new();
            inner.set(DIRECTOR_API_KEY, key).unwrap();
            Self {
                inner,
                reads: AtomicUsize::new(0),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    impl SecretStore for CountingStore {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.get(account)
        }
        fn set(&self, account: &str, value: &str) -> Result<(), String> {
            self.inner.set(account, value)
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.inner.delete(account)
        }
    }

    /// The bug: a Harness launch prompted for a key it can never send. The
    /// Harness is the Completer for every Instance (ADR-0008), so the HTTP
    /// settings are not consulted and the dialog buys the user nothing.
    #[test]
    fn a_harness_launch_never_reads_the_store() {
        let store = CountingStore::with_key("sk-saved-key");
        let settings = endpoint_settings();
        model::tests::with_env(None, None, None, || {
            let director = resolve_director(&settings, &store, true).expect("resolve");
            assert_eq!(store.reads(), 0, "an attached Harness needs no stored key");
            assert!(director.api_key.is_empty());
        });
    }

    /// The other half: with the HTTP Completer in force the key is read at
    /// startup, where the user is starting the app, and `configured` is a
    /// settled fact — so Toggle Director has something to turn on.
    #[test]
    fn an_http_launch_reads_once_and_toggle_director_still_works() {
        let store = CountingStore::with_key("sk-saved-key");
        let settings = Settings {
            director_enabled: false,
            ..endpoint_settings()
        };
        model::tests::with_env(None, None, None, || {
            let director = resolve_director(&settings, &store, false).expect("resolve");
            assert_eq!(store.reads(), 1, "the HTTP Completer needs the key now");
            assert_eq!(director.api_key, "sk-saved-key");

            let mut config = model::config_from(&director);
            config.apply_switch(settings.director_enabled);
            assert!(config.configured);
            assert!(!config.enabled, "precondition: the switch is off");
            config.apply_switch(true);
            assert!(config.enabled, "Toggle Director turns the model on");
            assert_eq!(store.reads(), 1, "and asks the store nothing further");
        });
    }

    /// A wake is not a moment a user can answer a dialog in: it lands while
    /// they are working in another window, with the wake worker blocked
    /// behind it. Everything a wake touches is built from the startup read.
    #[test]
    fn no_wake_ever_reads_the_store() {
        let store = CountingStore::with_key("sk-saved-key");
        let settings = endpoint_settings();
        model::tests::with_env(None, None, None, || {
            let director = resolve_director(&settings, &store, false).expect("resolve");
            let config = model::config_from(&director);
            assert_eq!(store.reads(), 1, "precondition: startup read the key");

            // What the frame loop does to put a Completer in front of a wake,
            // and what the wake itself sends.
            let id = "fidget".to_string();
            let mut slots = completer::tests::slots_awaiting_a_wake(&id);
            let mut completer = None;
            completer::retarget_model(
                &mut slots,
                &id,
                &mut completer,
                ["stroll"],
                "cat",
                &director,
                config.configured,
            );
            let endpoint = model::endpoint_from(&director).expect("a keyed remote is a Completer");
            assert_eq!(
                endpoint.key_fingerprint(),
                model::key_fingerprint("sk-saved-key")
            );

            assert_eq!(store.reads(), 1, "no wake may reach the secret store");
        });
    }

    /// A store read is a Keychain prompt on macOS, and one whose answer
    /// `resolve` throws away is a prompt for nothing. `FailingStore` is the
    /// assertion: this can only resolve if nothing consulted the store.
    #[test]
    fn an_exported_key_leaves_the_store_unread() {
        let settings = endpoint_settings();
        model::tests::with_env(Some("sk-env-key"), None, None, || {
            let resolved = director_settings(&settings, &FailingStore)
                .expect("the env owns the key, so the store has nothing to say");
            assert_eq!(resolved.api_key, "sk-env-key");
        });
    }

    /// A blank export is a mistake — `$XAI_API_KEY` that expanded to nothing —
    /// and the warning that names it is what the launch owes the user. A
    /// stored key must not answer in its place, quietly or at the price of a
    /// prompt.
    #[test]
    fn a_blank_exported_key_leaves_the_store_unread_and_still_warns() {
        let settings = endpoint_settings();
        model::tests::with_env(Some("  "), None, None, || {
            let resolved = director_settings(&settings, &FailingStore)
                .expect("a blank export is still the env answering");
            assert!(resolved.api_key.is_empty());
            assert!(
                resolved.key_invalid,
                "the blank export must still reach the startup warning"
            );
        });
    }

    #[test]
    fn a_store_get_error_is_not_presented_as_unset() {
        let (set, fingerprint, error) = stored_key_status(&FailingStore);
        assert!(
            !error.is_empty(),
            "a get Err must carry the failure, not look like Unset"
        );
        assert!(!set);
        assert!(fingerprint.is_empty());
        let view = SettingsView::from_parts(
            &Settings::default(),
            Path::new("/tmp/fidget/memory.md"),
            None,
            Vec::new(),
            Vec::new(),
            (set, fingerprint, error),
            None,
        );
        assert_ne!(view.api_key_placeholder(), "Not set");
        assert!(view.clear_key_enabled());
        let (unset, empty, no_error) = stored_key_status(&MemoryStore::new());
        assert!(!unset);
        assert!(empty.is_empty());
        assert!(no_error.is_empty());
    }

    fn endpoint_settings() -> Settings {
        Settings {
            director_base_url: "https://api.openai.com".into(),
            director_model: "gpt-4o-mini".into(),
            ..Settings::default()
        }
    }

    fn stall_the_save_of(path: &Path) {
        *SAVE_STALL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path.to_path_buf());
    }

    /// The two save stalls share one slot and one stalling flag. Running them
    /// together would let one replace the stall the other is waiting on.
    fn settings_save_tests() -> std::sync::MutexGuard<'static, ()> {
        static ORDER: Mutex<()> = Mutex::new(());
        ORDER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn a_slow_settings_save_does_not_block_the_frame_lock() {
        let _order = settings_save_tests();
        let settings = Arc::new(Mutex::new(Settings::default()));
        let path = temp_path();
        stall_the_save_of(&path);
        let (tx, rx) = mpsc::channel();
        let shared = Arc::clone(&settings);
        let file = path.clone();
        thread::spawn(move || {
            let _ = tx.send(save_after_release(&shared, &file));
        });
        let until = Instant::now() + Duration::from_secs(5);
        while !SAVE_STALLING.load(Ordering::SeqCst) {
            assert!(Instant::now() < until, "the settings save never stalled");
            thread::sleep(Duration::from_millis(10));
        }
        let started = Instant::now();
        let guard = settings.lock().expect("settings");
        let elapsed = started.elapsed();
        drop(guard);
        assert!(
            elapsed < Duration::from_millis(500),
            "the frame loop waited {:?} on the settings file",
            elapsed
        );
        assert!(rx.recv_timeout(Duration::from_secs(4)).unwrap().is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_later_settings_edit_is_what_the_file_keeps() {
        let _order = settings_save_tests();
        let settings = Arc::new(Mutex::new(Settings::default()));
        let path = temp_path();
        stall_the_save_of(&path);
        let first = {
            let settings = Arc::clone(&settings);
            let path = path.clone();
            thread::spawn(move || flush_settings(&settings, &path))
        };
        let until = Instant::now() + Duration::from_secs(5);
        while !SAVE_STALLING.load(Ordering::SeqCst) {
            assert!(Instant::now() < until, "the settings save never stalled");
            thread::sleep(Duration::from_millis(10));
        }
        settings.lock().expect("settings").hidden = true;
        let second = {
            let settings = Arc::clone(&settings);
            let path = path.clone();
            thread::spawn(move || flush_settings(&settings, &path))
        };
        assert!(first.join().expect("first flush").is_ok());
        assert!(second.join().expect("second flush").is_ok());
        assert!(
            Settings::load(&path).hidden,
            "the stalled snapshot overwrote the later edit"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_slow_keychain_write_does_not_block_the_frame_lock() {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let store = MemoryStore::new();
        let mut patch = SettingsPatch::default();
        patch.completer.director_api_key = Some("sk-from-settings".into());
        KEY_STALL.store(true, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        let shared = Arc::clone(&settings);
        thread::spawn(move || {
            let _ = tx.send(write_key_off_the_settings_lock(&shared, &store, &patch));
        });
        let until = Instant::now() + Duration::from_secs(5);
        while !KEY_STALLING.load(Ordering::SeqCst) {
            assert!(Instant::now() < until, "the key write never stalled");
            thread::sleep(Duration::from_millis(10));
        }
        let started = Instant::now();
        let guard = settings.lock().expect("settings");
        let elapsed = started.elapsed();
        drop(guard);
        assert!(
            elapsed < Duration::from_millis(500),
            "the frame loop waited {:?} on the keychain write",
            elapsed
        );
        assert!(rx.recv_timeout(Duration::from_secs(4)).unwrap().is_ok());
    }

    #[test]
    fn a_slow_retarget_keychain_read_does_not_block_the_frame_lock() {
        let settings = Arc::new(Mutex::new(endpoint_settings()));
        let snapshot = settings.lock().expect("settings").clone();
        let store = MemoryStore::new();
        RETARGET_STALL.store(true, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        let shared = Arc::clone(&settings);
        thread::spawn(move || {
            let result = retarget_off_the_settings_lock(&shared, &snapshot, &store).map(|_| ());
            let _ = tx.send(result);
        });
        let until = Instant::now() + Duration::from_secs(5);
        while !RETARGET_STALLING.load(Ordering::SeqCst) {
            assert!(Instant::now() < until, "the retarget read never stalled");
            thread::sleep(Duration::from_millis(10));
        }
        let started = Instant::now();
        let guard = settings.lock().expect("settings");
        let elapsed = started.elapsed();
        drop(guard);
        assert!(
            elapsed < Duration::from_millis(500),
            "the frame loop waited {:?} on the retarget keychain read",
            elapsed
        );
        assert!(rx.recv_timeout(Duration::from_secs(4)).unwrap().is_ok());
    }

    /// The AI tab's live state, with or without a stored key.
    fn director_view(api_key_set: bool) -> SettingsView {
        let fingerprint = if api_key_set {
            "len=12 last=key1".to_string()
        } else {
            String::new()
        };
        SettingsView::from_parts(
            &endpoint_settings(),
            Path::new("/tmp/fidget/memory.md"),
            None,
            Vec::new(),
            Vec::new(),
            (api_key_set, fingerprint, String::new()),
            None,
        )
    }

    /// One patch for the whole tab, so the three edits #279 opens with cost
    /// one rebuild instead of three.
    #[test]
    fn one_apply_retargets_once_for_a_new_url_and_model() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(false);
            let description = form::describe();
            let patch = AiDraft::drawn(&description, serde_json::json!({ "director_base_url": "https://api.x.ai", "director_model": "grok-4.6" }))
            .patch(&view)
            .expect("a new URL and model is dirty");
            assert_eq!(
                patch.completer.director_base_url.as_deref(),
                Some("https://api.x.ai")
            );
            assert_eq!(patch.completer.director_model.as_deref(), Some("grok-4.6"));
            assert!(
                patch.completer.director_api_key.is_none(),
                "an untouched key field is not part of the batch"
            );
            assert!(
                completer_retargets(&endpoint_settings(), &patch),
                "the one patch has to carry the one Retarget"
            );
        });
    }

    /// #272: the window shows what the variable imposes, so the text in a
    /// frozen field differs from the file and would otherwise read as dirty.
    ///
    /// The variable is exported for real here, so the frozen rule is tested
    /// through the description both windows build from rather than through a
    /// `None` a renderer had to remember to pass.
    #[test]
    fn a_frozen_row_never_applies_even_when_its_text_differs() {
        model::tests::with_env(None, Some("https://env.example"), None, || {
            let view = director_view(false);
            let description = form::describe();
            assert!(
                description.frozen(form::DIRECTOR_BASE_URL_ID),
                "precondition: the variable owns the URL row"
            );
            let draft = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_base_url": "https://typed.example", "director_model": "grok-4.6" }),
            );
            let patch = draft
                .patch(&view)
                .expect("the model row is still the user's");
            assert!(patch.completer.director_base_url.is_none());
            assert_eq!(patch.completer.director_model.as_deref(), Some("grok-4.6"));
        });
    }

    /// Cancel builds no patch at all — it is `refresh()`. What this pins is
    /// the state it leaves behind: a blank field is untouched, not a delete.
    #[test]
    fn a_cancelled_key_never_reaches_a_patch() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(true);
            let description = form::describe();
            let typed = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_api_key": "sk-typed-then-cancelled" }),
            );
            assert!(
                typed.patch(&view).is_some(),
                "precondition: a typed key is dirty"
            );
            // What Cancel leaves behind: the blank field a redraw writes.
            let after_cancel =
                AiDraft::drawn(&description, serde_json::json!({ "director_api_key": "" }));
            assert!(
                after_cancel.patch(&view).is_none(),
                "a key never typed is not a delete"
            );
        });
    }

    #[test]
    fn a_staged_clear_deletes_the_stored_key() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(true);
            let description = form::describe();
            let patch = AiDraft::drawn(&description, serde_json::json!({ "clear_key": true }))
                .patch(&view)
                .expect("a staged clear is dirty");
            assert_eq!(patch.completer.director_api_key.as_deref(), Some(""));
        });
    }

    #[test]
    fn ai_switches_and_wake_interval_commit_together() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(false);
            let description = form::describe();
            let draft = AiDraft::drawn(
                &description,
                serde_json::json!({ "director": !view.director_enabled, "proactive": !view.proactive_wakes, "pi_project_mcp": !view.development_switches[form::PI_PROJECT_MCP_ID], "director_wake_secs": "240", "byo_harness": "hermes" }),
            );
            let patch = draft.patch(&view).expect("the edits are staged");
            assert_eq!(
                patch.completer.director_enabled,
                Some(!view.director_enabled)
            );
            assert_eq!(patch.completer.proactive_wakes, Some(!view.proactive_wakes));
            assert_eq!(
                patch.completer.pi_project_mcp,
                Some(!view.development_switches[form::PI_PROJECT_MCP_ID])
            );
            assert_eq!(patch.completer.director_wake_secs.as_deref(), Some("240"));
            assert_eq!(patch.completer.byo_harness.as_deref(), Some("hermes"));
        });
    }

    /// Nothing to clear is nothing to apply: the buttons would otherwise
    /// offer a delete of a key that is not there.
    #[test]
    fn a_staged_clear_on_an_unset_key_is_not_a_change() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(false);
            assert!(!view.clear_key_enabled(), "precondition: no key is stored");
            let description = form::describe();
            let draft = AiDraft::drawn(&description, serde_json::json!({ "clear_key": true }));
            assert!(draft.patch(&view).is_none());
        });
    }

    /// Clear key blanks the field, so text in it afterwards is the later
    /// intent.
    #[test]
    fn a_typed_key_beats_a_staged_clear() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(true);
            let description = form::describe();
            let patch = AiDraft::drawn(&description, serde_json::json!({ "director_api_key": "sk-typed-after-clear", "clear_key": true }))
            .patch(&view)
            .expect("a typed key is dirty");
            assert_eq!(
                patch.completer.director_api_key.as_deref(),
                Some("sk-typed-after-clear")
            );
        });
    }

    /// A redraw asks per field, not per tab. Freezing the whole tab on a
    /// typed key would hold stale endpoint text on screen, and Apply would
    /// write it back (#279).
    #[test]
    fn a_typed_key_stages_the_key_alone() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(true);
            let description = form::describe();
            let patch = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_api_key": "sk-typed" }),
            )
            .patch(&view);
            let mut key = SettingsPatch::default();
            key.completer.director_api_key = Some("sk-typed".into());
            assert_eq!(patch, Some(key));
        });
    }

    /// #279: a typed edit reads as staged, so the redraw that follows an app
    /// switch or any `SettingsOp` leaves it where the user left it.
    #[test]
    fn a_typed_endpoint_edit_survives_a_redraw() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(true);
            let description = form::describe();
            let draft = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_base_url": "https://api.x.ai" }),
            );
            let mut url = SettingsPatch::default();
            url.completer.director_base_url = Some("https://api.x.ai".into());
            assert_eq!(draft.patch(&view), Some(url));
        });
    }

    #[test]
    fn a_clean_tab_has_nothing_to_apply() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(true);
            let description = form::describe();
            let draft = AiDraft::live(&description);
            assert!(
                draft.patch(&view).is_none(),
                "a clean tab is what disables both buttons"
            );
        });
    }

    /// #663: a source pick is the same batch as the endpoint. Apply is the
    /// one patch, and Cancel is a draft that matches live state again.
    #[test]
    fn a_picked_source_is_one_apply_patch() {
        crate::model::tests::with_harness(None, || {
            let view = director_view(false);
            let description = form::describe();
            let draft = AiDraft::drawn(
                &description,
                serde_json::json!({ "harness": "Harness · opencode" }),
            );
            let patch = draft.patch(&view).expect("a pick is dirty");
            assert_eq!(patch.completer.harness.as_deref(), Some("opencode"));
            assert!(patch.completer.director_base_url.is_none());
            assert!(
                completer_retargets(&endpoint_settings(), &patch),
                "Apply has to retarget once, not the pick"
            );
            let cancelled = AiDraft::live(&description);
            assert!(cancelled.patch(&view).is_none());
            assert!(!completer_retargets(
                &endpoint_settings(),
                &SettingsPatch::default()
            ));
        });
    }

    /// Apply of a non-source Director edit must not reconnect the Harness.
    /// The URL retargets the Completer. The source rows stay out of the patch,
    /// so `harness::retarget` is not called for the child already running.
    #[test]
    fn an_endpoint_apply_does_not_retarget_an_unchanged_harness() {
        crate::model::tests::with_harness(None, || {
            let view = director_view(false);
            let description = form::describe();
            let draft = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_base_url": "https://api.x.ai" }),
            );
            let patch = draft.patch(&view).expect("a typed URL is dirty");
            assert_eq!(
                patch.completer.director_base_url.as_deref(),
                Some("https://api.x.ai")
            );
            assert!(patch.completer.harness.is_none());
            assert!(patch.completer.harness_command.is_none());
            let settings = endpoint_settings();
            assert!(!harness_retargets(&settings, &patch));
            assert!(completer_retargets(&settings, &patch));
        });
    }

    /// #272 on the source row: the variable owns the pick, so Apply must not
    /// write a title the launch would throw away (#663).
    #[test]
    fn a_frozen_source_never_applies() {
        crate::model::tests::with_harness(Some("claude"), || {
            let view = director_view(false);
            let description = form::describe();
            assert!(
                description.frozen(form::HARNESS_ID),
                "precondition: the variable owns the source row"
            );
            let draft = AiDraft::drawn(
                &description,
                serde_json::json!({ "harness": "Harness · opencode", "harness_command": "typed acp" }),
            );
            assert!(draft.patch(&view).is_none());
        });
    }

    /// #530: the fields a window is built with are empty, and a redraw that
    /// respects staging reads them back before anything has filled them. Every
    /// row differs from live state, so the whole tab reads as staged: the
    /// redraw skips filling it and Apply is armed over a patch of empty
    /// strings that wipes the saved endpoint.
    ///
    /// This is the reason construction draws with the reset instead of through
    /// `refresh`, and the reason it is not fixed here: a blank field has to
    /// stay a value the user can apply, or clearing a Base URL back to the
    /// default becomes inexpressible.
    #[test]
    fn empty_fields_read_back_as_an_edit_that_wipes_the_endpoint() {
        model::tests::with_env(None, None, None, || {
            let view = director_view(true);
            let description = form::describe();
            // The fields as a freshly built window holds them, a line before
            // its first redraw.
            let unfilled = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_base_url": "", "director_model": "" }),
            );
            let mut wipe = SettingsPatch::default();
            wipe.completer.director_base_url = Some(String::new());
            wipe.completer.director_model = Some(String::new());
            assert_eq!(
                unfilled.patch(&view),
                Some(wipe),
                "empty text differs from live state, so Apply is armed over the wipe"
            );
        });
    }

    #[test]
    fn tabbing_out_of_an_unchanged_endpoint_does_not_retarget() {
        let settings = endpoint_settings();
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_base_url: Some(settings.director_base_url.clone()),
                director_model: Some(settings.director_model.clone()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(
            !completer_retargets(&settings, &patch),
            "presence of the same URL and model must not reset session history"
        );
    }

    /// Trap 1 of #638: the level is baked into the `Endpoint`, so without a
    /// clause here an Apply changes the file and never reaches the running
    /// Director.
    #[test]
    fn a_changed_reasoning_effort_retargets() {
        let settings = Settings {
            director_reasoning_effort: "low".into(),
            ..endpoint_settings()
        };
        let changed = SettingsPatch {
            completer: CompleterPatch {
                director_reasoning_effort: Some("high".into()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(
            completer_retargets(&settings, &changed),
            "a new level only reaches the Director through a rebuild"
        );

        let same = SettingsPatch {
            completer: CompleterPatch {
                director_reasoning_effort: Some("low".into()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(
            !completer_retargets(&settings, &same),
            "both windows commit on blur, so the same value is not an edit"
        );
    }

    #[test]
    fn a_changed_base_url_or_model_retargets() {
        let settings = endpoint_settings();
        let url = SettingsPatch {
            completer: CompleterPatch {
                director_base_url: Some("https://api.x.ai".into()),
                director_model: Some(settings.director_model.clone()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(completer_retargets(&settings, &url));
        let model = SettingsPatch {
            completer: CompleterPatch {
                director_base_url: Some(settings.director_base_url.clone()),
                director_model: Some("grok-4.6".into()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(completer_retargets(&settings, &model));
    }

    /// #657: the mode decides the opening turn, and `opened` has no way back
    /// to one — so a toggle has to rebuild the Director. Without the rebuild
    /// the switch would only change the next session, and the current one
    /// would go on answering out of a Character Prompt it claims not to have.
    #[test]
    fn toggling_blank_ai_retargets_and_leaving_it_alone_does_not() {
        let settings = endpoint_settings();
        let on = SettingsPatch {
            completer: CompleterPatch {
                director_blank: Some(true),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(completer_retargets(&settings, &on));
        let unchanged = SettingsPatch {
            completer: CompleterPatch {
                director_blank: Some(settings.director_blank),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(
            !completer_retargets(&settings, &unchanged),
            "both windows commit on blur; the same value is not a change"
        );
    }

    #[test]
    fn a_key_patch_always_retargets() {
        let settings = endpoint_settings();
        let set = SettingsPatch {
            completer: CompleterPatch {
                director_base_url: Some(settings.director_base_url.clone()),
                director_model: Some(settings.director_model.clone()),
                director_api_key: Some("sk-new".into()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(completer_retargets(&settings, &set));
        let clear = SettingsPatch {
            completer: CompleterPatch {
                director_api_key: Some(String::new()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(completer_retargets(&settings, &clear));
    }

    #[test]
    fn retarget_payload_carries_resolved_settings() {
        // The payload is resolved, so an exported key would win over the
        // store's and this would be asserting the shell's environment.
        model::tests::with_env(None, None, None, || {
            let store = MemoryStore::new();
            store.set(DIRECTOR_API_KEY, "sk-stored-key").unwrap();
            let settings = endpoint_settings();
            match retarget_payload(&settings, &store).unwrap() {
                SettingsOp::Retarget {
                    settings,
                    enabled,
                    configured,
                    proactive_allowed,
                } => {
                    assert_eq!(settings.api_key, "sk-stored-key");
                    assert!(configured);
                    assert!(enabled);
                    assert!(proactive_allowed);
                    let dump = format!("{settings:?}");
                    assert!(
                        !dump.contains("sk-stored-key"),
                        "Retarget Debug must not echo the key: {dump}"
                    );
                }
                other => panic!("expected Retarget, got {other:?}"),
            }
        });
    }

    /// Saving is when the file's switch reaches the frame loop, so it is
    /// where a vetoed Director would come back on.
    #[test]
    fn retarget_cannot_switch_on_a_director_the_process_vetoed() {
        model::tests::with_env_switch("off", || {
            let store = MemoryStore::new();
            store.set(DIRECTOR_API_KEY, "sk-stored-key").unwrap();
            let settings = endpoint_settings();
            assert!(settings.director_enabled, "precondition: the file says on");
            match retarget_payload(&settings, &store).unwrap() {
                SettingsOp::Retarget {
                    enabled,
                    configured,
                    ..
                } => {
                    assert!(configured, "the stored key still configures it");
                    assert!(!enabled, "FIDGET_DIRECTOR=off outranks the file");
                }
                other => panic!("expected Retarget, got {other:?}"),
            }
        });
    }

    /// The row is frozen, so the box it draws has to be the value in force —
    /// in both directions, since either can disagree with the file.
    #[test]
    fn an_env_owned_director_reads_as_exported_in_the_window() {
        for (exported, saved) in [("off", true), ("on", false)] {
            model::tests::with_env_switch(exported, || {
                let settings = Settings {
                    director_enabled: saved,
                    ..Settings::default()
                };
                let view = SettingsView::from_parts(
                    &settings,
                    Path::new("/tmp/fidget/memory.md"),
                    None,
                    Vec::new(),
                    Vec::new(),
                    (false, String::new(), String::new()),
                    None,
                );

                assert_eq!(
                    view.director_enabled,
                    exported == "on",
                    "the file said {saved}, the process said {exported}"
                );
            });
        }
    }

    fn endpoint_view(settings: &Settings) -> SettingsView {
        SettingsView::from_parts(
            settings,
            Path::new("/tmp/fidget/memory.md"),
            None,
            Vec::new(),
            Vec::new(),
            (false, String::new(), String::new()),
            None,
        )
    }

    /// #272: the window has to print the endpoint the Director will use. The
    /// file value it used to print is the one `model::resolve` throws away.
    #[test]
    fn the_view_shows_the_endpoint_the_env_imposes() {
        let settings = endpoint_settings();
        model::tests::with_env(
            Some("sk-env-key"),
            Some("https://api.x.ai"),
            Some("grok-4.6"),
            || {
                let view = endpoint_view(&settings);
                assert_eq!(view.director_base_url, "https://api.x.ai");
                assert_eq!(view.director_model, "grok-4.6");
                assert_eq!(view.api_key_placeholder(), "Overridden by env");
            },
        );
        model::tests::with_env(None, None, None, || {
            let view = endpoint_view(&settings);
            assert_eq!(view.director_base_url, settings.director_base_url);
            assert_eq!(view.director_model, settings.director_model);
            assert_eq!(view.api_key_placeholder(), "Not set");
        });
    }

    /// Every Development control the window draws has to find its value in
    /// the view. A row whose id is missing renders off while the switch is on
    /// (#273), and the first click on a persisted `true` then sends
    /// `Some(true)` — a no-op the user reads as a dead checkbox.
    #[test]
    fn every_development_row_has_a_value_in_the_view() {
        model::tests::with_env(None, None, None, || {
            let view = endpoint_view(&Settings::default());
            let description = form::describe();
            let development = description
                .tabs
                .iter()
                .find(|tab| tab.title == "Development")
                .expect("the Development tab exists");

            for row in development
                .sections
                .iter()
                .flat_map(|section| &section.rows)
            {
                match row {
                    form::FormRow::Checkbox { id, .. } => {
                        assert!(
                            view.development_switches.contains_key(id),
                            "{id} has no value to draw"
                        )
                    }
                    form::FormRow::TextField { id, .. } => {
                        assert!(
                            view.development_texts.contains_key(id),
                            "{id} has no value to draw"
                        )
                    }
                    _ => {}
                }
            }
        });
    }

    /// The value in force, not the file's: an exported variable freezes the
    /// row, so printing the file's would draw a switch off while the trace it
    /// names is running (#273).
    #[test]
    fn the_view_shows_the_switch_the_env_imposes() {
        model::tests::with_env(None, None, None, || {
            let off = Settings {
                trace_frames: false,
                director_timeout_secs: "45".into(),
                ..Settings::default()
            };
            std::env::set_var(dev_flags::TRACE_FRAMES.var(), "1");
            std::env::set_var(model::TIMEOUT_SECS, "7");
            let view = endpoint_view(&off);
            assert!(
                view.development_switches[form::TRACE_FRAMES_ID],
                "the exported variable wins"
            );
            assert_eq!(view.development_texts[form::DIRECTOR_TIMEOUT_SECS_ID], "7");

            std::env::remove_var(dev_flags::TRACE_FRAMES.var());
            std::env::remove_var(model::TIMEOUT_SECS);
            let view = endpoint_view(&off);
            assert!(
                !view.development_switches[form::TRACE_FRAMES_ID],
                "the file wins with nothing exported"
            );
            assert_eq!(view.development_texts[form::DIRECTOR_TIMEOUT_SECS_ID], "45");
        });
    }

    /// A limit no read site can use has to read as unset, not as a number.
    ///
    /// `dev_flags::seed` parses the value and calls zero unset, so `=abc` and
    /// `=0` both leave `model::timeout_for` on its default. The row showed the
    /// export verbatim and named a timeout nothing waited that long for.
    #[test]
    fn an_unusable_limit_shows_as_blank_so_the_placeholder_names_the_default() {
        model::tests::with_env(None, None, None, || {
            let file = Settings {
                director_timeout_secs: "45".into(),
                ..Settings::default()
            };
            // Empty is no override at all, so the file's 45 stands there.
            for exported in ["abc", "0", "-1", ""] {
                std::env::set_var(model::TIMEOUT_SECS, exported);
                dev_flags::seed(&file);
                let expected = match dev_flags::director_timeout_secs() {
                    Some(secs) => secs.to_string(),
                    None => String::new(),
                };
                let view = endpoint_view(&file);
                assert_eq!(
                    view.development_texts[form::DIRECTOR_TIMEOUT_SECS_ID],
                    expected,
                    "exported {exported:?}"
                );
            }
            std::env::remove_var(model::TIMEOUT_SECS);
        });
    }

    /// The hop the settings window makes, end to end, for each endpoint field:
    /// the patch a committed field carries, the retarget decision, the payload
    /// resolved off the frame thread, and the rebuild the frame loop does with
    /// it (#272). The three fields took a route of their own, and only the
    /// decision at the end of it was covered.
    #[test]
    fn a_committed_endpoint_field_rebuilds_the_completer_for_the_new_host() {
        let edits = [
            (
                "base URL",
                SettingsPatch {
                    completer: CompleterPatch {
                        director_base_url: Some("https://api.x.ai".into()),
                        ..CompleterPatch::default()
                    },
                    ..SettingsPatch::default()
                },
                "https://api.x.ai/",
                "grok-4.6",
            ),
            (
                "model",
                SettingsPatch {
                    completer: CompleterPatch {
                        director_model: Some("gpt-5".into()),
                        ..CompleterPatch::default()
                    },
                    ..SettingsPatch::default()
                },
                "https://api.openai.com/",
                "gpt-5",
            ),
            (
                "API key",
                SettingsPatch {
                    completer: CompleterPatch {
                        director_api_key: Some("sk-typed-in-the-window".into()),
                        ..CompleterPatch::default()
                    },
                    ..SettingsPatch::default()
                },
                "https://api.openai.com/",
                "grok-4.6",
            ),
        ];

        model::tests::with_env(None, None, None, || {
            for (field, patch, host, model_name) in edits {
                let store = MemoryStore::new();
                store.set(DIRECTOR_API_KEY, "sk-stored-key").unwrap();
                let mut settings = Settings {
                    director_model: "grok-4.6".into(),
                    ..endpoint_settings()
                };
                assert!(
                    completer_retargets(&settings, &patch),
                    "{field} has to reach the running Director"
                );
                apply_with_store(&mut settings, &store, patch).unwrap();

                let SettingsOp::Retarget {
                    settings: director,
                    configured,
                    ..
                } = retarget_payload(&settings, &store).unwrap()
                else {
                    panic!("an edited endpoint must send Retarget");
                };

                // What frame_loop.rs does with the payload.
                let id = "fidget".to_string();
                let mut slots = completer::tests::slots_awaiting_a_wake(&id);
                let mut completer = None;
                completer::retarget_model(
                    &mut slots,
                    &id,
                    &mut completer,
                    ["stroll"],
                    "cat",
                    &director,
                    configured,
                );
                assert!(completer.is_some(), "{field} needs a Completer");
                // ADR-0008: a Wake already on the wire cannot propose against
                // the target that was just replaced.
                assert!(!slots.waiting(&id), "{field} must drop the open session");

                let endpoint =
                    model::endpoint_from(&director).expect("configured means a Completer");
                assert!(
                    endpoint.url().starts_with(host),
                    "{field}: the next wake has to reach {host}, not {}",
                    endpoint.url()
                );
                assert_eq!(endpoint.model(), model_name, "{field}: model");
                assert_eq!(
                    director.api_key,
                    if field == "API key" {
                        "sk-typed-in-the-window"
                    } else {
                        "sk-stored-key"
                    },
                    "{field}: key"
                );
            }
        });
    }

    /// #275 built the rebuild path. The two limits ride it because
    /// `model::endpoint_from` bakes them into the Endpoint at construction, so
    /// nothing else would carry a change to the Director already running.
    #[test]
    fn an_edited_limit_reaches_the_running_director() {
        let settings = Settings {
            director_timeout_secs: "20".into(),
            director_max_tokens: "80".into(),
            ..endpoint_settings()
        };

        for patch in [
            SettingsPatch {
                completer: CompleterPatch {
                    director_timeout_secs: Some("45".into()),
                    ..CompleterPatch::default()
                },
                ..SettingsPatch::default()
            },
            SettingsPatch {
                completer: CompleterPatch {
                    director_max_tokens: Some("300".into()),
                    ..CompleterPatch::default()
                },
                ..SettingsPatch::default()
            },
        ] {
            assert!(completer_retargets(&settings, &patch));
        }

        // Both windows commit every field on blur, so an unchanged value
        // arrives as `Some` and must not drop the open session.
        assert!(!completer_retargets(
            &settings,
            &SettingsPatch {
                completer: CompleterPatch {
                    director_timeout_secs: Some("20".into()),
                    director_max_tokens: Some("80".into()),
                    ..CompleterPatch::default()
                },
                ..SettingsPatch::default()
            }
        ));
    }

    /// The wake interval rides the rebuild for a reason of its own: it is
    /// `model::config_from` that reads it, and the frame loop re-paces every
    /// Instance from the config it rebuilds there (#262).
    #[test]
    fn an_edited_wake_interval_reaches_the_running_director() {
        let settings = Settings {
            director_wake_secs: "120".into(),
            ..endpoint_settings()
        };

        assert!(completer_retargets(
            &settings,
            &SettingsPatch {
                completer: CompleterPatch {
                    director_wake_secs: Some("300".into()),
                    ..CompleterPatch::default()
                },
                ..SettingsPatch::default()
            }
        ));
        assert!(!completer_retargets(
            &settings,
            &SettingsPatch {
                completer: CompleterPatch {
                    director_wake_secs: Some("120".into()),
                    ..CompleterPatch::default()
                },
                ..SettingsPatch::default()
            }
        ));
    }

    /// #447: a Development row is worth nothing unless the patch it writes
    /// reaches the file `dev_flags::seed` reads. The path is trimmed for the
    /// same reason the Harness command line is — a pasted line carries space.
    #[test]
    fn the_harness_knobs_round_trip_through_a_patch() {
        let mut patch = SettingsPatch::default();
        assert!(patch.set_text(TextField::HarnessTurnTimeoutSecs, "90"));
        assert!(patch.set_text(TextField::HarnessAuthRetrySecs, "5"));
        assert!(patch.set_text(TextField::McpBin, "  /tmp/fidget-mcp  "));
        assert!(patch.set_text(TextField::HarnessCwd, "  /tmp/project  "));

        let mut settings = Settings::default();
        settings.apply(patch);
        assert_eq!(settings.harness_turn_timeout_secs, "90");
        assert_eq!(settings.harness_auth_retry_secs, "5");
        assert_eq!(settings.mcp_bin, "/tmp/fidget-mcp");
        assert_eq!(settings.harness_cwd, "/tmp/project");
    }

    /// The row is the AI tab's, and the window fills it from the same
    /// keyed map the Development rows use, so a missing key draws blank over
    /// a value that is in force.
    #[test]
    fn the_view_shows_the_wake_interval_the_env_imposes() {
        model::tests::with_env(None, None, None, || {
            let file = Settings {
                director_wake_secs: "300".into(),
                ..Settings::default()
            };
            let view = endpoint_view(&file);
            assert_eq!(view.development_texts[form::DIRECTOR_WAKE_SECS_ID], "300");

            std::env::set_var(model::WAKE_SECS, "30");
            let view = endpoint_view(&file);
            assert_eq!(view.development_texts[form::DIRECTOR_WAKE_SECS_ID], "30");
            std::env::remove_var(model::WAKE_SECS);
        });
    }

    #[test]
    fn the_view_shows_the_working_directory_the_env_imposes() {
        model::tests::with_env(None, None, None, || {
            let file = Settings {
                harness_cwd: "/tmp/from-the-file".into(),
                ..Settings::default()
            };
            let view = endpoint_view(&file);
            assert_eq!(
                view.development_texts[form::HARNESS_CWD_ID],
                "/tmp/from-the-file"
            );

            std::env::set_var(crate::harness::CWD, "/tmp/from-the-env");
            let view = endpoint_view(&file);
            assert_eq!(
                view.development_texts[form::HARNESS_CWD_ID],
                "/tmp/from-the-env"
            );
            std::env::remove_var(crate::harness::CWD);
        });
    }

    /// The switch is only worth a checkbox if the read sites see it move
    /// without a relaunch, which means the patch has to reach `dev_flags`.
    #[test]
    fn a_patched_switch_moves_the_live_flag() {
        model::tests::with_env(None, None, None, || {
            let mut settings = Settings::default();
            let store = MemoryStore::new();

            apply_with_store(
                &mut settings,
                &store,
                SettingsPatch {
                    trace_director: Some(true),
                    ..SettingsPatch::default()
                },
            )
            .unwrap();
            assert!(settings.trace_director, "the file holds it");
            assert!(model::tracing(), "and the read site loads it");

            apply_with_store(
                &mut settings,
                &store,
                SettingsPatch {
                    trace_director: Some(false),
                    ..SettingsPatch::default()
                },
            )
            .unwrap();
            assert!(!model::tracing());

            // Each switch is its own static, so one of them moving proves
            // nothing about the next one being wired to anything (#273).
            apply_with_store(
                &mut settings,
                &store,
                SettingsPatch {
                    trace_engine: Some(true),
                    ..SettingsPatch::default()
                },
            )
            .unwrap();
            assert!(settings.trace_engine, "the file holds it");
            assert!(
                dev_flags::TRACE_ENGINE.is_on(),
                "and the frame loop loads it"
            );

            apply_with_store(
                &mut settings,
                &store,
                SettingsPatch {
                    trace_engine: Some(false),
                    ..SettingsPatch::default()
                },
            )
            .unwrap();
            assert!(!dev_flags::TRACE_ENGINE.is_on());
        });
    }

    #[test]
    fn a_store_set_error_leaves_settings_unchanged() {
        let mut settings = Settings::default();
        let err = apply_with_store(
            &mut settings,
            &FailingStore,
            SettingsPatch {
                completer: CompleterPatch {
                    director_base_url: Some("https://api.x.ai".into()),
                    director_api_key: Some("sk-new".into()),
                    ..CompleterPatch::default()
                },
                ..SettingsPatch::default()
            },
        );
        assert!(err.is_err());
        assert!(
            settings.director_base_url.is_empty(),
            "a failed key write must not leave a URL that was never saved"
        );
    }

    #[test]
    fn a_store_delete_error_leaves_settings_unchanged() {
        let mut settings = Settings::default();
        let err = apply_with_store(
            &mut settings,
            &FailingStore,
            SettingsPatch {
                completer: CompleterPatch {
                    director_base_url: Some("https://api.x.ai".into()),
                    director_api_key: Some(String::new()),
                    ..CompleterPatch::default()
                },
                ..SettingsPatch::default()
            },
        );
        assert!(err.is_err());
        assert!(
            settings.director_base_url.is_empty(),
            "a failed key delete must not leave a URL that was never saved"
        );
    }

    #[test]
    fn a_bad_hotkey_is_refused_rather_than_half_applied() {
        assert_eq!(
            parse_hotkey("B"),
            Some(Hotkey {
                control: false,
                option: false,
                shift: false,
                command: false,
                key: 'B',
            })
        );
        assert_eq!(parse_hotkey("Control-Option-Command"), None);
        assert_eq!(parse_hotkey("Control-F1"), None);
        assert_eq!(parse_hotkey("Control-B-C"), None);
        assert_eq!(parse_hotkey(""), None);
    }

    /// One chord in four spellings, because the file keeps whichever of them
    /// the user last named.
    #[test]
    fn one_chord_parses_the_same_from_every_platforms_words() {
        let mac = parse_hotkey("Control-Option-Command-B").expect("mac words");
        assert_eq!(parse_hotkey("Ctrl-Alt-Super-B"), Some(mac.clone()));
        assert_eq!(parse_hotkey("Ctrl-Alt-Win-B"), Some(mac.clone()));
        assert_eq!(parse_hotkey("Ctrl-Alt-Meta-B"), Some(mac));
    }

    /// #194: the menu used to print the stored Mac spelling everywhere, which
    /// names keys a Linux or Windows keyboard does not have.
    #[test]
    fn the_shipped_default_reads_as_each_platforms_own_chord() {
        let default = parse_hotkey(DEFAULT_HIDE_HOTKEY).expect("default parses");
        for (words, expected) in [
            (ModifierWords::Mac, "Control-Option-Command-B"),
            (ModifierWords::Linux, "Ctrl-Alt-Super-B"),
            (ModifierWords::Windows, "Ctrl-Alt-Win-B"),
        ] {
            assert_eq!(default.display(words), expected);
        }
        for words in [ModifierWords::Linux, ModifierWords::Windows] {
            let printed = default.display(words);
            assert!(!printed.contains("Option"), "{printed} names a Mac key");
            assert!(!printed.contains("Command"), "{printed} names a Mac key");
        }
    }

    /// What `display` prints is also what the hotkey field accepts back, or a
    /// user who retypes what the settings window shows them loses the binding.
    #[test]
    fn every_platforms_words_parse_back_to_the_chord_they_printed() {
        let chord = Hotkey {
            control: true,
            option: true,
            shift: true,
            command: true,
            key: 'B',
        };
        for words in [
            ModifierWords::Mac,
            ModifierWords::Linux,
            ModifierWords::Windows,
        ] {
            let printed = chord.display(words);
            assert_eq!(
                parse_hotkey(&printed),
                Some(chord.clone()),
                "{printed} did not parse back"
            );
        }
    }

    /// The window renders the view verbatim, so the view is where the stored
    /// spec becomes this machine's words — whichever words the file used.
    #[test]
    fn the_view_shows_the_hotkey_in_this_machines_words() {
        let settings = Settings {
            hide_hotkey: "Ctrl-Alt-Super-B".to_string(),
            ..Settings::default()
        };
        let view = SettingsView::from_parts(
            &settings,
            Path::new("/tmp/memory.md"),
            None,
            Vec::new(),
            Vec::new(),
            (false, String::new(), String::new()),
            None,
        );
        let expected = parse_hotkey("Ctrl-Alt-Super-B")
            .expect("stored spec parses")
            .display(ModifierWords::current());
        assert_eq!(view.hide_hotkey, expected);
    }

    /// A file nobody can parse must still name the chord the shell registers.
    #[test]
    fn an_unreadable_spec_is_shown_as_the_default_the_shell_binds() {
        assert_eq!(
            display_hotkey("Control-F1"),
            parse_hotkey(DEFAULT_HIDE_HOTKEY)
                .expect("default parses")
                .display(ModifierWords::current())
        );
    }

    #[test]
    fn a_chord_with_no_modifiers_prints_just_the_letter() {
        let printed = parse_hotkey("H")
            .expect("letter")
            .display(ModifierWords::Mac);
        assert_eq!(printed, "H");
    }

    /// The row is a picker over one string, and the string is the grammar
    /// `FIDGET_HARNESS` already takes — so a hand-edit of the file and an
    /// export mean the same thing.
    #[test]
    fn the_completer_source_maps_a_title_to_what_launch_reads() {
        let cases = [
            (form::HARNESS_OFF, "", None),
            ("Harness · hermes", "hermes", Some("hermes")),
            ("Harness · claude", "claude", Some("claude")),
            (form::HARNESS_CUSTOM, "custom", Some("opencode acp")),
        ];
        for (title, stored, source) in cases {
            let mut settings = Settings {
                harness_command: "opencode acp".to_string(),
                ..Settings::default()
            };
            let mut patch = SettingsPatch::default();
            assert!(patch.set_text(TextField::Harness, title));
            settings.apply(patch);

            assert_eq!(settings.harness, stored, "{title} is stored as {stored:?}");
            assert_eq!(
                settings.harness_source().as_deref(),
                source,
                "{title} names the Harness to launch"
            );
        }
    }

    /// Custom with nothing typed is Off, not a spawn of nothing: `launch`
    /// would index `argv[0]` on an empty command line.
    #[test]
    fn a_custom_source_with_no_command_line_is_off() {
        let settings = Settings {
            harness: form::HARNESS_CUSTOM_VALUE.to_string(),
            harness_command: "   ".to_string(),
            ..Settings::default()
        };
        assert_eq!(settings.harness_source(), None);
        assert!(crate::harness::launch(settings.harness_source().as_deref()).is_none());
    }

    /// What the user picked is what the app spawns — the whole of the row, and
    /// the half `FIDGET_HARNESS` alone could not survive (#436).
    #[test]
    fn the_completer_source_round_trips_through_the_file() {
        crate::model::tests::with_harness(None, || {
            let path = temp_path();
            let _ = fs::remove_file(&path);

            let mut settings = Settings::default();
            let mut patch = SettingsPatch::default();
            patch.set_text(TextField::Harness, form::HARNESS_CUSTOM);
            patch.set_text(TextField::HarnessCommand, "  grok agent stdio  ");
            settings.apply(patch);
            settings.save(&path).expect("save");

            let read = Settings::load(&path);
            assert_eq!(read.harness, form::HARNESS_CUSTOM_VALUE);
            assert_eq!(read.harness_command, "grok agent stdio");
            let launch = crate::harness::from_settings(read.harness_source().as_deref())
                .expect("the saved row names a Harness");
            assert_eq!(launch.argv, ["grok", "agent", "stdio"]);

            // The typed command line outlives a swing through a preset, so
            // coming back to Custom does not ask for it again.
            let mut patch = SettingsPatch::default();
            patch.set_text(TextField::Harness, "Harness · hermes");
            let mut read = read;
            read.apply(patch);
            assert_eq!(read.harness_source().as_deref(), Some("hermes"));
            assert_eq!(read.harness_command, "grok agent stdio");

            let _ = fs::remove_file(&path);
        });
    }

    /// #272's rule on the source row: the variable wins at launch, so the
    /// window prints the variable's Harness and the frozen row takes no edit.
    #[test]
    fn an_exported_harness_is_what_the_window_shows_and_what_launches() {
        let saved = Settings {
            harness: "hermes".to_string(),
            harness_command: "grok agent stdio".to_string(),
            ..Settings::default()
        };
        crate::model::tests::with_harness(Some("opencode acp"), || {
            let view = endpoint_view(&saved);
            assert_eq!(view.harness, form::HARNESS_CUSTOM);
            assert_eq!(
                view.development_texts
                    .get(form::HARNESS_COMMAND_ID)
                    .map(String::as_str),
                Some("opencode acp"),
                "the command line in force is the variable's, not the file's"
            );
            let launch = crate::harness::from_settings(saved.harness_source().as_deref())
                .expect("the variable names a Harness");
            assert_eq!(launch.argv, ["opencode", "acp"]);
            assert!(
                form::describe().frozen(form::HARNESS_ID),
                "a row the variable owns takes no edit"
            );
        });
        crate::model::tests::with_harness(None, || {
            let view = endpoint_view(&saved);
            assert_eq!(view.harness, "Harness · hermes");
            assert_eq!(
                view.development_texts
                    .get(form::HARNESS_COMMAND_ID)
                    .map(String::as_str),
                Some("grok agent stdio")
            );
            let launch = crate::harness::from_settings(saved.harness_source().as_deref())
                .expect("the file names a Harness");
            assert_eq!(launch.argv, ["hermes", "acp"]);
        });
    }

    /// Not attached, attached with a session, or attached but not signed in.
    /// The login command is named for the user's own terminal and nothing
    /// here runs it.
    #[test]
    fn the_source_row_reads_the_three_attachment_states() {
        assert!(harness_state(None).contains("Not attached"));

        let attached = crate::harness::HarnessInspect {
            name: "hermes".to_string(),
            command: "hermes acp".to_string(),
            session_id: Some("sess-7".to_string()),
            alive: true,
            ..Default::default()
        };
        let line = harness_state(Some(&attached));
        assert!(line.contains("hermes attached"), "got {line:?}");
        assert!(line.contains("sess-7"), "got {line:?}");

        let unauthenticated = crate::harness::HarnessInspect {
            name: "claude".to_string(),
            command: "npx -y @agentclientprotocol/claude-agent-acp".to_string(),
            login: Some("claude /login".to_string()),
            alive: true,
            ..Default::default()
        };
        let line = harness_state(Some(&unauthenticated));
        assert!(line.contains("not authenticated"), "got {line:?}");
        assert!(line.contains("`claude /login`"), "got {line:?}");
        assert!(
            !line.to_lowercase().contains("api key") && !line.to_lowercase().contains("password"),
            "a credential is never asked for, got {line:?}"
        );
    }

    /// Production change that would fail this: a different Harness left to the
    /// next launch, which is what #436 shipped and #500 undid. `retarget` has
    /// swapped the handle by the time the payload is built, so a Retarget that
    /// does not follow leaves the Director on the Session it just shut down.
    #[test]
    fn a_different_harness_retargets_the_running_director() {
        crate::model::tests::with_harness(None, || {
            let settings = Settings {
                harness: "hermes".into(),
                ..Settings::default()
            };
            let mut patch = SettingsPatch::default();
            patch.set_text(TextField::Harness, "Harness · opencode");
            assert!(harness_retargets(&settings, &patch));
            assert!(completer_retargets(&settings, &patch));
        });
    }

    /// Two rows can name one Harness: the `hermes` preset and a custom
    /// `hermes acp` join to the same `Launch`. Retargeting on that would kill
    /// a child and open an identical one, throwing away the session with it.
    #[test]
    fn a_row_that_names_the_attached_harness_another_way_does_not_retarget() {
        crate::model::tests::with_harness(None, || {
            let settings = Settings {
                harness: "hermes".into(),
                ..Settings::default()
            };
            let mut patch = SettingsPatch::default();
            patch.set_text(TextField::Harness, form::HARNESS_CUSTOM);
            patch.set_text(TextField::HarnessCommand, "hermes acp");
            assert!(harness_source_changed(&settings, &patch), "the row moved");
            assert!(
                !harness_retargets(&settings, &patch),
                "the Harness behind it did not"
            );
        });
    }

    #[test]
    fn a_cwd_that_resolves_differently_retargets_and_empty_vs_data_dir_stands() {
        crate::model::tests::with_harness(None, || {
            let settings = Settings {
                harness: "hermes".into(),
                ..Settings::default()
            };
            let other_dir = std::env::temp_dir().join("other-project");
            let mut other = SettingsPatch::default();
            other.set_text(TextField::HarnessCwd, &other_dir.to_string_lossy());
            assert!(harness_retargets(&settings, &other));

            let mut same_empty = SettingsPatch::default();
            same_empty.set_text(TextField::HarnessCwd, "");
            assert!(
                !harness_retargets(&settings, &same_empty),
                "the raw row did not move"
            );

            let data = fidget_core::memory::data_dir();
            let mut data_patch = SettingsPatch::default();
            data_patch.set_text(TextField::HarnessCwd, &data.to_string_lossy());
            assert!(
                !harness_retargets(&settings, &data_patch),
                "empty and an explicit data_dir resolve equal"
            );

            let home = fidget_core::memory::home_dir().expect("the test user has a home");
            let mut home_patch = SettingsPatch::default();
            home_patch.set_text(TextField::HarnessCwd, &home.to_string_lossy());
            assert!(
                harness_retargets(&settings, &home_patch),
                "explicit home is a different project from empty"
            );
        });
    }

    /// Production change that would fail this: picking Off leaves `attached()`
    /// in place, so `completer_from` keeps handing the Director a Harness that
    /// no longer matches the row, and the HTTP Completer the user just chose
    /// never sees a wake. Off is the user naming that Completer, not a dead
    /// session falling through (ADR-0008, #500).
    #[test]
    fn turning_the_harness_off_retargets_to_the_http_completer() {
        crate::model::tests::with_harness(None, || {
            let settings = Settings {
                harness: "claude".into(),
                ..Settings::default()
            };
            let mut patch = SettingsPatch::default();
            patch.set_text(TextField::Harness, form::HARNESS_OFF);
            assert!(
                completer_retargets(&settings, &patch),
                "Off has to reach the running Director"
            );
        });
    }

    /// `FIDGET_HARNESS` owns the row. Clearing the file cannot drop a
    /// Harness the export is still spawning.
    #[test]
    fn an_exported_harness_does_not_retarget_when_the_row_says_off() {
        crate::model::tests::with_harness(Some("claude"), || {
            let settings = Settings {
                harness: "claude".into(),
                ..Settings::default()
            };
            let mut patch = SettingsPatch::default();
            patch.set_text(TextField::Harness, form::HARNESS_OFF);
            assert!(!completer_retargets(&settings, &patch));
        });
    }

    /// Production change that would fail this: a Director-off patch that does
    /// not tell an already-open Chat surface. Payload is `chat_opening_from`
    /// with inspect after `apply_switch`. #473.
    #[test]
    fn a_director_off_patch_tells_an_open_chat_surface_it_is_disabled() {
        let settings = Settings::default();
        assert!(settings.director_enabled);
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_enabled: Some(false),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(
            chat_surface_reloads(&settings, &patch),
            "an already-open surface must hear the new opening"
        );
    }

    /// Production change that would fail this: Director on again that does
    /// not tell an already-open Chat surface. #473.
    #[test]
    fn a_director_on_patch_readies_the_composer_when_configured() {
        let settings = Settings {
            director_enabled: false,
            ..Settings::default()
        };
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_enabled: Some(true),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(chat_surface_reloads(&settings, &patch));
    }

    #[test]
    fn an_unchanged_director_switch_does_not_reload_chat() {
        let settings = Settings::default();
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_enabled: Some(true),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(!chat_surface_reloads(&settings, &patch));
    }

    /// #679: Start a new session is a session boundary and nothing else. A
    /// patch that moved a row would be the very thing the button exists to
    /// avoid — the only "start over" today is a setting the user did not want
    /// to change.
    #[test]
    fn a_new_session_patch_changes_no_setting() {
        let settings = Settings::default();
        let patch = SettingsPatch {
            new_session: true,
            ..SettingsPatch::default()
        };
        let mut applied = settings.clone();
        applied.apply(patch.clone());
        assert_eq!(applied, settings, "a session boundary is not a file field");
        assert!(
            !completer_retargets(&settings, &patch),
            "the Completer, the model, and the key stay what they are"
        );
        assert!(
            !harness_retargets(&settings, &patch),
            "the attached Harness child is not restarted"
        );
    }

    /// #679: the new session must reach an already-open Chat surface the way
    /// a Completer-source change does, or the window keeps a transcript the
    /// thing about to answer has never read.
    #[test]
    fn a_new_session_patch_tells_an_open_chat_surface() {
        let patch = SettingsPatch {
            new_session: true,
            ..SettingsPatch::default()
        };
        assert!(chat_surface_reloads(&Settings::default(), &patch));
    }

    #[test]
    fn a_sound_patch_does_not_reload_chat() {
        let patch = SettingsPatch {
            sound: Some(false),
            ..SettingsPatch::default()
        };
        assert!(!chat_surface_reloads(&Settings::default(), &patch));
    }

    /// The Chat header names the model and the host, so an endpoint edit has
    /// to reach an open window. Production change that would fail this: a
    /// `chat_surface_reloads` that watches only the switch and the Harness
    /// source, which is what it did before #474 — the header would keep
    /// naming the endpoint the user just left.
    #[test]
    fn a_new_endpoint_reloads_chat() {
        let settings = Settings::default();
        let patch = SettingsPatch {
            completer: CompleterPatch {
                director_base_url: Some("http://localhost:11434".to_string()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(chat_surface_reloads(&settings, &patch));

        let unchanged = SettingsPatch {
            completer: CompleterPatch {
                director_base_url: Some(settings.director_base_url.clone()),
                ..CompleterPatch::default()
            },
            ..SettingsPatch::default()
        };
        assert!(
            !chat_surface_reloads(&settings, &unchanged),
            "both windows commit on blur, so an untouched row is not a change"
        );
    }

    /// Completer source still reloads Chat even when Off also Retargets,
    /// so the surface hears `configured` after the handle is dropped. #473.
    #[test]
    fn a_harness_source_patch_tells_an_open_chat_surface_the_inspect_mode() {
        let settings = Settings {
            harness: "hermes".into(),
            ..Settings::default()
        };
        let mut patch = SettingsPatch::default();
        patch.set_text(TextField::Harness, form::HARNESS_OFF);
        assert!(
            chat_surface_reloads(&settings, &patch),
            "an already-open surface must hear the opening"
        );
    }

    /// The bug that mattered most: a source row naming a Harness this machine
    /// has not got leaves the handle in place, and freezing the HTTP rows on
    /// that presence would leave no reachable Completer at all (#452).
    #[test]
    fn a_harness_that_never_started_leaves_the_http_rows_live() {
        let configured = crate::harness::HarnessInspect {
            name: "claude".to_string(),
            command: "npx -y @agentclientprotocol/claude-agent-acp".to_string(),
            alive: false,
            ..Default::default()
        };
        let line = harness_state(Some(&configured));
        assert!(
            line.contains("not running"),
            "a handle that never answered must not read as attached, got {line:?}"
        );
        assert!(
            line.contains("static weights"),
            "the line has to name what is answering instead, got {line:?}"
        );
    }

    /// #949: Apply attaches on the spot, and the ACP handshake takes a second
    /// or two after it. The line the user reads in that second used to be the
    /// one for a Harness that is set and never coming up, which is why Apply
    /// read as having done nothing.
    #[test]
    fn a_harness_mid_handshake_says_it_is_starting() {
        let starting = crate::harness::HarnessInspect {
            name: "hermes".to_string(),
            command: "hermes acp".to_string(),
            alive: false,
            initializing: true,
            ..Default::default()
        };
        let line = harness_state(Some(&starting));
        assert!(
            line.contains("is starting"),
            "the wait has to read as a wait, got {line:?}"
        );
        assert!(
            !line.contains("not running"),
            "attach is under way, so nothing here says it is not, got {line:?}"
        );
    }

    /// The missing launcher outranks the wait: `initializing` is cleared by the
    /// same failure that sets `missing`, and a line that raced them would tell
    /// the user to wait for a CLI this machine has not got.
    #[test]
    fn a_missing_launcher_outranks_a_handshake_in_flight() {
        let missing = crate::harness::HarnessInspect {
            name: "hermes".to_string(),
            command: "hermes acp".to_string(),
            alive: false,
            initializing: true,
            missing: Some("hermes".to_string()),
            ..Default::default()
        };
        let line = harness_state(Some(&missing));
        assert!(line.contains("not installed"), "got {line:?}");
        assert!(!line.contains("is starting"), "got {line:?}");
    }

    /// #659: the machine has not got the CLI, which is not the same state as a
    /// child that stopped answering. The line has to say so in words a user can
    /// act on - an errno is not one of them - and name the binary that was
    /// looked for, since fidget bundles no Harness (ADR-0018).
    #[test]
    fn a_harness_this_machine_has_not_got_names_the_command_not_an_errno() {
        let missing = crate::harness::HarnessInspect {
            name: "codex".to_string(),
            command: "npx -y @agentclientprotocol/codex-acp@latest".to_string(),
            missing: Some("npx".to_string()),
            alive: false,
            ..Default::default()
        };
        let line = harness_state(Some(&missing));
        assert!(line.contains("not installed"), "got {line:?}");
        assert!(line.contains("`npx`"), "got {line:?}");
        assert!(line.contains("does not bundle `npx`"), "got {line:?}");
        assert!(
            line.contains("install it from https://nodejs.org/,"),
            "Settings names the page Chat names, got {line:?}"
        );
        assert!(
            !line.contains("bundle a Harness"),
            "should name the command, not 'a Harness', got {line:?}"
        );
        assert!(
            !line.contains("os error") && !line.to_lowercase().contains("no such file"),
            "an errno is not a sentence, got {line:?}"
        );
        assert!(
            line.contains("switch AI source to Model API"),
            "the line has to name the AI source pick that hands control back, got {line:?}"
        );
        assert!(
            line.contains("an HTTP endpoint"),
            "the line has to name Model API as the HTTP path, got {line:?}"
        );
        assert!(
            !line.contains("HTTP endpoint below"),
            "Settings already places the HTTP rows; no 'below' deixis, got {line:?}"
        );
    }

    /// A launcher that ran and died says how, not only that nothing runs.
    #[test]
    fn a_launcher_that_died_at_startup_says_why() {
        let died = crate::harness::HarnessInspect {
            name: "codex".to_string(),
            failed: Some(crate::harness::LaunchFailure {
                command: Some("npx".to_string()),
                reason: "exited before initialize, signal: 6 (SIGABRT)".to_string(),
                output: "dyld[0]: Library not loaded".to_string(),
                node_check: None,
            }),
            ..Default::default()
        };
        assert_eq!(
            harness_state(Some(&died)),
            "codex failed to start: `npx` exited before initialize, signal: 6 (SIGABRT). The fidget \
             runs on static weights until it answers."
        );
    }

    #[test]
    fn an_unhealthy_launcher_says_why_without_endpoint_jargon() {
        let sick = crate::harness::HarnessInspect {
            name: "codex".to_string(),
            unhealthy: Some(crate::harness::LaunchFailure {
                command: Some("npx --version".to_string()),
                reason: "timed out after 3.0s".to_string(),
                output: String::new(),
                node_check: Some("node --version".to_string()),
            }),
            ..Default::default()
        };
        assert_eq!(
            harness_state(Some(&sick)),
            "codex is unhealthy: `npx --version` timed out after 3.0s. `npx` runs on Node.js: run \
             `node --version` in a terminal to check that it starts. The fidget runs on static \
             weights until it is fixed."
        );
    }

    #[test]
    fn a_missing_first_party_cli_names_that_binary_not_npx() {
        let missing = crate::harness::HarnessInspect {
            name: "cursor-agent".to_string(),
            command: "cursor-agent".to_string(),
            missing: Some("cursor-agent".to_string()),
            alive: false,
            ..Default::default()
        };
        let line = harness_state(Some(&missing));
        assert!(line.contains("not installed"), "got {line:?}");
        assert!(line.contains("`cursor-agent`"), "got {line:?}");
        assert!(
            line.contains("does not bundle `cursor-agent`"),
            "got {line:?}"
        );
        assert!(
            !line.contains("`npx`"),
            "should not mention npx, got {line:?}"
        );
        assert!(
            !line.contains("bundle a Harness"),
            "should name the command, not 'a Harness', got {line:?}"
        );
    }

    /// #469: the handle stays authoritative while it is set, so the rows #452
    /// kept live are a way back and not a live switch — handing a dead session
    /// to the HTTP Completer mid-run is the second mind ADR-0008 refuses.
    ///
    /// #500: what ends the wait is a pick, not a relaunch. The line is where
    /// the user learns which, so it names Model API in AI source and never a launch.
    #[test]
    fn a_dead_harness_names_model_api_as_the_way_back() {
        let dead = crate::harness::HarnessInspect {
            name: "claude".to_string(),
            command: "npx -y @agentclientprotocol/claude-agent-acp".to_string(),
            alive: false,
            ..Default::default()
        };
        let line = harness_state(Some(&dead));
        assert!(
            !line.contains("next launch"),
            "nothing waits for one any more, got {line:?}"
        );
        assert!(
            line.contains("you may switch AI source at any time"),
            "the line has to invite switching AI source without Model API jargon, got {line:?}"
        );
        assert!(
            line.contains("Apply takes effect at once"),
            "the line has to say Apply is what commits the pick, got {line:?}"
        );
        assert!(
            !line.contains("HTTP endpoint"),
            "dead-handle line should not point at HTTP rows, got {line:?}"
        );
        assert!(
            !line.contains("Model API"),
            "dead-handle line should not name Model API; the AI source row is enough, got {line:?}"
        );
        assert!(
            !line.contains("is the AI brain"),
            "the dead handle is still the AI brain; the HTTP rows are not, got {line:?}"
        );
        assert!(
            !line.contains("hands the HTTP endpoint back"),
            "no harness-internal jargon, got {line:?}"
        );
    }

    /// `FIDGET_HARNESS=` has been the kill switch since #433. Going through
    /// `env_override` dropped it, and the saved row spawned instead (#452).
    #[test]
    fn an_exported_empty_harness_is_off_whatever_the_row_saved() {
        let saved = Settings {
            harness: "hermes".to_string(),
            ..Settings::default()
        };
        crate::model::tests::with_harness(Some(""), || {
            assert!(
                crate::harness::from_settings(saved.harness_source().as_deref()).is_none(),
                "an exported blank is Off, not a fall-through"
            );
            let view = endpoint_view(&saved);
            assert_eq!(view.harness, form::HARNESS_OFF);
            assert!(
                form::describe().frozen(form::HARNESS_ID),
                "the variable owns the row it is silencing"
            );
        });
        crate::model::tests::with_harness(None, || {
            assert_eq!(
                crate::harness::from_settings(saved.harness_source().as_deref())
                    .map(|launch| launch.name),
                Some("hermes".to_string()),
                "unexported still falls through to the row"
            );
        });
    }

    /// A command line sitting in the source field is drawn in the row that
    /// writes the *other* field, so picking the Custom entry already on screen
    /// used to leave the command empty and the source Off (#452).
    #[test]
    fn picking_the_custom_entry_already_shown_keeps_the_command_line() {
        crate::model::tests::with_harness(None, || {
            // What a hand-edit of the file, or an older export, leaves behind.
            let mut settings = Settings {
                harness: "grok agent stdio".to_string(),
                ..Settings::default()
            };
            let view = endpoint_view(&settings);
            assert_eq!(view.harness, form::HARNESS_CUSTOM);
            assert_eq!(
                view.development_texts
                    .get(form::HARNESS_COMMAND_ID)
                    .map(String::as_str),
                Some("grok agent stdio")
            );

            let mut patch = SettingsPatch::default();
            patch.set_text(TextField::Harness, form::HARNESS_CUSTOM);
            settings.apply(patch);

            assert_eq!(settings.harness_command, "grok agent stdio");
            assert_eq!(
                settings.harness_source().as_deref(),
                Some("grok agent stdio"),
                "the pick must not be a silent Off"
            );

            // And a renderer asks before it commits at all, so the click on
            // the entry already selected writes nothing.
            assert_eq!(
                view.popup_value(form::HARNESS_ID).as_deref(),
                Some(form::HARNESS_CUSTOM)
            );
            assert_eq!(view.popup_value(form::CHARACTER_ID), Some(String::new()));
            assert_eq!(
                view.popup_value("chat_appearance").as_deref(),
                Some("System")
            );
            assert_eq!(view.popup_value("nothing_like_it"), None);
        });
    }

    /// A Harness credential is not logged or fingerprinted. A flag on a
    /// custom command line is somewhere a token can sit, and the key beside
    /// it is already fingerprinted (#452).
    #[test]
    fn a_custom_command_line_is_not_logged_verbatim() {
        let mut patch = SettingsPatch::default();
        patch.set_text(
            TextField::HarnessCommand,
            "my-agent acp --token sk-live-abcdef",
        );
        let printed = format!("{patch:?}");
        assert!(
            !printed.contains("sk-live-abcdef"),
            "a token on the command line reached the log: {printed}"
        );
        assert!(
            printed.contains("my-agent"),
            "the program name is still worth reading: {printed}"
        );
        assert_eq!(command_line_debug("my-agent acp"), "my-agent +1 arg(s)");
        assert_eq!(command_line_debug("   "), "");
    }

    /// Every registration shape #580 verified against the installed CLI, and
    /// the one precondition a user cannot see: Claude Code answers a re-add of
    /// an existing name by keeping the old URL and token and exiting 0, so a
    /// snippet without the remove installs a stale entry that cannot connect.
    ///
    /// After Architect redesign: Codex exports the raw token, Hermes shows token
    /// in steps only (interactive paste). Every harness carries the URL in snippet
    /// or steps as appropriate.
    #[test]
    fn every_byo_snippet_carries_the_url_and_the_raw_token() {
        for harness in form::HARNESS_PRESETS {
            let (snippet, steps, byo_token) =
                byo_registration(harness, "http://127.0.0.1:5051/mcp", "beef");

            // Every harness must carry the URL somewhere
            assert!(
                snippet.contains("http://127.0.0.1:5051/mcp")
                    || steps.contains("http://127.0.0.1:5051/mcp"),
                "{harness} must carry the URL in snippet or steps, got snippet: {snippet:?}, steps: {steps:?}"
            );

            // Check token presence based on harness type
            match harness {
                "codex" => {
                    // Codex: raw token in export, no Bearer in snippet
                    assert!(
                        snippet.contains("FIDGET_MCP_TOKEN='beef'"),
                        "codex must export raw token in FIDGET_MCP_TOKEN, got {snippet:?}"
                    );
                    assert!(
                        !snippet.contains("Bearer beef"),
                        "codex snippet should not contain Bearer+token directly"
                    );
                }
                "hermes" => {
                    // Hermes: snippet has --auth header only, token in separate byo_token field
                    assert!(
                        snippet.contains("--auth header"),
                        "hermes snippet must have --auth header, got {snippet:?}"
                    );
                    assert_eq!(
                        byo_token, "beef",
                        "hermes must show raw token via byo_token, got {byo_token:?}"
                    );
                    assert!(
                        !snippet.contains("beef"),
                        "hermes snippet should not embed the token, got {snippet:?}"
                    );
                }
                "goose" | "antigravity" => {
                    // No vendor registration command is verified. The catch-all
                    // still hands the URL and the raw token, and the steps name
                    // the Bearer header.
                    assert!(
                        snippet.contains("http://127.0.0.1:5051/mcp") && snippet.contains("beef"),
                        "{harness} falls through to the URL and token pair, got {snippet:?}"
                    );
                    assert!(
                        steps.contains("Bearer"),
                        "{harness} steps still name the Bearer header, got {steps:?}"
                    );
                }
                _ => {
                    // Claude, Copilot, Cursor, Grok, OpenCode, Pi: snippet
                    // still has Bearer+token
                    assert!(
                        snippet.contains("Bearer beef"),
                        "{harness} must carry Bearer and the token in snippet, got {snippet:?}"
                    );
                    assert!(
                        !snippet.contains("Bearer Bearer"),
                        "{harness} double-prefixed the token"
                    );
                }
            }

            assert!(!steps.is_empty(), "{harness} needs its instructions");
        }
    }

    #[test]
    fn the_claude_snippet_removes_before_it_adds() {
        let (snippet, _, _) = byo_registration("claude", "http://127.0.0.1:5051/mcp", "beef");
        let remove = snippet
            .find("claude mcp remove")
            .expect("the remove line is what makes the snippet re-runnable");
        let add = snippet.find("claude mcp add").expect("the add line");
        assert!(remove < add, "the remove must come first, got {snippet:?}");
        assert!(
            !snippet.contains("-s project"),
            "project scope writes a checked-in .mcp.json and would commit the token"
        );
        assert!(
            !snippet.contains("-s user"),
            "omit scope to use local default, got {snippet:?}"
        );
    }

    /// Pi pastes into a file that may already exist, so its box holds a
    /// fragment and the words beside it have to say so. It should not say it
    /// "starts" or "launches" the harness.
    #[test]
    fn the_file_fragment_harnesses_say_they_are_fragments() {
        let harness = "pi";
        let file = ".mcp.json";
        let (snippet, steps, _token) =
            byo_registration(harness, "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            steps.contains(file),
            "{harness} must name the file it merges into, got {steps:?}"
        );
        assert!(
            steps.contains("fragment")
                || steps.to_lowercase().contains("merge")
                || steps.to_lowercase().contains("add or update"),
            "{harness} must say it is a fragment, got {steps:?}"
        );
        let lower_steps = steps.to_lowercase();
        let lower_snippet = snippet.to_lowercase();
        assert!(
            !lower_steps.contains("starts ")
                && !lower_steps.contains(" start ")
                && !lower_steps.starts_with("start ")
                && !lower_snippet.contains(" start ")
                && !lower_snippet.starts_with("start ")
                && !lower_steps.contains("launch"),
            "{harness} must not say it starts/launches the harness, got snippet: {snippet:?}, steps: {steps:?}"
        );
    }

    /// Codex uses `http_headers` not `headers`. Architect verified `headers`
    /// is silently ignored (#599).
    #[test]
    fn codex_snippet_is_cli_with_export_and_bearer_token_env_var() {
        let (snippet, _, _) = byo_registration("codex", "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            snippet.contains("export FIDGET_MCP_TOKEN="),
            "codex must export the token env var, got {snippet:?}"
        );
        assert!(
            snippet.contains("codex mcp add"),
            "codex must use CLI add, got {snippet:?}"
        );
        assert!(
            snippet.contains("--url"),
            "codex must use --url flag, got {snippet:?}"
        );
        assert!(
            snippet.contains("--bearer-token-env-var FIDGET_MCP_TOKEN"),
            "codex must use --bearer-token-env-var, got {snippet:?}"
        );
        assert!(
            !snippet.starts_with("[mcp_servers"),
            "codex must not start with TOML fragment, got {snippet:?}"
        );
    }

    /// OpenCode uses flat mcp.fidget with type: remote and oauth: false,
    /// not nested mcp.servers.<name> with type: http (#599).
    #[test]
    fn opencode_snippet_is_cli_with_url_and_header() {
        let (snippet, _, _) = byo_registration("opencode", "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            snippet.contains("opencode mcp add"),
            "opencode must use CLI add, got {snippet:?}"
        );
        assert!(
            snippet.contains("--url"),
            "opencode must use --url flag, got {snippet:?}"
        );
        assert!(
            snippet.contains("--header \"Authorization=Bearer"),
            "opencode must use --header with Authorization=Bearer, got {snippet:?}"
        );
        assert!(
            !snippet.starts_with("{"),
            "opencode must not start with JSON fragment, got {snippet:?}"
        );
    }

    /// Hermes uses CLI with --auth header and interactive token paste (#599).
    #[test]
    fn hermes_snippet_is_cli_with_auth_header() {
        let (snippet, steps, _token) =
            byo_registration("hermes", "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            snippet.contains("hermes mcp add"),
            "hermes must use CLI add, got {snippet:?}"
        );
        assert!(
            snippet.contains("--url"),
            "hermes must use --url flag, got {snippet:?}"
        );
        assert!(
            snippet.contains("--auth header"),
            "hermes must use --auth header, got {snippet:?}"
        );
        assert!(
            !snippet.starts_with("mcp_servers:"),
            "hermes must not start with YAML fragment, got {snippet:?}"
        );
        assert!(
            steps.contains("paste the raw token"),
            "hermes instructions must mention pasting the raw token, got {steps:?}"
        );
        assert!(
            steps.contains("no `Bearer` prefix"),
            "hermes instructions must mention no Bearer prefix, got {steps:?}"
        );
    }

    /// A harness name the popup cannot offer - blank, `custom`, or a
    /// hand-edited file - still gets the pair, because that is all any of the
    /// eight templates is made of.
    #[test]
    fn an_unknown_harness_still_gets_the_url_and_the_token() {
        let (snippet, steps, _token) =
            byo_registration("custom", "http://127.0.0.1:5051/mcp", "beef");
        assert!(snippet.contains("http://127.0.0.1:5051/mcp"));
        assert!(snippet.contains("beef"));
        assert!(!steps.is_empty());
    }

    /// Hermes shows the raw token separately for interactive paste. Others
    /// embed it in the snippet and return empty token.
    #[test]
    fn hermes_returns_separate_token_others_do_not() {
        let (_, _, token) = byo_registration("hermes", "http://127.0.0.1:5051/mcp", "beef");
        assert_eq!(token, "beef", "hermes must return the raw token separately");

        for harness in form::HARNESS_PRESETS
            .iter()
            .filter(|name| **name != "hermes")
        {
            let (_, _, token) = byo_registration(harness, "http://127.0.0.1:5051/mcp", "beef");
            assert!(
                token.is_empty(),
                "{harness} must return empty token (embeds in snippet)"
            );
        }
    }

    /// OpenCode does not re-read MCP mid-session; `/reload` is not shipped
    /// (#6719, #751). After-steps must say restart OpenCode, not run `/reload`.
    #[test]
    fn opencode_byo_after_steps_say_restart_not_reload() {
        let (_, steps, _) = byo_registration("opencode", "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            !steps.contains("/reload"),
            "opencode steps must not mention /reload, got {steps:?}"
        );
        assert!(
            steps.to_lowercase().contains("restart"),
            "opencode steps must tell user to restart OpenCode, got {steps:?}"
        );
    }

    /// Hermes and Pi reload commands must stay unchanged (regression guard).
    #[test]
    fn hermes_and_pi_still_have_reload_commands() {
        let (_, hermes_steps, _) = byo_registration("hermes", "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            hermes_steps.contains("/reload-mcp"),
            "hermes must still mention /reload-mcp, got {hermes_steps:?}"
        );

        let (_, pi_steps, _) = byo_registration("pi", "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            pi_steps.contains("/reload"),
            "pi must still mention /reload, got {pi_steps:?}"
        );
    }

    #[test]
    fn pi_fragment_emits_lifecycle_eager_cursor_agent_does_not() {
        let (pi_snippet, _, _) = byo_registration("pi", "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            pi_snippet.contains(r#""lifecycle": "eager""#),
            "pi fragment must include lifecycle eager, got {pi_snippet:?}"
        );

        let (cursor_snippet, _, _) =
            byo_registration("cursor-agent", "http://127.0.0.1:5051/mcp", "beef");
        assert!(
            !cursor_snippet.contains("lifecycle"),
            "cursor-agent fragment must not include lifecycle key, got {cursor_snippet:?}"
        );
    }
}
