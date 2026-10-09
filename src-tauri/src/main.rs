//! fidget's overlay shell.
//!
//! One transparent, always-on-top window per display renders the Character.
//! Click-through on macOS is per-window rather than per-pixel, so a screen-sized
//! transparent window would swallow every click. The shell therefore tracks the
//! cursor and toggles ignore-mouse-events by hit-testing the sprite's alpha,
//! which is what makes the overlay feel like a sprite on the desktop instead of
//! a sheet of glass over it.
//!
//! It also owns the frame loop, which is the only thing that can: the Engine is
//! pure and cannot read a clock, and `WindowSource` reports geometry and nothing
//! else. The loop reads the wall clock and the cursor, asks
//! `SnapshotAssembler` for a `WorldSnapshot`, ticks the Engine, and hands the
//! resulting `Frame` to the webview and to the hit-test.
//!
//! Waking the Director is the loop's too, and for the same reason: a timer
//! is a clock. Static may wake often. A session wake is reactive or backed
//! off (ADR-0008). What it proposes is `director`'s; when it is asked is here.

// A release exe started from Explorer would otherwise get a console window for
// its whole life. Piped std handles still reach `--mcp-stdio` and `--probe-*`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// ponytail: module-wide, though only part of each module is dead on Windows.
// The ceiling is that dead code added inside them goes unwarned there; narrow
// it to `mod form` and the view types when a Windows-only item first lands.
#[cfg_attr(not(unix), allow(dead_code))]
mod acp_wire;
mod action_log;
mod chat_surface;
mod completer;
mod consent;
mod cursor_mcp;
mod debug;
mod dev_flags;
mod frame_loop;
mod harness;
#[cfg(unix)]
mod login_path;
mod mcp_http;
mod mcp_resources;
mod menu;
mod model;
mod names_hint;
mod package;
mod pi_mcp;
mod platform;
#[cfg_attr(not(unix), allow(dead_code))] // see the note on `consent`
mod secrets;
mod session_log;
#[cfg_attr(not(unix), allow(dead_code))] // see the note on `consent`
mod settings;
mod tray;

use chat_surface::{
    Settled, CHAT_ELICITATION_EVENT, CHAT_EVENT, CHAT_OPENING_EVENT, CHAT_PERMISSION_EVENT,
    CHAT_PERMISSION_SETTLED_EVENT, CHAT_PLAN_EVENT, CHAT_RESTORED_EVENT, CHAT_THOUGHT_EVENT,
};
use frame_loop::run_frame_loop;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use fidget_core::character::{Character, Primitive};
use fidget_core::director::{
    app_instructions, happened_cell, Happened, ModelDirector, Pace, Recency, Seeded, StaticDirector,
};
use fidget_core::engine::{Cue, Point, State, Verb};
use fidget_core::input::Pointer;
use fidget_core::memory;
use fidget_core::overlay::{display_index_for, SpriteRect};
use fidget_core::roster::{self, InstanceId, InstanceSpec, Roster};
use fidget_core::snapshot::starting_position;
use fidget_core::speech::SpeechBubble;
use fidget_core::visibility::HideRules;
use fidget_core::window_source::{Rect, WindowSource};
use harness::Owner;
use secrets::{KeyringStore, SecretStore};
use serde::Serialize;
use settings::{ChatAppearance, InstanceRow, Settings, SettingsOp, SettingsSession};
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Where the shipped Character Packages sit inside the app's resources. Kept in
/// step with `bundle.resources` in `tauri.conf.json`.
const BUNDLED_CHARACTERS: &str = "characters";

/// One turn of the frame loop: 62.5Hz while moving. A poll, not an event stream: a
/// click-through window receives no mouse events, and the Engine advances on
/// elapsed time.
const ENGINE_TICK: Duration = Duration::from_millis(16);

/// How often the Free tier is read. Far less often than the frame loop: the
/// answers change at human speed, and each read is two AppKit/CoreGraphics
/// calls the sprite's physics have no use for.
const SENSE_INTERVAL: Duration = Duration::from_secs(1);

/// The overlay covering the display at `index`. The index is both the name
/// and how the frame loop finds it; `capabilities/overlay.json` grants every
/// `overlay-*` the same permissions.
fn overlay_label(index: usize) -> String {
    format!("overlay-{index}")
}

/// The Chat surface belonging to `id`. Outside `overlay-` on purpose:
/// `place_overlays` hides every `overlay-{n}` past the display count, so a
/// Chat surface sharing that prefix would vanish when a display goes away.
fn chat_label(id: &str) -> String {
    format!("chat-{id}")
}

/// Whether the window labelled `label` is a Chat that draws a row `owner`
/// owns. A row no Instance owns, as a sign-in link before any turn, goes to
/// every Chat so whichever one is open can answer it (#1422).
fn draws_in(owner: &Owner, label: &str) -> bool {
    match owner {
        Owner::Instance(instance) => label == chat_label(instance),
        Owner::EveryChat => label.starts_with("chat-"),
    }
}

/// Whether `id` has a Chat window the user has not minimized.
fn chat_is_up(app: &tauri::AppHandle, id: &str) -> bool {
    let label = chat_label(id);
    app.get_webview_window(&label).is_some()
        && !app
            .try_state::<MinimizedChats>()
            .is_some_and(|chats| chats.hides(&label))
}

/// Deliver `chat-opening` to Chat and to each open overlay, in index order.
/// `emit_to` a Chat label does not reach the overlay, and Chat may not be
/// open when the harness settles. The first missing overlay ends the scan.
fn fan_out_chat_opening(
    chat: &str,
    mut present: impl FnMut(&str) -> bool,
    mut emit: impl FnMut(&str, &'static str),
) {
    emit(chat, CHAT_OPENING_EVENT);
    let mut index = 0;
    loop {
        let overlay = overlay_label(index);
        if !present(&overlay) {
            break;
        }
        emit(&overlay, CHAT_OPENING_EVENT);
        index += 1;
    }
}

#[cfg(test)]
mod chat_opening_fanout_tests {
    use super::fan_out_chat_opening;

    fn deliver(open: &[&str]) -> Vec<(String, &'static str)> {
        let mut got = Vec::new();
        fan_out_chat_opening(
            "chat-bmo",
            |label| open.contains(&label),
            |label, event| got.push((label.to_string(), event)),
        );
        got
    }

    #[test]
    fn every_open_overlay_receives_chat_opening() {
        assert_eq!(
            deliver(&["overlay-0", "overlay-1"]),
            vec![
                ("chat-bmo".to_string(), "chat-opening"),
                ("overlay-0".to_string(), "chat-opening"),
                ("overlay-1".to_string(), "chat-opening"),
            ]
        );
    }

    #[test]
    fn a_missing_overlay_stops_the_scan() {
        assert_eq!(
            deliver(&["overlay-1"]),
            vec![("chat-bmo".to_string(), "chat-opening")]
        );
    }
}

/// The event carrying each `Frame` to the webview.
const FRAME_EVENT: &str = "frame";

/// Unsettled forwarded asks, held so `chat_ready` can replay them to a
/// surface that opens later. One lock over both fields and the emits that
/// read them: a settlement between replay and emit would draw a live row that is never retired.
/// Each beside the owner whose Chat draws it, as `draws_in` reads it.
#[derive(Default)]
struct Pending {
    asks: Vec<(Owner, harness::PermissionAsk)>,
    forms: Vec<(Owner, harness::ElicitationForm)>,
    /// Owners whose Chat has already been asked for. Opening posts to the
    /// main thread, so a second row before that lands would queue a second
    /// focus grab. Keyed by owner because no other Chat draws that owner's
    /// rows. An owner leaves when its last ask or form settles.
    opened: HashSet<Owner>,
}

impl Pending {
    /// Keep `ask` for the next Chat to open, and say whether to open one for
    /// it now. Do Not Disturb opens nothing, even for a question with a
    /// deadline: the turn times out, never an answer of ours (ADR-0018).
    fn hold_ask(
        &mut self,
        owner: &Owner,
        ask: &harness::PermissionAsk,
        shut: bool,
        dnd: bool,
    ) -> bool {
        self.asks.push((owner.clone(), ask.clone()));
        shut && !dnd && self.opened.insert(owner.clone())
    }

    /// Retire `request` from the replay.
    fn settle(&mut self, request: &str) {
        self.asks.retain(|(_, ask)| ask.request != request);
        self.forms.retain(|(_, form)| form.request != request);
        let (asks, forms) = (&self.asks, &self.forms);
        self.opened.retain(|owner| {
            asks.iter().any(|(held, _)| held == owner)
                || forms.iter().any(|(held, _)| held == owner)
        });
    }

    /// Keep `form` for the next Chat to open, and say whether to open one for
    /// it now. A link that waits opens nothing, as Do Not Disturb does.
    fn hold_form(
        &mut self,
        owner: &Owner,
        form: &harness::ElicitationForm,
        shut: bool,
        dnd: bool,
    ) -> bool {
        self.forms.push((owner.clone(), form.clone()));
        shut && !form.waits && !dnd && self.opened.insert(owner.clone())
    }

    /// The open asks and forms the Chat labelled `label` draws when it opens.
    fn drawn_in<'a>(
        &'a self,
        label: &'a str,
    ) -> (
        impl Iterator<Item = &'a harness::PermissionAsk>,
        impl Iterator<Item = &'a harness::ElicitationForm>,
    ) {
        (
            self.asks
                .iter()
                .filter(move |(owner, _)| draws_in(owner, label))
                .map(|(_, ask)| ask),
            self.forms
                .iter()
                .filter(move |(owner, _)| draws_in(owner, label))
                .map(|(_, form)| form),
        )
    }
}

struct PendingAsks(Mutex<Pending>);

/// Chats the user minimized, by label. Each Chat's window events and, on macOS,
/// its miniaturize notifications keep it, so the frame loop never asks a window.
#[derive(Default)]
struct MinimizedChats(Mutex<HashSet<String>>);

impl MinimizedChats {
    /// `Some(true)` floats Chat above the overlay, `Some(false)` drops it to a
    /// normal level. No event says "minimized", so focus and resize re-read it.
    /// See `set` for macOS.
    fn note(
        &self,
        label: &str,
        event: &tauri::WindowEvent,
        minimized: impl FnOnce() -> bool,
    ) -> Option<bool> {
        let (floats, minimized) = match event {
            tauri::WindowEvent::Focused(focused) => (Some(*focused), minimized()),
            tauri::WindowEvent::Resized(_) => (None, minimized()),
            tauri::WindowEvent::Destroyed => (None, false),
            _ => return None,
        };
        self.set(label, minimized);
        floats
    }

    /// macOS sends no focus or resize for an unfocused minimize, so the
    /// `observe_minimize` callback calls this.
    fn set(&self, label: &str, minimized: bool) {
        if let Ok(mut hidden) = self.0.lock() {
            if minimized && hidden.insert(label.to_string()) {
                eprintln!("chat: {label} is minimized");
            } else if !minimized && hidden.remove(label) {
                eprintln!("chat: {label} is not minimized");
            }
        }
    }

    fn hides(&self, label: &str) -> bool {
        self.0.lock().is_ok_and(|hidden| hidden.contains(label))
    }
}

/// Director config and the last Character Prompt, for the frame loop.
struct DirectorRun {
    config: model::DirectorConfig,
    settings: model::DirectorSettings,
    inspect: Arc<Mutex<model::DirectorInspect>>,
}

/// Where the sprite was last drawn, and what it was drawn as. Kept for one
/// tick so the hit-test asks about the sprite the user is looking at rather
/// than the one this tick is about to produce.
struct Drawn {
    rect: SpriteRect,
    animation: &'static str,
    animation_ms: u32,
    /// The variant draw the art was picked with, so the hit-test measures the
    /// strip the user saw rather than whatever the next draw lands on.
    variant_draw: u64,
    /// Which way the sprite was pointed, so the hit-test resolves the strip
    /// the art was drawn from and mirrors it the same way — this tick's
    /// facing may already differ.
    facing: f64,
}

/// What the last `engine:` line said about an Instance. Trace prints on
/// change; everything the line carries is in here, or a field that moved
/// alone would stop appearing after the first tick that moved only it.
#[derive(PartialEq)]
struct Traced {
    behavior: Option<String>,
    primitive: Option<Primitive>,
    animation: &'static str,
    state: State,
}

/// Shell state one Instance keeps between ticks. Sharing any of it would be
/// visible: one Director would lockstep two characters; one `Pointer` would
/// count a double-click on one toward a Summon on the other.
struct InstanceState {
    id: InstanceId,
    /// The Character this Instance runs, shared with every other Instance
    /// running the same one.
    character: Arc<Character>,
    director: StaticDirector,
    model: Option<Arc<ModelDirector<completer::AnyCompleter>>>,
    recent: Vec<String>,
    /// The last few seconds, for the wake that answers a verb.
    recency: Recency,
    pace: Pace,
    since_wake: Duration,
    since_proactive: Duration,
    previous_idle: Duration,
    /// Whether the call on the wire answers a typed line, so the surface can be
    /// told when newest-wins throws that answer away (ADR-0016). `Slots` knows
    /// only that a call is out, and the wake clears `happened` as it sends.
    chat_turn: bool,
    /// The `Happened` that drove the last session wake, as `happened_cell`
    /// names it. `None` until one has: nothing has been asked here yet.
    happened_last: Option<&'static str>,
    since_state: Duration,
    last_state: Option<State>,
    last_position: Point,
    addressed: bool,
    happened: Happened,
    pointer: Pointer,
    /// The last line spoken and which overlay showed it, so a crossing
    /// carries it (#178). See `carry_line`.
    spoken: Option<Spoken>,
    speech: SpeechBubble,
    drawn_last: Option<Drawn>,
    /// The open quick-message draft, refreshed from the overlay's report each tick.
    qm: Option<fidget_core::quick_message::QmDraft>,
    /// This tick's verbs, decided before any Instance is ticked. Held on the
    /// Instance because `press_target` has to see every hit-test before any
    /// pointer is told whether the press was its own.
    verbs: Vec<Verb>,
    /// The menu this Instance has open, and `None` when it has none. While it is
    /// `Some`, the frame loop re-injects `Verb::Menu` every tick, which is what
    /// holds the Instance still under the popup.
    menu_hold: Option<MenuHold>,
    /// The subject of the last `engine:` line. `None` while the switch is off,
    /// so turning it on always opens with a line rather than waiting for the
    /// sprite to do something new.
    traced_last: Option<Traced>,
    /// What the status bar was last told, so the push happens on change rather
    /// than every tick. `None` re-sends: a surface that has just said it is
    /// listening has drawn nothing yet.
    status_last: Option<ChatStatus>,
    /// The countdown last pushed with it. Kept out of `ChatStatus` because it
    /// falls a millisecond per millisecond and the window subtracts for itself;
    /// only a deadline that *moved* is worth a push.
    status_wake_ms: Option<u64>,
}

/// One open menu, from the frame loop's side.
struct MenuHold {
    /// What the rows of the menu on screen mean. Kept rather than looked up
    /// again when the click arrives: a package installed while it is open
    /// must not change what its rows do.
    actions: HashMap<String, menu::MenuAction>,
    elapsed: Duration,
}

/// What the main thread tells the frame loop about the menu it was asked to
/// pop. Two messages rather than one because a menu can close without
/// choosing, and nothing arrives on the event channel when the user presses Escape.
enum MenuSignal {
    /// A row was chosen, by the id the description gave it.
    Chose(String),
    /// The popup is gone, whether or not anything was chosen.
    Closed,
}

/// Both ends of the menu's channel. They travel as a pair because the app's
/// menu event hook is registered before the frame loop starts and needs a
/// sender of its own.
struct MenuChannel {
    sender: mpsc::Sender<MenuSignal>,
    receiver: mpsc::Receiver<MenuSignal>,
    quit_generation: Arc<AtomicU64>,
}

/// Settings plus the live roster the settings window reads.
struct SettingsState {
    settings: Arc<Mutex<Settings>>,
    path: PathBuf,
    memory_path: PathBuf,
    installed: Vec<String>,
    /// Each installed Character's Personality Prompt, by Character name. Read
    /// once at launch, because a package changes only between runs, and shared
    /// so the Prompt tab asking for it costs no copy of every prompt installed.
    personalities: Arc<BTreeMap<String, String>>,
    /// Declared Behavior names, same key as `personalities`. The Prompt tab
    /// draws the app-level instructions from these, so the roster it shows is
    /// the one the opening turn names.
    behavior_names: Arc<BTreeMap<String, Vec<String>>>,
    instances: Arc<Mutex<Vec<InstanceRow>>>,
    inspect: Arc<Mutex<model::DirectorInspect>>,
    ops: mpsc::Sender<SettingsOp>,
    rules: Arc<Mutex<HideRules>>,
    secrets: Arc<dyn SecretStore>,
    /// Taken by the next snapshot. A window that is still loading has no
    /// listeners, so an event aimed at it would be gone.
    reveal: Mutex<Option<settings::form::Reveal>>,
    /// Generation of the Settings window. `settings_loaded` matches it once
    /// navigation finishes. MoveFocus while they differ hangs WebView2.
    settings_built: AtomicU64,
    settings_loaded: AtomicU64,
    /// When this document started loading. Cleared once navigation finishes.
    /// An open past the deadline drops the window.
    loading_since: Mutex<Option<Instant>>,
    /// Set while a stalled window is being destroyed. The replacement is
    /// built once `Destroyed` frees the label.
    rebuild_after_destroy: AtomicBool,
}

/// How long a hold survives without hearing anything. A backstop: `Closed`
/// ends the hold; this only matters if it never comes, or an Instance stays
/// frozen under a menu that is no longer there for as long as the app runs.
const MENU_HOLD_TIMEOUT: Duration = Duration::from_secs(120);

/// Where one Instance is to be drawn in one overlay, in logical points from
/// that overlay's top-left, and which Animation frame to draw.
#[derive(Clone, Serialize)]
struct SpritePlacement<'a> {
    /// The Instance this sprite belongs to, so the renderer keeps one element
    /// per Instance across ticks rather than redrawing a fresh set. An id that
    /// stops arriving is an Instance that was dismissed, and its element goes.
    id: &'a str,
    /// Which Character's art to draw from. Instances may run different
    /// Characters, and two running the same one name the same art.
    character: &'a str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    /// The Animation whose art to draw — the one `Character::draw` resolved
    /// (a variant, or an optional Animation's fallback), which is not always
    /// the name the Engine asked with.
    animation: &'a str,
    frame_index: usize,
    /// -1 to mirror, 1 as authored. `Character::draw`'s answer, not the
    /// heading (#345): a left-strip Character already faces that way, and
    /// mirroring would turn it back round. Hit-test uses the same answer.
    mirror: i8,
    /// A line to speak on this tick only. Dialogue is an event, not a state.
    /// #119: the webview latches it and owns display duration. `None` on every
    /// overlay but the bubble owner's.
    dialogue: Option<String>,
    /// Whether to show the thinking ellipsis. Derived from what the Instance
    /// has on the wire. #119: grace and min-hold are in the webview so the
    /// Engine stays tick-pure. False on every overlay but the bubble owner's.
    thinking: bool,
    /// On this tick only: a touch was dropped because a question waits on the
    /// user (ADR-0016), so the bubble points at Chat. False off the bubble owner.
    asking: bool,
    /// Whether this Instance has a Chat window that is not minimized. Sent to
    /// every overlay, because the pill that must stay down can open on any of them.
    chatting: bool,
    /// Whether this overlay draws this Instance's bubble (#178, `bubble_owner`).
    /// Still sent to the overlays that lost, which drop the bubble they were
    /// showing on the tick the answer changes.
    bubble: bool,
    /// Cue to play this tick only, by the name the webview keys visual and
    /// sound by. `None` on every overlay but the bubble owner's: every overlay
    /// draws the art, so a cue from all of them is one sound per display. #277
    cue: Option<&'static str>,
    /// The open quick-message draft. `None` off the bubble owner, so exactly one
    /// overlay draws the pill and the text follows the Instance across a seam.
    qm: Option<&'a fidget_core::quick_message::QmDraft>,
}

impl<'a> SpritePlacement<'a> {
    /// One Instance as one overlay is told about it, in that overlay's own
    /// coordinates.
    ///
    /// Every overlay draws the art. Only the bubble owner is told the line,
    /// the indicator and the cue (#178, #277). Decided here because the Shell
    /// already knows the owner: the webview used to strip these itself, which
    /// made it reconstruct an answer it had been handed.
    fn new(instance: &'a Placed, display: Rect, index: usize) -> Self {
        let local = instance.sprite.in_overlay(display);
        let bubble = instance.owner == Some(index);
        Self {
            id: &instance.id,
            character: &instance.character,
            x: local.x,
            y: local.y,
            width: instance.width,
            height: instance.height,
            animation: &instance.animation,
            frame_index: instance.frame_index,
            mirror: instance.mirror,
            dialogue: instance.dialogue.as_ref().filter(|_| bubble).cloned(),
            thinking: bubble && instance.thinking,
            asking: bubble && instance.asking,
            chatting: instance.chatting,
            bubble,
            cue: instance.cue.filter(|_| bubble).map(Cue::name),
            qm: instance.qm.as_ref().filter(|_| bubble),
        }
    }
}

/// One tick's instruction to the renderer. Pushed so the webview holds no
/// state. One message for every sprite, because the list is also which
/// Instances still exist; sent separately, a dismiss would look like a late frame.
#[derive(Clone, Serialize)]
struct Placement<'a> {
    sprites: Vec<SpritePlacement<'a>>,
    /// Hide-rules visibility, on every frame: the first tick fires before the
    /// webview is listening, and a repeated frame is not sent (#741), so
    /// `FRAME_RESEND` is what keeps a hidden-at-launch Character from staying on top.
    visible: bool,
    fade_ms: u32,
    /// Whether a cue this frame may be heard as well as seen. Decided in
    /// Settings, where Do Not Disturb takes part (#277); the webview only
    /// obeys.
    sound: bool,
}

struct Spoken {
    line: String,
    at: Instant,
    owner: Option<usize>,
}

/// The longest the renderer keeps a line up — `bubbleDuration`'s clamp in
/// `src/bubble.js`. A line older than this cannot still be showing anywhere,
/// so it is never carried.
const CARRY_WINDOW: Duration = Duration::from_secs(8);

/// Dialogue this tick: the Engine's new line, or the last one re-pulsed to a
/// new owner. Only the overlay that owns the bubble latches the one-tick
/// pulse, so a seam crossing mid-line would otherwise lose it (#178).
fn carry_line(
    spoken: &mut Option<Spoken>,
    said: Option<&str>,
    owner: Option<usize>,
    now: Instant,
) -> Option<String> {
    if let Some(line) = said {
        *spoken = Some(Spoken {
            line: line.to_string(),
            at: now,
            owner,
        });
        return Some(line.to_string());
    }
    let carried = spoken.as_mut()?;
    if carried.owner == owner || now.duration_since(carried.at) > CARRY_WINDOW {
        return None;
    }
    carried.owner = owner;
    Some(carried.line.clone())
}

/// What one Instance's tick decided to draw, in the space every display
/// shares. Worked out once, then turned into each overlay's rectangle.
/// Art names are owned so this outlives the Character borrow.
struct Placed {
    id: InstanceId,
    character: String,
    sprite: SpriteRect,
    width: i32,
    height: i32,
    animation: String,
    frame_index: usize,
    mirror: i8,
    dialogue: Option<String>,
    thinking: bool,
    asking: bool,
    chatting: bool,
    cue: Option<Cue>,
    /// The overlay that draws the bubble, decided once from the feet
    /// (#178, `bubble_owner`); `None` while the feet are on no display.
    owner: Option<usize>,
    /// The open quick-message draft, carried to the bubble owner like `dialogue`.
    qm: Option<fidget_core::quick_message::QmDraft>,
    #[allow(dead_code)]
    mask: fidget_core::overlay::AlphaMask,
}

/// Every Animation's frames as `data:` URLs, in play order. Paths would need
/// a filesystem scope for packages outside the front end; the webview indexes
/// this list the same way `Character::draw` walks `Animation::frames`.
fn art_urls(character: &Character) -> BTreeMap<String, Vec<String>> {
    // A frame two Animations share is encoded once and named twice.
    let urls: BTreeMap<&String, String> = character
        .art
        .iter()
        .map(|(frame, art)| {
            let url = format!("data:image/png;base64,{}", STANDARD.encode(&art.png));
            (frame, url)
        })
        .collect();

    character
        .animations
        .iter()
        .map(|(name, animation)| {
            let frames = animation.frames.iter().map(|frame| urls[frame].clone());
            (name.clone(), frames.collect())
        })
        .collect()
}

/// What the webview needs of one Character: the art as `data:` URLs, and
/// whether to smooth it when scaling (the Character Manifest's `render_mode`).
#[derive(Clone, serde::Serialize)]
struct CharacterArt {
    art: BTreeMap<String, Vec<String>>,
    smooth: bool,
}

/// Every Character on screen, keyed by Character rather than Instance so two
/// Instances of one Character share one encoded sheet. A struct rather than
/// a bare map so managed state, keyed by type, cannot collide.
#[derive(Clone, serde::Serialize)]
struct ArtUrls {
    characters: BTreeMap<String, CharacterArt>,
}

/// The art of every Character on screen, fetched once when the webview
/// loads. A command rather than an event: setup would race the listener, and
/// the art does not change while the app runs.
#[tauri::command]
fn character(art: tauri::State<'_, ArtUrls>) -> ArtUrls {
    art.inner().clone()
}

/// The Settings window's handle on the running app. `SettingsSession::apply`
/// is the one path that persists a setting and acts on it; any other writer
/// takes this rather than the file (#654).
fn settings_session(app: &tauri::AppHandle, state: &SettingsState) -> SettingsSession {
    SettingsSession {
        settings: Arc::clone(&state.settings),
        path: state.path.clone(),
        memory_path: state.memory_path.clone(),
        rules: Arc::clone(&state.rules),
        inspect: Arc::clone(&state.inspect),
        instances: Arc::clone(&state.instances),
        installed: state.installed.clone(),
        ops: state.ops.clone(),
        app: app.clone(),
        on_rebind: bind_hide_hotkey,
        secrets: Arc::clone(&state.secrets),
        key_cache: Mutex::new(None),
    }
}

/// The Settings form and the values in force, together: a row is only
/// renderable with both. Committed fixtures pin the shape; the values are
/// this machine's and pin nothing.
#[derive(serde::Serialize)]
struct SettingsSnapshot {
    form: settings::form::FormDescription,
    /// Keyed by form row id, which is what `src/settings.js` indexes (#875).
    view: std::collections::BTreeMap<String, settings::RowValue>,
    /// One shot, cleared by this read. Absent when nothing asked to point.
    reveal: Option<settings::form::RevealTarget>,
}

#[tauri::command]
fn settings_snapshot(app: tauri::AppHandle) -> Result<SettingsSnapshot, String> {
    let state = app
        .try_state::<SettingsState>()
        .ok_or("settings: asked for before the shell was ready")?;
    let session = settings_session(&app, &state);
    let view = session.view();
    // The one caller that can fill the API key row's placeholder and the
    // Character popups' choices. `current()` leaves both empty because the
    // status is a store read and the package list is the view's; the view has
    // the key status from the cache that keeps become-key off Keychain. #875, #921.
    let cwd = view
        .development_texts
        .get(settings::form::HARNESS_CWD_ID)
        .map(String::as_str)
        .unwrap_or("");
    let live = settings::form::Live {
        pi_mcp_dir: harness::project_dir_label(cwd),
        api_key_placeholder: view.api_key_placeholder(),
        installed: view.installed.clone(),
        ..settings::form::Live::current()
    };
    let reveal = state
        .reveal
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .map(settings::form::Reveal::target);
    Ok(SettingsSnapshot {
        form: settings::form::describe_with(&live),
        view: view.row_values(),
        reveal,
    })
}

