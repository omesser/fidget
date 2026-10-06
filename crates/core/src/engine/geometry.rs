//! Spatial queries over a `WorldSnapshot`.

use super::{Point, Rect, Window, WorldSnapshot, EDGE_CLEARANCE};
use crate::window_source::DOCK_PERCH_ID;

/// A surface the sprite can come to rest on.
pub(super) struct Support {
    pub(super) y: f64,
    pub(super) surface: Surface,
}

/// What a Support is made of. The fact geometry reports — what standing on
/// one turns the sprite into is `transition`'s to say, not geometry's.
#[derive(Clone, Copy)]
pub(super) enum Surface {
    /// The bottom of a display.
    Floor,
    /// A window's top edge.
    Perch,
}

/// The first surface at or below `position`: the nearest window top edge, or
/// the floor when no window is in the way. Sides and bottoms are not surfaces,
/// so the sprite rises through a window from underneath instead of trapping.
pub(super) fn support_below(
    position: Point,
    snapshot: &WorldSnapshot,
    clearance: f64,
) -> Option<Support> {
    let floor = floor_under(position.x, snapshot).map(|y| Support {
        y,
        surface: Surface::Floor,
    });

    snapshot
        .windows
        .iter()
        .enumerate()
        .filter(|(index, window)| {
            window.rect.y >= position.y && is_perch(*index, position.x, snapshot, clearance)
        })
        .map(|(_, window)| Support {
            y: window.rect.y,
            surface: Surface::Perch,
        })
        .chain(floor)
        .min_by(|a, b| a.y.total_cmp(&b.y))
}

/// Whether the top edge of the window at `index` is somewhere the sprite can
/// stand at `x`: not covered at this x by a window in front (which is why
/// `snapshot.windows` is ordered), over a display, and clear of the usable top.
///
/// Only narrows the candidates; nearest support still wins over frontmost.
/// Taking the first match in z-order would drop the sprite through an edge it
/// can plainly see whenever the window in front sits above it and is no support.
pub(super) fn is_perch(index: usize, x: f64, snapshot: &WorldSnapshot, clearance: f64) -> bool {
    let window = &snapshot.windows[index].rect;
    window.spans_x(x)
        && on_a_display(Point { x, y: window.y }, snapshot, clearance)
        && !snapshot.windows[..index].iter().any(|front| {
            front.rect.spans_x(x) && window.y >= front.rect.y && window.y <= front.rect.bottom()
        })
}

/// Whether the feet can be put down at `position`: some display covers it,
/// with the room the art needs above. Split out of `is_perch` because a ride
/// between polls has no window sample yet must not place the sprite out there.
pub(super) fn on_a_display(position: Point, snapshot: &WorldSnapshot, clearance: f64) -> bool {
    displays_spanning(position.x, snapshot).any(|display| {
        position.y >= display.y + room_above(clearance, display) && position.y <= display.bottom()
    })
}

/// What a resting sprite is standing on: its Perch, unless a window in front
/// has come to contain it, in which case that window's top edge is the surface
/// instead. `swallowing` is the caller's judgement; see `Engine::swallowed_by`.
pub(super) fn footing(
    position: Point,
    snapshot: &WorldSnapshot,
    clearance: f64,
    swallowing: impl Fn(&Window) -> bool,
) -> Option<Support> {
    // The floor is exempt: it is under every window and the sprite is drawn in
    // front of them all, so ground in front of a window is not the trapped-inside
    // case (DESIGN.md decision 7). Anything behind the Dock contains the floor.
    if floor_under(position.x, snapshot) == Some(position.y) {
        return support_below(position, snapshot, clearance);
    }

    // The Perch it is on: a window whose top edge is exactly its own height.
    // Without one it stands on nothing and falls; a Perch moving out from under
    // it leaves it in the air, not on the first window that happens to contain it.
    let perch = snapshot
        .windows
        .iter()
        .position(|window| window.rect.spans_x(position.x) && window.rect.y == position.y)?;

    // Only a window in front of the Perch can swallow the sprite: what is behind
    // the Perch is behind the sprite too. And an edge you cannot see is gone, so
    // a hidden Perch falls rather than floating in mid-air after alt-tab.
    let held = (is_perch(perch, position.x, snapshot, clearance)
        && !snapshot.windows[..perch].iter().any(&swallowing))
    .then_some(Support {
        y: position.y,
        surface: Surface::Perch,
    });

    snapshot.windows[..perch]
        .iter()
        .enumerate()
        // Swallowed only onto a top edge that is somewhere to stand: lifting
        // the sprite onto an edge that is hidden or off-screen strands it in
        // the place this is meant to get it out of.
        .filter(|(index, window)| {
            swallowing(window) && is_perch(*index, position.x, snapshot, clearance)
        })
        .map(|(_, window)| Support {
            y: window.rect.y,
            surface: Surface::Perch,
        })
        .chain(held)
        .chain(support_below(position, snapshot, clearance))
        .min_by(|a, b| a.y.total_cmp(&b.y))
}

