//! The HTTP Completer and the env config that builds it.
//!
//! One of two Completers. `harness.rs` is the attached Harness over ACP; this
//! file is the chat-completions stand-in that stays for everyone who attaches
//! nothing. `completer` picks between them and holds the in-flight call.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use fidget_core::director::{self, Completer, Pace, Reply, WakeRequest, WAKE_EVERY};
use serde::Serialize;
use url::{Host, Url};

/// Model API turn. After this, fall back to `StaticDirector`.
/// One budget for remote and local HTTP, not `harness::TURN_TIMEOUT`.
/// A cold local server sets `FIDGET_DIRECTOR_TIMEOUT_SECS`.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Prompt, raw reply, and parse. Off unless asked: a Character Prompt is
/// a paragraph, and printing it sixty times a minute would bury everything
/// else. Same gate as the hit-test and frame traces.
pub fn tracing() -> bool {
    crate::dev_flags::TRACE_DIRECTOR.is_on()
}

/// Blank-AI mode in force: every turn carries what just happened and nothing
/// else. Read where a `ModelDirector` is built, for the reason
/// `ModelDirector::blank` gives.
pub fn blank() -> bool {
    crate::dev_flags::DIRECTOR_BLANK.is_on()
}

fn trace_block(which: &str, text: &str) {
    eprintln!("director: --- {which} ---");
    eprint!("{text}");
    if !text.ends_with('\n') {
        eprintln!();
    }
    eprintln!("director: --- end {which} ---");
}

/// `pub(crate)` so the settings window can name the variable that owns a row.
pub(crate) const API_KEY: &str = "FIDGET_DIRECTOR_API_KEY";
pub(crate) const BASE_URL: &str = "FIDGET_DIRECTOR_BASE_URL";
pub(crate) const MODEL: &str = "FIDGET_DIRECTOR_MODEL";
pub(crate) const ENABLED: &str = "FIDGET_DIRECTOR";
/// First ambient session wait, in seconds. Not a heartbeat.
/// `pub(crate)` so the settings window can name the row it owns.
pub(crate) const WAKE_SECS: &str = "FIDGET_DIRECTOR_WAKE_SECS";

/// Model API timeout, in seconds, and the turn ceiling, in tokens.
/// `pub(crate)` so the settings window can name the frozen row.
pub(crate) const TIMEOUT_SECS: &str = "FIDGET_DIRECTOR_TIMEOUT_SECS";
pub(crate) const MAX_TOKENS: &str = "FIDGET_DIRECTOR_MAX_TOKENS";

/// How hard the model is asked to think. Takes any string: what a value
/// means is the host's, and on llama.cpp and oMLX the chat template's.
pub(crate) const REASONING_EFFORT: &str = "FIDGET_DIRECTOR_REASONING_EFFORT";

/// Blank-AI mode: send what just happened and nothing else. A switch, so it
/// reads the same words every other switch does, and owns its Development row.
pub(crate) const BLANK: &str = "FIDGET_DIRECTOR_BLANK";

const DEFAULT_BASE: &str = "https://api.openai.com";

/// What one HTTP Completer turn is capped at when nothing else decides.
///
/// A safeguard against a model that will not stop, not a budget tuned to a
/// surface. Nothing downstream needs it smaller: the Speech bubble draws six
/// lines of whatever arrives (`src/bubble.js`) and the Chat transcript
/// scrolls. So there is one number for every wake, the same local or hosted.
///
/// It replaced 80 hosted and 512 local (#606). That split was there because a
/// local reasoning model spends a reply-sized cap thinking, which is what
/// `THINK_CEILING` solves properly, and because 80 was cutting a typed
/// question off mid-answer. A higher floor costs a runaway turn more tokens
/// before the guard trips, which is the trade this number is.
const TURN_CEILING: u32 = 1024;

/// What a host that marks its reasoning is capped at instead (#606 §C).
///
/// The wire carries one number covering thought and answer together on every
/// path we speak, and no server can be told not to count thinking, so a cap
/// sized for an answer is one a reasoning model hits mid-thought. #598
/// measured a median burn of 479 tokens against a 512 cap. Anything derived
/// from `TURN_CEILING` brings that back, so this is absolute.
///
/// It is not a reply length. What it guards is a runaway turn on a hosted
/// key: a model that loops instead of concluding would bill until the
/// timeout. 8192 is an order of magnitude above the measured burn, which is
/// the room a long reasoning turn wants, and matches what the endpoints in
/// scope already allow reasoning by default. A turn still going at 8192 is
/// stuck rather than thinking, and the Action Log says so.
///
/// `FIDGET_DIRECTOR_MAX_TOKENS` outranks it: a number the user typed is an
/// instruction, not a default to improve on.
const THINK_CEILING: u32 = 8192;

/// The low level. Tests and a host that has not refused the field use it
/// as a concrete value. An unset row does not: that omits the field.
#[cfg(test)]
const DEFAULT_EFFORT: &str = "low";

/// Last user turn and the config that produced it. #18 displays this.
#[derive(Clone, Debug, Serialize)]
pub struct DirectorInspect {
    pub enabled: bool,
    pub configured: bool,
    pub proactive_wakes: bool,
    pub wake_secs: u64,
    pub last_payload: Option<String>,
    /// The attached Harness, when `FIDGET_HARNESS` named one. Its `login`
    /// is set when attached but not authenticated, and it names the command.
    pub harness: Option<crate::harness::HarnessInspect>,
    /// The HTTP Completer in force. The model, and the host without its
    /// scheme, path or userinfo. A credential is not logged or shown.
    pub model: String,
    pub host: String,
}

/// Director on/off and the first ambient session wait. Read from the env.
#[derive(Clone, Debug)]
pub struct DirectorConfig {
    pub enabled: bool,
    pub configured: bool,
    /// What `FIDGET_DIRECTOR` says, if it says anything. Read here rather
    /// than in `apply_switch`, which the frame loop calls every tick, and
    /// nothing sets the variable once the process is running.
    env_says: Option<bool>,
    /// The env var is set, but trim left nothing usable — `$XAI_API_KEY`
    /// expanding to empty used to look like the key was never offered.
    pub key_invalid: bool,
    /// Static Director interval. Free, so it stays short.
    pub wake_every: Duration,
    /// First ambient session wait. `Pace` doubles from here.
    pub ambient_first: Duration,
    /// Proactive session wakes. Off keeps reactive wakes and Static idle life.
    pub proactive_allowed: bool,
}

impl DirectorConfig {
    /// Fold the saved switch in. The only place that composes switch and
    /// configured into what the Director does. Setting `enabled` from
    /// `configured` alone loses the variable.
    pub fn apply_switch(&mut self, saved_on: bool) {
        self.enabled = self.env_says.unwrap_or(saved_on) && self.configured;
    }

    /// `settings` because the switch and the endpoint are read from different
    /// places and the Chat header needs both: the config says whether anything
    /// answers, the settings say what would.
    pub fn inspect(&self, settings: &DirectorSettings) -> DirectorInspect {
        DirectorInspect {
            enabled: self.enabled,
            configured: self.configured,
            proactive_wakes: self.proactive_allowed,
            wake_secs: self.ambient_first.as_secs(),
            last_payload: None,
            harness: crate::harness::attached().map(|session| session.inspect()),
            model: settings.model.clone(),
            host: host_of(&settings.base_url),
        }
    }
}

/// What `FIDGET_DIRECTOR_API_KEY` held, after quotes and whitespace.
#[derive(Clone, Debug, PartialEq, Eq)]
enum KeyRead {
    Unset,
    Invalid,
    Present(String),
}

/// Resolved base URL, model, and key before they become a Completer.
/// Empty `api_key` is unset or invalid. `key_invalid` is set-but-unusable.
#[derive(Clone)]
pub struct DirectorSettings {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub key_invalid: bool,
}

impl std::fmt::Debug for DirectorSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectorSettings")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("key_fingerprint", &key_fingerprint(&self.api_key))
            .field("key_invalid", &self.key_invalid)
            .finish()
    }
}

/// Env first, then persisted settings, then defaults. Does not write env.
/// Empty env values fall through. An invalid env key still wins over a stored key.
/// Blank or whitespace leaves the model unset. No default name is inserted.
pub fn resolve(
    persisted_base: &str,
    persisted_model: &str,
    stored_key: Option<&str>,
) -> DirectorSettings {
    let base_url = resolve_string(BASE_URL, persisted_base, DEFAULT_BASE);
    let model = env_or_file(MODEL, persisted_model).trim().to_string();
    let key = match key_from_env() {
        KeyRead::Unset => key_from_raw(stored_key),
        other => other,
    };
    let (api_key, key_invalid) = match key {
        KeyRead::Present(key) => (key, false),
        KeyRead::Invalid => (String::new(), true),
        KeyRead::Unset => (String::new(), false),
    };
    DirectorSettings {
        base_url,
        model,
        api_key,
        key_invalid,
    }
}

fn resolve_string(var: &str, persisted: &str, default: &str) -> String {
    match env_override(var) {
        Some(value) => value,
        None if !persisted.is_empty() => persisted.to_string(),
        None => default.to_string(),
    }
}

/// What `var` will impose on the file, if the process exported one.
/// Empty is unset: a `$VAR` that expanded to nothing is a mistake, not an override.
pub(crate) fn env_override(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|value| !value.is_empty())
}

/// The value in force for `var`: the exported one, else the file's.
/// For rows whose blank means the endpoint's own default.
pub(crate) fn env_or_file(var: &str, file: &str) -> String {
    env_override(var).unwrap_or_else(|| file.to_string())
}

/// Build Director on/off from already-resolved settings.
pub fn config_from(settings: &DirectorSettings) -> DirectorConfig {
    let configured = crate::harness::attached().is_some()
        || !settings.api_key.is_empty()
        || is_local(&settings.base_url);
    let env_says = env_switch(ENABLED);
    DirectorConfig {
        enabled: env_says.unwrap_or(true) && configured,
        configured,
        env_says,
        key_invalid: settings.key_invalid,
        wake_every: WAKE_EVERY,
        ambient_first: ambient_first(),
        proactive_allowed: true,
    }
}

/// An OpenAI-compatible chat Completer from already-resolved settings, or
/// `None` when a remote host has no key set.
pub fn endpoint_from(settings: &DirectorSettings) -> Option<Endpoint> {
    let local = is_local(&settings.base_url);
    let api_key = if !settings.api_key.is_empty() {
        settings.api_key.clone()
    } else if local {
        // `headers` omits Authorization when the key is empty, so a local
        // server sees a plain request rather than a made-up Bearer token.
        String::new()
    } else {
        return None;
    };
    Some(Endpoint {
        api_key,
        url: completions_url(&settings.base_url),
        model: settings.model.clone(),
        timeout: timeout_for(),
        max_tokens: max_tokens_for(),
        cap_pinned: crate::dev_flags::director_max_tokens().is_some(),
        effort: effort_for(),
        session: Mutex::new(Session::default()),
        streams: AtomicBool::new(true),
        takes_effort: AtomicBool::new(true),
        takes_max_tokens: AtomicBool::new(true),
        marks_thinking: AtomicBool::new(false),
        agent: ureq::agent(),
    })
}

/// Length and last four. Enough to tell two keys apart, not enough to use.
pub fn key_fingerprint(key: &str) -> String {
    let n = key.len();
    let last = if n >= 4 { &key[n - 4..] } else { "****" };
    format!("len={n} last={last}")
}

/// Read Director config from the env. No API key means `StaticDirector`
/// only, unless the server is on this machine or this LAN.
/// Env-only wrapper. The overlay resolves from settings and the store.
#[expect(dead_code)] // env-only wrapper; overlay call sites now use config_from
pub fn config() -> DirectorConfig {
    config_from(&resolve("", "", None))
}

/// One line for the mode, and a warning when a key was offered but unusable.
/// Empty is almost always a `$VAR` that expanded to nothing, not a choice.
pub fn startup_lines(config: &DirectorConfig) -> Vec<String> {
    let mut lines = crate::harness::startup_lines(config.enabled);
    if config.key_invalid {
        lines.push(format!(
            "director: warning: {API_KEY} is set but not a usable key; using StaticDirector"
        ));
    }
    if config.enabled {
        lines.push(format!(
            "director: model, ambient first {}s",
            config.ambient_first.as_secs()
        ));
    } else if config.configured {
        lines.push("director: off; using StaticDirector".to_string());
    } else {
        lines.push("director: StaticDirector".to_string());
    }
    lines
}

/// Strip wrapping quotes and whitespace. `.env` files quote keys; a
/// trailing newline is enough to 401 a Bearer token.
pub(crate) fn trim_key(raw: &str) -> Option<String> {
    let key = raw
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .to_string();
    (!key.is_empty()).then_some(key)
}

fn key_from_raw(raw: Option<&str>) -> KeyRead {
    match raw {
        None => KeyRead::Unset,
        Some(value) => match trim_key(value) {
            Some(key) => KeyRead::Present(key),
            None => KeyRead::Invalid,
        },
    }
}

fn key_from_env() -> KeyRead {
    match std::env::var(API_KEY) {
        Err(std::env::VarError::NotPresent) => KeyRead::Unset,
        Err(std::env::VarError::NotUnicode(_)) => KeyRead::Invalid,
        Ok(raw) => key_from_raw(Some(&raw)),
    }
}

/// Has the process already settled the key on its own?
/// True means reading the secret store cannot change the outcome.
/// Set-but-unusable still counts: the process asked to override.
pub(crate) fn env_owns_key() -> bool {
    !matches!(key_from_env(), KeyRead::Unset)
}