/// A gesture from the webview Settings page.
#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum SettingsEventPayload {
    SetBool {
        set_bool: String,
        value: bool,
    },
    SetText {
        set_text: String,
        value: String,
    },
    Press {
        press: String,
        #[serde(default)]
        draft: Option<settings::DraftRows>,
        /// New reads the name and Character beside it from here (#875).
        #[serde(default)]
        fields: std::collections::HashMap<String, String>,
    },
    Pick {
        pick: String,
        value: String,
        fills: Option<PickFills>,
    },
    Dismiss {
        dismiss: String,
        value: String,
    },
}

#[derive(serde::Deserialize, Debug)]
struct PickFills {
    row: String,
}

#[cfg(test)]
mod settings_event_tests {
    use super::*;

    #[test]
    fn set_bool_deserializes_from_js() {
        let json = r#"{"set_bool": "director", "value": true}"#;
        let payload: SettingsEventPayload =
            serde_json::from_str(json).expect("set_bool payload should deserialize");
        match payload {
            SettingsEventPayload::SetBool { set_bool, value } => {
                assert_eq!(set_bool, "director");
                assert!(value);
            }
            _ => panic!("expected SetBool variant"),
        }
    }

    #[test]
    fn set_text_deserializes_from_js() {
        let json = r#"{"set_text": "director_base_url", "value": "https://api.x.ai"}"#;
        let payload: SettingsEventPayload =
            serde_json::from_str(json).expect("set_text payload should deserialize");
        match payload {
            SettingsEventPayload::SetText { set_text, value } => {
                assert_eq!(set_text, "director_base_url");
                assert_eq!(value, "https://api.x.ai");
            }
            _ => panic!("expected SetText variant"),
        }
    }

    #[test]
    fn press_deserializes_from_js() {
        let json = r#"{"press": "apply"}"#;
        let payload: SettingsEventPayload =
            serde_json::from_str(json).expect("press payload should deserialize");
        match payload {
            SettingsEventPayload::Press { press, draft, .. } => {
                assert_eq!(press, "apply");
                assert!(draft.is_none());
            }
            _ => panic!("expected Press variant"),
        }
    }

    #[test]
    fn press_with_draft_deserializes_from_js() {
        let json = r#"{"press":"director_apply","draft":{"harness":"Harness · opencode"}}"#;
        let payload: SettingsEventPayload =
            serde_json::from_str(json).expect("press with draft should deserialize");
        match payload {
            SettingsEventPayload::Press { press, draft, .. } => {
                assert_eq!(press, "director_apply");
                assert_eq!(
                    draft.expect("draft").values.get("harness"),
                    Some(&settings::RowValue::Text("Harness · opencode".into()))
                );
            }
            _ => panic!("expected Press variant"),
        }
    }

    #[test]
    fn press_with_fields_deserializes_from_js() {
        let json = r#"{"press":"spawn","fields":{"new_name":"Nim","new_character":"ghost"}}"#;
        let payload: SettingsEventPayload =
            serde_json::from_str(json).expect("press with fields should deserialize");
        match payload {
            SettingsEventPayload::Press { press, fields, .. } => {
                assert_eq!(press, "spawn");
                assert_eq!(fields.get("new_name").map(String::as_str), Some("Nim"));
                assert_eq!(
                    fields.get("new_character").map(String::as_str),
                    Some("ghost")
                );
            }
            _ => panic!("expected Press variant"),
        }
    }

    #[test]
    fn dismiss_deserializes_from_js() {
        let json = r#"{"dismiss": "instances", "value": "bmo-1"}"#;
        let payload: SettingsEventPayload =
            serde_json::from_str(json).expect("dismiss payload should deserialize");
        match payload {
            SettingsEventPayload::Dismiss { dismiss, value } => {
                assert_eq!(dismiss, "instances");
                assert_eq!(value, "bmo-1");
            }
            _ => panic!("expected Dismiss variant"),
        }
    }

    #[test]
    fn pick_without_fills_deserializes() {
        let json = r#"{"pick": "character", "value": "bmo"}"#;
        let payload: SettingsEventPayload =
            serde_json::from_str(json).expect("pick payload should deserialize");
        match payload {
            SettingsEventPayload::Pick { pick, value, fills } => {
                assert_eq!(pick, "character");
                assert_eq!(value, "bmo");
                assert!(fills.is_none());
            }
            _ => panic!("expected Pick variant"),
        }
    }

    #[test]
    fn pick_with_fills_deserializes() {
        let json = r#"{"pick": "director_base_url_pick", "value": "OpenAI (https://api.openai.com)", "fills": {"row": "director_base_url"}}"#;
        let payload: SettingsEventPayload =
            serde_json::from_str(json).expect("pick with fills should deserialize");
        match payload {
            SettingsEventPayload::Pick {
                pick,
                value,
                fills: Some(fills),
            } => {
                assert_eq!(pick, "director_base_url_pick");
                assert_eq!(value, "OpenAI (https://api.openai.com)");
                assert_eq!(fills.row, "director_base_url");
            }
            _ => panic!("expected Pick variant with fills"),
        }
    }

    #[derive(Default)]
    struct Recorded {
        ran: std::cell::RefCell<Vec<String>>,
        fails: bool,
    }

    impl Recorded {
        fn note(&self, what: &str) -> Result<(), String> {
            self.ran.borrow_mut().push(what.to_string());
            if self.fails {
                return Err("the store said no".to_string());
            }
            Ok(())
        }

        fn ran(&self) -> Vec<String> {
            self.ran.borrow().clone()
        }
    }

    impl Operations for Recorded {
        fn open_memory(&self) -> Result<(), String> {
            self.note("open_memory")
        }

        fn wipe_memory(&self) -> Result<(), String> {
            self.note("wipe_memory")
        }

        fn spawn(&self, character: String, name: String) {
            let _ = self.note(&format!("spawn {character} as {name}"));
        }

        fn dismiss(&self, id: String) {
            let _ = self.note(&format!("dismiss {id}"));
        }
    }

    fn one_instance() -> settings::SettingsView {
        settings::SettingsView::from_parts(
            &settings::Settings::default(),
            std::path::Path::new("/tmp/memory.md"),
            None,
            Vec::new(),
            vec![settings::InstanceRow {
                id: "bmo-1".to_string(),
                name: "BMO".to_string(),
                character: "bmo".to_string(),
                prompt: String::new(),
            }],
            (false, String::new(), String::new()),
            None,
        )
    }

    fn no_fields() -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    /// A press handed to the page reaches nothing that acts on it. #875.
    #[test]
    fn opening_and_wiping_memory_run_here_and_do_not_cross_to_the_page() {
        use settings::form::RowOperation;

        let session = Recorded::default();
        assert_eq!(
            run_operation(&session, &RowOperation::OpenMemory, &no_fields()),
            Ok(SettingsEventResponse::Nothing)
        );
        assert_eq!(
            run_operation(&session, &RowOperation::WipeMemory, &no_fields()),
            Ok(SettingsEventResponse::Refresh)
        );
        assert_eq!(session.ran(), ["open_memory", "wipe_memory"]);
    }

    /// `AiDraft` carries neither the name nor the Character, so they
    /// ride the Composite's own fields. #875.
    #[test]
    fn new_spawns_under_the_name_and_character_the_page_shows() {
        use settings::form::RowOperation;

        let session = Recorded::default();
        let fields = std::collections::HashMap::from([
            (settings::form::NEW_NAME_ID.to_string(), "Nim".to_string()),
            (
                settings::form::NEW_CHARACTER_ID.to_string(),
                "ghost".to_string(),
            ),
        ]);
        assert_eq!(
            run_operation(&session, &RowOperation::Spawn, &fields),
            Ok(SettingsEventResponse::Nothing)
        );
        assert_eq!(session.ran(), ["spawn ghost as Nim"]);
    }

    #[test]
    fn dismiss_reaches_the_roster_and_a_stale_press_does_not() {
        let session = Recorded::default();
        let view = one_instance();

        assert_eq!(
            dismiss_press(&session, &view, "instances", "bmo-1"),
            SettingsEventResponse::Nothing
        );
        assert_eq!(session.ran(), ["dismiss bmo-1"]);

        // A character the roster let go while the list was on screen.
        assert_eq!(
            dismiss_press(&session, &view, "instances", "ghost-1"),
            SettingsEventResponse::Nothing
        );
        assert_eq!(session.ran(), ["dismiss bmo-1"]);
    }

    /// The clipboard is the page's, because WebKit gives `writeText` the
    /// click's own turn and this command has already spent it (#855).
    #[test]
    fn the_clipboard_operations_still_cross_to_the_page() {
        use settings::form::RowOperation;

        let session = Recorded::default();
        assert_eq!(
            run_operation(&session, &RowOperation::CopyByoSnippet, &no_fields()),
            Ok(SettingsEventResponse::Run {
                operation: "copy_byo_snippet".to_string()
            })
        );
        assert!(session.ran().is_empty());
    }

    /// A failed wipe is the user's to see. Answering Refresh would redraw the
    /// same Memory file and read as a wipe that worked.
    #[test]
    fn a_refused_wipe_answers_with_the_reason() {
        use settings::form::RowOperation;

        let session = Recorded {
            fails: true,
            ..Recorded::default()
        };
        assert_eq!(
            run_operation(&session, &RowOperation::WipeMemory, &no_fields()),
            Err("the store said no".to_string())
        );
    }

