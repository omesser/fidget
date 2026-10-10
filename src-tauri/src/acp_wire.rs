//! The ACP wire. The official SDK and its executor, on one thread.
//! No SDK type leaves the file. Reversing the crate choice (ADR-0022)
//! rewrites this file only. The frame loop never sees it (ADR-0004).

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::mpsc::{self as sync_mpsc, RecvTimeoutError};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::content_mark::chunk_text;
use crate::tool_content::{pieces, places, Place, ToolPiece};
use agent_client_protocol::schema::v1::{
    AuthMethod, AuthenticateRequest, CancelNotification, ClientCapabilities, CloseSessionRequest,
    CompleteElicitationNotification, ContentBlock, CreateElicitationRequest,
    CreateElicitationResponse, ElicitationAcceptAction, ElicitationAction, ElicitationCapabilities,
    ElicitationContentValue, ElicitationFormCapabilities, ElicitationId, ElicitationMode,
    ElicitationPropertySchema, ElicitationScope, ElicitationSessionScope,
    ElicitationUrlCapabilities, EnvVariable, Error, ErrorCode, HttpHeader, Implementation,
    InitializeRequest, LoadSessionRequest, McpServer, McpServerHttp, McpServerStdio, MessageId,
    Meta, NewSessionRequest, Plan, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionConfigId, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionId, SessionNotification, SessionUpdate, SetSessionConfigOptionRequest, StopReason,
    TextContent, ToolCallContent, ToolCallLocation,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo, Responder};
use fidget_core::director::Reply;
use serde::Serialize;
use tokio::sync::mpsc;

/// After `session/cancel`, how long the turn lock waits for the `cancelled`
/// reply before it is given back regardless.
const CANCEL_GRACE: Duration = Duration::from_secs(2);

/// How long `authenticate` may block. A browser login is the user, not a
/// stalled child, and a turn's budget would abandon it.
const SIGN_IN_WAIT: Duration = Duration::from_secs(5 * 60);

/// Bound on `session/close` for a session we will not keep. A stuck agent
/// must not hold the next open for the whole turn budget.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// The MCP server `session/new` is told about.
/// The loopback server is the shipped path (ADR-0023). Stdio is the fallback
/// when the Harness advertises no `mcpCapabilities.http`.
#[derive(Clone)]
pub enum McpChoice {
    /// The loopback server the app serves, and the `Authorization` value that
    /// reaches it. The only one whose tools reach the character on screen.
    Http { url: String, authorization: String },
    /// A stdio shim the Harness spawns. It relays to the loopback server so
    /// the tools reach the same Instances.
    Stdio(McpLaunch),
}

impl McpChoice {
    /// What a log line, the Action Log, or a probe may say about this choice.
    /// Never the token. A credential is not logged, printed, or fingerprinted,
    /// including one this process minted.
    pub fn label(&self) -> String {
        match self {
            Self::Http { url, .. } => url.clone(),
            Self::Stdio(launch) => launch.line(),
        }
    }
}

/// What `initialize` told us, in the words the rest of the shell uses.
#[derive(Clone, Debug, Default)]
pub struct Handshake {
    pub agent: Option<String>,
    pub load_session: bool,
    /// Whether the Harness takes HTTP MCP servers.
    pub mcp_http: bool,
    pub auth_methods: Vec<AuthOffer>,
}

/// An agent auth method id copied from the handshake. Nothing else can build one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AgentMethodId(String);

impl AgentMethodId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// One advertised auth method, without a terminal method's args or env.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthOffer {
    Agent {
        id: AgentMethodId,
        name: String,
        description: Option<String>,
    },
    /// No id: a terminal method is not sent to `authenticate`.
    Terminal {
        name: String,
        description: Option<String>,
    },
    Unrecognized {
        name: String,
        description: Option<String>,
    },
}

impl AuthOffer {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Agent { name, .. }
            | Self::Terminal { name, .. }
            | Self::Unrecognized { name, .. } => name,
        }
    }

    pub(crate) fn description(&self) -> Option<&str> {
        match self {
            Self::Agent { description, .. }
            | Self::Terminal { description, .. }
            | Self::Unrecognized { description, .. } => description.as_deref(),
        }
    }
}

/// An in-app sign-in the landing can show. The id is an agent method's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SignIn {
    pub id: AgentMethodId,
    pub label: String,
}

/// One `AuthMethod` as an `AuthOffer`. A blank agent id is not sendable, and
/// a terminal method's args and env never leave this function.
pub(crate) fn auth_offer(method: &AuthMethod) -> AuthOffer {
    match method {
        AuthMethod::Agent(agent) => {
            let id = agent.id.0.trim();
            if id.is_empty() {
                AuthOffer::Unrecognized {
                    name: agent.name.clone(),
                    description: agent.description.clone(),
                }
            } else {
                AuthOffer::Agent {
                    id: AgentMethodId(id.to_string()),
                    name: agent.name.clone(),
                    description: agent.description.clone(),
                }
            }
        }
        AuthMethod::Terminal(terminal) => AuthOffer::Terminal {
            name: terminal.name.clone(),
            description: terminal.description.clone(),
        },
        // A future variant is not an agent method. Treating it as one would
        // send an id `authenticate` was never offered.
        _ => AuthOffer::Unrecognized {
            name: "sign in".to_string(),
            description: None,
        },
    }
}

/// Agent methods that need a key or a cloud project this app never sends, so
/// a button cannot collect them. Codex reads `api-key` from `authenticate`
/// `_meta` or `CODEX_API_KEY` / `OPENAI_API_KEY`. Antigravity answers Ok to a
/// keyless `gemini-api-key` and stores it as the method, and refuses
/// `agent-platform` without `GOOGLE_CLOUD_PROJECT` or a key.
const SHELVED: [&str; 3] = ["api-key", "gemini-api-key", "agent-platform"];

/// Whether an agent method id can be a button. The shelved key ids cannot.
pub(crate) fn sign_in_offered(id: &str) -> bool {
    !SHELVED.contains(&id)
}

/// Agent methods with a visible name, in advertisement order, while login is
/// still required. Terminal methods, unrecognized methods, and the shelved
/// key ids are not buttons.
pub(crate) fn sign_in_button(login_active: bool, offers: &[AuthOffer]) -> Vec<SignIn> {
    if !login_active {
        return Vec::new();
    }
    offers
        .iter()
        .filter_map(|offer| {
            let AuthOffer::Agent { id, name, .. } = offer else {
                return None;
            };
            if !sign_in_offered(id.as_str()) {
                return None;
            }
            let label = name.trim();
            if label.is_empty() {
                return None;
            }
            Some(SignIn {
                id: id.clone(),
                label: label.to_string(),
            })
        })
        .collect()
}

/// A forwarded `session/request_permission`, as the Chat surface draws it.
/// Everything but `request` and `options` is untrusted. A Harness fills it
/// in from a tool call an MCP server can steer.
#[derive(Clone, Debug, Serialize)]
pub struct PermissionAsk {
    /// The request id, as text, handed back with the answer.
    pub request: String,
    /// The tool call's title, absent when it had none. Not a placeholder.
    /// The surface has to tell an ask that described itself badly from one
    /// this file emptied out (#678).
    pub title: Option<String>,
    pub kind: Option<String>,
    /// The tool call's `content`, as the text of it. Where a question from an
    /// MCP server arrives, and so the first thing the row has to show.
    pub content: Vec<String>,
    /// The arguments the call was made with. ACP's `rawInput`, arbitrary JSON.
    /// What the row falls back to when a call carried no content.
    pub input: Option<serde_json::Value>,
    /// Every path the call says it would touch, `content` diffs included.
    pub locations: Vec<String>,
    pub options: Vec<PermissionOption>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    pub kind: Option<String>,
}

/// One `elicitation/create` form, as Chat draws it.
/// One question at a time: the first string-enum field of the schema.
/// `message` is untrusted Harness text. Decline is a valid answer, not an error.
#[derive(Clone, Debug, Serialize)]
pub struct ElicitationForm {
    /// The request id, as text, handed back with the answer.
    pub request: String,
    pub message: String,
    /// The schema property the chosen option fills. Empty when the form had
    /// no multiple-choice field, so Decline is the only answer Chat can send.
    pub field: String,
    pub options: Vec<ElicitationChoice>,
    /// A URL-mode form's link, untrusted like `message`. Accept means the
    /// user opened it; the Harness learns the rest on its own side.
    pub url: Option<String>,
    /// A link Fidget did not ask for, such as an MCP server's sign-in after
    /// `session/new`. It waits in Chat rather than opening it. A link that
    /// arrives during Fidget's own `authenticate`, or that a tool call blocks
    /// on, opens Chat instead.
    #[serde(skip)]
    pub waits: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ElicitationChoice {
    pub value: String,
    pub name: String,
}

/// The user's answer to a form. Decline is a first-class choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElicitationAnswer {
    Accept(String),
    Decline,
}

/// One step of the agent's plan, as the Chat surface draws it.
/// `priority` and `status` are the crate's serde spellings through `name_of`.
/// `_meta` is out. ACP says not to assume anything of it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlanStep {
    pub content: String,
    pub priority: String,
    pub status: String,
}

/// What the session stream said, minus the text. The text comes back with
/// the turn.
#[derive(Clone, Debug)]
pub enum Event {
    ToolCall {
        session: String,
        id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<String>,
        /// Absent when this update did not carry locations. Empty replaces them.
        locations: Option<Vec<Place>>,
        /// Absent when this update did not carry content. Empty replaces it.
        content: Option<Vec<ToolPiece>>,
    },
    /// The agent's steps and which one is current. ACP has the agent send a
    /// complete list and the client replace the plan entirely, so this is never
    /// a delta; an empty list is the turn's plan going away. `session` is the
    /// one that planned, for the same reason as `Thought`'s.
    Plan {
        session: String,
        steps: Vec<PlanStep>,
    },
    Usage {
        session: String,
        used: u64,
        size: u64,
    },
    /// `session` is the one that asked, so the ask is owed by that session's
    /// Instance and not by whoever holds the turn.
    Permission { session: String, ask: PermissionAsk },
    /// One `elicitation/create` form. Separate from `Permission` because the
    /// answer is accept-content or decline, not a permission `optionId`.
    /// `session` is `None` for a form no session scopes, as a sign-in link.
    Elicitation {
        session: Option<String>,
        form: ElicitationForm,
    },
    /// The whole thinking so far, for the Chat surface's Thinking row
    /// (ADR-0034). Each one replaces the last; the Action Log gets none.
    /// `session` is the turn's, because one child serves every Instance.
    Thought { session: String, text: String },
    /// Between-turn agent text accumulated in `Inbound` and flushed
    /// when a session transitions (Ask, Elicit, Prompt, or Close).
    InboundWake { session: String, speech: String },
    /// A forwarded ask that can no longer be answered. Every Chat surface
    /// that drew the ask has to hear this.
    PermissionSettled {
        request: String,
        /// The option that won, or `None` when nothing was picked.
        /// Two surfaces can draw one request. Every answer after the first
        /// is dropped here, so the others still draw the decision that won.
        option: Option<String>,
    },
}

/// The stdio MCP server handed to `session/new` and `session/load`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpLaunch {
    pub path: PathBuf,
    pub args: Vec<String>,
    /// What the shim reads to find the app and authorise itself.
    /// Deliberately absent from `line`.
    pub env: Vec<(String, String)>,
}

impl McpLaunch {
    pub fn line(&self) -> String {
        if self.args.is_empty() {
            self.path.display().to_string()
        } else {
            format!("{} {}", self.path.display(), self.args.join(" "))
        }
    }
}

pub type OnEvent = Box<dyn Fn(Event) + Send + Sync>;

/// A value the Completer config can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Model,
    Effort,
}

impl Field {
    pub fn name(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Effort => "effort",
        }
    }
}

/// What became of one value the Completer config asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `session/set_config_option` was answered, and the returned options
    /// list that option at the requested value.
    Confirmed,
    /// The set succeeded, but the returned options do not show the option at the
    /// requested value, so nothing says it took.
    Unconfirmed,
    /// The session lists no option for it, so nothing was sent.
    Unadvertised,
}

/// What a Completer config did on the wire, not what was asked: one outcome per
/// field that had a value, model first. A field with no value is absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub outcomes: Vec<(Field, Outcome)>,
}

impl Applied {
    /// At least one value was sent and every one of them is confirmed.
    pub fn proved(&self) -> bool {
        !self.outcomes.is_empty()
            && self
                .outcomes
                .iter()
                .all(|(_, outcome)| *outcome == Outcome::Confirmed)
    }

    pub fn fields(&self, wanted: Outcome) -> Vec<Field> {
        self.outcomes
            .iter()
            .filter(|(_, outcome)| *outcome == wanted)
            .map(|(field, _)| *field)
            .collect()
    }
}

/// The session `open` handed back, and what `session/load` replayed of it.
#[derive(Debug)]
pub struct Opened {
    pub id: String,
    /// The session from before this connection, oldest first. Empty for
    /// `session/new`, and for a load that failed: the session that answers
    /// never said what that replay holds.
    pub history: Vec<Replayed>,
    /// What the model and effort set at open did on the wire.
    pub applied: Applied,
}

/// One entry of a `session/load` replay, in the order the session sent it.
/// `type` names the variant for the Chat surface.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Replayed {
    /// A `session/prompt` Fidget sent: the Director's frame, with a typed
    /// line in it when the user typed one.
    Prompt {
        text: String,
    },
    /// The line the user typed inside a `Prompt`'s frame. `session_log::drawn`
    /// adds it, never the replay.
    Typed {
        text: String,
    },
    /// One turn's agent messages joined, `<think>` blocks peeled into the
    /// `Thought` before it. A Director reply keeps its Behavior line here.
    /// `session_log::drawn` reads it before Chat draws the row.
    Reply {
        text: String,
    },
    Thought {
        text: String,
    },
    /// A tool call with every update to it folded in. An update replaces
    /// `content` and `locations` whole, as ACP does.
    ToolCall {
        id: String,
        title: String,
        kind: Option<String>,
        status: Option<String>,
        locations: Vec<Place>,
        content: Vec<ToolPiece>,
    },
    /// The agent's plan as it last stood before the next prompt.
    Plan {
        steps: Vec<PlanStep>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// `-32000`. The Harness wants a login it does not have.
    AuthRequired,
    /// The child is gone.
    Lost,
    Failed(String),
    /// The session exists. The Harness refused this value, in its own words.
    Refused(Field, String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum TurnError {
    /// The Completer timeout passed; `session/cancel` has been sent.
    Timeout,
    /// The child is gone.
    Lost,
    /// A `stopReason` other than `end_turn`, by name.
    Stopped(String),
    /// A turn was already in flight on the wire.
    Busy,
    /// The Harness wants a login it does not have. `auth_refused` says how.
    AuthRequired,
    Failed(String),
}

/// `-32000` is ACP's word for it. claude-code-acp says a token that died
/// mid-session as `-32603` with `errorKind: "authentication_failed"` in
/// `data`, the field it documents for clients to dispatch on instead of the
/// message text (#991).
fn auth_refused(error: &Error) -> bool {
    error.code == ErrorCode::AuthRequired
        || error
            .data
            .as_ref()
            .and_then(|data| data.get("errorKind"))
            .and_then(|kind| kind.as_str())
            == Some("authentication_failed")
}

/// An ACP error as one line: its message, then the detail in `data`. grok
/// sends "Internal error" as the message and the reason only in `data.message`.
fn error_text(error: Error) -> String {
    use serde_json::Value;
    let detail = match error.data {
        None | Some(Value::Null) => return error.message,
        Some(Value::String(text)) => text,
        Some(data) => match data.get("message").and_then(Value::as_str) {
            Some(text) => text.to_string(),
            None => data.to_string(),
        },
    };
    format!("{}: {detail}", error.message)
}

/// Why no wire came back from a spawn.
/// `Missing` is `ErrorKind::NotFound`. Never the locale text of os error 2.
/// No respawn mends a PATH that has no such file.
#[derive(Debug)]
pub enum SpawnError {
    Missing,
    /// The child ran and was gone before `initialize` answered, with its
    /// status when the wire saw it go and the tail of what it printed.
    Exited {
        status: Option<std::process::ExitStatus>,
        output: String,
    },
    Failed(String),
}

impl SpawnError {
    fn start(why: std::io::Error) -> Self {
        match why.kind() {
            std::io::ErrorKind::NotFound => Self::Missing,
            _ => Self::Failed(format!("could not start: {why}")),
        }
    }
}

enum Msg {
    Open {
        load: Option<String>,
        cwd: PathBuf,
        mcp: Option<McpChoice>,
        claude_mcp_approval: bool,
        /// Empty sends no model option.
        model: String,
        /// `None` sends no effort option.
        effort: Option<String>,
        reply: sync_mpsc::Sender<Result<Opened, OpenError>>,
    },
    Prompt {
        session_id: String,
        text: String,
        reply: sync_mpsc::Sender<Progress>,
    },
    /// `apply_completer` on a session this connection opened.
    Configure {
        session_id: String,
        model: String,
        effort: Option<String>,
        reply: sync_mpsc::Sender<Result<Applied, OpenError>>,
    },
    Cancel,
    Answer {
        request: String,
        option: String,
    },
    AnswerElicitation {
        request: String,
        answer: ElicitationAnswer,
    },
    Authenticate {
        method_id: String,
        reply: sync_mpsc::Sender<Result<(), String>>,
    },
    Close {
        session_id: String,
        reply: sync_mpsc::Sender<Result<(), String>>,
    },
    Shutdown,
}

/// What the wire thread tells `prompt` about the turn it is running. The
/// budget is kept on `prompt`'s side; only the turn knows when the Harness
/// has stopped to ask the user something, and the user is not on the clock
/// (#1001).
enum Progress {
    /// A permission ask or elicitation form is open. The budget stops.
    Asked,
    /// Nothing is open any more. A fresh budget starts.
    Settled,
    /// The answer so far, after a chunk of it landed.
    Said(String),
    Done(Result<Reply, TurnError>),
}

/// What we send on `initialize`. Named so a test can assert the payload
/// without spawning a child.
fn initialize_request() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1)
        .client_info(Implementation::new("fidget", env!("CARGO_PKG_VERSION")))
        .client_capabilities(
            ClientCapabilities::new().elicitation(
                ElicitationCapabilities::new()
                    .form(ElicitationFormCapabilities::new())
                    .url(ElicitationUrlCapabilities::new()),
            ),
        )
}