/// The vocabulary every switch answers to, and the only place it is stated.
/// One vocabulary so `=true` cannot turn one switch on and another off.
/// A word outside it is a typo; `env_switch_warnings` names it at launch.
fn switch_from(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" => Some(true),
        "0" | "off" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// What `var` says a switch should be, if it says anything a switch can hear.
pub(crate) fn env_switch(var: &str) -> Option<bool> {
    switch_from(&env_override(var)?)
}

/// One line per variable in `vars` holding a value no switch could read.
pub fn env_switch_warnings(vars: &[&str]) -> Vec<String> {
    vars.iter()
        .filter_map(|var| {
            let value = env_override(var)?;
            switch_from(&value).is_none().then(|| {
                format!(
                    "env: warning: {var}={value} is not on or off (1/0, true/false, yes/no); ignoring it"
                )
            })
        })
        .collect()
}

/// The Director switch in force: the exported value, else the saved one.
/// Not folded with `configured`. The box shows the switch, and a key lives
/// in the secret store that this layer does not read.
pub(crate) fn director_in_force(saved_on: bool) -> bool {
    env_switch(ENABLED).unwrap_or(saved_on)
}

/// The first ambient wait in force.
/// Precedence is `dev_flags::seed`'s. Zero and unparsable are unset there.
fn ambient_first() -> Duration {
    crate::dev_flags::director_wake_secs().map_or(Pace::FIRST, Duration::from_secs)
}

/// Parse a base URL that has a host.
/// `Url` so `10.0.0.1@172.16.evil.com` is credentials plus evil.com, not LAN.
/// `host_str` never hands a password to a caller that draws it.
fn url_of(base: &str) -> Option<Url> {
    Url::parse(base).ok().filter(Url::has_host)
}

/// Host and port for the Chat header. Empty when `base` is not a URL with a host.
pub fn host_of(base: &str) -> String {
    let Some(url) = url_of(base) else {
        return String::new();
    };
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// Is this base URL served from this machine or this LAN?
/// A local host makes `FIDGET_DIRECTOR_API_KEY` optional.
fn is_local(base: &str) -> bool {
    let Some(url) = url_of(base) else {
        return false;
    };
    match url.host() {
        // A fully-qualified name ends in a dot, and DNS reads it as the same
        // name. `Url` has already lowercased it.
        Some(Host::Domain(name)) => {
            let name = name.trim_end_matches('.');
            name == "localhost" || name.ends_with(".local")
        }
        Some(Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private(),
        // fc00::/7 is the IPv6 private range. `Ipv6Addr::is_unique_local` is
        // still unstable, and this repo builds on the pinned stable toolchain.
        Some(Host::Ipv6(ip)) => ip.is_loopback() || ip.octets()[0] & 0xfe == 0xfc,
        None => false,
    }
}

fn timeout_for() -> Duration {
    // `dev_flags` holds the value the variable or the file settled on, so the
    // precedence is not re-decided here (#273).
    crate::dev_flags::director_timeout_secs().map_or(TIMEOUT, Duration::from_secs)
}

/// The cap a turn goes out with, before `wire_budget` hands a host that
/// marks its thinking the ceiling instead. No argument: the guard does not
/// vary. As with the timeout, decided in `dev_flags::seed`. A zero cap is
/// unset there: a reply with no room to answer in is not a value to keep.
fn max_tokens_for() -> u32 {
    crate::dev_flags::director_max_tokens().unwrap_or(TURN_CEILING)
}

/// The reasoning effort in force. Decided in `dev_flags::seed`, as the
/// timeout and the cap are. Blank there is unset, and unset is omitted.
fn effort_for() -> Option<String> {
    crate::dev_flags::director_reasoning_effort()
}

/// What an empty Model API timeout field means, in seconds.
pub(crate) fn timeout_placeholder() -> String {
    TIMEOUT.as_secs().to_string()
}

/// What an empty turn-ceiling field means, in tokens. See `timeout_placeholder`.
/// Both numbers, because the second is the one a reasoning model meets.
pub(crate) fn max_tokens_placeholder() -> String {
    format!("{TURN_CEILING} ({THINK_CEILING} once the host marks thinking)")
}

/// Empty reasoning-effort field. Leave blank to leave effort unset.
/// The field shows no level.
pub(crate) fn effort_placeholder() -> String {
    String::new()
}

/// What an empty wake-interval field means, in seconds. One default here:
/// the first wait does not depend on where the Completer runs.
pub(crate) fn wake_secs_placeholder() -> String {
    Pace::FIRST.as_secs().to_string()
}

/// An OpenAI-compatible chat Completer, or `None` when a remote host has no
/// key set. Env-only wrapper. The overlay resolves from settings and the store.
pub fn endpoint() -> Option<Endpoint> {
    endpoint_from(&resolve("", "", None))
}

/// Join a provider base onto the inference path without doubling `/v1`.
/// OpenAI, Anthropic's compatibility layer, and Ollama speak
/// `/v1/chat/completions`. xAI's current path is `/v1/responses`.
fn completions_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/chat/completions") || base.ends_with("/responses") {
        return base.to_string();
    }
    let path = if host_is_xai(base) {
        "responses"
    } else {
        "chat/completions"
    };
    if base.ends_with("/v1") {
        format!("{base}/{path}")
    } else {
        format!("{base}/v1/{path}")
    }
}

/// Whether this URL is served by xAI, which decides the inference path below.
fn host_is_xai(url: &str) -> bool {
    url_of(url)
        .and_then(|url| url.host_str().map(str::to_string))
        .is_some_and(|host| host == "api.x.ai" || host.ends_with(".api.x.ai"))
}

/// Whether this URL already points at the Responses path.
/// Uses the parsed path, not a substring of the whole URL.
fn uses_responses(url: &str) -> bool {
    url_of(url).is_some_and(|url| url.path().contains("/responses"))
}

#[derive(Clone)]
struct Message {
    role: &'static str,
    content: String,
}

/// The conversation, and which turn is open in it.
/// A counter rather than last-message position: two calls can be inside
/// `post` at once, and "the question at the end" does not say whose.
#[derive(Default)]
struct Session {
    messages: Vec<Message>,
    opened: u64,
}

pub struct Endpoint {
    api_key: String,
    url: String,
    model: String,
    timeout: Duration,
    max_tokens: u32,
    /// Did `max_tokens` come from `FIDGET_DIRECTOR_MAX_TOKENS` or the
    /// settings row, rather than from the built-in default? A pinned cap is
    /// the one thing `THINK_CEILING` does not overrule. Decided here, not at
    /// the read site, for the reason `effort` and `timeout` are: the endpoint
    /// is rebuilt on a settings change (`completer_retargets`), so this is as
    /// fresh as they are, and `wire_budget` stays a function of the endpoint
    /// rather than of whatever `dev_flags` holds at the moment it is called.
    cap_pinned: bool,
    /// How hard to ask this host to think, verbatim. Baked in here so a
    /// settings change reaches a running Director through `completer_retargets`.
    /// `None` omits `reasoning_effort` and `reasoning.effort`.
    effort: Option<String>,
    /// Opening + replies, so a follow-up can be short. ADR-0008.
    session: Mutex<Session>,
    /// Does this host stream? Starts optimistic and only ever falls, once a
    /// whole reply has succeeded where a stream did not. Per host, not per
    /// `url`, or a path that streamed would latch the other path off too.
    streams: AtomicBool,
    /// Does this host take `reasoning_effort`? Same shape as `streams`.
    /// About the field, never the value, so a new `effort` does not invalidate
    /// it. `completer_retargets` builds a fresh `Endpoint` optimistic again.
    takes_effort: AtomicBool,
    /// Does this host take `max_tokens`? The fallback renames rather than
    /// drops: a refusal retries with `max_completion_tokens`. Optimistic
    /// because Ollama has no such field and would silently answer with no cap.
    takes_max_tokens: AtomicBool,
    /// Has a turn against this host ever marked its reasoning? Same
    /// probe-then-remember shape as `streams`, with the polarity the other
    /// way round: pessimistic, and it only ever rises. The first turn against
    /// an unknown host therefore sends today's single combined number and
    /// behaves exactly as it does today (#606 §C, acceptance box 5); a host
    /// that marked once is given think room on top from the next turn on.
    marks_thinking: AtomicBool,
    /// Held rather than built per call: `ureq::get`/`ureq::post` are use-once
    /// Agents, so each wake would throw away the pooled connection.
    agent: ureq::Agent,
}

impl Endpoint {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn is_xai(&self) -> bool {
        host_is_xai(&self.url)
    }

    /// Length and last four. Enough to tell two keys apart, not enough to use.
    pub fn key_fingerprint(&self) -> String {
        key_fingerprint(&self.api_key)
    }

    pub fn origin(&self) -> String {
        origin(&self.url)
    }

    /// The other xAI inference path, if this URL has one.
    /// Keys are granted per-endpoint. `/v1/responses` is current; many console
    /// keys only have the legacy chat-completions ACL, which is a 403.
    pub fn alternate_url(&self) -> Option<String> {
        alternate_url(&self.url)
    }

    /// GET `url`. Non-2xx is still `Ok`. The status and body are the answer.
    pub fn get(&self, url: &str) -> Result<(u16, String), String> {
        let request = self
            .headers(self.agent.get(url), "application/json")
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(self.timeout))
            .build();
        match request.call() {
            Ok(response) => read_response(response),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Send `prompt` as the next session turn and read the reply.
    /// Takes `url` so the probe can show both xAI paths and `complete` can retry.
    /// A fallback retries the same session snapshot; a succeeded drop latches.
    /// `instance` is whose Chat surface a streamed thought is drawn in.
    pub fn post(&self, url: &str, prompt: &str, instance: &str) -> Result<Reply, String> {
        let (turn, snapshot) = self.open_turn(prompt);
        let mut wire = if self.streams.load(Ordering::SeqCst) {
            Wire::Stream
        } else {
            Wire::Whole
        };
        let mut effort = self.takes_effort.load(Ordering::SeqCst) && self.effort.is_some();
        let mut cap = self.takes_max_tokens.load(Ordering::SeqCst);
        let mut reply = self.send(url, &snapshot, wire, effort, cap, instance);
        // A loop rather than one retry: three guarded fields, and a validator
        // strict enough to refuse two would otherwise lose the wake. Bounded
        // by the field count so a body that keeps naming one cannot post forever.
        for _ in 0..Field::ALL.len() {
            let Err(unsent) = &reply else { break };
            let Some((field, settles)) = unsent.retry_settles() else {
                break;
            };
            let sent = match field {
                Field::Stream => wire == Wire::Stream,
                Field::Effort => effort,
                // The Responses path spells the cap `max_output_tokens`, so
                // a body naming `max_tokens` there is about something this
                // request never sent.
                Field::Cap => cap && !uses_responses(url),
            };
            if !sent {
                break;
            }
            if tracing() {
                // The value, not just the name: a host that takes `low` and
                // refuses `high` latches the field off, and this line is
                // the only place that says which value cost it.
                eprintln!(
                    "director: {}; retrying without {}",
                    unsent.why(),
                    dropped_field(field, self.effort.as_deref().unwrap_or(""))
                );
            }
            // A call dropped between two attempts must not become a fresh
            // request the frame loop can no longer reach.
            if crate::completer::abandoned() {
                reply = Err(Unsent::Abandoned);
                break;
            }
            match field {
                Field::Stream => wire = Wire::Whole,
                Field::Effort => effort = false,
                Field::Cap => cap = false,
            }
            reply = self.send(url, &snapshot, wire, effort, cap, instance);
            // Evidence, not a guess: the server rejected the field and the
            // request without it worked. A misread 400 fails twice and settles
            // nothing, and neither does a stream that merely broke.
            if reply.is_ok() && settles {
                match field {
                    Field::Stream => self.streams.store(false, Ordering::SeqCst),
                    Field::Effort => self.takes_effort.store(false, Ordering::SeqCst),
                    Field::Cap => self.takes_max_tokens.store(false, Ordering::SeqCst),
                }
            }
        }
        self.close_turn(turn, reply.map_err(Unsent::into_error))
    }

    /// Paired with `close_turn`: the session only grows here and is only
    /// trimmed there. Snapshot rather than the lock, so a fallback asks
    /// the identical question. A trailing question is a superseded call.
    fn open_turn(&self, prompt: &str) -> (u64, Vec<Message>) {
        let mut session = self.session.lock().expect("session lock");
        if session
            .messages
            .last()
            .is_some_and(|last| last.role == "user")
        {
            session.messages.pop();
        }
        session.messages.push(Message {
            role: "user",
            content: prompt.to_string(),
        });
        session.opened += 1;
        (session.opened, session.messages.clone())
    }

    /// A failed turn pops its user message so the next prompt is not two
    /// questions. A superseded turn touches nothing. Its question is already
    /// gone, and mutating the winner's turn would attach the loser's reply.
    fn close_turn(&self, turn: u64, reply: Result<Reply, String>) -> Result<Reply, String> {
        let mut session = self.session.lock().expect("session lock");
        if session.opened != turn {
            return reply;
        }
        match reply {
            Ok(reply) => {
                // Truncated or not: the user heard these words, so the next
                // turn is built on them. The mark is session/Chat only.
                // `parse_proposal` would speak a mark in the parser's text.
                session.messages.push(Message {
                    role: "assistant",
                    content: director::marked(&reply.text, reply.truncated),
                });
                Ok(reply)
            }
            Err(error) => {
                session.messages.pop();
                Err(error)
            }
        }
    }

    /// One POST. Both attempts come through here, so the fallback differs
    /// from the first try in exactly one field.
    fn send(
        &self,
        url: &str,
        session: &[Message],
        wire: Wire,
        effort: bool,
        cap: bool,
        instance: &str,
    ) -> Result<Reply, Unsent> {
        let accept = match wire {
            Wire::Stream => "text/event-stream",
            Wire::Whole => "application/json",
        };
        let responses = uses_responses(url);
        let body = request_body(
            &self.model,
            session,
            responses,
            self.wire_budget(),
            wire,
            effort,
            self.effort.as_deref(),
            cap,
        );
        let request = self
            .headers(self.agent.post(url), accept)
            .header("Content-Type", "application/json")
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(self.timeout))
            .build();
        let response = request
            .send_json(body)
            .map_err(|error| Unsent::Failed(error.to_string()))?;

        let code = response.status().as_u16();
        if !(200..300).contains(&code) {
            let (_, text) = read_response(response).map_err(Unsent::Failed)?;
            let error = status_error(url, code, &text);
            // Only a field this request actually sent: a body that names one
            // we left out is talking about something else, and dropping it
            // again would send the identical request.
            let refused = refused_field(code, &text).filter(|field| match field {
                Field::Stream => wire == Wire::Stream,
                Field::Effort => effort,
                Field::Cap => cap && !responses,
            });
            return Err(match refused {
                Some(field) => Unsent::Refused(field, error),
                None => Unsent::Failed(error),
            });
        }

        match wire {
            Wire::Whole => {
                let (_, text) = read_response(response).map_err(Unsent::Failed)?;
                // The same three endings as a stream, in the shape a server
                // that will not stream sends them.
                let said = content_from_body(&text);
                match (said, truncated_body(&text)) {
                    (Ok(said), false) => Ok(Reply::whole(said)),
                    (Err(error), false) => Err(Unsent::Failed(format!("{url}: {error}"))),
                    (said, true) => self.reply_from(
                        url,
                        Streamed::Truncated(classify_truncation(
                            &said.unwrap_or_default(),
                            thought_in_body(&text),
                        )),
                    ),
                }
            }
            Wire::Stream => {
                // Capped like the whole-body read. `into_reader` is unlimited
                // by default, and a server that never stops sending would
                // otherwise grow this String until the machine gave out.
                let reader = response
                    .into_body()
                    .into_with_config()
                    .limit(STREAM_LIMIT)
                    .reader();
                match read_stream(reader, crate::completer::abandoned, |line| {
                    think(instance, line)
                }) {
                    Ok((streamed, marked)) => {
                        // Evidence, like the three dropped fields above it:
                        // this host marked its reasoning, so the next turn
                        // gets think room on top of the reply budget.
                        if marked {
                            self.marks_thinking.store(true, Ordering::SeqCst);
                        }
                        self.reply_from(url, streamed)
                    }
                    Err(error) => Err(Unsent::Failed(format!("{url}: {error}"))),
                }
            }
        }
    }

    /// The one number the wire carries, in the order the three cases settle.
    /// A cap the user pinned is sent verbatim; a host seen to mark its
    /// reasoning is given `THINK_CEILING` instead of a reply-sized cap; every
    /// other host sends `max_tokens`, which is what it sent before #606.
    fn wire_budget(&self) -> u32 {
        if self.cap_pinned || !self.marks_thinking.load(Ordering::SeqCst) {
            self.max_tokens
        } else {
            THINK_CEILING
        }
    }

    /// What a finished stream is worth. Apart from the socket, so all five
    /// endings are checked without one.
    fn reply_from(&self, url: &str, streamed: Streamed) -> Result<Reply, Unsent> {
        match streamed {
            Streamed::Complete(content) if !content.trim().is_empty() => Ok(Reply::whole(content)),
            Streamed::Complete(_) => Err(Unsent::Failed(format!(
                "{url}: streamed reply had no text content"
            ))),
            Streamed::Truncated(what) => {
                // Parseable is the one #606 row that is still Speech: a full
                // Behavior / sayable line, marked. The other three are Unsent.
                if let Truncation::Parseable(content) = what {
                    Ok(Reply::truncated(content))
                } else {
                    Err(Unsent::Truncated(self.out_of_budget(url, &what)))
                }
            }
            Streamed::Cut => Err(Unsent::Cut(format!("{url}: the stream ended mid-reply"))),
            Streamed::NotEventStream => Err(Unsent::Refused(
                Field::Stream,
                format!("{url}: answered 200 with no event stream in it"),
            )),
            Streamed::Abandoned => Err(Unsent::Abandoned),
        }
    }

    /// Action Log copy for a capped turn. Names the model, the cap, and the
    /// setting that moves it. The four #606 rows call for different settings.
    fn out_of_budget(&self, url: &str, what: &Truncation) -> String {
        // The number this turn was actually sent, which is the ceiling on a
        // host that marks and `max_tokens` on every other. Naming the field
        // would tell a user to raise a knob that was not what stopped them.
        let cap = self.wire_budget();
        let happened = match what {
            Truncation::ThinkingOnly => {
                format!("spent all {cap} tokens thinking and wrote no reply")
            }
            Truncation::Parseable(_) | Truncation::MidSentence(_) => {
                format!("was cut off mid-reply by the {cap}-token cap")
            }
            Truncation::Unmarked(_) => {
                format!("hit the {cap}-token cap with unmarked content; not spoken")
            }
        };
        format!(
            "{url}: {} {happened}; raise {MAX_TOKENS} or lower the model's reasoning effort",
            self.model
        )
    }

    fn headers<B>(
        &self,
        request: ureq::RequestBuilder<B>,
        accept: &str,
    ) -> ureq::RequestBuilder<B> {
        let mut request = request
            .header("User-Agent", "fidget")
            .header("Accept", accept);
        if !self.api_key.is_empty() {
            request = request.header("Authorization", &format!("Bearer {}", self.api_key));
        }
        // Anthropic's OpenAI layer accepts Bearer; the native Messages path
        // wants these two. Sending both covers either.
        if self.url.contains("api.anthropic.com") {
            request = request.header("anthropic-version", "2023-06-01");
            if !self.api_key.is_empty() {
                request = request.header("x-api-key", &self.api_key);
            }
        }
        request
    }
}

/// One Action Log pair for an HTTP Completer wake, `prompt` then `turn`.
/// One pair per `complete`, including after a fallback POST. Logs `chars`
/// rather than the Character Prompt body.
pub(crate) fn note_http_call(
    dir: &std::path::Path,
    request: &WakeRequest,
    result: Result<&Reply, &str>,
    // Same words as the refusal. Read only when the reply is marked.
    truncated: &str,
) {
    crate::action_log::append(
        dir,
        "prompt",
        serde_json::json!({
            "instance": request.instance,
            "wake": if request.reactive { "reactive" } else { "proactive" },
            "chars": request.prompt.len(),
        }),
    );
    match result {
        // A truncated turn still produced words. The line carries both so the
        // log says what was shown and why there was no more.
        Ok(reply) if reply.truncated => crate::action_log::append(
            dir,
            "turn",
            serde_json::json!({ "text": reply.text, "truncated": truncated }),
        ),
        Ok(reply) => {
            crate::action_log::append(dir, "turn", serde_json::json!({ "text": reply.text }))
        }
        Err(why) => crate::action_log::append(dir, "turn", serde_json::json!({ "error": why })),
    }
}

impl Completer for Endpoint {
    fn complete(&self, request: &WakeRequest) -> Result<Reply, String> {
        let prompt = &request.prompt;
        if tracing() {
            eprintln!("director: sending POST {} model={}", self.url, self.model);
            trace_block("prompt", prompt);
            eprintln!("director: waiting for model");
        }
        let result = match self.post(&self.url, prompt, &request.instance) {
            Ok(reply) => {
                if tracing() {
                    trace_block("model", &reply.text);
                }
                Ok(reply)
            }
            Err(error) => {
                if tracing() {
                    eprintln!("director: http {error}");
                }
                if let Some(alt) = fallback_url(&self.url, &error) {
                    if tracing() {
                        eprintln!("director: trying {alt}");
                    }
                    match self.post(&alt, prompt, &request.instance) {
                        Ok(reply) => {
                            if tracing() {
                                trace_block("model", &reply.text);
                            }
                            Ok(reply)
                        }
                        Err(alt_error) => {
                            if tracing() {
                                eprintln!("director: http {alt_error}");
                            }
                            Err(alt_error)
                        }
                    }
                } else {
                    Err(error)
                }
            }
        };
        let noted = match &result {
            Ok(reply) => Ok(reply),
            Err(why) => Err(why.as_str()),
        };
        note_http_call(
            &fidget_core::memory::data_dir(),
            request,
            noted,
            &self.out_of_budget(&self.url, &Truncation::Parseable(String::new())),
        );
        result
    }
}

/// Scheme, host, and port. The pre-flight probe hangs `/v1/models` off this,
/// and a trace line names the endpoint by it.
fn origin(url: &str) -> String {
    url_of(url).map_or_else(|| url.to_string(), |url| url.origin().ascii_serialization())
}

fn alternate_url(url: &str) -> Option<String> {
    if !host_is_xai(url) {
        return None;
    }
    if uses_responses(url) {
        Some(url.replacen("/responses", "/chat/completions", 1))
    } else if url.contains("/chat/completions") {
        Some(url.replacen("/chat/completions", "/responses", 1))
    } else {
        None
    }
}

/// Which optional field the server rejected the request for. Only 400 and 422
/// mean the body is wrong. A 401 or 403 fails the same without the field, and
/// on xAI a 403 is what `fallback_url` handles.
fn refused_field(code: u16, body: &str) -> Option<Field> {
    if !matches!(code, 400 | 422) {
        return None;
    }
    let body = body.to_ascii_lowercase();
    Field::ALL
        .into_iter()
        .find(|field| names(&body, field.name()))
}

/// The field as a word, so a gateway's "upstream connect error" is not read
/// as a refusal of `stream` and charged a second POST.
fn names(body: &str, field: &str) -> bool {
    body.match_indices(field).any(|(at, _)| {
        !body[..at]
            .chars()
            .next_back()
            .is_some_and(|before| before.is_alphanumeric() || before == '_')
    })
}

/// Retry the legacy xAI path only when Responses was refused, not when the
/// body was wrong (400) or the key was unknown (401).
fn fallback_url(url: &str, error: &str) -> Option<String> {
    let refused = error.contains("status 403") || error.contains("status 404");
    (refused && uses_responses(url)).then(|| url.replacen("/responses", "/chat/completions", 1))
}

fn status_error(url: &str, code: u16, body: &str) -> String {
    const CAP: usize = 400;
    let body = body.trim();
    if body.is_empty() {
        format!("{url}: status {code}")
    } else if body.len() > CAP {
        format!("{url}: status {code} {}…", &body[..CAP])
    } else {
        format!("{url}: status {code} {body}")
    }
}

fn read_response(mut response: ureq::http::Response<ureq::Body>) -> Result<(u16, String), String> {
    let code = response.status().as_u16();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    Ok((code, text))
}

/// Truncate a provider body for a terminal. The probe prints these; a WAF
/// HTML page should not scroll the useful lines off the screen.
fn clip_body(body: &str) -> String {
    const CAP: usize = 400;
    let body = body.trim();
    if body.len() > CAP {
        format!("{}…", &body[..CAP])
    } else {
        body.to_string()
    }
}

/// Whether a served id names the asked-for model. Ollama reports
/// `llama3.2:latest` for `llama3.2`, so exact comparison would miss it.
fn model_matches(served: &str, wanted: &str) -> bool {
    served == wanted || served.trim_end_matches(":latest") == wanted.trim_end_matches(":latest")
}

/// Read a `/v1/models` answer. An unreadable body gets the benefit of the
/// doubt, because MLX and some llama.cpp builds omit `data`. An empty `data`
/// list means the server serves nothing, which is worth hearing.
fn preflight_verdict(models: Result<(u16, String), String>, model: &str) -> Result<(), String> {
    let (code, body) = models.map_err(|error| format!("unreachable: {error}"))?;
    if !(200..300).contains(&code) {
        return Err(format!("/v1/models answered {code}"));
    }
    let parsed = serde_json::from_str::<serde_json::Value>(&body).ok();
    let Some(items) = parsed.as_ref().and_then(|value| value["data"].as_array()) else {
        return Ok(());
    };
    let served: Vec<&str> = items
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    if served.is_empty() {
        return Err("is up and serving no models".to_string());
    }
    if served.iter().any(|id| model_matches(id, model)) {
        return Ok(());
    }
    Err(format!(
        "model {model:?} is not served; it has {}",
        served.join(", ")
    ))
}

/// Say once, in the background, whether the configured server is there.
/// Diagnostic only, because a failed wake already falls to `StaticDirector`.
/// Spawned rather than awaited so a stopped server cannot hold up startup. ADR-0004.
pub fn spawn_preflight(settings: &DirectorSettings) {
    if let Some(session) = crate::harness::attached() {
        session.spawn_preflight();
        return;
    }
    let Some(endpoint) = endpoint_from(settings) else {
        return;
    };
    thread::spawn(move || {
        let origin = endpoint.origin();
        let models = endpoint.get(&format!("{origin}/v1/models"));
        match preflight_verdict(models, endpoint.model()) {
            Ok(()) => {
                if tracing() {
                    eprintln!("director: {origin} answered, model {}", endpoint.model());
                }
            }
            // A transport error already quotes the URL it failed to reach;
            // naming the origin again would say it twice.
            Err(why) => {
                let reason = if why.contains(&origin) {
                    why
                } else {
                    format!("{origin} {why}")
                };
                eprintln!("director: {reason}; staying on StaticDirector until it answers");
            }
        }
    });
}

const PING: &str = "Reply with the single word pong and nothing else.";

/// Same Completer the overlay uses, without starting the overlay.
/// `scripts/probe-model.sh` is the face of this.
pub fn run_probe() -> i32 {
    // No settings file on this path. `dev_flags::seed` is where the exported
    // timeout and turn ceiling are read.
    crate::dev_flags::seed(&crate::settings::Settings::default());
    let Some(endpoint) = endpoint() else {
        eprintln!(
            "probe-model: no FIDGET_DIRECTOR_API_KEY, and \
             FIDGET_DIRECTOR_BASE_URL is not a local server"
        );
        return 2;
    };

    println!("probe-model");
    println!("  url    {}", endpoint.url());
    println!("  model  {}", endpoint.model());
    if endpoint.api_key.is_empty() {
        println!("  key    none (local server)");
    } else {
        println!("  key    {}", endpoint.key_fingerprint());
    }
    println!();

    let origin = endpoint.origin();
    let models = endpoint.get(&format!("{origin}/v1/models"));
    probe_result(&format!("{origin}/v1/models"), &models);
    match preflight_verdict(models, endpoint.model()) {
        Ok(()) => println!("  model {} is served", endpoint.model()),
        Err(why) => println!("  {why}"),
    }
    println!();
    if endpoint.is_xai() {
        probe_get(&endpoint, &format!("{origin}/v1/api-key"));
    }

    let mut ok = probe_post(&endpoint, endpoint.url());
    if let Some(alt) = endpoint.alternate_url() {
        ok = probe_post(&endpoint, &alt) || ok;
    }

    if ok {
        0
    } else {
        if endpoint.is_xai() {
            eprintln!(
                "The body above is the answer. 401 is a bad Bearer. \
                 403 is credits, a key ACL, or team mTLS."
            );
        }
        1
    }
}

fn probe_get(endpoint: &Endpoint, url: &str) {
    let answer = endpoint.get(url);
    probe_result(url, &answer);
    println!();
}

fn probe_result(url: &str, answer: &Result<(u16, String), String>) {
    println!("GET {url}");
    match answer {
        Ok((code, body)) => println!("  {code} {}", clip_body(body)),
        Err(error) => println!("  transport {error}"),
    }
}

fn probe_post(endpoint: &Endpoint, url: &str) -> bool {
    println!("POST {url}");
    match endpoint.post(url, PING, "probe") {
        Ok(reply) => {
            println!("  ok {}", clip_body(&reply.text));
            println!();
            true
        }
        Err(error) => {
            println!("  {}", clip_body(&error));
            println!();
            false
        }
    }
}

/// How this request asks for its reply. Stream so the Behavior name can start
/// motion before the dialogue arrives, and so closing the connection ends
/// generation instead of billing a whole-body run the client already dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wire {
    Stream,
    Whole,
}

