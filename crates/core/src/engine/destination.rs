//! Walking to an x. Chase walks to the cursor's, read again every tick. A move
//! walks to one fixed when it starts, kept in `Engine::destination` beside
//! `playing` and not in it, so no Primitive's turn ends it.

use super::geometry::standing_reach;
use super::{Direction, Engine, State, WorldSnapshot, ARRIVAL_THRESHOLD, WALK_SPEED};
use std::fmt;
use std::num::NonZeroU32;

/// An x in the Engine's coordinate space, global points across every display.
/// `Placement::engine_x` turns a display's own x into one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalX(f64);

impl GlobalX {
    /// `None` unless `x` is finite.
    pub(crate) fn new(x: f64) -> Option<Self> {
        x.is_finite().then_some(Self(x))
    }
}

/// How far to walk, in points. Finite and more than zero, so a negative
/// distance cannot turn a heading round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance(f64);

impl Distance {
    /// `None` unless `points` is finite and more than zero.
    pub(crate) fn new(points: f64) -> Option<Self> {
        (points.is_finite() && points > 0.0).then_some(Self(points))
    }
}

/// How much walking a heading asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extent {
    Distance(Distance),
    /// This long at `WALK_SPEED`. The Engine owns the speed, so no caller converts.
    Time {
        ms: NonZeroU32,
    },
}

/// A move, parsed. A heading always carries exactly one extent, and a spot
/// carries no `y`, because a walk never changes height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MoveRequest {
    /// Walk `heading` for `extent`, counted from the feet when the Engine takes it.
    Along { heading: Direction, extent: Extent },
    /// Walk to `x`.
    To(GlobalX),
}

impl MoveRequest {
    /// The one x every request becomes.
    fn target(self, feet_x: f64) -> GlobalX {
        match self {
            Self::To(x) => x,
            Self::Along { heading, extent } => {
                let points = match extent {
                    Extent::Distance(Distance(points)) => points,
                    Extent::Time { ms } => WALK_SPEED * f64::from(ms.get()) / 1000.0,
                };
                GlobalX(feet_x + heading.facing() * points)
            }
        }
    }
}

/// Why the Engine would not take a move. `Display` is the sentence the caller reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveRefusal {
    Falling,
    /// `State::Dragged`, in the user's hand.
    Held,
    Climbing,
    Asleep,
    /// A Poke or a Summon holds the sprite still for `POKE_COOLDOWN_MS`.
    JustPoked,
    Possessed,
    DoNotDisturb,
    /// The target is inside `ARRIVAL_THRESHOLD`, so the walk would end before its first stride.
    AlreadyThere,
}

impl fmt::Display for MoveRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Falling => {
                f.write_str("The Character is in the air, so it cannot walk until it lands.")
            }
            Self::Held => f.write_str("The user is holding the Character, so it cannot walk."),
            Self::Climbing => f.write_str("The Character is climbing a wall, so it cannot walk."),
            Self::Asleep => f.write_str("The Character is asleep, so it cannot walk."),
            Self::JustPoked => f.write_str(
                "The Character was just poked and stands still for a moment. Try again in a few seconds.",
            ),
            Self::Possessed => f.write_str(
                "The user is driving the Character from the keyboard, so it cannot walk on its own.",
            ),
            Self::DoNotDisturb => {
                f.write_str("Do Not Disturb is on, so the Character starts nothing.")
            }
            Self::AlreadyThere => write!(
                f,
                "The Character already stands within {ARRIVAL_THRESHOLD} points of where it was sent, so it did not walk."
            ),
        }
    }
}

/// The State and cooldown facts that stop any walk, a Behavior's or a move's.
pub(super) fn footing_refusal(state: State, poke_cooldown_ms: u32) -> Option<MoveRefusal> {
    match state {
        State::Falling => Some(MoveRefusal::Falling),
        State::Dragged => Some(MoveRefusal::Held),
        State::Climbing => Some(MoveRefusal::Climbing),
        State::Asleep => Some(MoveRefusal::Asleep),
        State::Grounded | State::Perched if poke_cooldown_ms > 0 => Some(MoveRefusal::JustPoked),
        State::Grounded | State::Perched => None,
    }
}