/// The window whose top edge the sprite is standing on, if any.
pub(super) fn perch_at(position: Point, windows: &[Window]) -> Option<Window> {
    windows
        .iter()
        .copied()
        .find(|window| window.rect.spans_x(position.x) && window.rect.y == position.y)
}

/// Whether the sprite is inside `window` rather than on top of it. A top edge
/// is a Perch to stand on, so only what is strictly below it is inside.
pub(super) fn swallows(window: &Rect, position: Point) -> bool {
    window.spans_x(position.x) && window.y < position.y && position.y < window.bottom()
}

/// The bottom of the display the sprite is over, or nothing when it is over no
/// display at all — a sprite outside every display has nothing to land on.
fn floor_under(x: f64, snapshot: &WorldSnapshot) -> Option<f64> {
    displays_spanning(x, snapshot)
        .map(Rect::bottom)
        .max_by(f64::total_cmp)
}

/// The highest the feet may go: the usable top plus the room the art needs
/// above them. A climb lets go here; a Throw bumps it.
pub(super) fn ceiling_over(x: f64, snapshot: &WorldSnapshot, clearance: f64) -> Option<f64> {
    displays_spanning(x, snapshot)
        .map(|display| display.y + room_above(clearance, display))
        .min_by(f64::total_cmp)
}

/// The clearance actually applied on one display: no display gives up more
/// than half its height to the ceiling, or art taller than the screen would
/// leave nowhere to stand.
fn room_above(clearance: f64, display: &Rect) -> f64 {
    clearance.min(display.height / 2.0)
}

fn displays_spanning<'a>(
    x: f64,
    snapshot: &'a WorldSnapshot,
) -> impl Iterator<Item = &'a Rect> + 'a {
    snapshot
        .displays
        .iter()
        .filter(move |display| display.spans_x(x))
}

/// The screen edge the sprite has just arrived at while moving into it, or the
/// nearest one when it is over no display at all: out there it has no floor and
/// would fall for ever, so grabbing an edge is how it gets back over a display.
pub(super) fn wall_reached(x: f64, velocity_x: f64, snapshot: &WorldSnapshot) -> Option<f64> {
    if displays_spanning(x, snapshot).next().is_none() {
        return nearest_edge(x, snapshot);
    }

    // Still over a display, so the only wall left is the outermost edge, and
    // only while the sprite is moving into it. Every inner edge is somewhere to
    // cross rather than something to catch.
    let left = snapshot
        .displays
        .iter()
        .map(|display| display.x)
        .min_by(f64::total_cmp)?;
    let right = snapshot
        .displays
        .iter()
        .map(|display| display.x + display.width)
        .max_by(f64::total_cmp)?;

    if velocity_x > 0.0 && x >= right {
        Some(right)
    } else if velocity_x < 0.0 && x <= left {
        Some(left)
    } else {
        None
    }
}

/// The Dock, when the snapshot carries one: the Perch wearing the reserved id.
pub(super) fn dock_in(snapshot: &WorldSnapshot) -> Option<Rect> {
    snapshot
        .windows
        .iter()
        .find(|window| window.id == DOCK_PERCH_ID)
        .map(|window| window.rect)
}