// One over the clippy cap. Folding two of these would pair a guard with the
// value it guards, or a cap's name with its number.
#[allow(clippy::too_many_arguments)]
fn request_body(
    model: &str,
    session: &[Message],
    responses: bool,
    max_tokens: u32,
    wire: Wire,
    effort: bool,
    level: Option<&str>,
    cap: bool,
) -> serde_json::Value {
    let input = if responses && session.len() == 1 {
        // xAI's first-request example is `input` as a string. Later turns
        // use the role/content array so the opening is not sent again as
        // a new conversation.
        serde_json::Value::String(session[0].content.clone())
    } else {
        serde_json::Value::Array(
            session
                .iter()
                .map(|message| {
                    serde_json::json!({
                        "role": message.role,
                        "content": message.content,
                    })
                })
                .collect(),
        )
    };
    let model = model.trim();
    let mut body = if responses {
        let mut responses_body = serde_json::json!({
            "input": input,
            "max_output_tokens": max_tokens,
            "store": false,
        });
        if !model.is_empty() {
            responses_body["model"] = serde_json::Value::String(model.to_string());
        }
        // grok-4.6 defaults to high. Unguarded by `Field::Effort`, which
        // looks for the chat-completions name. xAI accepts this spelling.
        // Unset omits the object. Sending `low` for a blank row was the bug.
        if let Some(level) = level {
            responses_body["reasoning"] = serde_json::json!({ "effort": level });
        }
        responses_body
    } else {
        let mut chat = serde_json::json!({
            "messages": input,
        });
        if !model.is_empty() {
            chat["model"] = serde_json::Value::String(model.to_string());
        }
        // Local servers still read `max_tokens`, and Ollama has no
        // `max_completion_tokens`. Leading with the new name would leave a
        // local reply uncapped. `Field::Cap` swaps it on refusal.
        chat[if cap { "max_tokens" } else { NEW_CAP }] = max_tokens.into();
        // Without it a reasoning model spends the token cap thinking before
        // writing any content. `Field::Effort` guards this, because not every
        // server accepts the field. A blank row never reaches that guard.
        if effort {
            if let Some(level) = level {
                chat["reasoning_effort"] = serde_json::Value::String(level.to_string());
            }
        }
        chat
    };
    if wire == Wire::Stream {
        body["stream"] = serde_json::Value::Bool(true);
    }
    body
}

/// What a retry says it gave up, for `trace_director`. Effort names the value
/// it carried. Remembering which values a host takes is a cache of its
/// validation rules. Cap says it was renamed, or a reader looks for an uncapped reply.
fn dropped_field(field: Field, effort: &str) -> String {
    match field {
        Field::Stream => field.name().to_string(),
        Field::Effort => format!("{}={effort}", field.name()),
        Field::Cap => format!("{}, under {NEW_CAP}", field.name()),
    }
}

/// The cap's name once a host has refused `max_tokens`.
const NEW_CAP: &str = "max_completion_tokens";

/// Ceiling on a streamed reply, in bytes. `into_reader` is unlimited by
/// default. A megabyte is far past a working two-line reply. It bounds a
/// broken server that never stops sending.
const STREAM_LIMIT: u64 = 1024 * 1024;

/// A request field a server may refuse the whole request over.
/// Send it where it works. Never lose a wake to it. One `Endpoint` flag
/// apiece, so a host that refuses pays one extra POST per session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Stream,
    /// The Responses branch spells this as `reasoning.effort` and is not
    /// guarded here. xAI is the only host that takes that path, and it
    /// accepts it.
    Effort,
    /// The one field that is renamed rather than dropped. The retry still
    /// carries a cap, spelled `max_completion_tokens`.
    Cap,
}

impl Field {
    /// Every guarded field, in the order a retry gives them up. One list, so
    /// the rejection reader and the retry bound cannot disagree about how
    /// many there are.
    const ALL: [Field; 3] = [Field::Cap, Field::Effort, Field::Stream];

    /// The name in the body, which is also the name a rejection uses.
    fn name(self) -> &'static str {
        match self {
            Field::Stream => "stream",
            Field::Effort => "reasoning_effort",
            Field::Cap => "max_tokens",
        }
    }
}

/// Why one attempt produced no reply.
enum Unsent {
    /// The server rejected the request for naming this field. Worth one more
    /// send without it, and worth remembering, because that is about the
    /// server rather than this call.
    Refused(Field, String),
    /// The stream broke before the server marked its end. Worth the same one
    /// retry, but a broken connection says nothing about whether the next
    /// stream will work, so it settles nothing.
    Cut(String),
    /// The model reached the token cap. Never worth a retry, because the same
    /// question at the same cap gets the same nothing. The text names the cap,
    /// the model, and which #606 row it was.
    Truncated(String),
    /// Superseded while the tokens were arriving. No error is composed here,
    /// and not because nobody is waiting: this is simply not where a waiting
    /// caret is told. The frame loop names the wake that cancelled it, as that
    /// wake starts (#890).
    Abandoned,
    /// A status, a transport error, or an unreadable reply.
    Failed(String),
}

impl Unsent {
    /// Borrowed, for the trace line that runs before the retry has decided
    /// anything.
    fn why(&self) -> &str {
        match self {
            Unsent::Refused(_, why)
            | Unsent::Cut(why)
            | Unsent::Truncated(why)
            | Unsent::Failed(why) => why,
            Unsent::Abandoned => "abandoned",
        }
    }

    /// Which field is the same question worth one more send without, and
    /// does an answer settle whether this host takes that field at all?
    fn retry_settles(&self) -> Option<(Field, bool)> {
        match self {
            Unsent::Refused(field, _) => Some((*field, true)),
            Unsent::Cut(_) => Some((Field::Stream, false)),
            Unsent::Abandoned | Unsent::Truncated(_) | Unsent::Failed(_) => None,
        }
    }

    fn into_error(self) -> String {
        self.why().to_string()
    }
}

/// What arrived when the Completer hit the token cap. One variant per #606
/// table row, so Speech policy is a match, not a pile of emptiness checks.
/// Marked reasoning is routed later; nothing here dumps thinking into Speech.
#[derive(Debug, PartialEq, Eq)]
enum Truncation {
    /// Marked reasoning, no answer text. Action Log only. Never Speech.
    ThinkingOnly,
    /// Answer text that still parses to a Behavior / sayable line. Spoken
    /// and marked. The other three rows are refused.
    Parseable(String),
    /// Answer started but will not parse. #302: do not speak or half-parse.
    MidSentence(String),
    /// Bytes in `content` with no reasoning mark. Not Speech. No v1
    /// heuristic onto the Thinking row.
    Unmarked(String),
}

/// How a streamed reply ended.
#[derive(Debug, PartialEq, Eq)]
enum Streamed {
    /// The server marked the end. Empty when the model spent its whole
    /// budget without writing anything.
    Complete(String),
    /// The server marked the end and named the token cap as the reason,
    /// `finish_reason: "length"`, `response.incomplete`, or Anthropic
    /// `stop_reason: "max_tokens"`.
    Truncated(Truncation),
    /// The body ended with the server never saying it was finished, so
    /// whatever arrived is half a sentence.
    ///
    /// ponytail: this trusts every OpenAI-compatible server to mark the end
    /// — `[DONE]`, a `finish_reason`, or `response.completed`. Measured on
    /// xAI (both paths) and oMLX, and it is what OpenAI's own stream does,
    /// so the untested servers in the README's table are expected to follow.
    /// One that does not still answers, because `post` retries it whole, but
    /// it looks truncated on every wake and so pays two POSTs forever without
    /// ever learning better; `FIDGET_TRACE_DIRECTOR` names it in one line.
    /// The upgrade, if a real server ever turns up like this, is to keep what
    /// arrived rather than re-ask for it (#302).
    Cut,
    /// The body held no `data:` frame, so it was never an event stream.
    /// A server took `stream` and ignored it. The refusal has no status of
    /// its own, which makes this the only place it shows.
    NotEventStream,
    /// Superseded, so the reader is dropped mid-generation.
    Abandoned,
}

/// Assemble an SSE reply. Takes a `Read` rather than a response so the
/// shapes below are checked against canned bytes. `thought` is a parameter
/// because the Shell's door is a process global that a test cannot reach.
fn read_stream(
    reader: impl std::io::Read,
    abandoned: impl Fn() -> bool,
    thought: impl Fn(&str),
) -> Result<(Streamed, bool), String> {
    let mut thinking = String::new();
    let ended = read_frames(reader, abandoned, &thought, &mut thinking);
    // The empty thought tells the Chat surface this turn stopped thinking
    // (ADR-0034). Same path as the draw, so all-whitespace thinking, which
    // opened no row, closes none.
    if crate::acp_wire::thought_to_show(&thinking).is_some() {
        thought("");
    }
    // Whether this turn marked, for the caller to remember on the host. Any
    // marked chunk counts, including whitespace the strip never drew: the
    // question is whether the server types reasoning apart, not what it said.
    ended.map(|ended| (ended, !thinking.is_empty()))
}

/// The frame loop itself, split out so every ending (a marker, a cut body,
/// an abandon, an I/O error) leaves through the one line above that takes
/// the strip away.
fn read_frames(
    reader: impl std::io::Read,
    abandoned: impl Fn() -> bool,
    thought: &impl Fn(&str),
    thinking: &mut String,
) -> Result<Streamed, String> {
    use std::io::BufRead;

    let mut reader = std::io::BufReader::new(reader);
    let mut content = crate::acp_wire::Answer::default();
    let mut line = String::new();
    let mut framed = false;
    let mut finished = false;
    let mut truncated = false;
    let mut started = false;
    let ended = |content: crate::acp_wire::Answer, truncated: bool, thought: &str| {
        let content = content.finish();
        if truncated {
            Streamed::Truncated(classify_truncation(&content, !thought.trim().is_empty()))
        } else {
            Streamed::Complete(content)
        }
    };
    loop {
        // Cancel is checked between frames, not between bytes. `read_line`
        // parks until the server writes, so a cancel lands one frame late.
        if abandoned() {
            return Ok(Streamed::Abandoned);
        }
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Ok(match (framed, finished) {
                (false, _) => Streamed::NotEventStream,
                (true, true) => ended(content, truncated, thinking),
                (true, false) => Streamed::Cut,
            });
        }
        let Some(payload) = line.trim().strip_prefix("data:") else {
            // A `:` comment holding a connection open, an event name, or the
            // blank line between frames. None of them carries text.
            continue;
        };
        framed = true;
        let payload = payload.trim();
        if payload == "[DONE]" {
            return Ok(ended(content, truncated, thinking));
        }
        let event = read_event(payload);
        finished |= event.finished;
        truncated |= event.truncated;
        let mut chunk = event.thought.unwrap_or_default();
        if let Some(delta) = event.delta {
            if !started && !delta.is_empty() {
                started = true;
                // A Behavior name is one to three tokens, so the first token
                // is roughly when the sprite could start moving.
                if tracing() {
                    eprintln!("director: first token");
                }
            }
            chunk.push_str(&content.push(&delta));
        }
        if !chunk.is_empty() {
            thinking.push_str(&chunk);
            // The whole thought so far, by the same rule the ACP lane draws:
            // a chunk lands mid-sentence, and half a sentence on its own
            // reads as nonsense.
            if let Some(text) = crate::acp_wire::thought_to_show(thinking) {
                thought(text);
            }
        }
    }
}

/// What one SSE event contributes. Named for the wire rather than the
/// animation `Frame` this codebase means everywhere else.
#[derive(Default)]
struct Event {
    /// Text it adds, if it adds any. Events that carry none (a role
    /// announcement, usage, an end marker) are not errors.
    delta: Option<String>,
    /// Thinking it adds, if the server marked any as thinking. Never `delta`:
    /// that is the reply, whose first line has to parse as a Behavior name and
    /// whose rest the fidget says out loud (ADR-0034).
    thought: Option<String>,
    /// It says the server is done, so an end of body after it is a whole
    /// reply rather than a connection cut.
    finished: bool,
    /// The reason it is done is the token cap, not the model having said
    /// what it had to say.
    truncated: bool,
}

/// Read one event in either chat-completions or Responses shape.
/// Markers distinguish a finished reply from a cut, because `/v1/responses`
/// sends no `[DONE]`. `length`, `response.incomplete`, and Anthropic
/// `stop_reason: "max_tokens"` mean the token cap.
fn read_event(payload: &str) -> Event {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return Event::default();
    };
    let choice = &value["choices"][0];
    let chunk = &choice["delta"];
    let kind = value["type"].as_str();
    let finish = choice["finish_reason"].as_str();
    let stop = value["stop_reason"]
        .as_str()
        .or_else(|| value["delta"]["stop_reason"].as_str());
    // A Responses event's `delta` *is* the text, so its `type` is the only
    // thing that says which text it is.
    let typed = |name| {
        if kind == Some(name) {
            value["delta"].as_str()
        } else {
            None
        }
    };
    Event {
        delta: chunk["content"]
            .as_str()
            .or_else(|| typed("response.output_text.delta"))
            .or_else(|| {
                (value["delta"]["type"] == "text_delta")
                    .then(|| value["delta"]["text"].as_str())
                    .flatten()
            })
            .map(str::to_string),
        // Two names for one field. `reasoning_content` (llama.cpp, oMLX,
        // SGLang, LM Studio for R1) and `reasoning` (vLLM, Ollama, gpt-oss).
        // Responses types reasoning apart, raw or as a summary.
        // Anthropic types it apart too, one key along from `text_delta`. The
        // `content_block_start` that opens the run carries no text, so the
        // deltas are the whole of it.
        thought: chunk["reasoning_content"]
            .as_str()
            .or_else(|| chunk["reasoning"].as_str())
            .or_else(|| typed("response.reasoning_summary_text.delta"))
            .or_else(|| typed("response.reasoning_text.delta"))
            .or_else(|| {
                (value["delta"]["type"] == "thinking_delta")
                    .then(|| value["delta"]["thinking"].as_str())
                    .flatten()
            })
            .map(str::to_string),
        // Responses ends a capped reply with `response.incomplete` and no
        // `[DONE]`. Without that name the body looks cut. Anthropic names
        // the same fact `stop_reason`.
        finished: finish.is_some()
            || stop.is_some()
            || matches!(kind, Some("response.completed" | "response.incomplete")),
        // `length` is the chat-completions cap. Every other finish_reason
        // (`stop`, `tool_calls`, `content_filter`) is a reply the server
        // chose to end, and is left alone.
        truncated: finish == Some("length")
            || kind == Some("response.incomplete")
            || stop == Some("max_tokens"),
    }
}