/// What a walker at some x does about a target x this tick.
pub(super) enum Heading {
    Arrived,
    Toward(Direction),
}

/// The steering rule toward a target x, asked again every tick.
pub(super) fn heading_to(from_x: f64, target_x: f64) -> Heading {
    if (target_x - from_x).abs() < ARRIVAL_THRESHOLD {
        Heading::Arrived
    } else if target_x > from_x {
        Heading::Toward(Direction::Right)
    } else {
        Heading::Toward(Direction::Left)
    }
}

impl Engine {
    /// Take a move, or say why not. A move taken replaces the one under way.
    /// The answer is the Engine's own, given on the state the next tick
    /// starts from.
    pub fn start_move(&mut self, request: MoveRequest) -> Result<(), MoveRefusal> {
        let target = request.target(self.position.x);
        if let Some(refusal) = self.stride_blocked(self.state) {
            return Err(refusal);
        }
        if let Heading::Arrived = heading_to(self.position.x, target.0) {
            return Err(MoveRefusal::AlreadyThere);
        }
        self.destination = Some(target);
        // A move is activity, as a proposal is, or a drowsy sprite nods off on arrival.
        self.idle_ms = 0;
        Ok(())
    }

    /// Stride toward the destination, or end the move on arrival, at the end
    /// of the Surface underfoot, or once `state` stops it.
    pub(super) fn apply_destination(&mut self, state: State, dt: f64, snapshot: &WorldSnapshot) {
        let Some(GlobalX(target_x)) = self.destination else {
            return;
        };
        let stop_x =
            standing_reach(self.position, state, snapshot).map(|reach| reach.clamp(target_x));
        match (self.stride_blocked(state), stop_x) {
            (None, Some(stop_x)) => match heading_to(self.position.x, stop_x) {
                Heading::Toward(toward) => self.stride(toward, stop_x, dt),
                Heading::Arrived => self.end_move(state),
            },
            _ => self.end_move(state),
        }
    }

    /// Whether a move holds the feet. Asked instead of the field, so a move
    /// the gate has closed on holds nothing in the tick before it ends.
    pub(super) fn feet_owned(&self) -> bool {
        self.destination.is_some() && self.stride_blocked(self.state).is_none()
    }

    /// Face `toward` and walk at `WALK_SPEED`. The step that reaches `target_x`
    /// stops on it, or one long tick could carry the walk past the arrival band
    /// and turn it round.
    pub(super) fn stride(&mut self, toward: Direction, target_x: f64, dt: f64) {
        self.facing = toward.facing();
        let speed = WALK_SPEED.min((target_x - self.position.x).abs() / dt);
        self.velocity.x = self.facing * speed;
    }

    /// Why a move cannot stride in `state`, if it cannot. A speech bubble or a
    /// quick message holds a Behavior's walk and not a move's, so `feet_held`
    /// is not read here.
    fn stride_blocked(&self, state: State) -> Option<MoveRefusal> {
        footing_refusal(state, self.poke_cooldown_ms).or(if self.possessed {
            Some(MoveRefusal::Possessed)
        } else if self.do_not_disturb {
            Some(MoveRefusal::DoNotDisturb)
        } else {
            None
        })
    }

    /// The only way out of a move. Nothing keeps the x, so nothing resumes it.
    /// On its feet the sprite stops too. In the air its stride carries into
    /// the fall, as any walk's does.
    pub(super) fn end_move(&mut self, state: State) {
        if matches!(state, State::Grounded | State::Perched) {
            self.velocity.x = 0.0;
        }
        self.destination = None;
    }
}