/// Where to stand when the Dock is in the way: clear of its nearer side, on the
/// floor or in the air. The Dock is the one thing drawn in front of the sprite,
/// so under it the sprite can be neither seen nor grabbed; its side is a wall.
pub(super) fn dock_side_reached(
    position: Point,
    velocity_x: f64,
    snapshot: &WorldSnapshot,
) -> Option<f64> {
    let dock = dock_in(snapshot)?;
    // `EDGE_CLEARANCE` out from the Dock's edge, the same half-sprite
    // `at_horizontal_edge` keeps on screen, so a sprite put on the line stands
    // beside the Dock and stays put rather than being set back a step.
    let (left, right) = (
        dock.x - EDGE_CLEARANCE,
        dock.x + dock.width + EDGE_CLEARANCE,
    );
    if position.y <= dock.y || position.x <= left || position.x >= right {
        return None;
    }

    // Behind the Dock's own display, not merely its x-range, or a display
    // stacked below would keep every sprite on its floor behind the Dock. The
    // Dock's center picks the display; its bottom, not the Dock's, is the floor.
    let center = (dock.x + dock.width / 2.0, dock.y + dock.height / 2.0);
    snapshot.displays.iter().find(|display| {
        display.spans_x(center.0)
            && center.1 >= display.y
            && center.1 <= display.bottom()
            && display.spans_x(position.x)
            && position.y <= display.bottom()
    })?;

    // Nearer side, not the side it came from: the Dock can unhide around a
    // resting sprite, and a dropped sprite arrives from above.
    let nearer_left = position.x - left <= right - position.x;

    // Strictly inside, so a sprite level with the Dock's own edge counts as
    // out in the margin and may leave. It is the position stepping off the top
    // lands on, and treating it as behind would keep the trap shut.
    let behind = position.x > dock.x && position.x < dock.x + dock.width;
    let into_dock = if nearer_left {
        velocity_x > 0.0
    } else {
        velocity_x < 0.0
    };
    // In the margin the side is a wall only for a sprite moving into the Dock,
    // as `wall_reached` treats a display edge. Strictly behind there is no such
    // reprieve either way: the way out from under a real Dock is long and unseen.
    if !behind && !into_dock {
        return None;
    }

    Some(if nearer_left { left } else { right })
}

/// Where a climb beside the Dock steps onto its top, once the feet reach it:
/// `EDGE_CLEARANCE` in from the side climbed, so the whole sprite stands where
/// `perch_at` agrees is the Dock. `None` for a climb nowhere near the Dock.
pub(super) fn dock_top_at(x: f64, dock: Rect) -> Option<f64> {
    let (left, right) = (dock.x, dock.x + dock.width);
    if (left - EDGE_CLEARANCE..=left + EDGE_CLEARANCE).contains(&x) {
        Some((left + EDGE_CLEARANCE).min(right))
    } else if (right - EDGE_CLEARANCE..=right + EDGE_CLEARANCE).contains(&x) {
        Some((right - EDGE_CLEARANCE).max(left))
    } else if dock.spans_x(x) {
        Some(x)
    } else {
        None
    }
}

/// Adjusted position and facing when at a horizontal display edge, so the full
/// sprite stays on-screen and faces away from the wall.
pub(super) fn at_horizontal_edge(x: f64, snapshot: &WorldSnapshot) -> Option<(f64, f64)> {
    let left = snapshot
        .displays
        .iter()
        .map(|display| display.x)
        .min_by(f64::total_cmp)?;
    let right = snapshot
        .displays
        .iter()
        .map(|display| display.x + display.width)
        .max_by(f64::total_cmp)?;

    // Only correct when very close to the boundary. Sprites that settled
    // naturally within EDGE_CLEARANCE but not AT the edge should stay put.
    const SNAP_THRESHOLD: f64 = EDGE_CLEARANCE / 2.0;

    if x <= left + SNAP_THRESHOLD {
        Some((left + EDGE_CLEARANCE, 1.0))
    } else if x >= right - SNAP_THRESHOLD {
        Some((right - EDGE_CLEARANCE, -1.0))
    } else {
        None
    }
}

/// The display edge nearest `x`, for a sprite that is over none of them.
/// Nearest rather than the one it came from: the sprite has no memory of that,
/// and the shortest way back to somewhere it can stand is the least surprising.
fn nearest_edge(x: f64, snapshot: &WorldSnapshot) -> Option<f64> {
    snapshot
        .displays
        .iter()
        .flat_map(|display| [display.x, display.x + display.width])
        .min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs()))
}