/// One spawned Harness and the thread that speaks to it.
pub struct Wire {
    tx: mpsc::UnboundedSender<Msg>,
    handshake: Handshake,
    /// Nothing is ever sent on this. The thread owns the sender, so the
    /// receiver disconnects when the thread ends, which is how `wait_for_exit`
    /// knows. Behind a `Mutex` because a `Receiver` is `Send` and not `Sync`.
    done: Mutex<sync_mpsc::Receiver<()>>,
}

impl Wire {
    /// Spawn `command`, connect, and `initialize`. Blocks for at most
    /// `timeout`; the thread lives on for as long as the child does.
    pub fn spawn(
        command: Command,
        timeout: Duration,
        on_event: OnEvent,
    ) -> Result<Self, SpawnError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = sync_mpsc::channel();
        let (done_tx, done) = sync_mpsc::channel();
        thread::Builder::new()
            .name("acp-wire".into())
            .spawn(move || run(command, rx, ready_tx, done_tx, on_event))
            .map_err(|why| SpawnError::Failed(format!("could not start the wire thread: {why}")))?;
        let handshake = match ready_rx.recv_timeout(timeout) {
            Ok(Ok(handshake)) => handshake,
            Ok(Err(why)) => return Err(why),
            Err(RecvTimeoutError::Timeout) => {
                let _ = tx.send(Msg::Shutdown);
                return Err(SpawnError::Failed("did not answer initialize".to_string()));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(SpawnError::Exited {
                    status: None,
                    output: String::new(),
                })
            }
        };
        Ok(Self {
            tx,
            handshake,
            done: Mutex::new(done),
        })
    }

    /// Wait, bounded, for the thread to end and the child to be reaped.
    /// `shutdown` only posts the message. The child is in its own process
    /// group, so ending this process does not take it with us.
    pub fn wait_for_exit(&self, timeout: Duration) -> bool {
        self.done.lock().is_ok_and(|done| {
            matches!(
                done.recv_timeout(timeout),
                Err(RecvTimeoutError::Disconnected)
            )
        })
    }

    pub fn handshake(&self) -> &Handshake {
        &self.handshake
    }

    /// Whether the thread, and so the child, is still there.
    pub fn alive(&self) -> bool {
        !self.tx.is_closed()
    }

    /// `session/load` when `load` names one, falling back to `session/new`.
    /// An empty `model` or effort is not sent.
    #[expect(
        clippy::too_many_arguments,
        reason = "keep ACP session fields explicit at the wire boundary"
    )]
    pub fn open(
        &self,
        load: Option<String>,
        cwd: &Path,
        mcp: Option<McpChoice>,
        claude_mcp_approval: bool,
        timeout: Duration,
        model: &str,
        effort: Option<&str>,
    ) -> Result<Opened, OpenError> {
        let (reply, rx) = sync_mpsc::channel();
        self.tx
            .send(Msg::Open {
                load,
                cwd: cwd.to_path_buf(),
                mcp,
                claude_mcp_approval,
                model: model.to_string(),
                effort: effort.map(str::to_string),
                reply,
            })
            .map_err(|_| OpenError::Lost)?;
        rx.recv_timeout(timeout).unwrap_or(Err(OpenError::Lost))
    }

    /// Set the model and effort on a session `open` returned, as `open` does, against
    /// the options the session last listed. The id stays.
    pub fn configure(
        &self,
        session_id: &str,
        model: &str,
        effort: Option<&str>,
        timeout: Duration,
    ) -> Result<Applied, OpenError> {
        let (reply, rx) = sync_mpsc::channel();
        self.tx
            .send(Msg::Configure {
                session_id: session_id.to_string(),
                model: model.to_string(),
                effort: effort.map(str::to_string),
                reply,
            })
            .map_err(|_| OpenError::Lost)?;
        rx.recv_timeout(timeout).unwrap_or(Err(OpenError::Lost))
    }

    /// One `session/prompt`. The concatenated `agent_message_chunk`s once the
    /// turn ends in `end_turn`. `timeout` guards a Harness that went quiet,
    /// not the user: it stops while an ask or form waits on them and starts
    /// over whole once the last one is settled (#1001). Past it,
    /// `session/cancel` goes out and the reply is waited on for
    /// `CANCEL_GRACE` so the wire is quiet again. `said` hears the answer so
    /// far each time a chunk of it lands.
    pub fn prompt(
        &self,
        session_id: &str,
        text: &str,
        timeout: Duration,
        said: &dyn Fn(&str),
    ) -> Result<Reply, TurnError> {
        let (reply, rx) = sync_mpsc::channel();
        self.tx
            .send(Msg::Prompt {
                session_id: session_id.to_string(),
                text: text.to_string(),
                reply,
            })
            .map_err(|_| TurnError::Lost)?;
        let mut deadline = Some(Instant::now() + timeout);
        loop {
            let next = match deadline {
                Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match next {
                Ok(Progress::Done(outcome)) => return outcome,
                Ok(Progress::Said(text)) => said(&text),
                Ok(Progress::Asked) => deadline = None,
                Ok(Progress::Settled) => deadline = Some(Instant::now() + timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(TurnError::Lost),
                // An ask that lands in the same instant the budget runs out is
                // cancelled with it. A Harness that spent the whole budget
                // before asking was a stall until that instant.
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.tx.send(Msg::Cancel);
                    let grace = Instant::now() + CANCEL_GRACE;
                    loop {
                        match rx.recv_timeout(grace.saturating_duration_since(Instant::now())) {
                            Ok(Progress::Done(_)) | Err(_) => return Err(TurnError::Timeout),
                            Ok(Progress::Asked | Progress::Settled | Progress::Said(_)) => {}
                        }
                    }
                }
            }
        }
    }

    /// Cancel the turn in flight, for a caller about to send a newer prompt.
    /// No-op on an idle wire. Unlike `prompt`'s timeout cancel, this waits
    /// for nothing. The turn's own caller holds its reply channel.
    pub fn cancel(&self) {
        let _ = self.tx.send(Msg::Cancel);
    }

    /// The user's pick on a forwarded permission request.
    pub fn answer(&self, request: &str, option: &str) {
        let _ = self.tx.send(Msg::Answer {
            request: request.to_string(),
            option: option.to_string(),
        });
    }

    /// `authenticate` for an agent method, and nothing else. The response body
    /// is not a session: the caller still has to open one.
    pub fn authenticate(&self, method_id: &str) -> Result<(), String> {
        let (reply, rx) = sync_mpsc::channel();
        self.tx
            .send(Msg::Authenticate {
                method_id: method_id.to_string(),
                reply,
            })
            .map_err(|_| "harness exited".to_string())?;
        match rx.recv_timeout(SIGN_IN_WAIT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err("sign-in timed out".to_string()),
            Err(RecvTimeoutError::Disconnected) => Err("harness exited".to_string()),
        }
    }

    /// `session/close` for a session this process will not store. Best effort:
    /// a Harness that has no close capability answers with an error, and the
    /// caller still drops the id.
    pub fn close(&self, session_id: &str) -> Result<(), String> {
        let (reply, rx) = sync_mpsc::channel();
        self.tx
            .send(Msg::Close {
                session_id: session_id.to_string(),
                reply,
            })
            .map_err(|_| "harness exited".to_string())?;
        match rx.recv_timeout(CLOSE_WAIT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err("session/close timed out".to_string()),
            Err(RecvTimeoutError::Disconnected) => Err("harness exited".to_string()),
        }
    }

    /// The user's pick on a forwarded elicitation form. Decline is valid.
    pub fn answer_elicitation(&self, request: &str, answer: ElicitationAnswer) {
        let _ = self.tx.send(Msg::AnswerElicitation {
            request: request.to_string(),
            answer,
        });
    }

    /// Cancel whatever is in flight, answer open asks `cancelled`, close
    /// stdin, and kill the child.
    pub fn shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown);
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The whole life of one child, on the wire thread.
fn run(
    command: Command,
    rx: mpsc::UnboundedReceiver<Msg>,
    ready: sync_mpsc::Sender<Result<Handshake, SpawnError>>,
    // Held, never sent on, and dropped when this function returns. That drop
    // is what `wait_for_exit` waits for.
    _done: sync_mpsc::Sender<()>,
    on_event: OnEvent,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(runtime) => runtime,
        Err(why) => {
            let _ = ready.send(Err(SpawnError::Failed(format!(
                "no runtime for the wire: {why}"
            ))));
            return;
        }
    };
    // Piped rather than inherited so a launcher that dies at startup can show
    // what it printed. The pump still copies every byte to our own stderr.
    let tail = Tail::default();
    let stderr = match std::io::pipe() {
        Ok((reader, writer)) => {
            tail.pump_stderr(reader);
            std::process::Stdio::from(writer)
        }
        Err(_) => std::process::Stdio::inherit(),
    };
    runtime.block_on(async move {
        #[cfg(windows)]
        let spawned = windows_job::spawn_in_job(command, stderr);

        #[cfg(not(windows))]
        let spawned = {
            let mut async_command = async_process::Command::from(command);
            async_command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(stderr);
            async_command.spawn()
        };
        // Both platforms hand back the spawn's own `io::Error`, so the missing
        // file is read off its kind in one place rather than two.
        let mut child = match spawned {
            Ok(child) => child,
            Err(why) => {
                let _ = ready.send(Err(SpawnError::start(why)));
                return;
            }
        };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = ready.send(Err(SpawnError::Failed("no pipes to the child".to_string())));
            return;
        };
        // What the Harness sends us, routed into `serve`, which is the one
        // place that knows whether a turn is open to receive it. Anything
        // else (fs, terminal) the SDK answers with method-not-found.
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        let updates = incoming_tx.clone();
        let asks = incoming_tx.clone();
        let completes = incoming_tx.clone();
        let mut ready = Some(ready);
        let mut refused = None;
        let outcome = Client
            .builder()
            .name("fidget")
            .on_receive_notification(
                async move |notification: SessionNotification, _cx| {
                    let _ = updates.send(Incoming::Update(notification));
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_notification(
                async move |notification: CompleteElicitationNotification, _cx| {
                    let _ = completes.send(Incoming::Complete(notification.elicitation_id));
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |request: RequestPermissionRequest, responder, _cx| {
                    let _ = asks.send(Incoming::Ask(request, responder));
                    Ok(())
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: CreateElicitationRequest, responder, _cx| {
                    let _ = incoming_tx.send(Incoming::Elicit(request, responder));
                    Ok(())
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(
                ByteStreams::new(
                    stdin,
                    Tee {
                        inner: stdout,
                        tail: tail.clone(),
                    },
                ),
                async |cx: ConnectionTo<Agent>| {
                    let handshake = cx
                        .send_request(initialize_request())
                        .block_task()
                        .await
                        .map(|response| Handshake {
                            agent: response.agent_info.map(|info| info.name),
                            load_session: response.agent_capabilities.load_session,
                            mcp_http: response.agent_capabilities.mcp_capabilities.http,
                            auth_methods: response.auth_methods.iter().map(auth_offer).collect(),
                        });
                    // A refusal is labelled below, once the child has had
                    // the chance to say whether it is gone.
                    match handshake {
                        Ok(handshake) => {
                            tail.close_stdout();
                            if let Some(ready) = ready.take() {
                                let _ = ready.send(Ok(handshake));
                            }
                            serve(&cx, rx, incoming_rx, &on_event).await;
                        }
                        Err(why) => refused = Some(error_text(why)),
                    }
                    Ok(())
                },
            )
            .await;
        // `ready` still held here means `initialize` never completed. The
        // SDK can end the connection from its transport side first, a reply
        // written into a child that already exited, and drop the closure
        // above before it labels the error. The label belongs here too, or
        // the same death reads as two different failures (#907). A child
        // that is gone says how it went, which is the one clue to a launcher
        // that died at startup, such as `node` failing to load.
        if let Some(ready) = ready.take() {
            let refused = refused.or(outcome.err().map(error_text));
            let status = exit_status(&mut child);
            let output = tail.text(Duration::from_millis(250));
            let _ = ready.send(Err(match (status, refused) {
                (Some(status), _) => SpawnError::Exited {
                    status: Some(status),
                    output,
                },
                (None, Some(why)) => SpawnError::Failed(format!("initialize: {why}")),
                (None, None) => SpawnError::Exited {
                    status: None,
                    output,
                },
            }));
        }
        // The Harness may have been started through `npx`, which does not
        // reliably die on stdin EOF; kill the process group rather than
        // orphan grandchildren. Direct kill is the fallback.
        kill_harness_tree(child.id());
        let _ = child.kill();
        let _ = child.status().await;
    });
}

/// How the child ended, if it has. The transport closes as the child exits,
/// a moment before the kernel can report it, so this waits that moment and no
/// longer. A child that only closed its stdout is still running.
fn exit_status(child: &mut async_process::Child) -> Option<std::process::ExitStatus> {
    let until = Instant::now() + Duration::from_millis(500);
    loop {
        match child.try_status() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < until => thread::sleep(Duration::from_millis(10)),
            _ => return None,
        }
    }
}

/// How much of a child's output a failure keeps: the last 4 KiB, whole lines.
const TAIL_BYTES: usize = 4096;

/// The tail of what a child printed: all of its stderr, and its stdout until
/// `initialize` answers, in the order they arrived. After that stdout is the
/// ACP stream, which the SDK owns and a failure has no use for.
#[derive(Clone, Default)]
struct Tail(std::sync::Arc<TailInner>);

#[derive(Default)]
struct TailInner {
    bytes: Mutex<TailBytes>,
    stdout_closed: std::sync::atomic::AtomicBool,
    stderr_done: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct TailBytes {
    kept: Vec<u8>,
    cut: bool,
}

impl Tail {
    fn push(&self, bytes: &[u8]) {
        let Ok(mut tail) = self.0.bytes.lock() else {
            return;
        };
        tail.kept.extend_from_slice(bytes);
        // Trimmed at twice the bound, so a chatty child costs one drain per
        // 4 KiB rather than one per write.
        if tail.kept.len() > 2 * TAIL_BYTES {
            let cut = tail.kept.len() - TAIL_BYTES;
            tail.kept.drain(..cut);
            tail.cut = true;
        }
    }

    fn close_stdout(&self) {
        self.0
            .stdout_closed
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Our stderr still gets every byte, so the terminal reads as it did
    /// before the pipe. A thread and not a task: the wire's runtime blocks in
    /// `exit_status`, and the last lines arrive while it does.
    fn pump_stderr(&self, mut from: std::io::PipeReader) {
        let tail = self.clone();
        let spawned = thread::Builder::new()
            .name("harness-stderr".into())
            .spawn(move || {
                use std::io::{Read, Write};
                let mut buf = [0u8; 4096];
                while let Ok(read @ 1..) = from.read(&mut buf) {
                    let _ = std::io::stderr().write_all(&buf[..read]);
                    tail.push(&buf[..read]);
                }
                tail.0
                    .stderr_done
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            });
        if let Err(why) = spawned {
            eprintln!("harness: no stderr pump: {why}");
        }
    }

    /// What was kept, once stderr has closed or `wait` is up. A grandchild
    /// can hold stderr open after the child is gone, so the wait is bounded.
    fn text(&self, wait: Duration) -> String {
        let until = Instant::now() + wait;
        while !self.0.stderr_done.load(std::sync::atomic::Ordering::SeqCst)
            && Instant::now() < until
        {
            thread::sleep(Duration::from_millis(10));
        }
        let Ok(tail) = self.0.bytes.lock() else {
            return String::new();
        };
        let start = tail.kept.len().saturating_sub(TAIL_BYTES);
        let cut = tail.cut || start > 0;
        let text = String::from_utf8_lossy(&tail.kept[start..]);
        // A cut tail starts mid-line; that fragment reads as noise.
        let text = match (cut, text.find('\n')) {
            (true, Some(newline)) => &text[newline + 1..],
            _ => &text[..],
        };
        text.trim().to_string()
    }
}

/// The child's stdout on its way to the SDK, copied into the tail until
/// `initialize` answers. Bytes pass through untouched; the SDK does all the
/// reading of what they mean.
struct Tee<R> {
    inner: R,
    tail: Tail,
}

impl<R: futures_io::AsyncRead + Unpin> futures_io::AsyncRead for Tee<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = &mut *self;
        let read = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let std::task::Poll::Ready(Ok(read)) = &read {
            if !this
                .tail
                .0
                .stdout_closed
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                this.tail.push(&buf[..*read]);
            }
        }
        read
    }
}

/// Whether `pgid` is a group we may SIGKILL. A real group, and not our own.
/// Isolation is supposed to move the child out first. If that fails, the
/// child is still in our group, and SIGKILLing it there kills us (#457).
#[cfg(unix)]
pub(crate) fn killable_group(pgid: libc::pid_t) -> bool {
    pgid > 0 && pgid != unsafe { libc::getpgrp() }
}

/// SIGKILL the Harness's process group. `npx` grandchildren share it, and a
/// direct `Child::kill` leaves them running. Never our own group.
/// On Windows this terminates the Job Object so grandchildren die too.
pub(crate) fn kill_harness_tree(pid: u32) {
    #[cfg(unix)]
    {
        let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
        if killable_group(pgid) {
            // SIGKILL, not SIGTERM. Claude's ACP adapter dumps
            // `Query closed before response received` on a polite
            // signal, which is the dump this isolation exists to avoid.
            let _ = unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
    }
    #[cfg(windows)]
    {
        windows_job::terminate_job(pid);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

enum Incoming {
    Update(SessionNotification),
    Ask(
        RequestPermissionRequest,
        Responder<RequestPermissionResponse>,
    ),
    Elicit(
        CreateElicitationRequest,
        Responder<CreateElicitationResponse>,
    ),
    /// `elicitation/complete`: the Harness says a link's flow finished.
    Complete(ElicitationId),
}

/// A `session/request_permission` held open until the user answers it.
type PendingAsk = (String, Responder<RequestPermissionResponse>);

/// A turn the Harness started on its own, as a cron fire does: updates for a
/// session with no `session/prompt` open.
#[derive(Default)]
struct Inbound {
    said: Answer,
    thought: String,
    /// The `messageId` of the message arriving. A different one is the next
    /// fire, and ends this wake (#1435).
    message: Option<MessageId>,
}

struct PendingElicit {
    form: ElicitationForm,
    responder: Responder<CreateElicitationResponse>,
    /// A URL form's `elicitationId`, which `elicitation/complete` names.
    link: Option<ElicitationId>,
}

/// Lent by `serve`, which runs Fidget's side of the Harness connection, to
/// `turn` for one `session/prompt`: Fidget's commands (`rx`), the Harness's
/// requests and updates (`incoming`), and the forms and asks that outlive it.
struct Serving<'a> {
    rx: &'a mut mpsc::UnboundedReceiver<Msg>,
    incoming: &'a mut mpsc::UnboundedReceiver<Incoming>,
    held: &'a mut Vec<PendingElicit>,
    held_asks: &'a mut Vec<PendingAsk>,
    /// Other sessions' work while this turn runs. One child serves every
    /// Instance, so their traffic is between turns for them.
    inbound: &'a mut HashMap<SessionId, Inbound>,
}

type InflightAuth<'a> = (
    Pin<Box<dyn Future<Output = Result<(), String>> + 'a>>,
    sync_mpsc::Sender<Result<(), String>>,
);

/// Commands until shutdown. `authenticate` stays in flight beside them: a
/// device flow can sit for minutes, and awaiting it here would queue every
/// other Instance's `session/new`.
async fn serve(
    cx: &ConnectionTo<Agent>,
    mut rx: mpsc::UnboundedReceiver<Msg>,
    mut incoming: mpsc::UnboundedReceiver<Incoming>,
    on_event: &OnEvent,
) {
    let mut auth: Option<InflightAuth<'_>> = None;
    // Forms asked between turns, as a sign-in link is during `authenticate`,
    // and links that outlive a turn. A turn keeps its own, so each is
    // cancelled with what it belongs to.
    let mut forms: Vec<PendingElicit> = Vec::new();
    let mut asks: Vec<PendingAsk> = Vec::new();
    let mut inbound: HashMap<SessionId, Inbound> = HashMap::new();
    // What each open session last listed. A set answers with the whole list, and a
    // model set can change which effort option exists.
    let mut options: HashMap<SessionId, Vec<SessionConfigOption>> = HashMap::new();
    loop {
        enum Step {
            Auth(Result<(), String>),
            Command(Msg),
            Stop,
        }
        let step = {
            // Copied before the future borrows `auth`. The `if` below would
            // otherwise fight that borrow, and the device flow would go back
            // to being the only command the loop can run.
            let waiting = auth.is_some();
            let auth_fut = async {
                match auth.as_mut() {
                    Some((fut, _)) => fut.as_mut().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                biased;
                outcome = auth_fut, if waiting => Step::Auth(outcome),
                command = rx.recv() => match command {
                    Some(command) => Step::Command(command),
                    None => Step::Stop,
                },
                message = incoming.recv() => {
                    if let Some(message) = message {
                        between_turns(message, &mut forms, &mut asks, &mut inbound, waiting, on_event);
                    }
                    continue;
                }
                () = cx.incoming_closed() => Step::Stop,
            }
        };
        match step {
            Step::Stop => {
                if let Some((_, reply)) = auth.take() {
                    let _ = reply.send(Err("harness exited".to_string()));
                }
                break;
            }
            Step::Auth(outcome) => {
                if let Some((_, reply)) = auth.take() {
                    let _ = reply.send(outcome);
                }
                // The sign-in's own forms. A link that waits is the session's.
                let (mut signed, kept) = forms.drain(..).partition(|pending| !pending.form.waits);
                forms = kept;
                cancel_forms(&mut signed, on_event);
            }
            Step::Command(Msg::AnswerElicitation { request, answer }) => {
                answer_form(&mut forms, request, answer, on_event)
            }
            Step::Command(Msg::Authenticate { method_id, reply }) => {
                if auth.is_some() {
                    let _ = reply.send(Err("sign-in already in progress".to_string()));
                    continue;
                }
                // `async move` copies the `&ConnectionTo`. The method id is
                // owned, so the request does not borrow the command.
                auth = Some((
                    Box::pin(async move {
                        match cx
                            .send_request(AuthenticateRequest::new(method_id))
                            .block_task()
                            .await
                        {
                            Ok(_) => Ok(()),
                            Err(error) => Err(error_text(error)),
                        }
                    }),
                    reply,
                ));
            }
            Step::Command(Msg::Open {
                load,
                cwd,
                mcp,
                claude_mcp_approval,
                model,
                effort,
                reply,
            }) => {
                let loading = load.clone().map(SessionId::new);
                let opened = open(
                    cx,
                    load,
                    &cwd,
                    mcp,
                    claude_mcp_approval,
                    &model,
                    effort.as_deref(),
                )
                .await
                .map(|(id, listed, applied)| {
                    options.insert(id.clone(), listed);
                    (id, applied)
                });
                // ACP replays a loaded conversation as updates before it
                // answers, even a load that then fails and falls back to
                // `session/new`. Every update queued for either id is history,
                // never work between turns. Only a load that answered keeps it.
                let loaded = matches!(&opened, Ok((id, _)) if loading.as_ref() == Some(id));
                let replayed = |id: &SessionId| {
                    loading.as_ref() == Some(id)
                        || matches!(&opened, Ok((opened, _)) if opened == id)
                };
                let mut history = Restore::default();
                while let Ok(message) = incoming.try_recv() {
                    let message = match message {
                        Incoming::Update(update) if replayed(&update.session_id) => {
                            if loaded {
                                history.take(hear(&update.update));
                            }
                            continue;
                        }
                        other => other,
                    };
                    let signing_in = auth.is_some();
                    between_turns(
                        message,
                        &mut forms,
                        &mut asks,
                        &mut inbound,
                        signing_in,
                        on_event,
                    );
                }
                let history = history.finish();
                let _ = reply.send(opened.map(|(id, applied)| Opened {
                    id: id.0.to_string(),
                    history,
                    applied,
                }));
            }
            Step::Command(Msg::Prompt {
                session_id,
                text,
                reply,
            }) => {
                let id = SessionId::new(session_id);
                // Drain any pending updates for this session before flushing,
                // so between-turn speech that has been emitted but not yet
                // processed doesn't get dropped.
                while let Ok(message) = incoming.try_recv() {
                    let signing_in = auth.is_some();
                    between_turns(
                        message,
                        &mut forms,
                        &mut asks,
                        &mut inbound,
                        signing_in,
                        on_event,
                    );
                }
                end_inbound(&mut inbound, &id, on_event);
                let serving = Serving {
                    rx: &mut rx,
                    incoming: &mut incoming,
                    held: &mut forms,
                    held_asks: &mut asks,
                    inbound: &mut inbound,
                };
                let outcome = turn(cx, &id, serving, &text, &reply, on_event).await;
                let lost = outcome == Err(TurnError::Lost);
                let _ = reply.send(Progress::Done(outcome));
                if lost {
                    break;
                }
            }
            Step::Command(Msg::Configure {
                session_id,
                model,
                effort,
                reply,
            }) => {
                let id = SessionId::new(session_id);
                let listed = options.get(&id).cloned().unwrap_or_default();
                let set = apply_completer(cx, &id, &model, effort.as_deref(), listed)
                    .await
                    .map(|(listed, applied)| {
                        options.insert(id, listed);
                        applied
                    });
                let _ = reply.send(set);
            }
            Step::Command(Msg::Close { session_id, reply }) => {
                let id = SessionId::new(session_id);
                options.remove(&id);
                // Drain any pending updates for this session before flushing,
                // so between-turn speech that has been emitted but not yet
                // processed doesn't get dropped.
                while let Ok(message) = incoming.try_recv() {
                    let signing_in = auth.is_some();
                    between_turns(
                        message,
                        &mut forms,
                        &mut asks,
                        &mut inbound,
                        signing_in,
                        on_event,
                    );
                }
                end_inbound(&mut inbound, &id, on_event);
                let outcome = cx
                    .send_request(CloseSessionRequest::new(id))
                    .block_task()
                    .await;
                let _ = reply.send(match outcome {
                    Ok(_) => Ok(()),
                    Err(error) => Err(error_text(error)),
                });
            }
            Step::Command(Msg::Answer { request, option }) => {
                answer_ask(&mut asks, request, option, on_event)
            }
            // No turn is running, so there is nothing to cancel. An inbound
            // turn is the Harness's own, and `session/cancel` is for ours.
            Step::Command(Msg::Cancel) => {}
            Step::Command(Msg::Shutdown) => {
                if let Some((_, reply)) = auth.take() {
                    let _ = reply.send(Err("harness exited".to_string()));
                }
                break;
            }
        }
    }
    cancel_asks(&mut asks, on_event);
    cancel_forms(&mut forms, on_event);
}

/// A load replay. Inside a prompted turn, agent messages join into one reply
/// and a new `messageId` breaks the paragraph. With no prompt, each id is a reply.
#[derive(Default)]
struct Restore {
    entries: Vec<Replayed>,
    /// The agent message still arriving, and the `<think>` peeled out of it.
    message: Option<(Answer, String)>,
    /// The `messageId` already joined into this turn. A missing id leaves it.
    joined: Option<MessageId>,
    /// The open message's id differs from `joined`, so joining it breaks the paragraph.
    break_next: bool,
}

impl Restore {
    fn take(&mut self, heard: Heard<'_>) {
        if let Heard::Said { message, .. } = &heard {
            // A missing id continues the message already open. Only a different
            // id is the next message.
            let changed = message_changed(self.joined.as_ref(), *message);
            if self.message.is_none() || changed {
                self.end_message();
            }
            if changed {
                self.break_next = true;
            }
            if let Some(id) = *message {
                self.joined = Some(id.clone());
            }
        } else {
            self.end_message();
        }
        match heard {
            Heard::Said { block, .. } => {
                let (answer, thought) = self
                    .message
                    .get_or_insert_with(|| (Answer::default(), String::new()));
                thought.push_str(&answer.hear(block));
            }
            Heard::Prompt(block) => match self.entries.last_mut() {
                Some(Replayed::Prompt { text: prompt }) => {
                    prompt.push_str(&chunk_text(block, prompt));
                }
                _ => self.entries.push(Replayed::Prompt {
                    text: chunk_text(block, ""),
                }),
            },
            Heard::Thought(block) => {
                let text = chunk_text(block, self.thought_so_far());
                self.add_turn_thought(&text);
            }
            Heard::ToolCall(tool) => self.entries.push(tool.row()),
            Heard::ToolUpdate(tool) => {
                let known =
                    self.entries.iter_mut().rev().find(
                        |entry| matches!(entry, Replayed::ToolCall { id, .. } if *id == tool.id),
                    );
                match known {
                    Some(row) => tool.fold_into(row),
                    None => self.entries.push(tool.row()),
                }
            }
            Heard::Plan(steps) => {
                // ACP replaces a plan whole. One per prompt: the last word.
                let turn = self.turn_start();
                match self.entries[turn..]
                    .iter_mut()
                    .find(|entry| matches!(entry, Replayed::Plan { .. }))
                {
                    Some(Replayed::Plan { steps: held }) => *held = steps,
                    _ => self.entries.push(Replayed::Plan { steps }),
                }
            }
            Heard::Usage { .. } | Heard::Ignored => {}
        }
    }

    /// The index just after the latest prompt. Entries before it are an earlier turn.
    fn turn_start(&self) -> usize {
        self.entries
            .iter()
            .rposition(|entry| matches!(entry, Replayed::Prompt { .. }))
            .map_or(0, |at| at + 1)
    }

    fn reply_at(&self, turn: usize) -> Option<usize> {
        self.entries[turn..]
            .iter()
            .position(|entry| matches!(entry, Replayed::Reply { .. }))
            .map(|offset| turn + offset)
    }

    /// The thought text a new chunk joins, so a mark keeps its paragraph break.
    fn thought_so_far(&self) -> &str {
        let turn = self.turn_start();
        if let Some(at) = self.reply_at(turn) {
            if at > turn {
                if let Some(Replayed::Thought { text }) = self.entries.get(at - 1) {
                    return text;
                }
            }
            return "";
        }
        match self.entries.last() {
            Some(Replayed::Thought { text }) => text,
            _ => "",
        }
    }

    /// A turn's thought stays on the row above its reply. Live Chat files it there.
    fn add_turn_thought(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let turn = self.turn_start();
        if let Some(at) = self.reply_at(turn) {
            if at > turn {
                if let Some(Replayed::Thought { text: held }) = self.entries.get_mut(at - 1) {
                    held.push_str(text);
                    return;
                }
            }
            self.entries.insert(
                at,
                Replayed::Thought {
                    text: text.to_string(),
                },
            );
            return;
        }
        self.think(text);
    }

    fn think(&mut self, text: &str) {
        match self.entries.last_mut() {
            Some(Replayed::Thought { text: thought }) => thought.push_str(text),
            _ => self.entries.push(Replayed::Thought {
                text: text.to_string(),
            }),
        }
    }

    fn end_message(&mut self) {
        let Some((answer, thought)) = self.message.take() else {
            return;
        };
        let break_before = self.break_next;
        self.break_next = false;
        let text = answer.finish();
        let turn = self.turn_start();
        // No prompt means the replay never marked a turn, so a new id is the
        // next reply. Inside a prompt, that id stays in the one reply.
        let split = break_before && turn == 0;
        if self.reply_at(turn).is_some() && !split {
            self.add_turn_thought(&thought);
            if let Some(Replayed::Reply { text: held }) = self.entries[turn..]
                .iter_mut()
                .find(|entry| matches!(entry, Replayed::Reply { .. }))
            {
                if break_before {
                    break_paragraph(held);
                }
                held.push_str(&text);
            }
            return;
        }
        if !thought.is_empty() {
            self.think(&thought);
        }
        self.entries.push(Replayed::Reply { text });
    }

    /// Text entries that hold only whitespace are dropped, and the rest
    /// trimmed: a replayed chunk boundary is not something anyone wrote.
    fn finish(mut self) -> Vec<Replayed> {
        self.end_message();
        self.entries
            .into_iter()
            .filter_map(|entry| match entry {
                Replayed::Prompt { text } => trimmed(text).map(|text| Replayed::Prompt { text }),
                Replayed::Reply { text } => trimmed(text).map(|text| Replayed::Reply { text }),
                Replayed::Thought { text } => trimmed(text).map(|text| Replayed::Thought { text }),
                other => Some(other),
            })
            .collect()
    }
}

/// One `session/update`, read once. The live sink (`show`) raises `Event`s
/// from it and the restore sink (`Restore`) collects `Replayed` entries. They
/// differ in what they do with an update, not in how it is read.
enum Heard<'a> {
    /// A chunk of an agent message, and the `messageId` it carries.
    Said {
        message: Option<&'a MessageId>,
        block: &'a ContentBlock,
    },
    /// A chunk of the agent's reasoning.
    Thought(&'a ContentBlock),
    /// A chunk of a prompt the session was sent. A load replays these.
    Prompt(&'a ContentBlock),
    /// A tool call, as first announced.
    ToolCall(Tool<'a>),
    /// Fields a tool call changed. A field that is absent did not change.
    ToolUpdate(Tool<'a>),
    Plan(Vec<PlanStep>),
    Usage {
        used: u64,
        size: u64,
    },
    /// An update neither side draws.
    Ignored,
}

/// A tool call's fields, with kind and status in the wire's words. Content and
/// locations stay as sent: only the restore sink draws them.
struct Tool<'a> {
    id: &'a str,
    title: Option<&'a str>,
    kind: Option<String>,
    status: Option<String>,
    locations: Option<&'a [ToolCallLocation]>,
    content: Option<&'a [ToolCallContent]>,
}

impl Tool<'_> {
    /// A new row from these fields.
    fn row(self) -> Replayed {
        Replayed::ToolCall {
            title: self.title.unwrap_or_default().to_string(),
            kind: self.kind,
            status: self.status,
            locations: self.locations.map(places).unwrap_or_default(),
            content: self.content.map(pieces).unwrap_or_default(),
            id: self.id.to_string(),
        }
    }

    /// An update replaces each field it carries whole, as ACP does.
    fn fold_into(self, row: &mut Replayed) {
        let Replayed::ToolCall {
            title,
            kind,
            status,
            locations,
            content,
            ..
        } = row
        else {
            return;
        };
        if let Some(changed) = self.title {
            *title = changed.to_string();
        }
        if let Some(changed) = self.kind {
            *kind = Some(changed);
        }
        if let Some(changed) = self.status {
            *status = Some(changed);
        }
        if let Some(changed) = self.locations {
            *locations = places(changed);
        }
        if let Some(changed) = self.content {
            *content = pieces(changed);
        }
    }
}

fn hear(update: &SessionUpdate) -> Heard<'_> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => Heard::Said {
            message: chunk.message_id.as_ref(),
            block: &chunk.content,
        },
        SessionUpdate::AgentThoughtChunk(chunk) => Heard::Thought(&chunk.content),
        SessionUpdate::UserMessageChunk(chunk) => Heard::Prompt(&chunk.content),
        SessionUpdate::ToolCall(call) => Heard::ToolCall(Tool {
            id: &call.tool_call_id.0,
            title: Some(&call.title),
            kind: Some(name_of(&call.kind)),
            status: Some(name_of(&call.status)),
            locations: Some(&call.locations),
            content: Some(&call.content),
        }),
        SessionUpdate::ToolCallUpdate(update) => Heard::ToolUpdate(Tool {
            id: &update.tool_call_id.0,
            title: update.fields.title.as_deref(),
            kind: update.fields.kind.as_ref().map(name_of),
            status: update.fields.status.as_ref().map(name_of),
            locations: update.fields.locations.as_deref(),
            content: update.fields.content.as_deref(),
        }),
        SessionUpdate::Plan(plan) => Heard::Plan(plan_steps(plan)),
        SessionUpdate::UsageUpdate(usage) => Heard::Usage {
            used: usage.used,
            size: usage.size,
        },
        _ => Heard::Ignored,
    }
}

fn plan_steps(plan: &Plan) -> Vec<PlanStep> {
    plan.entries
        .iter()
        .map(|entry| PlanStep {
            content: entry.content.clone(),
            priority: name_of(&entry.priority),
            status: name_of(&entry.status),
        })
        .collect()
}

fn trimmed(text: String) -> Option<String> {
    Some(text.trim().to_string()).filter(|text| !text.is_empty())
}

/// ACP v1 says nothing when a turn the Harness started is over. Flush any
/// accumulated between-turn speech and close open thinking when Fidget prompts
/// or closes that session.
fn end_inbound(inbound: &mut HashMap<SessionId, Inbound>, id: &SessionId, on_event: &OnEvent) {
    flush_inbound(id, inbound, on_event);
}

/// One message with no `session/prompt` open, held as a turn would hold it.
/// `signing_in` is whether Fidget's own `authenticate` is in flight.
/// Between-turn updates accumulate in `Inbound`; flushes emit inbound wakes.
fn between_turns(
    message: Incoming,
    forms: &mut Vec<PendingElicit>,
    asks: &mut Vec<PendingAsk>,
    inbound: &mut HashMap<SessionId, Inbound>,
    signing_in: bool,
    on_event: &OnEvent,
) {
    match message {
        Incoming::Update(update) => {
            let session_id = update.session_id.clone();
            let heard = hear(&update.update);
            if let Heard::Said {
                message: Some(new), ..
            } = &heard
            {
                let next_fire = inbound
                    .get(&session_id)
                    .and_then(|held| held.message.as_ref())
                    .is_some_and(|held| held != *new);
                if next_fire {
                    flush_inbound(&session_id, inbound, on_event);
                }
            }
            let held = inbound.entry(session_id.clone()).or_default();
            show(
                heard,
                &session_id,
                &mut held.said,
                &mut held.thought,
                &mut held.message,
                on_event,
            );
        }
        Incoming::Ask(request, responder) => {
            flush_inbound(&request.session_id, inbound, on_event);
            hold_ask(asks, &request, responder, on_event);
        }
        Incoming::Elicit(request, responder) => {
            if let Some(session_id) = elicitation_session(&request) {
                flush_inbound(&session_id, inbound, on_event);
            }
            hold_form(forms, &request, responder, signing_in, on_event);
        }
        Incoming::Complete(link) => complete_form(forms, &link, on_event),
    }
}

/// Flush accumulated between-turn agent text and thinking as an inbound wake.
/// Called when a between-turn ask or form arrives, giving each Harness fire
/// a coherent wake boundary. Clears the entry so multiple fires don't pile up.
fn flush_inbound(
    session: &SessionId,
    inbound: &mut HashMap<SessionId, Inbound>,
    on_event: &OnEvent,
) {
    if let Some(held) = inbound.remove(session) {
        let speech = held.said.finish();
        if !speech.is_empty() {
            on_event(Event::InboundWake {
                session: session.0.to_string(),
                speech,
            });
        }
        if !held.thought.is_empty() {
            on_event(Event::Thought {
                session: session.0.to_string(),
                text: String::new(),
            });
        }
    }
}

/// The session a message from the Harness names, if it names one.
fn incoming_session(message: &Incoming) -> Option<SessionId> {
    match message {
        Incoming::Update(update) => Some(update.session_id.clone()),
        Incoming::Ask(request, _) => Some(request.session_id.clone()),
        Incoming::Elicit(request, _) => elicitation_session(request),
        Incoming::Complete(_) => None,
    }
}

/// The session id from an elicitation request, when it's session-scoped.
fn elicitation_session(request: &CreateElicitationRequest) -> Option<SessionId> {
    match &request.mode {
        ElicitationMode::Url(link) => match &link.scope {
            ElicitationScope::Session(scope) => Some(scope.session_id.clone()),
            _ => None,
        },
        ElicitationMode::Form(form) => match &form.scope {
            ElicitationScope::Session(scope) => Some(scope.session_id.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// One `McpChoice` in the protocol's own words.
/// The token rides in a header, not the URL or an argv, so it is neither
/// written to a config file the Harness keeps nor visible in a process list.
fn mcp_server(choice: &McpChoice) -> McpServer {
    match choice {
        McpChoice::Http { url, authorization } => {
            McpServer::Http(McpServerHttp::new("fidget", url.clone()).headers(vec![
                HttpHeader::new("Authorization", authorization.clone()),
            ]))
        }
        McpChoice::Stdio(launch) => McpServer::Stdio(
            McpServerStdio::new("fidget", launch.path.clone())
                .args(launch.args.clone())
                .env(
                    launch
                        .env
                        .iter()
                        .map(|(name, value)| EnvVariable::new(name.clone(), value.clone()))
                        .collect(),
                ),
        ),
    }
}

fn claude_mcp_meta(approve: bool) -> Option<Meta> {
    approve.then(|| {
        Meta::from_iter([(
            "claudeCode".to_string(),
            serde_json::json!({"options": {"allowedTools": ["mcp__fidget__*"]}}),
        )])
    })
}

/// `session/load` when asked and answered, else `session/new`. Raw requests
/// rather than the SDK's session builders. Those tear the connection down
/// when the Harness refuses, and `auth_required` is a refusal we recover from.
async fn open(
    cx: &ConnectionTo<Agent>,
    load: Option<String>,
    cwd: &Path,
    mcp: Option<McpChoice>,
    claude_mcp_approval: bool,
    model: &str,
    effort: Option<&str>,
) -> Result<(SessionId, Vec<SessionConfigOption>, Applied), OpenError> {
    let servers = || -> Vec<McpServer> { mcp.iter().map(mcp_server).collect() };
    let approval = || claude_mcp_meta(claude_mcp_approval && mcp.is_some());
    let loaded = if let Some(id) = load {
        match cx
            .send_request(
                LoadSessionRequest::new(id.clone(), cwd)
                    .mcp_servers(servers())
                    .meta(approval()),
            )
            .block_task()
            .await
        {
            Ok(response) => Some((SessionId::new(id), response.config_options)),
            Err(_) => None,
        }
    } else {
        None
    };
    // A loaded id is already on disk. A new one is closed if the Completer
    // config is rejected, so a refusal does not leave the session open.
    let created = loaded.is_none();
    let (session_id, options) = match loaded {
        Some(opened) => opened,
        None => cx
            .send_request(
                NewSessionRequest::new(cwd)
                    .mcp_servers(servers())
                    .meta(approval()),
            )
            .block_task()
            .await
            .map(|response| (response.session_id, response.config_options))
            .map_err(|error| {
                if auth_refused(&error) {
                    OpenError::AuthRequired
                } else if cx.is_incoming_closed() {
                    OpenError::Lost
                } else {
                    OpenError::Failed(error_text(error))
                }
            })?,
    };
    match apply_completer(cx, &session_id, model, effort, options.unwrap_or_default()).await {
        Ok((listed, applied)) => Ok((session_id, listed, applied)),
        Err(error) => {
            if created && !matches!(error, OpenError::Lost) {
                let _ = cx
                    .send_request(CloseSessionRequest::new(session_id))
                    .block_task()
                    .await;
            }
            Err(error)
        }
    }
}

/// Set the model, then the effort. A model change can replace the effort options,
/// so the effort goes out against the options the model call returned. A value with
/// no option is not sent. A value counts as set only when the answer lists the
/// option at that value.
async fn apply_completer(
    cx: &ConnectionTo<Agent>,
    session_id: &SessionId,
    model: &str,
    effort: Option<&str>,
    mut options: Vec<SessionConfigOption>,
) -> Result<(Vec<SessionConfigOption>, Applied), OpenError> {
    let mut applied = Applied::default();
    let model = model.trim();
    if !model.is_empty() {
        let outcome = match model_config_id(&options) {
            Some(config_id) => {
                options =
                    set_config(cx, session_id, config_id.clone(), model, Field::Model).await?;
                confirm(&options, &config_id, model)
            }
            None => Outcome::Unadvertised,
        };
        applied.outcomes.push((Field::Model, outcome));
    }
    if let Some(effort) = effort.map(str::trim).filter(|effort| !effort.is_empty()) {
        let outcome = match effort_config_id(&options) {
            Some(config_id) => {
                options =
                    set_config(cx, session_id, config_id.clone(), effort, Field::Effort).await?;
                confirm(&options, &config_id, effort)
            }
            None => Outcome::Unadvertised,
        };
        applied.outcomes.push((Field::Effort, outcome));
    }
    for (field, outcome) in &applied.outcomes {
        match outcome {
            Outcome::Confirmed => {}
            Outcome::Unconfirmed => eprintln!(
                "harness: the {} set was answered, but the session does not list it at the new value",
                field.name()
            ),
            Outcome::Unadvertised => eprintln!(
                "harness: the session lists no {} option, so it was not set",
                field.name()
            ),
        }
    }
    Ok((options, applied))
}

/// Whether the options a set answered with list `config_id` at `value`.
fn confirm(options: &[SessionConfigOption], config_id: &SessionConfigId, value: &str) -> Outcome {
    let shows_value = options.iter().any(|option| {
        &option.id == config_id
            && matches!(&option.kind,
                SessionConfigKind::Select(select) if select.current_value.0.as_ref() == value)
    });
    if shows_value {
        Outcome::Confirmed
    } else {
        Outcome::Unconfirmed
    }
}

async fn set_config(
    cx: &ConnectionTo<Agent>,
    session_id: &SessionId,
    config_id: SessionConfigId,
    value: &str,
    field: Field,
) -> Result<Vec<SessionConfigOption>, OpenError> {
    match cx
        .send_request(SetSessionConfigOptionRequest::new(
            session_id.clone(),
            config_id,
            value,
        ))
        .block_task()
        .await
    {
        Ok(response) => Ok(response.config_options),
        Err(_) if cx.is_incoming_closed() => Err(OpenError::Lost),
        Err(error) => Err(OpenError::Refused(field, error_text(error))),
    }
}

/// First option whose category is `model`.
fn model_config_id(options: &[SessionConfigOption]) -> Option<SessionConfigId> {
    options
        .iter()
        .find(|option| option.category.as_ref() == Some(&SessionConfigOptionCategory::Model))
        .map(|option| option.id.clone())
}

/// `thought_level` wins over `model_config`. A non-select is not a level.
fn effort_config_id(options: &[SessionConfigOption]) -> Option<SessionConfigId> {
    let select = |category: SessionConfigOptionCategory| {
        options.iter().find(|option| {
            option.category.as_ref() == Some(&category)
                && matches!(option.kind, SessionConfigKind::Select(_))
        })
    };
    select(SessionConfigOptionCategory::ThoughtLevel)
        .or_else(|| select(SessionConfigOptionCategory::ModelConfig))
        .map(|option| option.id.clone())
}

/// One prompt turn. Chunks accumulate, other updates become `Event`s, and a
/// permission request is held open until `Answer` or `Cancel`.
/// "The turn finished" is read here, not on an idle `state_update` from ACP v2.
async fn turn(
    cx: &ConnectionTo<Agent>,
    session: &SessionId,
    serving: Serving<'_>,
    text: &str,
    reply: &sync_mpsc::Sender<Progress>,
    on_event: &OnEvent,
) -> Result<Reply, TurnError> {
    let Serving {
        rx,
        incoming,
        held,
        held_asks,
        inbound,
    } = serving;
    let sent = cx.send_request(PromptRequest::new(
        session.clone(),
        vec![ContentBlock::Text(TextContent::new(text.to_string()))],
    ));
    let mut finished = std::pin::pin!(sent.block_task());
    let mut said = Answer::default();
    // Beside `said` and never inside it. The thought is kept only so a chunk
    // that arrives mid-sentence can be shown as the sentence it belongs to.
    // It dies with the turn.
    let mut thought = String::new();
    let mut open_message = None;
    let mut asks: Vec<PendingAsk> = Vec::new();
    let mut forms: Vec<PendingElicit> = Vec::new();
    // Reported on the transition only, and at the top of the loop rather than
    // in the arms that push or settle: several asks can be open at once, and
    // a Cancel that drains them all has to restart the budget too, or a
    // Harness that ignores the cancel leaves `prompt` waiting for nothing.
    let mut blocked = false;
    loop {
        let open = !asks.is_empty() || !forms.is_empty();
        if open != blocked {
            blocked = open;
            let _ = reply.send(if open {
                Progress::Asked
            } else {
                Progress::Settled
            });
        }
        // `biased`, updates first. The SDK dispatches a turn's chunks before
        // its response, so the response is read only once the channel ahead
        // of it is empty, and no chunk is left behind on the way out.
        tokio::select! {
            biased;
            message = incoming.recv() => match message {
                // Another session's update or question is not this turn's.
                // It is held the way it would be with no prompt open.
                Some(message) if incoming_session(&message).is_some_and(|other| &other != session) => {
                    between_turns(message, held, held_asks, inbound, false, on_event)
                }
                Some(Incoming::Update(update)) => {
                    let heard = hear(&update.update);
                    let answered = matches!(heard, Heard::Said { .. });
                    show(
                        heard,
                        session,
                        &mut said,
                        &mut thought,
                        &mut open_message,
                        on_event,
                    );
                    if answered {
                        let _ = reply.send(Progress::Said(said.so_far().to_string()));
                    }
                }
                Some(Incoming::Ask(request, responder)) => {
                    hold_ask(&mut asks, &request, responder, on_event)
                }
                // A link that outlives the turn goes to `serve`'s forms, so
                // `end_turn` does not cancel it and the budget does not stop.
                Some(Incoming::Elicit(request, responder)) if outlives_turn(&request) => {
                    hold_form(held, &request, responder, false, on_event)
                }
                Some(Incoming::Elicit(request, responder)) => {
                    hold_form(&mut forms, &request, responder, false, on_event)
                }
                // Either list may hold it: a link can wait from before the turn.
                Some(Incoming::Complete(link)) => {
                    complete_form(&mut forms, &link, on_event);
                    complete_form(held, &link, on_event);
                }
                None => {
                    end_turn(session, &mut asks, &mut forms, &mut thought, on_event);
                    return Err(TurnError::Lost);
                }
            },
            response = &mut finished => {
                end_turn(session, &mut asks, &mut forms, &mut thought, on_event);
                return match response {
                    Ok(response) => outcome(response.stop_reason, said.finish()),
                    Err(error) if auth_refused(&error) => Err(TurnError::AuthRequired),
                    Err(_) if cx.is_incoming_closed() => Err(TurnError::Lost),
                    Err(error) => Err(TurnError::Failed(error_text(error))),
                };
            }
            command = rx.recv() => match command {
                Some(Msg::Cancel) => {
                    let _ = cx.send_notification(CancelNotification::new(session.clone()));
                    end_turn(session, &mut asks, &mut forms, &mut thought, on_event);
                }
                // Shutdown is not Cancel (#634). The turn leaves with the wire
                // rather than looping for a `cancelled` stop the Harness may
                // never send. The reply is lost. The sender kills the child next.
                Some(Msg::Shutdown) => {
                    let _ = cx.send_notification(CancelNotification::new(session.clone()));
                    end_turn(session, &mut asks, &mut forms, &mut thought, on_event);
                    return Err(TurnError::Lost);
                }
                // Either list may hold it: an ask can wait from between turns.
                Some(Msg::Answer { request, option }) => {
                    let mine = asks.iter().any(|(id, _)| *id == request);
                    let from = if mine { &mut asks } else { &mut *held_asks };
                    answer_ask(from, request, option, on_event)
                }
                Some(Msg::AnswerElicitation { request, answer }) => {
                    let mine = forms.iter().any(|pending| pending.form.request == request);
                    let from = if mine { &mut forms } else { &mut *held };
                    answer_form(from, request, answer, on_event)
                }
                Some(Msg::Prompt { reply, .. }) => {
                    let _ = reply.send(Progress::Done(Err(TurnError::Busy)));
                }
                Some(Msg::Open { reply, .. }) => {
                    let _ = reply.send(Err(OpenError::Failed("a turn is in flight".to_string())));
                }
                Some(Msg::Configure { reply, .. }) => {
                    let _ = reply.send(Err(OpenError::Failed("a turn is in flight".to_string())));
                }
                // Not sent under `session/prompt`. The click hears it now,
                // instead of waiting out the sign-in budget on a dropped message.
                Some(Msg::Authenticate { reply, .. }) => {
                    let _ = reply.send(Err("a turn is in flight".to_string()));
                }
                Some(Msg::Close { reply, .. }) => {
                    let _ = reply.send(Err("a turn is in flight".to_string()));
                }
                None => {
                    end_turn(session, &mut asks, &mut forms, &mut thought, on_event);
                    return Err(TurnError::Lost);
                }
            },
            () = cx.incoming_closed() => {
                end_turn(session, &mut asks, &mut forms, &mut thought, on_event);
                return Err(TurnError::Lost);
            }
        }
    }
}

/// Every field of the tool call a consent row could read, as plain values.
/// A diff contributes its path. `name` is absent because the SDK gates it
/// behind `unstable_tool_call_name`, which this crate does not enable.
fn permission_ask(request: &RequestPermissionRequest, id: String) -> PermissionAsk {
    let fields = &request.tool_call.fields;
    let mut locations: Vec<String> = fields
        .locations
        .iter()
        .flatten()
        .map(|at| at.path.display().to_string())
        .collect();
    let mut content = Vec::new();
    for piece in fields.content.iter().flatten() {
        match piece {
            ToolCallContent::Content(block) => {
                // Not drawn here yet unless it is text. A replayed tool
                // row marks the other blocks in `tool_content`.
                if let ContentBlock::Text(text) = &block.content {
                    content.push(text.text.clone());
                }
            }
            // A diff is a file the call would rewrite, and the path is the
            // part a consent row can use. Its text is a whole new file.
            // The same path can arrive both ways and is still one path.
            ToolCallContent::Diff(diff) => {
                let path = diff.path.display().to_string();
                if !locations.contains(&path) {
                    locations.push(path);
                }
            }
            // Not drawn on the consent row yet. A replayed tool row
            // names a terminal, and marks any other variant, in
            // `tool_content`.
            _ => {}
        }
    }
    PermissionAsk {
        request: id,
        title: fields.title.clone(),
        kind: fields.kind.as_ref().map(name_of),
        content,
        input: fields.raw_input.clone(),
        locations,
        options: request
            .options
            .iter()
            .map(|option| PermissionOption {
                id: option.option_id.0.to_string(),
                name: option.name.clone(),
                kind: Some(name_of(&option.kind)),
            })
            .collect(),
    }
}

/// One form, as Chat can draw it. An unknown mode keeps the message and no
/// options, so Decline is the only answer.
fn elicitation_form(request: &CreateElicitationRequest, id: String) -> ElicitationForm {
    let (field, options) = match &request.mode {
        ElicitationMode::Form(form) => first_choice_field(&form.requested_schema.properties),
        _ => (String::new(), Vec::new()),
    };
    let url = match &request.mode {
        // What `open_link` would open, so the row shows the host Open goes to.
        // Parsing drops tabs and newlines and punycodes a lookalike host.
        ElicitationMode::Url(link) => {
            Some(crate::platform::openable(&link.url).unwrap_or_else(|_| link.url.clone()))
        }
        _ => None,
    };
    ElicitationForm {
        request: id,
        message: request.message.clone(),
        url,
        field,
        options,
        waits: false,
    }
}

fn first_choice_field(
    properties: &BTreeMap<String, ElicitationPropertySchema>,
) -> (String, Vec<ElicitationChoice>) {
    for (field, property) in properties {
        let ElicitationPropertySchema::String(schema) = property else {
            continue;
        };
        let options = if let Some(one_of) = &schema.one_of {
            one_of
                .iter()
                .map(|option| ElicitationChoice {
                    value: option.value.clone(),
                    name: option.title.clone(),
                })
                .collect()
        } else if let Some(values) = &schema.enum_values {
            values
                .iter()
                .map(|value| ElicitationChoice {
                    value: value.clone(),
                    name: value.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        if !options.is_empty() {
            return (field.clone(), options);
        }
    }
    (String::new(), Vec::new())
}

fn elicitation_response(
    form: &ElicitationForm,
    answer: ElicitationAnswer,
) -> CreateElicitationResponse {
    let allowed = |value: &str| form.options.iter().any(|option| option.value == value);
    match answer {
        ElicitationAnswer::Accept(_) if form.url.is_some() => CreateElicitationResponse::new(
            ElicitationAction::Accept(ElicitationAcceptAction::new()),
        ),
        ElicitationAnswer::Accept(value) if !form.field.is_empty() && allowed(&value) => {
            let mut content = BTreeMap::new();
            content.insert(form.field.clone(), ElicitationContentValue::from(value));
            CreateElicitationResponse::new(ElicitationAction::Accept(
                ElicitationAcceptAction::new().content(content),
            ))
        }
        ElicitationAnswer::Accept(_) | ElicitationAnswer::Decline => {
            CreateElicitationResponse::new(ElicitationAction::Decline)
        }
    }
}

/// The live sink: one update heard, into the turn's text or an `Event`.
/// A different `messageId` is the next message. A missing id stays with the
/// message already open, as a harness that omits it does.
fn message_changed(open: Option<&MessageId>, next: Option<&MessageId>) -> bool {
    match (open, next) {
        (Some(open), Some(next)) => open != next,
        _ => false,
    }
}

/// The next message starts a paragraph. A mark that already closed on a blank
/// line keeps that one break.
fn break_paragraph(text: &mut String) {
    if text.is_empty() || text.ends_with("\n\n") {
        return;
    }
    if text.ends_with('\n') {
        text.push('\n');
    } else {
        text.push_str("\n\n");
    }
}

fn show(
    heard: Heard<'_>,
    session: &SessionId,
    said: &mut Answer,
    thought: &mut String,
    message: &mut Option<MessageId>,
    on_event: &OnEvent,
) {
    let think = |thought: &mut String, text: &str| {
        if text.is_empty() {
            return;
        }
        thought.push_str(text);
        if let Some(shown) = thought_to_show(thought) {
            on_event(Event::Thought {
                session: session.0.to_string(),
                text: shown.to_string(),
            });
        }
    };
    match heard {
        Heard::Said {
            message: incoming,
            block,
        } => {
            if message_changed(message.as_ref(), incoming) {
                said.separate();
            }
            if let Some(id) = incoming {
                *message = Some(id.clone());
            }
            think(thought, &said.hear(block));
        }
        // None leaves the row's previous content or locations. An update
        // replaces only the fields it carries, as the replay does.
        Heard::ToolCall(tool) | Heard::ToolUpdate(tool) => on_event(Event::ToolCall {
            session: session.0.to_string(),
            id: tool.id.to_string(),
            title: tool.title.map(str::to_string),
            kind: tool.kind,
            status: tool.status,
            locations: tool.locations.map(places),
            content: tool.content.map(pieces),
        }),
        Heard::Plan(steps) => on_event(Event::Plan {
            session: session.0.to_string(),
            steps,
        }),
        Heard::Usage { used, size } => on_event(Event::Usage {
            session: session.0.to_string(),
            used,
            size,
        }),
        // Never into `said`. That is the Director's reply, whose first line
        // has to parse as a Behavior name and whose rest the character says out
        // loud. Reasoning is neither, so it leaves by its own door (ADR-0034).
        Heard::Thought(block) => {
            let text = chunk_text(block, thought);
            think(thought, &text);
        }
        // Live draws the user's row from the session log.
        Heard::Prompt(_) | Heard::Ignored => {}
    }
}

/// The thought so far, once it holds something other than whitespace.
///
/// Every line stays, blank ones too: the Thinking row draws the whole thought,
/// and a paragraph break is something the harness wrote. `None` while an
/// adapter has streamed only empty thinking blocks.
pub(crate) fn thought_to_show(thought: &str) -> Option<&str> {
    if thought.trim().is_empty() {
        None
    } else {
        Some(thought)
    }
}

/// Answer text with its `<think>` and `<thinking>` blocks peeled off as it
/// streams. The tags are how a server marks reasoning when it never opened a
/// thought channel. Untagged text is the answer: nothing else marks a split.
#[derive(Default)]
pub(crate) struct Answer {
    said: String,
    /// A tail that may be the start of a tag, held until the next chunk says.
    held: String,
    thinking: bool,
}

const OPEN: [&str; 2] = ["<think>", "<thinking>"];
const CLOSE: [&str; 2] = ["</think>", "</thinking>"];

impl Answer {
    /// Adds a chunk to the answer and returns the part that sat inside tags.
    pub(crate) fn push(&mut self, chunk: &str) -> String {
        let mut rest = std::mem::take(&mut self.held) + chunk;
        let mut thought = String::new();
        loop {
            let found_open = OPEN
                .iter()
                .filter_map(|tag| rest.find(tag).map(|at| (at, tag.len(), true)))
                .min();
            let found_close = CLOSE
                .iter()
                .filter_map(|tag| rest.find(tag).map(|at| (at, tag.len(), false)))
                .min();
            let found = match (found_open, found_close) {
                (Some((at_o, len_o, _)), Some((at_c, _, _))) if at_o < at_c => {
                    Some((at_o, len_o, true))
                }
                (Some(_), Some((at_c, len_c, _))) => Some((at_c, len_c, false)),
                (Some(o), None) => Some(o),
                (None, Some(c)) => Some(c),
                (None, None) => None,
            };

            let Some((at, len, is_open)) = found else {
                let keep = OPEN
                    .iter()
                    .chain(CLOSE.iter())
                    .flat_map(|tag| (1..tag.len()).filter(|&k| rest.ends_with(&tag[..k])))
                    .max()
                    .unwrap_or(0);
                self.held = rest.split_off(rest.len() - keep);
                if self.thinking {
                    thought.push_str(&rest);
                } else {
                    self.said.push_str(&rest);
                }
                return thought;
            };

            if is_open {
                if self.thinking {
                    thought.push_str(&rest[..at]);
                } else {
                    self.said.push_str(&rest[..at]);
                }
                rest.drain(..at + len);
                self.thinking = true;
            } else {
                if self.thinking {
                    thought.push_str(&rest[..at]);
                    rest.drain(..at + len);
                    self.thinking = false;
                } else {
                    thought.push_str(&std::mem::take(&mut self.said));
                    thought.push_str(&rest[..at]);
                    rest.drain(..at + len);
                }
            }
        }
    }

    /// The next message is its own paragraph in the answer.
    fn separate(&mut self) {
        break_paragraph(&mut self.said);
    }

    /// Adds a chunk's block to the answer, a mark as its own paragraph, and
    /// returns the part that sat inside tags.
    pub(crate) fn hear(&mut self, block: &ContentBlock) -> String {
        let text = chunk_text(block, self.so_far());
        self.push(&text)
    }

    /// The answer so far, short of a tail that may yet open a tag.
    pub(crate) fn so_far(&self) -> &str {
        &self.said
    }

    /// The whole answer. A held tail that never became a tag is answer text,
    /// unless it sits in a block that never closed.
    pub(crate) fn finish(mut self) -> String {
        if !self.thinking {
            self.said.push_str(&self.held);
        }
        self.said
    }
}

/// A link scoped to the session and no tool call, as an MCP server's sign-in
/// after `session/new` is. It belongs to the session, not the turn it lands in.
fn outlives_turn(request: &CreateElicitationRequest) -> bool {
    session_link(request).is_some_and(|scope| scope.tool_call_id.is_none())
}

/// A link tied to a tool call, which blocks its turn until answered.
fn on_tool_call(request: &CreateElicitationRequest) -> bool {
    session_link(request).is_some_and(|scope| scope.tool_call_id.is_some())
}

/// The scope of a URL form tied to a session rather than a request.
fn session_link(request: &CreateElicitationRequest) -> Option<&ElicitationSessionScope> {
    match &request.mode {
        ElicitationMode::Url(link) => match &link.scope {
            ElicitationScope::Session(scope) => Some(scope),
            _ => None,
        },
        _ => None,
    }
}

/// A form the Harness asked, held open and handed on to Chat. `signing_in` is
/// whether Fidget's own `authenticate` is in flight.
fn hold_form(
    forms: &mut Vec<PendingElicit>,
    request: &CreateElicitationRequest,
    responder: Responder<CreateElicitationResponse>,
    signing_in: bool,
    on_event: &OnEvent,
) {
    let mut form = elicitation_form(request, responder.id().to_string());
    form.waits = form.url.is_some() && !signing_in && !on_tool_call(request);
    let link = match &request.mode {
        ElicitationMode::Url(link) => Some(link.elicitation_id.clone()),
        _ => None,
    };
    forms.push(PendingElicit {
        form: form.clone(),
        responder,
        link,
    });
    on_event(Event::Elicitation {
        session: elicitation_session(request).map(|session| session.0.to_string()),
        form,
    });
}

/// A permission request the Harness asked, held open and handed on to Chat.
fn hold_ask(
    asks: &mut Vec<PendingAsk>,
    request: &RequestPermissionRequest,
    responder: Responder<RequestPermissionResponse>,
    on_event: &OnEvent,
) {
    let ask = permission_ask(request, responder.id().to_string());
    asks.push((ask.request.clone(), responder));
    on_event(Event::Permission {
        session: request.session_id.0.to_string(),
        ask,
    });
}

/// The user's pick on the open ask it names, if that ask is still open.
fn answer_ask(asks: &mut Vec<PendingAsk>, request: String, option: String, on_event: &OnEvent) {
    let Some(at) = asks.iter().position(|(id, _)| *id == request) else {
        return;
    };
    let (_, responder) = asks.remove(at);
    let _ = responder.respond(RequestPermissionResponse::new(
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option.clone())),
    ));
    on_event(Event::PermissionSettled {
        request,
        option: Some(option),
    });
}

/// Every open ask, answered with the protocol's `cancelled`.
fn cancel_asks(asks: &mut Vec<PendingAsk>, on_event: &OnEvent) {
    for (request, responder) in asks.drain(..) {
        let _ = responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ));
        on_event(Event::PermissionSettled {
            request,
            option: None,
        });
    }
}

/// The user's answer to the open form it names, if that form is still open.
fn answer_form(
    forms: &mut Vec<PendingElicit>,
    request: String,
    answer: ElicitationAnswer,
    on_event: &OnEvent,
) {
    let Some(at) = forms
        .iter()
        .position(|pending| pending.form.request == request)
    else {
        return;
    };
    let pending = forms.remove(at);
    let option = match &answer {
        ElicitationAnswer::Accept(value) => Some(value.clone()),
        ElicitationAnswer::Decline => Some("decline".to_string()),
    };
    let _ = pending
        .responder
        .respond(elicitation_response(&pending.form, answer));
    on_event(Event::PermissionSettled { request, option });
}

/// A link the Harness says is done, retired as an answer would retire it.
/// The user finished elsewhere and made no choice in Chat, so the request is
/// closed with `cancel`. An id no open form holds is ignored.
fn complete_form(forms: &mut Vec<PendingElicit>, link: &ElicitationId, on_event: &OnEvent) {
    let Some(at) = forms
        .iter()
        .position(|pending| pending.link.as_ref() == Some(link))
    else {
        return;
    };
    let pending = forms.remove(at);
    let _ = pending
        .responder
        .respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
    on_event(Event::PermissionSettled {
        request: pending.form.request,
        option: None,
    });
}

/// Every open form, answered with the protocol's `cancel`.
fn cancel_forms(forms: &mut Vec<PendingElicit>, on_event: &OnEvent) {
    for pending in forms.drain(..) {
        let request = pending.form.request;
        let _ = pending
            .responder
            .respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
        on_event(Event::PermissionSettled {
            request,
            option: None,
        });
    }
}

/// Close out what this side was holding for a turn that is over.
/// Open questions get the protocol-mandated `cancelled` reply. The empty
/// thought says the turn stopped thinking; the plan goes dark.
fn end_turn(
    session: &SessionId,
    asks: &mut Vec<PendingAsk>,
    forms: &mut Vec<PendingElicit>,
    thought: &mut String,
    on_event: &OnEvent,
) {
    cancel_asks(asks, on_event);
    cancel_forms(forms, on_event);
    if !thought.is_empty() {
        thought.clear();
        on_event(Event::Thought {
            session: session.0.to_string(),
            text: String::new(),
        });
    }
    // Unconditional. Nothing here remembers whether the turn planned, and
    // threading a flag through every exit path in the turn loop would buy
    // one idempotent event.
    on_event(Event::Plan {
        session: session.0.to_string(),
        steps: Vec::new(),
    });
}

/// What a finished turn is worth, from the reason it stopped and the words
/// it streamed. A cap-ended turn is shown as far as it got and marked. With
/// nothing said it errors like any other stop and the Static Director takes it.
fn outcome(stop: StopReason, said: String) -> Result<Reply, TurnError> {
    match stop {
        StopReason::EndTurn => Ok(Reply::whole(said)),
        StopReason::MaxTokens if !said.trim().is_empty() => Ok(Reply::truncated(said)),
        other => Err(TurnError::Stopped(name_of(&other))),
    }
}

/// The wire spelling of a schema enum (`end_turn`, `execute`, `allow_once`).
fn name_of<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        Ok(other) => other.to_string(),
        Err(_) => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        AuthMethodAgent, ContentChunk, ImageContent, ResourceLink, ToolKind,
    };
    use std::sync::Arc;

    fn thinking(text: &str) -> SessionUpdate {
        SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(TextContent::new(
            text,
        ))))
    }

    fn collector() -> (Arc<Mutex<Vec<Event>>>, OnEvent) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        (
            seen,
            Box::new(move |event| kept.lock().unwrap().push(event)),
        )
    }

    fn drive(updates: Vec<SessionUpdate>) -> (String, Vec<Event>) {
        let (seen, on_event) = collector();
        let mut said = Answer::default();
        let mut thought = String::new();
        let mut message = None;
        for update in updates {
            show(
                hear(&update),
                &SessionId::new("s"),
                &mut said,
                &mut thought,
                &mut message,
                &on_event,
            );
        }
        let events = seen.lock().unwrap().clone();
        (said.finish(), events)
    }

    fn thoughts(events: &[Event]) -> Vec<&str> {
        events
            .iter()
            .map(|event| match event {
                Event::Thought { text, .. } => text.as_str(),
                other => panic!("expected a thought, got {other:?}"),
            })
            .collect()
    }

    /// A cap-ended turn is best effort on both fills. What the agent said is
    /// shown and marked. With nothing said the turn errors and the Static
    /// Director takes it, matching the empty half of the Completer lane.
    #[test]
    fn a_max_tokens_stop_shows_what_it_said_and_errors_when_it_said_nothing() {
        assert_eq!(
            outcome(
                StopReason::MaxTokens,
                "prowl\nMine now, and the".to_string()
            ),
            Ok(Reply::truncated("prowl\nMine now, and the"))
        );
        assert_eq!(
            outcome(StopReason::MaxTokens, "   ".to_string()),
            Err(TurnError::Stopped("max_tokens".to_string())),
            "nothing to show is silence, as it is on the Completer lane"
        );
        assert_eq!(
            outcome(StopReason::EndTurn, "prowl".to_string()),
            Ok(Reply::whole("prowl")),
            "a turn the agent ended is not marked"
        );
        for stop in [
            StopReason::Refusal,
            StopReason::Cancelled,
            StopReason::MaxTurnRequests,
        ] {
            assert_eq!(
                outcome(stop, "half a sentence".to_string()),
                Err(TurnError::Stopped(name_of(&stop))),
                "only the cap is best effort; every other stop errors as before"
            );
        }
    }

    /// The names the Action Log and the Chat surface see are the wire's own
    /// spellings, not Rust's.
    #[test]
    fn names_are_the_wire_spelling() {
        assert_eq!(name_of(&StopReason::EndTurn), "end_turn");
        assert_eq!(name_of(&StopReason::MaxTurnRequests), "max_turn_requests");
        assert_eq!(name_of(&ToolKind::Execute), "execute");
    }

    #[test]
    fn an_auth_method_keeps_its_description_for_the_login_hint() {
        let method =
            AuthMethod::Agent(AuthMethodAgent::new("x", "Sign in").description("run x login"));
        let offer = auth_offer(&method);
        assert_eq!(offer.name(), "Sign in");
        assert_eq!(offer.description(), Some("run x login"));
    }

    /// The shape `session/new` actually puts on the wire. The token rides in
    /// a header and nowhere else (ADR-0023). Not in the URL, which a Harness
    /// may keep in a session file, and not in an argv, which is in a process list.
    #[test]
    fn an_http_choice_carries_the_token_in_a_header_and_not_in_the_url() {
        let server = mcp_server(&McpChoice::Http {
            url: "http://127.0.0.1:51234/mcp".to_string(),
            authorization: "Bearer deadbeef".to_string(),
        });
        let wire = serde_json::to_value(&server).expect("serializes");
        assert_eq!(wire["type"], serde_json::json!("http"));
        assert_eq!(wire["name"], serde_json::json!("fidget"));
        assert_eq!(wire["url"], serde_json::json!("http://127.0.0.1:51234/mcp"));
        assert_eq!(
            wire["headers"][0]["name"],
            serde_json::json!("Authorization")
        );
        assert_eq!(
            wire["headers"][0]["value"],
            serde_json::json!("Bearer deadbeef")
        );
        assert!(!wire["url"].as_str().unwrap().contains("deadbeef"));
    }

    /// The fallback keeps the shape it always had. Every Agent must take stdio,
    /// and the untagged variant is how the protocol spells it.
    #[test]
    fn a_stdio_choice_is_still_a_bare_command_and_carries_its_args() {
        let sidecar = mcp_server(&McpChoice::Stdio(McpLaunch {
            path: PathBuf::from("/opt/fidget-mcp"),
            args: Vec::new(),
            env: Vec::new(),
        }));
        let wire = serde_json::to_value(&sidecar).expect("serializes");
        assert_eq!(wire["command"], serde_json::json!("/opt/fidget-mcp"));
        assert_eq!(wire["name"], serde_json::json!("fidget"));
        assert_eq!(wire["args"], serde_json::json!([]));

        // The app binary re-executed as its own server.
        let embedded = mcp_server(&McpChoice::Stdio(McpLaunch {
            path: PathBuf::from("/opt/fidget"),
            args: vec!["--mcp-stdio".to_string()],
            env: Vec::new(),
        }));
        let wire = serde_json::to_value(&embedded).expect("serializes");
        assert_eq!(wire["command"], serde_json::json!("/opt/fidget"));
        assert_eq!(wire["args"], serde_json::json!(["--mcp-stdio"]));
    }

    /// How the shim is told where to dial. The Harness applies it in `env`
    /// to the child it spawns, not in an argv, which is in a process list.
    #[test]
    fn a_stdio_choice_carries_the_endpoint_in_its_environment() {
        let server = mcp_server(&McpChoice::Stdio(McpLaunch {
            path: PathBuf::from("/opt/fidget-mcp"),
            args: Vec::new(),
            env: vec![
                (
                    "FIDGET_MCP_URL".to_string(),
                    "http://127.0.0.1:51234/mcp".to_string(),
                ),
                ("FIDGET_MCP_TOKEN".to_string(), "deadbeef".to_string()),
            ],
        }));
        let wire = serde_json::to_value(&server).expect("serializes");
        assert_eq!(wire["env"][0]["name"], serde_json::json!("FIDGET_MCP_URL"));
        assert_eq!(
            wire["env"][0]["value"],
            serde_json::json!("http://127.0.0.1:51234/mcp")
        );
        assert_eq!(
            wire["env"][1]["name"],
            serde_json::json!("FIDGET_MCP_TOKEN")
        );
        assert_eq!(wire["env"][1]["value"], serde_json::json!("deadbeef"));
        assert!(!wire["args"].to_string().contains("deadbeef"));
    }

    #[test]
    fn a_label_never_carries_the_token() {
        let choice = McpChoice::Http {
            url: "http://127.0.0.1:1/mcp".to_string(),
            authorization: "Bearer secret".to_string(),
        };
        assert_eq!(choice.label(), "http://127.0.0.1:1/mcp");
        assert!(!choice.label().contains("secret"));

        // The Action Log takes this line too, and the stdio choice carries
        // a token of the same kind.
        let stdio = McpChoice::Stdio(McpLaunch {
            path: PathBuf::from("/opt/fidget-mcp"),
            args: Vec::new(),
            env: vec![("FIDGET_MCP_TOKEN".to_string(), "secret".to_string())],
        });
        assert_eq!(stdio.label(), "/opt/fidget-mcp");
    }

    /// A thought reaches the Shell and never the answer. `said` is the
    /// Director's reply, whose first line has to parse as a Behavior name
    /// and whose rest is spoken out loud. A thought is neither (ADR-0034).
    #[test]
    fn a_thought_is_an_event_and_never_part_of_the_answer() {
        let (said, events) = drive(vec![
            thinking("Reading the roster"),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("wave"),
            ))),
        ]);
        assert_eq!(said, "wave");
        assert_eq!(thoughts(&events), ["Reading the roster"]);
    }

    /// Chunks arrive as fragments. The open line is the tail of the thought
    /// so far, blank lines included: half a sentence on its own reads as
    /// nonsense, and a paragraph break is something the harness wrote.
    fn message(text: &str) -> SessionUpdate {
        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(TextContent::new(
            text,
        ))))
    }

    fn resource_link(name: &str, uri: &str) -> ContentBlock {
        ContentBlock::ResourceLink(ResourceLink::new(name, uri))
    }

    fn picture() -> ContentBlock {
        ContentBlock::Image(ImageContent::new("AAAA", "image/png"))
    }

    /// A turn that only hands over a resource says so. Dropped, it came back
    /// empty and the turn read as silence (ADR-0028).
    #[test]
    fn a_resource_only_turn_is_its_mark_not_nothing() {
        let (said, events) = drive(vec![
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(picture())),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(resource_link(
                "spec",
                "https://example.com/spec.md",
            ))),
        ]);
        assert_eq!(said, "[spec](https://example.com/spec.md)\n\n");
        assert_eq!(thoughts(&events), ["[image image/png]\n\n"]);
    }

    /// A mark is a paragraph between the text before and after it.
    #[test]
    fn a_mark_between_two_chunks_of_text_stays_between_them() {
        let (said, _) = drive(vec![
            message("Here is the shot:"),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(picture())),
            message("Anything else?"),
        ]);
        assert_eq!(
            said,
            "Here is the shot:\n\n[image image/png]\n\nAnything else?"
        );
    }

    /// A load replays the same chunks, and a resource in any of the three
    /// kinds of chunk reaches the entry its text would have.
    #[test]
    fn a_replayed_resource_is_kept_in_its_prompt_thought_and_reply() {
        let mut replay = Restore::default();
        replay.take(hear(&SessionUpdate::UserMessageChunk(ContentChunk::new(
            resource_link("brief", "https://example.com/brief"),
        ))));
        replay.take(hear(&SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            picture(),
        ))));
        replay.take(hear(&SessionUpdate::AgentMessageChunk(ContentChunk::new(
            resource_link("spec", "file:///spec.md"),
        ))));
        assert_eq!(
            replay.finish(),
            vec![
                Replayed::Prompt {
                    text: "[brief](https://example.com/brief)".to_string()
                },
                Replayed::Thought {
                    text: "[image image/png]".to_string()
                },
                Replayed::Reply {
                    text: "[link spec file:///spec.md]".to_string()
                },
            ]
        );
    }

    /// A Harness that never opened `agent_thought_chunk` marks its reasoning
    /// with `<think>` tags in the answer instead. The tag can split across
    /// chunks, and the inside is a thought, never a line the character says.
    #[test]
    fn a_think_tag_in_the_answer_is_a_thought() {
        let (said, events) = drive(vec![
            message("<thin"),
            message("king>They want the titles."),
            message(" I'll look again.</think"),
            message("ing>mutter\nFidget's in front."),
        ]);
        assert_eq!(said, "mutter\nFidget's in front.");
        assert_eq!(
            thoughts(&events),
            [
                "They want the titles.",
                "They want the titles. I'll look again."
            ]
        );
    }

    /// Text that only looks like the start of a tag is the answer once the
    /// next chunk shows it is not one. Untagged reasoning stays the answer.
    #[test]
    fn a_partial_tag_that_never_completes_stays_the_answer() {
        let (said, events) = drive(vec![
            message("They want <th"),
            message("ree> titles.mutter"),
            message(" <think"),
        ]);
        assert_eq!(said, "They want <three> titles.mutter <think");
        assert!(events.is_empty());
    }

    /// Some templates open the reasoning block in the prompt, so model output
    /// starts inside it. A lone closing tag with no opening tag treats the
    /// text before it as reasoning and keeps only what follows.
    #[test]
    fn a_stray_closing_tag_treats_text_before_it_as_reasoning() {
        let (said, events) = drive(vec![
            message("They want the titles. I'll look again."),
            message("</think>mutter\nFidget's in front."),
        ]);
        assert_eq!(said, "mutter\nFidget's in front.");
        assert_eq!(
            thoughts(&events),
            ["They want the titles. I'll look again."]
        );
    }

    /// A stray closing tag split across chunks is held and completed, so the
    /// reasoning boundary is still honored when the tag arrives in fragments.
    #[test]
    fn a_stray_closing_tag_split_across_chunks_is_held() {
        let (said, events) = drive(vec![
            message("They want the titles. I'll look again.</thin"),
            message("k>mutter\nFidget's in front."),
        ]);
        assert_eq!(said, "mutter\nFidget's in front.");
        assert_eq!(
            thoughts(&events),
            ["They want the titles. I'll look again."]
        );
    }

    #[test]
    fn a_thought_shows_the_line_being_written() {
        let (_, events) = drive(vec![
            thinking("Reading the"),
            thinking(" roster.\n\nNow the manifest"),
        ]);
        assert_eq!(
            thoughts(&events),
            ["Reading the", "Reading the roster.\n\nNow the manifest"]
        );
    }

    /// The wire sends the whole thought (#994), so a sixth line and a blank in
    /// the middle both survive.
    #[test]
    fn a_thought_keeps_every_line_including_a_blank() {
        let (_, events) = drive(vec![
            thinking("Reading the"),
            thinking(" roster.\n\nChecking the desk.\nWeighing a nap"),
            thinking(" against the desk.\nCounting the windows.\nNaming the display.\nPicking a"),
            thinking(" spot."),
        ]);
        assert_eq!(
            thoughts(&events),
            [
                "Reading the",
                "Reading the roster.\n\nChecking the desk.\nWeighing a nap",
                "Reading the roster.\n\nChecking the desk.\nWeighing a nap against the desk.\nCounting the windows.\nNaming the display.\nPicking a",
                "Reading the roster.\n\nChecking the desk.\nWeighing a nap against the desk.\nCounting the windows.\nNaming the display.\nPicking a spot.",
            ]
        );
    }

    /// Nothing on the Chat surface knows when a Harness stopped thinking. A turn
    /// that ends without an answer would otherwise leave its row streaming.
    #[test]
    fn the_thinking_goes_dark_when_the_turn_ends() {
        let (seen, on_event) = collector();
        let mut thought = "Reading the roster".to_string();
        let session = SessionId::new("s");
        end_turn(
            &session,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut thought,
            &on_event,
        );
        assert!(matches!(
            seen.lock().unwrap().as_slice(),
            [Event::Thought { text, .. }, Event::Plan { steps, .. }]
                if text.is_empty() && steps.is_empty()
        ));

        // A turn that thought nothing has no row to close. The plan clear is
        // unconditional, so it is all that is left.
        let (seen, on_event) = collector();
        end_turn(
            &session,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut String::new(),
            &on_event,
        );
        assert!(matches!(
            seen.lock().unwrap().as_slice(),
            [Event::Plan { steps, .. }] if steps.is_empty()
        ));
    }

    /// Nothing to show is not a thought. Adapters stream signature-only
    /// thinking blocks whose text is empty, and a blank Thinking row is worse
    /// than none.
    #[test]
    fn a_thought_with_no_words_raises_nothing() {
        let (_, events) = drive(vec![thinking("   \n")]);
        assert!(events.is_empty(), "{events:?}");
    }

    /// Driven off the wire bytes rather than built types, because the
    /// spellings are what the surface draws.
    #[test]
    fn a_plan_update_carries_its_steps_and_not_a_count() {
        let update: SessionUpdate = serde_json::from_value(serde_json::json!({
            "sessionUpdate": "plan",
            "entries": [
                {"content": "read the roster", "priority": "high", "status": "completed"},
                {"content": "write the patch", "priority": "medium", "status": "in_progress"}
            ]
        }))
        .unwrap();

        let (_, events) = drive(vec![update]);

        let [Event::Plan { session, steps }] = events.as_slice() else {
            panic!("a plan update raised {events:?}");
        };
        assert_eq!(session, "s");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].content, "write the patch");
        assert_eq!(steps[1].status, "in_progress");
        assert_eq!(steps[1].priority, "medium");
    }

    /// One `session/update` as the wire spells it.
    fn wire(update: serde_json::Value) -> SessionUpdate {
        serde_json::from_value(update).unwrap()
    }

    fn wire_text(kind: &str, text: &str) -> SessionUpdate {
        wire(serde_json::json!({
            "sessionUpdate": kind,
            "content": {"type": "text", "text": text},
        }))
    }

    /// An agent message chunk that carries a `messageId`. `wire_text` leaves it off.
    fn wire_message(id: &str, text: &str) -> SessionUpdate {
        wire(serde_json::json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": id,
            "content": {"type": "text", "text": text},
        }))
    }

    /// What a load replay of `updates` hands Chat.
    fn restored_rows(updates: &[SessionUpdate]) -> Vec<Replayed> {
        let mut replay = Restore::default();
        for update in updates {
            replay.take(hear(update));
        }
        replay.finish()
    }

    /// The same updates, live and replayed, draw the same rows. The reply is
    /// the turn's answer, the thought is the last thought the live turn
    /// showed, and a tool call is its deltas folded into one row.
    #[test]
    fn one_update_sequence_draws_the_same_rows_live_and_restored() {
        let sequences: Vec<(&str, Vec<SessionUpdate>)> = vec![
            (
                "thought chunks then a reply",
                vec![
                    wire_text("agent_thought_chunk", "Reading the "),
                    wire_text("agent_thought_chunk", "roster"),
                    wire_text("agent_message_chunk", "wave\nHello"),
                ],
            ),
            (
                "a think tag split across chunks",
                vec![
                    message("<thin"),
                    message("king>They want the titles."),
                    message(" I'll look again.</think"),
                    message("ing>mutter\nFidget's in front."),
                ],
            ),
            (
                "marks in a thought and a reply",
                vec![
                    SessionUpdate::AgentThoughtChunk(ContentChunk::new(picture())),
                    message("Here is the shot:"),
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(picture())),
                    message("Anything else?"),
                ],
            ),
            (
                "a stray closing tag",
                vec![message("Looking around.</think>nod | Hi")],
            ),
        ];
        for (name, updates) in sequences {
            let (reply, events) = drive(updates.clone());
            let live_thought = events
                .iter()
                .rev()
                .find_map(|event| match event {
                    Event::Thought { text, .. } if !text.is_empty() => Some(text.trim()),
                    _ => None,
                })
                .unwrap_or("");
            let rows = restored_rows(&updates);
            let restored_reply: Vec<&str> = rows
                .iter()
                .filter_map(|row| match row {
                    Replayed::Reply { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let restored_thought: String = rows
                .iter()
                .filter_map(|row| match row {
                    Replayed::Thought { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(restored_reply, [reply.trim()], "{name}: reply");
            assert_eq!(restored_thought, live_thought, "{name}: thought");
        }
    }

    /// A tool between two agent messages is still one turn. A new `messageId`
    /// breaks the paragraph and does not open a second reply. The words stay
    /// ahead of that tool, and the Behavior line still reads.
    #[test]
    fn one_turn_with_several_message_ids_draws_one_reply_live_and_restored() {
        let updates = vec![
            wire_text("user_message_chunk", "go"),
            wire_message("m1", "nod | Hello"),
            wire(
                serde_json::json!({"sessionUpdate": "tool_call", "toolCallId": "t1",
                "title": "Read file", "kind": "read", "status": "pending"}),
            ),
            wire(
                serde_json::json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1",
                "status": "in_progress"}),
            ),
            wire(
                serde_json::json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1",
                "title": "Edit main.rs", "kind": "edit", "status": "completed"}),
            ),
            wire_message("m2", " there"),
        ];
        let (reply, _) = drive(updates.clone());
        assert_eq!(reply, "nod | Hello\n\n there");
        let behaviors = ["nod".to_string()];
        assert_eq!(
            fidget_core::director::dialogue_of(&reply, &behaviors).as_deref(),
            Some("Hello\n\n there")
        );

        let rows = restored_rows(&updates);
        assert_eq!(
            rows,
            vec![
                Replayed::Prompt {
                    text: "go".to_string()
                },
                Replayed::Reply {
                    text: "nod | Hello\n\n there".to_string()
                },
                Replayed::ToolCall {
                    id: "t1".to_string(),
                    title: "Edit main.rs".to_string(),
                    kind: Some("edit".to_string()),
                    status: Some("completed".to_string()),
                    locations: vec![],
                    content: vec![],
                },
            ]
        );
        let drawn_replies: Vec<String> = crate::session_log::drawn(rows, &behaviors)
            .into_iter()
            .filter_map(|row| match row {
                Replayed::Reply { text } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(drawn_replies, ["Hello\n\n there"]);
    }

    /// A thought chunk between two messages stays above the one reply, where
    /// live Chat files the turn's thought. Later words still join that reply.
    #[test]
    fn a_thought_between_messages_stays_above_the_one_reply() {
        let updates = vec![
            wire_text("user_message_chunk", "go"),
            wire_message("m1", "Hello"),
            wire_text("agent_thought_chunk", "first"),
            wire(
                serde_json::json!({"sessionUpdate": "tool_call", "toolCallId": "t1",
                "title": "Read file", "kind": "read", "status": "pending"}),
            ),
            wire_message("m2", "<think>second</think> there"),
        ];
        let (reply, events) = drive(updates.clone());
        assert_eq!(reply, "Hello\n\n there");
        assert_eq!(
            events.iter().rev().find_map(|event| match event {
                Event::Thought { text, .. } if !text.is_empty() => Some(text.as_str()),
                _ => None,
            }),
            Some("firstsecond")
        );
        assert_eq!(
            restored_rows(&updates),
            vec![
                Replayed::Prompt {
                    text: "go".to_string()
                },
                Replayed::Thought {
                    text: "firstsecond".to_string()
                },
                Replayed::Reply {
                    text: "Hello\n\n there".to_string()
                },
                Replayed::ToolCall {
                    id: "t1".to_string(),
                    title: "Read file".to_string(),
                    kind: Some("read".to_string()),
                    status: Some("pending".to_string()),
                    locations: vec![],
                    content: vec![],
                },
            ]
        );
    }

    /// A `<think>` that arrives after the tool still belongs to that one reply.
    /// `think` would otherwise file it on the tool row, after the words.
    #[test]
    fn a_think_tag_after_a_tool_stays_ahead_of_the_one_reply() {
        let updates = vec![
            wire_text("user_message_chunk", "go"),
            wire_message("m1", "Hello"),
            wire(
                serde_json::json!({"sessionUpdate": "tool_call", "toolCallId": "t1",
                "title": "Read file", "kind": "read", "status": "pending"}),
            ),
            wire_message("m2", "<think>hmm</think> there"),
        ];
        let (reply, events) = drive(updates.clone());
        assert_eq!(reply, "Hello\n\n there");
        assert_eq!(
            events.iter().rev().find_map(|event| match event {
                Event::Thought { text, .. } if !text.is_empty() => Some(text.as_str()),
                _ => None,
            }),
            Some("hmm")
        );
        assert_eq!(
            restored_rows(&updates),
            vec![
                Replayed::Prompt {
                    text: "go".to_string()
                },
                Replayed::Thought {
                    text: "hmm".to_string()
                },
                Replayed::Reply {
                    text: "Hello\n\n there".to_string()
                },
                Replayed::ToolCall {
                    id: "t1".to_string(),
                    title: "Read file".to_string(),
                    kind: Some("read".to_string()),
                    status: Some("pending".to_string()),
                    locations: vec![],
                    content: vec![],
                },
            ]
        );
    }

    /// A user prompt ends the turn, so the next agent message is a new reply.
    /// Live `drive` ignores prompts and would join them; this is restore only.
    #[test]
    fn a_user_prompt_starts_a_new_restored_reply() {
        let updates = vec![
            wire_message("m1", "One"),
            wire_text("user_message_chunk", "next"),
            wire_message("m2", "Two"),
        ];
        let rows = restored_rows(&updates);
        let replies: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                Replayed::Reply { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(replies, ["One", "Two"]);
    }

    /// No replayed user prompt means no turn to join. Each `messageId` is its
    /// own reply, and chunks that share an id stay one message.
    #[test]
    fn a_restore_without_a_user_chunk_keeps_each_message_its_own_reply() {
        let updates = vec![
            wire_message("m1", "Back from lunch."),
            wire_message("m1", " Hello."),
            wire_message("m2", "Build is green."),
        ];
        let rows = restored_rows(&updates);
        let replies: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                Replayed::Reply { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(replies, ["Back from lunch. Hello.", "Build is green."]);
    }

    /// An update that omits content or locations leaves them off the event.
    /// The row keeps what it already drew. A field that arrives replaces it.
    #[test]
    fn a_live_tool_call_carries_the_content_and_locations_an_update_sent() {
        let updates = vec![
            wire(serde_json::json!({
                "sessionUpdate": "tool_call", "toolCallId": "t1",
                "title": "Read file", "kind": "read", "status": "pending",
                "locations": [{"path": "/tmp/roster.json", "line": 4}],
                "content": [{"type": "content", "content": {"type": "text", "text": "waiting"}}],
            })),
            wire(serde_json::json!({
                "sessionUpdate": "tool_call_update", "toolCallId": "t1",
                "status": "completed",
                "content": [{"type": "content", "content": {"type": "text", "text": "names"}}],
            })),
        ];
        let (_, events) = drive(updates.clone());
        let Event::ToolCall {
            title,
            status,
            locations,
            content,
            ..
        } = &events[0]
        else {
            panic!("expected a tool call, got {events:?}");
        };
        assert_eq!(title.as_deref(), Some("Read file"));
        assert_eq!(status.as_deref(), Some("pending"));
        assert_eq!(
            locations.as_deref(),
            Some(
                [Place {
                    path: "/tmp/roster.json".to_string(),
                    line: Some(4),
                }]
                .as_slice()
            )
        );
        assert_eq!(
            content.as_deref(),
            Some(
                [ToolPiece::Text {
                    text: "waiting".to_string()
                }]
                .as_slice()
            )
        );
        let Event::ToolCall {
            title,
            locations,
            content,
            status,
            ..
        } = &events[1]
        else {
            panic!("expected a tool update, got {events:?}");
        };
        assert_eq!(title, &None);
        assert_eq!(locations, &None);
        assert_eq!(status.as_deref(), Some("completed"));
        assert_eq!(
            content.as_deref(),
            Some(
                [ToolPiece::Text {
                    text: "names".to_string()
                }]
                .as_slice()
            )
        );
        assert_eq!(
            restored_rows(&updates),
            vec![Replayed::ToolCall {
                id: "t1".to_string(),
                title: "Read file".to_string(),
                kind: Some("read".to_string()),
                status: Some("completed".to_string()),
                locations: vec![Place {
                    path: "/tmp/roster.json".to_string(),
                    line: Some(4),
                }],
                content: vec![ToolPiece::Text {
                    text: "names".to_string(),
                }],
            }]
        );
    }

    #[test]
    fn one_tool_call_is_deltas_live_and_one_row_restored() {
        let updates = vec![
            wire(
                serde_json::json!({"sessionUpdate": "tool_call", "toolCallId": "t1",
                "title": "Read file", "kind": "read", "status": "pending"}),
            ),
            wire(
                serde_json::json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1",
                "status": "in_progress"}),
            ),
            wire(
                serde_json::json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1",
                "title": "Edit main.rs", "kind": "edit", "status": "completed"}),
            ),
        ];
        let (_, events) = drive(updates.clone());
        let mut folded = (None, None, None);
        for event in events {
            let Event::ToolCall {
                id,
                title,
                kind,
                status,
                ..
            } = event
            else {
                panic!("expected a tool call, got {event:?}");
            };
            assert_eq!(id, "t1");
            folded.0 = title.or(folded.0);
            folded.1 = kind.or(folded.1);
            folded.2 = status.or(folded.2);
        }
        assert_eq!(
            folded,
            (
                Some("Edit main.rs".to_string()),
                Some("edit".to_string()),
                Some("completed".to_string())
            )
        );
        assert_eq!(
            restored_rows(&updates),
            vec![Replayed::ToolCall {
                id: "t1".to_string(),
                title: "Edit main.rs".to_string(),
                kind: Some("edit".to_string()),
                status: Some("completed".to_string()),
                locations: vec![],
                content: vec![],
            }]
        );
    }

    #[test]
    fn the_last_plan_is_the_plan_live_and_restored() {
        let plan = |status: &str| {
            wire(serde_json::json!({"sessionUpdate": "plan", "entries": [
                {"content": "Read the roster", "priority": "medium", "status": status}]}))
        };
        let updates = vec![plan("pending"), plan("completed")];
        let (_, events) = drive(updates.clone());
        let Some(Event::Plan { steps: live, .. }) = events.last() else {
            panic!("no plan event in {events:?}");
        };
        let rows = restored_rows(&updates);
        let [Replayed::Plan { steps }] = rows.as_slice() else {
            panic!("restored {rows:?}");
        };
        assert_eq!(steps, live);
        assert_eq!(steps[0].status, "completed");
    }

    fn inbound(session: &str, message: Option<&str>, text: &str) -> Incoming {
        let mut update = serde_json::json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": text},
        });
        if let Some(id) = message {
            update["messageId"] = id.into();
        }
        Incoming::Update(
            serde_json::from_value(serde_json::json!({"sessionId": session, "update": update}))
                .unwrap(),
        )
    }

    /// Between turns, every update for a session is read and held. Returns
    /// the wakes that came out, then what a close of `s` flushes.
    fn wakes(messages: Vec<Incoming>) -> Vec<(String, String)> {
        let (seen, on_event) = collector();
        let mut inbound = HashMap::new();
        let (mut forms, mut asks) = (Vec::new(), Vec::new());
        for message in messages {
            between_turns(
                message,
                &mut forms,
                &mut asks,
                &mut inbound,
                false,
                &on_event,
            );
        }
        end_inbound(&mut inbound, &SessionId::new("s"), &on_event);
        let events = seen.lock().unwrap().clone();
        events
            .into_iter()
            .filter_map(|event| match event {
                Event::InboundWake { session, speech } => Some((session, speech)),
                _ => None,
            })
            .collect()
    }

    /// Two Harness fires in a row are two wakes. The agent's `messageId`
    /// is what says the second one began (#1435).
    #[test]
    fn a_new_message_id_between_turns_starts_a_new_wake() {
        assert_eq!(
            wakes(vec![
                inbound("s", Some("m1"), "Back from lunch."),
                inbound("s", Some("m1"), " Hello."),
                inbound("s", Some("m2"), "Build is green."),
            ]),
            [
                ("s".to_string(), "Back from lunch. Hello.".to_string()),
                ("s".to_string(), "Build is green.".to_string()),
            ]
        );
    }

    /// A Harness that sends no `messageId` keeps one wake until something
    /// else flushes it, as it did before.
    #[test]
    fn a_harness_without_message_ids_keeps_one_wake() {
        assert_eq!(
            wakes(vec![
                inbound("s", None, "One."),
                inbound("s", None, " Two."),
            ]),
            [("s".to_string(), "One. Two.".to_string())]
        );
        assert_eq!(
            wakes(vec![
                inbound("s", None, "One."),
                inbound("s", Some("m1"), " Two."),
            ]),
            [("s".to_string(), "One. Two.".to_string())]
        );
    }

    /// Another session's id change is not this one's.
    #[test]
    fn a_message_id_is_a_boundary_within_its_own_session_only() {
        let both = wakes(vec![
            inbound("s", Some("m1"), "mine"),
            inbound("other", Some("m2"), "theirs"),
        ]);
        assert_eq!(both, [("s".to_string(), "mine".to_string())]);
    }

    /// One `session/request_permission`, deserialized rather than built, so
    /// the test reads the same wire shape the SDK hands this file.
    fn asked_for(tool_call: serde_json::Value) -> PermissionAsk {
        let request: RequestPermissionRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "s1",
            "toolCall": tool_call,
            "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}],
        }))
        .expect("a permission request");
        permission_ask(&request, "7".to_string())
    }

    /// Nothing downstream can draw what this file does not forward.
    #[test]
    fn an_ask_forwards_the_question_the_arguments_and_the_paths() {
        let ask = asked_for(serde_json::json!({
            "toolCallId": "t1",
            "title": "Question from MCP server",
            "kind": "other",
            "content": [{"type": "content", "content": {"type": "text", "text": "Which branch?"}}],
            "rawInput": {"question": "Which branch?"},
            "locations": [{"path": "/Users/oded/src/main.rs"}],
        }));

        assert_eq!(ask.request, "7");
        assert_eq!(ask.title.as_deref(), Some("Question from MCP server"));
        assert_eq!(ask.kind.as_deref(), Some("other"));
        assert_eq!(ask.content, ["Which branch?"]);
        assert_eq!(
            ask.input,
            Some(serde_json::json!({"question": "Which branch?"}))
        );
        assert_eq!(ask.locations, ["/Users/oded/src/main.rs"]);
        assert_eq!(ask.options.len(), 1);
    }

    /// A diff is a file the call would rewrite, which is the path it touches.
    /// Its text is a whole new file and has no business on a 420 point
    /// surface. The same path arriving both ways is still one path.
    #[test]
    fn a_diff_forwards_as_the_path_it_would_rewrite() {
        let ask = asked_for(serde_json::json!({
            "toolCallId": "t1",
            "kind": "edit",
            "content": [
                {"type": "diff", "path": "/tmp/a.rs", "newText": "fn main() {}"},
                {"type": "diff", "path": "/tmp/b.rs", "newText": "fn other() {}"},
            ],
            "locations": [{"path": "/tmp/a.rs"}],
        }));

        assert!(ask.content.is_empty(), "{:?}", ask.content);
        assert_eq!(ask.locations, ["/tmp/a.rs", "/tmp/b.rs"]);
    }

    /// Content this row cannot read out is not content. An image says nothing
    /// on a text surface, and the arguments still say what was asked.
    #[test]
    fn content_with_no_words_leaves_the_arguments_to_say_it() {
        let ask = asked_for(serde_json::json!({
            "toolCallId": "t1",
            "content": [{
                "type": "content",
                "content": {"type": "image", "data": "AAAA", "mimeType": "image/png"},
            }],
            "rawInput": {"path": "/tmp/a.png"},
        }));

        assert!(ask.content.is_empty(), "{:?}", ask.content);
        assert_eq!(ask.input, Some(serde_json::json!({"path": "/tmp/a.png"})));
    }

    /// A missing title is missing, not `(untitled)`. The surface has to tell
    /// an ask that said nothing from one this file emptied out (#678).
    #[test]
    fn a_titleless_ask_forwards_no_title_rather_than_a_placeholder() {
        let ask = asked_for(serde_json::json!({"toolCallId": "t1"}));

        assert_eq!(ask.title, None);
        assert_eq!(ask.kind, None);
        assert!(ask.content.is_empty());
        assert_eq!(ask.input, None);
        assert!(ask.locations.is_empty());
    }

    fn elicited(schema: serde_json::Value) -> ElicitationForm {
        let request: CreateElicitationRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "s1",
            "mode": "form",
            "message": "How should I approach this refactoring?",
            "requestedSchema": schema,
        }))
        .expect("an elicitation form");
        elicitation_form(&request, "43".to_string())
    }

    #[test]
    fn a_form_forwards_the_first_enum_as_one_question() {
        let form = elicited(serde_json::json!({
            "type": "object",
            "properties": {
                "strategy": {
                    "type": "string",
                    "enum": ["conservative", "balanced", "aggressive"]
                }
            },
            "required": ["strategy"]
        }));

        assert_eq!(form.request, "43");
        assert_eq!(form.message, "How should I approach this refactoring?");
        assert_eq!(form.field, "strategy");
        assert_eq!(form.options.len(), 3);
        assert_eq!(form.options[1].value, "balanced");
        assert_eq!(form.options[1].name, "balanced");
    }

    #[test]
    fn a_titled_enum_keeps_the_title_the_row_draws() {
        let form = elicited(serde_json::json!({
            "type": "object",
            "properties": {
                "strategy": {
                    "type": "string",
                    "oneOf": [
                        {"const": "conservative", "title": "Small steps"},
                        {"const": "aggressive", "title": "Rewrite it"}
                    ]
                }
            }
        }));

        assert_eq!(form.options[0].value, "conservative");
        assert_eq!(form.options[0].name, "Small steps");
    }

    #[test]
    fn accepting_an_option_sends_that_field_as_content() {
        let form = elicited(serde_json::json!({
            "type": "object",
            "properties": {
                "strategy": {"type": "string", "enum": ["conservative", "aggressive"]}
            }
        }));
        let value = serde_json::to_value(elicitation_response(
            &form,
            ElicitationAnswer::Accept("aggressive".to_string()),
        ))
        .expect("serializes");

        assert_eq!(
            value,
            serde_json::json!({
                "action": "accept",
                "content": {"strategy": "aggressive"}
            })
        );
    }

    #[test]
    fn declining_a_form_sends_decline_not_an_error() {
        let form = elicited(serde_json::json!({
            "type": "object",
            "properties": {
                "strategy": {"type": "string", "enum": ["conservative"]}
            }
        }));
        let value = serde_json::to_value(elicitation_response(&form, ElicitationAnswer::Decline))
            .expect("serializes");

        assert_eq!(value, serde_json::json!({"action": "decline"}));
    }

    fn linked(url: &str) -> ElicitationForm {
        let request: CreateElicitationRequest = serde_json::from_value(serde_json::json!({
            "requestId": 7,
            "mode": "url",
            "elicitationId": "e1",
            "message": "Sign in",
            "url": url,
        }))
        .expect("a URL elicitation");
        elicitation_form(&request, "44".to_string())
    }

    /// Production change that would fail this: drawing the Harness's raw
    /// string. `open_link` opens the parsed URL, so the row showed one host
    /// and Open went to another.
    #[test]
    fn a_link_is_drawn_as_the_url_open_opens() {
        let split = linked("https://accounts.example.com\n.evil.test/login");
        assert_eq!(
            split.url.as_deref(),
            Some("https://accounts.example.com.evil.test/login")
        );

        let lookalike = linked("https://аpple.com/");
        assert_eq!(lookalike.url.as_deref(), Some("https://xn--pple-43d.com/"));

        let unparsed = linked("not a url");
        assert_eq!(unparsed.url.as_deref(), Some("not a url"));
    }
}