    #[test]
    fn response_nothing_serializes() {
        let response = SettingsEventResponse::Nothing;
        let json = serde_json::to_string(&response).expect("should serialize");
        assert_eq!(json, r#"{"action":"nothing"}"#);
    }

    #[test]
    fn response_refresh_serializes() {
        let response = SettingsEventResponse::Refresh;
        let json = serde_json::to_string(&response).expect("should serialize");
        assert_eq!(json, r#"{"action":"refresh"}"#);
    }

    #[test]
    fn response_fill_serializes() {
        let response = SettingsEventResponse::Fill {
            id: "director_base_url".to_string(),
            value: "https://api.openai.com".to_string(),
        };
        let json = serde_json::to_string(&response).expect("should serialize");
        assert!(json.contains(r#""action":"fill""#));
        assert!(json.contains(r#""id":"director_base_url""#));
        assert!(json.contains(r#""value":"https://api.openai.com""#));
    }

    #[test]
    fn byo_preview_serializes_for_the_page() {
        let response = SettingsEventResponse::PreviewByo {
            snippet: "hermes mcp add fidget".into(),
            steps: "Run the command.".into(),
            token: "secret-token".into(),
        };
        let json = serde_json::to_value(response).expect("should serialize");
        assert_eq!(json["action"], "preview_byo");
        assert_eq!(json["snippet"], "hermes mcp add fidget");
        assert_eq!(json["steps"], "Run the command.");
        assert_eq!(json["token"], "secret-token");
    }

    #[test]
    fn response_run_uses_stable_wire_format() {
        use settings::form::RowOperation;
        let response = SettingsEventResponse::Run {
            operation: RowOperation::Spawn.as_str().to_string(),
        };
        let json = serde_json::to_string(&response).expect("should serialize");
        assert_eq!(json, r#"{"action":"run","operation":"spawn"}"#);

        let response = SettingsEventResponse::Run {
            operation: RowOperation::NewSession.as_str().to_string(),
        };
        let json = serde_json::to_string(&response).expect("should serialize");
        assert_eq!(json, r#"{"action":"run","operation":"new_session"}"#);
    }

    /// What Apply writes for a payload the page sent, against the view its
    /// fixtures were drawn from. That view has a key stored.
    fn applied(payload: serde_json::Value) -> Vec<settings::SettingsPatch> {
        let Ok(SettingsEventPayload::Press { press, draft, .. }) = serde_json::from_value(payload)
        else {
            panic!("the payload is a press");
        };
        let mut applied = Vec::new();
        model::tests::with_env(None, None, None, || {
            let view = settings::form::tests::fixture_view(false);
            let description = settings::form::describe();
            let staged = settings::AiDraft {
                rows: draft.unwrap_or_default(),
                description: &description,
            };
            let outcome = settings::controller::handle(
                &settings::controller::Event::Press { id: press },
                &staged,
                &view,
            );
            let response = respond(
                outcome,
                |patch| {
                    applied.push(patch);
                    Ok(())
                },
                |_| panic!("Apply and Cancel run no operation"),
            );
            assert_eq!(response, Ok(SettingsEventResponse::Reset));
        });
        applied
    }

    /// tests/settings-batched-draft.test.js asserts the page's Apply button
    /// sends exactly this after Clear key, so it is the real wire.
    fn apply_after_clear_key() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/settings-apply-clear-key.json"
        ))
        .expect("the fixture has to be JSON")
    }

    #[test]
    fn apply_after_clear_key_deletes_the_stored_key() {
        let mut delete = settings::SettingsPatch::default();
        delete.completer.director_api_key = Some(String::new());
        assert_eq!(applied(apply_after_clear_key()), vec![delete]);
    }

    #[test]
    fn a_key_typed_after_clear_key_replaces_the_stored_one() {
        let mut payload = apply_after_clear_key();
        payload["draft"]["director_api_key"] = "sk-new".into();
        let mut replace = settings::SettingsPatch::default();
        replace.completer.director_api_key = Some("sk-new".into());
        assert_eq!(applied(payload), vec![replace]);
    }

    /// tests/settings-batched-draft.test.js asserts the page sends exactly
    /// this when Reasoning effort's picker lands on high. Sent as a bare row
    /// string, it matched no payload variant and the page showed "Could not
    /// save changes" for every pick (#1426).
    #[test]
    fn a_reasoning_effort_pick_saves_the_level_and_fills_the_field() {
        let payload = serde_json::from_str(include_str!(
            "../../tests/fixtures/settings-pick-reasoning-effort.json"
        ))
        .expect("the page's pick has to deserialize");
        let SettingsEventPayload::Pick {
            pick,
            value,
            fills: Some(fills),
        } = payload
        else {
            panic!("the payload is a shortcut pick");
        };
        let mut applied = Vec::new();
        let mut response = None;
        model::tests::with_env(None, None, None, || {
            let view = settings::form::tests::fixture_view(false);
            let description = settings::form::describe();
            let outcome = settings::controller::handle(
                &shortcut_event(pick, value, &fills, &view),
                &settings::AiDraft::live(&description),
                &view,
            );
            response = Some(respond(
                outcome,
                |patch| {
                    applied.push(patch);
                    Ok(())
                },
                |_| panic!("a pick runs no operation"),
            ));
        });
        let mut high = settings::SettingsPatch::default();
        high.completer.director_reasoning_effort = Some("high".into());
        assert_eq!(applied, vec![high]);
        assert_eq!(
            response,
            Some(Ok(SettingsEventResponse::Fill {
                id: "director_reasoning_effort".into(),
                value: "high".into(),
            }))
        );
    }

    #[test]
    fn cancel_after_clear_key_writes_nothing() {
        let cancel = serde_json::json!({ "press": "director_cancel", "fields": {} });
        assert_eq!(applied(cancel), Vec::new());
    }

    /// The page redraws every tab switch from its cached snapshot, so a
    /// write it is not told about comes back undone: the consent checkbox in
    /// #995. `Apply` is the only outcome that writes and does not otherwise
    /// name a redraw, so it is the arm this test exists for.
    #[test]
    fn a_plain_apply_writes_and_answers_refresh() {
        use settings::controller::Outcome;
        let mut applied = Vec::new();
        let mut patch = settings::SettingsPatch::default();
        patch.set_bool(settings::BoolField::DoNotDisturb, true);
        let response = respond(
            Outcome::Apply(patch.clone()),
            |p| {
                applied.push(p);
                Ok(())
            },
            |_| panic!("Apply runs no operation"),
        );
        assert_eq!(response, Ok(SettingsEventResponse::Refresh));
        assert_eq!(applied, vec![patch]);
    }

    #[test]
    fn every_other_outcome_keeps_its_answer() {
        use settings::controller::Outcome;
        use settings::form::RowOperation;
        let mut applied = 0;
        let mut respond_counting = |outcome| {
            respond(
                outcome,
                |_| {
                    applied += 1;
                    Ok(())
                },
                |op| {
                    Ok(SettingsEventResponse::Run {
                        operation: op.as_str().to_string(),
                    })
                },
            )
        };
        let patch = settings::SettingsPatch::default();
        assert_eq!(
            respond_counting(Outcome::Nothing),
            Ok(SettingsEventResponse::Nothing)
        );
        assert_eq!(
            respond_counting(Outcome::Reset),
            Ok(SettingsEventResponse::Reset)
        );
        assert_eq!(
            respond_counting(Outcome::ClearKey),
            Ok(SettingsEventResponse::ClearKey)
        );
        assert_eq!(
            respond_counting(Outcome::Commit(Some(patch.clone()))),
            Ok(SettingsEventResponse::Reset)
        );
        assert_eq!(
            respond_counting(Outcome::Commit(None)),
            Ok(SettingsEventResponse::Reset)
        );
        assert_eq!(
            respond_counting(Outcome::Fill {
                id: "director_base_url",
                value: "https://api.anthropic.com",
                patch: None
            }),
            Ok(SettingsEventResponse::Fill {
                id: "director_base_url".to_string(),
                value: "https://api.anthropic.com".to_string(),
            })
        );
        assert_eq!(
            respond_counting(Outcome::Run(RowOperation::Spawn)),
            Ok(SettingsEventResponse::Run {
                operation: "spawn".to_string()
            })
        );
        assert_eq!(applied, 1, "only Commit(Some) wrote");
    }

    #[test]
    fn a_failed_write_is_the_answer_not_a_refresh() {
        use settings::controller::Outcome;
        let response = respond(
            Outcome::Apply(settings::SettingsPatch::default()),
            |_| Err("the store said no".to_string()),
            |_| unreachable!(),
        );
        assert_eq!(response, Err("the store said no".to_string()));
    }
}

/// What the webview must do about a gesture.
#[derive(serde::Serialize, Debug, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
enum SettingsEventResponse {
    Nothing,
    Refresh,
    Fill {
        id: String,
        value: String,
    },
    PreviewByo {
        snippet: String,
        steps: String,
        token: String,
    },
    ClearKey,
    Reset,
    Run {
        operation: String,
    },
}

#[tauri::command]
async fn settings_event(
    app: tauri::AppHandle,
    payload: SettingsEventPayload,
) -> Result<SettingsEventResponse, String> {
    // Sync commands run on the main thread. A settings save or a keychain
    // prompt there freezes Chat and the character the way a wire wait did.
    tauri::async_runtime::spawn_blocking(move || settings_event_blocking(app, payload))
        .await
        .map_err(|why| format!("settings: {why}"))?
}

fn settings_event_blocking(
    app: tauri::AppHandle,
    payload: SettingsEventPayload,
) -> Result<SettingsEventResponse, String> {
    use settings::controller;

    let state = app
        .try_state::<SettingsState>()
        .ok_or("settings: asked for before the shell was ready")?;
    let session = settings_session(&app, &state);
    let view = session.view();
    let description = settings::form::describe();

    let mut pressed: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let (event, draft) = match payload {
        SettingsEventPayload::SetBool { set_bool, value } => (
            controller::Event::SetBool {
                id: set_bool,
                value,
            },
            settings::AiDraft::live(&description),
        ),
        SettingsEventPayload::SetText { set_text, value } => (
            controller::Event::SetText {
                id: set_text,
                value,
            },
            settings::AiDraft::live(&description),
        ),
        SettingsEventPayload::Press {
            press,
            draft: wire,
            fields,
        } => {
            pressed = fields;
            let draft = settings::AiDraft {
                rows: wire.unwrap_or_default(),
                description: &description,
            };
            (controller::Event::Press { id: press }, draft)
        }
        SettingsEventPayload::Pick {
            pick,
            value,
            fills: None,
        } if pick == settings::form::BYO_HARNESS_ID => {
            let (snippet, steps, token) = settings::byo_rows(&value);
            return Ok(SettingsEventResponse::PreviewByo {
                snippet,
                steps,
                token,
            });
        }
        SettingsEventPayload::Pick {
            pick,
            value,
            fills: None,
        } => (
            controller::Event::Pick { id: pick, value },
            settings::AiDraft::live(&description),
        ),
        SettingsEventPayload::Pick {
            pick,
            value,
            fills: Some(fills),
        } => (
            shortcut_event(pick, value, &fills, &view),
            settings::AiDraft::live(&description),
        ),
        SettingsEventPayload::Dismiss { dismiss, value } => {
            return Ok(dismiss_press(&session, &view, &dismiss, &value));
        }
    };

    let outcome = controller::handle(&event, &draft, &view);
    respond(
        outcome,
        |patch| session.apply(patch).map_err(|e| e.to_string()),
        |op| run_operation(&session, &op, &pressed),
    )
}

/// A pick from the list inside a row that fills that row's field. `current` is
/// what the field holds now, so a pick of the value already there writes nothing.
fn shortcut_event(
    pick: String,
    value: String,
    fills: &PickFills,
    view: &settings::SettingsView,
) -> settings::controller::Event {
    let current = view
        .development_texts
        .get(&fills.row)
        .map(|s| s.as_str())
        .or_else(|| {
            if fills.row == settings::form::DIRECTOR_BASE_URL_ID {
                Some(view.director_base_url.as_str())
            } else {
                None
            }
        })
        .unwrap_or_default()
        .to_string();
    settings::controller::Event::Shortcut {
        id: pick,
        value,
        current,
    }
}

/// The wire answer for each `Outcome`, with the two things that need a live
/// session handed in, so a test can pin the mapping. `Apply` answers
/// `Refresh` because the page redraws every tab switch from its cached
/// snapshot: answering `Nothing` here is the consent checkbox that came back
/// unticked (#995).
fn respond(
    outcome: settings::controller::Outcome,
    mut apply: impl FnMut(settings::SettingsPatch) -> Result<(), String>,
    run: impl FnOnce(settings::form::RowOperation) -> Result<SettingsEventResponse, String>,
) -> Result<SettingsEventResponse, String> {
    use settings::controller::Outcome;
    match outcome {
        Outcome::Nothing => Ok(SettingsEventResponse::Nothing),
        Outcome::Apply(patch) => {
            apply(patch)?;
            Ok(SettingsEventResponse::Refresh)
        }
        Outcome::Commit(patch) => {
            if let Some(patch) = patch {
                apply(patch)?;
            }
            Ok(SettingsEventResponse::Reset)
        }
        Outcome::Reset => Ok(SettingsEventResponse::Reset),
        Outcome::ClearKey => Ok(SettingsEventResponse::ClearKey),
        Outcome::Fill { id, value, patch } => {
            if let Some(patch) = patch {
                apply(patch)?;
            }
            Ok(SettingsEventResponse::Fill {
                id: id.to_string(),
                value: value.to_string(),
            })
        }
        Outcome::Run(op) => run(op),
    }
}

/// What a button press performs, so that `run_operation` can be tested.
///
/// `SettingsSession` is the real one. It needs a live `AppHandle`, which a
/// unit test has not got, and the bug #875 closes is a press that reached no
/// method at all — which is what a test with no seam here cannot see.
trait Operations {
    fn open_memory(&self) -> Result<(), String>;
    fn wipe_memory(&self) -> Result<(), String>;
    fn spawn(&self, character: String, name: String);
    fn dismiss(&self, id: String);
}

impl Operations for settings::SettingsSession {
    fn open_memory(&self) -> Result<(), String> {
        settings::SettingsSession::open_memory(self)
    }

    fn wipe_memory(&self) -> Result<(), String> {
        settings::SettingsSession::wipe_memory(self)
    }

    fn spawn(&self, character: String, name: String) {
        settings::SettingsSession::spawn(self, character, name);
    }

    fn dismiss(&self, id: String) {
        settings::SettingsSession::dismiss(self, id);
    }
}

/// Dismiss the Instance a list press names.
///
/// Not a `RowOperation`: the press names a row of the list rather than the
/// form, and only the roster can say whether that Instance is still there.
/// The page draws from a snapshot, so a stale press does nothing rather than
/// sending an op under an id nothing answers to. #875.
fn dismiss_press(
    session: &dyn Operations,
    view: &settings::SettingsView,
    row: &str,
    id: &str,
) -> SettingsEventResponse {
    if row == settings::form::INSTANCES_ID && view.instance(id).is_some() {
        session.dismiss(id.to_string());
    }
    SettingsEventResponse::Nothing
}

/// Run what #706 keeps in Rust, and hand the page only what it owns.
///
/// The two clipboard writes cross because WebKit gives `writeText` the click's
/// own turn and nothing else. Everything else is a `SettingsSession` method,
/// and the page has no case for one it is handed instead (#875).
fn run_operation(
    session: &dyn Operations,
    op: &settings::form::RowOperation,
    fields: &std::collections::HashMap<String, String>,
) -> Result<SettingsEventResponse, String> {
    use settings::form::RowOperation;
    let field = |id: &str| fields.get(id).cloned().unwrap_or_default();
    match op {
        RowOperation::OpenMemory => session
            .open_memory()
            .map(|()| SettingsEventResponse::Nothing),
        // The file is gone, so the window redraws from what is left.
        RowOperation::WipeMemory => session
            .wipe_memory()
            .map(|()| SettingsEventResponse::Refresh),
        // Nothing to redraw yet: the frame loop owns the roster and pushes
        // `settings-refresh` on the tick that spawns the Instance.
        RowOperation::Spawn => {
            session.spawn(
                field(settings::form::NEW_CHARACTER_ID),
                field(settings::form::NEW_NAME_ID),
            );
            Ok(SettingsEventResponse::Nothing)
        }
        _ => Ok(SettingsEventResponse::Run {
            operation: op.as_str().to_string(),
        }),
    }
}

/// What one open does with the shared Settings window.
#[derive(Debug, PartialEq, Eq)]
enum SettingsOpen {
    Create,
    Show,
    Focus,
    FocusAndReload,
    Rebuild,
}

/// Side effects of one open, in order. `dispatch_settings` runs this list
/// and nothing else, so a second open cannot grow a focus call unnoticed.
#[derive(Debug, PartialEq, Eq)]
enum SettingsEffect {
    Build,
    Unminimize,
    Focus,
    Raise,
    Reload,
    /// Drop the window. Tauri keeps the label until `Destroyed`, so this
    /// list does not also build. Building before that fails.
    Destroy,
}

fn settings_effects(plan: SettingsOpen) -> &'static [SettingsEffect] {
    match plan {
        SettingsOpen::Create => &[SettingsEffect::Build, SettingsEffect::Raise],
        // Raise delivers WM_SETFOCUS. Wry's subclass then MoveFocus and hangs.
        SettingsOpen::Show => &[],
        SettingsOpen::Focus => &[
            SettingsEffect::Unminimize,
            SettingsEffect::Focus,
            SettingsEffect::Raise,
        ],
        SettingsOpen::FocusAndReload => &[
            SettingsEffect::Unminimize,
            SettingsEffect::Focus,
            SettingsEffect::Raise,
            SettingsEffect::Reload,
        ],
        // Focus here would MoveFocus a window that is still loading.
        SettingsOpen::Rebuild => &[SettingsEffect::Destroy],
    }
}

/// What to run once `Destroyed` has freed the label. A window the user
/// closed is not built again.
fn settings_rebuild_effects(rebuild_pending: bool) -> &'static [SettingsEffect] {
    if rebuild_pending {
        settings_effects(SettingsOpen::Create)
    } else {
        &[]
    }
}

/// `reload` is the Chat path. `document_ready` is navigation finished.
/// `load_stalled` is that load past the deadline. `rebuild_pending` means
/// destroy has started and this open waits for the label to free.
fn settings_open(
    exists: bool,
    reload: bool,
    document_ready: bool,
    load_stalled: bool,
    rebuild_pending: bool,
) -> SettingsOpen {
    if rebuild_pending {
        // The label stays registered until Destroyed. Building here fails.
        return SettingsOpen::Show;
    }
    if !exists {
        return SettingsOpen::Create;
    }
    if !document_ready && load_stalled {
        return SettingsOpen::Rebuild;
    }
    if !document_ready {
        return SettingsOpen::Show;
    }
    if reload {
        SettingsOpen::FocusAndReload
    } else {
        SettingsOpen::Focus
    }
}

/// Past this, the next open drops a Settings document that never finished.
/// Inside it, the open stays a no-op so MoveFocus cannot hang WebView2.
const SETTINGS_LOAD_TIMEOUT: Duration = Duration::from_secs(15);

fn settings_load_expired(loading_for: Option<Duration>) -> bool {
    loading_for.is_some_and(|elapsed| elapsed >= SETTINGS_LOAD_TIMEOUT)
}

fn settings_loading_for(app: &tauri::AppHandle) -> Option<Duration> {
    let state = app.try_state::<SettingsState>()?;
    let since = state.loading_since.lock().ok()?;
    since.map(|started| started.elapsed())
}

fn settings_rebuild_pending(app: &tauri::AppHandle) -> bool {
    app.try_state::<SettingsState>()
        .is_some_and(|state| state.rebuild_after_destroy.load(Ordering::Acquire))
}

fn arm_settings_rebuild(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<SettingsState>() {
        state.rebuild_after_destroy.store(true, Ordering::Release);
    }
}

fn take_settings_rebuild(app: &tauri::AppHandle) -> bool {
    app.try_state::<SettingsState>()
        .is_some_and(|state| state.rebuild_after_destroy.swap(false, Ordering::AcqRel))
}

fn settings_document_ready(app: &tauri::AppHandle) -> bool {
    let Some(state) = app.try_state::<SettingsState>() else {
        return false;
    };
    let built = state.settings_built.load(Ordering::Acquire);
    built != 0 && built == state.settings_loaded.load(Ordering::Acquire)
}

/// Start a Settings document. The page-load handler passes this generation
/// back, so a completion from an older window cannot mark the new one ready.
fn begin_settings_document(app: &tauri::AppHandle) -> u64 {
    let Some(state) = app.try_state::<SettingsState>() else {
        return 0;
    };
    if let Ok(mut since) = state.loading_since.lock() {
        *since = Some(Instant::now());
    }
    state.settings_built.fetch_add(1, Ordering::AcqRel) + 1
}

fn finish_settings_document(app: &tauri::AppHandle, generation: u64) {
    let Some(state) = app.try_state::<SettingsState>() else {
        return;
    };
    if state.settings_built.load(Ordering::Acquire) == generation {
        state.settings_loaded.store(generation, Ordering::Release);
        if let Ok(mut since) = state.loading_since.lock() {
            *since = None;
        }
    }
}

/// Open or raise Settings aimed at one row. A loaded window reloads from the
/// next snapshot. One still loading has no listener, so the snapshot carries
/// the aim.
fn open_settings_at(app: &tauri::AppHandle, reveal: settings::form::Reveal) {
    if let Some(state) = app.try_state::<SettingsState>() {
        if let Ok(mut slot) = state.reveal.lock() {
            *slot = Some(reveal);
        }
    }
    // Off the pump: `names_hint_act` is async, so this queues the build.
    dispatch_settings(app.clone(), true);
}

/// The notice's two buttons. `async` is load-bearing on Windows: a sync
/// command runs on WebView2's pump thread, and building Settings there
/// leaves the window blank.
#[tauri::command]
async fn names_hint_act(
    action: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, SettingsState>,
) -> Result<names_hint::HintPush, String> {
    let press = names_hint::Press::parse(&action)
        .ok_or_else(|| format!("unknown notice action: {action}"))?;
    let acted = names_hint::live()
        .acted(press, &state.settings, &state.path)
        .map_err(|why| why.to_string())?;
    if let names_hint::Then::Reveal(reveal) = acted.then {
        open_settings_at(&app, reveal);
    }
    Ok(acted.push)
}

/// Open Settings from a webview. `async` is load-bearing on Windows: a sync
/// command runs on WebView2's pump thread and building Settings there leaves
/// the window blank. Tray and the menu call `present_settings` instead.
#[tauri::command]
async fn show_settings(app: tauri::AppHandle) {
    present_settings(app);
}

/// Raise Settings, or build it. Main-thread callers run the build inline;
/// a webview command is async, so this queues off WebView2's pump.
fn present_settings(app: tauri::AppHandle) {
    dispatch_settings(app, false);
}

/// `reload` asks the open page to pick up a new aim. A window this call
/// creates is not reloaded: the emit would run before navigation finishes.
fn dispatch_settings(app: tauri::AppHandle, reload: bool) {
    if app.try_state::<SettingsState>().is_none() {
        eprintln!("settings: opened before the shell was ready");
        return;
    }

    // Same clone-then-post as `open_chat`: the closure takes the handle,
    // `run_on_main_thread` still borrows `app`.
    let handle = app.clone();
    if let Err(why) = app.run_on_main_thread(move || {
        let existing = handle.get_webview_window("settings");
        let ready = settings_document_ready(&handle);
        let plan = settings_open(
            existing.is_some(),
            reload,
            ready,
            settings_load_expired(settings_loading_for(&handle)),
            settings_rebuild_pending(&handle),
        );
        apply_settings_effects(&handle, existing, settings_effects(plan));
    }) {
        eprintln!("settings webview: {why}");
    }
}

/// The Harness did not take a model or effort Settings applied. Put the
/// saved value back before the wake that found it returns, so the next wake
/// does not try it again and a restart does not bring it back, then tell an
/// open Settings window to read the notice.
fn restore_not_applied(app: &tauri::AppHandle, failures: &[harness::ConfigFailure]) {
    let Some(state) = app.try_state::<SettingsState>() else {
        return;
    };
    if let Err(why) = settings::restore_failed(&state.settings, &state.path, failures) {
        eprintln!("settings: could not put back a value the Harness refused: {why}");
    }
    let window = app.clone();
    let _ = app.run_on_main_thread(move || platform::refresh_settings(&window));
}

fn apply_settings_effects(
    handle: &tauri::AppHandle,
    mut window: Option<tauri::WebviewWindow>,
    effects: &[SettingsEffect],
) {
    for effect in effects {
        match effect {
            SettingsEffect::Build => {
                let generation = begin_settings_document(handle);
                match build_settings(handle, generation) {
                    Ok(built) => window = Some(built),
                    Err(why) => {
                        eprintln!("settings webview: {why}");
                        return;
                    }
                }
            }
            SettingsEffect::Unminimize => {
                if let Some(window) = &window {
                    let _ = window.unminimize();
                }
            }
            SettingsEffect::Focus => {
                if let Some(window) = &window {
                    let _ = window.set_focus();
                }
            }
            SettingsEffect::Raise => {
                if let Some(window) = &window {
                    if let Err(why) = platform::raise_above_overlay(window) {
                        eprintln!("settings webview raise: {why}");
                    }
                }
            }
            SettingsEffect::Reload => platform::refresh_settings(handle),
            SettingsEffect::Destroy => {
                let Some(open) = window.take() else {
                    return;
                };
                arm_settings_rebuild(handle);
                let app = handle.clone();
                open.on_window_event(move |event| {
                    if !matches!(event, tauri::WindowEvent::Destroyed) {
                        return;
                    }
                    let next = settings_rebuild_effects(take_settings_rebuild(&app));
                    if next.is_empty() {
                        return;
                    }
                    apply_settings_effects(&app, None, next);
                });
                // Controller::Close before the HWND, same order as quit.
                if let Err(why) = tauri::Webview::close(open.as_ref()) {
                    eprintln!("settings webview close: {why}");
                    let _ = take_settings_rebuild(handle);
                    return;
                }
                if let Err(why) = open.destroy() {
                    eprintln!("settings webview destroy: {why}");
                    let _ = take_settings_rebuild(handle);
                }
            }
        }
    }
}

fn persist_settings(settings: &std::sync::Mutex<settings::Settings>, path: &std::path::Path) {
    if let Err(why) = settings::flush_settings(settings, path) {
        eprintln!("settings: {why}");
    }
}

/// Write the roster to settings, including the Instance id: the prompt hangs
/// off it, and without it the next launch mints a fresh uuid and loses the
/// user's words (ADR-0012).
fn remember_instances(roster: &Roster, settings: &Arc<Mutex<Settings>>, path: &std::path::Path) {
    if settings.lock().is_ok_and(|mut guard| {
        guard.instances = roster_specs(roster);
        true
    }) {
        persist_settings(settings, path);
    }
}

/// The roster as settings stores it. One mapping for startup and later
/// persists, so a path that dropped the id cannot mint a fresh uuid and
/// lose the user's words (ADR-0012).
fn roster_specs(roster: &Roster) -> Vec<InstanceSpec> {
    roster
        .list()
        .into_iter()
        .map(|(id, name)| {
            let instance = roster.get(&id);
            InstanceSpec {
                character: instance
                    .map(|instance| instance.character_name().to_string())
                    .unwrap_or_default(),
                name,
                prompt: instance
                    .map(|instance| instance.prompt().to_string())
                    .unwrap_or_default(),
                id: Some(id),
            }
        })
        .collect()
}

/// The overlay heard the primary button. The frame loop polls a session
/// query that can miss a click on this window; this is the other witness.
#[tauri::command]
fn overlay_primary(down: bool) {
    platform::set_overlay_primary(down);
}

/// The overlay's quick message gained or lost the caret. Empty means none.
/// Read each tick, so a dropped invoke cannot leave the pet stuck.
#[tauri::command]
fn overlay_composing(instance: String) {
    platform::set_overlay_composing(Some(instance));
}

/// The owning overlay's draft for one Instance, on every change. `None` is a
/// closed pill: sent, dismissed, dragged away empty, or the Instance gone.
#[tauri::command]
fn overlay_report_qm_draft(instance: String, payload: Option<fidget_core::quick_message::QmDraft>) {
    platform::set_qm_draft(instance, payload);
}

/// Same witness for the right button. Without it a right-click on the sprite
/// is swallowed by the webview and the session poll never sees a Menu.
#[tauri::command]
fn overlay_secondary(down: bool) {
    platform::set_overlay_secondary(down);
}

/// Whether reporting an off-art rectangle would actually win the click (#547).
/// Replaces a UA sniff that said "macOS" when the question is hotspot hit-testing.
/// Those agree today and would part the moment X11 or Windows unions off-art rects into its input region.
#[tauri::command]
fn overlay_hit_tests_hotspots() -> bool {
    platform::hotspots_hit_tested()
}

/// `FIDGET_TRACE_CADENCE`, a bench switch rather than a Development row: the
/// overlay records its display frames and the frame loop counts its ticks.
pub(crate) fn tracing_cadence() -> bool {
    model::env_switch("FIDGET_TRACE_CADENCE").unwrap_or(false)
}

/// Whether the overlay records its display frames for `overlay_cadence`.
#[tauri::command]
fn overlay_traces_cadence() -> bool {
    tracing_cadence()
}

/// Display frames an overlay drew, in Unix ms: when it ran, whether it asked
/// for another, and the two arrivals it drew between. For scripts/frame-cadence.mjs.
#[tauri::command]
fn overlay_cadence(window: tauri::Window, frames: Vec<(f64, bool, Option<f64>, f64)>) {
    for (now, rearmed, previous, latest) in frames {
        let previous = previous.map_or_else(|| "-".to_string(), |at| format!("{at:.3}"));
        eprintln!(
            "cadence: {} {now:.3} {} {previous} {latest:.3}",
            window.label(),
            u8::from(rearmed)
        );
    }
}

/// What this overlay draws outside the art, in its own coordinates. Only the
/// renderer knows where, because it lays the bubble and the pill out; the
/// frame loop converts and takes a click over the clickable ones.
#[tauri::command]
fn overlay_rects(window: tauri::Window, rects: Vec<fidget_core::overlay_region::OverlayRect>) {
    platform::set_overlay_rects(window.label(), rects);
}

/// Trace frontend bubble events to stderr when FIDGET_TRACE_BUBBLE is on.
#[tauri::command]
fn overlay_trace_bubble(app: tauri::AppHandle, label: String, message: String) {
    if dev_flags::TRACE_BUBBLE.is_on() {
        let dnd = do_not_disturb(&app);
        eprintln!(
            "overlay {}: {} dnd={}",
            label,
            message,
            if dnd { "on" } else { "off" }
        );
    }
}

/// Chat window title: Instance name, or the id when the roster holds no row.
/// The id is a real fallback: a Character switch can drop the row between
/// the click and this lookup, and a window titled by id is better than none.
fn instance_title(rows: &[InstanceRow], id: &str) -> String {
    rows.iter()
        .find(|row| row.id == id)
        .map_or_else(|| id.to_string(), |row| row.name.clone())
}

/// Open Chat from the bubble control, not as a Summon: the Engine hears
/// nothing. `async` is load-bearing on Windows (#588): a sync command runs on
/// WebView2's pump thread and building a second webview there deadlocks.
#[tauri::command]
async fn overlay_open_chat(app: tauri::AppHandle, id: String) {
    let title = app
        .try_state::<SettingsState>()
        .and_then(|state| {
            state
                .instances
                .lock()
                .ok()
                .map(|rows| instance_title(&rows, &id))
        })
        .unwrap_or_else(|| id.clone());
    open_chat(&app, &id, title, None);
}

/// The pill reopening on a new owner takes the caret. Windows refuses
/// SetForegroundWindow to a background thread, so the overlay borrows the
/// foreground thread's input for the call.
#[tauri::command]
fn overlay_request_focus(window: tauri::Window) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
        };

        #[link(name = "user32")]
        extern "system" {
            fn AttachThreadInput(idattach: u32, idattachto: u32, fattach: i32) -> i32;
        }

        let raw_handle = window
            .window_handle()
            .map_err(|e| format!("Window handle not available: {e}"))?;
        if let RawWindowHandle::Win32(win32_handle) = raw_handle.as_raw() {
            let hwnd = win32_handle.hwnd.get() as HWND;
            // SAFETY: hwnd is this live overlay's window; the thread ids come from
            // Win32 for live windows, and any attach is undone before returning.
            unsafe {
                let foreground = GetForegroundWindow();
                let mut attached = None;
                if !foreground.is_null() {
                    let foreground_thread =
                        GetWindowThreadProcessId(foreground, std::ptr::null_mut());
                    let overlay_thread = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());

                    if foreground_thread != overlay_thread
                        && AttachThreadInput(overlay_thread, foreground_thread, 1) != 0
                    {
                        attached = Some((overlay_thread, foreground_thread));
                    }
                }
                SetForegroundWindow(hwnd);
                if let Some((overlay_thread, foreground_thread)) = attached {
                    AttachThreadInput(overlay_thread, foreground_thread, 0);
                }
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        window.set_focus().map_err(|e| e.to_string())
    }
}

/// Put the overlay over one display. Size before move: growing a window
/// anchors bottom-left, so resize-after-place pushes the top edge off the
/// display it was just put on.
fn cover_display(window: &tauri::WebviewWindow, display: Rect) -> Result<(), tauri::Error> {
    window.set_size(LogicalSize::new(display.width, display.height))?;
    window.set_position(LogicalPosition::new(display.x, display.y))
}

/// Build one overlay, configure it, and put it over its display. The only
/// place an overlay is made, so click-through, window level, Spaces and hide
/// rules stay one set. Main thread only: it builds a window and calls AppKit.
fn build_overlay(
    app: &tauri::AppHandle,
    label: &str,
    display: Rect,
) -> Result<(), Box<dyn std::error::Error>> {
    // WebView2 spawned here inherits ignore Ctrl+C, including a display that
    // arrives after the quit handler is already installed.
    let _spawned_ctrl_c = platform::SpawnedCtrlC::hold();
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .title("Fidget")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .accept_first_mouse(true)
        .focused(false)
        .resizable(false)
        .skip_taskbar(true)
        .visible(false)
        .build()?;

    // On Linux/GTK, the GdkWindow handle is unavailable until realize (at
    // show). On Windows/macOS, the native handle exists while hidden.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    platform::set_overlay_click_through(&window, true)?;

    // X11: the window manager may put the overlay somewhere other than asked
    // (xfwm4 pushes it out of a desktop panel's reserved strip). Heard before
    // the first show, so the move that maps it is not missed.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let watched = window.clone();
        let owner = label.to_string();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::Moved(at) = event {
                let at = at.to_logical::<f64>(watched.scale_factor().unwrap_or(1.0));
                if platform::overlay_origin(&owner) != Some((at.x, at.y)) {
                    eprintln!("overlay: {owner} is at ({:.0},{:.0})", at.x, at.y);
                }
                platform::set_overlay_origin(&owner, (at.x, at.y));
            }
        });
    }

    cover_display(&window, display)?;

    // Windows: configure before show to prevent white flash.
    // HWND exists while hidden, so all operations succeed.
    #[cfg(windows)]
    platform::configure_overlay(&window)?;

    // Show the window. On Linux/GTK this realizes the widget and creates the
    // X11 window ID that configure_overlay needs.
    window.show()?;

    // Linux: configure_overlay after show. May fail if the widget is not yet
    // realized; the frame loop retries until success.
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Err(why) = platform::configure_overlay(&window) {
        eprintln!("overlay: {label} EWMH config deferred: {why}");
    }

    // macOS: configure after show. Configured as a panel before its first
    // show, the overlay plays an open animation and overshoots the display.
    #[cfg(target_os = "macos")]
    platform::configure_overlay(&window)?;

    eprintln!(
        "overlay: {label} covers {:.0}x{:.0} at ({:.0},{:.0})",
        display.width, display.height, display.x, display.y,
    );

    Ok(())
}

/// Build one Instance's Chat surface. None of `build_overlay`'s flags:
/// click-through would swallow the caret click; always-on-top would follow the
/// user out of the app. Chat floats over the bubble only while it has focus.
fn build_chat(
    app: &tauri::AppHandle,
    label: &str,
    title: &str,
    at: Option<(f64, f64)>,
) -> Result<tauri::WebviewWindow, tauri::Error> {
    let _spawned_ctrl_c = platform::SpawnedCtrlC::hold();
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("chat.html".into()))
        .title(title)
        .inner_size(CHAT_SIZE.0, CHAT_SIZE.1)
        .min_inner_size(320.0, 320.0)
        .focused(true);
    if let Some((x, y)) = at {
        builder = builder.position(x, y);
    }
    let window = builder.build()?;
    let (handle, label) = (app.clone(), label.to_string());
    let observer = platform::observe_minimize(&window, {
        let (handle, label) = (handle.clone(), label.clone());
        move |minimized| {
            if let Some(chats) = handle.try_state::<MinimizedChats>() {
                chats.set(&label, minimized);
            }
        }
    })
    .inspect_err(|why| eprintln!("chat: {label} minimize: {why}"))
    .ok();
    let observer = std::cell::Cell::new(observer);
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            observer.take();
        }
        let Some(chats) = handle.try_state::<MinimizedChats>() else {
            return;
        };
        let window = handle.get_webview_window(&label);
        let minimized = || {
            window
                .as_ref()
                .is_some_and(|window| window.is_minimized().unwrap_or(false))
        };
        let (Some(floats), Some(window)) = (chats.note(&label, event, minimized), &window) else {
            return;
        };
        let level = if floats {
            platform::raise_above_overlay(window)
        } else {
            platform::lower_to_normal_level(window)
        };
        if let Err(why) = level {
            eprintln!("chat: {label} level: {why}");
        }
    });
    Ok(window)
}

/// A new Chat window's inner size, in points.
const CHAT_SIZE: (f64, f64) = (420.0, 560.0);

/// Where a Chat opens for the sprite whose feet are at `feet`: beside it, on
/// the display it stands on, and inside that display. `None` leaves the
/// placement to the windowing layer, which centres on the main display.
fn chat_origin(feet: Point, displays: &[Rect], size: (f64, f64)) -> Option<(f64, f64)> {
    // Clears the half-width of a scale-2 sprite, so Chat does not cover it.
    const GAP: f64 = 96.0;
    let display = displays[display_index_for((feet.x, feet.y), displays)?];
    let right_edge = display.x + display.width;
    let x = if feet.x + GAP + size.0 <= right_edge {
        feet.x + GAP
    } else {
        feet.x - GAP - size.0
    };
    Some((
        x.clamp(display.x, (right_edge - size.0).max(display.x)),
        (feet.y - size.1).clamp(
            display.y,
            (display.y + display.height - size.1).max(display.y),
        ),
    ))
}

/// Build the Settings webview window. `generation` is the document this
/// build started; the load handler records it when navigation finishes.
fn build_settings(
    app: &tauri::AppHandle,
    generation: u64,
) -> Result<tauri::WebviewWindow, tauri::Error> {
    let _spawned_ctrl_c = platform::SpawnedCtrlC::hold();
    let handle = app.clone();
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Settings")
        .inner_size(600.0, 520.0)
        .min_inner_size(480.0, 400.0)
        .focused(true)
        .on_page_load(move |_window, payload| {
            if payload.event() != tauri::webview::PageLoadEvent::Finished {
                return;
            }
            // about:blank finishes first. It is not the Settings document.
            if payload.url().as_str().starts_with("about:") {
                return;
            }
            finish_settings_document(&handle, generation);
        })
        .build()
}

/// Open this Instance's Chat surface, or raise the one it already has.
/// Posted whole to the main thread; the lookup goes too, because Ok from
/// `run_on_main_thread` means queued, so two Summons a tick apart would each post a build.
/// A new window opens beside `feet` when the caller knows where the sprite is.
fn open_chat(app: &tauri::AppHandle, id: &InstanceId, title: String, feet: Option<Point>) {
    let label = chat_label(id);
    let at = feet
        .and_then(|feet| chat_origin(feet, &platform::read_displays(app).usable_frames, CHAT_SIZE));
    let handle = app.clone();
    if let Err(why) = app.run_on_main_thread(move || {
        let window = match handle.get_webview_window(&label) {
            Some(window) => {
                // The title is the Instance name, which a Character switch can
                // change without this window being rebuilt. #375.
                if let Err(why) = window.set_title(&title) {
                    eprintln!("chat: {label} could not be retitled: {why}");
                }
                let _ = window.unminimize();
                window
            }
            None => match build_chat(&handle, &label, &title, at) {
                Ok(window) => window,
                Err(why) => {
                    eprintln!("chat: {label}: {why}");
                    return;
                }
            },
        };
        // Both paths: `focused(true)` only orders the window to the front of
        // this application. The raise activates the process, which a Summon has
        // to do, and lifts Chat over the overlay so the bubble cannot cover it.
        if let Err(why) = platform::raise_above_overlay(&window) {
            eprintln!("chat: {label} could not be raised: {why}");
        }
    }) {
        eprintln!("chat: could not reach the main thread: {why}");
    }
}

/// Shut the Chat surface belonging to `id`, if it has one. Nothing else
/// closes a `chat-*`: they sit outside the `overlay-` namespace
/// `place_overlays` sweeps. Main thread only, for `open_chat`'s reason.
fn close_chat(app: &tauri::AppHandle, id: &InstanceId) {
    let label = chat_label(id);
    let handle = app.clone();
    if let Err(why) = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(&label) {
            if let Err(why) = window.close() {
                eprintln!("chat: {label} could not be closed: {why}");
            }
        }
    }) {
        eprintln!("chat: could not reach the main thread: {why}");
    }
}

/// Draw one forwarded permission request on the Chats its owner names
/// (`draws_in`). fidget never answers it (ADR-0018). Only a Chat surface draws
/// the options. A bubble can only point at a window that is not open.
fn forward_ask(app: &tauri::AppHandle, owner: Owner, ask: harness::PermissionAsk) {
    let Some(state) = app.try_state::<PendingAsks>() else {
        return;
    };
    let Ok(mut pending) = state.0.lock() else {
        return;
    };
    let mut on_screen = false;
    let mut shut = None;
    for (label, window) in app.webview_windows() {
        if !draws_in(&owner, &label) {
            continue;
        }
        let _ = app.emit_to(&label, CHAT_PERMISSION_EVENT, &ask);
        if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true) {
            on_screen = true;
        } else {
            shut = Some(label);
        }
    }
    let dnd = !on_screen && do_not_disturb(app);
    if pending.hold_ask(&owner, &ask, !on_screen, dnd) {
        show_chat_for_ask(app, owner, shut);
    }
    let next = if on_screen {
        "answer it in the Chat window"
    } else {
        "no Chat surface is on screen"
    };
    eprintln!(
        "harness: permission asked for `{}`; {next}",
        ask.title.as_deref().unwrap_or("—")
    );
}

/// A form, delivered as `forward_ask` delivers an ask.
fn forward_form(app: &tauri::AppHandle, owner: Owner, form: harness::ElicitationForm) {
    let Some(state) = app.try_state::<PendingAsks>() else {
        return;
    };
    let Ok(mut pending) = state.0.lock() else {
        return;
    };

    let mut on_screen = false;
    let mut shut = None;
    for (label, window) in app.webview_windows() {
        if !draws_in(&owner, &label) {
            continue;
        }
        let _ = app.emit_to(&label, CHAT_ELICITATION_EVENT, &form);
        if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true) {
            on_screen = true;
        } else {
            shut = Some(label);
        }
    }
    let dnd = !on_screen && do_not_disturb(app);
    if pending.hold_form(&owner, &form, !on_screen, dnd) {
        show_chat_for_ask(app, owner, shut);
    }
    let next = if on_screen {
        "answer it in the Chat window"
    } else if form.waits {
        "it waits for the next Chat to open"
    } else {
        "no Chat surface is on screen"
    };
    eprintln!("harness: elicitation `{}`; {next}", form.message);
}

/// Show the thought so far in the thinking Instance's Chat surface, from
/// either Completer lane (#611). The session log holds it until that
/// Instance's wake files it (ADR-0034).
fn show_thought(app: &tauri::AppHandle, instance: &str, line: String) {
    session_log::think(app, instance, &line, SystemTime::now());
    let _ = app.emit_to(chat_label(instance), CHAT_THOUGHT_EVENT, &line);
}

fn handle_inbound_wake(app: &tauri::AppHandle, wake: harness::InboundWake) {
    if let Some(state) = app.try_state::<ChatChannel>() {
        let _ = state.0.send(ChatMsg::InboundWake(wake));
    }
}

