//! The Engine: `WorldSnapshot` in, `Frame` out, once per tick.
//!
//! Pure and synchronous. It reads no clock, holds no timers and performs no
//! I/O, so time reaches it only as elapsed milliseconds on a snapshot. That is
//! what lets every spatial property be tested by constructing snapshots and
//! asserting frames, with no windowing system, no model and no waiting.

use crate::character::{Behavior, CursorReaction, Primitive};
use crate::director::Seeded;
use crate::overlay::{display_index_for, stands_on};
use crate::visibility::Desktop;
pub use crate::window_source::{Rect, WindowId};
use std::collections::{BTreeMap, BTreeSet};

mod geometry;
mod transition;
use geometry::*;

/// A point in the one coordinate space the Engine works in: points, y growing
/// downward, spanning every visible display.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// One visible window: which one it is, and where.
///
/// The id is what says the window under the sprite this tick is the one it was standing on last tick, which geometry cannot say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub id: WindowId,
    pub rect: Rect,
}

/// Where the sprite is anchored and which physics apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum State {
    Grounded,
    Falling,
    Dragged,
    Perched,
    Climbing,
    Asleep,
}

/// An interaction verb the user performed since the previous tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verb {
    /// Present on every tick the sprite is held, not just on the press.
    Grab,
    /// A Grab released while moving. The Shell measures the cursor's velocity;
    /// the Engine only flies it. Nothing else is a Throw, so one that arrives
    /// while the sprite is not being held is ignored.
    Throw { velocity: Point },
    /// A click on the sprite.
    Poke,
    /// A right-click on the sprite. Opens the same menu the tray icon opens.
    Menu,

    /// A double-click on the sprite. Opens the Chat surface, which the Shell draws.
    ///
    /// The verb set is fixed at five: every verb is a tax on every Character that will ever exist, and a sixth would mean a ninth Required Animation.
    Summon,
}

/// The one cue an interaction earned this tick, for the Shell to draw and to sound.
///
/// The Engine picks it because it is the only place that knows both the verbs and the `State::Dragged` transitions, and because it is pure. Vocabulary is global: no Character declares a pitch or a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// A left click.
    Poke,
    /// A double click. The Shell plays it over a Poke cue still in flight: the
    /// first click of the pair emitted its Poke before anything could know a
    /// second was coming.
    Summon,
    /// A right click.
    Menu,
    /// The sprite left its footing for the cursor.
    Pickup,
    /// The sprite was let go standing still.
    Drop,
    /// The sprite was let go moving. The Drop cue played harder rather than a
    /// sixth shape and a sixth sound.
    Throw,
}

impl Cue {
    /// The name the webview's cue machine keys its visual and its sound by.
    ///
    /// A name rather than a serialized enum, following `Frame::animation`: what crosses to the webview is already a table lookup on the other side.
    pub fn name(self) -> &'static str {
        match self {
            Self::Poke => "poke",
            Self::Summon => "summon",
            Self::Menu => "menu",
            Self::Pickup => "pickup",
            Self::Drop => "drop",
            Self::Throw => "throw",
        }
    }

    /// `Grab` and `Throw` carry none: a cue keyed on `Grab` would fire on every
    /// tick of a drag, and a `Throw` is answered by the transition out of
    /// `Dragged` — which a slow release, emitting no verb at all, has to be answered by anyway.
    fn of_verb(verb: &Verb) -> Option<Self> {
        match verb {
            Verb::Poke => Some(Self::Poke),
            Verb::Summon => Some(Self::Summon),
            Verb::Menu => Some(Self::Menu),
            Verb::Grab | Verb::Throw { .. } => None,
        }
    }
}

/// What the Director proposed since the previous tick.
///
/// Advisory: the Engine plays the named Behavior if the Character declares one by that name and the sprite's State permits its Primitives, and refuses it otherwise. The line is spoken either way, since speaking moves nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BehaviorProposal {
    pub behavior: String,
    pub dialogue: Option<String>,
}

/// Everything the Engine is told about the world for one tick.
#[derive(Clone, Debug, Default)]
pub struct WorldSnapshot {
    /// The part of each visible display a sprite may occupy.
    ///
    /// Not the whole display: the Shell strips furniture the sprite must not go behind, so floor, ceiling and walls derive from these. The Dock is known only by its Perch's reserved id, which makes its side a wall.
    pub displays: Vec<Rect>,
    /// Visible windows in descending z-order.
    pub windows: Vec<Window>,
    pub cursor: Point,
    /// Interaction verbs pending since the previous tick: what the user did.
    pub verbs: Vec<Verb>,
    /// A Poke settled into one the Director hears, from `Pointer::poke_settled`.
    pub poke_settled: bool,
    /// Milliseconds since the previous tick.
    pub elapsed_ms: u32,
    /// A Behavior proposal delivered since the previous tick, if the Director
    /// made one. Advisory: the Engine is free to refuse it.
    pub proposal: Option<BehaviorProposal>,

    /// Bumps when the assembler actually re-read the window list. Zero means
    /// the caller did not say, and the Engine treats every tick as a fresh sample.
    /// A reused generation is a tick between polls, so a riding sprite coasts or hitch-steps.
    pub poll_generation: u64,
    /// This Instance's quick message has the caret. Walk and chase stop;
    /// idle, sit and a perch in place do not.
    pub composing: bool,
    /// A speech bubble or the quick-message pill is showing. Walk and chase
    /// stop and none starts; a sprite on its feet draws `talk` meanwhile.
    pub locomotion_frozen: bool,
}

/// Everything the renderer is told after one tick.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The sprite's contact point — where its feet are, not its top-left. The
    /// renderer offsets by the art's size, which the Engine does not know.
    pub position: Point,
    pub velocity: Point,
    pub state: State,
    /// The Animation to play, by the name every Character Package must supply.
    pub animation: &'static str,

    /// How long the current Animation has been playing, in milliseconds.
    ///
    /// Not a frame index: fps and loop mode are the Manifest's. `character::Animation::frame_at` does that arithmetic.
    pub animation_ms: u32,

    /// The draw that decides which member of the Animation's variant ring is on screen.
    ///
    /// Held for as long as the Animation plays and taken afresh when it changes, so a Behavior stepping through `idle` shows one strip instead of swapping costumes every Primitive turn.
    pub variant_draw: u64,
    /// A line to speak on this frame only. Dialogue is an event, not a state.
    pub dialogue: Option<String>,

    /// The Behavior that started playing on this frame, if a proposal was taken.
    ///
    /// An event like `dialogue`, for the Shell rather than the renderer: what the Director suggested and what the user actually saw are different lists, and repetition is suppressed on the second.
    pub behavior: Option<String>,

    /// The Behavior whose Primitives are still playing, for every tick of the span rather than the one it began on.
    ///
    /// `behavior` says a proposal was taken now; this says what the sprite is in the middle of. `None` for Engine-played moments no Director named.
    pub playing_behavior: Option<String>,

    /// The Primitive on screen: how far into its chain the Engine has got.
    ///
    /// `None` once a chain has run out, including while a walk it started carries on under its own velocity.
    pub playing_primitive: Option<Primitive>,
    /// Whether this tick carried the sprite with a moving Perch. The Shell
    /// polls the window list at the frame rate only then.
    pub riding: bool,
    /// Which way the sprite is pointed, as -1.0 (left) or 1.0 (right). Only
    /// horizontal travel turns it, so a stop keeps the last heading and the
    /// renderer can mirror the art by it without flicker at rest.
    pub facing: f64,
    /// Whether the user addressed the character this tick: a settled Poke, a
    /// Summon, a Menu, a pickup, a Throw or a Dwell. The Shell wakes the session from this bit.
    pub addressed: bool,
    /// The cue this interaction earned, if one landed. A one-tick pulse like
    /// `dialogue`, and at most one a tick — the precedence is in `tick`.
    pub cue: Option<Cue>,

    /// A Behavior this Character declared, proposed this tick, and turned down by `permitted`.
    ///
    /// Reported rather than dropped: a refusal and a model that proposed nothing otherwise look identical in the trace. `crates/core` does no I/O, so the Shell prints it.
    pub refused: Option<String>,
}

/// How long one Primitive holds the screen.
///
/// ponytail: one duration for every Primitive of every Character. The Engine
/// deliberately does not know an Animation's length — fps and loop mode are the
/// Character Manifest's — so it cannot play a Primitive until the art runs out.
/// Art shorter than the turn costs nothing: `loop = once` holds its last frame
/// for the remainder, which is what a brief startle looks like. Longer art is
/// the ceiling, and Blip already sits on it — sleep's `fps = 1` over
/// two frames is a 2000ms strip the sprite leaves at frame 0. Give a Primitive
/// its own duration when a Character's art needs to outlast this one.
const PRIMITIVE_MS: u32 = 600;

/// How long a poked sprite stands still after reacting, before it may move again.
/// `PRIMITIVE_MS` is the length of the `react` art, not of a pause that reads as the character noticing you.
/// 2.5 s is long enough to register and short enough that a click never feels like a freeze.
const POKE_COOLDOWN_MS: u32 = 2_500;

/// How long a resting, untouched sprite waits before it goes to sleep. A tuning
/// knob: long enough not to nod off mid-conversation, short enough that a sprite
/// on an unattended desktop settles down.
const SLEEP_AFTER_MS: u32 = 60_000;

/// How long a sprite rests quietly on a Perch before leaving sit and idling in place.
/// Long enough to read as settling onto the edge, and far short of nodding off.
/// It bounds how much of a long idle strip a perched sprite gets through before the next wake.
const PERCHED_IDLE_AFTER_MS: u32 = 4_000;

/// Points per second the sprite hauls itself up a screen edge. A tuning knob.
const CLIMB_SPEED: f64 = 200.0;

/// Points per second the sprite walks along whatever it is standing on. A
/// tuning knob, slower than a climb: hauling yourself up an edge is urgent and
/// strolling along a title bar is not.
const WALK_SPEED: f64 = 120.0;

/// Points per second squared. A tuning knob: the number that makes a fall read as heavy rather than floaty is found by watching it, not by deriving it.
/// 3600 keeps a Throw on the display long enough to see, instead of spending the flight above the usable frame.
const GRAVITY: f64 = 3600.0;

/// Points per second a Jump launches upward at. A tuning knob. The peak is
/// `JUMP_SPEED` squared over twice `GRAVITY`, so 900 clears about 110 points —
/// roughly a sprite's height — and lands within half a second.
const JUMP_SPEED: f64 = 900.0;

/// How much of the art must stay below the usable top for the feet to be put down there.
/// Half: a character clipped at the crown still reads as itself, while a whole sprite's height refuses every window near the top of a display, a dead zone sized for the tallest Character and charged to all of them.
const CEILING_VISIBLE_SHARE: f64 = 0.5;

/// The clearance an Engine built without an art height uses: 32px at 4×.
/// Production always supplies one; this keeps a bare `Engine::new`
/// standing where it always did.
const CEILING_CLEARANCE_UNKNOWN: f64 = 128.0;

/// How far from a horizontal display edge the feet must stay so the full sprite is on-screen when standing (not climbing).
/// The Engine does not know the art's size; 64 is 16px at 4×, enough for a character of typical width.
/// Climb frames assume the wall is in the middle of the frame, so this inset applies only to non-climb states.
const EDGE_CLEARANCE: f64 = 64.0;

/// Points per second squared. The yank gate is this times `YANK_WINDOW_S`: a change in Perch speed larger than that, measured against the speed from about one idle poll ago, drops the sprite.
/// The last 16 ms slope treats WindowServer jitter as a yank.
pub const RIDE_ACCELERATION: f64 = 10_000.0;

/// How far back the yank gate looks. Fast poll still tracks the window;
/// only the fall decision stays on this cadence.
const YANK_WINDOW_S: f64 = 0.1;

/// Cursor proximity radius in points.
///
/// Close enough to react before the hand is on top of the sprite, far enough that the character does not jump at every scroll or window move.
const NEAR_RADIUS: f64 = 150.0;

/// How long the cursor must rest on the sprite to count as Dwell, in milliseconds.
///
/// Long enough that passing over the sprite does not wake the Director, short enough that intentionally addressing feels immediate.
const DWELL_MS: u32 = 400;

/// Minimum cursor velocity toward the sprite to count as a Rush, in points per second.
///
/// Fast enough to read as a startle (a flick or a fast approach), slow enough that ordinary cursor travel near the sprite does not trigger it.
const RUSH_VELOCITY: f64 = 800.0;

/// Maximum time the chase Primitive pursues the cursor before giving up, in milliseconds.
///
/// Long enough to catch a cursor that is moving or that starts far away, short enough that a chase that will never close still disengages. Boredom is the realism.
const CHASE_TIMEOUT_MS: u32 = 8000;

/// How close the sprite's x must be to the cursor's x to count as arrival, in points.
///
/// Close enough to read as catching it, loose enough that the sprite does not overshoot and backtrack.
const CHASE_ARRIVAL_THRESHOLD: f64 = 30.0;

pub struct Engine {
    position: Point,
    velocity: Point,
    state: State,
    /// Milliseconds the sprite has spent on its feet, untouched.
    idle_ms: u32,
    /// Milliseconds the sprite has been quietly resting while Perched: not
    /// riding, not walking, no Primitive playing.
    perched_rest_ms: u32,
    animation: &'static str,

    animation_ms: u32,

    /// The draw the renderer weighs against the Animation's variant ring, and the source it comes from.
    /// The Engine knows when an Animation starts and nothing about the art; which strip the number lands on is the Character's.
    variant_draw: u64,
    variants: Seeded,
    /// The Behaviors the Character declares, which a proposal names.
    behaviors: BTreeMap<String, Behavior>,

    sprite_height: Option<f64>,
    /// The windows of the previous tick, to tell a window that has come to
    /// contain the sprite from one that contained it all along. See
    /// `swallowed_by`.
    previous_windows: Vec<Window>,
    /// Where the sprite stood at the end of the previous tick — the other half
    /// of what says whether a window has come to contain it.
    previous_position: Point,

    /// The Primitives of the Behavior being played, last first, so the one on screen is on top.
    ///
    /// Not a State: playing a Behavior is something the sprite does while standing, falling or perched, and giving it a State would mean deciding what it resumes as.
    playing: Vec<Primitive>,

    /// The name of the Behavior `playing` was flattened from, while it lasts.
    ///
    /// The Primitives alone cannot be traced back to it: `chain` flattens a Behavior and the Behaviors it links to into one list, and two Characters may declare the same Primitives under different names. Kept beside `playing` and cleared with it, so the pair is never half true.
    playing_behavior: Option<String>,

    primitive_ms: u32,
    /// Which way the sprite is pointed, as -1 or 1. A walk needs a direction
    /// and the Primitive carries none, so it goes the way it was last heading.
    facing: f64,
    /// Whether this tick translated the sprite with a moving Perch. Decides
    /// the Hold animation; not a State — the sprite is still Perched.
    riding: bool,
    /// Last observed velocity of the ridden Perch, for the acceleration gate
    /// and for coasting between polls.
    perch_velocity: Point,
    /// Last observed acceleration of the ridden Perch. Constant-velocity
    /// coasting hitch-steps when a drag speeds up or slows; keeping the
    /// derivative lets the in-between ticks follow the curve.
    perch_acceleration: Point,
    /// Seconds since the last fresh window sample, so a coast integrates
    /// from the sample rather than compounding Euler error each tick.
    coast_s: f64,
    /// The assembler's last `poll_generation`. Equal generations mean the
    /// window list was reused and a ride has to coast rather than wait.
    last_poll_generation: u64,

    /// The Perch the sprite last stood on, remembered so a coast that has left the stale rectangle can still match the window when it updates.
    /// The id is what matches it in the next sample; the rectangle is the origin a coast integrates from.
    last_perch: Option<Window>,
    /// How far along that Perch the sprite stands, so a snap back onto a
    /// fresh sample keeps the place it was holding.
    hold_offset_x: f64,
    /// Seconds since the last fresh window sample. Velocity and acceleration
    /// are that interval, not a constant poll, because idle and ride differ.
    since_sample_s: f64,
    /// Perch velocity from about `YANK_WINDOW_S` ago. The fall decision
    /// compares against this, not the last 16 ms sample.
    yank_reference: Point,
    since_yank_ref_s: f64,
    /// Quiet but not gone. Director proposals are refused and unprompted
    /// dialogue is not spoken, while Poke/Grab/Throw still work and the
    /// Character stays visible.
    do_not_disturb: bool,

    near_reaction: CursorReaction,
    rush_reaction: CursorReaction,

    cursor_near: bool,
    /// Milliseconds the cursor has rested on the sprite without pressing.
    cursor_dwell_ms: u32,
    /// Whether this dwell session has already addressed the Director.
    cursor_dwell_addressed: bool,
    /// Last observed cursor position, for Rush velocity calculation.
    last_cursor: Point,
    /// Last observed cursor velocity, for Rush detection.
    cursor_velocity: Point,
    /// Whether a Rush has already been reported this Near session.
    /// Prevents repeated Rush reactions while the cursor stays near.
    rush_reported: bool,

    chase_ms: u32,
    /// Milliseconds left of standing still after a Poke. While it runs, a
    /// proposal that would move the sprite is refused; a Grab, a Throw or
    /// losing the ground ends it at once.
    poke_cooldown_ms: u32,

    /// Typing or a bubble holds the feet this tick. Copied off the snapshot
    /// at the start so a proposal later in the tick sees the same hold.
    feet_held: bool,

    /// Whether the previous tick already carried `Verb::Menu`. The Shell re-injects that verb every tick the popup is held, so a cue keyed on the verb would fire for as long as the menu is open.
    /// The press edge is the cue; a gap clears this and the next press cues again.
    menu_held: bool,
}

/// Where each sprite should stand to be on the display under `cursor`.
/// `floors` are the usable frames at the same indexes as `monitors`, so the
/// feet do not land behind the Dock.
pub fn bring_landings(
    feet: &[Point],
    widths: &[f64],
    monitors: &[Rect],
    floors: &[Rect],
    cursor: Point,
) -> Option<Vec<Option<Point>>> {
    let on = display_index_for((cursor.x, cursor.y), monitors)?;
    let monitor = monitors[on];
    let floor = floors.get(on).copied().unwrap_or(monitor);
    let mut landings = vec![None; feet.len()];

    let mut arrivals: Vec<usize> = feet
        .iter()
        .enumerate()
        .filter(|(_, at)| !stands_on((at.x, at.y), &monitor))
        .map(|(index, _)| index)
        .collect();
    if arrivals.is_empty() {
        return Some(landings);
    }
    arrivals.sort_by(|&a, &b| feet[a].x.total_cmp(&feet[b].x).then(a.cmp(&b)));

    let (left, right) = foot_bounds(&floor);
    let spans: Vec<f64> = arrivals
        .iter()
        .map(|&index| sprite_span(widths.get(index).copied().unwrap_or(0.0)))
        .collect();
    // Centre the row on the cursor, then shift it until it sits inside the
    // inset. A row wider than the display piles up at the far inset.
    let between: f64 = spans.windows(2).map(|pair| (pair[0] + pair[1]) / 2.0).sum();
    let mut start = cursor.x - between / 2.0;
    let end = start + between;
    if end > right {
        start -= end - right;
    }
    if start < left {
        start = left;
    }
    let mut xs = Vec::with_capacity(arrivals.len());
    let mut x = start;
    for (slot, span) in spans.iter().enumerate() {
        xs.push(x.clamp(left, right));
        if slot + 1 < spans.len() {
            x += (span + spans[slot + 1]) / 2.0;
        }
    }
    let stayers: Vec<(f64, f64)> = feet
        .iter()
        .enumerate()
        .filter(|(_, at)| stands_on((at.x, at.y), &monitor))
        .map(|(index, at)| (at.x, sprite_span(widths.get(index).copied().unwrap_or(0.0))))
        .collect();
    // A right-click lands on whoever is already here. Slide the arrivals off
    // them; if the display cannot hold that, keep the overlap.
    if let Some(shift) = shift_off_stayers(&xs, &spans, &stayers, left, right) {
        for place in &mut xs {
            *place += shift;
        }
    }
    let y = floor.bottom();
    for (slot, &index) in arrivals.iter().enumerate() {
        landings[index] = Some(Point { x: xs[slot], y });
    }
    Some(landings)
}

/// Where each sprite on a fullscreen display teleports to the first free display.
/// `desktop.fullscreen` shares `monitors`' indexes. Everyone else stays put, so
/// a sprite already moved plans nothing next time; when every display is taken, nobody moves.
pub fn bring_off_fullscreen(
    feet: &[Point],
    widths: &[f64],
    monitors: &[Rect],
    floors: &[Rect],
    desktop: &Desktop,
) -> Vec<Option<Point>> {
    let mut landings = vec![None; feet.len()];
    let Some(target) = desktop
        .first_free_display()
        .and_then(|index| monitors.get(index))
    else {
        return landings;
    };
    let stranded = |at: &Point| {
        monitors
            .iter()
            .zip(&desktop.fullscreen)
            .any(|(monitor, &taken)| taken && stands_on((at.x, at.y), monitor))
    };
    // Bring to the first free display, among only the stranded and those already there.
    // They are the arrivals and the stayers it lays out. Anyone on another
    // free display is not in it, so is not swept along.
    let picked: Vec<usize> = (0..feet.len())
        .filter(|&index| {
            let at = &feet[index];
            stranded(at) || stands_on((at.x, at.y), target)
        })
        .collect();
    let picked_feet: Vec<Point> = picked.iter().map(|&index| feet[index]).collect();
    let picked_widths: Vec<f64> = picked
        .iter()
        .map(|&index| widths.get(index).copied().unwrap_or(0.0))
        .collect();
    let centre = Point {
        x: target.x + target.width / 2.0,
        y: target.y + target.height / 2.0,
    };
    if let Some(planned) = bring_landings(&picked_feet, &picked_widths, monitors, floors, centre) {
        for (index, landing) in picked.into_iter().zip(planned) {
            landings[index] = landing;
        }
    }
    landings
}

fn sprite_span(width: f64) -> f64 {
    if width > 0.0 {
        width
    } else {
        EDGE_CLEARANCE
    }
}

/// Inset by the edge clearance. A narrower display shrinks the inset until the sides meet.
fn foot_bounds(floor: &Rect) -> (f64, f64) {
    let inset = EDGE_CLEARANCE.min(floor.width.max(0.0) / 2.0);
    (floor.x + inset, floor.x + floor.width.max(0.0) - inset)
}

/// How far to slide the row so it does not stand on someone already there.
/// Right before left. `None` when no slide both fits and clears.
fn shift_off_stayers(
    xs: &[f64],
    spans: &[f64],
    stayers: &[(f64, f64)],
    left: f64,
    right: f64,
) -> Option<f64> {
    if stayers.is_empty() || row_clears(xs, spans, stayers) {
        return Some(0.0);
    }
    let room = (right - left).max(0.0);
    let mut delta = 1.0;
    while delta <= room {
        for shift in [delta, -delta] {
            let moved: Vec<f64> = xs.iter().map(|x| x + shift).collect();
            let inside = moved.iter().all(|x| *x >= left && *x <= right);
            if inside && row_clears(&moved, spans, stayers) {
                return Some(shift);
            }
        }
        delta += 1.0;
    }
    None
}

fn row_clears(xs: &[f64], spans: &[f64], stayers: &[(f64, f64)]) -> bool {
    xs.iter().zip(spans).all(|(x, span)| {
        stayers
            .iter()
            .all(|(sx, sw)| (x - sx).abs() >= (span + sw) / 2.0)
    })
}

impl Engine {
    /// A sprite placed at `position`, falling until the world says otherwise.
    pub fn new(position: Point) -> Self {
        Self {
            position,
            velocity: Point::default(),
            state: State::Falling,
            idle_ms: 0,
            perched_rest_ms: 0,
            animation: animation_for(State::Falling),
            animation_ms: 0,
            variant_draw: 0,
            variants: Seeded::new(0),
            behaviors: BTreeMap::new(),
            sprite_height: None,
            previous_windows: Vec::new(),
            previous_position: position,
            playing: Vec::new(),
            playing_behavior: None,
            primitive_ms: 0,
            facing: 1.0,
            riding: false,
            perch_velocity: Point::default(),
            perch_acceleration: Point::default(),
            coast_s: 0.0,
            last_poll_generation: 0,
            last_perch: None,
            hold_offset_x: 0.0,
            since_sample_s: 0.0,
            yank_reference: Point::default(),
            since_yank_ref_s: 0.0,
            do_not_disturb: false,
            near_reaction: CursorReaction::default(),
            rush_reaction: CursorReaction::default(),
            cursor_near: false,
            cursor_dwell_ms: 0,
            cursor_dwell_addressed: false,
            last_cursor: Point::default(),
            cursor_velocity: Point::default(),
            rush_reported: false,
            chase_ms: 0,
            poke_cooldown_ms: 0,
            feet_held: false,
            menu_held: false,
        }
    }

    /// The Behaviors this Character declares. Nothing else reaches the Engine
    /// from a Character Package: art is the renderer's, and a Behavior is
    /// Primitives the Engine already owns.
    pub fn with_behaviors(mut self, behaviors: BTreeMap<String, Behavior>) -> Self {
        self.behaviors = behaviors;
        self
    }

    /// How tall this Instance's art stands, in points.
    ///
    /// Without it the Engine falls back to a fixed guess sized for the tallest Character and charged to every one.
    pub fn with_sprite_height(mut self, height: f64) -> Self {
        self.sprite_height = height.is_finite().then_some(height).filter(|h| *h > 0.0);
        self
    }

    /// A climber stands still on the wall through the post-Poke cooldown and while its feet are held, as a walker does on the floor.
    fn climb_paused(&self) -> bool {
        self.state == State::Climbing && (self.poke_cooldown_ms > 0 || self.feet_held)
    }

    /// How far below a display's usable top the feet may be put down.
    fn ceiling_clearance(&self) -> f64 {
        self.sprite_height
            .map_or(CEILING_CLEARANCE_UNKNOWN, |height| {
                height * CEILING_VISIBLE_SHARE
            })
    }

    /// The seed this Instance's variant draws come from, so two characters of one Character do not idle in lockstep.
    ///
    /// Passed in rather than read: the Engine has no clock, and a draw no test could reproduce would be worse than no draw at all. The first is taken here, because the Animation a sprite spawns in is one the seed should reach as well.
    pub fn with_variant_seed(mut self, seed: u64) -> Self {
        self.variants = Seeded::new(seed);
        self.variant_draw = self.variants.draw();
        self
    }

    /// The Character's cursor reactions.
    pub fn with_cursor_reactions(mut self, near: CursorReaction, rush: CursorReaction) -> Self {
        self.near_reaction = near;
        self.rush_reaction = rush;
        self
    }

    /// Toggle Do Not Disturb. The Character stays visible but stops starting things: no Director proposals are applied and no unprompted dialogue is spoken. Poke, Grab, and Throw still work.
    ///
    /// A walk already under way has to be sat down too. Walk velocity outlives the Primitive that started it, so refusing the next proposal would otherwise leave the sprite pacing.
    pub fn set_do_not_disturb(&mut self, enabled: bool) {
        self.do_not_disturb = enabled;
        if enabled
            && matches!(self.state, State::Grounded | State::Perched)
            && (self.is_walking()
                || matches!(self.on_screen(), Some(Primitive::Walk | Primitive::Chase)))
        {
            let _ = self.play(&[Primitive::Sit]);
            self.velocity.x = 0.0;
        }
    }

    pub fn do_not_disturb(&self) -> bool {
        self.do_not_disturb
    }

    /// Where the sprite's feet are.
    pub fn feet(&self) -> Point {
        self.position
    }

    /// Stand the sprite at `feet` and drop the motion that would carry it off.
    /// A walk, a perch and a fall all outlive the request, and an idle clock
    /// already near sleep would nod off on the tick it arrived.
    pub fn stand_at(&mut self, feet: Point) {
        self.position = feet;
        self.previous_position = feet;
        self.previous_windows.clear();
        self.velocity = Point::default();
        self.state = State::Grounded;
        self.animation = animation_for(State::Grounded);
        self.animation_ms = 0;
        self.idle_ms = 0;
        self.perched_rest_ms = 0;
        self.stop_playing();
        self.last_perch = None;
        self.hold_offset_x = 0.0;
        self.rest_perch();
        self.riding = false;
        self.chase_ms = 0;
        self.poke_cooldown_ms = 0;
    }