#[cfg(windows)]
mod windows_job {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenThread, ResumeThread, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
        CREATE_SUSPENDED, PROCESS_ALL_ACCESS, THREAD_SUSPEND_RESUME,
    };

    /// A Job Object handle, parked in `JOBS` until the process exits.
    /// The job carries `KILL_ON_JOB_CLOSE`, so closing the handle kills
    /// the child. `Drop` would be a bug. The leak is intentional.
    struct SafeHandle(HANDLE);

    // SAFETY: a Job Object handle is process-wide and thread-agnostic, so
    // moving it between threads reaches the same object. `HANDLE` is `!Send`
    // only as a raw pointer. Anything in `SafeHandle` has to be that kind too.
    unsafe impl Send for SafeHandle {}

    static JOBS: Mutex<Option<HashMap<u32, SafeHandle>>> = Mutex::new(None);

    /// Spawn with create-time Job Object association via `CREATE_SUSPENDED`.
    /// The process cannot fork children until after job assignment, which
    /// closes the post-spawn race.
    pub(super) fn spawn_in_job(
        mut command: std::process::Command,
        stderr: std::process::Stdio,
    ) -> Result<async_process::Child, std::io::Error> {
        use std::os::windows::process::CommandExt;

        let job = unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() || job == INVALID_HANDLE_VALUE {
                return spawn_fallback_no_job(command, "CreateJobObjectW failed", stderr);
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            if ok == 0 {
                CloseHandle(job);
                return spawn_fallback_no_job(command, "SetInformationJobObject failed", stderr);
            }

            job
        };

        let creation_flags = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_SUSPENDED;
        command.creation_flags(creation_flags);

        let mut async_command = async_process::Command::from(command);
        async_command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(stderr);

        let mut child = match async_command.spawn() {
            Ok(child) => child,
            // Handed on as it came. `SpawnError::start` reads the kind, and a
            // `format!` around it would turn a missing file into prose.
            Err(why) => {
                unsafe { CloseHandle(job) };
                return Err(why);
            }
        };

        let pid = child.id();

        unsafe {
            let process = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
            if process.is_null() || process == INVALID_HANDLE_VALUE {
                // Intentional leak. The job has `KILL_ON_JOB_CLOSE`. Closing it could kill the child.
                eprintln!("harness: OpenProcess failed for pid {pid}; resuming without job");
                if resume_primary_thread(pid).is_err() {
                    let _ = child.kill();
                    return Err(std::io::Error::other(format!(
                        "OpenProcess failed and resume failed for pid {pid}; child killed"
                    )));
                }
                return Ok(child);
            }

            let assigned = AssignProcessToJobObject(job, process);
            CloseHandle(process);

            if assigned == 0 {
                // Intentional leak. The job has `KILL_ON_JOB_CLOSE`. Closing it could kill the child.
                eprintln!(
                    "harness: AssignProcessToJobObject failed for pid {pid}; resuming without job"
                );
                if resume_primary_thread(pid).is_err() {
                    let _ = child.kill();
                    return Err(std::io::Error::other(format!("AssignProcessToJobObject failed and resume failed for pid {pid}; child killed")));
                }
                return Ok(child);
            }

            if let Err(why) = resume_primary_thread(pid) {
                CloseHandle(job);
                let _ = child.kill();
                return Err(std::io::Error::other(format!(
                    "ResumeThread failed for pid {pid}: {why}; child killed"
                )));
            }

            match JOBS.lock() {
                Ok(mut slot) => {
                    let map = slot.get_or_insert_with(HashMap::new);
                    map.insert(pid, SafeHandle(job));
                }
                Err(_) => {
                    eprintln!("harness: JOBS lock poisoned, handle leaked for pid {pid}");
                    CloseHandle(job);
                }
            }
        }

        Ok(child)
    }

    fn resume_primary_thread(pid: u32) -> Result<(), String> {
        unsafe {
            let tid = find_primary_thread(pid)?;
            let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, tid);
            if thread.is_null() || thread == INVALID_HANDLE_VALUE {
                return Err("OpenThread failed".to_string());
            }

            let result = ResumeThread(thread);
            CloseHandle(thread);

            if result == u32::MAX {
                return Err("ResumeThread failed".to_string());
            }

            Ok(())
        }
    }

    fn find_primary_thread(pid: u32) -> Result<u32, String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        };

        // Assumes the first thread enumerated for the PID is the primary
        // thread. For a `CREATE_SUSPENDED` spawn the primary is still
        // suspended, so it is the first found.

        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err("CreateToolhelp32Snapshot failed".to_string());
            }

            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;

            if Thread32First(snapshot, &mut entry) == 0 {
                CloseHandle(snapshot);
                return Err("Thread32First failed".to_string());
            }

            loop {
                if entry.th32OwnerProcessID == pid {
                    let tid = entry.th32ThreadID;
                    CloseHandle(snapshot);
                    return Ok(tid);
                }

                if Thread32Next(snapshot, &mut entry) == 0 {
                    break;
                }
            }

            CloseHandle(snapshot);
            Err("No thread found for process".to_string())
        }
    }

    fn spawn_fallback_no_job(
        command: std::process::Command,
        why: &str,
        stderr: std::process::Stdio,
    ) -> Result<async_process::Child, std::io::Error> {
        eprintln!("harness: {why}; spawning without Job Object (grandchildren may linger)");
        let mut async_command = async_process::Command::from(command);
        async_command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(stderr);
        async_command.spawn()
    }

    pub(super) fn terminate_job(pid: u32) {
        let job = {
            let mut slot = match JOBS.lock() {
                Ok(slot) => slot,
                Err(_) => return,
            };
            let Some(map) = slot.as_mut() else {
                return;
            };
            map.remove(&pid)
        };

        let Some(SafeHandle(job)) = job else {
            return;
        };

        unsafe {
            let _ = TerminateJobObject(job, 1);
            CloseHandle(job);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_terminate_job_missing_pid_is_noop() {
            terminate_job(0xFFFF_FFFE);
        }

        // Job assignment failure paths must resume the suspended child
        // before returning Ok. A child left suspended hangs later tests
        // waiting for stdio.
    }
}