/// A loaded session's history, into its Instance's log and open window. Not
/// sent to the frame loop: nothing is addressed and Pace is untouched (#1393).
/// Only to that window, and never as `CHAT_PLAN_EVENT`: a replayed plan is
/// history, not the plan on the wire now.
/// A window that listens before `chat_ready` reads the log hears it here and
/// again in that replay. `restored()` in chat.js replaces, so it draws once.
fn restore_history(app: &tauri::AppHandle, restored: harness::Restored) {
    let Some(state) = app.try_state::<PendingAsks>() else {
        return;
    };
    let Ok(_replaying) = state.0.lock() else {
        return;
    };
    let _ = app.emit_to(
        chat_label(&restored.instance),
        CHAT_RESTORED_EVENT,
        &restored.history,
    );
    session_log::restore(app, &restored.instance, restored.history);
}

/// Show the agent's plan in the Chat surface of the Instance whose session
/// planned, as a thought is shown.
fn show_plan(app: &tauri::AppHandle, instance: &str, steps: &[harness::PlanStep]) {
    let _ = app.emit_to(chat_label(instance), CHAT_PLAN_EVENT, steps);
}

/// Retire one request in every open Chat surface, under the same lock a replay
/// reads.
fn settle_ask(app: &tauri::AppHandle, settled: Settled) {
    let Some(state) = app.try_state::<PendingAsks>() else {
        return;
    };
    let Ok(mut pending) = state.0.lock() else {
        return;
    };
    pending.settle(&settled.request);
    for label in app.webview_windows().into_keys() {
        if label.starts_with("chat-") {
            let _ = app.emit_to(label, CHAT_PERMISSION_SETTLED_EVENT, &settled);
        }
    }
}

/// The app-wide Do Not Disturb switch, which is what the tray toggle writes.
fn do_not_disturb(app: &tauri::AppHandle) -> bool {
    app.try_state::<SettingsState>()
        .and_then(|state| state.settings.lock().ok().map(|read| read.do_not_disturb))
        .unwrap_or(false)
}

/// Put a Chat surface in front of the user for a request with nowhere to be
/// drawn: the owing Instance's, else a shut one that would draw it, else the
/// roster's first.
fn show_chat_for_ask(app: &tauri::AppHandle, owner: Owner, shut: Option<String>) {
    let rows = app
        .try_state::<SettingsState>()
        .and_then(|state| state.instances.lock().ok().map(|rows| rows.clone()))
        .unwrap_or_default();
    let id = match owner {
        Owner::Instance(instance) => Some(instance),
        Owner::EveryChat => shut
            .as_deref()
            .and_then(|label| label.strip_prefix("chat-"))
            .map(str::to_string)
            .or_else(|| rows.first().map(|row| row.id.clone())),
    };
    let Some(id) = id else {
        eprintln!("harness: no Instance to ask on; the request will time out");
        return;
    };
    let title = instance_title(&rows, &id);
    open_chat(app, &id, title, None);
}

/// Record what just happened, unless a typed line is still waiting.
/// `happened` is one slot: a Poke between a line arriving and the wake
/// would replace the question and answer one nobody asked.
fn note_happened(happened: &mut Happened, what: Happened) {
    if !matches!(happened, Happened::Chat(_)) {
        *happened = what;
    }
}

/// What a Chat surface needs to draw itself before anything is typed.
#[derive(Clone, Serialize)]
struct ChatOpening {
    name: String,
    character: String,
    /// Whether a Completer exists at all — a key, or a local host.
    configured: bool,
    /// Whether the switch is on as well. Configured and switched off is a
    /// different sentence from never configured, and the surface says which.
    enabled: bool,
    /// Which mind answers here, or would (#474): the Harness that was named,
    /// whether the child is up, and the session that proves it. `None` when
    /// none was named, and the HTTP rows below are the answer instead.
    harness: Option<ChatHarness>,
    /// The HTTP Completer in force: model and host, never a credential.
    model: String,
    host: String,
    /// Which Chat UI design the user picked: "minimal", "terminal", or "glass".
    chat_ui: String,
    chat_appearance: ChatAppearance,
    /// A Harness is attached but not signed in. Names the login command for
    /// the user's own terminal.
    login: Option<String>,
    /// Agent methods the landing can run in-app. Omitted when there are none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sign_in: Vec<harness::SignIn>,
    /// Which Harness is attached, when one is. Used to name it in the fourth
    /// empty state (needs authentication).
    harness_name: Option<String>,
    /// App-level instructions as sent: voice rules, Behavior roster, reply
    /// contract. Empty under Blank AI. The Prompt tab draws this frozen so an
    /// emptied control run is a visible Empty, not a missing block.
    instructions: String,
    /// The Character's frozen Personality Prompt, for the Prompt tab
    /// (ADR-0012). Empty when the package shipped none, and empty under
    /// Blank AI: the flag empties the layer rather than hiding the tab (#680).
    personality: String,
    /// This Instance's own layer, as it stands. Empty by default. Still sent
    /// under Blank AI, so a control run can iterate a prompt (#680).
    instance_prompt: String,
    /// What the tab may not exceed, so the box can say so before the save
    /// surface has to.
    prompt_limit: usize,
}

/// The Harness half of an opening. Facts and not a sentence: the wording is
/// the window's, and `chat-status.js` is where it has a test.
#[derive(Clone, Serialize)]
struct ChatHarness {
    name: String,
    /// Attached but not signed in. Names the login command for the user's
    /// own terminal.
    login: Option<String>,
    /// Whether the child is up. Set and dead is the state the Chat surface
    /// could not tell from attached before #474, and it is a lie worth more
    /// than a missing label.
    alive: bool,
    session: Option<String>,
    /// The binary `PATH` has not got, when that is why nothing is running.
    /// Settings already names it (#659). Chat used to drop it and say
    /// `not running` (#726).
    missing: Option<String>,
    /// The page that installs `missing`, from the table Settings names it from.
    install: Option<String>,
    /// Whether ACP handshake/spawn is in progress. Gates chat until ready or failed.
    initializing: bool,
    /// Why a launcher that was there gave no wire: its command, the reason,
    /// and what it printed, which the landing draws apart.
    failed: Option<harness::LaunchFailure>,
    /// Why preflight refused the launcher. Drawn as `failed` is.
    unhealthy: Option<harness::LaunchFailure>,
}

fn chat_harness(inspect: &model::DirectorInspect) -> Option<ChatHarness> {
    inspect.harness.as_ref().map(|attached| ChatHarness {
        name: attached.name.clone(),
        login: attached.login.clone(),
        alive: attached.alive,
        session: attached.session_id.clone(),
        missing: attached.missing.clone(),
        install: attached
            .missing
            .as_deref()
            .and_then(harness::install_page)
            .map(|(_, url)| url.to_string()),
        initializing: attached.initializing,
        failed: attached.failed.clone(),
        unhealthy: attached.unhealthy.clone(),
    })
}

#[cfg(test)]
fn chat_opening_from(
    instance: &roster::Instance,
    inspect: &model::DirectorInspect,
    personality: &str,
) -> ChatOpening {
    chat_opening_layers(
        instance,
        inspect,
        personality,
        std::iter::empty::<&str>(),
        "minimal",
        ChatAppearance::System,
    )
}

fn chat_opening_layers(
    instance: &roster::Instance,
    inspect: &model::DirectorInspect,
    personality: &str,
    behaviors: impl IntoIterator<Item = impl AsRef<str>>,
    chat_ui: &str,
    chat_appearance: ChatAppearance,
) -> ChatOpening {
    let blank = model::blank();
    ChatOpening {
        name: instance.name.clone(),
        character: instance.character_name().to_string(),
        configured: inspect.configured,
        enabled: inspect.enabled,
        harness: chat_harness(inspect),
        model: inspect.model.clone(),
        host: inspect.host.clone(),
        chat_ui: chat_ui.to_string(),
        chat_appearance,
        login: inspect
            .harness
            .as_ref()
            .and_then(|attached| attached.login.clone()),
        sign_in: harness::sign_in_actions(),
        harness_name: inspect
            .harness
            .as_ref()
            .map(|attached| attached.name.clone()),
        instructions: app_instructions(behaviors, blank),
        personality: if blank {
            String::new()
        } else {
            personality.to_string()
        },
        instance_prompt: instance.prompt().to_string(),
        prompt_limit: roster::INSTANCE_PROMPT_LIMIT,
    }
}

/// Personality Prompt of the Character `instance` is running. Read off the
/// loaded packages rather than held on the Instance: it is the author's
/// layer and the Prompt tab shows it frozen (ADR-0012).
fn personality_of(
    characters: &BTreeMap<String, Arc<Character>>,
    instance: &roster::Instance,
) -> String {
    characters
        .get(instance.character_name())
        .map(|character| character.personality.clone())
        .unwrap_or_default()
}

fn behavior_names_of(
    characters: &BTreeMap<String, Arc<Character>>,
    instance: &roster::Instance,
) -> Vec<String> {
    characters
        .get(instance.character_name())
        .map(|character| character.behaviors.keys().cloned().collect())
        .unwrap_or_default()
}

/// Who this Chat surface belongs to, and whether anything can answer. A
/// command rather than an event: Tauri buffers nothing for a listener that
/// is not there yet. Same `DirectorInspect` the Settings window renders.
#[tauri::command]
fn chat_opening(instance: String, state: tauri::State<'_, SettingsState>) -> ChatOpening {
    let (name, character, instance_prompt) = state
        .instances
        .lock()
        .ok()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.id == instance)
                .map(|row| (row.name.clone(), row.character.clone(), row.prompt.clone()))
        })
        .unwrap_or_default();
    // Re-read on every opening: a login the user ran mid-session moves the
    // Harness out of the not-authenticated state, and nothing else pushes it.
    if let Ok(mut inspect) = state.inspect.lock() {
        inspect.harness = harness::attached().map(|session| session.inspect());
    }
    let inspect = state.inspect.lock().ok();
    let blank = model::blank();
    ChatOpening {
        instructions: app_instructions(
            state
                .behavior_names
                .get(&character)
                .cloned()
                .unwrap_or_default(),
            blank,
        ),
        personality: if blank {
            String::new()
        } else {
            state
                .personalities
                .get(&character)
                .cloned()
                .unwrap_or_default()
        },
        name,
        character,
        configured: inspect.as_ref().is_some_and(|read| read.configured),
        enabled: inspect.as_ref().is_some_and(|read| read.enabled),
        harness: inspect.as_ref().and_then(|read| chat_harness(read)),
        model: inspect
            .as_ref()
            .map(|read| read.model.clone())
            .unwrap_or_default(),
        host: inspect
            .as_ref()
            .map(|read| read.host.clone())
            .unwrap_or_default(),
        login: inspect
            .as_ref()
            .and_then(|read| read.harness.as_ref())
            .and_then(|attached| attached.login.clone()),
        sign_in: harness::sign_in_actions(),
        harness_name: inspect
            .as_ref()
            .and_then(|read| read.harness.as_ref())
            .map(|attached| attached.name.clone()),
        chat_ui: state
            .settings
            .lock()
            .ok()
            .map(|settings| settings.chat_ui.clone())
            .unwrap_or_else(|| "minimal".to_string()),
        chat_appearance: state
            .settings
            .lock()
            .ok()
            .map(|settings| settings.chat_appearance)
            .unwrap_or_default(),
        instance_prompt,
        prompt_limit: roster::INSTANCE_PROMPT_LIMIT,
    }
}

/// A new Instance Prompt from its Chat surface. Refused rather than cut, so
/// the words still in the box are the words that were not saved. Persist and
/// reopen happen on the frame-loop thread (ADR-0012).
#[tauri::command]
fn chat_prompt(
    instance: String,
    text: String,
    chat: tauri::State<'_, ChatChannel>,
) -> Result<(), String> {
    let prompt = roster::instance_prompt(&text)?;
    chat.0
        .send(ChatMsg::Wrote(ChatLine {
            instance,
            text: prompt,
            echo: false,
        }))
        .map_err(|_| "Fidget is not listening.".to_string())
}

/// The user's pick on a forwarded permission request. The only path by which
/// a `session/request_permission` is ever answered.
#[tauri::command]
fn permission_answer(request: String, option: String) {
    if let Some(session) = harness::attached() {
        session.answer_permission(&request, &option);
    }
}

/// The user's pick on a forwarded elicitation form. `value` is the chosen
/// option; omitted is Decline, which is a valid answer.
#[tauri::command]
fn elicitation_answer(request: String, value: Option<String>) {
    if let Some(session) = harness::attached() {
        let answer = match value {
            Some(value) if !value.is_empty() => harness::ElicitationAnswer::Accept(value),
            _ => harness::ElicitationAnswer::Decline,
        };
        session.answer_elicitation(&request, answer);
    }
}

/// Open a clicked reply link in the user's browser. The webview has no
/// opener; an `<a href>` would navigate the chat window itself. Scheme
/// gating is `platform::open_url`'s, at the last edge; the URL is untrusted.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    platform::open_url(&url)
}

/// Connect a named Harness from Chat. Through `SettingsSession::apply`, so
/// the attachment moves now and `ReloadChat` carries state back. The answer
/// is a line to read, not a process to spawn; `harness::login_hint` owns why.
#[tauri::command]
async fn select_harness(
    harness: String,
    instance: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, SettingsState>,
) -> Result<String, String> {
    // Same as `settings_event`: the save and the keychain read must not run
    // on the main thread. The state handle is cloned into the worker.
    let settings = std::sync::Arc::clone(&state.settings);
    let session = settings_session(&app, &state);
    let key = session_key(&instance, &state);
    tauri::async_runtime::spawn_blocking(move || {
        select_harness_blocking(harness, key, session, settings)
    })
    .await
    .map_err(|why| format!("settings: {why}"))?
}

/// The session slot a wake from this Instance loads, or `None` for a row
/// Settings no longer has. A pick with no row still switches the Harness.
fn session_key(instance: &str, state: &SettingsState) -> Option<harness::SessionKey> {
    let rows = state.instances.lock().ok()?;
    let row = rows.iter().find(|row| row.id == instance)?;
    Some(harness::SessionKey {
        instance: instance.to_string(),
        character: row.character.clone(),
        blank: model::blank(),
    })
}

fn select_harness_blocking(
    harness: String,
    key: Option<harness::SessionKey>,
    session: settings::SettingsSession,
    settings: std::sync::Arc<std::sync::Mutex<settings::Settings>>,
) -> Result<String, String> {
    // The picker's own list, not a second copy of it: a name added to one and
    // not the other is a row the user can pick and this command then refuses.
    if !settings::form::HARNESS_PRESETS.contains(&harness.as_str()) {
        return Err(format!("unknown Harness preset: {harness}"));
    }
    let mut patch = settings::SettingsPatch::default();
    patch.set_text(settings::TextField::Harness, &harness);
    session.apply(patch)?;
    // Apply only retargets a changed row. The same Harness picked again after
    // its launch failed, or while it waits on a login, is the user asking
    // for another try.
    let spawning = settings
        .lock()
        .is_ok_and(|settings| model::director_in_force(settings.director_enabled));
    if let Some(attached) = harness::attached().filter(|_| spawning) {
        attached.repick(key)?;
    }
    Ok(harness::login_hint(&harness))
}

/// In-app sign-in for one advertised agent method. Keyed by the Instance id a
/// wake carries. The display name would open a slot nothing later loads.
/// Off the main thread: a device flow holds `authenticate` for minutes, and a
/// sync command would freeze the overlay for all of it.
#[tauri::command]
async fn sign_in(
    instance: String,
    method_id: String,
    state: tauri::State<'_, SettingsState>,
) -> Result<(), String> {
    // A missing row must not open a session under an empty Character. Wakes
    // key the slot on both, and an empty Character would not be the one they load.
    let Some(key) = session_key(&instance, &state) else {
        return Err("unknown instance".to_string());
    };
    let Some(session) = harness::attached() else {
        return Err(harness::LOST.to_string());
    };
    tauri::async_runtime::spawn_blocking(move || {
        session.sign_in(&method_id, &key.instance, &key.character, key.blank)
    })
    .await
    .map_err(|why| format!("sign-in stopped: {why}"))?
}

/// Push a full opening to an already-open Chat surface, without creating
/// one. After a switch the Character line still has to move; configured and
/// login too, because the window asked once at start.
fn push_chat_opening(
    app: &tauri::AppHandle,
    roster: &Roster,
    id: &InstanceId,
    inspect: &model::DirectorInspect,
    characters: &BTreeMap<String, Arc<Character>>,
    chat_ui: &str,
    chat_appearance: ChatAppearance,
) {
    let Some(instance) = roster.get(id) else {
        return;
    };
    let opening = chat_opening_layers(
        instance,
        inspect,
        &personality_of(characters, instance),
        behavior_names_of(characters, instance),
        chat_ui,
        chat_appearance,
    );
    let label = chat_label(id);
    let title = opening.name.clone();
    let handle = app.clone();
    fan_out_chat_opening(
        &label,
        |overlay| app.get_webview_window(overlay).is_some(),
        |target, event| {
            let _ = app.emit_to(target, event, &opening);
        },
    );
    if let Err(why) = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(&label) {
            if let Err(why) = window.set_title(&title) {
                eprintln!("chat: {label} could not be retitled: {why}");
            }
        }
    }) {
        eprintln!("chat: could not reach the main thread: {why}");
    }
}

fn push_chat_openings(
    app: &tauri::AppHandle,
    roster: &Roster,
    inspect: &model::DirectorInspect,
    characters: &BTreeMap<String, Arc<Character>>,
    chat_ui: &str,
    chat_appearance: ChatAppearance,
) {
    let ids: Vec<_> = roster.list().into_iter().map(|(id, _)| id).collect();
    for id in ids {
        push_chat_opening(
            app,
            roster,
            &id,
            inspect,
            characters,
            chat_ui,
            chat_appearance,
        );
    }
}

struct ChatLine {
    instance: InstanceId,
    text: String,
    /// Typed on the overlay. The open Chat surface did not draw this row.
    echo: bool,
}

/// What one Chat surface has to say to the frame loop.
enum ChatMsg {
    Said(ChatLine),
    /// A new Instance Prompt, already inside the bound. Saving it reopens that
    /// Instance's Director session, which is the frame loop's to do: it holds
    /// the roster and the Director slots (ADR-0012).
    Wrote(ChatLine),
    /// The surface is listening and has drawn nothing yet: the bar is pushed on
    /// change, so a window opened between two would sit at dashes. Sent after
    /// the listener is registered, or it is an answer nobody hears.
    Listening(InstanceId),
    InboundWake(harness::InboundWake),
}

/// The sender every Chat surface posts on. Not another `SettingsOp`: every
/// op drained from that one rewrites settings.json. A struct rather than a
/// bare `Sender`, because managed state is keyed by type.
struct ChatChannel(mpsc::Sender<ChatMsg>);

/// What the Shell tells one Chat surface when a turn of its own is over.
#[derive(Clone, Serialize)]
struct ChatReply {
    /// What the Instance said; `None` when the turn produced no line. Sent
    /// anyway, so no caret waits forever on a line that is not coming.
    said: Option<String>,
    /// The line was refused because one typed before it has not been asked
    /// yet. `said` is `None`; see the drain in `frame_loop`.
    busy: bool,
    /// What the Director was reacting to. `None` on a typed-line answer
    /// (that sits under the user's turn). A label rather than a flag: a
    /// double-click is a prompt, the user just did not type.
    reacting_to: Option<String>,
    /// A replayed line the user typed, or a quick message typed on the overlay.
    /// The Chat surface's own send draws its row itself, so a true here on
    /// that path would duplicate it.
    #[serde(default)]
    you: bool,
    /// A replayed Thinking row, with the thought in `said` (ADR-0034). Live
    /// thinking arrives on `CHAT_THOUGHT_EVENT` instead.
    thought: bool,
    /// Milliseconds since the epoch when the line was said. `None` on a live
    /// emit so the surface stamps wall-clock now; replay fills this from
    /// `Turn.at` so a line said before Chat opened keeps that moment.
    at: Option<u64>,
    /// What the Harness answered with when it answered with an error. `said`
    /// is `None` because static weights took the turn, but "no answer" is
    /// wrong when one named a version this CLI will not serve (#514).
    error: Option<String>,
    /// The Harness's own words when it failed the turn. Chat draws them as a
    /// Harness error rather than a note, because they are the diagnosis.
    failure: Option<String>,
    /// The Shell cancelled this caret because a newer wake started (ADR-0016),
    /// named in `happened_cell`'s word for that wake. `said` is `None`; this is
    /// not a turn that produced no Speech (#681). The surface says which wake
    /// did it, because "you poked me" reads as cause and "dropped" as a bug (#890).
    #[serde(default)]
    superseded_by: Option<&'static str>,
    /// Streaming answer-so-far from Speech, not the final reply. The turn
    /// stays open; the row replaces content instead of appending.
    #[serde(default)]
    streaming: bool,
}

/// What the Shell owes a Chat surface when a newer wake cancels the slot
/// (ADR-0016). `None` unless a typed question was the turn on the wire:
/// a poke or a proactive wake opened no question on this surface, so a
/// notice there would answer nobody.
fn cancelled_caret(chat_turn: bool, by: &Happened) -> Option<ChatReply> {
    chat_turn.then(|| ChatReply {
        said: None,
        busy: false,
        reacting_to: None,
        you: false,
        thought: false,
        at: None,
        error: None,
        failure: None,
        superseded_by: Some(happened_cell(by)),
        streaming: false,
    })
}

/// Spatial Layer state one Chat surface draws in its status bar.
/// Compared field by field to decide whether to push, so nothing in here
/// changes on a tick where the bar would not.
#[derive(Clone, PartialEq, Serialize)]
struct ChatStatus {
    /// The Behavior playing, and `None` for the Engine's own moments — a Land
    /// or a startle no Director proposed. The bar draws a dash, as the trace does.
    behavior: Option<String>,
    primitive: Option<Primitive>,
    animation: &'static str,
    state: State,
    /// What drove the last wake, in `happened_cell`'s word for it.
    happened: Option<&'static str>,
    /// -1 heading left, 1 heading right. The heading itself, and not
    /// `SpritePlacement::mirror`'s answer about the art (#345): the bar says
    /// which way the sprite is walking, which a left strip does not change.
    facing: i8,
    /// A turn is on the wire, and the character is not waiting on the user.
    /// The thinking ellipsis draws from this.
    thinking: bool,
}

#[derive(Clone, Serialize)]
struct ChatStatusPush<'a> {
    #[serde(flatten)]
    status: &'a ChatStatus,
    /// Milliseconds until the next proactive wake, or `None` when none is
    /// coming. A deadline pushed once rather than a number every second: the
    /// window counts it down itself.
    wake_ms: Option<u64>,
}

/// A line the user typed, on its way in. Bounded here, at `CHAT_LIMIT`:
/// this is where webview text enters, and the session keeps the line, so
/// cutting it later would still have paid for the whole paste.
#[tauri::command]
fn chat_send(
    instance: String,
    text: String,
    echo: Option<bool>,
    chat: tauri::State<'_, ChatChannel>,
) {
    let text: String = text
        .trim()
        .chars()
        .take(fidget_core::director::CHAT_LIMIT)
        .collect();
    if text.is_empty() {
        return;
    }
    let _ = chat.0.send(ChatMsg::Said(ChatLine {
        instance,
        text,
        echo: echo.unwrap_or(false),
    }));
}

/// A Chat surface reporting that it is listening. Events only reach windows
/// that existed, so this session's turns wait here too, Speech as well as
/// permission asks that arrived before this window existed.
#[tauri::command]
fn chat_ready(
    instance: String,
    app: tauri::AppHandle,
    chat: tauri::State<'_, ChatChannel>,
    pending: tauri::State<'_, PendingAsks>,
) {
    if let Ok(pending) = pending.0.lock() {
        let restored = session_log::restored(&app, &instance);
        if !restored.is_empty() {
            let _ = app.emit_to(chat_label(&instance), CHAT_RESTORED_EVENT, &restored);
        }
        for turn in session_log::replay(&app, &instance) {
            let _ = app.emit_to(
                chat_label(&instance),
                CHAT_EVENT,
                ChatReply {
                    said: turn.said,
                    busy: false,
                    reacting_to: turn.reacting_to,
                    you: turn.who == session_log::Who::You,
                    thought: turn.who == session_log::Who::Thinking,
                    error: None,
                    failure: None,
                    superseded_by: None,
                    streaming: false,
                    at: Some(
                        turn.at
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                    ),
                },
            );
        }
        let label = chat_label(&instance);
        let (asks, forms) = pending.drawn_in(&label);
        for ask in asks {
            let _ = app.emit_to(&label, CHAT_PERMISSION_EVENT, ask);
        }
        for form in forms {
            let _ = app.emit_to(&label, CHAT_ELICITATION_EVENT, form);
        }
    }
    let _ = chat.0.send(ChatMsg::Listening(instance));
}

/// What each `overlay-{n}` should be covering after a display change.
///
/// `Inactive` is an overlay that outlived its display. It stays built and
/// hidden rather than closed: closing a webview from the main-thread block
/// that runs during display reconfiguration tears down WebKit mid-recalculation
/// and the Objective-C exception crosses an `extern "C"` frame, which Rust
/// cannot catch and aborts the process. #868.
#[derive(Clone, Copy, Debug, PartialEq)]
enum OverlayTarget {
    Display(Rect),
    Inactive,
}

/// One target per label in use, which is one per display plus every overlay
/// left over from a larger arrangement.
fn overlay_targets(displays: &[Rect], existing: usize) -> Vec<OverlayTarget> {
    (0..displays.len().max(existing))
        .map(|index| {
            displays
                .get(index)
                .copied()
                .map_or(OverlayTarget::Inactive, OverlayTarget::Display)
        })
        .collect()
}

/// One overlay per display. A spanning window is invisible off its Space, so
/// a seam needs both overlays. Idempotent as displays move; every display is
/// attempted even after one fails, or the rest of the desktop would go blank.
fn place_overlays(app: &tauri::AppHandle, displays: &[Rect]) -> Result<(), String> {
    let mut failed = Vec::new();
    // Probed, not remembered: an overlay that lost its display is hidden and
    // still there, and it is the one this has to find again. Labels are handed
    // out in order, so the first missing one ends the set.
    let existing = (0..)
        .take_while(|index| app.get_webview_window(&overlay_label(*index)).is_some())
        .count();

    for (index, target) in overlay_targets(displays, existing).into_iter().enumerate() {
        let label = overlay_label(index);
        let placed = match (app.get_webview_window(&label), target) {
            // Shown, not just covered: this is the path a returning display
            // takes, and `build_overlay` shows for the same reason.
            (Some(window), OverlayTarget::Display(display)) => cover_display(&window, display)
                .and_then(|()| window.show())
                .map_err(|why| why.to_string()),
            (None, OverlayTarget::Display(display)) => {
                build_overlay(app, &label, display).map_err(|why| why.to_string())
            }
            (Some(window), OverlayTarget::Inactive) => {
                eprintln!("overlay: {label} has no display left to cover");
                window.hide().map_err(|why| why.to_string())
            }
            (None, OverlayTarget::Inactive) => Ok(()),
        };
        if let Err(why) = placed {
            failed.push(format!("{label}: {why}"));
        }
    }

    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

/// The hide-hotkey Shortcut. Three modifiers, because a global shortcut is
/// taken from every application on the machine and B alone belongs to most
/// of them.
fn shortcut_from_spec(spec: &str) -> Option<Shortcut> {
    let parsed = settings::parse_hotkey(spec)
        .or_else(|| settings::parse_hotkey(settings::DEFAULT_HIDE_HOTKEY))?;
    let code: Code = settings::key_code_name(parsed.key)?.parse().ok()?;
    let mut modifiers = Modifiers::empty();
    if parsed.control {
        modifiers |= Modifiers::CONTROL;
    }
    if parsed.option {
        modifiers |= Modifiers::ALT;
    }
    if parsed.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if parsed.command {
        modifiers |= Modifiers::SUPER;
    }
    Some(Shortcut::new(Some(modifiers), code))
}

fn install_hide_hotkey(
    app: &tauri::AppHandle,
    rules: Arc<Mutex<HideRules>>,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
) -> bool {
    let plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |_app, _shortcut, event| {
            // Pressed only. The handler is called again on release, and a
            // toggle that ran twice would hand the Character back before the
            // user had let go of the key.
            if event.state() == ShortcutState::Pressed {
                if let (Ok(mut rules), Ok(mut guard)) = (rules.lock(), settings.lock()) {
                    settings::toggle_away(&mut rules, &mut guard);
                    drop(guard);
                    drop(rules);
                    persist_settings(&settings, &settings_path);
                }
            }
        })
        .build();
    if let Err(why) = app.plugin(plugin) {
        eprintln!("hotkey: unavailable, so the Character cannot be hidden by hand: {why}");
        false
    } else {
        true
    }
}