    /// Swap the Character this Engine is playing without moving the sprite.
    ///
    /// A switch is a new set of Behaviors and cursor reactions, not a new body: dropping the sprite so it can fall as someone else would be a teleport the user did not ask for.
    pub fn retarget(
        &mut self,
        behaviors: BTreeMap<String, Behavior>,
        near: CursorReaction,
        rush: CursorReaction,
    ) {
        self.behaviors = behaviors;
        self.near_reaction = near;
        self.rush_reaction = rush;
        self.stop_playing();
    }

    pub fn tick(&mut self, snapshot: &WorldSnapshot) -> Frame {
        let dt = f64::from(snapshot.elapsed_ms) / 1000.0;
        self.feet_held = snapshot.composing || snapshot.locomotion_frozen;

        // Idling is resting untouched. Time spent in the air or in someone's
        // hand does not count towards nodding off.
        if snapshot.verbs.is_empty() {
            self.idle_ms = match self.state {
                State::Grounded | State::Perched | State::Asleep => {
                    self.idle_ms.saturating_add(snapshot.elapsed_ms)
                }
                _ => 0,
            };
        } else {
            self.idle_ms = 0;
        }

        // Quiet perch rest is narrower than idle: Perched only, not riding,
        // not walking, no Primitive playing.
        let quietly_resting_perched = matches!(self.state, State::Perched)
            && !self.riding
            && self.velocity.x == 0.0
            && self.on_screen().is_none()
            && snapshot.verbs.is_empty();
        if quietly_resting_perched {
            self.perched_rest_ms = self.perched_rest_ms.saturating_add(snapshot.elapsed_ms);
        } else {
            self.perched_rest_ms = 0;
        }

        // A proposal holds off the sleep timer without waking a sprite that has already nodded off; only a verb does that.
        // The timer is otherwise still running when the Behavior is played at the end of the tick, and a sprite that nods off first is asleep when the gate reads its State.
        // Do Not Disturb means proposals do not count as being addressed, so the idle timer keeps running.
        if snapshot.proposal.is_some() && !self.do_not_disturb {
            self.idle_ms = 0;
        }

        // Chase times out on wall time, not Primitive turns, so the clock
        // runs while Chase is on screen.
        if self.on_screen() == Some(Primitive::Chase) {
            self.chase_ms = self.chase_ms.saturating_add(snapshot.elapsed_ms);
        }

        // What was already playing ages before anything new starts, so a
        // Primitive begun this tick gets its whole turn rather than losing this
        // tick's milliseconds to the one it replaced.
        let mut started = self.advance(snapshot.elapsed_ms);

        // `woke` marks a sprite a verb roused, so the footing it is put back
        // on is not mistaken for one it arrived at. See the landing below.
        let (state, woke) = transition::on_verbs(self.state, &snapshot.verbs);

        // The cooldown is a thing the sprite does on its feet or on a wall. Being picked up or letting go ends it, because a cooldown that outlived the ground would refuse the first walk after the landing.
        // `state` is what the verbs made of it; a fall the world causes is decided at `on_contact` below, so that clear lands one tick late — harmless, since `permitted` refuses a walk while Falling anyway.
        self.poke_cooldown_ms = if stopped_by_a_poke(state) {
            self.poke_cooldown_ms.saturating_sub(snapshot.elapsed_ms)
        } else {
            0
        };

        if !snapshot.verbs.is_empty() && self.on_screen() == Some(Primitive::Chase) {
            self.stop_playing();
            self.chase_ms = 0;
        }

        // A Grab wins over whatever the sprite was doing, the usable floor included: the user's hand outranks the world.
        // Pickup and drop cues are decided here: `Verb::Grab` is present on every tick the sprite is held, and a slow release emits no verb at all.
        // The transition is the edge, and both States are in hand only between `on_verbs` above and `on_contact` below.
        let mut cue = None;
        if state == State::Dragged {
            if self.state != State::Dragged {
                cue = Some(Cue::Pickup);
            }
            self.position = snapshot.cursor;
            self.velocity = Point::default();
        } else if self.state == State::Dragged {
            let thrown = thrown_velocity(snapshot);
            cue = Some(if thrown.is_some() {
                Cue::Throw
            } else {
                Cue::Drop
            });
            self.velocity = thrown.unwrap_or_default();
        }

        // The cursor already arrives every tick for hit-testing, so noticing
        // costs no new sensing.
        let cursor_distance =
            (snapshot.cursor.x - self.position.x).hypot(snapshot.cursor.y - self.position.y);
        let was_near = self.cursor_near;
        self.cursor_near = cursor_distance < NEAR_RADIUS;

        let cursor_moved = Point {
            x: snapshot.cursor.x - self.last_cursor.x,
            y: snapshot.cursor.y - self.last_cursor.y,
        };
        self.cursor_velocity = if snapshot.elapsed_ms > 0 {
            Point {
                x: cursor_moved.x * 1000.0 / f64::from(snapshot.elapsed_ms),
                y: cursor_moved.y * 1000.0 / f64::from(snapshot.elapsed_ms),
            }
        } else {
            Point::default()
        };
        self.last_cursor = snapshot.cursor;

        // Crossing into Near plays the Character's near_reaction, including under DND.
        if self.cursor_near && !was_near {
            self.rush_reported = false;
            match self.near_reaction {
                CursorReaction::Indifferent => {}
                CursorReaction::Speak => {
                    started |= self.play(&[Primitive::Talk]);
                }
                CursorReaction::Face => {
                    self.facing = if snapshot.cursor.x > self.position.x {
                        1.0
                    } else {
                        -1.0
                    };
                }
                CursorReaction::Toward => {
                    // One-shot walk, not pursuit: Chase is the Primitive that follows.
                    self.facing = if snapshot.cursor.x > self.position.x {
                        1.0
                    } else {
                        -1.0
                    };
                    started |= self.play(&[Primitive::Walk]);
                }
                CursorReaction::Away => {
                    self.facing = if snapshot.cursor.x < self.position.x {
                        1.0
                    } else {
                        -1.0
                    };
                    started |= self.play(&[Primitive::Walk]);
                }
                CursorReaction::React => {
                    started |= self.play(&[Primitive::React]);
                }
            }
        }

        // Leaving Near does not cancel a walk the reaction started. Chase is
        // what follows a cursor; this walk keeps going until it runs out.
        if !self.cursor_near && was_near {
            self.cursor_dwell_ms = 0;
            self.cursor_dwell_addressed = false;
            self.rush_reported = false;
        }

        // Rush (startle): once per Near session, or a fast cursor still in the
        // radius retriggers every tick.
        if self.cursor_near && !self.rush_reported {
            let velocity_magnitude = self.cursor_velocity.x.hypot(self.cursor_velocity.y);
            // Speed while already Near is the whole test; direction is not
            // measured, so a flick past startles the same as a flick at it.
            let toward = cursor_distance < NEAR_RADIUS * 1.2 && velocity_magnitude > RUSH_VELOCITY;
            if toward {
                self.rush_reported = true;
                match self.rush_reaction {
                    CursorReaction::Indifferent => {}
                    CursorReaction::Speak => {
                        started |= self.play(&[Primitive::Talk]);
                    }
                    CursorReaction::Face => {
                        self.facing = if snapshot.cursor.x > self.position.x {
                            1.0
                        } else {
                            -1.0
                        };
                    }
                    CursorReaction::Toward => {
                        self.facing = if snapshot.cursor.x > self.position.x {
                            1.0
                        } else {
                            -1.0
                        };
                        started |= self.play(&[Primitive::Walk]);
                    }
                    CursorReaction::Away => {
                        self.facing = if snapshot.cursor.x < self.position.x {
                            1.0
                        } else {
                            -1.0
                        };
                        started |= self.play(&[Primitive::Walk]);
                    }
                    CursorReaction::React => {
                        started |= self.play(&[Primitive::React]);
                    }
                }
            }
        }

        // Dwell is the cursor on the art, not merely Near. 30 points is a typical sprite half-width; `NEAR_RADIUS` would address from a window away.
        // Counts as addressing: `addressed` makes the next Director wake reactive.
        let mut addressed = false;
        if cursor_distance < 30.0 {
            self.cursor_dwell_ms = self.cursor_dwell_ms.saturating_add(snapshot.elapsed_ms);
            if self.cursor_dwell_ms >= DWELL_MS && !self.cursor_dwell_addressed {
                self.cursor_dwell_addressed = true;
                addressed = true;
                started |= self.play(&[Primitive::Talk]);
            }
        } else {
            self.cursor_dwell_ms = 0;
            self.cursor_dwell_addressed = false;
        }

        // A walk lasts until the sprite runs out of Perch, so the velocity holds after the Behavior that started it. A still Primitive stops it (`walk sit` would otherwise slide).
        // Chase steers toward the cursor's x along the ground: y is a fall, not a pursuit.
        // Arrival is a swat, not overlap; without a timeout the sprite would walk off the display after a cursor that never stops.
        if matches!(state, State::Grounded | State::Perched) {
            match self.on_screen() {
                Some(Primitive::Walk) => self.velocity.x = self.facing * WALK_SPEED,
                Some(Primitive::Chase) => {
                    let target_x = snapshot.cursor.x;
                    let distance_x = (target_x - self.position.x).abs();

                    if distance_x < CHASE_ARRIVAL_THRESHOLD {
                        self.velocity.x = 0.0;
                        self.stop_playing();
                        started |= self.play(&[Primitive::React]);
                    } else if self.chase_ms >= CHASE_TIMEOUT_MS {
                        self.velocity.x = 0.0;
                        self.stop_playing();
                        started = true;
                    } else {
                        self.facing = if target_x > self.position.x {
                            1.0
                        } else {
                            -1.0
                        };
                        self.velocity.x = self.facing * WALK_SPEED;
                    }
                }

                // An upward velocity, plus a forward one so the flight is an arc.
                // `integrate` reads the rising sprite as a lost footing, `transition` makes that a fall, and the fall lands through the existing landing.
                Some(Primitive::Jump) => {
                    self.velocity = Point {
                        x: self.facing * WALK_SPEED,
                        y: -JUMP_SPEED,
                    };
                }
                Some(Primitive::Idle | Primitive::Sit | Primitive::Sleep | Primitive::Hold) => {
                    self.velocity.x = 0.0
                }
                _ => {}
            }
        }

        // Typing or a bubble holds the feet. Idle, sit and perch stay; a walk
        // already under way stops rather than striding in place. A jump still leaves.
        if self.feet_held && matches!(state, State::Grounded | State::Perched) {
            if matches!(self.on_screen(), Some(Primitive::Walk | Primitive::Chase)) {
                self.stop_playing();
            }
            if self.on_screen() != Some(Primitive::Jump) {
                self.velocity.x = 0.0;
            }
        }

        let contact = self.integrate(state, dt, snapshot);

        // Still moving is still awake. A walk proposed just before the timer
        // comes due would otherwise leave the sprite gliding along the edge
        // playing `sleep`.
        let rested = self.idle_ms >= SLEEP_AFTER_MS && self.velocity.x == 0.0;
        self.state = transition::on_contact(state, contact, rested);

        if self.velocity.x != 0.0 {
            self.facing = self.velocity.x.signum();
        }

        // Inset a standing sprite at a display edge so the full sprite is on-screen, and face away.
        // Dragged follows the cursor over edges; Falling must reach the wall; riding and coasting have their own edge logic.
        // Climb frames put the wall in the middle, so clipping is intentional. A held walk drives at the edge to reach the wall.
        let stationary = matches!(self.state, State::Grounded | State::Perched | State::Asleep)
            && self.velocity.x == 0.0
            && !self.riding
            && self.coast_s == 0.0;
        if stationary {
            if let Some((edge_x, face_direction)) = at_horizontal_edge(self.position.x, snapshot) {
                // A Perch narrower than `EDGE_CLEARANCE` would lose the sprite
                // if the inset ran anyway.
                let can_inset = if self.state == State::Perched {
                    perch_at(
                        Point {
                            x: edge_x,
                            y: self.position.y,
                        },
                        &snapshot.windows,
                    )
                    .is_some()
                } else {
                    true
                };

                if can_inset {
                    self.position.x = edge_x;
                }
                self.facing = face_direction;
            }
        }

        // Losing its footing abandons the rest of a Behavior: what the sprite was in the middle of doing was only ever a thing to do standing up.
        // A Jump is the exception: it lost the footing on purpose, so this gate would abandon it mid-arc and the flight would draw as a plain fall. It keeps the screen for its turn.
        if self.on_screen() != Some(Primitive::Jump) && !self.permitted(&self.playing) {
            self.stop_playing();
            started = true;
        }

        // Arriving is an event and not a State: by the time the sprite has landed it is already standing, and `land` is the animation of the moment in between. The Engine plays it itself because no Director could propose it in time.
        // Not a sprite woken onto the same footing it fell asleep on: settling that by falling is how the Engine asks what is underneath, and a sprite that answers in the tick it was asked never left the ground.
        // A wake with nothing under it still falls, and still lands, later.
        if matches!(contact, Some(transition::Contact::Landed(_))) && !woke {
            started |= self.play(&[Primitive::Land]);
        }

        // Riding is an event the Director cannot propose in time, the same
        // as landing. Holding on is not resting, so an Asleep sprite that
        // has to ride wakes rather than sleeping through the move.
        if self.riding {
            self.idle_ms = 0;
            self.state = State::Perched;
            if self.on_screen() != Some(Primitive::Hold) {
                started |= self.play(&[Primitive::Hold]);
            }
        } else if self.on_screen() == Some(Primitive::Hold) {
            self.stop_playing();
            started = true;
        }

        // A proposal is advisory, so a Behavior this Character does not declare is refused rather than reported, and refusing it interrupts nothing.
        // After the sprite has been moved, so the State the gate reads is the one the tick ends in. A walk therefore takes its first step on the tick after the proposal, which is what SPEC.md asks for.
        // Do Not Disturb refuses proposals before they reach the State gate, so the Character stops starting things while staying visible.
        let mut behavior = None;
        // The State gate's refusals only. Do Not Disturb is a silence the
        // user asked for, and an undeclared name is traced at the parse.
        let mut refused = None;
        if let Some(proposal) = &snapshot.proposal {
            if !self.do_not_disturb {
                if let Some(primitives) = self.chain(&proposal.behavior) {
                    // Mid-cooldown, `play` refuses a chain that would move the sprite — whole, greeting included, since a Behavior is one thing to refuse. A line or a gesture on its own plays.
                    // What comes after the cooldown is the Director's fresh call, not the interrupted walk resuming.
                    if self.play(&primitives) {
                        started = true;
                        behavior = Some(proposal.behavior.clone());
                        // The only place a name is attached: `play` is reached
                        // by Engine-played moments too, and it clears the name
                        // so that they cannot inherit the last proposal's.
                        self.playing_behavior = behavior.clone();
                    } else {
                        refused = Some(proposal.behavior.clone());
                    }
                } else if proposal.behavior.is_empty()
                    && proposal.dialogue.is_some()
                    && self.play(&[Primitive::Talk])
                {
                    started = true;
                }
            }
        }

        // A Poke is answered, whatever else is going on, including a Behavior and its own reaction: prodded again, it reacts again from the beginning.
        // A Summon is the second click of a pair, emitted in place of that click's Poke, so it is answered the same way — a double-click that did visibly less than a single click would read as a miss.
        // Menu is not: the tray menu opening is its response.
        if snapshot
            .verbs
            .iter()
            .any(|verb| matches!(verb, Verb::Poke | Verb::Summon))
        {
            // Noticing you does not keep strolling past, or climbing on.
            if stopped_by_a_poke(self.state) {
                self.velocity.x = 0.0;
                self.poke_cooldown_ms = POKE_COOLDOWN_MS;
            }
            started |= self.play(&[Primitive::React]);
        }
        // Every way the user reaches for the sprite addresses the Director here,
        // and only here. A Poke counts once settled: on its own tick it may be
        // the first half of a Summon. Dwell sets the same bit above.
        addressed |= snapshot.poke_settled
            || matches!(cue, Some(Cue::Pickup | Cue::Throw))
            || snapshot
                .verbs
                .iter()
                .any(|verb| matches!(verb, Verb::Summon | Verb::Menu));

        // One cue a tick, and a hand transition outranks a click: the verb that shares a tick with a pickup or a drop is the incidental one.
        // Among the click verbs the first is taken: two clicks cannot land inside one tick.
        // Menu is the Grab shape: the Shell leaves the verb on every held tick, so the cue keys on the press edge, not the level.
        let menu_now = snapshot.verbs.iter().any(|verb| matches!(verb, Verb::Menu));
        let cue = cue.or_else(|| {
            snapshot
                .verbs
                .iter()
                .find_map(|verb| match Cue::of_verb(verb) {
                    Some(Cue::Menu) if self.menu_held => None,
                    other => other,
                })
        });
        self.menu_held = menu_now;

        // A Behavior is drawn over whatever the sprite is doing, so a Poke shows
        // even mid-fall. It changes nothing about where the sprite is.
        let animation = match self.on_screen() {
            Some(primitive) => animation_of(primitive),
            // A walk outlasts the Primitive that starts it: the velocity holds
            // until the sprite runs out of Perch, so the Animation has to hold
            // with it rather than dropping back to standing mid-stride.
            None if self.is_walking() => "walk",
            // The bubble is the sprite speaking, so its feet held, it says so.
            None if snapshot.locomotion_frozen
                && matches!(self.state, State::Grounded | State::Perched) =>
            {
                "talk"
            }
            // A sprite that rests quietly on a Perch idles in place: still
            // Perched, same edge, idle art and idle life.
            None if matches!(self.state, State::Perched)
                && self.perched_rest_ms >= PERCHED_IDLE_AFTER_MS =>
            {
                "idle"
            }
            None => animation_for(self.state),
        };
        // A fresh variant draw on the Animation, not on the Primitive: `idle`
        // and a variant of `idle` are one family, and re-drawing every turn of
        // a 600ms Primitive would flicker between strips mid-Behavior.
        let new_family = animation != self.animation;
        if new_family {
            self.animation = animation;
            self.variant_draw = self.variants.draw();
        }

        // `react` is `loop = once`, so a second Poke on the held last frame would be invisible unless the clock restarts.
        // `land` needs no such clause: a second arrival comes through a fall, which is a change of name.
        // Everything else keeps its clock across Primitive turns, because restarting each turn would draw the first 600ms of the strip and never the rest.
        let startled = self.on_screen() == Some(Primitive::React);
        // A paused climb keeps one pose: the strip's clock would otherwise climb in place.
        if new_family || (started && startled) {
            self.animation_ms = 0;
        } else if !(self.climb_paused() && animation == "climb") {
            self.animation_ms = self.animation_ms.saturating_add(snapshot.elapsed_ms);
        }

        self.previous_windows.clone_from(&snapshot.windows);
        self.previous_position = self.position;
        self.last_poll_generation = snapshot.poll_generation;
        if matches!(self.state, State::Perched | State::Asleep) {
            if let Some(perch) = perch_at(self.position, &snapshot.windows) {
                self.last_perch = Some(perch);
                self.hold_offset_x = self.position.x - perch.rect.x;
            }
        } else {
            self.rest_perch();
            self.last_perch = None;
            self.hold_offset_x = 0.0;
        }

        Frame {
            position: self.position,
            velocity: self.velocity,
            state: self.state,
            animation: self.animation,
            animation_ms: self.animation_ms,
            variant_draw: self.variant_draw,
            dialogue: if self.do_not_disturb {
                None
            } else {
                snapshot
                    .proposal
                    .as_ref()
                    .and_then(|proposal| proposal.dialogue.clone())
            },
            behavior,
            playing_behavior: self.playing_behavior.clone(),
            playing_primitive: self.on_screen(),
            riding: self.riding,
            facing: self.facing,
            addressed,
            cue,
            refused,
        }
    }

    /// Move the sprite through one tick's worth of `state`'s physics, and report what its body met.
    ///
    /// Position and velocity are settled here; what the sprite becomes as a result is `transition`'s to say, so no State is read or written past the one this is handed.
    fn integrate(
        &mut self,
        state: State,
        dt: f64,
        snapshot: &WorldSnapshot,
    ) -> Option<transition::Contact> {
        use transition::Contact;

        self.riding = false;
        match state {
            State::Falling => {
                self.velocity.y += GRAVITY * dt;
                self.position.x += self.velocity.x * dt;

                if let Some(wall) = wall_reached(self.position.x, self.velocity.x, snapshot)
                    .or_else(|| dock_side_reached(self.position, self.velocity.x, snapshot))
                {
                    // Arriving at a screen edge sideways is a catch, not a stop. It also keeps the sprite inside the displays.
                    // The Dock's side catches the same way: a low throw or a drop under the Dock climbs out instead of resting behind it.
                    self.position.x = wall;
                    self.velocity = Point::default();
                    Some(Contact::Wall)
                } else {
                    let next_y = self.position.y + self.velocity.y * dt;

                    // Rising only: a Grab can put the feet above the ceiling,
                    // and snapping them down would teleport a drop.
                    if self.velocity.y < 0.0 {
                        if let Some(ceiling) =
                            ceiling_over(self.position.x, snapshot, self.ceiling_clearance())
                        {
                            let stop_at = self.position.y.min(ceiling);
                            if next_y < stop_at {
                                self.position.y = stop_at;
                                self.velocity.y = 0.0;
                                return Some(Contact::Ceiling);
                            }
                        }
                    }

                    match support_below(self.position, snapshot, self.ceiling_clearance()) {
                        Some(support) if next_y >= support.y => {
                            self.position.y = support.y;
                            self.velocity = Point::default();
                            Some(Contact::Landed(support.surface))
                        }
                        _ => {
                            self.position.y = next_y;
                            Some(Contact::Airborne)
                        }
                    }
                }
            }
            State::Climbing => {
                let speed = if self.climb_paused() {
                    0.0
                } else {
                    CLIMB_SPEED
                };
                let next_y = self.position.y - speed * dt;

                // Climbing the Dock's side ends on its top: the Dock is a Perch, and the feet reaching its top edge is a landing, not a ceiling.
                // The step inward is what the climb was for — a sprite that stopped clear of the side is standing beside the Dock, not on it.
                if let Some(dock) = dock_in(snapshot) {
                    if self.position.y > dock.y && next_y <= dock.y {
                        if let Some(x) = dock_top_at(self.position.x, dock) {
                            self.position = Point { x, y: dock.y };
                            return Some(Contact::Landed(Surface::Perch));
                        }
                    }
                }
                self.position.y = next_y;

                // Off the top of the display there is nothing left to hold, so
                // it lets go. A sprite over no display at all is already
                // holding nothing, hence its own y as the fallback ceiling.
                let ceiling = ceiling_over(self.position.x, snapshot, self.ceiling_clearance())
                    .unwrap_or(self.position.y);
                if self.position.y <= ceiling {
                    self.position.y = ceiling;
                    Some(Contact::Ceiling)
                } else {
                    None
                }
            }

            // Resting is only ever resting on something. When that something
            // moves slowly the sprite Holds and rides it, a resize of the top edge included.
            // A yank, a close, or walking off the end leaves it in the air, carrying whatever speed it had.
            State::Grounded | State::Perched | State::Asleep => {
                // Rising off the surface it stood on. A Jump is the only way a resting sprite gets upward velocity.
                // Reporting the Contact lets `transition` make the fall, so no State is written outside that module.
                // The arc starts one tick later.
                if self.velocity.y < 0.0 {
                    return Some(Contact::Airborne);
                }
                let dx = self.velocity.x * dt;
                self.position.x += dx;
                if self.last_perch.is_some() {
                    self.hold_offset_x += dx;
                }

                // A walk on the floor beside the Dock stops clear of its side and climbs it.
                // Placed there once rather than corrected back a step each tick, which a held walk would re-close into a stutter.
                // The Engine does not clamp `dt`, so a step longer than the Dock is wide would cross it unseen; the snapshot assembler caps `elapsed_ms` at one poll interval, which is what keeps a step small.
                if let Some(side) = dock_side_reached(self.position, self.velocity.x, snapshot) {
                    self.position.x = side;
                    self.velocity = Point::default();
                    return Some(Contact::Wall);
                }

                let fresh = snapshot.poll_generation == 0
                    || snapshot.poll_generation != self.last_poll_generation;
                self.since_sample_s += dt;

                if fresh {
                    let sample_s = if snapshot.poll_generation == 0 {
                        dt
                    } else {
                        self.since_sample_s
                    };
                    self.since_sample_s = 0.0;
                    match self.perch_carry(snapshot, sample_s) {
                        PerchCarry::Ride(window) => {
                            self.place_on(window);
                            self.last_perch = Some(window);
                            self.riding = true;
                        }
                        PerchCarry::Still(window) => {
                            self.place_on(window);
                            self.last_perch = Some(window);
                            self.rest_perch();
                        }
                        PerchCarry::Yank => {
                            self.rest_perch();
                            return Some(Contact::Airborne);
                        }
                        PerchCarry::Lost => self.rest_perch(),
                    }
                } else if self.coasting() {
                    // The window is still moving on screen; we just have not
                    // been told yet. Stale rectangles would call this a fall.
                    self.coast_s += dt;
                    let t = self.coast_s;
                    if let Some(origin) = self.last_perch.map(|perch| perch.rect) {
                        let coasted = Point {
                            x: origin.x
                                + self.hold_offset_x
                                + self.perch_velocity.x * t
                                + 0.5 * self.perch_acceleration.x * t * t,
                            y: origin.y
                                + self.perch_velocity.y * t
                                + 0.5 * self.perch_acceleration.y * t * t,
                        };

                        // A coast places the sprite with no sample to approve it, and it runs on every late-poll tick,
                        // so an extrapolation off the displays is a long absence, not a frame of one.
                        // It lets go instead.
                        if !on_a_display(coasted, snapshot, self.ceiling_clearance()) {
                            self.rest_perch();
                            return Some(Contact::Airborne);
                        }
                        self.position = coasted;
                    }
                    self.riding = true;
                    return Some(Contact::Standing);
                }

                match footing(
                    self.position,
                    snapshot,
                    self.ceiling_clearance(),
                    |window| self.swallowed_by(window),
                ) {
                    Some(footing) if footing.y < self.position.y => {
                        self.position.y = footing.y;
                        Some(Contact::Lifted(footing.surface))
                    }
                    Some(footing) if footing.y == self.position.y => Some(Contact::Standing),
                    _ => Some(Contact::Airborne),
                }
            }
            State::Dragged => None,
        }
    }

    /// What a fresh window sample says about the Perch the sprite was on.
    fn perch_carry(&mut self, snapshot: &WorldSnapshot, sample_s: f64) -> PerchCarry {
        let Some(previous) = self
            .last_perch
            .or_else(|| perch_at(self.previous_position, &self.previous_windows))
        else {
            return PerchCarry::Lost;
        };
        let Some(index) = snapshot
            .windows
            .iter()
            .position(|window| window.id == previous.id)
        else {
            return PerchCarry::Lost;
        };
        let current = snapshot.windows[index];

        // An edge you cannot see is gone, whether the sprite was landing or already standing on it.
        // Asked at the arrival x, because both answers below place the sprite
        // and a sideways ride carries it as far as the window went.
        if !is_perch(
            index,
            self.arrival_x(current),
            snapshot,
            self.ceiling_clearance(),
        ) {
            return PerchCarry::Lost;
        }
        let delta = Point {
            x: current.rect.x - previous.rect.x,
            y: current.rect.y - previous.rect.y,
        };
        if delta.x == 0.0 && delta.y == 0.0 {
            return PerchCarry::Still(current);
        }

        let sample_s = sample_s.max(0.001);
        let velocity = Point {
            x: delta.x / sample_s,
            y: delta.y / sample_s,
        };
        let ax = (velocity.x - self.perch_velocity.x) / sample_s;
        let ay = (velocity.y - self.perch_velocity.y) / sample_s;
        let dv = (velocity.x - self.yank_reference.x).hypot(velocity.y - self.yank_reference.y);
        if dv > RIDE_ACCELERATION * YANK_WINDOW_S {
            return PerchCarry::Yank;
        }
        // From rest there is no curve yet — only a speed. Treating that
        // first sample as constant velocity avoids a fake launch that
        // snaps back when the drag holds.
        self.perch_acceleration = if self.perch_velocity.x == 0.0 && self.perch_velocity.y == 0.0 {
            Point::default()
        } else {
            Point { x: ax, y: ay }
        };
        self.perch_velocity = velocity;
        self.since_yank_ref_s += sample_s;
        if self.since_yank_ref_s >= YANK_WINDOW_S {
            self.yank_reference = velocity;
            self.since_yank_ref_s = 0.0;
        }
        self.coast_s = 0.0;
        PerchCarry::Ride(current)
    }

    fn rest_perch(&mut self) {
        self.perch_velocity = Point::default();
        self.perch_acceleration = Point::default();
        self.coast_s = 0.0;
        self.since_sample_s = 0.0;
        self.yank_reference = Point::default();
        self.since_yank_ref_s = 0.0;
    }