fn content_from_body(body: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    if let Some(text) = value["choices"][0]["message"]["content"].as_str() {
        // A tagged thought is dropped, as Anthropic's `thinking` below is.
        let mut answer = crate::acp_wire::Answer::default();
        answer.push(text);
        return Ok(answer.finish());
    }
    if let Some(text) = value["output_text"]
        .as_str()
        .filter(|text| !text.is_empty())
    {
        return Ok(text.to_string());
    }
    if let Some(items) = value["output"].as_array() {
        for item in items.iter().filter(|item| item["type"] != "reasoning") {
            if let Some(parts) = item["content"].as_array() {
                for part in parts {
                    if let Some(text) = part["text"].as_str().filter(|text| !text.is_empty()) {
                        return Ok(text.to_string());
                    }
                }
            }
        }
    }
    // Anthropic `/v1/messages`. `thinking` blocks are marked reasoning, not
    // the answer. Not yet drawn: a whole body arrives after the turn is over,
    // and the Thinking row is fed by streamed deltas. Text blocks are the reply.
    if let Some(blocks) = value["content"].as_array() {
        let mut text = String::new();
        for block in blocks {
            if block["type"] == "text" {
                if let Some(part) = block["text"].as_str() {
                    text.push_str(part);
                }
            }
        }
        if !text.is_empty() {
            return Ok(text);
        }
    }
    Err("model reply had no text content".to_string())
}

/// Whether a whole body ended at the token cap.
/// `finish_reason` on chat-completions, `incomplete_details.reason` on
/// Responses, `stop_reason` on Anthropic. An empty cap is named as the
/// cap, not as missing text.
fn truncated_body(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    value["choices"][0]["finish_reason"] == "length"
        || value["incomplete_details"]["reason"] == "max_output_tokens"
        || value["stop_reason"] == "max_tokens"
}

/// Whether the body marked reasoning apart from the answer. Used only to
/// pick a #606 row: marked + no text is ThinkingOnly, unmarked prose is
/// Unmarked. Not a thought-strip route.
fn thought_in_body(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let nonempty =
        |value: &serde_json::Value| value.as_str().is_some_and(|text| !text.trim().is_empty());
    let message = &value["choices"][0]["message"];
    if nonempty(&message["reasoning_content"]) || nonempty(&message["reasoning"]) {
        return true;
    }
    if let Some(blocks) = value["content"].as_array() {
        if blocks.iter().any(|block| block["type"] == "thinking") {
            return true;
        }
    }
    if let Some(items) = value["output"].as_array() {
        if items.iter().any(|item| item["type"] == "reasoning") {
            return true;
        }
    }
    false
}

/// Pick the #606 table row from what the cap-ended turn actually held.
/// `parse_proposal` is the contract, not a thought heuristic: a first word
/// like `Okay` still parses, and v1 does not second-guess that.
fn classify_truncation(content: &str, marked_thought: bool) -> Truncation {
    if content.trim().is_empty() {
        return Truncation::ThinkingOnly;
    }
    if director::parse_proposal(content).is_ok() {
        Truncation::Parseable(content.to_string())
    } else if marked_thought {
        Truncation::MidSentence(content.to_string())
    } else {
        Truncation::Unmarked(content.to_string())
    }
}

/// The Shell's thought door, set once at startup.
///
/// The Harness lane reaches the Chat surface through `Forwarded::Thought`,
/// which it is handed when it is attached. The HTTP lane is built from
/// settings and holds no window handle, so it is given the door instead of a
/// route to it — and a `OnceLock` rather than a field on `Endpoint`, because a
/// Retarget rebuilds the Completer and would drop a field (#611).
///
/// Unset in the probe and the tests, which have no Chat surface to draw on.
///
/// One door for every Instance, so each thought names its Instance: `Slots`
/// gives every Instance its own thread and endpoint, so two can think at once.
static THOUGHT: std::sync::OnceLock<ThoughtDoor> = std::sync::OnceLock::new();

/// The Instance whose turn thought, and the whole thought so far.
pub type ThoughtDoor = Box<dyn Fn(&str, String) + Send + Sync>;

/// Hand the Completer lane the door to every open Chat surface. The first
/// door wins and a later one is dropped: the Shell opens exactly one, and a
/// second caller would be a test racing the app it is testing.
pub fn on_thought(door: ThoughtDoor) {
    let _ = THOUGHT.set(door);
}