/// Bind the hide hotkey. A hotkey another application already holds is
/// reported and let go: losing it costs one way to hide the Character, which
/// is not worth losing the Character over.
fn bind_hide_hotkey(app: &tauri::AppHandle, spec: &str) {
    if app
        .try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>()
        .is_none()
    {
        return;
    }
    let Some(shortcut) = shortcut_from_spec(spec) else {
        eprintln!("hotkey: {spec} could not be parsed");
        return;
    };
    let gs = app.global_shortcut();
    if let Err(why) = gs.unregister_all() {
        eprintln!("hotkey: could not drop the previous binding: {why}");
    }
    if let Err(why) = gs.register(shortcut) {
        eprintln!(
            "hotkey: {spec} is unavailable, so the Character cannot be hidden by hand: {why}"
        );
    }
}

fn check_for_update(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_updater::UpdaterExt;
        let updater = match app.updater() {
            Ok(updater) => updater,
            Err(why) => {
                eprintln!("updater: {why}");
                return;
            }
        };
        match updater.check().await {
            Ok(Some(update)) => {
                eprintln!("updater: {} available, downloading", update.version);
                if let Err(why) = update.download_and_install(|_, _| {}, || {}).await {
                    eprintln!("updater: {why}");
                }
            }
            Ok(None) => eprintln!("updater: up to date"),
            Err(why) => eprintln!("updater: {why}"),
        }
    });
}

/// Move every Instance onto the display under `cursor`. One already there stays.
fn bring_roster_to_display(
    roster: &mut Roster,
    widths: &[(InstanceId, f64)],
    monitors: &[Rect],
    floors: &[Rect],
    cursor: Point,
) {
    stand_roster(roster, widths, |feet, widths| {
        fidget_core::engine::bring_landings(feet, widths, monitors, floors, cursor)
            .unwrap_or_default()
    });
}

/// Stand each Instance named in `widths` where `plan` lands it. `plan` gets
/// their feet and widths in that order and answers at the same indexes.
fn stand_roster(
    roster: &mut Roster,
    widths: &[(InstanceId, f64)],
    plan: impl FnOnce(&[Point], &[f64]) -> Vec<Option<Point>>,
) {
    let mut ids = Vec::new();
    let mut feet = Vec::new();
    let mut spans = Vec::new();
    for (id, width) in widths {
        if let Some(instance) = roster.get(id) {
            ids.push(id);
            feet.push(instance.feet());
            spans.push(*width);
        }
    }
    for (id, landing) in ids.into_iter().zip(plan(&feet, &spans)) {
        if let (Some(at), Some(instance)) = (landing, roster.get_mut(id)) {
            instance.stand_at(at);
        }
    }
}

// One over the clippy cap. Settings, hide rules, and the Director each have
// to hear the same click, and folding them would mix persist with proposal.
#[allow(clippy::too_many_arguments)]
fn apply_menu_action(
    action: menu::MenuAction,
    roster: &mut Roster,
    lives: &mut Vec<InstanceState>,
    slots: &mut completer::Slots,
    instance_id: &InstanceId,
    rules: &Arc<Mutex<HideRules>>,
    settings: &Arc<Mutex<Settings>>,
    settings_path: &std::path::Path,
    characters: &BTreeMap<String, Arc<Character>>,
    config: &mut model::DirectorConfig,
    director: &model::DirectorSettings,
    inspect: &Arc<Mutex<model::DirectorInspect>>,
    app: &tauri::AppHandle,
    cursor: Point,
    monitors: &[Rect],
    floors: &[Rect],
) {
    match action {
        menu::MenuAction::SwitchCharacter(name) => {
            if let Some(character) = characters.get(&name).cloned() {
                switch_instance(
                    roster,
                    lives,
                    slots,
                    instance_id,
                    character,
                    config,
                    director,
                    app,
                );
                let (chat_ui, chat_appearance) = settings
                    .lock()
                    .ok()
                    .map(|s| (s.chat_ui.clone(), s.chat_appearance))
                    .unwrap_or_else(|| ("minimal".to_string(), ChatAppearance::System));
                if let Ok(inspect) = inspect.lock() {
                    push_chat_opening(
                        app,
                        roster,
                        instance_id,
                        &inspect,
                        characters,
                        &chat_ui,
                        chat_appearance,
                    );
                }
                if let Ok(mut guard) = settings.lock() {
                    guard.character = name.clone();
                    drop(guard);
                    persist_settings(settings, settings_path);
                }
                eprintln!("menu: switching to {name}");
            } else {
                eprintln!("menu: no Character named {name}");
            }
        }
        menu::MenuAction::SpawnInstance => {
            let character_name = settings
                .lock()
                .ok()
                .map(|s| s.character.clone())
                .filter(|name| !name.is_empty())
                .or_else(|| lives.first().map(|live| live.character.name.clone()));
            if let Some(name) = character_name {
                spawn_live(
                    roster,
                    lives,
                    characters,
                    &name,
                    name.clone(),
                    config,
                    director,
                );
            }
        }
        menu::MenuAction::ToggleDirector => {
            let enabled = if let Ok(mut guard) = settings.lock() {
                guard.director_enabled = !guard.director_enabled;
                let chat_ui = guard.chat_ui.clone();
                let chat_appearance = guard.chat_appearance;
                config.apply_switch(guard.director_enabled);
                if let Ok(mut inspect) = inspect.lock() {
                    inspect.enabled = config.enabled;
                    push_chat_openings(
                        app,
                        roster,
                        &inspect,
                        characters,
                        &chat_ui,
                        chat_appearance,
                    );
                }
                Some(guard.director_enabled)
            } else {
                None
            };
            if let Some(enabled) = enabled {
                persist_settings(settings, settings_path);
                eprintln!("menu: Director {}", if enabled { "on" } else { "off" });
            }
        }
        menu::MenuAction::ToggleDnd => {
            if let Some(instance) = roster.get_mut(instance_id) {
                let new_state = !instance.do_not_disturb();
                instance.set_do_not_disturb(new_state);
                if let Ok(mut guard) = settings.lock() {
                    guard.do_not_disturb = new_state;
                    drop(guard);
                    persist_settings(settings, settings_path);
                }
                eprintln!("menu: DND {}", if new_state { "on" } else { "off" });
            }
        }
        menu::MenuAction::Hide => {
            if let Ok(mut r) = rules.lock() {
                let saved = if let Ok(mut guard) = settings.lock() {
                    settings::toggle_away(&mut r, &mut guard);
                    true
                } else {
                    false
                };
                let away = r.is_away();
                drop(r);
                if saved {
                    persist_settings(settings, settings_path);
                }
                eprintln!("menu: {}", if away { "away" } else { "back" });
            }
        }
        menu::MenuAction::BringToThisDisplay => {
            // Every Instance, not the one the menu was opened on. The row is
            // about the display under the cursor, and a buddy left on the
            // other monitor is the one the user still cannot see.
            let widths: Vec<(InstanceId, f64)> = lives
                .iter()
                .map(|live| (live.id.clone(), sprite_width(&live.character)))
                .collect();
            bring_roster_to_display(roster, &widths, monitors, floors, cursor);
            eprintln!("menu: bring to this display");
        }
        menu::MenuAction::ToggleFullscreenHide => {
            if let Ok(mut r) = rules.lock() {
                let next = !r.hide_in_fullscreen();
                r.set_hide_in_fullscreen(next);
                let saved = if let Ok(mut guard) = settings.lock() {
                    guard.hide_in_fullscreen = r.hide_in_fullscreen();
                    true
                } else {
                    false
                };
                drop(r);
                if saved {
                    persist_settings(settings, settings_path);
                }
            }
        }
        menu::MenuAction::OpenMemory => {
            let _ = platform::open_path(&memory::shared_path());
        }
        menu::MenuAction::OpenActionLog => {
            let _ = platform::open_path(&action_log::current_path());
        }
        menu::MenuAction::OpenSettings => present_settings(app.clone()),
        menu::MenuAction::Summon => {
            if let Some(instance) = roster.get(instance_id) {
                let title = instance.name.clone();
                open_chat(app, instance_id, title, Some(instance.feet()));
                eprintln!("menu: Summon");
            }
        }
        menu::MenuAction::Quit => quit_now(),
    }
}

/// Leave without AppKit's `terminate:`. `PredefinedMenuItem::quit` deadlocks
/// overlay webviews from inside the tray tracking run loop; `process::exit`
/// skips that path and the window server drops the overlays with the process.
fn quit_now() -> ! {
    eprintln!("quit");
    harness::shutdown();
    std::process::exit(0);
}

/// One step of a console interrupt. The order is the behavior: WebView
/// `Close` has to run on the UI thread before that thread calls `exit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuitAction {
    Announce,
    ShutdownHarness,
    CloseWebviews,
    ExitEventLoop,
    ExitProcess,
}

/// What a console interrupt does, in order. Windows closes each WebView on
/// the UI thread before the event loop ends. `process::exit` from the
/// console handler kills that thread first, so `Chrome_WidgetWin_0` unregisters as 1411.
fn quit_actions(orderly_platform: bool, already_quitting: bool) -> &'static [QuitAction] {
    if already_quitting {
        &[QuitAction::ExitProcess]
    } else if orderly_platform {
        &[
            QuitAction::Announce,
            QuitAction::ShutdownHarness,
            QuitAction::CloseWebviews,
            QuitAction::ExitEventLoop,
        ]
    } else {
        &[
            QuitAction::Announce,
            QuitAction::ShutdownHarness,
            QuitAction::ExitProcess,
        ]
    }
}

/// Ctrl+C is not `RunEvent::Exit`. The Harness child is in its own process
/// group so that signal does not dump inside Node; this then kills it.
/// Isolation waits until the handler is installed, or a failed catch leaves a tree Ctrl+C cannot reap.
fn quit_harness_on_interrupt(app: tauri::AppHandle) {
    match ctrlc::set_handler(move || {
        let already = crate::harness::interrupt_already_quitting();
        for action in quit_actions(cfg!(windows), already) {
            perform_quit_action(*action, &mut AppQuit(&app));
        }
    }) {
        Ok(()) => crate::harness::own_interrupt(),
        Err(why) => eprintln!(
            "harness: could not catch interrupt: {why}; child stays in this process group"
        ),
    }
}

/// What [`perform_quit_action`] asks the host to do. Tests record the calls.
/// `close_webview` is `Webview::close`. `close_window` exists only in tests,
/// so a recording can show Ctrl+C did not take that path.
trait QuitHost {
    fn announce(&mut self);
    fn shutdown_harness(&mut self);
    fn close_webview(&mut self);
    #[cfg(test)]
    fn close_window(&mut self);
    fn exit_event_loop(&mut self);
    fn exit_process(&mut self);
}

fn perform_quit_action(action: QuitAction, host: &mut impl QuitHost) {
    match action {
        QuitAction::Announce => host.announce(),
        QuitAction::ShutdownHarness => host.shutdown_harness(),
        QuitAction::CloseWebviews => host.close_webview(),
        QuitAction::ExitEventLoop => host.exit_event_loop(),
        QuitAction::ExitProcess => host.exit_process(),
    }
}

struct AppQuit<'a>(&'a tauri::AppHandle);

impl QuitHost for AppQuit<'_> {
    fn announce(&mut self) {
        eprintln!("quit");
    }

    fn shutdown_harness(&mut self) {
        crate::harness::shutdown();
    }

    fn close_webview(&mut self) {
        close_webviews_for_quit(self.0);
    }

    #[cfg(test)]
    fn close_window(&mut self) {
        // Not what Ctrl+C calls. Window close destroys the parent HWND
        // before `Controller::Close`, which is the 1411 path.
        for (label, window) in self.0.webview_windows() {
            if let Err(why) = window.close() {
                eprintln!("quit: {label} window could not be closed: {why}");
            }
        }
    }

    fn exit_event_loop(&mut self) {
        self.0.exit(0);
    }

    fn exit_process(&mut self) {
        std::process::exit(0);
    }
}

/// Posted before [`tauri::AppHandle::exit`], so the loop runs `Close` on the
/// UI thread and only then stops. Dropping the WebView calls
/// `ICoreWebView2Controller::Close` before the parent HWND goes away.
fn close_webviews_for_quit(app: &tauri::AppHandle) {
    for (label, window) in app.webview_windows() {
        // `Webview::close`, not the window. The window close destroys the
        // parent HWND first; the webview close runs `Controller::Close` first.
        if let Err(why) = tauri::Webview::close(window.as_ref()) {
            eprintln!("quit: {label} could not be closed: {why}");
        }
    }
}

/// One Instance's wake clock: config interval grown at the Character's rate.
/// The two halves come from different places every time, so the pairing is
/// written once rather than at each of the four sites that builds a `Pace`.
pub(crate) fn paced(config: &model::DirectorConfig, character: &Character) -> Pace {
    Pace::with_growth(
        config.ambient_first,
        character.model_base,
        character.model_power,
    )
}

// One over the clippy cap, for the same reason `apply_menu_action` is: the new
// session belongs beside the `retarget_model` that opens it, and the Chat
// surface it has to tell is reached through the app handle.
#[allow(clippy::too_many_arguments)]
fn switch_instance(
    roster: &mut Roster,
    lives: &mut [InstanceState],
    slots: &mut completer::Slots,
    instance_id: &InstanceId,
    character: Arc<Character>,
    config: &model::DirectorConfig,
    settings: &model::DirectorSettings,
    app: &tauri::AppHandle,
) {
    roster.retarget(instance_id, &character);
    if let Some(live) = lives.iter_mut().find(|live| live.id == *instance_id) {
        live.character = character;
        live.director = StaticDirector::new(live.character.behaviors.clone(), 0);
        live.pace = paced(config, &live.character);
        // The old session is the previous Character's. A Wake still on the
        // wire would propose as them; drop it and ask for this opening turn.
        completer::retarget_model(
            slots,
            instance_id,
            &mut live.model,
            live.character.behaviors.keys().cloned(),
            live.character.name.clone(),
            settings,
            config.configured,
        );
        session_log::new_session(app, instance_id, "the Character changed");
        live.recent.clear();
        live.happened = Happened::Proactive;
        live.addressed = true;
    }
}

fn spawn_live(
    roster: &mut Roster,
    lives: &mut Vec<InstanceState>,
    characters: &BTreeMap<String, Arc<Character>>,
    character_name: &str,
    instance_name: String,
    config: &model::DirectorConfig,
    settings: &model::DirectorSettings,
) {
    let Some(character) = characters.get(character_name).cloned() else {
        eprintln!("menu: no Character named {character_name}");
        return;
    };
    let start = lives
        .last()
        .and_then(|live| live.drawn_last.as_ref())
        .map(|drawn| Point {
            x: f64::from(drawn.rect.x) + sprite_width(&character) + 16.0,
            y: f64::from(drawn.rect.y),
        })
        .unwrap_or(Point { x: 80.0, y: 80.0 });
    let name = if instance_name.is_empty() {
        character.name.clone()
    } else {
        instance_name
    };
    let id = roster.spawn(&character, name, start);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    lives.push(InstanceState {
        id: id.clone(),
        director: StaticDirector::new(character.behaviors.clone(), seed),
        model: config.configured.then(|| {
            Arc::new(ModelDirector::new(
                completer::completer_from(settings).expect("configured means a Completer exists"),
                character.behaviors.keys().cloned(),
                id.clone(),
                character.name.clone(),
                model::blank(),
            ))
        }),
        recent: Vec::new(),
        recency: Recency::default(),
        pace: paced(config, &character),
        since_wake: Duration::ZERO,
        since_state: Duration::ZERO,
        since_proactive: Duration::ZERO,
        previous_idle: Duration::MAX,
        last_state: None,
        last_position: start,
        addressed: false,
        happened: Happened::Proactive,
        chat_turn: false,
        pointer: Pointer::with_double_click_ms(platform::double_click_interval_ms()),
        spoken: None,
        speech: SpeechBubble::default(),
        drawn_last: None,
        qm: None,
        traced_last: None,
        status_last: None,
        status_wake_ms: None,
        happened_last: None,
        verbs: Vec::new(),
        menu_hold: None,
        character,
    });
}

/// The menu bar icon. Held so a toggle on the frame-loop thread can rebuild
/// the menu on the main thread, where the native objects live.
struct TrayHandle(Mutex<Option<tauri::tray::TrayIcon>>);

struct FrameExtras {
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    characters: BTreeMap<String, Arc<Character>>,
    instances: Arc<Mutex<Vec<InstanceRow>>>,
    ops: mpsc::Receiver<SettingsOp>,
    chat: mpsc::Receiver<ChatMsg>,
    /// `tools/call`s from the loopback MCP server, which are dispatched on the
    /// frame-loop thread because that is where the `Roster` is (ADR-0023).
    mcp: mpsc::Receiver<mcp_http::Call>,
}

fn publish_instances(roster: &Roster, dest: &Arc<Mutex<Vec<InstanceRow>>>) {
    if let Ok(mut rows) = dest.lock() {
        *rows = roster
            .list()
            .into_iter()
            .map(|(id, name)| {
                let instance = roster.get(&id);
                InstanceRow {
                    id,
                    name,
                    character: instance
                        .map(|instance| instance.character_name().to_string())
                        .unwrap_or_default(),
                    // The Chat surface asks for this through `chat_opening`,
                    // which sees the roster only through these rows.
                    prompt: instance
                        .map(|instance| instance.prompt().to_string())
                        .unwrap_or_default(),
                }
            })
            .collect();
    }
}

fn describe_menu(
    installed: &[String],
    current: &str,
    roster: &Roster,
    instance_id: &str,
    settings: &Settings,
    rules: &HideRules,
) -> menu::MenuDescription {
    let instances = roster.list();
    let hide_hotkey = settings::display_hotkey(&settings.hide_hotkey);
    menu::describe(menu::MenuSnapshot {
        installed,
        current_character: current,
        instances: &instances,
        director_enabled: model::director_in_force(settings.director_enabled),
        director_env_owned: model::env_switch(model::ENABLED).is_some(),
        do_not_disturb: roster
            .get(instance_id)
            .map(|instance| instance.do_not_disturb())
            .unwrap_or(false),
        hidden: rules.is_away(),
        hide_in_fullscreen: rules.hide_in_fullscreen(),
        hide_hotkey: &hide_hotkey,
    })
}

/// The environment variable naming the Instances to run. An env var rather
/// than a flag because that is how fidget is already configured, and a
/// second mechanism for the same kind of answer is a second place to look it up.
const INSTANCES_VAR: &str = "FIDGET_INSTANCES";

/// Which Instances the launch configuration asks for. The environment wins
/// when a developer set it; otherwise settings. Empty is still first-run:
/// `load_instances` turns it into the one character the app has always run.
fn requested_instances(settings: &Settings) -> Result<Vec<InstanceSpec>, String> {
    match std::env::var(INSTANCES_VAR) {
        Ok(raw) => roster::parse_specs(&raw),
        Err(_) if !settings.instances.is_empty() => Ok(settings.instances.clone()),
        Err(_) => Ok(Vec::new()),
    }
}

fn load_all_characters(
    app: &tauri::AppHandle,
) -> (
    BTreeMap<String, CharacterArt>,
    BTreeMap<String, Arc<Character>>,
) {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join(BUNDLED_CHARACTERS));
    let search_paths = package::search_paths(bundled);
    let mut art = BTreeMap::new();
    let mut cache = BTreeMap::new();
    for path in package::installed(&search_paths) {
        let files = match package::read(&path) {
            Ok(files) => files,
            Err(_) => continue,
        };
        if let Ok(character) = fidget_core::character::load(&files) {
            // Earlier directories win, so a package the user added is the one a switch loads.
            if cache.contains_key(&character.name) {
                continue;
            }
            art.insert(
                character.name.clone(),
                CharacterArt {
                    art: art_urls(&character),
                    smooth: character.smooth,
                },
            );
            cache.insert(character.name.clone(), Arc::new(character));
        }
    }
    (art, cache)
}

/// Load the Character each requested Instance names, sharing one load
/// among namesakes. None asked still runs the one character the app has always
/// run; an Instance with no name of its own takes the Character's.
fn load_instances(
    app: &tauri::AppHandle,
    wanted: &[InstanceSpec],
    settings: &Settings,
) -> Result<Vec<(InstanceSpec, Arc<Character>)>, String> {
    if wanted.is_empty() {
        let wanted = std::env::var_os(package::CHARACTER_VAR).or_else(|| {
            (!settings.character.is_empty()).then(|| std::ffi::OsString::from(&settings.character))
        });
        let character = Arc::new(load_named(app, wanted)?);
        let name = character.name.clone();
        return Ok(vec![(
            InstanceSpec::fresh(character.name.clone(), name),
            character,
        )]);
    }

    let mut loaded: BTreeMap<String, Arc<Character>> = BTreeMap::new();
    let mut instances = Vec::with_capacity(wanted.len());

    for spec in wanted {
        let character = match loaded.get(&spec.character) {
            Some(character) => Arc::clone(character),
            None => {
                let character = Arc::new(load_named(
                    app,
                    Some(std::ffi::OsString::from(&spec.character)),
                )?);
                loaded.insert(spec.character.clone(), Arc::clone(&character));
                character
            }
        };

        let name = if spec.name.is_empty() {
            character.name.clone()
        } else {
            spec.name.clone()
        };
        instances.push((
            InstanceSpec {
                character: spec.character.clone(),
                name,
                // Carried rather than dropped: these two are how the Instance
                // that ran last time is the Instance that runs now (ADR-0012).
                id: spec.id.clone(),
                prompt: spec.prompt.clone(),
            },
            character,
        ));
    }

    Ok(instances)
}

/// A lone leftover default `{ character: "Timber Wolf", name: "buddy-bot" }` takes
/// this Character's name before the overlay log prints it and before spawn
/// persists it. Several Instances keep the names that tell them apart.
fn follow_lone_default(
    loaded: &mut [(InstanceSpec, Arc<Character>)],
    known: &BTreeMap<String, Arc<Character>>,
) {
    if loaded.len() != 1 {
        return;
    }
    let names: Vec<&str> = known.keys().map(String::as_str).collect();
    let (spec, character) = &mut loaded[0];
    spec.name = roster::adopted_name(&spec.name, &character.name, names);
}

/// Spawn every requested Instance into a Roster, and build the Shell state
/// each keeps beside its Engine. Memory is one file for every Instance:
/// `Roster` holds it behind an `Arc` so a second character already knows the user.
fn spawn_instances(
    loaded: &[(InstanceSpec, Arc<Character>)],
    start: Point,
    config: &model::DirectorConfig,
    settings: &model::DirectorSettings,
    known_names: impl IntoIterator<Item = String>,
) -> (Roster, Vec<InstanceState>) {
    let mut roster = Roster::new();
    roster.set_known_names(known_names);
    let mut lives = Vec::with_capacity(loaded.len());

    // The wall clock, so two runs are not the same afternoon, the one thing
    // the Engine's purity forbids. Mixed with the Instance's place in the
    // list: same-nanosecond Instances would otherwise share a seed and lockstep.
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);

    // Where each Instance's wake clock starts. Drawn from the same launch
    // seed rather than the clock again: `as_nanos` three times in a row
    // differs in low bits only, and would put every character within a millisecond.
    let mut phases = Seeded::new(seed);

    let widths: Vec<f64> = loaded
        .iter()
        .map(|(_, character)| sprite_width(character))
        .collect();
    let positions = starting_positions(start, &widths);

    for (index, (spec, character)) in loaded.iter().enumerate() {
        let id = roster.restore(
            character,
            spec.name.clone(),
            positions[index],
            spec.id.clone(),
            spec.prompt.clone(),
        );

        lives.push(InstanceState {
            id: id.clone(),
            character: Arc::clone(character),
            director: StaticDirector::new(character.behaviors.clone(), seed ^ index as u64),
            // ponytail: N Instances with a key make N times the model calls, on
            // N independent `Pace` clocks and against no shared budget. Fine for
            // the handful a desktop holds; the cap belongs in `Slots`, which is
            // the one thing that sees every Instance, and it wants somewhere to
            // show the spend, which is #18's panel.
            model: config.configured.then(|| {
                Arc::new(ModelDirector::new(
                    completer::completer_from(settings)
                        .expect("configured means a Completer exists"),
                    character.behaviors.keys().cloned(),
                    id.clone(),
                    character.name.clone(),
                    model::blank(),
                ))
            }),
            recent: Vec::new(),
            recency: Recency::default(),
            pace: paced(config, character),
            // Started somewhere inside the interval rather than at nothing, so
            // N characters do not all decide on the same tick. Deciding together
            // still reads as coordinated and puts N model calls in one instant.
            since_wake: phase_of(config.wake_every, phases.draw()),
            since_state: Duration::ZERO,
            since_proactive: Duration::ZERO,
            previous_idle: Duration::MAX,
            last_state: None,
            last_position: positions[index],
            addressed: false,
            happened: Happened::Proactive,
            chat_turn: false,
            pointer: Pointer::with_double_click_ms(platform::double_click_interval_ms()),
            spoken: None,
            speech: SpeechBubble::default(),
            drawn_last: None,
            qm: None,
            traced_last: None,
            status_last: None,
            status_wake_ms: None,
            happened_last: None,
            verbs: Vec::new(),
            menu_hold: None,
        });
    }

    (roster, lives)
}

/// How far through the wake interval an Instance's clock starts. A draw
/// each, never an even spread (that is its own mechanical lockstep), and
/// never the whole interval. Randomness is injected so the arithmetic is testable.
fn phase_of(interval: Duration, draw: u64) -> Duration {
    let millis = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
    if millis == 0 {
        return Duration::ZERO;
    }
    Duration::from_millis(draw % millis)
}

/// How wide a Character's sprite usually is, in points. The idle Animation's
/// frame, blown up by scale. Animations may declare different frame sizes,
/// so this is usual rather than always.
fn sprite_width(character: &Character) -> f64 {
    character
        .draw("idle", 0, 0, 1.0)
        .map_or(0.0, |drawn| f64::from(drawn.frame_size.0))
        * f64::from(character.scale)
}