    fn coasting(&self) -> bool {
        self.perch_velocity.x != 0.0
            || self.perch_velocity.y != 0.0
            || self.perch_acceleration.x != 0.0
            || self.perch_acceleration.y != 0.0
    }

    /// Where `place_on` will put the feet, which is what a ride has to ask
    /// its questions about rather than about where they are.
    fn arrival_x(&self, window: Window) -> f64 {
        window.rect.x + self.hold_offset_x
    }

    /// Put the sprite back on `window` at the offset it was holding.
    fn place_on(&mut self, window: Window) {
        self.position.x = self.arrival_x(window);
        self.position.y = window.rect.y;
    }

    /// Whether `window` has come to contain the sprite this tick.
    ///
    /// Already-inside is not swallowing: a maximized window contains every smaller window, so raising it would fling the sprite to the top. Lives here because "come to" takes the previous tick, and the Engine remembers one.
    fn swallowed_by(&self, window: &Window) -> bool {
        swallows(&window.rect, self.position)
            && !self.previous_windows.iter().any(|before| {
                before.id == window.id && swallows(&before.rect, self.previous_position)
            })
    }

    /// The Primitive being played, if any. Last of `playing`, because `play`
    /// stores the sequence reversed so the one on screen is the one on top.
    fn on_screen(&self) -> Option<Primitive> {
        self.playing.last().copied()
    }

    /// Under way on foot, rather than in the air with the same speed on it.
    fn is_walking(&self) -> bool {
        matches!(self.state, State::Grounded | State::Perched) && self.velocity.x != 0.0
    }

    /// True when the Primitive on screen changed.
    fn advance(&mut self, elapsed_ms: u32) -> bool {
        let mut left = elapsed_ms;
        let mut moved_on = false;

        while !self.playing.is_empty() && left >= self.primitive_ms {
            left -= self.primitive_ms;
            self.playing.pop();
            self.primitive_ms = PRIMITIVE_MS;
            moved_on = true;
        }
        if self.playing.is_empty() {
            // Draining is the ordinary end of a Behavior and the only one that
            // is not an abort, so it drops the name here rather than through
            // `stop_playing` like every other clear site.
            self.primitive_ms = 0;
            self.playing_behavior = None;
        } else {
            self.primitive_ms -= left;
        }

        moved_on
    }

    /// Abandon whatever is playing, name included.
    ///
    /// One method rather than a `playing.clear()` at each abort site, because the name and the Primitives it was flattened from have to go together: a site that dropped only the list would leave the Engine reporting a Behavior that stopped several ticks ago.
    fn stop_playing(&mut self) {
        self.playing.clear();
        self.playing_behavior = None;
        self.primitive_ms = 0;
    }

    /// Start playing `primitives`, unless the State the sprite is in forbids any of them. True when it started.
    ///
    /// All or nothing: a Behavior is a sequence its author meant to be seen whole, and playing the half of it that fits leaves the sprite stopping mid-thought.
    fn play(&mut self, primitives: &[Primitive]) -> bool {
        if primitives.is_empty() || !self.permitted(primitives) {
            return false;
        }

        self.playing = primitives.iter().rev().copied().collect();
        // Nameless until a caller says otherwise. Most callers are the Engine
        // playing a moment of its own — a Land, a Hold, a startle — and the
        // name of the Behavior they interrupted is not theirs to keep.
        self.playing_behavior = None;

        // A new Chase is a new pursuit; leftover ms from the last one would
        // time out mid-stride.
        if primitives.contains(&Primitive::Chase) {
            self.chase_ms = 0;
        }
        self.primitive_ms = PRIMITIVE_MS;
        true
    }

    /// Whether the sprite's State permits every one of `primitives`. Motion
    /// needs the post-Poke cooldown (#177). Walk and chase also wait out a
    /// quick message or a bubble; a jump still may.
    fn permitted(&self, primitives: &[Primitive]) -> bool {
        let on_feet = matches!(self.state, State::Grounded | State::Perched);
        primitives.iter().all(|primitive| match primitive {
            Primitive::React | Primitive::Talk => true,
            Primitive::Walk | Primitive::Chase => {
                on_feet && self.poke_cooldown_ms == 0 && !self.feet_held
            }
            Primitive::Jump => on_feet && self.poke_cooldown_ms == 0,
            _ => on_feet,
        })
    }

    /// Every Primitive a named Behavior plays, the Behaviors it chains into included, or nothing when the Character does not declare it.
    ///
    /// Flattened when play starts because a Behavior is one thing to abandon and refuse. Stops a cycle anyway: the Engine is handed Behaviors rather than a validated Character, and hanging the frame loop is the one thing ADR-0002 promises no package can do.
    fn chain(&self, behavior: &str) -> Option<Vec<Primitive>> {
        let mut primitives = Vec::new();
        let mut walked: BTreeSet<&str> = BTreeSet::new();
        let mut current = self.behaviors.get_key_value(behavior)?;

        while walked.insert(current.0.as_str()) {
            primitives.extend(current.1.primitives.iter().copied());
            match current.1.then.as_deref() {
                Some(next) => match self.behaviors.get_key_value(next) {
                    Some(next) => current = next,
                    None => break,
                },
                None => break,
            }
        }

        Some(primitives)
    }
}

/// Which of the Required Animation Set a Primitive plays.
fn animation_of(primitive: Primitive) -> &'static str {
    match primitive {
        Primitive::Idle => "idle",
        // The Animation only. The motion belongs to the walk a proposal starts,
        // which outlives this Primitive's turn and ends on running out of Perch.
        Primitive::Walk => "walk",
        Primitive::Land => "land",
        Primitive::Sit => "sit",
        Primitive::Sleep => "sleep",
        Primitive::React => "react",
        Primitive::Talk => "talk",
        Primitive::Hold => "hold",
        // No chase Animation in the required set; walk is the motion.
        Primitive::Chase => "walk",
        // Optional art: the renderer resolves it to `fall` when a package
        // draws none, so the required set stays at nine (ADR-0007).
        Primitive::Jump => "jump",
    }
}

/// Where a Poke stops the sprite for the cooldown: on its feet or on a wall. Mid-air it changes nothing about the flight.
fn stopped_by_a_poke(state: State) -> bool {
    matches!(state, State::Grounded | State::Perched | State::Climbing)
}

/// Which Animation a State plays.
///
/// A grab cannot reuse `hold`, which the required set spends on riding a moving Perch. It gets an optional Animation of its own: a package that declares no `grab` goes on dangling from the cursor in its `fall`.
fn animation_for(state: State) -> &'static str {
    match state {
        State::Grounded => "idle",
        State::Falling => "fall",
        State::Dragged => "grab",
        State::Perched => "sit",
        // An optional Animation: a Character without climb art draws its walk
        // instead, resolved by the renderer, never by a missing sprite here.
        State::Climbing => "climb",
        State::Asleep => "sleep",
    }
}

fn thrown_velocity(snapshot: &WorldSnapshot) -> Option<Point> {
    snapshot.verbs.iter().find_map(|verb| match verb {
        Verb::Throw { velocity } => Some(*velocity),
        _ => None,
    })
}

/// What a fresh window sample says about the Perch the sprite was standing on.
enum PerchCarry {
    /// Dragged slowly: snap onto the new edge and coast until the next sample.
    Ride(Window),
    /// Same edge as last time: snap back if a coast overshot, then sit.
    Still(Window),
    /// Dragged hard enough to lose footing.
    Yank,
    /// Closed, minimized, or no longer somewhere the sprite can stand.
    Lost,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::DEFAULT_WEIGHT;
    use crate::window_source::DOCK_PERCH_ID;

