//! Walking to an x. Chase walks to the cursor's, read again every tick. A move
//! walks to one fixed when it starts, kept in `Engine::destination` beside
//! `playing` and not in it, so no Primitive's turn ends it.

use super::{Direction, Engine, ARRIVAL_THRESHOLD, WALK_SPEED};
use std::fmt;
use std::num::NonZeroU32;

/// An x in the Engine's coordinate space, global points across every display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalX(f64);

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

/// A move, parsed. A heading always carries exactly one extent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MoveRequest {
    /// Walk `heading` for `extent`, counted from the feet when the Engine takes it.
    Along { heading: Direction, extent: Extent },
}

impl MoveRequest {
    /// The one x every request becomes.
    fn target(self, feet_x: f64) -> GlobalX {
        match self {
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
    /// The target is inside `ARRIVAL_THRESHOLD`, so the walk would end before its first stride.
    AlreadyThere,
}

impl fmt::Display for MoveRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyThere => write!(
                f,
                "The Character already stands within {ARRIVAL_THRESHOLD} points of where it was sent, so it did not walk."
            ),
        }
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
    /// Take a move, or say why not. The answer is the Engine's own, given on
    /// the state the next tick starts from.
    pub fn start_move(&mut self, request: MoveRequest) -> Result<(), MoveRefusal> {
        let target = request.target(self.position.x);
        if let Heading::Arrived = heading_to(self.position.x, target.0) {
            return Err(MoveRefusal::AlreadyThere);
        }
        self.destination = Some(target);
        // An order is activity, as a proposal is, or a drowsy sprite nods off on arrival.
        self.idle_ms = 0;
        Ok(())
    }

    /// Stride toward the destination, or end the move on arrival.
    pub(super) fn apply_destination(&mut self) {
        let Some(GlobalX(target_x)) = self.destination else {
            return;
        };
        match heading_to(self.position.x, target_x) {
            Heading::Arrived => self.end_move(),
            Heading::Toward(toward) => self.stride(toward),
        }
    }

    /// Face `toward` and walk at `WALK_SPEED`.
    pub(super) fn stride(&mut self, toward: Direction) {
        self.facing = toward.facing();
        self.velocity.x = self.facing * WALK_SPEED;
    }

    /// The only way out of a move. Nothing keeps the x, so nothing resumes it.
    fn end_move(&mut self) {
        self.velocity.x = 0.0;
        self.destination = None;
    }
}