/// Where each Instance comes into the world. Several dropped on one point
/// would land in a stack; each is placed the previous sprite's width past
/// it. Nothing clamps here; far enough out, the Engine's walls stop them.
fn starting_positions(start: Point, widths: &[f64]) -> Vec<Point> {
    let mut x = start.x;
    widths
        .iter()
        .map(|width| {
            let at = Point { x, y: start.y };
            x += width;
            at
        })
        .collect()
}

/// The folder stem (`trump`) or the Character name (`Trump`) both name a package.
fn names_the_package(path: &Path, character_name: &str, wanted: &OsStr) -> bool {
    path.file_stem() == Some(wanted) || OsStr::new(character_name) == wanted
}

/// The Character an Instance asked for: the first package that loads.
/// Every rejection is reported; finding none stops startup and names every
/// directory searched. Takes the name rather than reading the env itself.
fn load_named(
    app: &tauri::AppHandle,
    wanted: Option<std::ffi::OsString>,
) -> Result<Character, String> {
    // The shipped Characters are an app resource, which `tauri-build` copies
    // next to the binary for `cargo run` as well as into a bundle.
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join(BUNDLED_CHARACTERS));

    let search_paths = package::search_paths(bundled);
    let installed = package::installed(&search_paths);
    let candidates = match &wanted {
        Some(name) => {
            // Folder stem first (`trump`). A switch persists Character.name
            // (`Trump`); that is not a stem, so fall through to every package
            // and match after load.
            let by_stem = package::named(installed.clone(), Some(name));
            if by_stem.is_empty() {
                installed
            } else {
                by_stem
            }
        }
        None => package::preferring(installed, package::DEFAULT_CHARACTER),
    };

    for candidate in &candidates {
        let files = match package::read(candidate) {
            Ok(files) => files,
            Err(package::ReadError::NotAPackage(_)) => continue,
            Err(why) => {
                eprintln!("character: {why}");
                continue;
            }
        };

        match fidget_core::character::load(&files) {
            Ok(character) => {
                if let Some(wanted) = &wanted {
                    if !names_the_package(candidate, &character.name, wanted) {
                        continue;
                    }
                }
                eprintln!("character: {} from {}", character.name, candidate.display());
                return Ok(character);
            }
            // A rejection like any other: one broken package should not cost
            // the user every Character behind it in the search.
            Err(errors) => eprintln!(
                "character: {} is not a valid Character Package:\n  - {}",
                candidate.display(),
                errors.join("\n  - ")
            ),
        }
    }

    let looked_in = search_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    // Which of the two failed matters: a Character that is not installed is a
    // typo in the name, and every Character failing to load is a broken build.
    Err(match wanted {
        Some(wanted) => format!(
            "no Character Package named {} loaded. Fidget looked in: {looked_in}",
            wanted.to_string_lossy()
        ),
        None => format!("no Character Package loaded. Fidget looked in: {looked_in}"),
    })
}

/// Off-screen park, in pixels.
///
/// Win32 still carries some positions in a signed 16-bit. -32000 stays
/// inside that range, so a truncated coordinate cannot wrap onto the
/// desktop, and a 200x200 window there misses a normal virtual screen.
#[cfg(not(target_os = "macos"))]
const ANCHOR_PARK: (i32, i32) = (-32_000, -32_000);

#[cfg(not(target_os = "macos"))]
fn anchor_origin(requested: (i32, i32), locked: bool) -> (i32, i32) {
    if locked {
        requested
    } else {
        ANCHOR_PARK
    }
}

#[cfg(any(all(test, not(target_os = "macos")), target_os = "windows"))]
fn anchor_position_locked(flags: u32, nomove: u32) -> bool {
    (flags & nomove) != 0
}

/// Taskbar button that opens Settings. Main thread only. A webview here
/// would be a second WebKit process beside the overlay, and it draws nothing.
#[cfg(target_os = "linux")]
fn build_anchor_window(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use gtk::prelude::*;

    if std::env::var("FIDGET_NO_ANCHOR").is_ok() {
        eprintln!("anchor: skipped (FIDGET_NO_ANCHOR is set)");
        return Ok(());
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Fidget");
    window.set_resizable(false);
    window.set_decorated(false);
    window.set_accept_focus(true);
    // A mapped window takes focus, and focus opens Settings. A launch must not.
    window.set_focus_on_map(false);
    window.set_skip_taskbar_hint(false);
    // GTK's unset size is 200×200, which would sit on the desktop. The button
    // is the point; the window itself stays one pixel. Opacity covers a window
    // manager that refuses the off-screen park and would otherwise leave a pixel.
    window.set_size_request(1, 1);
    window.set_default_size(1, 1);
    window.set_opacity(0.0);
    if let Ok(icon) = gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(
        include_bytes!("../icons/icon.png").as_slice(),
    )) {
        window.set_icon(Some(&icon));
    }

    let app_handle = app.clone();
    window.connect_focus_in_event(move |_, _| {
        // `show_settings` is async for the Chat webview. This callback is the
        // shell thread, and dropping that future would never open Settings.
        present_settings(app_handle.clone());
        gtk::glib::Propagation::Proceed
    });

    let (x, y) = anchor_origin((0, 0), false);
    window.show_all();
    window.resize(1, 1);
    // Drop destroys the button. It has to live as long as the process.
    let anchor: &'static gtk::Window = Box::leak(Box::new(window));
    // move_ before the loop runs does not stick. Ask again once it is mapped.
    gtk::glib::idle_add_local_once(move || {
        anchor.resize(1, 1);
        anchor.move_(x, y);
        let (placed_x, placed_y) = anchor.position();
        eprintln!("anchor: 1x1 at ({placed_x},{placed_y})");
    });
    Ok(())
}

/// Taskbar button that opens Settings. Main thread only. A webview here
/// would be a second WebView2 process beside the overlay, and it draws nothing.
#[cfg(target_os = "windows")]
fn build_anchor_window(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, GetWindowRect, LoadCursorW, RegisterClassExW, SendMessageW, SetWindowPos,
        ShowWindow, ICON_BIG, ICON_SMALL, IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNA,
        WM_SETICON, WNDCLASSEXW, WS_EX_APPWINDOW, WS_POPUP,
    };

    if std::env::var("FIDGET_NO_ANCHOR").is_ok() {
        eprintln!("anchor: skipped (FIDGET_NO_ANCHOR is set)");
        return Ok(());
    }

    // Set before CreateWindowExW. The class procedure can run before that call returns.
    ANCHOR_APP.set(app.clone()).ok();

    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if instance.is_null() {
        return Err(format!("anchor: module: {}", std::io::Error::last_os_error()).into());
    }

    let icon = anchor_icon().unwrap_or(std::ptr::null_mut());
    let class_name = windows_sys::w!("FidgetAnchor");
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(anchor_wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: icon,
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name,
        hIconSm: icon,
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        let err = std::io::Error::last_os_error();
        const ERROR_CLASS_ALREADY_EXISTS: i32 = 1410;
        if err.raw_os_error() != Some(ERROR_CLASS_ALREADY_EXISTS) {
            return Err(format!("anchor: register class: {}", err).into());
        }
    }

    let (x, y) = anchor_origin((0, 0), false);
    // The shell omits a popup from the taskbar. WS_EX_APPWINDOW puts it there.
    // WS_EX_TOOLWINDOW and an owner window would take it off again.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name,
            windows_sys::w!("Fidget"),
            WS_POPUP,
            x,
            y,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err(format!("anchor: create window: {}", std::io::Error::last_os_error()).into());
    }

    if !icon.is_null() {
        unsafe {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon as isize);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon as isize);
        }
    }

    // SW_SHOW would take focus at launch. The button has to appear without that.
    unsafe {
        ShowWindow(hwnd, SW_SHOWNA);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            1,
            1,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }

    let mut rect = RECT {
        left: x,
        top: y,
        right: x + 1,
        bottom: y + 1,
    };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        rect.left = x;
        rect.top = y;
    }
    eprintln!("anchor: 1x1 at ({},{})", rect.left, rect.top);
    // Win32 does not destroy a window when this handle goes out of scope.
    // DestroyWindow would remove the taskbar button.
    Ok(())
}

/// Low word of `WM_ACTIVATE`. A click is `WA_CLICKACTIVE`. A show is `WA_ACTIVE`,
/// and a launch must not open Settings.
#[cfg(target_os = "windows")]
const fn anchor_click_opens_settings(active: u16) -> bool {
    active == windows_sys::Win32::UI::WindowsAndMessaging::WA_CLICKACTIVE as u16
}

#[cfg(target_os = "windows")]
static ANCHOR_APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