    fn one_display() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        }
    }

    /// A window in a snapshot. Ids only have to differ, except in the tests
    /// that are about identity, which say so.
    fn window(id: WindowId, rect: Rect) -> Window {
        Window { id, rect }
    }

    fn snapshot(elapsed_ms: u32) -> WorldSnapshot {
        WorldSnapshot {
            displays: vec![one_display()],
            elapsed_ms,
            ..WorldSnapshot::default()
        }
    }

    /// Ticks long enough for anything in flight over an 800-point display to
    /// come to rest, and returns the last frame.
    fn settle(engine: &mut Engine, snapshot: &WorldSnapshot) -> Frame {
        (0..40).map(|_| engine.tick(snapshot)).last().unwrap()
    }

    /// Asked of a point rather than of a resting place.
    fn covered(position: Point, snapshot: &WorldSnapshot) -> bool {
        snapshot.displays.iter().any(|display| {
            display.spans_x(position.x) && position.y >= display.y && position.y <= display.bottom()
        })
    }

    /// A Perch on a second display too short to hold it, `x` points along.
    /// Nothing covers x 1000..2000 outside y 300..500, and nothing at all
    /// covers past x 2000, so a window dragged right runs its edge out of the displays while still spanning the sprite.
    fn strip_perch(x: f64) -> WorldSnapshot {
        WorldSnapshot {
            displays: vec![
                one_display(),
                Rect {
                    x: 1000.0,
                    y: 300.0,
                    width: 1000.0,
                    height: 200.0,
                },
            ],
            windows: vec![window(
                1,
                Rect {
                    x,
                    y: 450.0,
                    width: 600.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        }
    }

    /// A day in the life, as snapshots: the sprite falls onto a window, the
    /// window closes, it lands on the floor, dozes off, is poked awake, is
    /// picked up and carried, is dropped, then is picked up again and flung at the screen edge, which it climbs.
    fn a_day_in_the_life() -> Vec<WorldSnapshot> {
        let on_a_window = WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 50.0,
                    y: 400.0,
                    width: 300.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        };

        let mut script: Vec<WorldSnapshot> = (0..40).map(|_| on_a_window.clone()).collect();
        script.extend((0..40).map(|_| snapshot(100)));
        script.push(snapshot(60_000));
        script.push(WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });
        script.extend((0..2).map(|_| WorldSnapshot {
            cursor: Point { x: 500.0, y: 100.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        }));
        script.extend((0..10).map(|_| snapshot(100)));
        script.push(WorldSnapshot {
            cursor: Point { x: 500.0, y: 100.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        script.push(WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point {
                    x: 2000.0,
                    y: -400.0,
                },
            }],
            ..snapshot(100)
        });
        script.extend((0..60).map(|_| snapshot(100)));
        script
    }

    fn play(script: &[WorldSnapshot]) -> Vec<Frame> {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        script.iter().map(|s| engine.tick(s)).collect()
    }

    #[test]
    fn every_state_is_reached_and_every_state_is_left_again() {
        let states: Vec<State> = play(&a_day_in_the_life())
            .iter()
            .map(|frame| frame.state)
            .collect();

        for state in [
            State::Grounded,
            State::Falling,
            State::Dragged,
            State::Perched,
            State::Climbing,
            State::Asleep,
        ] {
            assert!(states.contains(&state), "{state:?} is never reached");
            assert!(
                states
                    .windows(2)
                    .any(|pair| pair[0] == state && pair[1] != state),
                "{state:?} is a dead end"
            );
        }
    }

    #[test]
    fn the_same_snapshots_twice_produce_the_same_frames() {
        let script = a_day_in_the_life();

        assert_eq!(play(&script), play(&script));
    }

    fn declared_behaviors() -> BTreeMap<String, Behavior> {
        BTreeMap::from([
            (
                "walk".to_string(),
                Behavior {
                    primitives: vec![Primitive::Walk],
                    then: None,
                    weight: DEFAULT_WEIGHT,
                    trigger: None,
                },
            ),
            (
                "greet".to_string(),
                Behavior {
                    primitives: vec![Primitive::React, Primitive::Talk],
                    then: Some("settle".to_string()),
                    weight: DEFAULT_WEIGHT,
                    trigger: None,
                },
            ),
            (
                "settle".to_string(),
                Behavior {
                    primitives: vec![Primitive::Sit, Primitive::Sleep],
                    then: None,
                    weight: DEFAULT_WEIGHT,
                    trigger: None,
                },
            ),
            (
                "jump".to_string(),
                Behavior {
                    primitives: vec![Primitive::Jump],
                    then: None,
                    weight: DEFAULT_WEIGHT,
                    trigger: None,
                },
            ),
        ])
    }

    fn a_character_at(position: Point) -> Engine {
        Engine::new(position).with_behaviors(declared_behaviors())
    }

    /// A held walk drives the sprite at the display edge on purpose: that is
    /// how it reaches the wall. Correcting a moving sprite teleports it back
    /// a step each time it closes the gap, and the wall becomes unreachable.
    #[test]
    fn a_walk_reaches_the_display_edge_without_being_set_back() {
        let mut engine = a_character_at(Point { x: 300.0, y: 0.0 });
        settle(&mut engine, &snapshot(16));
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 300.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(16)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: -200.0, y: 0.0 },
            }],
            ..snapshot(16)
        });
        settle(&mut engine, &snapshot(16));
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..snapshot(16)
        });

        let mut previous = engine.tick(&snapshot(16));
        for _ in 0..400 {
            let frame = engine.tick(&snapshot(16));
            if frame.state == State::Climbing {
                return; // the wall was reached, which is what a walk is for
            }
            assert!(
                frame.position.x <= previous.position.x,
                "walking left never moves the sprite back right: {} -> {}",
                previous.position.x,
                frame.position.x
            );
            previous = frame;
        }
        panic!("the walk never reached the display edge: {previous:?}");
    }

    /// The Dock on `one_display`, as the snapshot assembler adds it: frontmost,
    /// wearing the reserved id, reaching down to the floor.
    fn dock() -> Rect {
        Rect {
            x: 400.0,
            y: 700.0,
            width: 200.0,
            height: 100.0,
        }
    }

    fn dock_snapshot(elapsed_ms: u32) -> WorldSnapshot {
        WorldSnapshot {
            windows: vec![window(DOCK_PERCH_ID, dock())],
            ..snapshot(elapsed_ms)
        }
    }

    /// Where a climb up the Dock's left side comes to rest, and where one up
    /// its right side does: a half sprite in from the corner, on the top.
    fn on_the_dock_from_the_left() -> Point {
        Point {
            x: dock().x + EDGE_CLEARANCE,
            y: dock().y,
        }
    }

    fn on_the_dock_from_the_right() -> Point {
        Point {
            x: dock().x + dock().width - EDGE_CLEARANCE,
            y: dock().y,
        }
    }

    /// The Dock is the one thing on screen drawn in front of the sprite, so a
    /// walk that carries on under it puts the sprite where nobody can see or
    /// grab it. Its side is a wall.
    #[test]
    fn a_walk_into_the_dock_climbs_onto_it_rather_than_behind_it() {
        let dock = dock();
        let mut engine = a_character_at(Point { x: 300.0, y: 0.0 });
        settle(&mut engine, &dock_snapshot(100));
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..dock_snapshot(16)
        });

        let mut previous = engine.tick(&dock_snapshot(16));
        let mut climbed = false;
        for _ in 0..400 {
            let frame = engine.tick(&dock_snapshot(16));
            let behind = frame.position.y > dock.y
                && frame.position.x > dock.x
                && frame.position.x < dock.x + dock.width;
            assert!(!behind, "behind the Dock: {:?}", frame.position);
            if frame.state == State::Grounded {
                assert!(
                    frame.position.x >= previous.position.x,
                    "walking right never sets the sprite back: {} -> {}",
                    previous.position.x,
                    frame.position.x
                );
            }
            climbed |= frame.state == State::Climbing;
            if frame.state == State::Perched {
                assert!(climbed, "on the Dock's top without climbing its side");
                assert_eq!(frame.position, on_the_dock_from_the_left(), "{frame:?}");
                return;
            }
            previous = frame;
        }
        panic!("the walk never reached the Dock's top: {previous:?}");
    }

    /// The Dock's side is a wall for a sprite walking into it, and not for one
    /// that has just walked off the top. Catching that one puts it back on the
    /// top to walk off again, and everything past the Dock stays unreachable.
    #[test]
    fn a_walk_off_the_dock_top_carries_on_past_it_rather_than_climbing_back() {
        let dock = dock();
        let mut engine = a_character_at(Point { x: 700.0, y: 0.0 });
        settle(&mut engine, &dock_snapshot(100));

        // Thrown leftwards into the Dock's right side, which it climbs, leaving
        // it on the top facing left with the far edge ahead of it.
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 700.0, y: 760.0 },
            verbs: vec![Verb::Grab],
            ..dock_snapshot(16)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: -400.0, y: 0.0 },
            }],
            ..dock_snapshot(16)
        });
        let on_top = settle(&mut engine, &dock_snapshot(16));
        assert_eq!(
            (on_top.state, on_top.position.y),
            (State::Perched, dock.y),
            "the throw is meant to leave it on the Dock: {on_top:?}"
        );

        let mut stepped_off = false;
        for _ in 0..400 {
            let frame = engine.tick(&WorldSnapshot {
                proposal: walk(),
                ..dock_snapshot(16)
            });
            let on_dock = frame.state == State::Perched && frame.position.y == dock.y;
            stepped_off |= !on_dock;
            assert!(
                !(stepped_off && on_dock),
                "put back on the Dock it had just walked off: {frame:?}"
            );
            if frame.state == State::Climbing && frame.position.x == one_display().x {
                return; // the far wall, which is what walking that way is for
            }
        }
        panic!("the walk never got past the Dock to the display edge");
    }

    /// Standing on the Dock's top is standing above its side, not behind it.
    #[test]
    fn a_walk_along_the_dock_top_meets_no_wall() {
        let mut engine = a_character_at(Point { x: 500.0, y: 0.0 });
        let landed = settle(&mut engine, &dock_snapshot(100));
        assert_eq!(
            (landed.state, landed.position.y),
            (State::Perched, dock().y)
        );
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..dock_snapshot(16)
        });

        for _ in 0..20 {
            let frame = engine.tick(&dock_snapshot(16));
            assert_ne!(frame.state, State::Climbing, "{frame:?}");
            assert_eq!(frame.position.y, dock().y, "{frame:?}");
        }
    }

    /// Let go with the cursor over the Dock and the sprite falls behind it.
    /// It climbs the nearer side out: a real Dock is wide, and the way out
    /// from the middle of one is a long way sideways.
    #[test]
    fn a_sprite_dropped_behind_the_dock_climbs_out_onto_it() {
        let mut engine = a_character_at(Point { x: 300.0, y: 0.0 });
        settle(&mut engine, &dock_snapshot(100));
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 480.0, y: 760.0 },
            verbs: vec![Verb::Grab],
            ..dock_snapshot(16)
        });

        let frame = settle(&mut engine, &dock_snapshot(16));
        assert_eq!(frame.state, State::Perched, "{frame:?}");
        assert_eq!(frame.position, on_the_dock_from_the_left());
    }

    /// The nearer side is whichever one it is. Dropped in the right-hand half,
    /// the sprite climbs out to the right, not back across everything it was
    /// hidden behind.
    #[test]
    fn a_sprite_dropped_near_the_docks_right_end_climbs_out_that_side() {
        let mut engine = a_character_at(Point { x: 300.0, y: 0.0 });
        settle(&mut engine, &dock_snapshot(100));
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 560.0, y: 760.0 },
            verbs: vec![Verb::Grab],
            ..dock_snapshot(16)
        });

        let frame = settle(&mut engine, &dock_snapshot(16));
        assert_eq!(frame.state, State::Perched, "{frame:?}");
        assert_eq!(frame.position, on_the_dock_from_the_right());
    }

    /// An autohidden Dock slides out around whatever is standing there, asleep
    /// included. The sprite that was resting in front of nothing is now behind
    /// something, and climbs out the same way.
    #[test]
    fn a_dock_that_unhides_around_a_sleeping_sprite_wakes_it_onto_the_top() {
        let mut engine = a_character_at(Point { x: 450.0, y: 0.0 });
        settle(&mut engine, &snapshot(100));
        let asleep = engine.tick(&snapshot(SLEEP_AFTER_MS));
        assert_eq!(asleep.state, State::Asleep, "{asleep:?}");

        let frame = settle(&mut engine, &dock_snapshot(16));
        assert_eq!(frame.state, State::Perched, "{frame:?}");
        assert_eq!(frame.position, on_the_dock_from_the_left());
    }

    /// Behind the Dock means behind the Dock's own display. A display below
    /// this one shares its x-range: a sprite on that floor is under the Dock's
    /// columns and behind none of it. Treating it as hidden loops it for ever.
    #[test]
    fn a_sprite_on_a_display_below_the_docks_is_not_behind_the_dock() {
        let below = Rect {
            x: 0.0,
            y: 800.0,
            width: 1000.0,
            height: 800.0,
        };
        let stacked = WorldSnapshot {
            displays: vec![one_display(), below],
            ..dock_snapshot(16)
        };
        let standing = Point {
            x: 500.0,
            y: below.bottom(),
        };

        let mut engine = a_character_at(standing);
        for _ in 0..80 {
            let frame = engine.tick(&stacked);
            assert_ne!(frame.state, State::Climbing, "{frame:?}");
            assert_eq!(frame.position, standing, "{frame:?}");
        }
    }

    fn a_resting_sprite() -> Engine {
        let mut engine = a_character_at(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &snapshot(100));
        engine
    }

    fn proposing(behavior: &str) -> WorldSnapshot {
        WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: behavior.to_string(),
                dialogue: None,
            }),
            ..snapshot(100)
        }
    }

    fn played(engine: &mut Engine, ticks: usize) -> Vec<(&'static str, usize)> {
        let mut run: Vec<(&'static str, usize)> = Vec::new();
        for _ in 0..ticks {
            let animation = engine.tick(&snapshot(100)).animation;
            match run.last_mut() {
                Some((last, count)) if *last == animation => *count += 1,
                _ => run.push((animation, 1)),
            }
        }
        run
    }

    /// `greet` is two Primitives and then the two of `settle`, so a chain is
    /// played as one Behavior rather than stopping at the word that joins them.
    #[test]
    fn a_behavior_plays_its_primitives_in_order_and_follows_the_one_it_names() {
        let mut engine = a_resting_sprite();

        assert_eq!(
            engine.tick(&proposing("greet")).animation,
            "react",
            "the proposal is applied on the tick it arrives"
        );

        assert_eq!(
            played(&mut engine, 29),
            [
                ("react", 5),
                ("talk", 6),
                ("sit", 6),
                ("sleep", 6),
                ("idle", 6),
            ],
            "each Primitive holds the screen for as long as the next, \
             and the sprite goes back to idling when the Behavior ends"
        );
    }

    /// Landing is not a State: the sprite is standing the moment it arrives,
    /// so the end of a fall is played as a Primitive over the standing.
    #[test]
    fn a_fall_ends_in_the_landing_animation_before_the_sprite_idles() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });

        let landed = (0..40)
            .map(|_| engine.tick(&snapshot(100)))
            .find(|frame| frame.state == State::Grounded)
            .expect("a sprite dropped over a display lands on it");
        assert_eq!(landed.animation, "land");

        assert_eq!(
            played(&mut engine, 10),
            [("land", 5), ("idle", 5)],
            "and the landing gives way to idling rather than holding"
        );
    }

    /// The window it was sitting on closes mid-Behavior, and sitting in
    /// mid-air is not a thing the sprite can be doing.
    #[test]
    fn a_behavior_that_becomes_invalid_mid_play_is_abandoned() {
        let window = window(
            1,
            Rect {
                x: 50.0,
                y: 400.0,
                width: 300.0,
                height: 200.0,
            },
        );
        let on_a_window = WorldSnapshot {
            windows: vec![window],
            ..snapshot(100)
        };

        let mut engine = a_character_at(Point { x: 100.0, y: 0.0 });
        assert_eq!(settle(&mut engine, &on_a_window).state, State::Perched);

        assert_eq!(
            engine
                .tick(&WorldSnapshot {
                    windows: vec![window],
                    ..proposing("settle")
                })
                .animation,
            "sit"
        );

        let fell = engine.tick(&snapshot(100));
        assert_eq!(fell.state, State::Falling);
        assert_eq!(fell.animation, "fall", "it stops sitting the moment it can");

        let after = played(&mut engine, 40);
        assert!(
            !after
                .iter()
                .any(|(animation, _)| ["sit", "sleep"].contains(animation)),
            "and the rest of the Behavior never comes back: {after:?}"
        );
    }

    /// Refused rather than deferred: a Behavior proposed for a sprite in
    /// mid-air was proposed for a sprite that no longer exists by the time it lands.
    #[test]
    fn a_behavior_the_state_forbids_is_refused() {
        let mut engine = a_character_at(Point { x: 100.0, y: 0.0 });
        let falling = engine.tick(&proposing("settle"));

        assert_eq!(falling.state, State::Falling);
        assert_eq!(falling.animation, "fall", "it goes on falling instead");
        assert_eq!(
            falling.refused.as_deref(),
            Some("settle"),
            "the report covers every Behavior, not the Jump alone (#374)"
        );

        let after = played(&mut engine, 40);
        assert!(
            !after
                .iter()
                .any(|(animation, _)| ["sit", "sleep"].contains(animation)),
            "and landing does not start what was refused: {after:?}"
        );
    }

    /// The gate is one rule for every State that is not standing on something:
    /// asleep is a thing to be woken out of rather than acted from, and a climb
    /// has no more floor than a fall. `settle` opens on `sit`, which neither draws.
    #[test]
    fn a_behavior_that_settles_is_refused_asleep_and_mid_climb() {
        let mut engine = a_resting_sprite();
        assert_eq!(engine.tick(&snapshot(60_000)).state, State::Asleep);

        let asleep = engine.tick(&proposing("settle"));
        assert_eq!(asleep.state, State::Asleep);
        assert_eq!(asleep.animation, "sleep", "it stays asleep instead");

        let mut engine = a_character_at(Point { x: 900.0, y: 400.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        assert_eq!(
            engine
                .tick(&WorldSnapshot {
                    verbs: vec![Verb::Throw {
                        velocity: Point { x: 2000.0, y: 0.0 },
                    }],
                    ..snapshot(100)
                })
                .state,
            State::Climbing
        );

        let climbing = engine.tick(&proposing("settle"));
        assert_eq!(climbing.state, State::Climbing);
        assert_eq!(climbing.animation, "climb", "it goes on climbing");
    }

    /// Expression is the exception: being startled or speaking says nothing
    /// about where the sprite's feet are, so a Poke is answered mid-fall and so
    /// is a Behavior made only of those.
    #[test]
    fn a_behavior_of_expression_alone_plays_in_any_state() {
        let mut engine =
            Engine::new(Point { x: 100.0, y: 0.0 }).with_behaviors(BTreeMap::from([(
                "chatter".to_string(),
                Behavior {
                    primitives: vec![Primitive::Talk],
                    then: None,
                    weight: DEFAULT_WEIGHT,
                    trigger: None,
                },
            )]));

        let falling = engine.tick(&proposing("chatter"));
        assert_eq!(falling.state, State::Falling);
        assert_eq!(falling.animation, "talk");
    }

    /// Load-time validation rejects a chain that comes back on itself, so this
    /// is the second lock: the Engine is handed Behaviors rather than a
    /// validated Character, and ADR-0002 promises no package can hang the frame loop.
    #[test]
    fn a_behavior_that_chains_back_to_itself_still_ends() {
        let pacing = |then: &str| Behavior {
            primitives: vec![Primitive::Idle],
            then: Some(then.to_string()),
            weight: DEFAULT_WEIGHT,
            trigger: None,
        };
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 }).with_behaviors(BTreeMap::from([
            ("here".to_string(), pacing("there")),
            ("there".to_string(), pacing("here")),
        ]));
        settle(&mut engine, &snapshot(100));

        assert_eq!(engine.tick(&proposing("here")).animation, "idle");
        assert_eq!(
            played(&mut engine, 20),
            [("idle", 20)],
            "each Behavior of the loop is played once and the sprite goes back \
             to idling, rather than the tick never returning"
        );
    }

    /// A proposal the Character cannot play is refused, and refusing it is not
    /// an interruption: the Director names a Behavior, and a Character that was
    /// swapped or is simply older may not declare it.
    #[test]
    fn an_unknown_behavior_is_refused_without_disrupting_what_is_playing() {
        let mut engine = a_resting_sprite();
        engine.tick(&proposing("greet"));

        let unknown = engine.tick(&proposing("cartwheel"));
        assert_eq!(unknown.animation, "react", "still greeting");
        assert_eq!(
            unknown.animation_ms, 100,
            "and on its own clock, not restarted by the refusal"
        );
    }

    /// The Shell suppresses Behaviors the user has recently seen. A proposal
    /// is advisory, so what was proposed and what was played are different
    /// lists, and only the Engine knows the second.
    #[test]
    fn a_frame_names_the_behavior_that_started_and_not_one_that_was_refused() {
        let mut engine =
            Engine::new(Point { x: 100.0, y: 0.0 }).with_behaviors(declared_behaviors());

        let airborne = engine.tick(&proposing("settle"));
        assert_eq!(airborne.state, State::Falling);
        assert_eq!(
            airborne.behavior, None,
            "there is no sitting down in mid-air, so nobody saw it"
        );

        settle(&mut engine, &snapshot(100));
        assert_eq!(
            engine.tick(&proposing("settle")).behavior.as_deref(),
            Some("settle"),
            "on the floor it plays, and the Shell may remember it"
        );
        assert_eq!(
            engine.tick(&snapshot(100)).behavior,
            None,
            "starting is an event, not a state to hold"
        );
        assert_eq!(
            engine.tick(&proposing("cartwheel")).behavior,
            None,
            "nor does a Behavior nobody declares count as played"
        );
    }

    /// The other half of the pair above: `behavior` is the tick a proposal was
    /// taken, `playing_behavior` is every tick it runs for. A developer asking
    /// what the sprite is doing gets one answer per tick, not one per Behavior.
    #[test]
    fn the_playing_behavior_is_reported_for_its_whole_span_and_not_past_it() {
        let mut engine = a_resting_sprite();

        // `greet` is React and Talk and chains into `settle`'s Sit and Sleep:
        // four Primitives of PRIMITIVE_MS, so 100ms ticks 0..=23 are the span.
        let started = engine.tick(&proposing("greet"));
        assert_eq!(started.behavior.as_deref(), Some("greet"));
        assert_eq!(started.playing_behavior.as_deref(), Some("greet"));
        assert_eq!(started.playing_primitive, Some(Primitive::React));

        for tick in 1..24 {
            let frame = engine.tick(&snapshot(100));
            assert_eq!(
                frame.playing_behavior.as_deref(),
                Some("greet"),
                "tick {tick} is still greeting"
            );
            assert_eq!(frame.behavior, None, "tick {tick} started nothing");
        }

        let over = engine.tick(&snapshot(100));
        assert_eq!(
            over.playing_behavior, None,
            "the chain ran out, so nothing is playing"
        );
        assert_eq!(over.playing_primitive, None);
    }

    /// Losing its footing abandons the rest of a Behavior, and the name has to
    /// go with the Primitives: a Behavior reported as playing after the hand
    /// took the sprite away is a trace that lies about the screen.
    #[test]
    fn a_grab_mid_behavior_drops_the_behavior_with_it() {
        let mut engine = a_resting_sprite();
        assert_eq!(
            engine.tick(&proposing("greet")).playing_behavior.as_deref(),
            Some("greet")
        );

        let grabbed = engine.tick(&WorldSnapshot {
            cursor: Point { x: 400.0, y: 200.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        assert_eq!(grabbed.state, State::Dragged);
        assert_eq!(grabbed.playing_behavior, None);
        assert_eq!(grabbed.playing_primitive, None);
    }

    /// A Land, a Hold and the answer to a Poke are the Engine's own: no
    /// Director proposes one in time, so there is no name to report. The
    /// interrupted Behavior's name is not it — that Behavior is over.
    #[test]
    fn an_engine_played_moment_reports_no_behavior() {
        let mut engine = a_resting_sprite();
        assert_eq!(
            engine
                .tick(&proposing("settle"))
                .playing_behavior
                .as_deref(),
            Some("settle")
        );

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });
        assert_eq!(poked.playing_primitive, Some(Primitive::React));
        assert_eq!(poked.playing_behavior, None);
    }

    #[test]
    fn a_proposals_dialogue_is_spoken_once_and_not_repeated() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });

        let spoken = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "greet".to_string(),
                dialogue: Some("morning".to_string()),
            }),
            ..snapshot(100)
        });
        assert_eq!(spoken.dialogue.as_deref(), Some("morning"));

        let quiet = engine.tick(&snapshot(100));
        assert_eq!(quiet.dialogue, None, "a line is said once, not held");
    }

    #[test]
    fn the_frame_names_the_animation_and_how_long_it_has_been_playing() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });

        let first = engine.tick(&snapshot(100));
        assert_eq!(first.animation, "fall");
        assert_eq!(first.animation_ms, 100);

        let second = engine.tick(&snapshot(100));
        assert_eq!(second.animation_ms, 200, "still falling, still accruing");

        let landed = settle(&mut engine, &snapshot(100));
        assert_eq!(landed.animation, "idle");

        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        engine.tick(&snapshot(1_000));
        let restarted = engine.tick(&snapshot(100));
        assert_eq!(restarted.animation, "land", "it landed in that first tick");
        assert_eq!(
            restarted.animation_ms, 100,
            "a new animation starts its own clock rather than inheriting one"
        );
    }

    /// The Engine learns nothing about menu bars here. It is handed the usable
    /// part of the display instead of the whole of it, and the ceiling it
    /// already derives moves with it.
    #[test]
    fn a_climb_ends_at_the_usable_top_rather_than_behind_the_menu_bar() {
        // A display reserving 30 points at the top, as a menu bar does.
        let usable = || WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 30.0,
                width: 1000.0,
                height: 770.0,
            }],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        };

        let mut engine = Engine::new(Point { x: 900.0, y: 400.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..usable()
        });
        let caught = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..usable()
        });
        assert_eq!(caught.state, State::Climbing);

        let highest = (0..200)
            .map(|_| engine.tick(&usable()).position.y)
            .fold(f64::INFINITY, f64::min);
        assert_eq!(
            highest, 158.0,
            "it lets go with the art still on the usable frame, not at \
             the display's own top of 0"
        );

        let landed = settle(&mut engine, &usable());
        assert_eq!(
            landed.position.y, 800.0,
            "and falls to the usable floor, which this display does not inset"
        );
    }

    /// A second Poke restarts the reaction rather than extending a held frame.
    /// `react` is `loop = once`, so without restarting the clock, prodding twice
    /// would look exactly like prodding once.
    #[test]
    fn poking_again_mid_reaction_starts_the_reaction_over() {
        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        settle(&mut engine, &snapshot(100));

        let poke = || WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        };
        engine.tick(&poke());
        let mid = engine.tick(&snapshot(100));
        assert_eq!(mid.animation, "react");
        assert!(mid.animation_ms > 0, "the reaction has been running");

        let again = engine.tick(&poke());
        assert_eq!(again.animation, "react");
        assert_eq!(
            again.animation_ms, 0,
            "and the second Poke plays it from its first frame"
        );
    }

    /// `PRIMITIVE_MS` is how long the Engine gives a Primitive, not how long
    /// the art runs. Every base `idle` is longer — twelve seconds for Black Mage,
    /// shortest 750ms — so a restarted clock would draw only the opening of the strip.
    #[test]
    fn a_behaviors_idle_keeps_one_clock_across_its_primitive_turns() {
        let mut engine =
            Engine::new(Point { x: 100.0, y: 0.0 }).with_behaviors(BTreeMap::from([(
                "ponder".to_string(),
                Behavior {
                    primitives: vec![Primitive::Idle, Primitive::Idle, Primitive::Idle],
                    then: None,
                    weight: DEFAULT_WEIGHT,
                    trigger: None,
                },
            )]));
        let base = settle(&mut engine, &snapshot(100)).animation_ms;
        assert!(base > 0, "the sprite has been idling since it landed");

        let started = engine.tick(&proposing("ponder"));
        assert_eq!(started.behavior.as_deref(), Some("ponder"), "it plays");

        let mut clock = vec![started.animation_ms];
        for _ in 0..18 {
            let frame = engine.tick(&snapshot(100));
            assert_eq!(frame.animation, "idle", "and idles throughout");
            clock.push(frame.animation_ms);
        }

        assert_eq!(
            clock,
            (1..=19).map(|tick| base + tick * 100).collect::<Vec<u32>>(),
            "three Primitive turns of the same Animation are one loop, and the \
             idle it interrupted was already that Animation"
        );
    }

    /// The ride replays Hold for as long as the Perch keeps moving, and
    /// nim's `hold` is a second of `loop = once`. A clock the replay restarted
    /// would leave the grip it ends on unreachable.
    #[test]
    fn a_long_ride_holds_on_one_clock() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        let mut clock = Vec::new();
        for tick in 0..12 {
            let frame = engine.tick(&perch(50.0, 400.0 - f64::from(tick + 1) * 10.0));
            assert!(frame.riding, "the Perch is still moving on tick {tick}");
            assert_eq!(frame.animation, "hold");
            clock.push(frame.animation_ms);
        }

        assert_eq!(
            clock,
            (0..12).map(|tick| tick * 100).collect::<Vec<u32>>(),
            "one grip, held for the whole ride"
        );
    }

    /// The cursor's own startle is a startle. Reaching at a sprite that is
    /// still mid-`react` has to look like a second reach, so the `loop = once`
    /// art plays again from frame 0 rather than holding the clamped frame.
    #[test]
    fn a_cursor_reaction_starts_a_reaction_already_playing_over() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::React, CursorReaction::Indifferent);
        let near = Point {
            x: engine.position.x + 50.0,
            y: engine.position.y,
        };
        let away = Point {
            x: engine.position.x + 300.0,
            y: engine.position.y,
        };

        let first = engine.tick(&WorldSnapshot {
            cursor: near,
            ..snapshot(100)
        });
        assert_eq!(first.animation, "react");
        let leaving = engine.tick(&WorldSnapshot {
            cursor: away,
            ..snapshot(100)
        });
        assert_eq!(leaving.animation_ms, 100, "the reaction has been running");

        let again = engine.tick(&WorldSnapshot {
            cursor: near,
            ..snapshot(100)
        });
        assert_eq!(again.animation, "react");
        assert_eq!(again.animation_ms, 0, "and it startles again");
    }

    /// Speaking is not startling. A second approach while the first line is
    /// still on screen is a hitch with nothing behind it — `talk` loops, so
    /// there is no held frame to escape.
    #[test]
    fn a_cursor_reaction_lets_a_talk_already_playing_run_on() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Speak, CursorReaction::Indifferent);
        let near = Point {
            x: engine.position.x + 50.0,
            y: engine.position.y,
        };
        let away = Point {
            x: engine.position.x + 300.0,
            y: engine.position.y,
        };

        let first = engine.tick(&WorldSnapshot {
            cursor: near,
            ..snapshot(100)
        });
        assert_eq!(first.animation, "talk");
        engine.tick(&WorldSnapshot {
            cursor: away,
            ..snapshot(100)
        });

        let again = engine.tick(&WorldSnapshot {
            cursor: near,
            ..snapshot(100)
        });
        assert_eq!(again.animation, "talk");
        assert_eq!(again.animation_ms, 200, "one line, still being said");
    }

    /// Verbs in the same tick resolve deterministically. A Grab and a Poke
    /// together is the ordinary case of a press becoming a drag, and the hand
    /// has to win, because a sprite that reacts instead of being picked up is a sprite that ignored you.
    #[test]
    fn a_grab_and_a_poke_in_one_tick_resolve_the_same_way_every_time() {
        let together = || WorldSnapshot {
            cursor: Point { x: 400.0, y: 200.0 },
            verbs: vec![Verb::Poke, Verb::Grab],
            ..snapshot(100)
        };
        let reversed = || WorldSnapshot {
            verbs: vec![Verb::Grab, Verb::Poke],
            ..together()
        };

        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        let first = engine.tick(&together());

        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        let second = engine.tick(&reversed());

        assert_eq!(
            first.state,
            State::Dragged,
            "the hand wins over the reaction"
        );
        assert_eq!(first.position, Point { x: 400.0, y: 200.0 });
        assert_eq!(
            first, second,
            "and the order the verbs arrive in changes nothing"
        );
    }

    /// A Grab overrides any State. The hand is the one input that outranks
    /// everything else the sprite might be doing, because a companion you
    /// cannot pick up whenever you like is furniture.
    #[test]
    fn a_grab_takes_the_sprite_out_of_whatever_state_it_was_in() {
        let day = a_day_in_the_life();

        for state in [
            State::Grounded,
            State::Falling,
            State::Dragged,
            State::Perched,
            State::Climbing,
            State::Asleep,
        ] {
            // Replay the day only as far as the first tick in this State, so
            // the Grab lands on a sprite that is genuinely in it.
            let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
            assert!(
                day.iter().any(|s| engine.tick(s).state == state),
                "{state:?} is never reached, so the Grab is untested from it"
            );

            let grabbed = engine.tick(&WorldSnapshot {
                cursor: Point { x: 640.0, y: 360.0 },
                verbs: vec![Verb::Grab],
                ..snapshot(100)
            });

            assert_eq!(grabbed.state, State::Dragged, "grabbed while {state:?}");
            assert_eq!(grabbed.position, Point { x: 640.0, y: 360.0 });
        }
    }

    /// Summon opens the chat surface and Menu opens the tray's menu; neither
    /// moves the sprite. They are still the user reaching for it, so a sleeping
    /// one wakes, and the Director hears it.
    #[test]
    fn a_summon_or_a_menu_wakes_the_sprite_and_addresses_the_director() {
        for verb in [Verb::Summon, Verb::Menu] {
            let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
            let resting = settle(&mut engine, &snapshot(100));
            assert_eq!(resting.state, State::Grounded);

            // A full minute of nobody touching it.
            assert_eq!(engine.tick(&snapshot(60_000)).state, State::Asleep);

            let addressed = engine.tick(&WorldSnapshot {
                cursor: Point { x: 900.0, y: 700.0 },
                verbs: vec![verb],
                ..snapshot(100)
            });

            assert_eq!(addressed.state, State::Grounded, "awake, after {verb:?}");
            assert_eq!(
                addressed.position, resting.position,
                "{verb:?} does not walk the sprite to the cursor"
            );
            assert!(
                addressed.addressed,
                "{verb:?} is the user reaching for the sprite"
            );
        }
    }

    /// A double-click is a Poke and then a Summon, and the Summon stands
    /// where the second click's Poke used to. It plays that reaction, or a
    /// double-click would visibly do less than a single click.
    #[test]
    fn a_summon_plays_the_reaction() {
        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        settle(&mut engine, &snapshot(100));

        let summoned = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Summon],
            ..snapshot(100)
        });
        assert_eq!(summoned.animation, "react");
    }

    /// The cue is the Engine's to pick: it is the only place that knows both
    /// the verbs and the `Dragged` transitions. A click verb's cue is a pulse,
    /// or a single click would sound for as long as the reaction plays.
    #[test]
    fn a_click_verb_carries_its_own_cue() {
        for (verb, cue) in [
            (Verb::Poke, Cue::Poke),
            (Verb::Summon, Cue::Summon),
            (Verb::Menu, Cue::Menu),
        ] {
            let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
            settle(&mut engine, &snapshot(100));

            let clicked = engine.tick(&WorldSnapshot {
                verbs: vec![verb],
                ..snapshot(100)
            });
            assert_eq!(clicked.cue, Some(cue), "{verb:?}");

            let after = engine.tick(&snapshot(100));
            assert_eq!(after.cue, None, "and rides one tick only, after {verb:?}");
        }
    }

    /// The Shell re-injects `Verb::Menu` every tick the popup is held, the
    /// way `Verb::Grab` is present for a whole drag. A cue keyed on the verb
    /// would sound sixty times a second; only the press edge cues.
    #[test]
    fn the_menu_cue_fires_on_the_press_and_not_every_held_tick() {
        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        settle(&mut engine, &snapshot(100));

        let held = WorldSnapshot {
            verbs: vec![Verb::Menu],
            ..snapshot(100)
        };
        let opened = engine.tick(&held);
        assert_eq!(opened.cue, Some(Cue::Menu));

        let carried: Vec<Option<Cue>> = (0..5).map(|_| engine.tick(&held).cue).collect();
        assert!(
            carried.iter().all(Option::is_none),
            "held, and cued once: {carried:?}"
        );

        let _ = engine.tick(&snapshot(100));
        let again = engine.tick(&held);
        assert_eq!(again.cue, Some(Cue::Menu), "a later press after a gap");
    }

    /// `Verb::Grab` is present on every tick the sprite is held, so a cue
    /// keyed on the verb would sound sixty times a second for as long as the
    /// drag lasts. The pickup cue keys on entering `Dragged`.
    #[test]
    fn the_pickup_cue_fires_on_the_transition_and_not_every_held_tick() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &snapshot(100));

        let held = WorldSnapshot {
            cursor: Point { x: 200.0, y: 200.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        };
        let grabbed = engine.tick(&held);
        assert_eq!(grabbed.state, State::Dragged);
        assert_eq!(grabbed.cue, Some(Cue::Pickup));
        assert!(grabbed.addressed, "a pickup addresses the Director");
        assert!(!engine.tick(&held).addressed, "a carry does not");

        let carried: Vec<Option<Cue>> = (0..5).map(|_| engine.tick(&held).cue).collect();
        assert!(
            carried.iter().all(Option::is_none),
            "held, and cued once: {carried:?}"
        );
    }

    /// There is no drop verb and a sixth is not allowed, so both cues key
    /// on leaving `Dragged` — which is also what tells them apart, a throw
    /// being the only one of the two that carries a velocity.
    #[test]
    fn letting_go_cues_a_drop_and_letting_go_moving_cues_a_throw() {
        for (release, cue) in [
            (Vec::new(), Cue::Drop),
            (
                vec![Verb::Throw {
                    velocity: Point {
                        x: 400.0,
                        y: -200.0,
                    },
                }],
                Cue::Throw,
            ),
        ] {
            let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
            settle(&mut engine, &snapshot(100));
            engine.tick(&WorldSnapshot {
                cursor: Point { x: 200.0, y: 200.0 },
                verbs: vec![Verb::Grab],
                ..snapshot(100)
            });

            let released = engine.tick(&WorldSnapshot {
                cursor: Point { x: 200.0, y: 200.0 },
                verbs: release.clone(),
                ..snapshot(100)
            });
            assert_eq!(released.state, State::Falling);
            assert_eq!(released.cue, Some(cue), "released with {release:?}");
            assert_eq!(
                released.addressed,
                cue == Cue::Throw,
                "only a Throw addresses"
            );

            let falling = engine.tick(&snapshot(100));
            assert_eq!(falling.cue, None, "and once, after {release:?}");
        }
    }

    /// The tray menu opening is its response, and a reaction under a context
    /// menu would be a sprite gesturing at a list.
    #[test]
    fn a_menu_plays_nothing() {
        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        settle(&mut engine, &snapshot(100));

        let menu_opened = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Menu],
            ..snapshot(100)
        });
        assert_eq!(menu_opened.animation, "idle");
    }

    /// Menu interrupts what the sprite is doing, not where it is going: the
    /// walk it was on carries on. This matches Poke: both acknowledge the
    /// user without stopping the sprite's motion.
    #[test]
    fn a_menu_mid_stroll_does_not_stop_the_walk() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let under_way = engine.tick(&a_long_perch());
        assert_eq!(under_way.velocity.x, WALK_SPEED);

        let menu_opened = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Menu],
            ..a_long_perch()
        });
        assert_eq!(
            menu_opened.velocity.x, WALK_SPEED,
            "the walk continues while the menu is open"
        );

        let strolling: Vec<Frame> = (0..12).map(|_| engine.tick(&a_long_perch())).collect();
        assert!(
            strolling.iter().all(|frame| frame.velocity.x == WALK_SPEED),
            "and keeps going after the menu is dismissed: {strolling:?}"
        );
    }

    /// The caret in a quick message is a hold, not a menu: the walk stops
    /// where it is, and a jump is still how the sprite leaves.
    #[test]
    fn typing_a_quick_message_stops_the_walk_in_place() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let under_way = engine.tick(&a_long_perch());
        assert_eq!(under_way.velocity.x, WALK_SPEED);
        let x = under_way.position.x;

        let held = engine.tick(&WorldSnapshot {
            composing: true,
            ..a_long_perch()
        });
        assert_eq!(
            held.velocity.x, 0.0,
            "the first composing tick does not step"
        );
        assert_eq!(held.position.x, x);
        assert_ne!(held.animation, "walk");
        assert_eq!(held.state, State::Perched);

        let refused = engine.tick(&WorldSnapshot {
            composing: true,
            proposal: walk(),
            ..a_long_perch()
        });
        let still = engine.tick(&WorldSnapshot {
            composing: true,
            ..a_long_perch()
        });
        assert_eq!(refused.velocity.x, 0.0);
        assert_eq!(still.velocity.x, 0.0, "a new walk is refused while typing");
        assert_eq!(still.position.x, x);

        let resumed = engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        assert_eq!(
            resumed.velocity.x, 0.0,
            "the proposal's step is the next tick"
        );
        let stepping = engine.tick(&a_long_perch());
        assert_eq!(stepping.velocity.x, WALK_SPEED);
    }

    #[test]
    fn typing_a_quick_message_still_lets_a_jump_leave() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        engine.tick(&WorldSnapshot {
            composing: true,
            proposal: Some(BehaviorProposal {
                behavior: "jump".to_string(),
                dialogue: None,
            }),
            ..a_long_perch()
        });
        let jumped = engine.tick(&WorldSnapshot {
            composing: true,
            ..a_long_perch()
        });
        assert!(
            jumped.velocity.y < 0.0,
            "typing does not pin a jump, got {jumped:?}"
        );
    }

    /// Menu during a drag behaves like Poke: the sprite stays grabbed. The
    /// menu blocks, so the drag is paused while it is shown, but releasing the
    /// button after dismissing the menu still ends the Grab.
    #[test]
    fn a_menu_during_a_drag_does_not_drop_the_sprite() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &snapshot(100));

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 100.0, y: 0.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        let dragging = engine.tick(&WorldSnapshot {
            cursor: Point { x: 200.0, y: 0.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        assert_eq!(dragging.state, State::Dragged);

        let menu_opened = engine.tick(&WorldSnapshot {
            cursor: Point { x: 200.0, y: 0.0 },
            verbs: vec![Verb::Menu, Verb::Grab],
            ..snapshot(100)
        });
        assert_eq!(
            menu_opened.state,
            State::Dragged,
            "the sprite stays grabbed while the menu is open"
        );

        let released = engine.tick(&WorldSnapshot {
            cursor: Point { x: 200.0, y: 0.0 },
            verbs: Vec::new(),
            ..snapshot(100)
        });
        assert_eq!(
            released.state,
            State::Falling,
            "releasing after dismissing the menu drops it as usual"
        );
    }

    /// Menu during a chase behaves like Poke during a walk: the chase is
    /// interrupted (aborted), but the sprite's current velocity persists.
    /// This pins the Menu verb's contract against the chase Primitive.
    #[test]
    fn a_menu_during_a_chase_aborts_the_chase() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        engine.play(&[Primitive::Chase]);

        let chasing = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: 500.0,
                y: engine.position.y,
            },
            ..a_long_perch()
        });
        assert_eq!(chasing.animation, "walk", "chase is under way");
        assert!(
            chasing.velocity.x.abs() > 0.0,
            "sprite is moving toward cursor"
        );

        let menu_opened = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: 500.0,
                y: engine.position.y,
            },
            verbs: vec![Verb::Menu],
            ..a_long_perch()
        });
        assert!(
            engine.on_screen() != Some(Primitive::Chase),
            "chase is aborted by Menu"
        );
        assert!(
            menu_opened.velocity.x.abs() > 0.0,
            "but velocity persists like during walk interrupt"
        );
    }

    /// A wake is not an arrival only because the sprite is put straight back
    /// on the footing it fell asleep on. Woken with that footing gone, it is
    /// in the air, and the landing at the end of the fall is a real one.
    #[test]
    fn a_sprite_woken_with_its_perch_gone_still_lands() {
        let perch = window(
            2,
            Rect {
                x: 0.0,
                y: 400.0,
                width: 1000.0,
                height: 200.0,
            },
        );
        let resting = WorldSnapshot {
            windows: vec![perch],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 500.0, y: 0.0 });
        assert_eq!(settle(&mut engine, &resting).state, State::Perched);
        assert_eq!(
            engine
                .tick(&WorldSnapshot {
                    elapsed_ms: 60_000,
                    ..resting
                })
                .state,
            State::Asleep
        );

        // Woken by a Summon in the tick the window it stood on goes.
        let woken = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Summon],
            ..snapshot(100)
        });
        assert_eq!(woken.state, State::Falling, "there is nothing under it");
        assert_ne!(woken.animation, "land", "it has not arrived anywhere yet");

        let arrived = (0..40)
            .map(|_| engine.tick(&snapshot(100)))
            .find(|frame| frame.state == State::Grounded)
            .expect("it reaches the floor");
        assert_eq!(
            arrived.animation, "land",
            "and the floor it reaches is an arrival like any other"
        );
    }

    /// A proposal during a Grab is deferred or dropped, never yanking the
    /// sprite. `settle` is a Behavior this Character does declare, so what
    /// refuses it is the State gate and not the name.
    #[test]
    fn a_proposal_during_a_grab_never_moves_the_sprite() {
        let mut engine = a_character_at(Point { x: 500.0, y: 100.0 });
        let held = WorldSnapshot {
            cursor: Point { x: 300.0, y: 300.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        };
        engine.tick(&held);

        let proposed = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "settle".to_string(),
                dialogue: Some("off we go".to_string()),
            }),
            ..held.clone()
        });

        assert_eq!(proposed.state, State::Dragged, "still in the hand");
        assert_eq!(
            proposed.position,
            Point { x: 300.0, y: 300.0 },
            "and exactly where the cursor left it"
        );
        assert_eq!(
            proposed.animation, "grab",
            "it hangs from the cursor rather than sitting down in mid-air"
        );
        assert_eq!(
            proposed.dialogue.as_deref(),
            Some("off we go"),
            "it may still speak, which moves nothing"
        );
    }

    /// Held and tumbling are different beats, so they name different
    /// Animations. The Engine asks for `grab`; a package that declares none
    /// draws its `fall` instead, which the loader asserts in `character.rs`.
    #[test]
    fn a_grab_and_a_fall_ask_for_different_animations() {
        let mut engine = a_character_at(Point { x: 500.0, y: 100.0 });

        let falling = engine.tick(&snapshot(100));
        assert_eq!(falling.state, State::Falling);
        assert_eq!(falling.animation, "fall");

        let grabbed = engine.tick(&WorldSnapshot {
            cursor: Point { x: 300.0, y: 300.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        assert_eq!(grabbed.state, State::Dragged);
        assert_eq!(
            grabbed.animation, "grab",
            "being picked up is neither tumbling nor `hold`, which #98 spent \
             the ninth required slot on and ADR-0007 keeps for a Perch ride"
        );

        let thrown = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 0.0, y: -100.0 },
            }],
            ..snapshot(100)
        });
        assert_eq!(thrown.state, State::Falling);
        assert_eq!(thrown.animation, "fall", "let go, it is tumbling again");
    }

    /// A held sprite may be taken below the usable floor, over the Dock,
    /// because the cursor may go there. Letting go puts it back somewhere
    /// it can stand.
    #[test]
    fn a_sprite_dropped_below_the_usable_floor_settles_back_onto_it() {
        // A display whose usable part stops short of its bottom edge, as one
        // with a Dock does.
        let usable = || WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 700.0,
            }],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        };

        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        let held = engine.tick(&WorldSnapshot {
            cursor: Point { x: 500.0, y: 900.0 },
            verbs: vec![Verb::Grab],
            ..usable()
        });
        assert_eq!(held.position.y, 900.0, "the hand may take it over the Dock");

        let landed = settle(&mut engine, &usable());
        assert_eq!(
            landed.position.y, 700.0,
            "and letting go settles it on the usable floor"
        );
        assert_eq!(landed.state, State::Grounded);
    }

    /// A Poke is the one interaction that has to be visible, and the
    /// Required Animation Set carries `react` for it.
    #[test]
    fn a_poke_plays_the_reaction_and_then_goes_back_to_what_it_was_doing() {
        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        let resting = settle(&mut engine, &snapshot(100));
        assert_eq!(resting.animation, "idle");

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });
        assert_eq!(poked.animation, "react");
        assert_eq!(
            poked.state,
            State::Grounded,
            "reacting is not a State: it is still standing where it stood"
        );

        assert_eq!(
            engine.tick(&snapshot(100)).animation,
            "react",
            "and it lasts longer than the tick it started on"
        );

        let after = (0..20)
            .map(|_| engine.tick(&snapshot(100)))
            .last()
            .expect("twenty ticks produce twenty frames");
        assert_eq!(after.animation, "idle", "then back to idling");
    }

    #[test]
    fn a_poke_while_falling_reacts_without_interrupting_the_fall() {
        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        let falling = engine.tick(&snapshot(100));
        assert_eq!(falling.state, State::Falling);

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });
        assert_eq!(poked.animation, "react");
        assert_eq!(poked.state, State::Falling);
        assert!(
            poked.position.y > falling.position.y,
            "still going down: {poked:?}"
        );
    }

    /// A Poke reacts on screen at once and addresses the Director only once it
    /// settles, when no second click can make the pair a Summon.
    #[test]
    fn a_poke_reacts_at_once_and_addresses_the_director_once_settled() {
        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 });
        settle(&mut engine, &snapshot(100));

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });
        assert!(!poked.addressed, "not yet: it may be half a Summon");
        let settled = engine.tick(&WorldSnapshot {
            poke_settled: true,
            ..snapshot(100)
        });
        assert!(
            settled.addressed,
            "the Shell wakes the session from Frame.addressed"
        );
        assert_eq!(poked.animation, "react");
    }

    #[test]
    fn a_sprite_thrown_at_the_screen_edge_climbs_it_and_lets_go_at_the_top() {
        let mut engine = Engine::new(Point { x: 900.0, y: 400.0 });

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        let grabbed_wall = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..snapshot(100)
        });
        assert_eq!(grabbed_wall.state, State::Climbing);
        assert_eq!(
            grabbed_wall.position.x, 1000.0,
            "the display's right edge, not past it"
        );

        let ascending = engine.tick(&snapshot(100));
        assert_eq!(ascending.state, State::Climbing);
        assert!(
            ascending.position.y < grabbed_wall.position.y,
            "it goes up: {ascending:?}"
        );

        let landed = settle(&mut engine, &snapshot(100));
        assert_eq!(landed.state, State::Grounded);
        assert_eq!(landed.position.y, 800.0);
    }

    /// The wall gets the floor's answer to a Poke: the climb stops for the
    /// cooldown, and only then carries on up, since a wall has nowhere to rest.
    #[test]
    fn a_poke_mid_climb_pauses_on_the_wall_for_a_beat_then_climbs_on() {
        let mut engine = Engine::new(Point { x: 900.0, y: 400.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..snapshot(100)
        });
        let climbing = engine.tick(&snapshot(100));
        assert_eq!(climbing.state, State::Climbing);

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });
        assert_eq!(poked.animation, "react");
        assert_eq!(poked.state, State::Climbing);

        let paused: Vec<Frame> = (0..24).map(|_| engine.tick(&snapshot(100))).collect();
        assert!(
            paused
                .iter()
                .all(|frame| frame.state == State::Climbing && frame.position == poked.position),
            "it stays put on the wall through the cooldown: {paused:?}"
        );
        let resting: Vec<&Frame> = paused
            .iter()
            .filter(|frame| frame.animation == "climb")
            .collect();
        assert!(
            resting.len() > 1
                && resting
                    .iter()
                    .all(|frame| frame.animation_ms == resting[0].animation_ms),
            "once the reaction is over it keeps one climb pose, not climbing in place: {paused:?}"
        );

        let resumed = engine.tick(&snapshot(100));
        assert_eq!(resumed.state, State::Climbing);
        assert!(
            resumed.position.y < poked.position.y,
            "and climbs on once it is over: {resumed:?}"
        );
    }

    /// The pause stops the climb, not the world: a wall that goes away mid-pause still drops the sprite at once.
    #[test]
    fn a_climb_paused_by_a_poke_still_lets_go_when_its_display_goes() {
        let mut engine = Engine::new(Point { x: 900.0, y: 400.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..snapshot(100)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });

        let unplugged = engine.tick(&WorldSnapshot {
            displays: vec![],
            ..snapshot(100)
        });
        assert_eq!(unplugged.state, State::Falling, "{unplugged:?}");
    }

    /// A Throw at the right edge of `one_display`, then one tick up the wall from y 400.
    fn climbing_engine() -> (Engine, Frame) {
        let mut engine = Engine::new(Point { x: 900.0, y: 400.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..snapshot(100)
        });
        let climbing = engine.tick(&snapshot(100));
        (engine, climbing)
    }

    /// The wall gets the floor's answer to a reply bubble: the climb holds in one `climb` pose while the bubble is up, and carries on once it goes.
    #[test]
    fn a_reply_bubble_mid_climb_holds_the_sprite_on_the_wall_until_it_goes() {
        let (mut engine, climbing) = climbing_engine();
        assert_eq!(climbing.state, State::Climbing);
        assert_eq!(
            climbing.position,
            Point {
                x: 1000.0,
                y: 380.0
            }
        );

        let bubble = WorldSnapshot {
            locomotion_frozen: true,
            ..snapshot(100)
        };
        let held: Vec<Frame> = (0..10).map(|_| engine.tick(&bubble)).collect();
        assert!(
            held.iter().all(|frame| frame.state == State::Climbing
                && frame.position
                    == Point {
                        x: 1000.0,
                        y: 380.0
                    }
                && frame.animation == "climb"
                && frame.animation_ms == held[0].animation_ms),
            "it stays put on the wall in one climb pose while the bubble is up: {held:?}"
        );

        let resumed = engine.tick(&snapshot(100));
        assert_eq!(resumed.state, State::Climbing);
        assert_eq!(
            resumed.position,
            Point {
                x: 1000.0,
                y: 360.0
            },
            "and climbs on once it goes"
        );
    }

    /// The hold is what keeps the sprite from letting go at the top under its own bubble, the caret of a quick message included.
    #[test]
    fn a_climber_held_one_step_below_the_top_does_not_let_go_until_the_hold_ends() {
        let (mut engine, _) = climbing_engine();
        let near_top = (0..12).map(|_| engine.tick(&snapshot(100))).last().unwrap();
        assert_eq!(
            near_top.position,
            Point {
                x: 1000.0,
                y: 140.0
            }
        );

        for held in [
            WorldSnapshot {
                locomotion_frozen: true,
                ..snapshot(100)
            },
            WorldSnapshot {
                composing: true,
                ..snapshot(100)
            },
        ] {
            let frames: Vec<Frame> = (0..30).map(|_| engine.tick(&held)).collect();
            assert!(
                frames.iter().all(|frame| frame.state == State::Climbing
                    && frame.position
                        == Point {
                            x: 1000.0,
                            y: 140.0
                        }),
                "{frames:?}"
            );
        }

        let released = engine.tick(&snapshot(100));
        assert_eq!(released.position.y, 128.0, "{released:?}");
        assert_ne!(released.state, State::Climbing, "{released:?}");
    }

    #[test]
    fn a_sprite_left_alone_falls_asleep_and_a_poke_wakes_it() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        assert_eq!(settle(&mut engine, &snapshot(100)).state, State::Grounded);

        // A full minute of nobody touching it.
        let asleep = engine.tick(&snapshot(60_000));
        assert_eq!(asleep.state, State::Asleep);

        let woken = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });
        assert_eq!(woken.state, State::Grounded, "awake and back on its feet");
    }

    #[test]
    fn time_spent_in_the_air_is_not_time_spent_idling() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });

        // One enormous tick: a minute passes while the sprite is airborne.
        assert_eq!(engine.tick(&snapshot(60_000)).state, State::Grounded);

        assert_eq!(
            engine.tick(&snapshot(100)).state,
            State::Grounded,
            "it has only just landed, so it has not been idle a minute"
        );
    }

    fn perch_at_height(y: f64) -> WorldSnapshot {
        WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 100.0,
                    y,
                    width: 800.0,
                    height: 200.0,
                },
            )],
            displays: vec![one_display()],
            elapsed_ms: 16,
            ..WorldSnapshot::default()
        }
    }

    /// Falls onto the window from above it and reports where it came to rest.
    /// Longer than `settle`: a sprite the window refuses falls the whole
    /// display, which takes more ticks than one that lands part way down.
    fn dropped_onto(mut engine: Engine, world: &WorldSnapshot) -> (State, f64) {
        let frame = (0..120).map(|_| engine.tick(world)).last().unwrap();
        (frame.state, frame.position.y)
    }

    /// The dead zone is the Character's own height, not the tallest one
    /// anybody ships. A 64-point character needs 32 above its feet, so a title bar
    /// at y=80 holds it; the fixed 128 refused every window in the top 128 points.
    #[test]
    fn a_short_character_perches_where_a_tall_one_may_not() {
        let world = perch_at_height(80.0);

        let (short_state, short_y) = dropped_onto(
            Engine::new(Point { x: 500.0, y: 0.0 }).with_sprite_height(64.0),
            &world,
        );
        let (tall_state, _) = dropped_onto(
            Engine::new(Point { x: 500.0, y: 0.0 }).with_sprite_height(300.0),
            &world,
        );

        assert_eq!(
            short_state,
            State::Perched,
            "64-point art needs 32 above it"
        );
        assert_eq!(short_y, 80.0, "it should be standing on the title bar");
        assert_eq!(
            tall_state,
            State::Grounded,
            "300-point art needs 150 above it, which y=80 does not leave"
        );
    }

    /// An Engine told nothing keeps the fixed clearance, which is what
    /// every existing test in this file is written against.
    #[test]
    fn an_unknown_height_keeps_the_old_clearance() {
        let world = perch_at_height(80.0);

        let (state, _) = dropped_onto(Engine::new(Point { x: 500.0, y: 0.0 }), &world);

        assert_eq!(
            state,
            State::Grounded,
            "without a height the ceiling is 128, so y=80 is still refused"
        );
    }

    /// Art taller than the screen must not leave nowhere to stand: no display
    /// gives up more than half its height.
    #[test]
    fn art_taller_than_the_display_still_leaves_somewhere_to_stand() {
        let world = perch_at_height(500.0);

        let (state, y) = dropped_onto(
            Engine::new(Point { x: 500.0, y: 0.0 }).with_sprite_height(4000.0),
            &world,
        );

        assert_eq!(state, State::Perched, "the clamp caps the ceiling at 400");
        assert_eq!(y, 500.0);
    }

    fn a_long_perch() -> WorldSnapshot {
        WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 100.0,
                    y: 400.0,
                    width: 800.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        }
    }

    /// The Director asking for a walk, by the name the Blip Character
    /// declares for a Behavior of one `walk` Primitive.
    fn walk() -> Option<BehaviorProposal> {
        Some(BehaviorProposal {
            behavior: "walk".to_string(),
            dialogue: None,
        })
    }

    /// The art has one heading, so the renderer mirrors it by `facing`. Only
    /// travel may turn the sprite: a stop, a doze, a straight fall all keep
    /// the last heading, or it would spin on the spot at rest.
    #[test]
    fn the_sprite_faces_the_way_it_travels_and_keeps_it_at_rest() {
        let mut engine = a_character_at(Point { x: 400.0, y: 0.0 });
        let perched = settle(&mut engine, &a_long_perch());
        assert_eq!(perched.facing, 1.0, "untraveled, it points right");

        // Thrown leftwards onto the Perch.
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 800.0, y: 100.0 },
            verbs: vec![Verb::Grab],
            ..a_long_perch()
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: -300.0, y: 0.0 },
            }],
            ..a_long_perch()
        });
        let flying = engine.tick(&a_long_perch());
        assert_eq!(flying.facing, -1.0, "it flies the way it was thrown");

        let at_rest = settle(&mut engine, &a_long_perch());
        assert_eq!(at_rest.velocity.x, 0.0);
        assert_eq!(
            at_rest.facing, -1.0,
            "and coming to rest keeps the heading: {at_rest:?}"
        );

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let strolling = engine.tick(&a_long_perch());
        assert_eq!(
            strolling.velocity.x, -WALK_SPEED,
            "it walks the way it faces"
        );
        assert_eq!(strolling.facing, -1.0);
    }

    /// The Primitive is what walks, not the name over it. A Character is free
    /// to call a stroll anything, and a Director that proposes one gets a
    /// sprite that moves.
    #[test]
    fn a_walk_is_the_primitive_that_reaches_the_screen_and_not_the_behaviors_name() {
        let mut engine = Engine::new(Point { x: 200.0, y: 0.0 }).with_behaviors(BTreeMap::from([
            (
                // Opens on a Primitive that stands still, so a sprite that
                // moves before the walk comes up is one moved by the name.
                "amble".to_string(),
                Behavior {
                    primitives: vec![Primitive::React, Primitive::Walk],
                    then: None,
                    weight: DEFAULT_WEIGHT,
                    trigger: None,
                },
            ),
            (
                // Named for the walk it is not: a Character that declares this
                // has declared sitting down, whatever the Director reads into
                // the name.
                "walk".to_string(),
                Behavior {
                    primitives: vec![Primitive::Sit],
                    then: None,
                    weight: DEFAULT_WEIGHT,
                    trigger: None,
                },
            ),
        ]));
        settle(&mut engine, &a_long_perch());

        let reacting = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "amble".to_string(),
                dialogue: None,
            }),
            ..a_long_perch()
        });
        assert_eq!(reacting.animation, "react");
        assert_eq!(
            reacting.position.x, 200.0,
            "the Behavior opens on a Primitive that does not move it"
        );

        // `react` holds the screen for six of these ticks, so the eighth is
        // inside the walk's own turn rather than the hold-over after it.
        let strolling = (0..8).map(|_| engine.tick(&a_long_perch())).last().unwrap();
        assert_eq!(strolling.animation, "walk");
        assert!(
            strolling.position.x > 200.0,
            "and sets off once the walk comes up: {strolling:?}"
        );

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let sitting = engine.tick(&a_long_perch());
        assert_eq!(sitting.animation, "sit");
        assert_eq!(
            sitting.velocity.x, 0.0,
            "a Behavior called walk that does not walk stops the one under way"
        );
    }

    /// A proposal is gated on the State the tick ends in, not the one it
    /// opened with. Reading the State before a Summon wake is asking a sprite
    /// standing on the floor whether it is in mid-air.
    #[test]
    fn a_proposal_is_gated_on_the_state_the_tick_ends_in() {
        let mut engine = a_resting_sprite();
        assert_eq!(engine.tick(&snapshot(60_000)).state, State::Asleep);

        let woken = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Summon],
            proposal: Some(BehaviorProposal {
                behavior: "greet".to_string(),
                dialogue: None,
            }),
            ..snapshot(100)
        });
        assert_eq!(woken.state, State::Grounded);
        assert_eq!(
            woken.animation, "react",
            "the greeting is played: {woken:?}"
        );
    }

    /// The Summon that replaced the second click's Poke keeps its stop
    /// and its cooldown too, so a double-click on a strolling sprite holds it
    /// exactly as it did before. Menu is the verb that lets the walk carry on.
    #[test]
    fn a_summon_mid_stroll_stops_the_walk_like_a_poke() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        assert_eq!(engine.tick(&a_long_perch()).velocity.x, WALK_SPEED);

        let summoned = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Summon],
            ..a_long_perch()
        });
        assert_eq!(summoned.velocity.x, 0.0, "the summon stops the feet");

        let held: Vec<Frame> = (0..24).map(|_| engine.tick(&a_long_perch())).collect();
        assert!(
            held.iter()
                .all(|frame| frame.position.x == summoned.position.x),
            "it stays put through the cooldown: {held:?}"
        );
    }

    /// A Poke stops the stroll. Reacting while carrying on walking read as
    /// something that happened to the animation rather than to the character,
    /// so the sprite stands for a beat and only then may move.
    #[test]
    fn a_poke_mid_stroll_stops_the_walk_for_a_beat() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let under_way = engine.tick(&a_long_perch());
        assert_eq!(under_way.velocity.x, WALK_SPEED);

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..a_long_perch()
        });
        assert_eq!(poked.animation, "react");
        assert_eq!(poked.velocity.x, 0.0, "the click stops the feet");

        let held: Vec<Frame> = (0..24).map(|_| engine.tick(&a_long_perch())).collect();
        assert!(
            held.iter()
                .all(|frame| frame.position.x == poked.position.x),
            "it stays put through the cooldown: {held:?}"
        );
        assert_eq!(
            held.last().unwrap().animation,
            animation_for(State::Perched),
            "and rests the way a perched sprite does once the reaction is over"
        );
    }

    /// The hold is a pause the Director cannot walk through: a proposal that
    /// moves the sprite is refused until it is over, while one that only
    /// speaks is not — the character was just addressed, and answering is fine.
    #[test]
    fn a_walk_proposed_mid_cooldown_waits_and_one_after_it_does_not() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..a_long_perch()
        });
        for _ in 0..10 {
            engine.tick(&a_long_perch());
        }

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let refused = engine.tick(&a_long_perch());
        assert_eq!(
            refused.velocity.x, 0.0,
            "a walk mid-cooldown waits: {refused:?}"
        );
        assert_eq!(refused.position.x, poked.position.x);

        let greeted = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "greet".to_string(),
                dialogue: None,
            }),
            ..a_long_perch()
        });
        assert_eq!(greeted.animation, "react", "speaking mid-cooldown is fine");

        for _ in 0..30 {
            engine.tick(&a_long_perch());
        }
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let resumed = engine.tick(&a_long_perch());
        assert_eq!(
            resumed.velocity.x, WALK_SPEED,
            "after the cooldown a walk is taken up: {resumed:?}"
        );
    }

    /// The hand outranks standing still: a Grab mid-cooldown picks the sprite up
    /// at once, and once it is thrown and lands, nothing of the cooldown is left
    /// to refuse the next walk.
    #[test]
    fn a_grab_mid_cooldown_takes_over_at_once_and_clears_the_cooldown() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..a_long_perch()
        });

        let grabbed = engine.tick(&WorldSnapshot {
            cursor: Point { x: 300.0, y: 200.0 },
            verbs: vec![Verb::Grab],
            ..a_long_perch()
        });
        assert_eq!(grabbed.state, State::Dragged);
        assert_eq!(grabbed.position, Point { x: 300.0, y: 200.0 });

        let thrown = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 0.0, y: 0.0 },
            }],
            ..a_long_perch()
        });
        assert_eq!(thrown.state, State::Falling);
        settle(&mut engine, &a_long_perch());

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let walking = engine.tick(&a_long_perch());
        assert_eq!(
            walking.velocity.x, WALK_SPEED,
            "the hand ended the cooldown, not the clock: {walking:?}"
        );
    }

    /// Losing the ground mid-cooldown is a fall like any other, and the fall
    /// ends the cooldown: the first walk after the landing is not refused.
    #[test]
    fn losing_the_ground_mid_cooldown_falls_at_once_and_clears_the_cooldown() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..a_long_perch()
        });

        let dropped = engine.tick(&snapshot(100));
        assert_eq!(dropped.state, State::Falling, "{dropped:?}");

        settle(&mut engine, &snapshot(100));
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..snapshot(100)
        });
        let walking = engine.tick(&snapshot(100));
        assert_eq!(
            walking.velocity.x, WALK_SPEED,
            "nothing of the cooldown survives the fall: {walking:?}"
        );
    }

    /// The cooldown's edge, pinned to the tick. A tick is 100 ms and the Poke
    /// sets POKE_COOLDOWN_MS after that tick's decrement, so the cooldown is
    /// exactly POKE_COOLDOWN_MS / 100 ticks after the Poke.
    #[test]
    fn the_cooldown_ends_on_the_tick_it_says_it_does() {
        let ticks = (POKE_COOLDOWN_MS / 100) as usize;
        let propose_after = |plain: usize| {
            let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
            settle(&mut engine, &a_long_perch());
            engine.tick(&WorldSnapshot {
                verbs: vec![Verb::Poke],
                ..a_long_perch()
            });
            for _ in 0..plain {
                engine.tick(&a_long_perch());
            }
            engine.tick(&WorldSnapshot {
                proposal: walk(),
                ..a_long_perch()
            });
            engine.tick(&a_long_perch()).velocity.x
        };

        assert_eq!(
            propose_after(ticks - 2),
            0.0,
            "one tick short, still settling"
        );
        assert_eq!(
            propose_after(ticks - 1),
            WALK_SPEED,
            "on the tick, taken up"
        );
    }

    /// The cooldown is one gate for every path that starts motion, not a check
    /// at the proposal alone: a cursor reaction that would walk the sprite
    /// toward the pointer is refused mid-cooldown, and walks once it is over.
    #[test]
    fn a_cursor_reaction_cannot_walk_the_sprite_mid_cooldown() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Toward, CursorReaction::Indifferent);
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });

        let beside = Point {
            x: engine.position.x + 100.0,
            y: engine.position.y,
        };
        let near = || WorldSnapshot {
            cursor: beside,
            ..snapshot(100)
        };
        let approached = engine.tick(&near());
        assert_ne!(approached.animation, "walk", "{approached:?}");
        assert_eq!(
            approached.velocity.x, 0.0,
            "the cursor does not move settling feet"
        );

        for _ in 0..26 {
            engine.tick(&snapshot(100));
        }
        let again = engine.tick(&near());
        assert_eq!(
            again.animation, "walk",
            "once the cooldown is over it walks: {again:?}"
        );
    }

    /// The same cooldown on the display floor: the mechanism is the sprite's,
    /// not the Perch's, and at rest on the floor it shows `idle`.
    #[test]
    fn a_poke_mid_stroll_on_the_floor_stops_the_walk_too() {
        let mut engine = a_character_at(Point { x: 300.0, y: 0.0 });
        settle(&mut engine, &snapshot(100));
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..snapshot(100)
        });
        assert_eq!(engine.tick(&snapshot(100)).velocity.x, WALK_SPEED);

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });
        let held: Vec<Frame> = (0..24).map(|_| engine.tick(&snapshot(100))).collect();
        assert!(
            held.iter()
                .all(|frame| frame.position.x == poked.position.x),
            "{held:?}"
        );
        assert_eq!(held.last().unwrap().animation, "idle");
    }

    #[test]
    fn a_second_poke_mid_cooldown_restarts_the_cooldown() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());
        let poke = || WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..a_long_perch()
        };

        engine.tick(&poke());
        for _ in 0..20 {
            engine.tick(&a_long_perch());
        }
        engine.tick(&poke());
        for _ in 0..20 {
            engine.tick(&a_long_perch());
        }

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let still_held = engine.tick(&a_long_perch());
        assert_eq!(
            still_held.velocity.x, 0.0,
            "the second Poke started the cooldown over: {still_held:?}"
        );

        for _ in 0..6 {
            engine.tick(&a_long_perch());
        }
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let released = engine.tick(&a_long_perch());
        assert_eq!(released.velocity.x, WALK_SPEED, "{released:?}");
    }

    #[test]
    fn the_sprite_walks_along_a_window_top_edge() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        let perched = settle(&mut engine, &a_long_perch());
        assert_eq!(perched.state, State::Perched);
        assert_eq!(perched.position.x, 200.0, "it landed where it fell");

        let told = engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        assert_eq!(told.animation, "walk");

        // The tick after, because the Behavior is played once this tick's State
        // is settled and the sprite has already been moved by then. SPEC.md
        // asks only that a valid proposal be applied on the next tick.
        let setting_off = engine.tick(&a_long_perch());
        assert!(
            setting_off.position.x > 200.0,
            "it sets off: {setting_off:?}"
        );

        let carrying_on = engine.tick(&a_long_perch());
        assert_eq!(carrying_on.state, State::Perched, "still on the edge");
        assert_eq!(carrying_on.position.y, 400.0, "and at its height");
        assert!(
            carrying_on.position.x > setting_off.position.x,
            "and keeps going without being told again: {carrying_on:?}"
        );
    }

    #[test]
    fn a_walk_is_preempted_while_a_speech_bubble_is_up() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let started = engine.tick(&a_long_perch());
        assert!(
            started.position.x > 200.0,
            "the stroll has started: {started:?}"
        );
        assert_eq!(started.animation, "walk");

        let held_x = started.position.x;
        let tick_ms = 100u32;
        let halt_ms = PRIMITIVE_MS + tick_ms;
        let halt_ticks = halt_ms / tick_ms;
        let mut halted = started;
        for _ in 0..halt_ticks {
            halted = engine.tick(&WorldSnapshot {
                locomotion_frozen: true,
                ..a_long_perch()
            });
        }

        assert_eq!(halted.position.x, held_x, "position held under the bubble");
        assert_eq!(halted.velocity.x, 0.0, "the walk is dropped, not paused");
        assert_eq!(
            halted.animation, "talk",
            "under the bubble the sprite speaks instead of striding in place"
        );
        assert_eq!(
            halted.animation_ms,
            halt_ms - tick_ms,
            "talk started on the first held tick and has run since"
        );

        let refused = engine.tick(&WorldSnapshot {
            locomotion_frozen: true,
            proposal: walk(),
            ..a_long_perch()
        });
        assert_eq!(
            refused.position.x, held_x,
            "no walk starts under the bubble"
        );
        assert_eq!(refused.animation, "talk");

        let thawed = engine.tick(&a_long_perch());
        assert_eq!(thawed.position.x, held_x, "the thaw restores nothing");
        assert_eq!(thawed.velocity.x, 0.0);
        assert_eq!(thawed.animation, "sit", "Perched and unasked, it rests");

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let next_pick = engine.tick(&a_long_perch());
        assert_eq!(next_pick.animation, "walk");
        assert_eq!(next_pick.position.x, held_x + 12.0, "the next pick walks");
    }

    /// Both ends, because a walk that only ever goes one way would leave the
    /// other end untested. Which way it goes is the way it was already
    /// heading, so the throw that puts it on the Perch also aims the walk.
    #[test]
    fn the_sprite_walks_off_either_end_of_a_perch() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());

        let off_the_right = walked_off(&mut engine);
        assert!(
            off_the_right.position.x > 900.0,
            "past the window's right edge: {off_the_right:?}"
        );
        assert_eq!(off_the_right.state, State::Grounded);
        assert_eq!(off_the_right.position.y, 800.0, "down on the floor");

        // Thrown back onto the Perch leftwards, so it walks off the other end.
        let mut engine = a_character_at(Point { x: 800.0, y: 100.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 800.0, y: 100.0 },
            verbs: vec![Verb::Grab],
            ..a_long_perch()
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: -300.0, y: 0.0 },
            }],
            ..a_long_perch()
        });
        let perched = settle(&mut engine, &a_long_perch());
        assert_eq!(perched.state, State::Perched, "back on it: {perched:?}");

        let off_the_left = walked_off(&mut engine);
        assert!(
            off_the_left.position.x < 100.0,
            "past the window's left edge: {off_the_left:?}"
        );
        assert_eq!(off_the_left.state, State::Grounded);
        assert_eq!(off_the_left.position.y, 800.0);
    }

    fn walked_off(engine: &mut Engine) -> Frame {
        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        (0..200)
            .map(|_| engine.tick(&a_long_perch()))
            .last()
            .expect("two hundred ticks produce two hundred frames")
    }

    /// A walk under way keeps the sprite awake. Nodding off is for a sprite
    /// that has been left alone, and a Director prodding an idle sprite into a
    /// walk is exactly when the sleep timer is about to come due.
    #[test]
    fn a_walking_sprite_does_not_nod_off_mid_stride() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());

        // Poked, then left alone until it is one tick short of nodding off.
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..a_long_perch()
        });
        let nearly_asleep = engine.tick(&WorldSnapshot {
            elapsed_ms: SLEEP_AFTER_MS - 100,
            ..a_long_perch()
        });
        assert_eq!(nearly_asleep.state, State::Perched, "not asleep yet");

        // The tick the timer comes due is the tick it sets off walking.
        let setting_off = engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let strolling: Vec<Frame> = (0..20).map(|_| engine.tick(&a_long_perch())).collect();

        assert_eq!(setting_off.animation, "walk", "{setting_off:?}");
        assert!(
            strolling
                .iter()
                .all(|frame| frame.state == State::Perched && frame.animation == "walk"),
            "it walks the edge awake rather than sleeping its way along it: {strolling:?}"
        );
    }

    /// DESIGN.md decision 7: the bad case is a sprite trapped inside an
    /// occluded window, not a sprite standing on the ground in front of one.
    /// Windows hang below the usable floor, so being within one is the normal ground state.
    #[test]
    fn a_window_over_the_floor_leaves_the_sprite_standing_on_it() {
        let mut engine = Engine::new(Point { x: 500.0, y: 0.0 });
        let grounded = settle(&mut engine, &snapshot(100));
        assert_eq!(grounded.state, State::Grounded);
        assert_eq!(grounded.position.y, 800.0, "the usable floor");

        // A window is dragged over it, hanging below the usable floor.
        let covered = WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 0.0,
                    y: 100.0,
                    width: 1000.0,
                    height: 800.0,
                },
            )],
            ..snapshot(100)
        };
        let frames: Vec<Frame> = (0..10).map(|_| engine.tick(&covered)).collect();
        assert!(
            frames
                .iter()
                .all(|frame| frame.position == grounded.position && frame.state == State::Grounded),
            "it stays on the ground rather than flying up to the title bar: {frames:?}"
        );
    }

    /// The other half of what makes a window the sprite's footing: it has to
    /// have come to contain it. A window floating clear above the Perch is not
    /// something the sprite is inside, so it is not something to be lifted onto.
    #[test]
    fn a_window_floating_above_the_perch_is_not_footing() {
        let world = || WorldSnapshot {
            windows: vec![
                window(
                    1,
                    Rect {
                        x: 0.0,
                        y: 100.0,
                        width: 1000.0,
                        height: 150.0,
                    },
                ),
                window(
                    2,
                    Rect {
                        x: 0.0,
                        y: 400.0,
                        width: 1000.0,
                        height: 200.0,
                    },
                ),
            ],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 500.0, y: 300.0 });

        let perched = settle(&mut engine, &world());
        assert_eq!(perched.state, State::Perched);
        assert_eq!(perched.position.y, 400.0, "the edge below it");

        let frames: Vec<Frame> = (0..10).map(|_| engine.tick(&world())).collect();
        assert!(
            frames.iter().all(|frame| frame.position.y == 400.0),
            "and it stays there rather than being hoisted to the one above: {frames:?}"
        );
    }

    #[test]
    fn the_sprite_perches_on_a_window_top_edge_and_falls_when_the_window_goes() {
        let window = WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 50.0,
                    y: 400.0,
                    width: 300.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });

        let perched = settle(&mut engine, &window);
        assert_eq!(perched.state, State::Perched);
        assert_eq!(perched.position.y, 400.0, "the window's top edge");

        let dropped = engine.tick(&snapshot(100));
        assert_eq!(dropped.state, State::Falling);

        let landed = settle(&mut engine, &snapshot(100));
        assert_eq!(landed.state, State::Grounded);
        assert_eq!(landed.position.y, 800.0);
    }

    /// The sibling of the window closing: the window is still there, it has
    /// simply been yanked elsewhere in one poll. A slow drag is ridden, so
    /// what drops the sprite here is the speed, not the move.
    #[test]
    fn a_perch_yanked_out_from_under_the_sprite_drops_it() {
        let window = |x: f64| WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x,
                    y: 400.0,
                    width: 300.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });

        let perched = settle(&mut engine, &window(50.0));
        assert_eq!(perched.state, State::Perched);
        assert_eq!(perched.position.y, 400.0);

        // The same window, yanked out from under it rather than closed.
        let dropped = engine.tick(&window(600.0));
        assert_eq!(dropped.state, State::Falling);

        let landed = settle(&mut engine, &window(600.0));
        assert_eq!(landed.state, State::Grounded);
        assert_eq!(landed.position.y, 800.0, "down to the floor it left");
    }

    /// A window the sprite can stand on, moved without changing size.
    fn perch(x: f64, y: f64) -> WorldSnapshot {
        WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x,
                    y,
                    width: 300.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        }
    }

    /// A Perch dragged slowly is still underfoot. The sprite keeps the
    /// place it had on the edge rather than falling through the window that
    /// now contains it.
    #[test]
    fn the_sprite_rides_a_slowly_dragged_perch() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        let perched = settle(&mut engine, &perch(50.0, 400.0));
        assert_eq!(perched.state, State::Perched);
        let offset_x = perched.position.x - 50.0;

        let up = engine.tick(&perch(50.0, 380.0));
        assert_eq!(up.state, State::Perched, "up with the window: {up:?}");
        assert_eq!(
            up.position,
            Point {
                x: perched.position.x,
                y: 380.0
            }
        );
        assert_eq!(up.animation, "hold");
        assert!(up.riding, "the Shell polls fast only while this is set");
        assert_eq!(
            up.velocity,
            Point::default(),
            "#85 sub-decision 2: the ride is a position offset, so flinging a \
             window must not launch the sprite ballistically"
        );

        let across = engine.tick(&perch(70.0, 380.0));
        assert_eq!(across.state, State::Perched, "sideways: {across:?}");
        assert_eq!(
            across.position,
            Point {
                x: 70.0 + offset_x,
                y: 380.0
            }
        );
        assert_eq!(across.animation, "hold");
        assert_eq!(across.velocity, Point::default());

        let down = engine.tick(&perch(70.0, 400.0));
        assert_eq!(down.state, State::Perched, "down with the window: {down:?}");
        assert_eq!(
            down.position,
            Point {
                x: 70.0 + offset_x,
                y: 400.0
            }
        );
        assert_eq!(down.animation, "hold");
        assert_eq!(down.velocity, Point::default());
    }

    /// Window geometry is reused between polls. The sprite has to keep
    /// the last Perch velocity on those ticks, or it hitch-steps at the poll
    /// rate while the window itself slides every frame.
    #[test]
    fn the_sprite_coasts_with_the_perch_between_polls() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        let mut moving = perch(50.0, 380.0);
        moving.poll_generation = 1;
        let caught_up = engine.tick(&moving);
        assert_eq!(caught_up.position.y, 380.0);
        assert_eq!(caught_up.animation, "hold");

        // Same rectangle, same generation: the assembler has not read again.
        // 20 points in 100 ms is 200 points/s, so 16 ms is 3.2 points further.
        moving.elapsed_ms = 16;
        let coasting = engine.tick(&moving);
        assert_eq!(coasting.state, State::Perched, "{coasting:?}");
        assert_eq!(coasting.animation, "hold");
        assert!(
            (coasting.position.y - 376.8).abs() < 1e-9,
            "it continues at the Perch's last speed, not waiting for the next poll: {coasting:?}"
        );
    }

    /// A Perch that is speeding up is not at constant velocity between
    /// polls. Coasting with the last acceleration keeps the sprite on the
    /// window instead of hitching every time a new sample snaps it back.
    #[test]
    fn the_sprite_coasts_with_the_perch_acceleration_between_polls() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        let mut first = perch(50.0, 390.0);
        first.poll_generation = 1;
        assert_eq!(engine.tick(&first).position.y, 390.0);

        // 10 points, then 20: -100 points/s, then -200. Acceleration is
        // -1000 points/s² — under the yank gate, so it still rides.
        let mut faster = perch(50.0, 370.0);
        faster.poll_generation = 2;
        assert_eq!(engine.tick(&faster).position.y, 370.0);

        faster.elapsed_ms = 16;
        let coasting = engine.tick(&faster);
        assert_eq!(coasting.state, State::Perched, "{coasting:?}");
        assert_eq!(coasting.animation, "hold");
        // v Δt + ½ a Δt² = -200·0.016 + ½·(-1000)·0.016² = -3.328
        assert!(
            (coasting.position.y - 366.672).abs() < 1e-9,
            "it continues at the last speed and acceleration: {coasting:?}"
        );
    }

    /// The ride ends when the Perch is still. Holding on is the motion,
    /// not a new way to sit.
    #[test]
    fn a_still_perch_returns_the_sprite_to_sitting() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));
        assert_eq!(engine.tick(&perch(50.0, 380.0)).animation, "hold");

        let still = engine.tick(&perch(50.0, 380.0));
        assert_eq!(still.state, State::Perched);
        assert_eq!(still.position.y, 380.0);
        assert_eq!(still.animation, "sit");
        assert!(!still.riding);
    }

    /// A sprite that rests quietly on a Perch for a while idles in
    /// place rather than staying in sit. Still Perched, same edge, but idle
    /// art and idle life can run.
    #[test]
    fn a_perched_sprite_idles_after_resting_quietly() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        // Fresh perch rest: a Poke resets the timer. Wait for the reaction
        // to finish, then the sprite sits.
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..perch(50.0, 400.0)
        });
        engine.tick(&WorldSnapshot {
            elapsed_ms: PRIMITIVE_MS,
            ..perch(50.0, 400.0)
        });
        let fresh_rest = engine.tick(&perch(50.0, 400.0));
        assert_eq!(fresh_rest.animation, "sit", "sit after reset");

        // Accumulate time in one large tick, just under the threshold.
        let nearly_idle = engine.tick(&WorldSnapshot {
            elapsed_ms: PERCHED_IDLE_AFTER_MS - 101,
            ..perch(50.0, 400.0)
        });
        assert_eq!(nearly_idle.state, State::Perched);
        assert_eq!(nearly_idle.animation, "sit", "not idle yet");

        // One more tick crosses the threshold.
        let idling = engine.tick(&WorldSnapshot {
            elapsed_ms: 100,
            ..perch(50.0, 400.0)
        });
        assert_eq!(idling.state, State::Perched, "still Perched");
        assert_eq!(idling.animation, "idle", "now idle after the timer");
    }

    /// The timer resets when the sprite walks, so a stroll interrupted
    /// by rest briefly shows sit again before idling.
    #[test]
    fn perched_idle_timer_resets_when_walking() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());

        engine.tick(&WorldSnapshot {
            elapsed_ms: PERCHED_IDLE_AFTER_MS,
            ..a_long_perch()
        });
        assert_eq!(
            engine.tick(&a_long_perch()).animation,
            "idle",
            "idling after rest"
        );

        let proposing_walk = engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        assert_eq!(proposing_walk.animation, "walk");

        let walking_tick = engine.tick(&a_long_perch());
        assert_eq!(walking_tick.animation, "walk", "walking continues");

        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..a_long_perch()
        });
        engine.tick(&WorldSnapshot {
            elapsed_ms: PRIMITIVE_MS,
            ..a_long_perch()
        });
        let resting = engine.tick(&a_long_perch());
        assert_eq!(resting.animation, "sit", "back to sit after walk stops");

        engine.tick(&WorldSnapshot {
            elapsed_ms: PERCHED_IDLE_AFTER_MS,
            ..a_long_perch()
        });
        let idling_again = engine.tick(&a_long_perch());
        assert_eq!(idling_again.animation, "idle", "idle after rest again");
    }

    /// #310: the timer resets when riding a moving Perch, so a sprite that
    /// lands from a ride shows sit first.
    #[test]
    fn perched_idle_timer_resets_when_riding() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        engine.tick(&WorldSnapshot {
            elapsed_ms: PERCHED_IDLE_AFTER_MS,
            ..perch(50.0, 400.0)
        });
        assert_eq!(
            engine.tick(&perch(50.0, 400.0)).animation,
            "idle",
            "idling after rest"
        );

        let riding = engine.tick(&perch(50.0, 380.0));
        assert_eq!(riding.animation, "hold", "holding the moving Perch");

        let still = engine.tick(&perch(50.0, 380.0));
        assert_eq!(still.animation, "sit", "back to sit after ride ends");
    }

    /// The sprite sits briefly after a reaction before idling again.
    #[test]
    fn perched_idle_timer_resets_when_a_behavior_plays() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        engine.tick(&WorldSnapshot {
            elapsed_ms: PERCHED_IDLE_AFTER_MS,
            ..perch(50.0, 400.0)
        });
        assert_eq!(
            engine.tick(&perch(50.0, 400.0)).animation,
            "idle",
            "idling after rest"
        );

        let reacting = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..perch(50.0, 400.0)
        });
        assert_eq!(reacting.animation, "react");

        engine.tick(&WorldSnapshot {
            elapsed_ms: PRIMITIVE_MS,
            ..perch(50.0, 400.0)
        });
        let resting = engine.tick(&perch(50.0, 400.0));
        assert_eq!(resting.animation, "sit", "back to sit after reaction");
    }

    /// Landing back on the same edge shows sit first.
    #[test]
    fn perched_idle_timer_resets_when_leaving_perched() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        engine.tick(&WorldSnapshot {
            elapsed_ms: PERCHED_IDLE_AFTER_MS,
            ..perch(50.0, 400.0)
        });
        assert_eq!(
            engine.tick(&perch(50.0, 400.0)).animation,
            "idle",
            "idling after rest"
        );

        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Grab],
            ..perch(50.0, 400.0)
        });
        let dragged = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Grab],
            cursor: Point { x: 100.0, y: 350.0 },
            ..perch(50.0, 400.0)
        });
        assert_eq!(dragged.state, State::Dragged);

        settle(&mut engine, &perch(50.0, 400.0));
        let landed = engine.tick(&perch(50.0, 400.0));
        assert_eq!(landed.state, State::Perched);
        assert_eq!(landed.animation, "sit", "sit first after landing");
    }

    /// The idle timer is much shorter than sleep, so a sprite idles first
    /// and sleeps later.
    #[test]
    fn a_perched_sprite_idles_then_sleeps_if_left_alone() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        let idling = engine.tick(&WorldSnapshot {
            elapsed_ms: PERCHED_IDLE_AFTER_MS,
            ..perch(50.0, 400.0)
        });
        assert_eq!(idling.state, State::Perched);
        assert_eq!(idling.animation, "idle", "idle before sleep");

        let asleep = engine.tick(&WorldSnapshot {
            elapsed_ms: SLEEP_AFTER_MS,
            ..perch(50.0, 400.0)
        });
        assert_eq!(asleep.state, State::Asleep, "then sleep");
        assert_eq!(asleep.animation, "sleep");
    }

    /// Matching by size and displacement would take a nearby same-size window
    /// and slide the sprite onto it. The ids differ, so the Perch is gone and
    /// the sprite falls.
    #[test]
    fn a_perch_that_closes_is_not_the_same_window_as_one_of_its_size_nearby() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        assert_eq!(
            settle(&mut engine, &perch(50.0, 400.0)).state,
            State::Perched
        );

        let twin = WorldSnapshot {
            windows: vec![window(
                2,
                Rect {
                    x: 70.0,
                    y: 380.0,
                    width: 300.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        };

        let dropped = engine.tick(&twin);
        assert_eq!(dropped.state, State::Falling, "{dropped:?}");
        assert_eq!(
            settle(&mut engine, &twin).position.y,
            800.0,
            "and down to the floor, not carried onto a window it never stood on"
        );
    }

    /// Geometry would take the nearer same-size window and put the sprite on
    /// the wrong window at the wrong offset. The id picks the window it was
    /// actually standing on.
    #[test]
    fn a_twin_window_nearer_the_old_origin_does_not_steal_the_ride() {
        // Frontmost first: the sprite lands on the Perch, and the twin behind
        // it is the same size and overlaps it.
        let twins = |perch_x: f64, twin_x: f64| WorldSnapshot {
            windows: vec![
                window(
                    1,
                    Rect {
                        x: perch_x,
                        y: 400.0,
                        width: 300.0,
                        height: 200.0,
                    },
                ),
                window(
                    2,
                    Rect {
                        x: twin_x,
                        y: 400.0,
                        width: 300.0,
                        height: 200.0,
                    },
                ),
            ],
            ..snapshot(100)
        };

        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        let perched = settle(&mut engine, &twins(50.0, 60.0));
        assert_eq!(perched.position.y, 400.0);
        let offset_x = perched.position.x - 50.0;

        // Both are nudged right. The Perch moves 40 points, the twin 10, so
        // the twin is now the nearer match to where the Perch was.
        let ridden = engine.tick(&twins(90.0, 70.0));
        assert_eq!(ridden.state, State::Perched, "{ridden:?}");
        assert_eq!(
            ridden.position,
            Point {
                x: 90.0 + offset_x,
                y: 400.0
            },
            "carried by window 1, not snapped onto window 2"
        );
    }

    /// Matching the Perch by size dropped the sprite off a resized window
    /// still under it. Matching by id carries it, and the grip gate still
    /// governs how fast the edge may go.
    #[test]
    fn a_perch_resized_from_its_top_edge_is_ridden_like_one_that_moved() {
        let sized = |y: f64, height: f64| WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 50.0,
                    y,
                    width: 300.0,
                    height,
                },
            )],
            ..snapshot(100)
        };

        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        let perched = settle(&mut engine, &sized(400.0, 200.0));
        assert_eq!(perched.position.y, 400.0);

        // The top border dragged up 20 points: the window grows, the edge
        // moves, and the sprite goes with it.
        let taller = engine.tick(&sized(380.0, 220.0));
        assert_eq!(taller.state, State::Perched, "{taller:?}");
        assert_eq!(taller.position.y, 380.0);
        assert!(taller.riding);

        // The same border yanked 200 points: over the grip, so the sprite is
        // left behind rather than snapped onto the new edge.
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &sized(400.0, 200.0));
        let yanked = engine.tick(&sized(200.0, 400.0));
        assert_eq!(yanked.state, State::Falling, "{yanked:?}");
        assert_eq!(yanked.position.y, 400.0, "left where it stood");
    }

    /// The fast descent leaves the sprite in the air; it re-lands on the
    /// same edge, now below it.
    #[test]
    fn a_slow_descent_is_ridden_and_a_fast_one_leaves_the_sprite_behind() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));

        // 20 points in 100 ms: 200 pt/s, well under the gate.
        let ridden = engine.tick(&perch(50.0, 420.0));
        assert_eq!(ridden.state, State::Perched, "{ridden:?}");
        assert_eq!(ridden.position.y, 420.0);
        assert!(ridden.riding);

        // 200 points in the same poll, and the window is gone from under it.
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));
        let dropped = engine.tick(&perch(50.0, 600.0));
        assert_eq!(dropped.state, State::Falling, "{dropped:?}");
        assert_eq!(dropped.position.y, 400.0, "left in the air where it stood");

        let landed = settle(&mut engine, &perch(50.0, 600.0));
        assert_eq!(landed.state, State::Perched);
        assert_eq!(landed.position.y, 600.0, "onto the same edge, now below it");
    }

    /// A sideways yank with the edge still under the sprite. The visibility
    /// re-check in `perch_carry` stays satisfied, so only the gate can drop
    /// it — the two guards do not cover for each other.
    #[test]
    fn a_sideways_yank_leaves_the_sprite_standing_where_it_was() {
        let wide = |x: f64| WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x,
                    y: 400.0,
                    width: 900.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 500.0, y: 0.0 });
        let perched = settle(&mut engine, &wide(50.0));
        assert_eq!(perched.position, Point { x: 500.0, y: 400.0 });

        // 150 points left in one poll, and the edge still runs under the
        // sprite: -100..800 spans 500. A ride would slide it to 350.
        let dropped = engine.tick(&wide(-100.0));
        assert_eq!(dropped.state, State::Falling, "{dropped:?}");
        assert_eq!(
            dropped.position,
            Point { x: 500.0, y: 400.0 },
            "left standing where it was, not carried along the edge"
        );

        let landed = settle(&mut engine, &wide(-100.0));
        assert_eq!(landed.state, State::Perched);
        assert_eq!(
            landed.position,
            Point { x: 500.0, y: 400.0 },
            "and drops straight back onto the edge under it"
        );
    }

    /// Two displays, the right one short. The Perch is dragged slowly down
    /// until its edge is below the shorter usable floor, somewhere no display
    /// covers. The sprite lets go rather than riding out behind the Dock.
    #[test]
    fn a_ride_never_carries_the_sprite_where_no_display_covers() {
        // Nothing covers x 1000..2000 outside y 300..500.
        let world = |y: f64| WorldSnapshot {
            displays: vec![
                one_display(),
                Rect {
                    x: 1000.0,
                    y: 300.0,
                    width: 1000.0,
                    height: 200.0,
                },
            ],
            windows: vec![window(
                1,
                Rect {
                    x: 1000.0,
                    y,
                    width: 600.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point {
            x: 1200.0,
            y: 350.0,
        });
        assert_eq!(settle(&mut engine, &world(450.0)).position.y, 450.0);

        // 20 points a poll — 200 pt/s, well under the gate — so every step of
        // this is a ride, right up to the one that would leave the displays.
        for y in [470.0, 490.0] {
            let ridden = engine.tick(&world(y));
            assert_eq!(ridden.state, State::Perched, "{ridden:?}");
            assert_eq!(ridden.position.y, y);
        }

        let dropped = engine.tick(&world(510.0));
        assert_eq!(dropped.state, State::Falling, "{dropped:?}");
        assert_eq!(
            dropped.position.y, 490.0,
            "it let go rather than riding to a point no display covers"
        );

        let landed = settle(&mut engine, &world(510.0));
        assert_eq!(landed.state, State::Grounded);
        assert_eq!(
            landed.position.y, 500.0,
            "down to the shorter display's floor"
        );
    }

    /// Sideways the sprite moves with the edge, so the x it is leaving is a
    /// point the ride is about to abandon. Watch every tick rather than
    /// settling first: it has to be off the displays on the tick it is drawn there.
    #[test]
    fn a_sideways_ride_never_carries_the_sprite_where_no_display_covers() {
        let mut engine = Engine::new(Point {
            x: 1950.0,
            y: 350.0,
        });
        let perched = settle(&mut engine, &strip_perch(1400.0));
        assert_eq!(
            perched.position,
            Point {
                x: 1950.0,
                y: 450.0
            }
        );

        // 20 points a poll — 200 pt/s, well under the gate — dragged right
        // until the sprite's hold on the edge is past x 2000, which the third
        // step is: 1460 plus the 550 it holds at.
        for x in [1420.0, 1440.0, 1460.0, 1480.0] {
            let frame = engine.tick(&strip_perch(x));
            assert!(
                covered(frame.position, &strip_perch(x)),
                "carried off the displays with the window at {x}: {frame:?}"
            );
        }

        for _ in 0..40 {
            let frame = engine.tick(&strip_perch(1480.0));
            assert!(
                covered(frame.position, &strip_perch(1480.0)),
                "fell off the displays after letting go: {frame:?}"
            );
        }
    }

    /// Between polls the window list is stale and the sprite coasts on the last
    /// Perch velocity, which no sample gets to approve, so a coast runs off the
    /// displays for as many ticks as the poll is late rather than for one.
    #[test]
    fn a_coast_never_carries_the_sprite_where_no_display_covers() {
        let mut engine = Engine::new(Point {
            x: 1950.0,
            y: 350.0,
        });
        settle(&mut engine, &strip_perch(1400.0));

        // One fresh sample 20 points to the right: 200 pt/s of ride to coast
        // on, and 1990 is the last hold this side of the union's edge.
        let mut sample = strip_perch(1420.0);
        sample.poll_generation = 1;
        assert_eq!(engine.tick(&sample).position.x, 1970.0);

        // The same generation from here on: the assembler has not read again,
        // and 200 pt/s of coast crosses x 2000 on the second tick.
        for _ in 0..8 {
            let frame = engine.tick(&sample);
            assert!(
                covered(frame.position, &sample),
                "coasted off the displays: {frame:?}"
            );
        }
    }

    /// Maximizing moves the top edge a long way in one step, a yank. The new
    /// edge is perfectly good to stand on, so a naive implementation snaps the
    /// sprite up to it.
    #[test]
    fn a_maximized_perch_leaves_the_sprite_behind_rather_than_snapping_it_up() {
        let world = |rect: Rect| WorldSnapshot {
            windows: vec![window(1, rect)],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        assert_eq!(
            settle(
                &mut engine,
                &world(Rect {
                    x: 50.0,
                    y: 400.0,
                    width: 300.0,
                    height: 200.0,
                })
            )
            .position
            .y,
            400.0
        );

        // Zoomed to fill the usable frame: the edge lands clear of the
        // ceiling clearance, so it would be a Perch if the sprite could reach.
        let maximized = engine.tick(&world(Rect {
            x: 0.0,
            y: 130.0,
            width: 1000.0,
            height: 670.0,
        }));
        assert_eq!(maximized.state, State::Falling, "{maximized:?}");
        assert_eq!(
            maximized.position.y, 400.0,
            "not snapped onto the new edge at 130"
        );
    }

    /// A minimized window leaves the window server's on-screen list, so the
    /// Engine sees a close. Confirmed on a real window server: present in
    /// `CGWindowListCopyWindowInfo(.optionOnScreenOnly)`, absent while miniaturized.
    #[test]
    fn a_minimized_perch_drops_the_sprite_as_a_closed_one_does() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        assert_eq!(
            settle(&mut engine, &perch(50.0, 400.0)).state,
            State::Perched
        );

        let dropped = engine.tick(&snapshot(100));
        assert_eq!(dropped.state, State::Falling, "{dropped:?}");
        assert_eq!(dropped.position.y, 400.0, "left where it stood");
        assert_eq!(settle(&mut engine, &snapshot(100)).position.y, 800.0);
    }

    /// #98: ride poll is 16 ms so the sprite can track, but the yank gate
    /// looks back ~100 ms. Six points in 16 ms after a 200 pt/s ride is
    /// 10_937 pt/s² — over `RIDE_ACCELERATION` — and 175 pt/s against the
    /// speed from a poll ago. Fast poll has to keep tracking; only the
    /// fall decision stays low-pass.
    #[test]
    fn a_short_sample_wobble_does_not_yank() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        settle(&mut engine, &perch(50.0, 400.0));
        assert_eq!(engine.tick(&perch(50.0, 380.0)).state, State::Perched);

        let mut wobble = perch(50.0, 374.0);
        wobble.elapsed_ms = 16;
        let riding = engine.tick(&wobble);
        assert_eq!(riding.state, State::Perched, "{riding:?}");
        assert_eq!(riding.position.y, 374.0);
        assert_eq!(riding.animation, "hold");
    }

    /// A yank is a loss of footing even when the window moves up over the
    /// sprite. Lifted is for a *different* window that has come to contain
    /// it, not for the Perch it just lost.
    #[test]
    fn an_upward_yank_drops_the_sprite_rather_than_lifting_it() {
        // A second window below the Perch, clear of its bottom edge and so
        // plainly visible.
        let world = |y: f64| WorldSnapshot {
            windows: vec![
                window(
                    1,
                    Rect {
                        x: 50.0,
                        y,
                        width: 300.0,
                        height: 200.0,
                    },
                ),
                window(
                    2,
                    Rect {
                        x: 50.0,
                        y: 650.0,
                        width: 300.0,
                        height: 150.0,
                    },
                ),
            ],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        assert_eq!(settle(&mut engine, &world(400.0)).position.y, 400.0);

        // Far enough to exceed the ride gate, near enough that the sprite is
        // still inside the rectangle.
        let yanked = engine.tick(&world(250.0));
        assert_eq!(yanked.state, State::Falling, "{yanked:?}");
        assert_eq!(
            yanked.position.y, 400.0,
            "it is not carried onto the new edge"
        );

        let landed = settle(&mut engine, &world(250.0));
        assert_eq!(landed.state, State::Perched, "{landed:?}");
        assert_eq!(
            landed.position.y, 650.0,
            "the window passed through it, and it fell to the next Perch below"
        );
    }

    /// With its Perch gone the sprite is in the air. A window that comes over
    /// it in that same tick is not a rescue: stepping up onto a window is a
    /// step up from something.
    #[test]
    fn a_perch_that_moves_drops_the_sprite_even_as_a_window_arrives_over_it() {
        // Frontmost first: the window that arrives is in front of the Perch.
        let world = |cover_x: f64, perch_x: f64| WorldSnapshot {
            windows: vec![
                window(
                    1,
                    Rect {
                        x: cover_x,
                        y: 100.0,
                        width: 900.0,
                        height: 500.0,
                    },
                ),
                window(
                    2,
                    Rect {
                        x: perch_x,
                        y: 400.0,
                        width: 300.0,
                        height: 200.0,
                    },
                ),
            ],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 100.0, y: 150.0 });

        let perched = settle(&mut engine, &world(-1000.0, 50.0));
        assert_eq!(perched.state, State::Perched);
        assert_eq!(perched.position.y, 400.0);

        // In one poll: the Perch is dragged away and the other window arrives
        // over where the sprite was standing.
        let dropped = engine.tick(&world(0.0, 600.0));
        assert_eq!(
            dropped.state,
            State::Falling,
            "and not lifted onto the newcomer: {dropped:?}"
        );

        let landed = settle(&mut engine, &world(0.0, 600.0));
        assert_eq!(landed.state, State::Grounded);
        assert_eq!(landed.position.y, 800.0, "down to the floor it left");
    }

    /// Strictly inside: a top edge is a Perch to stand on, not somewhere the
    /// sprite has been swallowed.
    fn inside_a_window(position: Point, snapshot: &WorldSnapshot) -> bool {
        snapshot.windows.iter().any(|window| {
            position.x > window.rect.x
                && position.x < window.rect.x + window.rect.width
                && position.y > window.rect.y
                && position.y < window.rect.y + window.rect.height
        })
    }

    /// The overlay is always on top, so a sprite standing inside a window is
    /// not hidden by it — it is drawn floating in the middle, sitting on nothing.
    #[test]
    fn a_window_dragged_over_the_sprite_lifts_it_onto_its_edge() {
        let perch = window(
            2,
            Rect {
                x: 0.0,
                y: 400.0,
                width: 1000.0,
                height: 200.0,
            },
        );
        let resting = WorldSnapshot {
            windows: vec![perch],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 500.0, y: 0.0 });
        assert_eq!(settle(&mut engine, &resting).position.y, 400.0);

        let covered = WorldSnapshot {
            windows: vec![
                window(
                    1,
                    Rect {
                        x: 200.0,
                        y: 200.0,
                        width: 800.0,
                        height: 400.0,
                    },
                ),
                perch,
            ],
            ..snapshot(100)
        };
        let lifted = engine.tick(&covered);
        assert_eq!(
            lifted.position.y, 200.0,
            "up onto the new top edge: {lifted:?}"
        );
        assert_eq!(lifted.state, State::Perched);
        assert!(!inside_a_window(lifted.position, &covered));
    }

    /// And only onto an edge the sprite can stand on. The window dragged over
    /// it here has its own top edge over no display, so the sprite drops to the
    /// floor instead of being lifted off the screens.
    #[test]
    fn a_window_dragged_over_the_sprite_never_lifts_it_off_the_displays() {
        // Bottom-aligned displays of different heights: nothing covers
        // x 1000..2000 above y=300.
        let world = |windows: Vec<Window>| WorldSnapshot {
            displays: vec![
                one_display(),
                Rect {
                    x: 1000.0,
                    y: 300.0,
                    width: 1000.0,
                    height: 500.0,
                },
            ],
            windows,
            ..snapshot(100)
        };
        let perch = window(
            1,
            Rect {
                x: 1000.0,
                y: 500.0,
                width: 600.0,
                height: 200.0,
            },
        );
        let mut engine = Engine::new(Point {
            x: 1200.0,
            y: 350.0,
        });
        assert_eq!(settle(&mut engine, &world(vec![perch])).position.y, 500.0);

        // Dragged over the sprite, with its top edge up where only the taller
        // display reaches.
        let dragged = window(
            2,
            Rect {
                x: 1100.0,
                y: 100.0,
                width: 800.0,
                height: 500.0,
            },
        );
        let frames: Vec<Frame> = (0..40)
            .map(|_| engine.tick(&world(vec![dragged, perch])))
            .collect();
        assert!(
            frames.iter().all(|frame| frame.position.y >= 300.0),
            "it is never put where no display covers it: {frames:?}"
        );
        let landed = frames.last().expect("forty ticks produce forty frames");
        assert_eq!(landed.state, State::Grounded);
        assert_eq!(landed.position.y, 800.0, "down to the floor: {landed:?}");
    }

    /// A window only swallows the sprite when it is drawn in front of the Perch
    /// the sprite stands on. This one is dragged behind that Perch, so the edge
    /// under the sprite stays in plain sight.
    #[test]
    fn a_window_behind_the_perch_does_not_swallow_the_sprite() {
        let perch = window(
            2,
            Rect {
                x: 200.0,
                y: 400.0,
                width: 400.0,
                height: 200.0,
            },
        );
        // Frontmost first: the Perch is in front, the dragged window behind it.
        let world = |x: f64| WorldSnapshot {
            windows: vec![
                perch,
                window(
                    1,
                    Rect {
                        x,
                        y: 100.0,
                        width: 900.0,
                        height: 500.0,
                    },
                ),
            ],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 400.0, y: 150.0 });

        let perched = settle(&mut engine, &world(-1000.0));
        assert_eq!(perched.state, State::Perched);
        assert_eq!(perched.position.y, 400.0);

        // Dragged in from the left, under the sprite and over its Perch.
        let covered = engine.tick(&world(0.0));
        assert_eq!(
            covered.position.y, 400.0,
            "still on the edge in front of it: {covered:?}"
        );
        assert_eq!(covered.state, State::Perched);
    }

    /// A Perch you cannot see is gone: alt-tab puts another window in front and
    /// the edge under the sprite disappears. It falls, and is not hoisted onto
    /// the raised window's top edge, which already contained it.
    #[test]
    fn a_window_raised_over_the_perch_drops_the_sprite() {
        let maximized = window(
            1,
            Rect {
                // Its top edge under the menu bar, its bottom on the usable
                // floor.
                x: 0.0,
                y: 30.0,
                width: 1000.0,
                height: 770.0,
            },
        );
        let perch = window(
            2,
            Rect {
                x: 200.0,
                y: 400.0,
                width: 400.0,
                height: 200.0,
            },
        );
        // Frontmost first, so the sprite lands on the Perch in front of the
        // maximized window and is inside that window from the moment it does.
        let world = |windows: Vec<Window>| WorldSnapshot {
            windows,
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 400.0, y: 150.0 });

        let perched = settle(&mut engine, &world(vec![perch, maximized]));
        assert_eq!(perched.state, State::Perched);
        assert_eq!(perched.position.y, 400.0, "the window it fell onto");

        // The user clicks the maximized window, which comes to the front.
        let raised = world(vec![maximized, perch]);
        let frames: Vec<Frame> = (0..40).map(|_| engine.tick(&raised)).collect();
        assert!(
            frames.iter().any(|frame| frame.state == State::Falling),
            "the hidden edge is gone, so it falls: {frames:?}"
        );
        assert!(
            frames.iter().all(|frame| frame.position.y > 30.0),
            "and is not hoisted under the menu bar: {frames:?}"
        );
        let landed = frames.last().expect("forty ticks produce forty frames");
        assert_eq!(landed.state, State::Grounded);
        assert_eq!(landed.position.y, 800.0, "down to the floor: {landed:?}");
    }

    /// The same rule met by walking rather than by a window moving: the sprite
    /// strolls along one edge into the middle of a window that overlaps it.
    #[test]
    fn a_walk_under_an_overlapping_window_steps_up_onto_it() {
        let overlapping = || WorldSnapshot {
            windows: vec![
                window(
                    1,
                    Rect {
                        x: 500.0,
                        y: 250.0,
                        width: 500.0,
                        height: 400.0,
                    },
                ),
                window(
                    2,
                    Rect {
                        x: 0.0,
                        y: 400.0,
                        width: 1000.0,
                        height: 200.0,
                    },
                ),
            ],
            ..snapshot(100)
        };
        let mut engine = a_character_at(Point { x: 100.0, y: 0.0 });
        assert_eq!(settle(&mut engine, &overlapping()).position.y, 400.0);

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..overlapping()
        });
        let walked: Vec<Frame> = (0..40).map(|_| engine.tick(&overlapping())).collect();

        assert!(
            walked.iter().any(|frame| frame.position.y == 250.0),
            "it steps up onto the window it walked under: {walked:?}"
        );
        assert!(
            walked
                .iter()
                .filter(|frame| matches!(
                    frame.state,
                    State::Grounded | State::Perched | State::Asleep
                ))
                .all(|frame| !inside_a_window(frame.position, &overlapping())),
            "and never comes to rest inside one: {walked:?}"
        );
    }

    /// Two windows overlapping in x, the upper one hanging below the usable
    /// floor as a window behind the Dock does.
    fn overlapping_windows() -> WorldSnapshot {
        WorldSnapshot {
            windows: vec![
                window(
                    1,
                    Rect {
                        x: 0.0,
                        y: 300.0,
                        width: 600.0,
                        height: 600.0,
                    },
                ),
                window(
                    2,
                    Rect {
                        x: 200.0,
                        y: 500.0,
                        width: 600.0,
                        height: 300.0,
                    },
                ),
            ],
            ..snapshot(100)
        }
    }

    /// Two edges under one sprite is the arrangement that would have it
    /// flicking between them, one per tick, for as long as both windows are open.
    #[test]
    fn overlapping_windows_resolve_to_one_perch_without_jitter() {
        let mut engine = Engine::new(Point { x: 400.0, y: 0.0 });
        let falling: Vec<Frame> = (0..40)
            .map(|_| engine.tick(&overlapping_windows()))
            .collect();
        let landed = falling.last().expect("forty ticks produce forty frames");

        assert_eq!(landed.state, State::Perched);
        assert_eq!(landed.position.y, 300.0, "the upper of the two edges");
        assert!(
            falling.iter().all(|frame| frame.position.y <= 300.0),
            "and it never fell past that edge to the lower one: {falling:?}"
        );

        let settled: Vec<(Point, State)> = (0..20)
            .map(|_| {
                let frame = engine.tick(&overlapping_windows());
                (frame.position, frame.state)
            })
            .collect();
        assert!(
            settled
                .iter()
                .all(|resting| *resting == (landed.position, landed.state)),
            "it sits still rather than flicking between them: {settled:?}"
        );
    }

    #[test]
    fn an_edge_hidden_behind_the_window_in_front_of_it_is_not_a_perch() {
        // Frontmost first, and the second window's top edge falls inside the
        // first: only its lower half is anywhere on screen.
        let covered = WorldSnapshot {
            windows: vec![
                window(
                    1,
                    Rect {
                        x: 100.0,
                        y: 300.0,
                        width: 800.0,
                        height: 500.0,
                    },
                ),
                window(
                    2,
                    Rect {
                        x: 100.0,
                        y: 600.0,
                        width: 800.0,
                        height: 200.0,
                    },
                ),
            ],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 500.0, y: 400.0 });

        let landed = settle(&mut engine, &covered);
        assert_eq!(landed.state, State::Grounded);
        assert_eq!(
            landed.position.y, 800.0,
            "past the hidden edge and down to the floor: {landed:?}"
        );
    }

    /// One window straddles two displays of different heights, and the sprite
    /// is thrown out over the shorter one, where that edge hangs over nothing.
    #[test]
    fn an_edge_over_no_display_is_not_a_perch() {
        // Bottom-aligned displays of different heights, the ordinary
        // arrangement: nothing covers x 1000..2000 above y=300.
        let world = WorldSnapshot {
            displays: vec![
                one_display(),
                Rect {
                    x: 1000.0,
                    y: 300.0,
                    width: 1000.0,
                    height: 500.0,
                },
            ],
            windows: vec![window(
                1,
                Rect {
                    x: 700.0,
                    y: 100.0,
                    width: 1000.0,
                    height: 400.0,
                },
            )],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 900.0, y: 50.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 50.0 },
            verbs: vec![Verb::Grab],
            ..world.clone()
        });
        // Thrown right, hard enough to cross onto the shorter display before
        // gravity has taken it down to that display's top.
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..world.clone()
        });

        let landed = settle(&mut engine, &world);
        assert_eq!(landed.state, State::Grounded);
        assert!(
            covered(landed.position, &world),
            "it came to rest somewhere a display covers: {landed:?}"
        );
    }

    #[test]
    fn a_grab_takes_the_sprite_over_and_letting_go_drops_it() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });

        let held = engine.tick(&WorldSnapshot {
            cursor: Point { x: 400.0, y: 250.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        assert_eq!(held.state, State::Dragged);
        assert_eq!(
            held.position,
            Point { x: 400.0, y: 250.0 },
            "follows the cursor"
        );

        let dragged_on = engine.tick(&WorldSnapshot {
            cursor: Point { x: 420.0, y: 240.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        assert_eq!(dragged_on.position, Point { x: 420.0, y: 240.0 });

        // The Grab is gone from the snapshot: the user let go.
        let released = engine.tick(&snapshot(100));
        assert_eq!(released.state, State::Falling);
        assert!(released.position.y > 240.0, "it drops: {released:?}");
    }

    #[test]
    fn letting_go_with_velocity_throws_the_sprite_instead_of_dropping_it() {
        let mut engine = Engine::new(Point { x: 100.0, y: 400.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 200.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });

        // A production tick, not the 100 ms the other tests use to settle. On a
        // long tick gravity wins a 200-point throw in one step. Started below
        // the ceiling so the rise is not stopped in the same tick.
        let thrown = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point {
                    x: 300.0,
                    y: -200.0,
                },
            }],
            ..snapshot(16)
        });

        assert_eq!(thrown.state, State::Falling);
        assert_eq!(thrown.velocity.x, 300.0, "gravity does not slow the arc");
        assert!(thrown.position.x > 200.0, "it travels across: {thrown:?}");
        assert!(
            thrown.position.y < 400.0,
            "an upward throw rises before it falls: {thrown:?}"
        );
    }

    /// The usable top is a ceiling, and the sprite is heavy enough to come
    /// back down onto a Surface.
    #[test]
    fn a_hard_upward_throw_stays_on_the_display_and_lands() {
        let mut engine = Engine::new(Point { x: 500.0, y: 400.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 500.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(16)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point {
                    x: 400.0,
                    y: -2500.0,
                },
            }],
            ..snapshot(16)
        });

        let flight: Vec<Frame> = (0..180).map(|_| engine.tick(&snapshot(16))).collect();
        let highest = flight
            .iter()
            .map(|frame| frame.position.y)
            .min_by(|a, b| a.total_cmp(b))
            .expect("the flight produces frames");
        assert!(
            highest >= 0.0,
            "left the display at y={highest}, {flight:?}"
        );
        assert!(
            flight.iter().all(|frame| frame.position.y <= 800.0),
            "fell past the floor: {flight:?}"
        );
        let landed = flight.last().expect("the flight produces frames");
        assert_eq!(landed.state, State::Grounded, "it came to rest: {landed:?}");
        assert_eq!(landed.position.y, 800.0, "on the floor: {landed:?}");
    }

    /// Landing there leaves only the feet on screen, and often unclickable.
    #[test]
    fn a_window_flush_with_the_usable_top_is_not_a_perch() {
        let under_the_menu_bar = WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 0.0,
                    y: 30.0,
                    width: 1000.0,
                    height: 770.0,
                },
            )],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 500.0, y: 0.0 });

        let landed = settle(&mut engine, &under_the_menu_bar);
        assert_eq!(
            landed.state,
            State::Grounded,
            "past the title bar: {landed:?}"
        );
        assert_eq!(landed.position.y, 800.0, "down to the floor: {landed:?}");
    }

    #[test]
    fn a_sprite_in_mid_air_falls_and_comes_to_rest_on_the_floor() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });

        let first = engine.tick(&snapshot(100));
        assert_eq!(first.state, State::Falling);
        assert!(first.position.y > 0.0, "it descends: {first:?}");

        let landed = (0..40).map(|_| engine.tick(&snapshot(100))).last().unwrap();

        assert_eq!(landed.state, State::Grounded);
        assert_eq!(landed.position.y, 800.0, "the display's bottom edge");
        assert_eq!(landed.velocity, Point::default(), "at rest");
    }

    #[test]
    fn a_window_is_passed_through_from_below_and_landed_on_from_above() {
        let window = WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 50.0,
                    y: 400.0,
                    width: 300.0,
                    height: 200.0,
                },
            )],
            ..snapshot(100)
        };
        let mut engine = Engine::new(Point { x: 100.0, y: 550.0 });

        // Held inside the window, below its top edge, and flung straight up.
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 100.0, y: 550.0 },
            verbs: vec![Verb::Grab],
            ..window.clone()
        });
        let thrown = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 0.0, y: -2500.0 },
            }],
            ..window.clone()
        });
        assert_eq!(
            thrown.state,
            State::Falling,
            "the edge above it is not a surface from underneath: {thrown:?}"
        );
        assert!(
            thrown.position.y < 400.0,
            "it rises through the top edge: {thrown:?}"
        );

        // The same edge, approached from above, catches it.
        let perched = settle(&mut engine, &window);
        assert_eq!(perched.state, State::Perched);
        assert_eq!(perched.position.y, 400.0, "the window's top edge");
    }

    #[test]
    fn a_sprite_over_no_display_is_recovered_onto_the_nearest_one() {
        // A display was unplugged out from under it: nothing spans its x, so
        // there is no floor beneath it and nothing to fall towards.
        let mut engine = Engine::new(Point { x: 2000.0, y: 0.0 });

        let caught = engine.tick(&snapshot(100));
        assert_eq!(caught.position.x, 1000.0, "hauled back to the nearest edge");

        let landed = settle(&mut engine, &snapshot(100));
        assert_eq!(
            landed.state,
            State::Grounded,
            "it stops falling: {landed:?}"
        );
        assert_eq!(landed.position.y, 800.0, "the display's bottom edge");
    }

    #[test]
    fn a_throw_is_the_release_of_a_grab_so_a_resting_sprite_ignores_one() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        let resting = settle(&mut engine, &snapshot(100));
        assert_eq!(resting.state, State::Grounded);

        let unmoved = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point {
                    x: 2000.0,
                    y: -400.0,
                },
            }],
            ..snapshot(100)
        });
        assert_eq!(unmoved.state, State::Grounded, "not flung: {unmoved:?}");
        assert_eq!(unmoved.position, resting.position);
        assert_eq!(unmoved.velocity, Point::default());
    }

    /// Two displays with a gap between them: the union of visible display
    /// frames, not their bounding rectangle. A sprite in the gap is over no
    /// display at all: nothing holds it up, nothing draws it, nothing brings it back.
    fn displays_with_a_gap() -> WorldSnapshot {
        WorldSnapshot {
            displays: vec![
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 800.0,
                },
                Rect {
                    x: 1500.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 800.0,
                },
            ],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        }
    }

    /// Dropped rather than thrown: a throw carries enough sideways speed to
    /// clear the gap before gravity matters. Letting go leaves no horizontal
    /// velocity, so it stays over nothing with no edge to catch.
    #[test]
    fn a_sprite_dropped_into_the_gap_between_displays_is_caught_rather_than_lost() {
        let mut engine = Engine::new(Point { x: 900.0, y: 100.0 });

        // Carried by hand into the gap, then let go still.
        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: 1200.0,
                y: 100.0,
            },
            verbs: vec![Verb::Grab],
            ..displays_with_a_gap()
        });
        // Let go. The gap is caught on that same tick rather than after a
        // fall, because the sprite is already over nothing when the hand opens.
        engine.tick(&displays_with_a_gap());

        let landed = settle(&mut engine, &displays_with_a_gap());
        assert!(
            landed.position.y <= 800.0,
            "it came to rest rather than falling for ever: {landed:?}"
        );
        assert!(
            landed.position.x <= 1000.0 || landed.position.x >= 1500.0,
            "and over a display rather than in the gap: {landed:?}"
        );
    }

    /// The same gap from the other side, so the fix cannot be a one-sided clamp.
    #[test]
    fn the_gap_catches_a_sprite_dropped_nearer_its_far_edge() {
        let mut engine = Engine::new(Point {
            x: 1600.0,
            y: 100.0,
        });

        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: 1400.0,
                y: 100.0,
            },
            verbs: vec![Verb::Grab],
            ..displays_with_a_gap()
        });
        engine.tick(&displays_with_a_gap());

        let landed = settle(&mut engine, &displays_with_a_gap());
        assert!(landed.position.y <= 800.0, "it rests: {landed:?}");
        assert!(
            landed.position.x >= 1500.0,
            "recovered to the nearer display, which is the right-hand one: {landed:?}"
        );
    }

    /// L-shaped: the bounding rectangle and the union differ in y. It does not
    /// strand the sprite: every x in the bounding rectangle is spanned by some
    /// display, so there is always a floor somewhere below.
    #[test]
    fn an_l_shaped_arrangement_leaves_the_sprite_a_floor_everywhere() {
        let l_shaped = || WorldSnapshot {
            displays: vec![
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 800.0,
                },
                Rect {
                    x: 1000.0,
                    y: 400.0,
                    width: 1000.0,
                    height: 800.0,
                },
            ],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        };

        // Over the second display but above where it begins: inside the
        // bounding rectangle, outside the union.
        let mut engine = Engine::new(Point {
            x: 1500.0,
            y: 100.0,
        });
        let landed = settle(&mut engine, &l_shaped());

        assert_eq!(landed.state, State::Grounded);
        assert_eq!(
            landed.position.y, 1200.0,
            "it falls past the empty space onto the lower display"
        );
        assert_eq!(landed.position.x, 1500.0, "and does not drift sideways");
    }

    /// Two displays meeting at a corner. The x ranges still touch, so there
    /// is no gap to fall into.
    #[test]
    fn a_diagonal_arrangement_leaves_the_sprite_a_floor_everywhere() {
        let diagonal = || WorldSnapshot {
            displays: vec![
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 800.0,
                },
                Rect {
                    x: 1000.0,
                    y: 800.0,
                    width: 1000.0,
                    height: 800.0,
                },
            ],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        };

        let mut engine = Engine::new(Point {
            x: 1500.0,
            y: 100.0,
        });
        let landed = settle(&mut engine, &diagonal());

        assert_eq!(landed.state, State::Grounded);
        assert_eq!(landed.position.y, 1600.0);
        assert!(
            landed.position.x >= 1000.0 && landed.position.x <= 2000.0,
            "over the display it landed on: {landed:?}"
        );
    }

    /// Displays that touch have no gap, and the sprite must cross freely. A
    /// clamp that treats every display edge as a wall would trap it on one
    /// screen.
    #[test]
    fn a_sprite_crosses_freely_between_displays_that_touch() {
        let touching = || WorldSnapshot {
            displays: vec![
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 800.0,
                },
                Rect {
                    x: 1000.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 800.0,
                },
            ],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        };

        let mut engine = Engine::new(Point { x: 900.0, y: 100.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 100.0 },
            verbs: vec![Verb::Grab],
            ..touching()
        });
        // Gently, so it lands on the second display rather than sailing past
        // it to the outer edge — which is a catch, and a different test.
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 400.0, y: 0.0 },
            }],
            ..touching()
        });

        let landed = settle(&mut engine, &touching());
        assert!(
            landed.position.x > 1000.0,
            "it crossed onto the second display: {landed:?}"
        );
        assert_eq!(landed.position.y, 800.0, "and stands on its floor");
        assert_eq!(
            landed.state,
            State::Grounded,
            "rather than climbing an edge"
        );
    }

    /// The same throw with the second display left out of the world (e.g.,
    /// unplugged): the remaining display's edge is the outer wall now, and
    /// the sprite stops there.
    #[test]
    fn a_sprite_stops_at_the_seam_of_a_display_left_out() {
        let alone = || WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 800.0,
            }],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        };

        let mut engine = Engine::new(Point { x: 900.0, y: 100.0 });
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 100.0 },
            verbs: vec![Verb::Grab],
            ..alone()
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 400.0, y: 0.0 },
            }],
            ..alone()
        });

        let landed = settle(&mut engine, &alone());
        // Half a default sprite in from the seam at 1000, on the first floor.
        assert_eq!(landed.position, Point { x: 936.0, y: 800.0 });
        assert_eq!(landed.state, State::Grounded);
    }

    #[test]
    fn a_proposal_offered_under_do_not_disturb_is_not_applied() {
        let mut engine = a_resting_sprite();
        engine.set_do_not_disturb(true);

        let refused = engine.tick(&proposing("greet"));

        assert_eq!(
            refused.animation, "idle",
            "the proposal was refused and the sprite stays idle"
        );
        assert_eq!(
            refused.behavior, None,
            "no Behavior started playing on this frame"
        );
        assert!(
            refused.position.y <= 800.0,
            "the Character stays on screen: {refused:?}"
        );
    }

    /// Turning Do Not Disturb off resumes proposals on the next wake without
    /// reconstructing the Engine.
    #[test]
    fn the_same_proposal_is_applied_once_do_not_disturb_is_off() {
        let mut engine = a_resting_sprite();
        engine.set_do_not_disturb(true);

        engine.tick(&proposing("greet"));

        engine.set_do_not_disturb(false);
        let applied = engine.tick(&proposing("greet"));

        assert_eq!(
            applied.animation, "react",
            "the proposal is applied once Do Not Disturb is off"
        );
        assert_eq!(applied.behavior, Some("greet".to_string()));
    }

    /// Poke still plays `react`, the user-initiated reaction.
    #[test]
    fn poke_still_plays_react_while_do_not_disturb_is_on() {
        let mut engine = a_resting_sprite();
        engine.set_do_not_disturb(true);

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            ..snapshot(100)
        });

        assert_eq!(
            poked.animation, "react",
            "Poke still plays react while Do Not Disturb is on"
        );
    }

    #[test]
    fn grab_and_throw_still_move_the_sprite_under_do_not_disturb() {
        let mut engine = a_resting_sprite();
        let start = engine.tick(&snapshot(100)).position;
        engine.set_do_not_disturb(true);

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 400.0, y: 100.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        let grabbed = engine.tick(&WorldSnapshot {
            cursor: Point { x: 600.0, y: 100.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });

        assert_eq!(grabbed.state, State::Dragged);
        assert!(
            (grabbed.position.x - 600.0).abs() < 0.1,
            "Grab moved the sprite: {grabbed:?}"
        );

        let thrown = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point {
                    x: 500.0,
                    y: -200.0,
                },
            }],
            ..snapshot(100)
        });

        assert_eq!(thrown.state, State::Falling);
        assert!(
            thrown.position.x > start.x,
            "Throw moved the sprite: start={start:?}, thrown={thrown:?}"
        );
    }

    #[test]
    fn unprompted_director_dialogue_is_not_spoken_under_do_not_disturb() {
        let mut engine = a_resting_sprite();
        engine.set_do_not_disturb(true);

        let silent = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "greet".to_string(),
                dialogue: Some("hello there".to_string()),
            }),
            ..snapshot(100)
        });

        assert_eq!(
            silent.dialogue, None,
            "unprompted dialogue is refused under Do Not Disturb"
        );
    }

    #[test]
    fn idle_behaviors_do_not_start_and_the_buddy_settles_to_sleep() {
        let mut engine = a_resting_sprite();
        engine.set_do_not_disturb(true);

        let quietening: Vec<Frame> = (0..20)
            .map(|_| {
                engine.tick(&WorldSnapshot {
                    proposal: Some(BehaviorProposal {
                        behavior: "greet".to_string(),
                        dialogue: None,
                    }),
                    ..snapshot(100)
                })
            })
            .collect();

        assert!(
            quietening.iter().all(|frame| frame.animation == "idle"),
            "proposals do not start Behaviors while Do Not Disturb is on"
        );

        let asleep = engine.tick(&snapshot(60_000));
        assert_eq!(
            asleep.state,
            State::Asleep,
            "the sprite settles to sleep without Director proposals waking it"
        );
    }

    /// Walk velocity outlives the Primitive that started it, so refusing the
    /// next proposal is not enough. Sit is what stops the feet.
    #[test]
    fn toggling_do_not_disturb_stops_a_walk_and_sits_the_sprite_down() {
        let mut engine = a_character_at(Point { x: 200.0, y: 0.0 });
        settle(&mut engine, &a_long_perch());

        engine.tick(&WorldSnapshot {
            proposal: walk(),
            ..a_long_perch()
        });
        let strolling = engine.tick(&a_long_perch());
        assert_eq!(
            strolling.animation, "walk",
            "precondition: the sprite is walking"
        );
        assert_ne!(
            strolling.velocity.x, 0.0,
            "precondition: the walk has a heading"
        );

        engine.set_do_not_disturb(true);
        let settled = engine.tick(&a_long_perch());

        assert_eq!(settled.animation, "sit");
        assert_eq!(settled.velocity.x, 0.0, "sit is what stops the walk");
        assert_eq!(
            settled.state,
            State::Perched,
            "quiet is sit, not gone and not asleep"
        );

        let rest: Vec<Frame> = (0..20).map(|_| engine.tick(&a_long_perch())).collect();
        assert!(
            rest.iter()
                .all(|frame| frame.velocity.x == 0.0 && frame.animation == "sit"),
            "the walk does not resume: {rest:?}"
        );
    }

    #[test]
    fn a_sprite_standing_at_the_left_edge_faces_right() {
        let mut engine = Engine::new(Point { x: 0.0, y: 400.0 });

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 0.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        let _thrown_left = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: -500.0, y: 0.0 },
            }],
            ..snapshot(100)
        });

        let at_left_edge = settle(&mut engine, &snapshot(100));
        assert_eq!(
            at_left_edge.state,
            State::Grounded,
            "sprite lands on the floor after hitting left edge"
        );
        assert_eq!(
            at_left_edge.facing, 1.0,
            "sprite at left edge must face right (away from edge)"
        );
    }

    #[test]
    fn a_sprite_standing_at_the_right_edge_faces_left() {
        let mut engine = Engine::new(Point { x: 500.0, y: 400.0 });

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 500.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        let _thrown_right = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..snapshot(100)
        });

        let at_right_edge = settle(&mut engine, &snapshot(100));
        assert_eq!(
            at_right_edge.state,
            State::Grounded,
            "sprite lands on the floor after hitting right edge"
        );
        assert_eq!(
            at_right_edge.facing, -1.0,
            "sprite at right edge must face left (away from edge)"
        );
    }

    #[test]
    fn a_sprite_at_left_edge_is_fully_on_screen() {
        let mut engine = Engine::new(Point { x: 50.0, y: 400.0 });

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 50.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: -500.0, y: 0.0 },
            }],
            ..snapshot(100)
        });

        let at_left_edge = settle(&mut engine, &snapshot(100));
        assert_eq!(
            at_left_edge.position.x, EDGE_CLEARANCE,
            "sprite position must be inset by EDGE_CLEARANCE from left edge"
        );
    }

    #[test]
    fn a_sprite_at_right_edge_is_fully_on_screen() {
        let mut engine = Engine::new(Point { x: 950.0, y: 400.0 });

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 950.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 500.0, y: 0.0 },
            }],
            ..snapshot(100)
        });

        let at_right_edge = settle(&mut engine, &snapshot(100));
        let right_edge = one_display().x + one_display().width;
        assert_eq!(
            at_right_edge.position.x,
            right_edge - EDGE_CLEARANCE,
            "sprite position must be inset by EDGE_CLEARANCE from right edge"
        );
    }

    #[test]
    fn climbing_preserves_wall_centered_frames_and_may_clip() {
        let mut engine = Engine::new(Point { x: 900.0, y: 400.0 });

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        let climbing = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..snapshot(100)
        });

        assert_eq!(climbing.state, State::Climbing);
        assert_eq!(
            climbing.position.x, 1000.0,
            "during climb, position stays at the wall edge for wall-centered frames"
        );
    }

    #[test]
    fn dragging_to_the_edge_does_not_snap_position_or_facing() {
        let mut engine = Engine::new(Point { x: 900.0, y: 400.0 });

        // Establish facing -1.0 by throwing left and settling at the right edge
        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 500.0, y: 0.0 },
            }],
            ..snapshot(100)
        });
        let at_right = settle(&mut engine, &snapshot(100));
        assert_eq!(
            at_right.facing, -1.0,
            "facing left after settling at right edge"
        );

        let dragged = engine.tick(&WorldSnapshot {
            cursor: Point { x: 10.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });

        assert_eq!(dragged.state, State::Dragged);
        assert_eq!(
            dragged.position.x, 10.0,
            "dragged sprite follows cursor exactly, even near edge"
        );
        assert_eq!(
            dragged.facing, -1.0,
            "facing unchanged while dragged; no snap to face away"
        );
    }

    #[test]
    fn after_climb_ends_at_edge_standing_is_inset_and_faces_away() {
        let mut engine = Engine::new(Point { x: 900.0, y: 400.0 });

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            verbs: vec![Verb::Grab],
            ..snapshot(100)
        });
        engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            }],
            ..snapshot(100)
        });

        let landed = settle(&mut engine, &snapshot(100));
        assert_eq!(landed.state, State::Grounded);

        let right_edge = one_display().x + one_display().width;
        assert_eq!(
            landed.position.x,
            right_edge - EDGE_CLEARANCE,
            "after climb ends, standing position is inset from edge"
        );
        assert_eq!(
            landed.facing, -1.0,
            "after climb ends, sprite faces away from right edge"
        );
    }

    #[test]
    fn a_perched_sprite_near_the_edge_is_not_shoved_off_its_ledge() {
        // Window near right edge that does NOT include the edge snap position.
        // Right edge snap would be at x=936. Window spans x=950 to x=990.
        let narrow_perch = WorldSnapshot {
            windows: vec![window(
                1,
                Rect {
                    x: 950.0,
                    y: 200.0,
                    width: 40.0,
                    height: 100.0,
                },
            )],
            ..snapshot(100)
        };

        // Start away from edges, so no correction yet
        let mut engine = Engine::new(Point { x: 500.0, y: 0.0 });
        settle(&mut engine, &snapshot(100));

        engine.tick(&WorldSnapshot {
            cursor: Point { x: 970.0, y: 200.0 },
            verbs: vec![Verb::Grab],
            ..narrow_perch.clone()
        });
        let placed = engine.tick(&WorldSnapshot {
            verbs: vec![],
            ..narrow_perch.clone()
        });

        assert_eq!(placed.state, State::Perched, "sprite is on the perch");
        assert!(
            placed.position.x >= 950.0 && placed.position.x <= 990.0,
            "sprite stays on perch span [950, 990], not moved to x=936"
        );
        assert_eq!(placed.facing, -1.0, "sprite faces away from right edge");
    }

    /// Talk plays for PRIMITIVE_MS, independent of bubble duration.
    #[test]
    fn dialogue_with_empty_behavior_plays_talk() {
        let mut engine = a_resting_sprite();

        let spoken = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: String::new(),
                dialogue: Some("hello there".to_string()),
            }),
            ..snapshot(100)
        });

        assert_eq!(
            spoken.animation, "talk",
            "dialogue with no Behavior plays talk"
        );
        assert_eq!(spoken.dialogue.as_deref(), Some("hello there"));

        let playing: Vec<&'static str> = (0..10)
            .map(|_| engine.tick(&snapshot(100)).animation)
            .collect();

        let talk_ticks = playing.iter().filter(|&&anim| anim == "talk").count();
        assert!(
            talk_ticks == 5,
            "talk holds for PRIMITIVE_MS (600ms = 5 more ticks after the first): {playing:?}"
        );
    }

    /// The Behavior and its dialogue are independent.
    #[test]
    fn dialogue_with_a_playable_behavior_plays_the_behavior_not_talk() {
        let mut engine = a_resting_sprite();

        let spoken = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "greet".to_string(),
                dialogue: Some("hi".to_string()),
            }),
            ..snapshot(100)
        });

        assert_eq!(
            spoken.animation, "react",
            "greet plays its own animation, not talk"
        );
        assert_eq!(spoken.dialogue.as_deref(), Some("hi"));
        assert_eq!(spoken.behavior, Some("greet".to_string()));
    }

    #[test]
    fn a_behavior_without_dialogue_does_not_play_talk() {
        let mut engine = a_resting_sprite();

        let silent = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "greet".to_string(),
                dialogue: None,
            }),
            ..snapshot(100)
        });

        assert_eq!(silent.animation, "react", "greet plays its own animation");
        assert_eq!(silent.dialogue, None);
        assert_eq!(silent.behavior, Some("greet".to_string()));
    }

    // Cursor awareness tests: scripted pointer tracks with no windowing system.

    #[test]
    fn near_indifferent_keeps_the_sprite_doing_whatever_it_was_doing() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Indifferent, CursorReaction::Indifferent);

        let far = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: 1000.0,
                y: 100.0,
            },
            ..snapshot(16)
        });
        assert_eq!(far.animation, "idle");

        let near = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: engine.position.x + 50.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });
        assert_eq!(near.animation, "idle", "indifferent means no reaction");
    }

    #[test]
    fn near_speak_plays_talk_when_cursor_enters_radius() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Speak, CursorReaction::Indifferent);

        let near = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: engine.position.x + 50.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });
        assert_eq!(near.animation, "talk", "speak reaction plays talk");
    }

    #[test]
    fn near_face_turns_the_sprite_toward_the_cursor() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Face, CursorReaction::Indifferent);
        engine.facing = -1.0;

        let near = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: engine.position.x + 50.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });
        assert_eq!(near.facing, 1.0, "sprite faces right toward the cursor");
    }

    #[test]
    fn near_toward_walks_the_sprite_toward_the_cursor() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Toward, CursorReaction::Indifferent);

        let near = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: engine.position.x + 100.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });
        assert_eq!(near.animation, "walk", "toward reaction starts a walk");
        assert_eq!(near.facing, 1.0, "walks toward the cursor");
        assert_eq!(
            near.velocity.x, WALK_SPEED,
            "toward must move right when the cursor is to the right, not merely face it"
        );
    }

    #[test]
    fn near_away_walks_the_sprite_away_from_the_cursor() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Away, CursorReaction::Indifferent);

        let near = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: engine.position.x + 100.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });
        assert_eq!(near.animation, "walk", "away reaction starts a walk");
        assert_eq!(near.facing, -1.0, "walks away from the cursor");
        assert_eq!(
            near.velocity.x, -WALK_SPEED,
            "away must move left when the cursor is to the right"
        );
    }

    #[test]
    fn near_react_plays_react_when_cursor_enters_radius() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::React, CursorReaction::Indifferent);

        let near = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: engine.position.x + 50.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });
        assert_eq!(near.animation, "react", "react reaction plays react");
    }

    #[test]
    fn rush_plays_the_rush_reaction_once_on_high_velocity_approach() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Indifferent, CursorReaction::React);

        // The first sample is outside Near so `last_cursor` has a position to
        // measure speed from.
        let mut last_x = engine.position.x + 300.0;
        for _ in 0..3 {
            let frame = engine.tick(&WorldSnapshot {
                cursor: Point {
                    x: last_x,
                    y: engine.position.y,
                },
                ..snapshot(16)
            });
            last_x -= 100.0; // Fast approach: 100 points per 16ms = high velocity
            if frame.animation == "react" {
                for _ in 0..5 {
                    let _staying = engine.tick(&WorldSnapshot {
                        cursor: Point {
                            x: engine.position.x + 50.0,
                            y: engine.position.y,
                        },
                        ..snapshot(16)
                    });
                }
                return;
            }
        }
        panic!("Rush reaction was not triggered");
    }

    /// Facing alone is not enough — the art can face one way and the feet
    /// the other.
    #[test]
    fn rush_toward_walks_at_the_cursor_not_away() {
        let mut engine = a_resting_sprite()
            .with_cursor_reactions(CursorReaction::Indifferent, CursorReaction::Toward);

        let start_x = engine.position.x;
        // High-velocity approach from the right, ending still to the right.
        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: start_x + 300.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });
        let rushed = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: start_x + 80.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });

        assert_eq!(rushed.animation, "walk", "rush toward starts a walk");
        assert_eq!(rushed.facing, 1.0, "faces the cursor on the right");
        assert_eq!(
            rushed.velocity.x, WALK_SPEED,
            "feet travel toward the cursor, not away from it"
        );
    }

    #[test]
    fn chase_walks_toward_cursor_and_swats_on_arrival() {
        let mut engine = a_resting_sprite();
        let sprite_x = engine.position.x;

        // Start chase with cursor close by (just outside arrival threshold), so
        // the first tick is pursuit and not a swat.
        engine.play(&[Primitive::Chase]);

        let chasing = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + CHASE_ARRIVAL_THRESHOLD + 10.0,
                y: engine.position.y,
            },
            ..snapshot(100)
        });
        assert_eq!(chasing.animation, "walk", "chase uses walk art");
        assert_eq!(chasing.facing, 1.0, "faces toward cursor");

        for _ in 0..20 {
            let frame = engine.tick(&WorldSnapshot {
                cursor: Point {
                    x: sprite_x + 50.0, // Fixed target within walking distance
                    y: engine.position.y,
                },
                ..snapshot(100)
            });

            if frame.animation == "react" {
                return;
            }
        }

        panic!(
            "Chase did not swat on arrival. Sprite position: {}, cursor at: {}",
            engine.position.x,
            sprite_x + 50.0
        );
    }

    #[test]
    fn chase_times_out_if_cursor_escapes() {
        let mut engine = a_resting_sprite();
        let sprite_x = engine.position.x;

        engine.play(&[Primitive::Chase]);

        // Chase a cursor that keeps escaping (moving away).
        let mut cursor_x = sprite_x + 300.0;
        let mut ticks = 0;
        while ticks < (CHASE_TIMEOUT_MS / 100) + 5 {
            let _frame = engine.tick(&WorldSnapshot {
                cursor: Point {
                    x: cursor_x,
                    y: engine.position.y,
                },
                ..snapshot(100)
            });
            cursor_x += 50.0; // Stays ahead so arrival never fires; timeout is the path under test.
            ticks += 1;

            if ticks > (CHASE_TIMEOUT_MS / 100) && engine.on_screen() != Some(Primitive::Chase) {
                return;
            }
        }
        panic!("Chase did not time out as expected");
    }

    #[test]
    fn chase_aborts_on_any_verb() {
        let mut engine = a_resting_sprite();
        engine.play(&[Primitive::Chase]);

        let poked = engine.tick(&WorldSnapshot {
            verbs: vec![Verb::Poke],
            cursor: Point {
                x: engine.position.x + 200.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });

        assert_eq!(
            poked.animation, "react",
            "Poke aborts chase and plays react"
        );
        assert!(
            engine.on_screen() != Some(Primitive::Chase),
            "chase is no longer playing"
        );
    }

    #[test]
    fn chase_is_refused_under_do_not_disturb() {
        let mut engine = a_resting_sprite();
        engine.set_do_not_disturb(true);

        let frame = engine.tick(&WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "chase-test".to_string(),
                dialogue: None,
            }),
            cursor: Point {
                x: engine.position.x + 200.0,
                y: engine.position.y,
            },
            ..snapshot(16)
        });

        assert_eq!(frame.animation, "idle", "DND refuses chase proposals");
    }

    /// Near and Rush reactions still play under Do Not Disturb, like Poke.
    #[test]
    fn cursor_reactions_play_under_do_not_disturb() {
        let mut engine =
            a_resting_sprite().with_cursor_reactions(CursorReaction::Speak, CursorReaction::React);
        engine.set_do_not_disturb(true);

        let sprite_x = engine.position.x;
        let sprite_y = engine.position.y;

        // Start with cursor far away. Seeds `last_cursor` so the enter below is
        // a crossing, not a spawn.
        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + 500.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });

        // Slowly approach to avoid triggering Rush. Outside Near, speed cannot
        // Rush at all; the last step (160 → 149) is 11 points in 16 ms =
        // 687 pt/s, under `RUSH_VELOCITY`.
        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + 450.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });

        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + 400.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });

        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + 250.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });

        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + 200.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });

        engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + 160.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });

        let near = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + 149.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });
        assert_eq!(
            near.animation, "talk",
            "Near reaction (speak) plays under DND (answers the user)"
        );
    }

    #[test]
    fn dwell_addresses_the_director() {
        let mut engine = a_resting_sprite();
        let sprite_x = engine.position.x;
        let sprite_y = engine.position.y;

        let on_sprite = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x,
                y: sprite_y,
            },
            ..snapshot(16)
        });
        assert!(!on_sprite.addressed, "not addressed yet");

        // Cursor rests for half the dwell threshold.
        let dwelling = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x,
                y: sprite_y,
            },
            elapsed_ms: DWELL_MS / 2,
            ..snapshot(16)
        });
        assert!(!dwelling.addressed, "still not addressed yet");

        let addressed = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x,
                y: sprite_y,
            },
            elapsed_ms: DWELL_MS / 2 + 1,
            ..snapshot(16)
        });
        assert!(addressed.addressed, "addressed after dwell threshold");
        assert_eq!(addressed.animation, "talk", "plays talk's first moment");

        let still_dwelling = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x,
                y: sprite_y,
            },
            ..snapshot(16)
        });
        assert!(
            !still_dwelling.addressed,
            "addressed only once per dwell session"
        );
    }

    #[test]
    fn passing_cursor_does_not_address() {
        let mut engine = a_resting_sprite();
        let sprite_x = engine.position.x;
        let sprite_y = engine.position.y;

        let approach = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x - 50.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });
        assert!(!approach.addressed, "not addressed while approaching");

        let over = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x,
                y: sprite_y,
            },
            elapsed_ms: 16,
            ..snapshot(16)
        });
        assert!(
            !over.addressed,
            "not addressed: only on sprite for 16ms, below threshold"
        );

        let past = engine.tick(&WorldSnapshot {
            cursor: Point {
                x: sprite_x + 50.0,
                y: sprite_y,
            },
            ..snapshot(16)
        });
        assert!(!past.addressed, "not addressed after passing through");
    }

    /// `window_source::Rect` reaches the Engine without a field-by-field copy.
    #[test]
    fn window_source_rect_accepts_as_engine_rect() {
        use crate::window_source;

        fn takes_engine_rect(_rect: Rect) {}

        let source_rect = window_source::Rect {
            x: 100.0,
            y: 200.0,
            width: 300.0,
            height: 400.0,
        };

        takes_engine_rect(source_rect);

        let geometry = window_source::WorldGeometry {
            usable_frames: vec![source_rect],
            windows: vec![],
            dock: None,
        };

        let _snapshot = WorldSnapshot {
            displays: geometry.usable_frames, // No .map(rect) needed
            windows: vec![],
            cursor: Point { x: 0.0, y: 0.0 },
            elapsed_ms: 16,
            verbs: vec![],
            poke_settled: false,
            proposal: None,
            poll_generation: 0,
            composing: false,
            locomotion_frozen: false,
        };
    }

    /// A jump arrives through the existing landing. No second physics and no
    /// State of its own.
    #[test]
    fn a_jump_arcs_off_the_floor_and_lands_through_the_landing_path() {
        let mut engine = a_resting_sprite();
        let floor = engine.position.y;
        let launched_from = engine.position.x;

        let launch = engine.tick(&proposing("jump"));
        assert_eq!(launch.playing_primitive, Some(Primitive::Jump));

        // The tick after the proposal is the launch: a proposal is read after
        // the sprite has been moved. The launch sets a velocity and loses the
        // footing; the rise starts on the tick after that.
        let launched = engine.tick(&snapshot(100));
        assert_eq!(
            launched.state,
            State::Falling,
            "off the floor: {launched:?}"
        );
        assert!(launched.velocity.y < 0.0, "going up: {launched:?}");
        assert_eq!(
            launched.playing_primitive,
            Some(Primitive::Jump),
            "the Jump holds the screen for its turn, so its art can draw"
        );
        assert_eq!(launched.animation, "jump", "optional jump art, if drawn");

        let rising = engine.tick(&snapshot(100));
        assert!(rising.position.y < floor, "rising: {rising:?}");

        let mut peak = rising.position.y;
        let mut landed = None;
        for _ in 0..40 {
            let frame = engine.tick(&snapshot(100));
            peak = peak.min(frame.position.y);
            if frame.state == State::Grounded {
                landed = Some(frame);
                break;
            }
        }

        let landed = landed.expect("the arc comes down");
        assert!(
            floor - peak > 50.0,
            "clears more than 50 points: peak {peak}, floor {floor}"
        );
        assert_eq!(landed.position.y, floor, "back on the same floor");
        assert_ne!(landed.position.x, launched_from, "the flight travels in x");
        assert_eq!(
            landed.animation, "land",
            "arrives through the existing landing: {landed:?}"
        );
    }

    /// The Engine names the optional Animation and the renderer resolves it,
    /// as it already does for `climb` and `grab`.
    #[test]
    fn a_jump_asks_for_optional_jump_art() {
        assert_eq!(animation_of(Primitive::Jump), "jump");
    }

    /// The sprite already walks off a window edge, so it may jump off one too.
    #[test]
    fn a_perched_sprite_may_jump_off_its_edge() {
        let mut engine =
            Engine::new(Point { x: 100.0, y: 0.0 }).with_behaviors(declared_behaviors());
        settle(&mut engine, &perch(50.0, 400.0));
        assert_eq!(engine.state, State::Perched, "perched to begin with");

        engine.tick(&WorldSnapshot {
            proposal: proposing("jump").proposal,
            ..perch(50.0, 400.0)
        });
        let rising = engine.tick(&perch(50.0, 400.0));

        assert_eq!(rising.refused, None, "Perched permits a jump");
        assert_eq!(rising.state, State::Falling, "{rising:?}");
        assert!(
            rising.velocity.y < 0.0,
            "leaves the edge upward: {rising:?}"
        );
    }

    /// The Frame reports the refusal instead of dropping it.
    #[test]
    fn a_jump_is_refused_while_asleep_and_says_it_was() {
        let mut engine = a_resting_sprite();
        let asleep = engine.tick(&snapshot(SLEEP_AFTER_MS));
        assert_eq!(asleep.state, State::Asleep, "asleep to begin with");
        let resting_at = engine.position;

        let refused = engine.tick(&proposing("jump"));

        assert_eq!(
            refused.refused.as_deref(),
            Some("jump"),
            "the refusal names the Behavior: {refused:?}"
        );
        assert_eq!(refused.behavior, None, "nothing was played");
        assert_eq!(refused.playing_primitive, None, "nothing is on screen");
        assert_eq!(refused.state, State::Asleep, "still asleep");
        assert_eq!(refused.position, resting_at, "and has not moved");
    }

    fn monitor(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Two buddies on the primary, cursor on the second. Feet land on that
    /// floor, 64 points in from a side, spaced by their width and centred
    /// on the cursor. Left-to-right order follows where they stood.
    #[test]
    fn bring_landings_puts_every_sprite_on_the_cursor_display() {
        let primary = monitor(0.0, 0.0, 1920.0, 1080.0);
        let second = monitor(1920.0, 0.0, 1512.0, 982.0);
        let feet = [
            Point {
                x: 400.0,
                y: 1080.0,
            },
            Point {
                x: 100.0,
                y: 1080.0,
            },
        ];
        let cursor = Point {
            x: 2500.0,
            y: 400.0,
        };

        assert_eq!(
            bring_landings(
                &feet,
                &[128.0, 128.0],
                &[primary, second],
                &[primary, second],
                cursor
            ),
            Some(vec![
                Some(Point {
                    x: 2564.0,
                    y: 982.0
                }),
                Some(Point {
                    x: 2436.0,
                    y: 982.0
                }),
            ])
        );
    }

    /// The cursor is on the buddy already here, which is where a right-click
    /// lands. The one coming across stands beside them, not on top of them.
    #[test]
    fn bring_landings_does_not_drop_an_arrival_on_someone_already_there() {
        let primary = monitor(0.0, 0.0, 1920.0, 1080.0);
        let second = monitor(1920.0, 0.0, 1512.0, 982.0);
        let feet = [
            Point {
                x: 2500.0,
                y: 982.0,
            },
            Point {
                x: 100.0,
                y: 1080.0,
            },
        ];

        assert_eq!(
            bring_landings(
                &feet,
                &[128.0, 128.0],
                &[primary, second],
                &[primary, second],
                Point {
                    x: 2500.0,
                    y: 400.0
                },
            ),
            Some(vec![
                None,
                Some(Point {
                    x: 2628.0,
                    y: 982.0
                })
            ])
        );
    }

    /// Feet on the upper floor are the lower display's top edge. The art hangs
    /// above them, so a click on the lower display still has to bring them down.
    #[test]
    fn bring_landings_brings_a_sprite_down_off_the_upper_floor() {
        let upper = monitor(0.0, 0.0, 1920.0, 1080.0);
        let lower = monitor(0.0, 1080.0, 1920.0, 1080.0);

        assert_eq!(
            bring_landings(
                &[Point {
                    x: 400.0,
                    y: 1080.0
                }],
                &[128.0],
                &[upper, lower],
                &[upper, lower],
                Point {
                    x: 500.0,
                    y: 1500.0
                },
            ),
            Some(vec![Some(Point {
                x: 500.0,
                y: 2160.0
            })])
        );
    }

    /// Already standing there, including on the floor the display below
    /// would claim. Moving them would be a jump the click did not ask for.
    #[test]
    fn bring_landings_leaves_a_sprite_already_on_that_display() {
        let upper = monitor(0.0, 0.0, 1920.0, 1080.0);
        let lower = monitor(0.0, 1080.0, 1920.0, 1080.0);
        let on_the_floor = [Point {
            x: 400.0,
            y: 1080.0,
        }];
        let perched = [Point {
            x: 2500.0,
            y: 400.0,
        }];
        let second = monitor(1920.0, 0.0, 1512.0, 982.0);
        let cursor_on_second = Point {
            x: 2500.0,
            y: 400.0,
        };

        assert_eq!(
            bring_landings(
                &on_the_floor,
                &[128.0],
                &[upper, lower],
                &[upper, lower],
                Point { x: 500.0, y: 200.0 },
            ),
            Some(vec![None]),
            "the upper floor is still the upper display"
        );
        assert_eq!(
            bring_landings(
                &perched,
                &[128.0],
                &[upper, second],
                &[upper, second],
                cursor_on_second,
            ),
            Some(vec![None])
        );
    }

    /// The usable frame stops above the Dock. Feet go on that floor, not
    /// on the full frame's bottom edge behind it.
    #[test]
    fn bring_landings_uses_the_usable_floor() {
        let full = monitor(1920.0, 0.0, 1920.0, 1080.0);
        let usable = monitor(1920.0, 25.0, 1920.0, 980.0);
        let away = [Point { x: 100.0, y: 800.0 }];
        let home = monitor(0.0, 0.0, 1920.0, 1080.0);

        assert_eq!(
            bring_landings(
                &away,
                &[128.0],
                &[home, full],
                &[home, usable],
                Point {
                    x: 2500.0,
                    y: 100.0
                },
            ),
            Some(vec![Some(Point {
                x: 2500.0,
                y: 1005.0
            })])
        );
    }

    /// Nothing contains the cursor. The nearest display is the one it is
    /// brought to, and the feet stay inside that display.
    #[test]
    fn bring_landings_uses_the_nearest_display_when_the_cursor_is_in_a_gap() {
        let left = monitor(0.0, 0.0, 1000.0, 800.0);
        let right = monitor(1200.0, 0.0, 1000.0, 800.0);
        let on_the_right = [Point {
            x: 1500.0,
            y: 800.0,
        }];

        assert_eq!(
            bring_landings(
                &on_the_right,
                &[128.0],
                &[left, right],
                &[left, right],
                Point {
                    x: 1050.0,
                    y: 400.0
                },
            ),
            Some(vec![Some(Point { x: 936.0, y: 800.0 })])
        );
    }

    #[test]
    fn bring_landings_does_nothing_when_no_display_was_reported() {
        assert_eq!(
            bring_landings(
                &[Point { x: 10.0, y: 10.0 }],
                &[128.0],
                &[],
                &[],
                Point { x: 10.0, y: 10.0 },
            ),
            None
        );
    }

    /// Fullscreen on the second display. The sprite standing there goes to the
    /// middle of the free primary floor; the one already on the primary, and
    /// the one on a third display nobody took, stay where they are.
    #[test]
    fn bring_off_fullscreen_moves_only_the_sprites_on_a_fullscreen_display() {
        let primary = monitor(0.0, 0.0, 1920.0, 1080.0);
        let second = monitor(1920.0, 0.0, 1512.0, 982.0);
        let third = monitor(-1920.0, 0.0, 1920.0, 1080.0);
        let monitors = [primary, second, third];
        let feet = [
            Point {
                x: 2500.0,
                y: 982.0,
            },
            Point {
                x: 400.0,
                y: 1080.0,
            },
            Point {
                x: -1000.0,
                y: 1080.0,
            },
        ];

        assert_eq!(
            bring_off_fullscreen(
                &feet,
                &[128.0, 128.0, 128.0],
                &monitors,
                &monitors,
                &Desktop {
                    fullscreen: vec![false, true, false],
                },
            ),
            vec![
                Some(Point {
                    x: 960.0,
                    y: 1080.0
                }),
                None,
                None,
            ]
        );
    }

    /// Nobody stands on the fullscreen display, so nobody moves. This is what
    /// makes the shell's every-tick call settle after the first move.
    #[test]
    fn bring_off_fullscreen_leaves_a_sprite_already_on_a_free_display() {
        let primary = monitor(0.0, 0.0, 1920.0, 1080.0);
        let second = monitor(1920.0, 0.0, 1512.0, 982.0);

        assert_eq!(
            bring_off_fullscreen(
                &[Point {
                    x: 960.0,
                    y: 1080.0,
                }],
                &[128.0],
                &[primary, second],
                &[primary, second],
                &Desktop {
                    fullscreen: vec![false, true],
                },
            ),
            vec![None]
        );
    }

    /// Every display taken: there is nowhere to go, so nobody moves, and the
    /// Character fades instead.
    #[test]
    fn bring_off_fullscreen_moves_nobody_when_every_display_is_taken() {
        let primary = monitor(0.0, 0.0, 1920.0, 1080.0);
        let second = monitor(1920.0, 0.0, 1512.0, 982.0);

        assert_eq!(
            bring_off_fullscreen(
                &[Point {
                    x: 2500.0,
                    y: 982.0,
                }],
                &[128.0],
                &[primary, second],
                &[primary, second],
                &Desktop {
                    fullscreen: vec![true, true],
                },
            ),
            vec![None]
        );
    }

    /// A fall in progress is dropped. The next ticks stay on the point,
    /// including when a window on that display could be mistaken for a perch.
    #[test]
    fn standing_on_a_display_survives_the_next_tick() {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 });
        let there = Point {
            x: 2500.0,
            y: 800.0,
        };
        engine.stand_at(there);
        let world = WorldSnapshot {
            displays: vec![
                one_display(),
                Rect {
                    x: 2000.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 800.0,
                },
            ],
            windows: vec![window(
                1,
                Rect {
                    x: 2200.0,
                    y: 400.0,
                    width: 400.0,
                    height: 300.0,
                },
            )],
            elapsed_ms: 16,
            ..WorldSnapshot::default()
        };

        let frame = engine.tick(&world);
        assert_eq!(frame.position, there);
        assert_eq!(frame.state, State::Grounded);
        assert_eq!(frame.velocity, Point::default());
        let again = engine.tick(&world);
        assert_eq!(again.position, there, "the next tick does not hop");
    }
}
