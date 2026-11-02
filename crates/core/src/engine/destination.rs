//! Walking to an x. Chase walks to the cursor's, read again every tick.

use super::{Direction, Engine, ARRIVAL_THRESHOLD, WALK_SPEED};

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
    /// Face `toward` and walk at `WALK_SPEED`.
    pub(super) fn stride(&mut self, toward: Direction) {
        self.facing = toward.facing();
        self.velocity.x = self.facing * WALK_SPEED;
    }
}