#[cfg(target_os = "windows")]
unsafe extern "system" fn anchor_wndproc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_ACTIVATE, WM_CLOSE,
        WM_WINDOWPOSCHANGING,
    };

    if msg == WM_CLOSE {
        quit_now();
    }
    if msg == WM_WINDOWPOSCHANGING {
        let pos = lparam as *mut WINDOWPOS;
        if !pos.is_null() {
            let flags = (*pos).flags;
            let nomove = anchor_position_locked(flags, SWP_NOMOVE);
            let requested = ((*pos).x, (*pos).y);
            let (x, y) = anchor_origin(requested, nomove);
            (*pos).x = x;
            (*pos).y = y;
            // A taskbar restore can grow the window onto the desktop.
            if !anchor_position_locked(flags, SWP_NOSIZE) {
                (*pos).cx = 1;
                (*pos).cy = 1;
            }
        }
    }
    if msg == WM_ACTIVATE && anchor_click_opens_settings((wparam & 0xFFFF) as u16) {
        if let Some(app) = ANCHOR_APP.get() {
            present_settings(app.clone());
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Taskbar icon from the same PNG the bundle uses. The class stores the handle
/// until process exit, and `DestroyIcon` would blank the button.
#[cfg(target_os = "windows")]
fn anchor_icon() -> Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON> {
    use windows_sys::Win32::UI::WindowsAndMessaging::CreateIcon;

    let decoded = match image::load_from_memory(include_bytes!("../icons/icon.png")) {
        Ok(decoded) => decoded,
        Err(why) => {
            eprintln!("anchor: icon: {why}");
            return None;
        }
    };
    // The PNG is 512. The shell's jumbo slot is 256, and a larger icon is only memory.
    let rgba = decoded.thumbnail(256, 256).to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut pixels = rgba.into_raw();
    let mut mask = Vec::with_capacity(pixels.len() / 4);
    // CreateIcon's color plane is BGRA. The mask byte is inverted alpha so a
    // transparent pixel stays transparent on the taskbar.
    for chunk in pixels.as_chunks_mut::<4>().0 {
        mask.push(chunk[3].wrapping_sub(u8::MAX));
        chunk.swap(0, 2);
    }
    let icon = unsafe {
        CreateIcon(
            std::ptr::null_mut(),
            width as i32,
            height as i32,
            1,
            32,
            mask.as_ptr(),
            pixels.as_ptr(),
        )
    };
    if icon.is_null() {
        eprintln!("anchor: icon: {}", std::io::Error::last_os_error());
        None
    } else {
        Some(icon)
    }
}

fn main() {
    fidget::process_log::init();

    // Same Completer, no overlay. scripts/probe-model.sh is the face of this.
    if std::env::args().any(|arg| arg == "--probe-model") {
        std::process::exit(model::run_probe());
    }

    // The same, one hop further out: the Harness attached and one turn run.
    // scripts/probe-harness.sh is the face of this.
    if std::env::args().any(|arg| arg == "--probe-harness") {
        std::process::exit(harness::run_probe());
    }

    // This process *is* the stdio MCP server: never the overlay. The Harness
    // child is a new process, so it does not share the shell's ACP runtime.
    if std::env::args().any(|arg| arg == "--mcp-stdio") {
        fidget_mcp_server::run();
        return;
    }

    // Before the builder, because the builder is where GTK initializes and GDK
    // reads GDK_BACKEND once, when it opens the display. A no-op off Linux.
    platform::prefer_x11_backend();

    // Before the builder, so the first Harness probe and spawn see it. Windows
    // reads PATH from the registry and needs none of this.
    #[cfg(unix)]
    login_path::adopt();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            character,
            overlay_primary,
            overlay_secondary,
            overlay_composing,
            overlay_report_qm_draft,
            overlay_rects,
            overlay_trace_bubble,
            overlay_hit_tests_hotspots,
            overlay_traces_cadence,
            overlay_cadence,
            overlay_open_chat,
            overlay_request_focus,
            chat_opening,
            chat_send,
            chat_prompt,
            chat_ready,
            names_hint_act,
            permission_answer,
            elicitation_answer,
            open_link,
            select_harness,
            sign_in,
            show_settings,
            settings_snapshot,
            settings_event
        ])
        .setup(|app| {
            // No Character means no overlay. Reported and exited rather than
            // returned as a setup error: Tauri turns that into a panic the
            // event loop cannot unwind, burying the one line under a backtrace.
            let settings_file = settings::settings_path(&memory::data_dir());
            let mut settings = Settings::load(&settings_file);
            // Before anything reads a development switch: the frame loop and
            // the overlay panel load them from `dev_flags`, not the env.
            dev_flags::seed(&settings);
            #[cfg(not(target_os = "linux"))]
            consent::set_wanted(
                consent::CapabilityId::Accessibility,
                settings.use_accessibility,
            );
            consent::set_wanted(
                consent::CapabilityId::WindowNames,
                settings.use_window_names,
            );
            #[cfg(target_os = "macos")]
            consent::set_wanted(
                consent::CapabilityId::InputMonitoring,
                settings.use_input_monitoring,
            );
            let wanted = requested_instances(&settings).unwrap_or_else(|why| {
                fidget::eprintln_and_log!("instances: {why}");
                std::process::exit(1);
            });
            let mut loaded = load_instances(&app.handle().clone(), &wanted, &settings)
                .unwrap_or_else(|why| {
                    fidget::eprintln_and_log!("character: {why}");
                    std::process::exit(1);
                });

            // Every installed package's art, so a switch or a spawn does not
            // have to wait for a reload the overlay never does.
            let (art, character_cache) = load_all_characters(&app.handle().clone());
            follow_lone_default(&mut loaded, &character_cache);
            let mut characters = art;
            for (_, character) in &loaded {
                characters
                    .entry(character.name.clone())
                    .or_insert_with(|| CharacterArt {
                        art: art_urls(character),
                        smooth: character.smooth,
                    });
            }
            app.manage(ArtUrls { characters });
            let installed: Vec<String> = character_cache.keys().cloned().collect();

            // Show in the Dock so users have a findable anchor when the menu
            // bar is crowded. The tray icon remains the settings door.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            // On Windows and Linux, create an off-screen anchor window that appears
            // in the taskbar/panel, matching the macOS Dock presence.
            // Clicking it opens Settings. Overlays stay off the taskbar.
            // Held until the quit handler is installed, so a Ctrl+C in the
            // gap between webviews cannot take the default ExitProcess path.
            // Each build nests its own hold for a WebView created later.
            #[cfg(windows)]
            let spawned_ctrl_c = platform::SpawnedCtrlC::hold();
            #[cfg(not(target_os = "macos"))]
            build_anchor_window(app.handle())?;

            // Read before the overlays are built rather than after the loop
            // starts: reading which part of a display is usable means asking
            // AppKit, and only the main thread may do that.
            let (source, displays) = platform::window_source(app.handle().clone());
            let start = starting_position(&source.snapshot());

            // Which Dock the physics got. Printed because the difference is
            // invisible until a sprite walks past the Dock's real end.
            if cfg!(target_os = "macos") {
                match displays.read().dock {
                    Some((dock, source)) => fidget::eprintln_and_log!(
                        "dock: true bounds via {source:?}, {}x{} at {},{}",
                        dock.width,
                        dock.height,
                        dock.x,
                        dock.y
                    ),
                    None => fidget::eprintln_and_log!(
                        "dock: full-width floor; no source reported a bottom Dock — \
                         a side or hidden Dock has nothing to report, and granting \
                         Fidget Accessibility only helps where one exists"
                    ),
                }
            }

            // One overlay per display, so a Character straddling a seam is
            // drawn whole. The frame loop keeps the set in step with a desktop
            // that gains or loses a display.
            let covered = displays.read().frames;
            if covered.is_empty() {
                return Err("no displays reported".into());
            }
            place_overlays(app.handle(), &covered)?;

            // The sprite size is the first Instance's idle Animation, blown
            // up. Here because scripts/verify-overlay.sh crops a screenshot
            // to it, and that script runs one Instance.
            let (sprite_width, sprite_height) = loaded
                .first()
                .and_then(|(_, character)| {
                    let scale = character.scale as i32;
                    character.draw("idle", 0, 0, 1.0).map(|drawn| {
                        (
                            drawn.frame_size.0 as i32 * scale,
                            drawn.frame_size.1 as i32 * scale,
                        )
                    })
                })
                .unwrap_or((0, 0));

            fidget::eprintln_and_log!(
                "overlay: {} display(s); sprite {}x{}; {}",
                covered.len(),
                sprite_width,
                sprite_height,
                loaded
                    .iter()
                    .map(|(spec, character)| format!("{} as {}", character.name, spec.name))
                    .collect::<Vec<_>>()
                    .join(", "),
            );

            // Shared because the hotkey and the frame loop each see half of the
            // answer: the key is pressed on the main thread and the desktop is
            // read on the loop's.
            let mut hide_rules = HideRules::default();
            hide_rules.set_away(settings.hidden);
            hide_rules.set_hide_in_fullscreen(settings.hide_in_fullscreen);
            let rules = Arc::new(Mutex::new(hide_rules));

            if let Err(why) = app
                .handle()
                .plugin(tauri_plugin_updater::Builder::new().build())
            {
                fidget::eprintln_and_log!("updater: {why}");
            } else if !cfg!(debug_assertions) {
                // A success downloads and installs a GitHub release over this
                // process; `cargo run` is a debug binary that must not be
                // replaced that way.
                check_for_update(app.handle().clone());
            }

            let secrets: Arc<dyn SecretStore> = Arc::new(KeyringStore::new());
            // Before `director_settings` and `config_from`, both of which ask
            // whether a Harness is attached. Resolving the key first is what
            // made a Harness launch prompt for one it would never send (#290).
            app.manage(PendingAsks(Mutex::new(Pending::default())));
            app.manage(MinimizedChats::default());
            app.manage(Mutex::new(session_log::Log::new()));
            // Before `attach`, because `open_session` reads the endpoint to
            // decide what to put in `session/new`'s `mcpServers` and the
            // preflight thread can reach that within a tick of this line.
            let (mcp_tx, mcp_rx) = mpsc::channel();
            mcp_resources::publish_excluded(&settings.excluded_applications);
            mcp_http::serve(mcp_tx);
            let forward_to = app.handle().clone();
            // WebViews already exist, so the browser inherited the ignore bit.
            // Turning Ctrl+C back on does not reach that child. The handler
            // has to be in before the Harness leaves this process group.
            #[cfg(windows)]
            drop(spawned_ctrl_c);
            quit_harness_on_interrupt(app.handle().clone());
            harness::attach(
                harness::Target::from_settings(
                    settings.harness_source().as_deref(),
                    &settings.harness_cwd,
                ),
                Box::new(move |forwarded| match forwarded {
                    harness::Forwarded::Ask { owner, ask } => forward_ask(&forward_to, owner, ask),
                    harness::Forwarded::Form { owner, form } => {
                        forward_form(&forward_to, owner, form)
                    }
                    harness::Forwarded::Settled { request, option } => {
                        settle_ask(&forward_to, Settled { request, option })
                    }
                    harness::Forwarded::InboundWake(wake) => handle_inbound_wake(&forward_to, wake),
                    harness::Forwarded::Restored(restored) => {
                        restore_history(&forward_to, restored)
                    }
                    harness::Forwarded::Thought { instance, line } => {
                        show_thought(&forward_to, &instance, line)
                    }
                    harness::Forwarded::Plan { instance, steps } => {
                        show_plan(&forward_to, &instance, &steps)
                    }
                    harness::Forwarded::AttachSettled => {
                        if let Some(state) = forward_to.try_state::<SettingsState>() {
                            let _ = state.ops.send(SettingsOp::ReloadChat);
                        }
                    }
                    harness::Forwarded::NotApplied(failures) => {
                        restore_not_applied(&forward_to, &failures)
                    }
                }),
            );
            // The other lane's thoughts, through the same door. Only one lane
            // completes at a time — a Harness attached is the Completer
            // (ADR-0008) — so the strip is never written by both.
            let thought_to = app.handle().clone();
            model::on_thought(Box::new(move |instance, line| {
                show_thought(&thought_to, instance, line)
            }));
            let director = match settings::director_settings(&settings, secrets.as_ref()) {
                Ok(director) => director,
                Err(why) => {
                    eprintln!("director: secret store: {why}");
                    model::resolve(&settings.director_base_url, &settings.director_model, None)
                }
            };
            let mut config = model::config_from(&director);
            config.apply_switch(settings.director_enabled);
            config.proactive_allowed = settings.proactive_wakes;
            let inspect = Arc::new(Mutex::new(config.inspect(&director)));
            app.manage(Arc::clone(&inspect));
            for line in model::env_switch_warnings(&dev_flags::switch_vars()) {
                eprintln!("{line}");
            }
            for line in model::startup_lines(&config) {
                eprintln!("{line}");
            }
            if config.enabled {
                model::spawn_preflight(&director);
            }

            let (mut roster, lives) = spawn_instances(
                &loaded,
                start,
                &config,
                &director,
                character_cache.keys().cloned(),
            );
            if settings.do_not_disturb {
                for (id, _) in roster.list() {
                    if let Some(instance) = roster.get_mut(&id) {
                        instance.set_do_not_disturb(true);
                    }
                }
            }
            if settings.character.is_empty() {
                if let Some((_, character)) = loaded.first() {
                    settings.character = character.name.clone();
                }
            }
            // Persist the roster's names, not the specs spawn loaded: a leftover
            // `{ character: "Timber Wolf", name: "bmo" }` would write itself
            // back after spawn had already adopted the Character's name.
            if !settings.instances.is_empty() {
                settings.instances = roster_specs(&roster);
            }
            if let Err(why) = settings::write_settings_file(&settings, &settings_file) {
                eprintln!("settings: {why}");
            }

            let settings = Arc::new(Mutex::new(settings));
            if install_hide_hotkey(
                app.handle(),
                Arc::clone(&rules),
                Arc::clone(&settings),
                settings_file.clone(),
            ) {
                let spec = settings
                    .lock()
                    .ok()
                    .map(|s| s.hide_hotkey.clone())
                    .unwrap_or_default();
                bind_hide_hotkey(app.handle(), &spec);
            }
            let instance_rows = Arc::new(Mutex::new(Vec::new()));
            let (ops_tx, ops_rx) = mpsc::channel();
            let (chat_tx, chat_rx) = mpsc::channel();
            app.manage(ChatChannel(chat_tx));
            app.manage(SettingsState {
                settings: Arc::clone(&settings),
                path: settings_file.clone(),
                memory_path: memory::shared_path(),
                installed,
                personalities: Arc::new(
                    character_cache
                        .iter()
                        .map(|(name, character)| (name.clone(), character.personality.clone()))
                        .collect(),
                ),
                behavior_names: Arc::new(
                    character_cache
                        .iter()
                        .map(|(name, character)| {
                            (name.clone(), character.behaviors.keys().cloned().collect())
                        })
                        .collect(),
                ),
                instances: Arc::clone(&instance_rows),
                inspect: Arc::clone(&inspect),
                ops: ops_tx,
                rules: Arc::clone(&rules),
                secrets: Arc::clone(&secrets),
                reveal: Mutex::new(None),
                settings_built: AtomicU64::new(0),
                settings_loaded: AtomicU64::new(0),
                loading_since: Mutex::new(None),
                rebuild_after_destroy: AtomicBool::new(false),
            });
            app.manage(Arc::clone(&rules));

            // Dev/test hook: open settings immediately if FIDGET_OPEN_SETTINGS=1.
            // For verify/smoke scripts that need the settings window on launch.
            if model::env_switch("FIDGET_OPEN_SETTINGS").unwrap_or(false) {
                present_settings(app.handle().clone());
            }

            let tray = {
                let installed: Vec<String> = character_cache.keys().cloned().collect();
                let current = lives
                    .first()
                    .map(|live| live.character.name.clone())
                    .unwrap_or_default();
                let id = lives
                    .first()
                    .map(|live| live.id.clone())
                    .unwrap_or_default();
                let settings_now = settings.lock().ok().map(|s| s.clone()).unwrap_or_default();
                let rules_now = rules.lock().ok();
                let description = describe_menu(
                    &installed,
                    &current,
                    &roster,
                    &id,
                    &settings_now,
                    rules_now.as_deref().unwrap_or(&HideRules::default()),
                );
                #[cfg(target_os = "macos")]
                platform::seed_tray_position();
                match tray::install(app.handle(), &description, 0) {
                    Ok(icon) => Some(icon),
                    Err(why) => {
                        eprintln!("tray: {why}");
                        None
                    }
                }
            };
            app.manage(TrayHandle(Mutex::new(tray)));

            let director_run = DirectorRun {
                config,
                settings: director,
                inspect,
            };

            // Selections do not come back from the popup: it returns once the
            // menu is on screen, and the click arrives later on this channel.
            // The hook forwards ids to the frame loop, which knows the open menu.
            let (menu_sender, menu_receiver) = mpsc::channel();
            let hook_sender = menu_sender.clone();
            let quit_generation = Arc::new(AtomicU64::new(0));
            let live_quit = Arc::clone(&quit_generation);
            app.handle().on_menu_event(move |_app, event| {
                let id = event.id().0.clone();
                // Native Quit ids are per tray draw. A dismiss rebuilds the
                // tray and muda can click the item it just dropped; that id
                // is the previous draw's, so it must not call quit_now.
                if menu::is_live_quit(&id, live_quit.load(Ordering::SeqCst)) {
                    quit_now();
                }
                let _ = hook_sender.send(MenuSignal::Chose(id));
            });

            run_frame_loop(
                app.handle().clone(),
                roster,
                lives,
                source,
                displays,
                rules,
                covered,
                director_run,
                MenuChannel {
                    sender: menu_sender,
                    receiver: menu_receiver,
                    quit_generation,
                },
                FrameExtras {
                    settings,
                    settings_path: settings_file,
                    characters: character_cache,
                    instances: instance_rows,
                    ops: ops_rx,
                    chat: chat_rx,
                    mcp: mcp_rx,
                },
            );
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Fidget failed to start")
        // Every exit path ends here — window close, `ExitRequested`, the tray
        // Quit's `quit_now` aside — so the Harness child is never orphaned.
        .run(|_, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                harness::shutdown();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(request: &str, waits: bool) -> harness::ElicitationForm {
        harness::ElicitationForm {
            request: request.to_string(),
            message: "Authenticate with MCP server linear".to_string(),
            field: String::new(),
            options: Vec::new(),
            url: Some("https://example.test/oauth".to_string()),
            waits,
        }
    }

    /// Chat floats over the bubble while it has focus and drops to a normal
    /// level when it loses it. Minimizing also takes focus, so it drops too.
    #[test]
    fn chat_floats_only_while_focused() {
        let chats = MinimizedChats::default();
        let focused = |on| tauri::WindowEvent::Focused(on);
        assert_eq!(chats.note("chat-a", &focused(true), || false), Some(true));
        assert_eq!(chats.note("chat-a", &focused(false), || false), Some(false));
        assert_eq!(chats.note("chat-a", &focused(false), || true), Some(false));
        let resized = tauri::WindowEvent::Resized(tauri::PhysicalSize::new(420, 560));
        assert_eq!(
            chats.note("chat-a", &resized, || false),
            None,
            "a resize leaves the level"
        );
    }

    /// A minimized Chat lets the pill back. No event says "minimized": blur
    /// carries it on macOS, a resize to nothing on Windows.
    #[test]
    fn a_minimized_chat_stops_hiding_the_pill_until_it_comes_back() {
        let chats = MinimizedChats::default();
        chats.note("chat-a", &tauri::WindowEvent::Focused(false), || true);
        assert!(chats.hides("chat-a"));
        assert!(!chats.hides("chat-b"), "only the minimized Chat");
        chats.note("chat-a", &tauri::WindowEvent::Focused(true), || false);
        assert!(!chats.hides("chat-a"), "unminimized");

        let gone = tauri::WindowEvent::Resized(tauri::PhysicalSize::new(0, 0));
        chats.note("chat-a", &gone, || true);
        assert!(chats.hides("chat-a"));
        chats.note("chat-a", &tauri::WindowEvent::Destroyed, || true);
        assert!(!chats.hides("chat-a"), "a reopened Chat starts open");
    }

    /// A Chat minimized behind another app gets no window event. AppKit's
    /// miniaturize notification sets it directly.
    #[test]
    fn a_chat_minimized_without_focus_lets_the_pill_back() {
        let chats = MinimizedChats::default();
        chats.set("chat-a", true);
        assert!(chats.hides("chat-a"));
        chats.set("chat-a", false);
        assert!(!chats.hides("chat-a"), "deminiaturized");
    }

    fn owners() -> (Owner, Owner) {
        (
            Owner::Instance("buddy-a".to_string()),
            Owner::Instance("buddy-b".to_string()),
        )
    }

    fn ask(request: &str) -> harness::PermissionAsk {
        harness::PermissionAsk {
            request: request.to_string(),
            title: None,
            kind: None,
            content: Vec::new(),
            input: None,
            locations: Vec::new(),
            options: Vec::new(),
        }
    }

    /// Each owner's Chat opens once for its rows, however many arrive. B's
    /// row while A's is still pending opens B's Chat, because A's Chat does
    /// not draw it, and A's Chat opens again once its own rows settle.
    #[test]
    fn each_owners_chat_opens_once_while_its_asks_and_forms_are_pending() {
        let (a, b) = owners();
        let mut pending = Pending::default();
        assert!(pending.hold_ask(&a, &ask("1"), true, false));
        assert!(
            pending.hold_ask(&b, &ask("2"), true, false),
            "B's ask during A's opened no Chat"
        );
        assert!(
            !pending.hold_ask(&a, &ask("3"), true, false),
            "A's second ask opened A's Chat again"
        );
        assert!(
            !pending.hold_form(&b, &link("4", false), true, false),
            "B's form opened B's Chat again"
        );
        pending.settle("1");
        pending.settle("3");
        assert!(
            pending.hold_form(&a, &link("5", false), true, false),
            "A's Chat stayed opened after its last row settled"
        );
        assert!(
            !pending.hold_ask(&b, &ask("6"), true, false),
            "settling A's rows reopened B's"
        );
    }

    /// A Chat that opens after the rows arrived replays only its own Instance's
    /// ask and form, plus a sign-in link no Instance owns (#1422).
    #[test]
    fn a_chat_that_opens_later_replays_only_its_own_asks_and_forms() {
        let (a, _) = owners();
        let mut pending = Pending::default();
        pending.asks.push((a.clone(), ask("1")));
        pending.hold_form(&a, &link("2", false), false, false);
        pending.hold_form(&Owner::EveryChat, &link("3", false), false, false);
        let replay = |label: &str| {
            let (asks, forms) = pending.drawn_in(label);
            (
                asks.map(|ask| ask.request.clone()).collect::<Vec<_>>(),
                forms.map(|form| form.request.clone()).collect::<Vec<_>>(),
            )
        };
        assert_eq!(replay("chat-buddy-b"), (vec![], vec!["3".to_string()]));
        assert_eq!(
            replay("chat-buddy-a"),
            (
                vec!["1".to_string()],
                vec!["2".to_string(), "3".to_string()]
            )
        );
    }

    /// A link nobody asked for opens nothing, and the next Chat's replay,
    /// which reads `forms`, still draws it. A link that does not wait, a
    /// sign-in's or a tool call's, opens Chat once. Do Not Disturb holds it
    /// for replay, as it holds a permission ask.
    #[test]
    fn an_unsolicited_link_waits_for_the_next_chat_and_a_sign_in_link_opens_it() {
        let mut pending = Pending::default();
        assert!(!pending.hold_form(&Owner::EveryChat, &link("7", true), true, false));
        assert!(pending.opened.is_empty());
        assert_eq!(pending.forms.len(), 1);
        assert_eq!(pending.forms[0].1.request, "7");

        assert!(pending.hold_form(&Owner::EveryChat, &link("8", false), true, false));
        assert!(!pending.hold_form(&Owner::EveryChat, &link("9", false), true, false));
        assert_eq!(pending.forms.len(), 3);

        let mut quiet = Pending::default();
        assert!(!quiet.hold_form(&Owner::EveryChat, &link("10", false), true, true));
        assert!(quiet.opened.is_empty());
        assert_eq!(quiet.forms.len(), 1);
        assert!(!quiet.hold_form(&Owner::EveryChat, &link("11", false), false, false));
    }

    /// Sync commands run on the main thread. These two must stay async so a
    /// settings save or a keychain prompt cannot freeze Chat and the character.
    fn settings_event_returns_a_future(app: tauri::AppHandle, payload: SettingsEventPayload) {
        let fut = settings_event(app, payload);
        fn assert_send<T: Send>(_: &T) {}
        assert_send(&fut);
        let _: &dyn std::future::Future<Output = Result<SettingsEventResponse, String>> = &fut;
    }

    fn select_harness_returns_a_future(
        harness: String,
        instance: String,
        app: tauri::AppHandle,
        state: tauri::State<'_, SettingsState>,
    ) {
        let fut = select_harness(harness, instance, app, state);
        fn assert_send<T: Send>(_: &T) {}
        assert_send(&fut);
        let _: &dyn std::future::Future<Output = Result<String, String>> = &fut;
    }

    #[test]
    fn settings_and_harness_commands_leave_the_main_thread() {
        let _ = settings_event_returns_a_future as fn(_, _);
        let _ = select_harness_returns_a_future as fn(_, _, _, _);
    }

    use fidget_core::character::{
        Character, CursorReaction, PackageBytes, CHARACTER_MANIFEST_FILE, DEFAULT_MODEL_BASE,
        DEFAULT_MODEL_POWER, REQUIRED_ANIMATIONS,
    };

    fn stub_character(name: &str) -> Character {
        Character {
            name: name.to_string(),
            personality: String::new(),
            animations: BTreeMap::new(),
            behaviors: BTreeMap::new(),
            art: BTreeMap::new(),
            smooth: false,
            scale: 1,
            model_base: DEFAULT_MODEL_BASE,
            model_power: DEFAULT_MODEL_POWER,
            near_reaction: CursorReaction::default(),
            rush_reaction: CursorReaction::default(),
            source: None,
        }
    }

    /// A two-display Mac, the arrangement #868 was reported on.
    const PRIMARY: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };

    const SECOND: Rect = Rect {
        x: 1920.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
    };

    #[test]
    fn display_loss_keeps_the_unassigned_overlay_inactive() {
        assert_eq!(
            overlay_targets(&[PRIMARY], 2),
            vec![OverlayTarget::Display(PRIMARY), OverlayTarget::Inactive]
        );
    }

    /// Production change that would fail this: dropping the overlay that lost
    /// its display out of the plan, which is what closing it did. A returning
    /// display has to find the same label and cover it again.
    #[test]
    fn a_returning_display_takes_its_overlay_back() {
        assert_eq!(
            overlay_targets(&[PRIMARY, SECOND], 2),
            vec![
                OverlayTarget::Display(PRIMARY),
                OverlayTarget::Display(SECOND)
            ]
        );
    }

    /// Production change that would fail this: leaving `bmo` on a Timber Wolf
    /// spec. That is the leftover settings persist, and the overlay log reads
    /// these specs before spawn.
    #[test]
    fn follow_lone_default_renames_a_foreign_package_id() {
        let wolf = Arc::new(stub_character("Timber Wolf"));
        let bmo = Arc::new(stub_character("BMO"));
        let mut loaded = vec![(InstanceSpec::fresh("Timber Wolf", "bmo"), Arc::clone(&wolf))];
        let mut known = BTreeMap::new();
        known.insert("BMO".to_string(), bmo);
        known.insert("Timber Wolf".to_string(), Arc::clone(&wolf));

        follow_lone_default(&mut loaded, &known);

        assert_eq!(loaded[0].0.name, "Timber Wolf");
    }

    #[test]
    fn follow_lone_default_keeps_a_chosen_name() {
        let wolf = Arc::new(stub_character("Timber Wolf"));
        let mut loaded = vec![(InstanceSpec::fresh("Timber Wolf", "Pip"), Arc::clone(&wolf))];
        let mut known = BTreeMap::new();
        known.insert("BMO".to_string(), Arc::new(stub_character("BMO")));
        known.insert("Timber Wolf".to_string(), wolf);

        follow_lone_default(&mut loaded, &known);

        assert_eq!(loaded[0].0.name, "Pip");
    }

    fn stub_inspect() -> model::DirectorInspect {
        model::DirectorInspect {
            enabled: true,
            configured: true,
            proactive_wakes: true,
            wake_secs: 60,
            last_payload: None,
            harness: None,
            model: "gpt-4o-mini".to_string(),
            host: "api.openai.com".to_string(),
        }
    }

    /// Production change that would fail this: emitting the pre-switch name or
    /// Character, or stuffing `character.name` into both fields.
    #[test]
    fn chat_who_after_switch_uses_the_roster_name_and_character() {
        let mut roster = Roster::new();
        let first = stub_character("bmo");
        let second = stub_character("nim");
        let id = roster.spawn(&first, "bmo".to_string(), Point { x: 10.0, y: 20.0 });
        assert!(roster.retarget(&id, &second));
        let instance = roster.get(&id).expect("still there");

        let opening = chat_opening_from(instance, &stub_inspect(), "");
        assert_eq!(opening.name, "nim", "the payload name is the Instance's");
        assert_eq!(
            opening.character, "nim",
            "the payload Character is the Instance's"
        );
    }

    /// Production change that would fail this: opening Settings on WA_ACTIVE (1)
    /// or WA_INACTIVE (0) in addition to WA_CLICKACTIVE (2). #767.
    #[cfg(target_os = "windows")]
    mod windows_anchor_tests {
        #[test]
        fn clickactive_opens_settings() {
            const WA_CLICKACTIVE: u16 = 2;
            assert!(
                windows_activate_opens_settings(WA_CLICKACTIVE),
                "WA_CLICKACTIVE (2) should open Settings"
            );
        }

        #[test]
        fn active_does_not_open() {
            const WA_ACTIVE: u16 = 1;
            assert!(
                !windows_activate_opens_settings(WA_ACTIVE),
                "WA_ACTIVE (1) should not open Settings"
            );
        }

        #[test]
        fn inactive_does_not_open() {
            const WA_INACTIVE: u16 = 0;
            assert!(
                !windows_activate_opens_settings(WA_INACTIVE),
                "WA_INACTIVE (0) should not open Settings"
            );
        }

        fn windows_activate_opens_settings(f_active: u16) -> bool {
            crate::anchor_click_opens_settings(f_active)
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_move_parks_the_anchor_off_the_virtual_screen() {
        let (x, y) = anchor_origin((540, 260), false);
        let (wide, tall) = (200, 200);
        let span = 16_384;
        let misses_x = x + wide <= -span || x >= span;
        let misses_y = y + tall <= -span || y >= span;
        assert!(
            misses_x && misses_y,
            "anchor at {x},{y} size {wide}x{tall} still meets a screen inside +/-{span}"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_suppressed_move_leaves_the_anchor_where_it_is() {
        assert_eq!(anchor_origin((12, 34), true), (12, 34));
        assert!(anchor_position_locked(2 | 1, 2));
        assert!(anchor_position_locked(2, 2));
        assert!(!anchor_position_locked(1, 2));
        assert!(!anchor_position_locked(0, 2));
    }

    /// A chosen name survives retarget; the Chat header still has to name the
    /// new Character. Copying only `character.name` into both fields would fail.
    #[test]
    fn chat_who_after_switch_keeps_a_chosen_name() {
        let mut roster = Roster::new();
        let first = stub_character("bmo");
        let second = stub_character("nim");
        let id = roster.spawn(&first, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
        assert!(roster.retarget(&id, &second));
        let instance = roster.get(&id).expect("still there");

        let opening = chat_opening_from(instance, &stub_inspect(), "");
        assert_eq!(opening.name, "Pip");
        assert_eq!(opening.character, "nim");
    }

    /// Production change that would fail this: an opening whose enabled bit
    /// still matches the pre-toggle inspect. #473.
    #[test]
    fn chat_opening_from_inspect_carries_configured_and_enabled() {
        let mut roster = Roster::new();
        let character = stub_character("nim");
        let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
        let instance = roster.get(&id).expect("still there");
        let inspect = model::DirectorInspect {
            enabled: false,
            ..stub_inspect()
        };

        let opening = chat_opening_from(instance, &inspect, "");
        assert_eq!(opening.name, "Pip");
        assert_eq!(opening.character, "nim");
        assert!(opening.configured);
        assert!(!opening.enabled);
        assert!(opening.harness.is_none());
    }

    /// Production change that would fail this: an opening that says a Harness
    /// is there without carrying whether it is up, which is the difference
    /// between the header naming a mind and naming a hope (#474).
    #[test]
    fn chat_opening_carries_the_harness_facts_the_header_names() {
        let mut roster = Roster::new();
        let character = stub_character("nim");
        let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
        let instance = roster.get(&id).expect("still there");
        let inspect = model::DirectorInspect {
            harness: Some(crate::harness::HarnessInspect {
                name: "hermes".to_string(),
                session_id: Some("sess-7".to_string()),
                alive: false,
                ..Default::default()
            }),
            ..stub_inspect()
        };

        let harness = chat_opening_from(instance, &inspect, "")
            .harness
            .expect("the opening carries the attachment");
        assert_eq!(harness.name, "hermes");
        assert_eq!(harness.session.as_deref(), Some("sess-7"));
        assert!(!harness.alive, "a handle that never answered is not alive");
        assert_eq!(harness.login, None);
        assert_eq!(harness.missing, None);
    }

    /// #726: Settings already names a missing launcher. Chat has to carry
    /// the same fact or the header can only say `not running`.
    #[test]
    fn chat_opening_carries_a_missing_launcher() {
        let mut roster = Roster::new();
        let character = stub_character("nim");
        let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
        let instance = roster.get(&id).expect("still there");
        let inspect = model::DirectorInspect {
            harness: Some(crate::harness::HarnessInspect {
                name: "codex".to_string(),
                missing: Some("npx".to_string()),
                alive: false,
                ..Default::default()
            }),
            ..stub_inspect()
        };

        let harness = chat_opening_from(instance, &inspect, "")
            .harness
            .expect("the opening carries the attachment");
        assert_eq!(harness.name, "codex");
        assert_eq!(harness.missing.as_deref(), Some("npx"));
        assert_eq!(harness.install.as_deref(), Some("https://nodejs.org/"));
        assert!(!harness.alive);
    }

    /// A launcher that died at startup reaches Chat with its reason, or the
    /// landing can only say the Harness has not come up.
    #[test]
    fn chat_opening_carries_why_the_launcher_died() {
        let mut roster = Roster::new();
        let character = stub_character("nim");
        let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
        let instance = roster.get(&id).expect("still there");
        let why = crate::harness::LaunchFailure {
            command: Some("npx -y @agentclientprotocol/codex-acp@latest".to_string()),
            reason: "exited before initialize, signal: 6 (SIGABRT)".to_string(),
            output: "dyld[0]: Library not loaded".to_string(),
            node_check: Some("node --version".to_string()),
        };
        let inspect = model::DirectorInspect {
            harness: Some(crate::harness::HarnessInspect {
                name: "codex".to_string(),
                failed: Some(why.clone()),
                ..Default::default()
            }),
            ..stub_inspect()
        };

        let harness = chat_opening_from(instance, &inspect, "")
            .harness
            .expect("the opening carries the attachment");
        assert_eq!(harness.failed, Some(why));
    }

    /// A launcher that failed preflight reaches Chat with the sentence, or the
    /// landing can only say the Harness has not come up.
    #[test]
    fn chat_opening_carries_why_the_launcher_is_unhealthy() {
        let mut roster = Roster::new();
        let character = stub_character("nim");
        let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
        let instance = roster.get(&id).expect("still there");
        let inspect = model::DirectorInspect {
            harness: Some(crate::harness::HarnessInspect {
                name: "claude".to_string(),
                unhealthy: Some(crate::harness::LaunchFailure {
                    command: Some("npx --version".to_string()),
                    reason: "exited with exit status: 1".to_string(),
                    output: "npm ERR! code ENOENT".to_string(),
                    node_check: Some("node --version".to_string()),
                }),
                ..Default::default()
            }),
            ..stub_inspect()
        };

        let opening = serde_json::to_value(chat_opening_from(instance, &inspect, "")).unwrap();
        assert_eq!(
            opening["harness"]["unhealthy"],
            serde_json::json!({
                "command": "npx --version",
                "reason": "exited with exit status: 1",
                "output": "npm ERR! code ENOENT",
                "node_check": "node --version",
            })
        );
    }

    /// The landing payload while login is still required. The fragment is the
    /// button list, not a value recomputed while asserting.
    #[test]
    fn a_login_opening_carries_agent_sign_in_and_omits_it_otherwise() {
        use agent_client_protocol::schema::v1::{AuthMethod, AuthMethodAgent, AuthMethodTerminal};
        use std::collections::HashMap;

        let mut roster = Roster::new();
        let character = stub_character("nim");
        let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
        let instance = roster.get(&id).expect("still there");
        let inspect = model::DirectorInspect {
            harness: Some(crate::harness::HarnessInspect {
                name: "codex".to_string(),
                login: Some("codex login".to_string()),
                alive: true,
                ..Default::default()
            }),
            ..stub_inspect()
        };
        let mut env = HashMap::new();
        env.insert("TOKEN".to_string(), "super-secret".to_string());
        let offered = vec![
            AuthMethod::Agent(AuthMethodAgent::new("chatgpt", "ChatGPT")),
            AuthMethod::Terminal(
                AuthMethodTerminal::new("term", "Outside")
                    .args(vec!["--not-stored".to_string()])
                    .env(env),
            ),
        ];
        let json_of = |methods: Vec<AuthMethod>| {
            let mut found = String::new();
            crate::harness::with_sign_in_gate(Some("codex login"), methods, || {
                found = serde_json::to_string(&chat_opening_from(instance, &inspect, ""))
                    .expect("opening serializes");
            });
            found
        };
        let json = json_of(offered);
        assert!(
            json.contains(r#""sign_in":[{"id":"chatgpt","label":"ChatGPT"}]"#),
            "{json}"
        );
        assert!(json.contains(r#""login":"codex login""#), "{json}");
        assert!(!json.contains("super-secret"), "{json}");
        assert!(!json.contains("--not-stored"), "{json}");

        let terminal_only = json_of(vec![AuthMethod::Terminal(AuthMethodTerminal::new(
            "term", "Outside",
        ))]);
        assert!(
            terminal_only.contains(r#""login":"codex login""#),
            "{terminal_only}"
        );
        assert!(!terminal_only.contains("\"sign_in\""), "{terminal_only}");

        let empty = json_of(vec![]);
        assert!(empty.contains(r#""login":"codex login""#), "{empty}");
        assert!(!empty.contains("\"sign_in\""), "{empty}");
    }

    /// The HTTP half, and the rule that guards it. A credential is never
    /// drawn, and a base URL is where one hides in plain sight.
    #[test]
    fn chat_opening_names_the_endpoint_without_its_userinfo() {
        let mut roster = Roster::new();
        let character = stub_character("nim");
        let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
        let instance = roster.get(&id).expect("still there");
        let inspect = model::DirectorInspect {
            host: model::host_of("https://user:sk-secret@api.openai.com/v1"),
            ..stub_inspect()
        };

        let opening = chat_opening_from(instance, &inspect, "");
        assert_eq!(opening.model, "gpt-4o-mini");
        assert_eq!(opening.host, "api.openai.com");
    }

    /// ADR-0012: the Prompt tab draws the two authored layers, so the opening
    /// has to carry both, the Character's frozen and this Instance's own,
    /// and the bound the box has to stay inside. Sending the assembled Prompt would fail this.
    #[test]
    fn chat_opening_carries_both_authored_layers_and_the_bound() {
        let mut roster = Roster::new();
        let character = stub_character("nim");
        let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });

        let fresh = chat_opening_layers(
            roster.get(&id).expect("spawned"),
            &stub_inspect(),
            "Nim is patient.",
            ["wave"],
            "minimal",
            ChatAppearance::System,
        );
        assert_eq!(fresh.personality, "Nim is patient.");
        assert_eq!(fresh.instance_prompt, "", "empty by default");
        assert_eq!(fresh.prompt_limit, roster::INSTANCE_PROMPT_LIMIT);
        assert!(
            fresh
                .instructions
                .contains("You may propose one of these behaviors: wave"),
            "app-level instructions are the same string the opening turn sends: {}",
            fresh.instructions
        );

        assert!(roster.set_prompt(&id, "Answer in haiku.".to_string()));
        let written = chat_opening_from(
            roster.get(&id).expect("spawned"),
            &stub_inspect(),
            "Nim is patient.",
        );
        assert_eq!(written.instance_prompt, "Answer in haiku.");
        assert_eq!(
            written.personality, "Nim is patient.",
            "the author's layer stays the package's, frozen"
        );
    }

    /// #680: Blank AI empties the built-in Personality Prompt on the opening
    /// the tab draws. The Instance Prompt stays, so a control run can still
    /// iterate one.
    #[test]
    fn chat_opening_empties_personality_under_blank_ai() {
        crate::model::tests::with_env(None, None, None, || {
            let mut roster = Roster::new();
            let character = stub_character("nim");
            let id = roster.spawn(&character, "Pip".to_string(), Point { x: 10.0, y: 20.0 });
            assert!(roster.set_prompt(&id, "Answer in haiku.".to_string()));

            let off = chat_opening_layers(
                roster.get(&id).expect("spawned"),
                &stub_inspect(),
                "Nim is patient.",
                ["wave"],
                "minimal",
                ChatAppearance::System,
            );
            assert_eq!(off.personality, "Nim is patient.");
            assert_eq!(off.instance_prompt, "Answer in haiku.");
            assert!(
                off.instructions.contains("always in character"),
                "shaped openings still carry the app-level layer: {}",
                off.instructions
            );

            crate::dev_flags::seed(&settings::Settings {
                director_blank: true,
                ..settings::Settings::default()
            });
            let on = chat_opening_layers(
                roster.get(&id).expect("spawned"),
                &stub_inspect(),
                "Nim is patient.",
                ["wave"],
                "minimal",
                ChatAppearance::System,
            );
            assert_eq!(on.personality, "", "the built-in layer was emptied");
            assert_eq!(
                on.instructions, "",
                "app-level instructions were emptied with it"
            );
            assert_eq!(
                on.instance_prompt, "Answer in haiku.",
                "the Instance Prompt is still the one the user wrote"
            );
        });
    }

    /// #17: losing a line that is waiting in `happened` would answer a question
    /// the user never asked and drop the one they are waiting on.
    #[test]
    fn a_waiting_typed_line_survives_everything_else_that_happens() {
        let mut happened = Happened::Chat("what are you standing on?".to_string());
        note_happened(&mut happened, Happened::Poke);
        note_happened(&mut happened, Happened::Perch);

        assert_eq!(
            happened,
            Happened::Chat("what are you standing on?".to_string())
        );
    }

    #[test]
    fn with_nothing_waiting_the_latest_moment_wins() {
        let mut happened = Happened::Proactive;
        note_happened(&mut happened, Happened::Poke);
        note_happened(&mut happened, Happened::Throw);

        assert_eq!(happened, Happened::Throw);
    }

    /// Settings persist Character.name (`Trump`). The env var and the folder
    /// are still the package stem (`trump`). Either has to start the same fidget.
    #[test]
    fn a_package_answers_to_its_folder_or_its_character_name() {
        let folder = Path::new("/characters/trump");
        assert!(
            names_the_package(folder, "Trump", OsStr::new("trump")),
            "FIDGET_CHARACTER=trump still names the folder"
        );
        assert!(
            names_the_package(folder, "Trump", OsStr::new("Trump")),
            "settings.character after a switch is the Character name"
        );
        assert!(
            !names_the_package(Path::new("/characters/bmo"), "BMO", OsStr::new("Trump")),
            "some other package is not a match just because it loaded"
        );
    }

    /// A rebound hide hotkey must register the letter the user named, not B.
    #[test]
    fn a_rebound_spec_registers_its_letter() {
        let shortcut = shortcut_from_spec("Control-Shift-H").expect("parses");
        assert_eq!(shortcut.key, Code::KeyH);
        let shipped = shortcut_from_spec(settings::DEFAULT_HIDE_HOTKEY).expect("default");
        assert_eq!(shipped.key, Code::KeyB);
    }

    /// A 2x2 RGBA frame whose top-left pixel is transparent.
    const PATCHY: &[u8] = include_bytes!("../../crates/core/tests/fixtures/alpha-2x2.png");

    /// A 2x2 RGBA frame with every pixel drawn, so its URL is told apart from
    /// `PATCHY`'s.
    const SOLID: &[u8] = include_bytes!("../../crates/core/tests/fixtures/opaque-2x2.png");

    /// One Animation as these tests declare it: its name, then each frame as a
    /// file name and the bytes behind it.
    type Declared<'a> = (&'a str, &'a [(&'a str, &'a [u8])]);

    /// A Character whose Animations are `animations`, plus one frame each for
    /// every required Animation they do not name.
    fn character_declaring(animations: &[Declared<'_>]) -> Character {
        let mut manifest = String::from("name = \"Blip\"\n");
        let mut files = PackageBytes::new();

        let mut declare = |name: &str, frames: &[(&str, &[u8])]| {
            let names: Vec<String> = frames.iter().map(|(file, _)| format!("{file:?}")).collect();
            manifest.push_str(&format!(
                "[animations.{name}]\nframes = [{}]\n",
                names.join(", ")
            ));
            for (file, bytes) in frames {
                files.insert((*file).to_string(), bytes.to_vec());
            }
        };

        for required in REQUIRED_ANIMATIONS {
            if !animations.iter().any(|(name, _)| *name == required) {
                declare(required, &[(&format!("{required}.png"), PATCHY)]);
            }
        }
        for (name, frames) in animations {
            declare(name, frames);
        }

        files.insert(CHARACTER_MANIFEST_FILE.to_string(), manifest.into_bytes());
        fidget_core::character::load(&files).expect("the package is valid")
    }

    fn url(bytes: &[u8]) -> String {
        format!("data:image/png;base64,{}", STANDARD.encode(bytes))
    }

    /// The invariant `art_urls` exists to hold: the webview indexes this list
    /// by the index the frame loop computed over `Animation::frames`, so a
    /// dropped or reordered URL would put a different frame on screen.
    #[test]
    fn an_animations_urls_stand_in_the_order_its_frames_do() {
        let character = character_declaring(&[(
            "walk",
            &[("a.png", PATCHY), ("b.png", SOLID), ("c.png", PATCHY)],
        )]);
        let art = art_urls(&character);

        assert_eq!(art["walk"], vec![url(PATCHY), url(SOLID), url(PATCHY)]);
        assert_eq!(art["walk"].len(), character.animations["walk"].frames.len());
    }

    /// The frame two Animations share is encoded once and named twice, at each
    /// Animation's own index: a shared URL that only appeared once would shift
    /// every later frame of the second Animation.
    #[test]
    fn a_frame_two_animations_share_stands_at_each_animations_own_index() {
        let character = character_declaring(&[
            ("idle", &[("shared.png", PATCHY), ("bob.png", SOLID)]),
            ("sit", &[("down.png", SOLID), ("shared.png", PATCHY)]),
        ]);
        let art = art_urls(&character);

        assert_eq!(art["idle"], vec![url(PATCHY), url(SOLID)]);
        assert_eq!(art["sit"], vec![url(SOLID), url(PATCHY)]);
    }

    /// Every pre-Instances start path asks for no Instances, which
    /// `load_instances` turns into the one fidget it has always run. One test
    /// rather than three: they share an environment variable and would race.
    #[test]
    fn naming_no_instances_asks_for_none_and_a_list_is_read_in_full() {
        std::env::remove_var(INSTANCES_VAR);
        assert_eq!(
            requested_instances(&Settings::default()),
            Ok(Vec::new()),
            "the default single fidget is not a spec"
        );

        std::env::set_var(INSTANCES_VAR, "bmo:One,bmo:Two");
        let specs = requested_instances(&Settings::default()).expect("the list parses");
        assert_eq!(specs.len(), 2, "both Instances are asked for");
        assert!(specs.iter().all(|spec| spec.character == "bmo"));

        // A list that cannot be read stops startup rather than guessing.
        std::env::set_var(INSTANCES_VAR, "bmo:");
        assert!(requested_instances(&Settings::default()).is_err());

        std::env::remove_var(INSTANCES_VAR);

        let remembered = Settings {
            instances: vec![InstanceSpec::fresh("nim", "Nim")],
            ..Settings::default()
        };
        assert_eq!(
            requested_instances(&remembered).expect("settings list"),
            remembered.instances,
            "settings own the roster when the env is unset"
        );
    }

    /// The arithmetic that keeps fidgets from landing in a stack, and the reason
    /// it accumulates: stepping by each Character's own width puts a narrow
    /// sprite on top of the wide one it follows.
    #[test]
    fn each_instance_starts_a_sprites_width_past_the_one_before_it() {
        let start = Point { x: 100.0, y: 50.0 };

        assert_eq!(
            starting_positions(start, &[32.0, 32.0, 32.0]),
            vec![
                Point { x: 100.0, y: 50.0 },
                Point { x: 132.0, y: 50.0 },
                Point { x: 164.0, y: 50.0 },
            ]
        );

        // A wide Character followed by a narrow one: the gap is the width of the
        // sprite standing there, not the width of the one arriving.
        assert_eq!(
            starting_positions(start, &[128.0, 16.0, 16.0]),
            vec![
                Point { x: 100.0, y: 50.0 },
                Point { x: 228.0, y: 50.0 },
                Point { x: 244.0, y: 50.0 },
            ],
            "the narrow sprite clears the wide one"
        );

        assert_eq!(
            starting_positions(start, &[]),
            Vec::new(),
            "no Instances, no positions"
        );
        assert_eq!(
            starting_positions(start, &[64.0]),
            vec![start],
            "one fidget still comes into the world where it always did"
        );
    }

    /// A Summon on the right display opens Chat there, beside the sprite.
    #[test]
    fn chat_opens_on_the_display_the_sprite_stands_on() {
        let displays = [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            Rect {
                x: 1920.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
        ];
        let feet = Point {
            x: 2500.0,
            y: 1000.0,
        };
        assert_eq!(
            chat_origin(feet, &displays, CHAT_SIZE),
            Some((2596.0, 440.0))
        );
    }

    /// Near the right edge Chat flips to the sprite's left; no display, no answer.
    #[test]
    fn chat_stays_inside_the_sprites_display() {
        let display = Rect {
            x: 1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let feet = Point {
            x: 3800.0,
            y: 100.0,
        };
        assert_eq!(
            chat_origin(feet, &[display], CHAT_SIZE),
            Some((3800.0 - 96.0 - 420.0, 0.0))
        );
        assert_eq!(chat_origin(feet, &[], CHAT_SIZE), None);
    }

    /// Both buddies, not the one whose menu it was. The row is about the
    /// display. A second click finds them already there and does not move them.
    #[test]
    fn bring_to_this_display_puts_every_instance_on_the_cursor_monitor() {
        let character = stub_character("BMO");
        let mut roster = Roster::new();
        let left = roster.spawn(
            &character,
            "Left".to_string(),
            Point {
                x: 100.0,
                y: 1080.0,
            },
        );
        let right = roster.spawn(
            &character,
            "Right".to_string(),
            Point {
                x: 400.0,
                y: 1080.0,
            },
        );
        let widths = [(left.clone(), 128.0), (right.clone(), 128.0)];
        let cursor = Point {
            x: 2500.0,
            y: 400.0,
        };

        bring_roster_to_display(
            &mut roster,
            &widths,
            &[PRIMARY, SECOND],
            &[PRIMARY, SECOND],
            cursor,
        );

        assert_eq!(
            roster.get(&left).expect("left").feet(),
            Point {
                x: 2436.0,
                y: 982.0
            }
        );
        assert_eq!(
            roster.get(&right).expect("right").feet(),
            Point {
                x: 2564.0,
                y: 982.0
            }
        );

        bring_roster_to_display(
            &mut roster,
            &widths,
            &[PRIMARY, SECOND],
            &[PRIMARY, SECOND],
            cursor,
        );
        assert_eq!(
            roster.get(&left).expect("left").feet(),
            Point {
                x: 2436.0,
                y: 982.0
            },
            "a second bring does not hop"
        );
        assert_eq!(
            roster.get(&right).expect("right").feet(),
            Point {
                x: 2564.0,
                y: 982.0
            }
        );
    }

    /// Fullscreen on the second display, the frame loop's call. The free
    /// Instance lands on the primary and stays there on the next tick; the one
    /// in the user's hand is left out of `widths` and so is not moved.
    #[test]
    fn a_fullscreen_display_sends_all_but_a_held_instance_to_the_free_one() {
        let character = stub_character("BMO");
        let mut roster = Roster::new();
        let free = roster.spawn(
            &character,
            "Free".to_string(),
            Point {
                x: 2300.0,
                y: 982.0,
            },
        );
        let held_at = Point {
            x: 2800.0,
            y: 982.0,
        };
        let held = roster.spawn(&character, "Held".to_string(), held_at);
        let widths = [(free.clone(), 128.0)];
        let bring_off = |feet: &[Point], widths: &[f64]| {
            fidget_core::engine::bring_off_fullscreen(
                feet,
                widths,
                &[PRIMARY, SECOND],
                &[PRIMARY, SECOND],
                &fidget_core::visibility::Desktop {
                    fullscreen: vec![false, true],
                },
            )
        };

        stand_roster(&mut roster, &widths, bring_off);
        stand_roster(&mut roster, &widths, bring_off);

        assert_eq!(
            roster.get(&free).expect("free").feet(),
            Point {
                x: 960.0,
                y: 1080.0
            }
        );
        assert_eq!(roster.get(&held).expect("held").feet(), held_at);
    }

    #[test]
    fn bring_to_this_display_leaves_an_instance_already_on_that_monitor() {
        let character = stub_character("BMO");
        let mut roster = Roster::new();
        let home = Point {
            x: 2200.0,
            y: 982.0,
        };
        let staying = roster.spawn(&character, "Here".to_string(), home);
        let away = roster.spawn(
            &character,
            "There".to_string(),
            Point {
                x: 100.0,
                y: 1080.0,
            },
        );
        let widths = [(staying.clone(), 128.0), (away.clone(), 128.0)];

        bring_roster_to_display(
            &mut roster,
            &widths,
            &[PRIMARY, SECOND],
            &[PRIMARY, SECOND],
            Point {
                x: 2500.0,
                y: 400.0,
            },
        );

        assert_eq!(roster.get(&staying).expect("staying").feet(), home);
        assert_eq!(
            roster.get(&away).expect("away").feet(),
            Point {
                x: 2500.0,
                y: 982.0
            }
        );
    }

    /// A wake clock starts somewhere inside the interval, never past it.
    #[test]
    fn a_wake_clock_starts_somewhere_inside_the_interval() {
        let interval = Duration::from_secs(60);

        assert_eq!(phase_of(interval, 0), Duration::ZERO);
        assert_eq!(phase_of(interval, 1_500), Duration::from_millis(1_500));

        // A draw is a whole u64, so most of them are past the interval and wrap.
        assert_eq!(
            phase_of(interval, 60_000),
            Duration::ZERO,
            "a draw of exactly the interval wraps to the start of it"
        );
        assert_eq!(phase_of(interval, 61_234), Duration::from_millis(1_234));

        // Never already due: a phase equal to the interval would wake every
        // character on the first tick, which is the thing being avoided.
        for draw in [0, 1, u64::MAX / 2, u64::MAX] {
            assert!(
                phase_of(interval, draw) < interval,
                "draw {draw} lands inside the interval"
            );
        }

        // An interval of nothing cannot happen, and must not divide by zero.
        assert_eq!(phase_of(Duration::ZERO, u64::MAX), Duration::ZERO);
    }

    /// The property the randomness is for: fidgets from one launch start their
    /// clocks at different, unevenly spaced points.
    #[test]
    fn buddies_from_one_launch_start_their_clocks_apart() {
        let interval = Duration::from_secs(60);
        let mut draws = Seeded::new(0x5EED);

        let phases: Vec<Duration> = (0..4).map(|_| phase_of(interval, draws.draw())).collect();

        let mut distinct = phases.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            4,
            "no two fidgets wake together: {phases:?}"
        );

        // Uneven, which is what a draw buys over a share apiece: an even spread
        // would make every gap identical.
        let gaps: Vec<Duration> = distinct.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(
            gaps.windows(2).any(|pair| pair[0] != pair[1]),
            "the spacing is not a fixed step: {gaps:?}"
        );
    }

    /// #178: a line said on one display and carried across the seam is said
    /// again to the new owner, once, while it could still be showing — and
    /// never to the owner that already heard it.
    #[test]
    fn a_line_crosses_the_seam_with_the_sprite_once_and_only_while_fresh() {
        let t0 = Instant::now();
        let mut spoken = None;

        assert_eq!(
            carry_line(&mut spoken, Some("Yare yare daze."), Some(0), t0).as_deref(),
            Some("Yare yare daze."),
            "the pulse itself goes to the owner of the tick"
        );
        assert_eq!(
            carry_line(&mut spoken, None, Some(0), t0 + Duration::from_secs(1)),
            None,
            "the same owner is not told twice"
        );
        assert_eq!(
            carry_line(&mut spoken, None, Some(1), t0 + Duration::from_secs(2)).as_deref(),
            Some("Yare yare daze."),
            "mid-reading, the new owner is told the line"
        );
        assert_eq!(
            carry_line(&mut spoken, None, Some(1), t0 + Duration::from_secs(3)),
            None,
            "and then not again while it stays there"
        );
        assert_eq!(
            carry_line(
                &mut spoken,
                None,
                Some(0),
                t0 + CARRY_WINDOW + Duration::from_secs(1)
            ),
            None,
            "a line older than any reading window is not resurrected by a crossing"
        );

        let mut spoken = None;
        carry_line(&mut spoken, Some("first"), Some(0), t0);
        assert_eq!(
            carry_line(
                &mut spoken,
                Some("second"),
                Some(0),
                t0 + Duration::from_secs(1)
            )
            .as_deref(),
            Some("second"),
            "a new line replaces the remembered one"
        );
        assert_eq!(
            carry_line(&mut spoken, None, Some(1), t0 + Duration::from_secs(2)).as_deref(),
            Some("second"),
            "and it is the new line that crosses"
        );
    }

    #[test]
    fn dialogue_reaches_bubble_under_dnd() {
        let t0 = Instant::now();
        let mut spoken = None;

        let dialogue = carry_line(&mut spoken, Some("hello there"), Some(0), t0);

        assert_eq!(
            dialogue.as_deref(),
            Some("hello there"),
            "under DND, dialogue still reaches carry_line and the bubble"
        );
    }

    /// #178 and #277: every overlay is told about every Instance, and only the
    /// one that owns the bubble is told the line, the indicator and the cue.
    /// The webview used to strip these for itself.
    #[test]
    fn only_the_bubble_owner_is_told_the_line_the_indicator_and_the_cue() {
        let left = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let right = Rect { x: 1920.0, ..left };
        let placed = Placed {
            id: "one".to_string(),
            character: "bmo".to_string(),
            sprite: SpriteRect {
                x: 2000,
                y: 400,
                scale: 2,
            },
            width: 128,
            height: 128,
            animation: "idle".to_string(),
            frame_index: 0,
            mirror: 1,
            dialogue: Some("Yare yare daze.".to_string()),
            thinking: true,
            asking: true,
            chatting: true,
            cue: Some(Cue::Poke),
            owner: Some(1),
            qm: None,
            mask: fidget_core::overlay::AlphaMask::from_png(PATCHY, 128)
                .expect("the 2x2 fixture decodes"),
        };

        let owner = SpritePlacement::new(&placed, right, 1);
        assert_eq!(owner.dialogue.as_deref(), Some("Yare yare daze."));
        assert!(owner.thinking);
        assert!(owner.asking);
        assert!(owner.bubble);
        assert_eq!(owner.cue, Some("poke"));

        let elsewhere = SpritePlacement::new(&placed, left, 0);
        assert_eq!(elsewhere.dialogue, None, "no line to latch off the owner");
        assert!(!elsewhere.thinking, "no indicator to arm off the owner");
        assert!(
            !elsewhere.asking,
            "one pointer to Chat, not one per display"
        );
        assert!(!elsewhere.bubble);
        assert_eq!(elsewhere.cue, None, "or the cue sounds once per display");
        assert!(
            owner.chatting && elsewhere.chatting,
            "the pill can open on any overlay, so each one hears that Chat is up"
        );

        assert_eq!(
            (elsewhere.animation, elsewhere.frame_index, elsewhere.mirror),
            (owner.animation, owner.frame_index, owner.mirror),
            "both draw the same art"
        );
        assert_eq!(
            (owner.x, elsewhere.x),
            (80, 2000),
            "each in its own overlay's coordinates, so the halves meet on the seam"
        );
    }

    /// A typed draft is never lost on a crossing: it rides with the Instance,
    /// and only the overlay that owns the bubble is told it, like the line.
    #[test]
    fn only_the_bubble_owner_is_told_the_draft() {
        let left = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let right = Rect { x: 1920.0, ..left };
        let draft = fidget_core::quick_message::QmDraft {
            text: "half a thought".to_string(),
            focused: true,
        };
        let placed = Placed {
            id: "one".to_string(),
            character: "bmo".to_string(),
            sprite: SpriteRect {
                x: 2000,
                y: 400,
                scale: 2,
            },
            width: 128,
            height: 128,
            animation: "idle".to_string(),
            frame_index: 0,
            mirror: 1,
            dialogue: None,
            thinking: false,
            asking: false,
            chatting: false,
            cue: None,
            owner: Some(1),
            qm: Some(draft.clone()),
            mask: fidget_core::overlay::AlphaMask::from_png(PATCHY, 128)
                .expect("the 2x2 fixture decodes"),
        };

        assert_eq!(SpritePlacement::new(&placed, right, 1).qm, Some(&draft));
        assert_eq!(
            SpritePlacement::new(&placed, left, 0).qm,
            None,
            "one pill, on the display that owns the bubble"
        );
    }

    fn instance_row(id: &str, name: &str) -> InstanceRow {
        InstanceRow {
            id: id.to_string(),
            name: name.to_string(),
            character: "bmo".to_string(),
            prompt: String::new(),
        }
    }

    #[test]
    fn instance_title_is_the_roster_name() {
        let rows = vec![instance_row("a", "Beemo"), instance_row("b", "Pip")];
        assert_eq!(
            instance_title(&rows, "b"),
            "Pip",
            "the window opens titled by the Instance the click named"
        );
    }

    #[test]
    fn instance_title_falls_back_to_the_id_when_the_row_is_gone() {
        // A Character switch can drop the row between the click and this
        // lookup; the id is a title the window can still open under. #588.
        let rows = vec![instance_row("a", "Beemo")];
        assert_eq!(
            instance_title(&rows, "orphan"),
            "orphan",
            "an unknown id still yields a title so Chat opens"
        );
        assert_eq!(
            instance_title(&[], "lonely"),
            "lonely",
            "an empty roster falls back to the id, not a panic"
        );
    }

    /// Pins `overlay_open_chat` as `async`. On Windows a sync command builds
    /// the Chat webview on WebView2's pump thread and deadlocks (#588). A
    /// compile-time witness; the body only has to type-check.
    #[test]
    fn overlay_open_chat_is_async_so_windows_keeps_chat_off_the_webview_pump() {
        fn takes_async<F, Fut>(_f: F)
        where
            F: Fn(tauri::AppHandle, String) -> Fut,
            Fut: std::future::Future<Output = ()>,
        {
        }
        takes_async(overlay_open_chat);
    }

    /// Chat builds Settings, then the context menu opens it again. The menu
    /// must not activate a document that has not finished loading: raise
    /// delivers WM_SETFOCUS, and wry's subclass then MoveFocus and hangs.
    #[test]
    fn a_menu_open_after_chat_does_not_focus_settings_still_loading() {
        assert_eq!(
            settings_effects(settings_open(false, true, false, false, false)),
            &[SettingsEffect::Build, SettingsEffect::Raise],
            "Chat's first open builds the window and does not emit into it"
        );
        assert!(
            settings_effects(settings_open(true, false, false, false, false)).is_empty(),
            "the context menu must not raise or MoveFocus that window yet"
        );
        assert_eq!(
            settings_effects(settings_open(true, false, true, false, false)),
            &[
                SettingsEffect::Unminimize,
                SettingsEffect::Focus,
                SettingsEffect::Raise,
            ],
            "once the document has loaded, the menu raises and focuses it"
        );
    }

    /// An emit before navigation leaves the page on about:blank. The reveal
    /// rides the snapshot until the document is up, then a reload aims the row.
    #[test]
    fn chat_reloads_settings_only_after_the_document_has_loaded() {
        assert!(
            !settings_effects(settings_open(true, true, false, false, false))
                .contains(&SettingsEffect::Reload),
            "emitting settings-refresh before navigation leaves the page blank"
        );
        assert_eq!(
            settings_effects(settings_open(true, true, true, false, false)),
            &[
                SettingsEffect::Unminimize,
                SettingsEffect::Focus,
                SettingsEffect::Raise,
                SettingsEffect::Reload,
            ],
            "an open page reloads so the reveal lands"
        );
    }

    /// A load that never finishes makes every later open a no-op, and the
    /// window stays up until the process is killed.
    #[test]
    fn a_stalled_settings_load_is_not_a_permanent_no_op() {
        assert!(
            settings_effects(settings_open(true, false, false, false, false)).is_empty(),
            "inside the deadline an open still must not MoveFocus"
        );
        assert_ne!(
            settings_open(true, false, false, true, false),
            settings_open(true, false, false, false, false),
            "past the deadline the open is not the same no-op as a load still inside the deadline"
        );
        assert_eq!(
            settings_effects(settings_open(true, false, false, true, false)),
            &[SettingsEffect::Destroy],
            "past the deadline the open destroys the stuck window and does not MoveFocus it"
        );
        assert!(
            settings_effects(settings_open(false, false, false, true, true)).is_empty(),
            "an open while that destroy is in flight does not build into a label Tauri still holds"
        );
        assert_eq!(
            settings_open(true, false, true, true, false),
            SettingsOpen::Focus,
            "a document that has finished is focused, even if the clock is still set"
        );
        assert_eq!(
            settings_open(false, false, false, true, false),
            SettingsOpen::Create,
            "with no window, a stalled flag is a create and not a destroy"
        );
        assert_eq!(
            settings_rebuild_effects(true),
            &[SettingsEffect::Build, SettingsEffect::Raise],
            "when the label frees, the replacement is a new window and still not a Focus"
        );
        assert!(
            settings_rebuild_effects(false).is_empty(),
            "a window the user closed is not built again"
        );
    }

    #[test]
    fn a_settings_load_inside_the_deadline_is_not_stalled() {
        let inside = SETTINGS_LOAD_TIMEOUT.saturating_sub(Duration::from_millis(1));
        assert!(
            !settings_load_expired(Some(inside)),
            "a load still inside the deadline is left alone"
        );
        assert!(
            settings_load_expired(Some(SETTINGS_LOAD_TIMEOUT)),
            "a load that has reached the deadline is stalled"
        );
        assert!(
            !settings_load_expired(None),
            "a missing clock is not a stall, so a healthy open is not destroyed"
        );
    }

    /// Pins the Chat entry points as `async`. On Windows a sync command builds
    /// Settings on WebView2's pump thread and the window stays blank. Tray and
    /// the menu call `present_settings` on the shell thread.
    #[test]
    fn settings_opened_from_chat_stays_off_the_webview_pump() {
        fn show<F, Fut>(_f: F)
        where
            F: Fn(tauri::AppHandle) -> Fut,
            Fut: std::future::Future<Output = ()>,
        {
        }
        show(show_settings);

        fn hint(action: String, app: tauri::AppHandle, state: tauri::State<'_, SettingsState>) {
            let fut = names_hint_act(action, app, state);
            fn assert_send<T: Send>(_: &T) {}
            assert_send(&fut);
            let _: &dyn std::future::Future<Output = Result<names_hint::HintPush, String>> = &fut;
        }
        let _ = hint as fn(String, tauri::AppHandle, tauri::State<'_, SettingsState>);
    }

    /// Asserted on the serialized payload, because the webview reads the wire
    /// shape and not the struct. Production change that would fail this:
    /// sending a bare flag again, so the surface cannot name the cause (#890).
    #[test]
    fn a_preempted_typed_question_carries_the_wake_that_took_its_slot() {
        let poked = serde_json::to_value(cancelled_caret(true, &Happened::Poke).unwrap())
            .expect("a ChatReply should serialize");
        assert_eq!(poked["superseded_by"], "poked");
        assert!(poked["said"].is_null(), "a cancelled caret said nothing");

        let asked_again = serde_json::to_value(
            cancelled_caret(true, &Happened::Chat("and another thing".to_string())).unwrap(),
        )
        .expect("a ChatReply should serialize");
        assert_eq!(asked_again["superseded_by"], "spoken to");
    }

    /// The half a careless fix breaks. A poke or a proactive wake opened no
    /// question on the Chat surface, so a notice there answers nobody.
    #[test]
    fn an_ambient_turn_superseded_by_another_wake_tells_chat_nothing() {
        assert!(cancelled_caret(false, &Happened::Proactive).is_none());
        assert!(cancelled_caret(false, &Happened::Poke).is_none());
    }

    /// Production change that would fail this: `process::exit` from the
    /// console handler. That kills the UI thread before WebView2 `Close`.
    #[test]
    fn windows_ctrl_c_closes_webviews_before_the_event_loop_exits() {
        assert_eq!(
            quit_actions(true, false),
            &[
                QuitAction::Announce,
                QuitAction::ShutdownHarness,
                QuitAction::CloseWebviews,
                QuitAction::ExitEventLoop,
            ]
        );
    }

    /// Unix has no `Chrome_WidgetWin_0` to unregister, so the first Ctrl+C
    /// still leaves immediately. The Windows plan must not leak onto this path.
    #[test]
    fn unix_ctrl_c_still_exits_the_process() {
        assert_eq!(
            quit_actions(false, false),
            &[
                QuitAction::Announce,
                QuitAction::ShutdownHarness,
                QuitAction::ExitProcess,
            ]
        );
    }

    #[test]
    fn a_second_interrupt_exits_without_nesting_shutdown() {
        assert_eq!(quit_actions(false, true), &[QuitAction::ExitProcess]);
        assert_eq!(quit_actions(true, true), &[QuitAction::ExitProcess]);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Recorded {
        Announce,
        ShutdownHarness,
        CloseWebview,
        CloseWindow,
        ExitEventLoop,
        ExitProcess,
    }

    #[derive(Default)]
    struct Recording(Vec<Recorded>);

    impl QuitHost for Recording {
        fn announce(&mut self) {
            self.0.push(Recorded::Announce);
        }
        fn shutdown_harness(&mut self) {
            self.0.push(Recorded::ShutdownHarness);
        }
        fn close_webview(&mut self) {
            self.0.push(Recorded::CloseWebview);
        }
        fn close_window(&mut self) {
            self.0.push(Recorded::CloseWindow);
        }
        fn exit_event_loop(&mut self) {
            self.0.push(Recorded::ExitEventLoop);
        }
        fn exit_process(&mut self) {
            self.0.push(Recorded::ExitProcess);
        }
    }

    /// The recorder has to be able to name a window close, or the quit test
    /// could not tell that path from `Webview::close`.
    #[test]
    fn the_recorder_names_a_window_close_separately_from_a_webview_close() {
        let mut host = Recording::default();
        host.close_window();
        host.close_webview();
        assert_eq!(host.0, vec![Recorded::CloseWindow, Recorded::CloseWebview]);
    }

    fn recorded(orderly: bool, already: bool) -> Vec<Recorded> {
        let mut host = Recording::default();
        for action in quit_actions(orderly, already) {
            perform_quit_action(*action, &mut host);
        }
        host.0
    }

    /// `CloseWebviews` is `Webview::close`. `ExitEventLoop` is `AppHandle::exit`.
    /// Neither is `WebviewWindow::close`, and neither calls `process::exit`.
    #[test]
    fn perform_quit_action_closes_the_webview_then_exits_the_loop() {
        assert_eq!(
            recorded(true, false),
            vec![
                Recorded::Announce,
                Recorded::ShutdownHarness,
                Recorded::CloseWebview,
                Recorded::ExitEventLoop,
            ]
        );
    }

    #[test]
    fn perform_quit_action_on_unix_exits_the_process() {
        assert_eq!(
            recorded(false, false),
            vec![
                Recorded::Announce,
                Recorded::ShutdownHarness,
                Recorded::ExitProcess,
            ]
        );
    }

    #[test]
    fn perform_quit_action_on_a_second_interrupt_only_exits_the_process() {
        assert_eq!(recorded(false, true), vec![Recorded::ExitProcess]);
        assert_eq!(recorded(true, true), vec![Recorded::ExitProcess]);
    }
}