/// Draw `line` as what `instance`'s turn has thought so far, or end that
/// turn's thinking when it is empty (ADR-0034).
fn think(instance: &str, line: &str) {
    if let Some(door) = THOUGHT.get() {
        door(instance, line.to_string());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use fidget_core::director::{Context, Happened, ModelDirector};
    use std::sync::mpsc::{self, Receiver};

    /// Run `body` with the three Director vars set as given and the switch
    /// cleared, then all four restored. One lock for the whole test binary.
    /// `settings` tests the same vars, so a second mutex would not serialise.
    pub(crate) fn with_env(
        key: Option<&str>,
        base: Option<&str>,
        model: Option<&str>,
        body: impl FnOnce(),
    ) {
        with_vars(key, base, model, None, None, body)
    }

    /// Run `body` with `FIDGET_DIRECTOR` exported as `value` and the other
    /// three cleared, so the developer's shell cannot decide the result.
    pub(crate) fn with_env_switch(value: &str, body: impl FnOnce()) {
        with_vars(None, None, None, Some(value), None, body)
    }

    /// Run `body` with `FIDGET_HARNESS` set as given, under the same lock.
    /// Settings tests read that Completer-source row through the same
    /// `env_override` as the endpoint rows.
    pub(crate) fn with_harness(value: Option<&str>, body: impl FnOnce()) {
        with_vars(None, None, None, None, value, body)
    }

    fn with_vars(
        key: Option<&str>,
        base: Option<&str>,
        model: Option<&str>,
        enabled: Option<&str>,
        harness: Option<&str>,
        body: impl FnOnce(),
    ) {
        // Concurrent setenv/getenv is undefined behaviour. These vars are
        // process-global and the resolve tests share them; serialise mutation.
        static ENV: Mutex<()> = Mutex::new(());
        let _lock = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        struct Guard(Vec<(&'static str, Option<String>)>);

        impl Drop for Guard {
            fn drop(&mut self) {
                // Seed while the Development variables are still cleared, so
                // live `dev_flags` land on file defaults. Seeding after the
                // restore would load the shell's exports into them instead.
                crate::dev_flags::seed(&crate::settings::Settings::default());
                for (var, previous) in self.0.drain(..) {
                    apply(var, previous.as_deref());
                }
            }
        }

        fn apply(var: &str, value: Option<&str>) {
            match value {
                Some(value) => std::env::set_var(var, value),
                None => std::env::remove_var(var),
            }
        }

        // The five the caller sets, and every Development variable. A shell
        // export would otherwise freeze a row or seed a switch in a test that
        // never mentions it. Seed `dev_flags` in and out under this lock.
        let mut wanted = vec![
            (API_KEY, key),
            (BASE_URL, base),
            (MODEL, model),
            (ENABLED, enabled),
            (crate::harness::VAR, harness),
        ];
        wanted.extend(
            crate::dev_flags::test_vars()
                .into_iter()
                .map(|var| (var, None)),
        );

        let _guard = Guard(
            wanted
                .iter()
                .map(|(var, _)| (*var, std::env::var(var).ok()))
                .collect(),
        );
        for (var, value) in wanted {
            apply(var, value);
        }
        crate::dev_flags::seed(&crate::settings::Settings::default());
        body();
    }

    #[test]
    fn env_beats_persisted_base_and_model() {
        with_env(None, Some("https://api.x.ai"), Some("grok-4.6"), || {
            let settings = resolve("https://api.openai.com", "gpt-4o-mini", Some("sk-stored"));
            assert_eq!(settings.base_url, "https://api.x.ai");
            assert_eq!(settings.model, "grok-4.6");
        });
    }

    #[test]
    fn persisted_is_used_when_env_is_unset() {
        with_env(None, None, None, || {
            let settings = resolve("https://api.x.ai", "grok-4.6", Some("sk-stored-key"));
            assert_eq!(settings.base_url, "https://api.x.ai");
            assert_eq!(settings.model, "grok-4.6");
            assert_eq!(settings.api_key, "sk-stored-key");
            assert!(!settings.key_invalid);
        });
    }

    #[test]
    fn env_key_beats_the_stored_key() {
        with_env(Some("sk-env-key"), None, None, || {
            let settings = resolve("", "", Some("sk-stored-key"));
            assert_eq!(settings.api_key, "sk-env-key");
        });
    }

    #[test]
    fn invalid_env_beats_store() {
        with_env(Some(""), None, None, || {
            let settings = resolve(
                "https://api.openai.com",
                "gpt-4o-mini",
                Some("sk-stored-key"),
            );
            assert!(
                settings.api_key.is_empty(),
                "a blank env key must not fall through to the store"
            );
            assert!(settings.key_invalid);
        });
    }

    #[test]
    fn a_remote_url_without_a_key_is_not_configured() {
        with_env(None, None, None, || {
            let settings = resolve("https://api.openai.com", "gpt-4o-mini", None);
            let config = config_from(&settings);
            assert!(!config.configured);
            assert!(endpoint_from(&settings).is_none());
        });
    }

    /// One vocabulary, so no word that reads like on is quietly off.
    #[test]
    fn a_switch_variable_reads_one_vocabulary() {
        with_env(None, None, None, || {
            for (exported, want) in [
                (None, None),
                (Some("1"), Some(true)),
                (Some("on"), Some(true)),
                (Some("true"), Some(true)),
                (Some("yes"), Some(true)),
                (Some("0"), Some(false)),
                (Some("off"), Some(false)),
                (Some("false"), Some(false)),
                (Some("no"), Some(false)),
                (Some("ON"), Some(true)),
                (Some("Off"), Some(false)),
                // An expansion that produced nothing is a mistake, not a
                // choice, and a word no switch knows owns nothing.
                (Some(""), None),
                (Some("banana"), None),
            ] {
                match exported {
                    Some(value) => std::env::set_var(ENABLED, value),
                    None => std::env::remove_var(ENABLED),
                }
                assert_eq!(env_switch(ENABLED), want, "exported {exported:?}");
            }
            std::env::remove_var(ENABLED);
        });
    }

    /// A value nothing obeyed must not pass for one that was.
    #[test]
    fn an_unreadable_switch_value_is_named_at_launch() {
        with_env(None, None, None, || {
            std::env::set_var(ENABLED, "banana");
            let warnings = env_switch_warnings(&[ENABLED]);
            assert_eq!(warnings.len(), 1, "{warnings:?}");
            assert!(warnings[0].contains(ENABLED), "{warnings:?}");
            assert!(warnings[0].contains("banana"), "{warnings:?}");

            std::env::set_var(ENABLED, "off");
            assert!(
                env_switch_warnings(&[ENABLED]).is_empty(),
                "a word the vocabulary knows is not a warning"
            );
            std::env::remove_var(ENABLED);
            assert!(
                env_switch_warnings(&[ENABLED]).is_empty(),
                "unset is silent"
            );
        });
    }

    /// The variable decides in both directions, which is the whole point of
    /// one vocabulary: an exported on lifts a file that says off.
    #[test]
    fn an_exported_switch_decides_either_way() {
        with_vars(None, None, None, Some("on"), None, || {
            let settings = resolve("http://localhost:11434", "gemma4", None);
            let mut config = config_from(&settings);
            config.apply_switch(false);
            assert!(config.enabled, "the file said off, the process said on");
        });
    }

    /// On is still a request, not a Completer: the endpoint has to exist.
    #[test]
    fn an_exported_on_cannot_conjure_a_completer() {
        with_vars(None, None, None, Some("on"), None, || {
            let settings = resolve("https://api.openai.com", "gpt-4o-mini", None);
            let mut config = config_from(&settings);
            assert!(!config.configured, "a remote host with no key");
            config.apply_switch(true);
            assert!(!config.enabled);
        });
    }

    #[test]
    fn an_unreadable_switch_value_leaves_the_file_deciding() {
        with_vars(None, None, None, Some("banana"), None, || {
            let settings = resolve("http://localhost:11434", "gemma4", None);
            let mut config = config_from(&settings);
            config.apply_switch(true);
            assert!(config.enabled, "the file said on");
            config.apply_switch(false);
            assert!(!config.enabled, "the file said off");
        });
    }

    /// `off` keeps Static even when a key is set. A local host is configured
    /// without a key, so nothing but the variable can hold the Director back.
    #[test]
    fn the_env_switch_vetoes_a_director_the_file_would_allow() {
        with_env_switch("off", || {
            let settings = resolve("http://localhost:11434", "gemma4", None);
            let mut config = config_from(&settings);
            assert!(config.configured, "a local host needs no key");
            config.apply_switch(true);
            assert!(!config.enabled, "the file said on, the env vetoed it");
        });
    }

    #[test]
    fn the_saved_switch_decides_when_the_process_says_nothing() {
        with_env(None, None, None, || {
            let settings = resolve("http://localhost:11434", "gemma4", None);
            let mut config = config_from(&settings);
            config.apply_switch(true);
            assert!(config.enabled);
            config.apply_switch(false);
            assert!(!config.enabled, "the file can always turn it off");
        });
    }

    #[test]
    fn a_local_url_without_a_key_is_configured() {
        with_env(None, None, None, || {
            let settings = resolve("http://localhost:11434", "gemma4", None);
            let config = config_from(&settings);
            assert!(config.configured);
            let endpoint = endpoint_from(&settings).expect("local needs no key");
            assert!(endpoint.url().contains("11434"));
            assert_eq!(endpoint.model(), "gemma4");
        });
    }

    #[test]
    fn resolve_does_not_write_env() {
        with_env(None, None, None, || {
            let _ = resolve("https://api.x.ai", "grok-4.6", Some("sk-stored"));
            assert!(std::env::var("FIDGET_DIRECTOR_API_KEY").is_err());
            assert!(std::env::var("FIDGET_DIRECTOR_BASE_URL").is_err());
        });
    }

    #[test]
    fn director_settings_debug_prints_the_fingerprint_not_the_key() {
        with_env(None, None, None, || {
            let settings = resolve("", "", Some("sk-super-secret-key"));
            let dump = format!("{settings:?}");
            assert!(
                !dump.contains("sk-super-secret-key"),
                "Debug must not echo the key: {dump}"
            );
            assert!(
                dump.contains("key_fingerprint"),
                "Debug should name the fingerprint field: {dump}"
            );
            assert!(
                dump.contains(&key_fingerprint("sk-super-secret-key")),
                "Debug should name the fingerprint: {dump}"
            );
        });
    }

    #[test]
    fn a_chat_completion_body_yields_the_message_content() {
        let body = r#"{"choices":[{"message":{"content":"stroll\nhey"}}]}"#;
        assert_eq!(content_from_body(body).unwrap(), "stroll\nhey");
    }

    #[test]
    fn a_responses_body_yields_the_output_text() {
        let body = r#"{
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "stroll\nhey"}]
            }]
        }"#;
        assert_eq!(content_from_body(body).unwrap(), "stroll\nhey");
    }

    /// xAI puts a `reasoning` item ahead of the `message`. Its text is
    /// thinking, and the message is the answer.
    #[test]
    fn a_responses_body_skips_the_reasoning_item() {
        let body = r#"{
            "output": [
                {
                    "type": "reasoning",
                    "content": [{"type": "reasoning_text", "text": "They want the titles."}]
                },
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "mutter\nFidget's in front."}]
                }
            ]
        }"#;
        assert_eq!(
            content_from_body(body).unwrap(),
            "mutter\nFidget's in front."
        );
    }

    #[test]
    fn a_chat_completion_body_drops_a_think_block() {
        let body = r#"{"choices":[{"message":{"content":
            "<think>They want the titles.</think>\nmutter\nFidget's in front."}}]}"#;
        assert_eq!(
            content_from_body(body).unwrap(),
            "\nmutter\nFidget's in front."
        );
    }

    #[test]
    fn a_body_without_content_is_an_error() {
        assert!(content_from_body("{}").is_err());
        assert!(content_from_body("not json").is_err());
    }

    /// A non-streaming server sends the same failure whole. No `content`
    /// key, and the cap named in `finish_reason` or in `incomplete_details`.
    #[test]
    fn a_whole_body_that_hit_the_cap_says_so() {
        let spent = r#"{"choices":[{"message":{"role":"assistant","reasoning_content":"hmm"},
            "finish_reason":"length"}]}"#;
        assert!(content_from_body(spent).is_err());
        assert!(truncated_body(spent));
        assert!(thought_in_body(spent));

        let incomplete = r#"{"status":"incomplete",
            "incomplete_details":{"reason":"max_output_tokens"},"output":[]}"#;
        assert!(truncated_body(incomplete));

        let anthropic = r#"{"content":[{"type":"thinking","thinking":"hmm"}],
            "stop_reason":"max_tokens"}"#;
        assert!(truncated_body(anthropic));
        assert!(thought_in_body(anthropic));
        assert!(content_from_body(anthropic).is_err());

        let done = r#"{"choices":[{"message":{"content":"stroll"},"finish_reason":"stop"}]}"#;
        assert!(!truncated_body(done), "a natural stop is not a truncation");
        assert!(!truncated_body("not json"));
    }

    fn local_endpoint() -> Endpoint {
        endpoint_at("http://localhost:11434/v1/chat/completions")
    }

    fn endpoint_at(url: &str) -> Endpoint {
        Endpoint {
            api_key: String::new(),
            url: url.to_string(),
            model: "gemma4".to_string(),
            timeout: TIMEOUT,
            max_tokens: TURN_CEILING,
            cap_pinned: false,
            effort: Some(DEFAULT_EFFORT.to_string()),
            session: Mutex::new(Session::default()),
            streams: AtomicBool::new(true),
            takes_effort: AtomicBool::new(true),
            takes_max_tokens: AtomicBool::new(true),
            marks_thinking: AtomicBool::new(false),
            agent: ureq::agent(),
        }
    }

    /// A loopback server that refuses any request naming `field`, and hands
    /// back every body it was sent. Local servers in scope ignore unknown
    /// fields, so the OpenAI wording is exercised against a stub instead.
    fn server_refusing(field: Field) -> (String, Receiver<String>) {
        use std::io::{BufRead, BufReader, Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("the bound port").port();
        let (sent, seen) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().expect("the same socket"));
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    return;
                }
                let body = String::from_utf8_lossy(&body).to_string();
                let (status, payload) = if body.contains(field.name()) {
                    (
                        "400 Bad Request",
                        format!(
                            r#"{{"error":{{"message":"Unrecognized request argument supplied: {}"}}}}"#,
                            field.name()
                        ),
                    )
                } else {
                    (
                        "200 OK",
                        r#"{"choices":[{"message":{"content":"stroll\nhey"},"finish_reason":"stop"}]}"#
                            .to_string(),
                    )
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.flush();
                if sent.send(body).is_err() {
                    return;
                }
            }
        });
        (format!("http://127.0.0.1:{port}/v1/chat/completions"), seen)
    }

    /// The field goes out. A server that names it in a rejection gets one
    /// more request without it, and the next wake does not ask again.
    #[test]
    fn a_server_that_refuses_the_effort_field_is_asked_once_and_never_again() {
        let (url, seen) = server_refusing(Field::Effort);
        let endpoint = endpoint_at(&url);
        // Whole-body, so the stub can answer in one JSON object.
        endpoint.streams.store(false, Ordering::SeqCst);

        assert_eq!(
            endpoint.post(&url, "hello", "buddy-1").unwrap().text,
            "stroll\nhey",
            "the refusal costs a second POST, not the wake"
        );
        let asked = seen.recv().expect("the first request");
        assert!(asked.contains("reasoning_effort"), "sent optimistically");
        let retried = seen.recv().expect("the retry");
        assert!(
            !retried.contains("reasoning_effort"),
            "the retry drops the field the server just named"
        );

        endpoint
            .post(&url, "what just happened: poked", "buddy-1")
            .unwrap();
        let next_wake = seen.recv().expect("the next wake");
        assert!(
            !next_wake.contains("reasoning_effort"),
            "a host that refused it once is not asked again"
        );
        assert!(
            seen.try_recv().is_err(),
            "and the second wake pays one POST, not two"
        );
    }

    /// The row's value reaches the wire, not only `request_body`. The level
    /// is baked into the `Endpoint` and `send` is what carries it.
    /// `max` rather than a picker level. Nothing validates what is typed there.
    #[test]
    fn the_configured_effort_reaches_the_request_the_endpoint_sends() {
        // Refusing `stream` and answering everything else: this endpoint is
        // whole-body, so the stub sees no field it objects to.
        let (url, seen) = server_refusing(Field::Stream);
        let endpoint = Endpoint {
            effort: Some("max".to_string()),
            ..endpoint_at(&url)
        };
        endpoint.streams.store(false, Ordering::SeqCst);

        endpoint
            .post(&url, "hello", "buddy-1")
            .expect("the stub answers");
        let asked: serde_json::Value =
            serde_json::from_str(&seen.recv().expect("the request")).expect("the request is JSON");
        assert_eq!(
            asked["reasoning_effort"], "max",
            "the typed level goes out verbatim"
        );
    }

    /// The cap is the one guarded field that is never given up. A host that
    /// refuses `max_tokens` is asked again under the spec's name. Losing it
    /// would uncap the reply on the host least able to afford it.
    #[test]
    fn a_server_that_refuses_max_tokens_is_asked_again_under_the_new_name() {
        let (url, seen) = server_refusing(Field::Cap);
        let endpoint = endpoint_at(&url);
        // Whole-body, so the stub can answer in one JSON object.
        endpoint.streams.store(false, Ordering::SeqCst);

        assert_eq!(
            endpoint.post(&url, "hello", "buddy-1").unwrap().text,
            "stroll\nhey",
            "the refusal costs a second POST, not the wake"
        );
        let asked = seen.recv().expect("the first request");
        assert!(asked.contains(r#""max_tokens""#), "sent optimistically");
        let retried = seen.recv().expect("the retry");
        assert!(
            retried.contains(r#""max_completion_tokens""#),
            "the retry renames the cap rather than dropping it"
        );

        endpoint
            .post(&url, "what just happened: poked", "buddy-1")
            .unwrap();
        let next_wake = seen.recv().expect("the next wake");
        assert!(
            next_wake.contains(r#""max_completion_tokens""#),
            "a host that refused the old name is not asked under it again"
        );
        assert!(
            seen.try_recv().is_err(),
            "and the second wake pays one POST, not two"
        );
    }

    /// A streamed turn can end with no reply by abandon or cut, and each
    /// has to leave the session as an error does.
    #[test]
    fn a_turn_with_no_reply_leaves_no_half_answer_in_the_session() {
        let endpoint = local_endpoint();

        let (opening, asked) = endpoint.open_turn("hello");
        assert_eq!(asked.len(), 1, "the opening turn is the prompt alone");
        endpoint
            .close_turn(opening, Ok(Reply::whole("stroll")))
            .unwrap();

        let (poked, _) = endpoint.open_turn("what just happened: poked");
        endpoint
            .close_turn(poked, Err("abandoned".to_string()))
            .unwrap_err();

        assert_eq!(
            spoken(&endpoint.open_turn("what just happened: thrown").1),
            [
                ("user", "hello"),
                ("assistant", "stroll"),
                ("user", "what just happened: thrown"),
            ]
        );
    }

    /// The session is what the model reads its own last turn back from, so a
    /// capped turn says so there. The mark stays out of the parsed reply.
    /// `parse_proposal` would otherwise speak the mark out loud.
    #[test]
    fn the_session_keeps_the_mark_and_the_parser_never_sees_it() {
        let endpoint = local_endpoint();

        let (opening, _) = endpoint.open_turn("what now?");
        let handed = endpoint
            .close_turn(opening, Ok(Reply::truncated("prowl")))
            .unwrap();

        assert_eq!(
            handed.text, "prowl",
            "the Director parses the model's own words, with no mark in them"
        );
        assert_eq!(
            fidget_core::director::parse_proposal(&handed.text)
                .unwrap()
                .dialogue,
            None,
            "a marked text would be parsed as a Behavior with the mark as its line"
        );
        assert_eq!(
            spoken(&endpoint.session.lock().unwrap().messages),
            [
                ("user", "what now?"),
                ("assistant", "prowl\n[response truncated]")
            ],
            "the session says where the model was stopped"
        );
    }

    /// A superseded call may still be inside `post` when the wake that
    /// replaced it opens a turn on the same `Endpoint`. The loser must neither
    /// leave its question in the session nor take the winner's out.
    #[test]
    fn a_superseded_turn_neither_leaves_its_question_nor_takes_the_winners() {
        let endpoint = local_endpoint();
        let (opening, _) = endpoint.open_turn("hello");
        endpoint
            .close_turn(opening, Ok(Reply::whole("stroll")))
            .unwrap();

        let (ambient, _) = endpoint.open_turn("what just happened: nothing");
        let (poked, asked) = endpoint.open_turn("what just happened: poked");
        assert_eq!(
            spoken(&asked),
            [
                ("user", "hello"),
                ("assistant", "stroll"),
                ("user", "what just happened: poked"),
            ],
            "the abandoned question must not be asked alongside the new one"
        );

        endpoint
            .close_turn(ambient, Err("abandoned".to_string()))
            .unwrap_err();
        endpoint.close_turn(poked, Ok(Reply::whole("nap"))).unwrap();

        assert_eq!(
            spoken(&endpoint.open_turn("what just happened: thrown").1),
            [
                ("user", "hello"),
                ("assistant", "stroll"),
                ("user", "what just happened: poked"),
                ("assistant", "nap"),
                ("user", "what just happened: thrown"),
            ]
        );
    }

    /// Nothing orders the two workers, so the superseded one may open its
    /// turn after the wake that replaced it. An answer must never be recorded
    /// against another turn's question, which is what the next prompt is built from.
    #[test]
    fn an_answer_is_never_recorded_against_another_turns_question() {
        let endpoint = local_endpoint();
        let (poked, _) = endpoint.open_turn("what just happened: poked");
        let (ambient, _) = endpoint.open_turn("what just happened: nothing");

        endpoint.close_turn(poked, Ok(Reply::whole("nap"))).unwrap();
        endpoint
            .close_turn(ambient, Err("abandoned".to_string()))
            .unwrap_err();

        assert_eq!(
            spoken(&endpoint.open_turn("what just happened: thrown").1),
            [("user", "what just happened: thrown")],
            "a question whose turn is closed leaves nothing behind"
        );
    }

    fn spoken(session: &[Message]) -> Vec<(&str, &str)> {
        session
            .iter()
            .map(|message| (message.role, message.content.as_str()))
            .collect()
    }

    /// How a whole stream ended, for the shapes whose thoughts are not what
    /// is being asserted.
    fn streamed(sse: &str) -> Streamed {
        streamed_with_thoughts(sse).0
    }

    /// How a whole stream ended, and every thought the Chat surface was told to draw.
    fn streamed_with_thoughts(sse: &str) -> (Streamed, Vec<String>) {
        let drawn = std::cell::RefCell::new(Vec::new());
        let (ended, _) = read_stream(
            std::io::Cursor::new(sse),
            || false,
            |line| drawn.borrow_mut().push(line.to_string()),
        )
        .unwrap();
        (ended, drawn.into_inner())
    }

    /// Did this stream mark its reasoning? The bit `post` remembers on the host.
    fn streamed_marked(sse: &str) -> bool {
        read_stream(std::io::Cursor::new(sse), || false, |_| {})
            .unwrap()
            .1
    }

    #[test]
    fn a_streamed_chat_completion_assembles_its_deltas() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"stroll\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"\\nhey\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(streamed(sse), Streamed::Complete("stroll\nhey".to_string()));
    }

    #[test]
    fn a_streamed_responses_reply_assembles_its_deltas() {
        let sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"stroll\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"\\nhey\"}\n\n",
            "data: {\"type\":\"response.completed\"}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(streamed(sse), Streamed::Complete("stroll\nhey".to_string()));
    }

    /// A frame arrives in as many TCP reads as the network feels like, and
    /// a keep-alive comment arrives between frames. Neither is a boundary
    /// the parser gets to see.
    #[test]
    fn a_frame_split_across_reads_is_still_one_event() {
        struct Dribble {
            bytes: Vec<u8>,
            sent: usize,
        }

        impl std::io::Read for Dribble {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let take = (self.bytes.len() - self.sent).min(3).min(buf.len());
                buf[..take].copy_from_slice(&self.bytes[self.sent..self.sent + take]);
                self.sent += take;
                Ok(take)
            }
        }

        let sse = concat!(
            ": keep-alive\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"stroll\"}}]}\n\n",
            ": keep-alive\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"\\nhey\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let dribble = Dribble {
            bytes: sse.as_bytes().to_vec(),
            sent: 0,
        };
        assert_eq!(
            read_stream(dribble, || false, |_| {}).unwrap().0,
            Streamed::Complete("stroll\nhey".to_string())
        );
    }

    /// A server that takes `stream: true` and answers with an ordinary body
    /// never says so in a status. Absence of frames is the retry signal.
    /// An empty real stream is not retried, because that would spend a second call on nothing.
    #[test]
    fn a_body_with_no_frames_in_it_was_never_a_stream() {
        let whole = r#"{"choices":[{"message":{"content":"stroll\nhey"}}]}"#;
        assert_eq!(streamed(whole), Streamed::NotEventStream);

        let spent = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let (ended, drawn) = streamed_with_thoughts(spent);
        assert_eq!(
            ended,
            Streamed::Complete(String::new()),
            "a model that thought its whole budget away did stream"
        );
        assert_eq!(
            drawn,
            ["hmm", ""],
            "and the thought it spent it on was drawn and then taken away"
        );
    }

    /// Which half of the wire a delta is, one shape at a time. Every field
    /// name here is one a server in scope sends.
    #[test]
    fn a_marked_reasoning_delta_is_a_thought_and_content_is_still_speech() {
        let read = |payload| {
            let event = read_event(payload);
            (event.thought, event.delta)
        };

        // llama.cpp, oMLX, SGLang, LM Studio for R1.
        assert_eq!(
            read(r#"{"choices":[{"delta":{"reasoning_content":"hmm"}}]}"#),
            (Some("hmm".to_string()), None)
        );
        // vLLM, Ollama, LM Studio for gpt-oss.
        assert_eq!(
            read(r#"{"choices":[{"delta":{"reasoning":"hmm"}}]}"#),
            (Some("hmm".to_string()), None)
        );
        // Responses types its reasoning apart from its answer.
        assert_eq!(
            read(r#"{"type":"response.reasoning_summary_text.delta","delta":"hmm"}"#),
            (Some("hmm".to_string()), None)
        );
        // xAI and gpt-oss stream the raw reasoning rather than a summary.
        assert_eq!(
            read(r#"{"type":"response.reasoning_text.delta","delta":"hmm"}"#),
            (Some("hmm".to_string()), None)
        );
        // Anthropic types it apart one key along from its `text_delta`.
        assert_eq!(
            read(
                r#"{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hmm"}}"#
            ),
            (Some("hmm".to_string()), None)
        );
        assert_eq!(
            read(
                r#"{"type":"content_block_start","content_block":{"type":"thinking","thinking":""}}"#
            ),
            (None, None),
            "the frame that opens the run carries neither half"
        );
        assert_eq!(
            read(r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"stroll"}}"#),
            (None, Some("stroll".to_string())),
            "and its sibling name is still the reply"
        );
        assert_eq!(
            read(r#"{"choices":[{"delta":{"content":"stroll"}}]}"#),
            (None, Some("stroll".to_string())),
            "an unmarked delta is the reply, and nothing here second-guesses it"
        );
        assert_eq!(
            read(r#"{"type":"response.output_text.delta","delta":"stroll"}"#),
            (None, Some("stroll".to_string()))
        );
        assert_eq!(
            read(r#"{"choices":[{"delta":{"reasoning":"hmm","content":"stroll"}}]}"#),
            (Some("hmm".to_string()), Some("stroll".to_string())),
            "a server that marks both in one frame is read for both"
        );
    }

    /// Thinking reaches the Chat surface and never the reply, so nothing the
    /// fidget says out loud was thought at it (ADR-0034). It is handed the whole
    /// thought, blank lines included, and an empty one when the turn ends.
    #[test]
    fn thinking_is_drawn_while_a_turn_runs_and_never_joins_the_reply() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"the user\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\" waved\\n\\nso\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\" wave back\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"wave\\nhey\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(
            streamed_with_thoughts(sse),
            (
                Streamed::Complete("wave\nhey".to_string()),
                [
                    "the user",
                    "the user waved\n\nso",
                    "the user waved\n\nso wave back",
                    "",
                ]
                .map(str::to_string)
                .to_vec()
            )
        );
    }

    /// A server that never typed its reasoning apart leaves `<think>` tags in
    /// `content`, split wherever the chunks fall. The inside is drawn as a
    /// thought and the reply is what follows the closing tag.
    #[test]
    fn a_think_tag_in_streamed_content_is_drawn_and_never_joins_the_reply() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<th\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ink>They want\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" the titles.</th\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ink>mutter\\nhey\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(
            streamed_with_thoughts(sse),
            (
                Streamed::Complete("mutter\nhey".to_string()),
                ["They want", "They want the titles.", ""]
                    .map(str::to_string)
                    .to_vec()
            )
        );
    }

    /// Probe then remember, the shape `streams` and `takes_effort` already
    /// have, with the polarity the other way round: pessimistic, and it only
    /// ever rises. A host is given room to think only after a turn has been
    /// seen to mark, so the first turn against an unknown host is today's wire
    /// and #606 acceptance box 5 holds without a special case.
    #[test]
    fn a_host_that_marks_its_reasoning_stops_being_capped_at_a_reply_length() {
        let marked = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"stroll\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let unmarked = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"stroll\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert!(streamed_marked(marked));
        assert!(
            !streamed_marked(unmarked),
            "content is not a reasoning mark, whatever it reads like"
        );

        let endpoint = Endpoint {
            max_tokens: TURN_CEILING,
            ..local_endpoint()
        };
        assert_eq!(
            endpoint.wire_budget(),
            1024,
            "an unknown host is sent the flat guard and nothing else"
        );
        endpoint.marks_thinking.store(true, Ordering::SeqCst);
        assert_eq!(
            endpoint.wire_budget(),
            8192,
            "and a host that marks is no longer capped at a reply length"
        );
        assert_eq!(
            endpoint.max_tokens, TURN_CEILING,
            "the built-in guard itself never moved"
        );
    }

    /// The ceiling is a default, and a pinned cap outranks a default. A user
    /// who types a number and watches 8192 go out instead has been ignored.
    #[test]
    fn a_pinned_cap_outranks_the_ceiling_even_on_a_host_that_marks() {
        let endpoint = Endpoint {
            max_tokens: 300,
            cap_pinned: true,
            ..local_endpoint()
        };
        endpoint.marks_thinking.store(true, Ordering::SeqCst);
        assert_eq!(endpoint.wire_budget(), 300);
    }

    /// One number per turn, so the Action Log names the one this turn was
    /// sent. Telling a capped user to raise a knob that was not what stopped
    /// them is the failure this row is guarding against.
    #[test]
    fn a_capped_marked_host_logs_the_ceiling_it_was_sent() {
        let endpoint = Endpoint {
            max_tokens: TURN_CEILING,
            ..local_endpoint()
        };
        endpoint.marks_thinking.store(true, Ordering::SeqCst);
        let url = "http://127.0.0.1:1234/v1/chat/completions";

        let Err(Unsent::Truncated(why)) =
            endpoint.reply_from(url, Streamed::Truncated(Truncation::ThinkingOnly))
        else {
            panic!("a budget spent entirely on thinking has nothing to show");
        };
        assert!(
            why.contains("spent all 8192 tokens thinking"),
            "the ceiling is what it burned, not the 1024 it was never sent: {why}"
        );
    }

    /// xAI's `/v1/responses` ends the body with no `[DONE]`, so the end
    /// marker has to be enough on its own. A body that stops with no marker
    /// is half a sentence and must not reach the Speech bubble or the session.
    #[test]
    fn a_marked_end_is_enough_and_an_unmarked_one_is_a_cut() {
        let responses = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"stroll\"}\n\n",
            "data: {\"type\":\"response.completed\"}\n\n",
        );
        assert_eq!(
            streamed(responses),
            Streamed::Complete("stroll".to_string())
        );

        let completions = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"stroll\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        assert_eq!(
            streamed(completions),
            Streamed::Complete("stroll".to_string())
        );

        let cut = "data: {\"choices\":[{\"delta\":{\"content\":\"stroll\\nhey th\"}}]}\n\n";
        assert_eq!(streamed(cut), Streamed::Cut);
    }

    /// A model that spends its whole budget thinking ends with
    /// `finish_reason: "length"` and no content. That is not a reply. `stop`
    /// and `length` are both strings, so the value is what distinguishes them.
    #[test]
    fn an_empty_length_finish_is_a_truncation_and_a_stop_finish_is_a_reply() {
        let spent = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(
            streamed(spent),
            Streamed::Truncated(Truncation::ThinkingOnly),
            "the budget ran out before any text, which is not an empty reply"
        );

        let done = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"stroll\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(
            streamed(done),
            Streamed::Complete("stroll".to_string()),
            "a natural stop is untouched by reading the value"
        );
    }

    /// A cap reached after the model started writing is a truncation too.
    /// A first line that still parses is Parseable, not a half-sentence.
    #[test]
    fn a_length_finish_that_wrote_text_is_a_truncation_too() {
        let clipped = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"stroll\\nhey th\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(
            streamed(clipped),
            Streamed::Truncated(Truncation::Parseable("stroll\nhey th".to_string()))
        );
    }

    /// Best effort is the #606 table, not "any nonempty string". Thinking-only
    /// and unparsable cuts stay Unsent. A parseable Behavior is spoken and
    /// marked. Unmarked content is not Speech and is not heuristically split.
    #[test]
    fn a_truncation_follows_the_best_effort_table() {
        let endpoint = Endpoint {
            max_tokens: TURN_CEILING,
            ..local_endpoint()
        };
        let url = "http://127.0.0.1:1234/v1/chat/completions";

        let thinking = endpoint.reply_from(url, Streamed::Truncated(Truncation::ThinkingOnly));
        let Err(Unsent::Truncated(why)) = thinking else {
            panic!("a budget spent entirely on thinking has nothing to show");
        };
        assert!(
            why.contains("spent all 1024 tokens thinking") && why.contains(MAX_TOKENS),
            "the log line names the cap and the knob: {why}"
        );

        assert_eq!(
            endpoint
                .reply_from(
                    url,
                    Streamed::Truncated(Truncation::Parseable("stroll\nhey th".to_string()))
                )
                .ok(),
            Some(Reply::truncated("stroll\nhey th")),
            "the Behavior is acted on and the words are said, with the mark beside them"
        );

        let mid = endpoint.reply_from(
            url,
            Streamed::Truncated(Truncation::MidSentence("hey th".to_string())),
        );
        let Err(Unsent::Truncated(why)) = mid else {
            panic!("a mid-sentence cut is not spoken");
        };
        assert!(
            why.contains("cut off mid-reply"),
            "the log tells a budget spent thinking from a line that ran out: {why}"
        );

        let unmarked = endpoint.reply_from(
            url,
            Streamed::Truncated(Truncation::Unmarked(
                "Thinking Process:\n\n1.  Analyze the Request:".to_string(),
            )),
        );
        let Err(Unsent::Truncated(why)) = unmarked else {
            panic!("unmarked thinking is not Speech");
        };
        assert!(
            why.contains("unmarked content") && why.contains("not spoken"),
            "the log names the unmarked row: {why}"
        );

        assert_eq!(
            endpoint
                .reply_from(url, Streamed::Complete("stroll".to_string()))
                .ok(),
            Some(Reply::whole("stroll")),
            "a whole reply is not marked"
        );
    }

    /// The four #606 rows are a classification, not a content-emptiness check.
    #[test]
    fn classify_truncation_picks_the_table_row() {
        assert_eq!(
            classify_truncation("", true),
            Truncation::ThinkingOnly,
            "marked reasoning with no answer"
        );
        assert_eq!(
            classify_truncation("\n", false),
            Truncation::ThinkingOnly,
            "a lone newline is still no answer, as in the #598 §6.1 stream"
        );
        assert_eq!(
            classify_truncation("stroll\nhey th", true),
            Truncation::Parseable("stroll\nhey th".to_string())
        );
        assert_eq!(
            classify_truncation("hey th", true),
            Truncation::MidSentence("hey th".to_string()),
            "marked reasoning plus unparsable answer is a cut, not thinking"
        );
        assert_eq!(
            classify_truncation("Thinking Process:\n\n1.  Analyze the Request:", false),
            Truncation::Unmarked("Thinking Process:\n\n1.  Analyze the Request:".to_string()),
            "Qwen whole-body thought-in-content, no reasoning field"
        );
    }

    /// Responses ends a truncated reply with `response.incomplete` rather
    /// than `response.completed`, and sends no `[DONE]` after it. Treat that
    /// event as a truncation, not as a cut connection.
    #[test]
    fn a_responses_incomplete_is_a_truncation_not_a_cut() {
        let sse = concat!(
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"hmm\"}\n\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\
             \"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n",
        );
        assert_eq!(streamed(sse), Streamed::Truncated(Truncation::ThinkingOnly));
    }

    /// vLLM / Ollama / gpt-oss spell the sibling field `reasoning`. Same
    /// empty-length outcome as `reasoning_content`.
    #[test]
    fn a_reasoning_field_length_finish_is_thinking_only() {
        let spent = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning\":\"hmm\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(
            streamed(spent),
            Streamed::Truncated(Truncation::ThinkingOnly)
        );
    }

    /// Anthropic `/v1/messages` names the cap `stop_reason: "max_tokens"`.
    /// Empty text at that stop is ThinkingOnly, never Speech, and the
    /// thinking the budget went on was on the strip while it was being spent
    /// — the second half of #606's thinking-only row.
    #[test]
    fn an_anthropic_max_tokens_stop_is_a_truncation() {
        let thinking_only = concat!(
            "data: {\"type\":\"content_block_start\",\"index\":0,\
             \"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"thinking_delta\",\
             \"thinking\":\"the user\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"thinking_delta\",\
             \"thinking\":\" waved\\nso\"}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n",
        );
        assert_eq!(
            streamed_with_thoughts(thinking_only),
            (
                Streamed::Truncated(Truncation::ThinkingOnly),
                ["the user", "the user waved\nso", ""]
                    .map(str::to_string)
                    .to_vec()
            ),
            "thinking_delta is drawn and is not content, so a cap here is the \
             thinking-only row"
        );

        let parseable = concat!(
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\
             \"text\":\"stroll\\nhey\"}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n",
        );
        assert_eq!(
            streamed(parseable),
            Streamed::Truncated(Truncation::Parseable("stroll\nhey".to_string()))
        );
    }

    /// Whole-body Qwen: thought in `content`, no reasoning field, `length`.
    /// Not Speech, and not a thought-strip heuristic.
    #[test]
    fn an_unmarked_whole_body_length_finish_is_not_spoken() {
        let body = r#"{"choices":[{"message":{"role":"assistant",
            "content":"Thinking Process:\n\n1.  Analyze the Request:"},
            "finish_reason":"length"}]}"#;
        assert!(truncated_body(body));
        assert!(!thought_in_body(body));
        assert_eq!(
            classify_truncation(&content_from_body(body).unwrap(), thought_in_body(body)),
            Truncation::Unmarked("Thinking Process:\n\n1.  Analyze the Request:".to_string())
        );
    }

    /// An endless stream is the only honest test of abandon. A reader that
    /// stopped on its own would prove nothing, and one that drains would
    /// hang this test rather than fail it.
    #[test]
    fn an_abandoned_stream_stops_reading_rather_than_draining() {
        struct Endless;

        impl std::io::Read for Endless {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let frame = b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n";
                buf[..frame.len()].copy_from_slice(frame);
                Ok(frame.len())
            }
        }

        let asked = std::cell::Cell::new(0);
        let abandoned = || {
            asked.set(asked.get() + 1);
            asked.get() > 3
        };
        assert_eq!(
            read_stream(Endless, abandoned, |_| {}).unwrap().0,
            Streamed::Abandoned
        );
    }

    #[test]
    fn a_streaming_request_asks_for_a_stream_and_the_fallback_does_not() {
        let session = [Message {
            role: "user",
            content: "wave".to_string(),
        }];
        let streamed = request_body(
            "gpt-4o-mini",
            &session,
            false,
            TURN_CEILING,
            Wire::Stream,
            false,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert_eq!(streamed["stream"], true);
        let responses = request_body(
            "grok-4.6",
            &session,
            true,
            TURN_CEILING,
            Wire::Stream,
            false,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert_eq!(responses["stream"], true, "the Responses path streams too");

        let whole = request_body(
            "gpt-4o-mini",
            &session,
            false,
            TURN_CEILING,
            Wire::Whole,
            false,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert!(
            whole.get("stream").is_none(),
            "a retry must not name the field the server just refused"
        );
    }

    /// The retry is only honest if the second body drops the effort field.
    #[test]
    fn a_chat_request_asks_for_the_effort_it_is_given_and_the_fallback_does_not() {
        let session = [Message {
            role: "user",
            content: "wave".to_string(),
        }];
        let asked = request_body(
            "gpt-oss-20b",
            &session,
            false,
            TURN_CEILING,
            Wire::Stream,
            true,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert_eq!(asked["reasoning_effort"], "low");

        let dropped = request_body(
            "gpt-oss-20b",
            &session,
            false,
            TURN_CEILING,
            Wire::Stream,
            false,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert!(
            dropped.get("reasoning_effort").is_none(),
            "a retry must not name the field the server just refused"
        );

        let responses = request_body(
            "grok-4.6",
            &session,
            true,
            TURN_CEILING,
            Wire::Whole,
            true,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert!(
            responses.get("reasoning_effort").is_none(),
            "the Responses path spells its effort under `reasoning`"
        );
        assert_eq!(responses["reasoning"]["effort"], "low");
    }

    /// The typed value goes out verbatim on both paths. llama.cpp and oMLX
    /// hand the string to the model's chat template, so validity belongs to
    /// the model file.
    #[test]
    fn a_typed_effort_reaches_both_request_shapes_verbatim() {
        let session = [Message {
            role: "user",
            content: "wave".to_string(),
        }];
        for level in ["medium", "high", "max", "whatever-the-template-takes"] {
            let chat = request_body(
                "gpt-oss-20b",
                &session,
                false,
                TURN_CEILING,
                Wire::Stream,
                true,
                Some(level),
                true,
            );
            assert_eq!(chat["reasoning_effort"], level);

            let responses = request_body(
                "grok-4.6",
                &session,
                true,
                TURN_CEILING,
                Wire::Whole,
                true,
                Some(level),
                true,
            );
            assert_eq!(responses["reasoning"]["effort"], level);
        }
    }

    /// The drop is left as it is and made legible, not silent.
    #[test]
    fn the_retry_line_names_the_effort_value_it_gave_up() {
        assert_eq!(
            dropped_field(Field::Effort, "high"),
            "reasoning_effort=high",
            "the reader has to see which value cost them the field"
        );
        assert_eq!(
            dropped_field(Field::Stream, "high"),
            "stream",
            "the stream field carries no effort value"
        );
    }

    /// The cap is renamed, not dropped. The retry line has to say so, or a
    /// reader looks for an uncapped reply that never went out.
    #[test]
    fn the_retry_line_says_the_cap_is_renamed_rather_than_given_up() {
        assert_eq!(
            dropped_field(Field::Cap, "high"),
            "max_tokens, under max_completion_tokens",
            "the only field here whose retry still carries one"
        );
    }

    /// Blank and whitespace leave the model unset, and the HTTP body leaves
    /// the field off. A typed name is that string, trimmed.
    #[test]
    fn a_blank_model_is_omitted_and_a_named_one_is_sent() {
        let session = [Message {
            role: "user",
            content: "wave".to_string(),
        }];
        with_env(None, None, None, || {
            for blank in ["", "   "] {
                let settings = resolve("https://api.openai.com", blank, Some("sk-test"));
                assert_eq!(settings.model, "", "{blank:?}");
                for responses in [false, true] {
                    let body = request_body(
                        &settings.model,
                        &session,
                        responses,
                        TURN_CEILING,
                        if responses { Wire::Whole } else { Wire::Stream },
                        false,
                        None,
                        true,
                    );
                    assert!(
                        body.get("model").is_none(),
                        "blank must not send model: {blank:?} {body}"
                    );
                }
            }
            let named = resolve("https://api.openai.com", "  grok-4.6  ", Some("sk-test"));
            assert_eq!(named.model, "grok-4.6");
            let chat = request_body(
                &named.model,
                &session,
                false,
                TURN_CEILING,
                Wire::Stream,
                false,
                None,
                true,
            );
            assert_eq!(chat["model"], "grok-4.6");
            let responses = request_body(
                &named.model,
                &session,
                true,
                TURN_CEILING,
                Wire::Whole,
                false,
                None,
                true,
            );
            assert_eq!(responses["model"], "grok-4.6");
        });
        with_env(None, None, Some("   "), || {
            let settings = resolve("https://api.openai.com", "grok-4.6", Some("sk-test"));
            assert_eq!(
                settings.model, "",
                "whitespace in the env is unset, not the file's name"
            );
        });
        with_env(None, None, Some("gpt-4o"), || {
            let settings = resolve("https://api.openai.com", "grok-4.6", Some("sk-test"));
            assert_eq!(settings.model, "gpt-4o");
        });
    }

    /// Blank and whitespace omit the field on both HTTP shapes. A picked
    /// level is that string, not a default substituted for empty.
    #[test]
    fn an_unset_effort_omits_the_field_and_each_level_is_sent() {
        let session = [Message {
            role: "user",
            content: "wave".to_string(),
        }];
        tests::with_env(None, None, None, || {
            for blank in ["", "   "] {
                crate::dev_flags::seed(&crate::settings::Settings {
                    director_reasoning_effort: blank.to_string(),
                    ..crate::settings::Settings::default()
                });
                assert_eq!(effort_for(), None, "{blank:?}");
                let level = effort_for();
                let chat = request_body(
                    "gpt-oss-20b",
                    &session,
                    false,
                    TURN_CEILING,
                    Wire::Stream,
                    true,
                    level.as_deref(),
                    true,
                );
                assert!(
                    chat.get("reasoning_effort").is_none(),
                    "blank must not send reasoning_effort: {chat}"
                );
                let responses = request_body(
                    "grok-4.6",
                    &session,
                    true,
                    TURN_CEILING,
                    Wire::Whole,
                    true,
                    level.as_deref(),
                    true,
                );
                assert!(
                    responses.get("reasoning").is_none(),
                    "blank must not send reasoning.effort: {responses}"
                );
                assert!(responses.get("reasoning_effort").is_none());
            }

            for level in ["low", "medium", "high"] {
                crate::dev_flags::seed(&crate::settings::Settings {
                    director_reasoning_effort: level.to_string(),
                    ..crate::settings::Settings::default()
                });
                assert_eq!(effort_for().as_deref(), Some(level));
                let sent = effort_for();
                let chat = request_body(
                    "gpt-oss-20b",
                    &session,
                    false,
                    TURN_CEILING,
                    Wire::Stream,
                    true,
                    sent.as_deref(),
                    true,
                );
                assert_eq!(chat["reasoning_effort"], level);
                let responses = request_body(
                    "grok-4.6",
                    &session,
                    true,
                    TURN_CEILING,
                    Wire::Whole,
                    true,
                    sent.as_deref(),
                    true,
                );
                assert_eq!(responses["reasoning"]["effort"], level);
                assert!(responses.get("reasoning_effort").is_none());
            }
            crate::dev_flags::seed(&crate::settings::Settings::default());
        });
    }

    /// o-series models refuse `max_tokens`, but it is the only name Ollama
    /// reads. The rename is what a refusal buys, not the shape every request
    /// opens with.
    #[test]
    fn a_chat_request_names_the_cap_the_old_way_until_a_host_refuses_it() {
        let session = [Message {
            role: "user",
            content: "wave".to_string(),
        }];
        let asked = request_body(
            "gpt-4o-mini",
            &session,
            false,
            TURN_CEILING,
            Wire::Stream,
            false,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert_eq!(asked["max_tokens"], 1024);
        assert!(asked.get("max_completion_tokens").is_none());

        let renamed = request_body(
            "o3-mini",
            &session,
            false,
            TURN_CEILING,
            Wire::Stream,
            false,
            Some(DEFAULT_EFFORT),
            false,
        );
        assert_eq!(
            renamed["max_completion_tokens"], 1024,
            "the retry still carries a cap, under the name the spec prefers"
        );
        assert!(
            renamed.get("max_tokens").is_none(),
            "a retry must not name the field the server just refused"
        );

        let responses = request_body(
            "grok-4.6",
            &session,
            true,
            TURN_CEILING,
            Wire::Whole,
            false,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert_eq!(
            responses["max_output_tokens"], 1024,
            "the Responses path spells the cap its own way and is not guarded"
        );
    }

    #[test]
    fn a_responses_request_uses_input_and_does_not_store() {
        let session = [Message {
            role: "user",
            content: "wave".to_string(),
        }];
        let body = request_body(
            "grok-4.6",
            &session,
            true,
            TURN_CEILING,
            Wire::Whole,
            false,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert_eq!(body["input"], "wave");
        assert_eq!(body["max_output_tokens"], 1024);
        assert_eq!(body["store"], false);
        assert_eq!(body["reasoning"]["effort"], "low");
        assert!(body.get("messages").is_none());
    }

    #[test]
    fn a_follow_up_responses_request_sends_the_session() {
        let session = [
            Message {
                role: "user",
                content: "hello".to_string(),
            },
            Message {
                role: "assistant",
                content: "wave".to_string(),
            },
            Message {
                role: "user",
                content: "what just happened: thrown".to_string(),
            },
        ];
        let body = request_body(
            "grok-4.6",
            &session,
            true,
            TURN_CEILING,
            Wire::Whole,
            false,
            Some(DEFAULT_EFFORT),
            true,
        );
        assert_eq!(body["input"][2]["content"], "what just happened: thrown");
        assert!(body["input"].is_array());
    }

    #[test]
    fn the_completions_url_joins_a_base_without_doubling_v1() {
        assert_eq!(
            completions_url("https://api.openai.com"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            completions_url("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/chat/completions"
        );
        assert_eq!(
            completions_url("http://localhost:11434/v1"),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            completions_url("http://localhost:11434/v1/chat/completions"),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            completions_url("https://api.x.ai"),
            "https://api.x.ai/v1/responses"
        );
        assert_eq!(
            completions_url("https://api.x.ai/v1"),
            "https://api.x.ai/v1/responses"
        );
        assert_eq!(
            completions_url("https://api.x.ai/v1/chat/completions"),
            "https://api.x.ai/v1/chat/completions",
            "an explicit legacy path is honoured"
        );
        assert_eq!(
            completions_url("https://mtls.api.x.ai"),
            "https://mtls.api.x.ai/v1/responses"
        );
    }

    #[test]
    fn a_quoted_key_is_trimmed() {
        assert_eq!(trim_key("  sk-abc\n").as_deref(), Some("sk-abc"));
        assert_eq!(trim_key("\"sk-abc\"").as_deref(), Some("sk-abc"));
        assert_eq!(trim_key("   ").as_deref(), None);
    }

    /// Unset is a choice. `$XAI_API_KEY` expanding to nothing is a mistake.
    /// The log has to tell them apart.
    #[test]
    fn a_blank_provided_key_is_invalid_and_unset_is_not() {
        assert_eq!(key_from_raw(None), KeyRead::Unset);
        assert_eq!(key_from_raw(Some("")), KeyRead::Invalid);
        assert_eq!(key_from_raw(Some("  \n")), KeyRead::Invalid);
        assert_eq!(key_from_raw(Some("\"\"")), KeyRead::Invalid);
        assert!(matches!(key_from_raw(Some("sk-abc")), KeyRead::Present(_)));
    }

    #[test]
    fn startup_always_names_the_director_mode() {
        let static_only = DirectorConfig {
            enabled: false,
            configured: false,
            env_says: None,
            key_invalid: false,
            wake_every: WAKE_EVERY,
            ambient_first: Pace::FIRST,
            proactive_allowed: true,
        };
        assert_eq!(startup_lines(&static_only), ["director: StaticDirector"]);

        let blank = DirectorConfig {
            key_invalid: true,
            ..static_only.clone()
        };
        let warned = startup_lines(&blank);
        assert!(
            warned.iter().any(|line| line.contains("warning")
                && line.contains(API_KEY)
                && line.contains("StaticDirector")),
            "{warned:?}"
        );
        assert!(
            warned.iter().any(|line| line == "director: StaticDirector"),
            "{warned:?}"
        );

        let model = DirectorConfig {
            enabled: true,
            configured: true,
            env_says: None,
            key_invalid: false,
            wake_every: WAKE_EVERY,
            ambient_first: Duration::from_secs(45),
            proactive_allowed: true,
        };
        assert_eq!(
            startup_lines(&model),
            ["director: model, ambient first 45s"]
        );

        let off = DirectorConfig {
            enabled: false,
            configured: true,
            env_says: None,
            key_invalid: false,
            wake_every: WAKE_EVERY,
            ambient_first: Pace::FIRST,
            proactive_allowed: true,
        };
        assert_eq!(startup_lines(&off), ["director: off; using StaticDirector"]);
    }

    #[test]
    fn xai_has_a_legacy_alternate_and_openai_does_not() {
        assert_eq!(
            alternate_url("https://api.x.ai/v1/responses").as_deref(),
            Some("https://api.x.ai/v1/chat/completions")
        );
        assert_eq!(
            alternate_url("https://api.openai.com/v1/chat/completions"),
            None
        );
    }

    #[test]
    fn a_server_that_rejects_the_stream_field_earns_one_whole_retry() {
        assert_eq!(
            refused_field(
                400,
                r#"{"error":{"message":"Unrecognized request argument supplied: stream"}}"#
            ),
            Some(Field::Stream)
        );
        assert_eq!(
            refused_field(
                422,
                r#"{"detail":[{"loc":["body","stream"],"msg":"extra fields not permitted"}]}"#
            ),
            Some(Field::Stream),
            "a strict server validates the body rather than the field"
        );
        assert_eq!(
            refused_field(400, r#"{"error":{"message":"model gpt-9 does not exist"}}"#),
            None,
            "a 400 about anything else would fail the same way twice"
        );
        assert_eq!(
            refused_field(403, "streaming is not available"),
            None,
            "403 is the key, the credits, or a path ACL; fallback_url owns that"
        );
        assert_eq!(
            refused_field(400, "upstream connect error or disconnect/reset"),
            None,
            "a gateway saying upstream is not a server naming the stream field"
        );
        assert_eq!(
            refused_field(400, r#"{"error":"streaming is not supported here"}"#),
            Some(Field::Stream),
            "the word can still be inflected, it just cannot be a suffix"
        );
    }

    /// A body that names `reasoning_effort` is the only thing that drops it.
    /// Reading a plain 400 as a refusal costs a second POST that fails the
    /// same way.
    #[test]
    fn a_server_that_rejects_the_effort_field_earns_one_retry_without_it() {
        assert_eq!(
            refused_field(
                400,
                r#"{"error":{"message":"Unrecognized request argument supplied: reasoning_effort"}}"#
            ),
            Some(Field::Effort)
        );
        assert_eq!(
            refused_field(
                400,
                r#"{"error":{"message":"Unsupported parameter: 'reasoning_effort' is not supported with this model."}}"#
            ),
            Some(Field::Effort),
            "OpenAI's wording for a field a non-reasoning model will not take"
        );
        assert_eq!(
            refused_field(
                422,
                r#"{"detail":[{"loc":["body","reasoning_effort"],"msg":"extra fields not permitted"}]}"#
            ),
            Some(Field::Effort)
        );
        assert_eq!(
            refused_field(400, r#"{"error":{"message":"model gpt-9 does not exist"}}"#),
            None,
            "a 400 about anything else would fail the same way twice"
        );
        assert_eq!(
            refused_field(403, "reasoning_effort is not available"),
            None,
            "403 is the key, the credits, or a path ACL; fallback_url owns that"
        );
        assert_eq!(
            refused_field(
                400,
                r#"{"error":{"message":"unknown fields: reasoning_effort, stream"}}"#
            ),
            Some(Field::Effort),
            "a validator listing both drops the field this request added first"
        );
    }

    /// The o-series refusal names the field it will not take and the field it
    /// wants instead, two names that differ by an infix. Reading the new name
    /// as the old one would rename a body that is already renamed.
    #[test]
    fn a_server_that_rejects_max_tokens_earns_one_retry_under_the_new_name() {
        assert_eq!(
            refused_field(
                400,
                r#"{"error":{"message":"Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.","param":"max_tokens","code":"unsupported_parameter"}}"#
            ),
            Some(Field::Cap),
            "OpenAI's wording for an o-series model, verbatim"
        );
        assert_eq!(
            refused_field(
                400,
                r#"{"error":{"message":"Unsupported parameter: 'max_completion_tokens' is not supported."}}"#
            ),
            None,
            "the new name is not the old one wearing a prefix"
        );
        assert_eq!(
            refused_field(
                422,
                r#"{"detail":[{"loc":["body","max_tokens"],"msg":"extra fields not permitted"}]}"#
            ),
            Some(Field::Cap)
        );
        assert_eq!(
            refused_field(400, r#"{"error":{"message":"model gpt-9 does not exist"}}"#),
            None,
            "a 400 about anything else would fail the same way twice"
        );
        assert_eq!(
            refused_field(
                400,
                r#"{"error":{"message":"unknown fields: max_tokens, reasoning_effort"}}"#
            ),
            Some(Field::Cap),
            "a validator listing several concedes the cap first: the retry still has one"
        );
    }

    #[test]
    fn a_broken_stream_is_worth_a_retry_but_teaches_nothing() {
        assert_eq!(
            Unsent::Refused(Field::Stream, "names the field".to_string()).retry_settles(),
            Some((Field::Stream, true)),
            "a host that rejected the field will reject it on the next wake too"
        );
        assert_eq!(
            Unsent::Refused(Field::Effort, "names the field".to_string()).retry_settles(),
            Some((Field::Effort, true)),
            "and the effort field is remembered the same way"
        );
        assert_eq!(
            Unsent::Refused(Field::Cap, "names the field".to_string()).retry_settles(),
            Some((Field::Cap, true)),
            "and so is the name a host will take its cap under"
        );
        assert_eq!(
            Unsent::Cut("ended mid-reply".to_string()).retry_settles(),
            Some((Field::Stream, false)),
            "the answer is still owed, but one dropped body is no verdict on the host"
        );
        assert_eq!(
            Unsent::Failed("503".to_string()).retry_settles(),
            None,
            "dropping the stream field will not revive a server that is down"
        );
        assert_eq!(
            Unsent::Abandoned.retry_settles(),
            None,
            "nobody is waiting for a second attempt at a superseded call"
        );
        assert_eq!(
            Unsent::Truncated("512 tokens".to_string()).retry_settles(),
            None,
            "the same question at the same cap gets the same nothing"
        );
    }

    #[test]
    fn responses_falls_back_only_when_refused() {
        let url = "https://api.x.ai/v1/responses";
        assert!(fallback_url(url, "https://api.x.ai/v1/responses: status 403 {}").is_some());
        assert!(fallback_url(url, "https://api.x.ai/v1/responses: status 404").is_some());
        assert!(fallback_url(url, "https://api.x.ai/v1/responses: status 401").is_none());
        assert!(fallback_url(url, "https://api.x.ai/v1/responses: status 400").is_none());
    }

    /// The suffix test is the half that matters for a lookalike name. The
    /// normalised host ignores port and spelling, so `:443` or capitals still
    /// count as xAI.
    #[test]
    fn only_xais_own_hosts_answer_to_its_inference_path() {
        assert!(host_is_xai("https://api.x.ai/v1/responses"));
        assert!(host_is_xai("https://mtls.api.x.ai"), "a real subdomain");
        assert!(
            host_is_xai("https://api.x.ai:443/v1"),
            "an explicit port is not part of the name"
        );
        assert!(
            host_is_xai("https://API.X.AI/v1"),
            "nor is how the row was capitalised"
        );
        assert!(
            !host_is_xai("https://evil-api.x.ai"),
            "the dot is what makes it a subdomain"
        );
        assert!(
            !host_is_xai("https://api.x.ai.evil.com"),
            "a name that opens with theirs"
        );
        assert!(
            !host_is_xai("https://evil.com/api.x.ai"),
            "theirs in the path, not the host"
        );
        assert!(!host_is_xai("api.x.ai"), "no scheme, no host");
    }

    /// Production change that would fail this: cutting at the first `/` after
    /// the scheme, which keeps userinfo and puts a password in a trace line.
    #[test]
    fn an_origin_keeps_the_port_and_drops_the_credentials() {
        assert_eq!(
            origin("http://localhost:11434/v1"),
            "http://localhost:11434"
        );
        assert_eq!(
            origin("https://api.openai.com/v1"),
            "https://api.openai.com"
        );
        assert_eq!(
            origin("https://user:sk-secret@api.x.ai/v1/responses"),
            "https://api.x.ai"
        );
        assert_eq!(
            origin("https://api.openai.com:443/v1"),
            "https://api.openai.com",
            "the default port is not part of the origin"
        );
        assert_eq!(
            origin("localhost:11434"),
            "localhost:11434",
            "no host to serialize, so the value is handed back for the probe \
             to fail on and name"
        );
    }

    /// A query that mentions the path is not the path.
    #[test]
    fn the_responses_path_is_the_path_and_not_the_query() {
        assert!(uses_responses("https://api.x.ai/v1/responses"));
        assert!(!uses_responses("https://api.x.ai/v1/chat/completions"));
        assert!(!uses_responses(
            "https://api.x.ai/v1/chat/completions?from=/responses"
        ));
    }

    /// What the Chat header is handed. A password written into the row must
    /// not reach the window.
    #[test]
    fn a_host_is_named_without_its_credentials_or_its_path() {
        assert_eq!(host_of("https://api.openai.com/v1"), "api.openai.com");
        assert_eq!(host_of("http://localhost:8000"), "localhost:8000");
        assert_eq!(host_of("http://[fd00::1]:8080"), "[fd00::1]:8080");
        assert_eq!(
            host_of("https://user:sk-secret@api.openai.com/v1"),
            "api.openai.com",
            "neither half of the userinfo is drawn"
        );
        assert_eq!(
            host_of("http://10.0.0.1@172.16.evil.com/"),
            "172.16.evil.com",
            "the digits are userinfo; the host is evil.com"
        );
    }

    /// A base with no scheme is not a URL `completions_url` can concatenate
    /// onto, so it names no host and is not local. Remote keeps the key
    /// required.
    #[test]
    fn a_base_with_no_scheme_names_no_host_and_is_not_local() {
        assert_eq!(host_of("localhost:8000"), "");
        assert_eq!(host_of(""), "");
        assert!(!is_local("localhost:8000"));
        assert!(!is_local("api.openai.com"));
    }

    #[test]
    fn a_loopback_or_private_base_is_served_from_here() {
        assert!(is_local("http://localhost:11434"), "Ollama");
        assert!(is_local("http://127.0.0.1:8080"), "llama.cpp");
        assert!(is_local("http://[::1]:1234"), "LM Studio over IPv6");
        assert!(is_local("http://192.168.1.50:8000"), "a box on the LAN");
        assert!(is_local("http://10.0.0.5:8000"));
        assert!(is_local("http://172.16.4.2:8080"));
        assert!(is_local("http://studio.local:1234"), "mDNS");
        assert!(is_local("http://[fd00::1]:8080"), "an IPv6 private address");
        assert!(!is_local("https://api.openai.com"));
        assert!(!is_local("https://api.x.ai"));
        assert!(
            !is_local("http://172.32.0.1:8080"),
            "just outside the private range"
        );
        assert!(
            !is_local("https://localhost.example.com"),
            "a hostname that merely starts with localhost"
        );
    }

    #[test]
    fn a_remote_host_wearing_an_address_is_still_remote() {
        // Picking the numbers out of a name would read every one of these as
        // a machine on this LAN, and hand it a keyless Character Prompt.
        assert!(!is_local("http://10.0.0.5.evil.com:8080"));
        assert!(!is_local("http://192.168.1.1.attacker.net"));
        assert!(
            !is_local("http://api.10.0.0.5.example.com"),
            "digits in the middle of the name"
        );
        assert!(
            !is_local("http://10.0.0.1@172.16.evil.com/"),
            "the digits are userinfo; the host is evil.com"
        );
        assert!(
            is_local("http://user@10.0.0.1"),
            "userinfo before a real one"
        );
    }

    #[test]
    fn a_host_is_matched_however_it_is_spelled() {
        assert!(is_local("http://LOCALHOST:11434"));
        assert!(is_local("http://Localhost"));
        assert!(is_local("http://STUDIO.LOCAL:1234"));
        assert!(is_local("http://localhost.:11434"), "fully qualified");
    }

    /// The env outranks the file for these two, the same precedence `resolve`
    /// gives the endpoint. Each export needs a re-seed to reach a read site.
    #[test]
    fn an_exported_limit_outranks_the_persisted_one() {
        with_env(None, None, None, || {
            let file = crate::settings::Settings {
                director_timeout_secs: "45".into(),
                director_max_tokens: "300".into(),
                ..Default::default()
            };
            crate::dev_flags::seed(&file);
            assert_eq!(timeout_for(), Duration::from_secs(45));
            assert_eq!(max_tokens_for(), 300);

            std::env::set_var(TIMEOUT_SECS, "7");
            std::env::set_var(MAX_TOKENS, "11");
            crate::dev_flags::seed(&file);
            assert_eq!(timeout_for(), Duration::from_secs(7));
            assert_eq!(max_tokens_for(), 11);
            std::env::remove_var(TIMEOUT_SECS);
            std::env::remove_var(MAX_TOKENS);
        });
    }

    /// The pin travels with the endpoint rather than being re-read at the
    /// send, so a marked host built while a cap was exported still honours
    /// it. `completer_retargets` rebuilds the endpoint when the setting
    /// changes, which is how `effort` and `timeout` stay fresh too.
    #[test]
    fn an_exported_cap_pins_the_endpoint_against_the_ceiling() {
        with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            let loose = endpoint_from(&resolve("http://localhost:11434", "gemma4", None))
                .expect("a local endpoint needs no key");
            loose.marks_thinking.store(true, Ordering::SeqCst);
            assert_eq!(
                loose.wire_budget(),
                8192,
                "nothing pinned, so a host that marks gets the ceiling"
            );

            std::env::set_var(MAX_TOKENS, "300");
            crate::dev_flags::seed(&crate::settings::Settings::default());
            let pinned = endpoint_from(&resolve("http://localhost:11434", "gemma4", None))
                .expect("a local endpoint needs no key");
            pinned.marks_thinking.store(true, Ordering::SeqCst);
            assert_eq!(
                pinned.wire_budget(),
                300,
                "the number the user typed is an instruction, not a default"
            );

            std::env::remove_var(MAX_TOKENS);
            crate::dev_flags::seed(&crate::settings::Settings::default());
        });
    }

    /// A wait no source names is `Pace::FIRST`, not zero seconds.
    #[test]
    fn an_exported_wake_interval_outranks_the_persisted_one() {
        with_env(None, None, None, || {
            let file = crate::settings::Settings {
                director_wake_secs: "300".into(),
                ..Default::default()
            };
            crate::dev_flags::seed(&file);
            assert_eq!(ambient_first(), Duration::from_secs(300));

            std::env::set_var(WAKE_SECS, "30");
            crate::dev_flags::seed(&file);
            assert_eq!(ambient_first(), Duration::from_secs(30));
            std::env::remove_var(WAKE_SECS);

            crate::dev_flags::seed(&crate::settings::Settings::default());
            assert_eq!(ambient_first(), Pace::FIRST);
        });
    }

    /// One guard for every wake and every host. Nothing about the surface
    /// the reply lands on, or about where the server runs, moves this number.
    /// `THINK_CEILING` and a pinned cap are the only two things that do, and
    /// each has its own test above. Under the env lock because
    /// `max_tokens_for` reads the live `dev_flags` values another test in
    /// this binary sets and clears.
    #[test]
    fn one_runaway_guard_caps_every_wake_on_every_host() {
        with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            assert_eq!(timeout_for(), TIMEOUT, "timeout no longer splits on URL");
            assert_eq!(max_tokens_for(), 1024, "and the cap no longer does either");

            for base in ["http://localhost:11434", "https://api.openai.com"] {
                let endpoint = endpoint_from(&resolve(base, "gemma4", Some("sk-test")))
                    .expect("a key was given");
                assert_eq!(endpoint.wire_budget(), 1024, "{base}");
            }
        });
    }

    #[test]
    fn the_preflight_passes_when_the_server_lists_the_model() {
        let body = r#"{"data":[{"id":"llama3.2:latest"},{"id":"qwen3:8b"}]}"#;
        let ok = Ok((200, body.to_string()));
        assert!(preflight_verdict(ok.clone(), "qwen3:8b").is_ok());
        assert!(
            preflight_verdict(ok, "llama3.2").is_ok(),
            "Ollama reports a :latest tag the user does not type"
        );
    }

    #[test]
    fn the_preflight_names_why_it_did_not_pass() {
        let down = preflight_verdict(Err("connection refused".to_string()), "llama3.2");
        assert!(down.unwrap_err().contains("connection refused"));

        let refused = preflight_verdict(Ok((404, String::new())), "llama3.2");
        assert!(refused.unwrap_err().contains("404"));

        let missing = preflight_verdict(
            Ok((200, r#"{"data":[{"id":"qwen3:8b"}]}"#.to_string())),
            "llama3.2",
        );
        let missing = missing.unwrap_err();
        assert!(missing.contains("llama3.2"), "{missing}");
        assert!(missing.contains("qwen3:8b"), "names what is served");
    }

    #[test]
    fn a_body_this_cannot_read_is_left_alone() {
        // MLX and some llama.cpp builds answer without a `data` list. A probe
        // that cannot see the model must not claim it is absent.
        assert!(preflight_verdict(Ok((200, "not json".to_string())), "any").is_ok());
        assert!(preflight_verdict(Ok((200, r#"{"models":["a"]}"#.to_string())), "any").is_ok());
    }

    #[test]
    fn a_server_serving_nothing_says_so() {
        // Ollama with nothing pulled answers 200 with an empty list. That is
        // knowable, and the reason the fidget is about to stay quiet.
        let empty = preflight_verdict(Ok((200, r#"{"data":[]}"#.to_string())), "gemma4");
        assert!(empty.unwrap_err().contains("serving no models"));
    }

    #[test]
    fn a_status_error_keeps_the_body() {
        let error = status_error("https://api.x.ai/v1/responses", 403, " {\"error\":\"no\"} ");
        assert!(error.contains("status 403"));
        assert!(error.contains("\"error\":\"no\""));
    }

    #[test]
    fn a_present_key_is_used_even_when_the_base_is_local() {
        with_env(None, None, None, || {
            let settings = resolve(
                "http://localhost:8000",
                "local-model",
                Some("omlx-test-key"),
            );
            assert!(
                is_local(&settings.base_url),
                "precondition: the base is local"
            );
            let endpoint = endpoint_from(&settings).expect("local is configured");
            assert_eq!(
                endpoint.key_fingerprint(),
                key_fingerprint("omlx-test-key"),
                "a present key must not be dropped for a local base"
            );
        });
    }

    /// How the prompt phrasings differ. The personality file itself is never
    /// touched. Only the frame around the sample lines moves.
    #[derive(Clone, Copy, Debug)]
    enum Framing {
        /// The prompt as shipped. Personality first, format instruction after.
        Today,
        /// The quoted lines named as voice rather than as a reply to imitate.
        Framed,
        /// The personality moved below the format instruction.
        After,
    }

    /// The last line of the format instruction, and the seam `After` cuts on.
    const FORMAT_ENDS: &str = "Propose nothing else.\n";

    const VOICE_NOTE: &str = "Those quoted lines are how this character sounds, \
        not a format to copy: your own reply still begins with a behavior name.";

    /// `prompt` said under `framing`. A rewrite of the built prompt rather
    /// than a second builder, so the harness cannot drift from production.
    /// A later turn carries no Personality Prompt and comes back untouched.
    fn reframed(prompt: &str, personality: &str, framing: Framing) -> String {
        // An empty Personality Prompt renders as "(no personality)", which
        // `strip_prefix("")` would happily match and then frame as a voice.
        if personality.is_empty() {
            return prompt.to_string();
        }
        let Some(rest) = prompt.strip_prefix(personality) else {
            return prompt.to_string();
        };
        match framing {
            Framing::Today => prompt.to_string(),
            Framing::Framed => format!("{personality}\n\n{VOICE_NOTE}{rest}"),
            Framing::After => match rest.split_once(FORMAT_ENDS) {
                Some((head, tail)) => {
                    format!("{}{FORMAT_ENDS}\n{personality}\n{tail}", head.trim_start())
                }
                None => prompt.to_string(),
            },
        }
    }

    /// The quoted sample lines a Personality Prompt offers, per #156's
    /// convention ("It has been heard to say: …").
    ///
    /// ponytail: quote-character parity rather than the "heard to say" anchor,
    /// which black-mage already words differently. It costs nothing and holds
    /// for all eight shipped personalities; one unpaired quote in a future one
    /// would invert it and the harness would report a clean zero. Anchor on
    /// the colon if a personality ever needs a lone quote character.
    fn sample_lines(personality: &str) -> Vec<&str> {
        personality
            .split(['"', '\u{201c}', '\u{201d}'])
            .skip(1)
            .step_by(2)
            .collect()
    }

    /// Below this many squashed characters a quoted string is too short to
    /// be a sample line: `"Mad Cat"` appears inside timber-wolf's prose, and
    /// matching it would count any reply that used the name.
    const QUOTE_FLOOR: usize = 12;

    /// The sample line `reply` said back, when it said one back.
    ///
    /// ponytail: the reply containing a whole sample line, compared over
    /// squashed text, rather than an edit distance. It catches the line
    /// repunctuated, recased, or wrapped in a preamble, which is what #230
    /// saw; a model that truncates or paraphrases it reads as prose and is
    /// not counted. So the number is a floor on quoting, never an inflated
    /// one — reach for a similarity measure only if the paraphrases matter.
    fn quoted_sample_line(reply: &str, personality: &str) -> Option<String> {
        let said = squashed(reply);
        sample_lines(personality)
            .into_iter()
            .find(|line| {
                let sample = squashed(line);
                sample.len() >= QUOTE_FLOOR && said.contains(&sample)
            })
            .map(str::to_string)
    }

    fn squashed(text: &str) -> String {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Sends every wake under one phrasing. A Completer decorator, so
    /// `ModelDirector` still builds the prompt and still classifies the reply.
    /// The comparison changes the wording and nothing else.
    struct Reframing<C> {
        inner: C,
        personality: String,
        framing: Framing,
    }

    impl<C: Completer> Completer for Reframing<C> {
        fn complete(&self, request: &WakeRequest) -> Result<Reply, String> {
            let mut sent = request.clone();
            sent.prompt = reframed(&sent.prompt, &self.personality, self.framing);
            self.inner.complete(&sent)
        }
    }

    /// A Completer that fails every turn, for a test that only needs the
    /// prompt `ModelDirector` builds.
    struct Silent;

    impl Completer for Silent {
        fn complete(&self, _request: &WakeRequest) -> Result<Reply, String> {
            Err("no server here".to_string())
        }
    }

    /// A personality in the shipped sample-line shape, small enough to reason about.
    const TWO_SAMPLES: &str = "Cat claimed the desktop.\n\nIt has been heard to say: \
        \"What is that one? Show me.\" - \"You may continue.\"";

    /// The quote counter has to recognise a sample line said back with the
    /// model's own punctuation, and refuse a fragment short enough to turn
    /// up in any sentence.
    #[test]
    fn a_sample_line_said_back_is_recognised_however_it_is_punctuated() {
        let personality = TWO_SAMPLES;

        assert_eq!(
            quoted_sample_line("What is that one? Show me.", personality).as_deref(),
            Some("What is that one? Show me.")
        );
        assert_eq!(
            quoted_sample_line("what is that one - show me!!", personality).as_deref(),
            Some("What is that one? Show me."),
            "the punctuation is the model's, not the personality's"
        );
        assert_eq!(
            quoted_sample_line("Hmm. What is that one? Show me.", personality).as_deref(),
            Some("What is that one? Show me."),
            "a sample line with a preamble is still a quote"
        );
    }

    #[test]
    fn prose_of_its_own_is_not_counted_as_a_quote() {
        let personality = TWO_SAMPLES;

        assert_eq!(
            quoted_sample_line("I supervised that compile and I approve.", personality),
            None
        );
        assert_eq!(
            quoted_sample_line("Show me.", personality),
            None,
            "part of a sample line is the model's own sentence, not a quote"
        );
        assert_eq!(
            quoted_sample_line(
                "What is that one? Show me.",
                "A personality with no samples."
            ),
            None
        );
        assert_eq!(
            quoted_sample_line(
                "It is a \"Mad Cat\" and I caught it.",
                "It is a \"Mad Cat\"."
            ),
            None,
            "a quoted string too short to be a sample line is a coincidence"
        );
    }

    /// Against the shipped file, not a fixture. The convention separates the
    /// quoted lines with em dashes and wraps them mid-sentence, and extraction
    /// that failed on that would report a clean zero rather than an error.
    #[test]
    fn every_sample_line_the_shipped_cat_offers_is_recognised_verbatim() {
        let personality = include_str!("../../characters/cat/personality.txt");
        let lines = sample_lines(personality);

        assert!(
            !lines.is_empty(),
            "the cat still offers sample lines (#156)"
        );
        for line in lines {
            assert_eq!(
                quoted_sample_line(line, personality).as_deref(),
                Some(line),
                "a shipped sample line said back verbatim has to count"
            );
        }
    }

    #[test]
    fn framing_moves_the_personality_and_leaves_the_rest_alone() {
        let personality =
            "Cat claimed the desktop. It has been heard to say: \"Show me that one.\"";
        let director = ModelDirector::new(Silent, ["stroll", "nap"], "fidget", "Cat", false);
        let today = director.prompt(&Context {
            personality: personality.to_string(),
            happened: Happened::Poke,
            standing: "the display floor".to_string(),
            ..crate::completer::tests::wake_context()
        });

        assert_eq!(reframed(&today, personality, Framing::Today), today);

        let framed = reframed(&today, personality, Framing::Framed);
        assert!(framed.starts_with(personality), "the voice still opens");
        assert!(
            framed.contains("not a format to copy"),
            "the sample lines are marked as voice: {framed}"
        );

        let after = reframed(&today, personality, Framing::After);
        assert!(!after.starts_with(personality), "the voice no longer opens");
        assert!(
            after.find(FORMAT_ENDS) < after.find(personality),
            "and it now follows the format instruction: {after}"
        );
    }

    #[test]
    fn a_later_turn_carries_no_personality_and_is_left_alone() {
        let follow_up = "what just happened: poked\nrecent: (none)\n";
        for framing in [Framing::Today, Framing::Framed, Framing::After] {
            assert_eq!(
                reframed(follow_up, "Cat claimed the desktop.", framing),
                follow_up,
                "{framing:?} rewrote a turn that carries no personality"
            );
        }
    }

    /// How often a live local model breaks the reply contract. Ignored because
    /// it needs a server and spends real seconds; it is the harness, not a
    /// check of our own code.
    ///
    /// The classifier is `ModelDirector::wake` itself rather than a copy, so
    /// the measurement cannot drift from what the app does. One session
    /// throughout, because that is how the fidget runs. #175.
    ///
    /// ```sh
    /// FIDGET_DIRECTOR_BASE_URL=http://localhost:11434 \
    /// FIDGET_DIRECTOR_MODEL=gemma4 \
    /// cargo test -p fidget measure_the_reply_contract -- --ignored --nocapture
    /// ```
    ///
    /// `FIDGET_BENCH_WAKES` sets the sample size; it defaults to 40.
    /// `FIDGET_BENCH_FRAMING` picks the phrasing — `today` (the default),
    /// `framed`, or `after` — and the run reports how much of its prose was a
    /// personality sample line quoted back.
    #[test]
    #[ignore]
    fn measure_the_reply_contract_failure_rate() {
        use fidget_core::director::{Context, Happened, ModelDirector, Wake};
        use fidget_core::engine::State;
        use fidget_core::sensing::Activity;
        use std::path::Path;
        use std::time::{Instant, SystemTime};

        // Forty tells 5% from 50%. It does not tell 5% from 8%. Nothing
        // pins `temperature` or a seed, because the app sends neither.
        // Raise it when a tighter number is worth the minutes.
        let wakes: usize = std::env::var("FIDGET_BENCH_WAKES")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .filter(|&n: &usize| n > 0)
            .unwrap_or(40);

        // For the exported timeout, as `run_probe` does.
        crate::dev_flags::seed(&crate::settings::Settings::default());
        let endpoint = endpoint().expect("FIDGET_DIRECTOR_BASE_URL and _MODEL in the env");
        let model = endpoint.model().to_string();
        let origin = endpoint.origin();

        // A real shipped package, so the prompt is the one production sends:
        // its Personality Prompt and its declared Behavior names.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../characters/cat");
        let files = crate::package::read(&root).expect("the shipped cat package reads");
        let cat = fidget_core::character::load(&files).expect("and loads");
        let behaviors: Vec<String> = cat.behaviors.keys().cloned().collect();

        let framing = match std::env::var("FIDGET_BENCH_FRAMING")
            .unwrap_or_default()
            .as_str()
        {
            "framed" => Framing::Framed,
            "after" => Framing::After,
            _ => Framing::Today,
        };
        let director = ModelDirector::new(
            Reframing {
                inner: endpoint,
                personality: cat.personality.clone(),
                framing,
            },
            behaviors.clone(),
            "fidget",
            cat.name.clone(),
            false,
        );

        // Vary the wake so the prompts differ: the reactive verbs plus a proactive wake.
        let occasions = [
            (Happened::Poke, State::Grounded, "the display floor"),
            (Happened::Throw, State::Falling, "nothing"),
            (Happened::Summon, State::Grounded, "a Terminal window"),
            (Happened::Perch, State::Perched, "a Safari window"),
            (Happened::Proactive, State::Grounded, "the top of the Dock"),
        ];

        let (mut accepted, mut speech, mut failed) = (0usize, 0usize, 0usize);
        // A reply whose first line names a declared Behavior in the wrong
        // case is the contract kept and our matcher refusing it. `knows`
        // compares exactly, so this bucket is ours, not the model's.
        let mut case_only = 0usize;
        // Prose that is a sample line handed back. A subset of `speech`,
        // because a reply that names a Behavior kept the contract whatever
        // its dialogue borrowed.
        let mut quoted = 0usize;
        let mut examples: Vec<String> = Vec::new();
        let started = Instant::now();

        for turn in 0..wakes {
            let (happened, state, standing) = &occasions[turn % occasions.len()];
            let context = Context {
                activity: Activity {
                    frontmost_application: Some("Terminal".to_string()),
                    switched: turn % 3 == 0,
                    idle: Duration::from_secs((turn as u64 % 7) * 30),
                    at: SystemTime::now(),
                    hour: 9 + (turn as u8 % 12),
                    minute: ((turn as u32 * 7) % 60) as u8,
                    ..Activity::quiet()
                },
                personality: cat.personality.clone(),
                state: *state,
                happened: happened.clone(),
                standing: standing.to_string(),
                front_title: None,
                ..fidget_core::director::Context::quiet()
            };

            match director.wake(&context) {
                Wake::Proposed(proposal) if !proposal.behavior.is_empty() => {
                    accepted += 1;
                }
                Wake::Proposed(proposal) => {
                    // `as_speech` hands back the whole reply, so its first
                    // line is the name the model actually offered.
                    let said = proposal.dialogue.unwrap_or_default();
                    let offered = said
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .trim_end_matches(['.', ':', '!'])
                        .to_string();
                    let near = behaviors
                        .iter()
                        .any(|declared| declared.eq_ignore_ascii_case(&offered));
                    let quote = quoted_sample_line(&said, &cat.personality);
                    if near {
                        case_only += 1;
                    } else {
                        speech += 1;
                        if quote.is_some() {
                            quoted += 1;
                        }
                    }
                    if examples.len() < 6 {
                        let tag = match (near, quote.is_some()) {
                            (true, _) => "case-only",
                            (false, true) => "quoted",
                            (false, false) => "speech",
                        };
                        examples.push(format!("  {tag}: {}", said.replace('\n', " | ")));
                    }
                }
                Wake::Failed => {
                    failed += 1;
                    if examples.len() < 5 {
                        examples.push("  failed: unparsable or transport error".to_string());
                    }
                }
            }
        }

        let percent = |n: usize| (n as f64) * 100.0 / (wakes as f64);
        println!("\n#175 reply-contract outcomes over {wakes} wakes");
        println!("  model:     {model} at {origin}");
        println!("  framing:   {framing:?}  (#244)");
        println!("  behaviors: {}", behaviors.join(", "));
        println!("  elapsed:   {:.0}s", started.elapsed().as_secs_f64());
        println!("  accepted:   {accepted:>3}  ({:.0}%)", percent(accepted));
        println!(
            "  case-only:  {case_only:>3}  ({:.0}%)  contract kept, matcher refused",
            percent(case_only)
        );
        println!(
            "  speech:     {speech:>3}  ({:.0}%)  genuine prose",
            percent(speech)
        );
        println!(
            "  quoted:     {quoted:>3}  ({:.0}%)  of it a personality sample line",
            percent(quoted)
        );
        println!("  failed:     {failed:>3}  ({:.0}%)", percent(failed));
        println!(
            "  the model broke the contract on {:.0}% of wakes",
            percent(speech + failed)
        );
        println!("  (sampling is the server's own; runs of one model wander a few points)");
        for line in &examples {
            println!("{line}");
        }

        assert_eq!(
            accepted + case_only + speech + failed,
            wakes,
            "every wake lands in exactly one bucket"
        );
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "http-session-{label}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir is creatable");
            Self(dir)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Plan-seam change that would fail this: `note_http_call` leaving
    /// WakeRequest unread because "the Action Log belongs to the Harness".
    #[test]
    fn an_http_call_writes_prompt_and_turn() {
        let dir = TempDir::new("http-session");
        let request = WakeRequest {
            prompt: "hi".into(),
            instance: "buddy-1".into(),
            character: "bmo".into(),
            reactive: true,
            blank: false,
        };
        note_http_call(
            dir.path(),
            &request,
            Ok(&Reply::whole("the desktop floor")),
            "unused here",
        );

        let body = std::fs::read_to_string(dir.path().join(crate::action_log::FILE)).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2, "{body}");
        let prompt: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(prompt["event"], "prompt");
        assert_eq!(prompt["instance"], "buddy-1");
        assert_eq!(prompt["wake"], "reactive");
        assert_eq!(prompt["chars"], 2);
        let turn: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(turn["event"], "turn");
        assert_eq!(turn["text"], "the desktop floor");
    }

    /// A turn the cap ended is both the words that were shown and a reason
    /// there were no more of them. The line names the cap and the model, not
    /// a reply that just ends.
    #[test]
    fn a_truncated_http_call_writes_the_words_and_the_cap() {
        let dir = TempDir::new("http-session-cut");
        let request = WakeRequest {
            prompt: "hi".into(),
            instance: "buddy-1".into(),
            character: "bmo".into(),
            reactive: true,
            blank: false,
        };
        let endpoint = Endpoint {
            max_tokens: TURN_CEILING,
            ..local_endpoint()
        };

        note_http_call(
            dir.path(),
            &request,
            Ok(&Reply::truncated("prowl\nMine now, and the")),
            &endpoint.out_of_budget(&endpoint.url, &Truncation::Parseable(String::new())),
        );

        let body = std::fs::read_to_string(dir.path().join(crate::action_log::FILE)).unwrap();
        let turn: serde_json::Value = serde_json::from_str(body.lines().nth(1).unwrap()).unwrap();
        assert_eq!(turn["text"], "prowl\nMine now, and the");
        let why = turn["truncated"].as_str().unwrap_or_default();
        assert!(
            why.contains("gemma4") && why.contains("1024") && why.contains(MAX_TOKENS),
            "the line names the model, the cap and the knob: {why}"
        );
    }

    /// Plan-seam change that would fail this: `note_http_call` writing no
    /// turn line on a failed HTTP wake, so a later parsed/failed cannot be joined.
    #[test]
    fn a_failed_http_call_writes_the_error() {
        let dir = TempDir::new("http-session-err");
        let request = WakeRequest {
            prompt: "hi".into(),
            instance: "buddy-1".into(),
            character: "bmo".into(),
            reactive: false,
            blank: false,
        };
        note_http_call(
            dir.path(),
            &request,
            Err("connection refused"),
            "unused here",
        );

        let body = std::fs::read_to_string(dir.path().join(crate::action_log::FILE)).unwrap();
        let turn: serde_json::Value = serde_json::from_str(body.lines().nth(1).unwrap()).unwrap();
        assert_eq!(turn["wake"], serde_json::Value::Null); // wake lives on prompt, not turn
        assert_eq!(turn["error"], "connection refused");
        let prompt: serde_json::Value = serde_json::from_str(body.lines().next().unwrap()).unwrap();
        assert_eq!(prompt["wake"], "proactive");
    }
}
