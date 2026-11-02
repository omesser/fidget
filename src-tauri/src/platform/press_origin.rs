//! Where a button went down, so a later tick can hit-test that point.
//!
//! X11 stores this from the raw press. Windows has the same gap: the frame
//! loop reads the cursor when it wakes, and a drag in between has left the
//! sprite.

/// What a pointer message means for a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEdge {
    PrimaryDown,
    PrimaryUp,
    SecondaryDown,
    SecondaryUp,
    Other,
}

/// The cursor at the latest button-down. A move does not change it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PressRecord {
    at: Option<(i32, i32)>,
}

impl PressRecord {
    pub const fn new() -> Self {
        Self { at: None }
    }

    /// Remember `at` on a button-down. A release keeps that point and still
    /// wakes the frame loop. Motion does neither.
    pub fn observe(&mut self, edge: PointerEdge, at: (i32, i32)) -> bool {
        match edge {
            PointerEdge::PrimaryDown | PointerEdge::SecondaryDown => {
                self.at = Some(at);
                true
            }
            PointerEdge::PrimaryUp | PointerEdge::SecondaryUp => true,
            PointerEdge::Other => false,
        }
    }

    pub fn at(self) -> Option<(i32, i32)> {
        self.at
    }
}

/// `WM_LBUTTONDOWN` is `0x0201`, `WM_LBUTTONUP` `0x0202`, `WM_RBUTTONDOWN`
/// `0x0204`, `WM_RBUTTONUP` `0x0205`. Anything else, including motion, is
/// `Other`.
pub fn edge_of(message: u32) -> PointerEdge {
    match message {
        0x0201 => PointerEdge::PrimaryDown,
        0x0202 => PointerEdge::PrimaryUp,
        0x0204 => PointerEdge::SecondaryDown,
        0x0205 => PointerEdge::SecondaryUp,
        _ => PointerEdge::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_keeps_where_it_began_after_the_cursor_moves() {
        let mut record = PressRecord::new();
        assert!(
            record.observe(PointerEdge::PrimaryDown, (321, 675)),
            "a button-down wakes the loop"
        );
        assert!(
            !record.observe(PointerEdge::Other, (534, 675)),
            "motion is not a wake"
        );
        assert_eq!(record.at(), Some((321, 675)));
    }

    #[test]
    fn a_release_wakes_without_moving_the_press() {
        let mut record = PressRecord::new();
        record.observe(PointerEdge::PrimaryDown, (10, 20));
        assert!(record.observe(PointerEdge::PrimaryUp, (80, 90)));
        assert_eq!(record.at(), Some((10, 20)));
    }

    #[test]
    fn nothing_is_remembered_until_a_button_goes_down() {
        assert_eq!(PressRecord::new().at(), None);
    }

    #[test]
    fn a_later_press_replaces_where_the_last_one_began() {
        let mut record = PressRecord::new();
        record.observe(PointerEdge::SecondaryDown, (1, 2));
        record.observe(PointerEdge::PrimaryDown, (3, 4));
        assert_eq!(record.at(), Some((3, 4)));
    }

    #[test]
    fn the_button_messages_are_presses_and_motion_is_not() {
        assert_eq!(edge_of(0x0201), PointerEdge::PrimaryDown);
        assert_eq!(edge_of(0x0202), PointerEdge::PrimaryUp);
        assert_eq!(edge_of(0x0204), PointerEdge::SecondaryDown);
        assert_eq!(edge_of(0x0205), PointerEdge::SecondaryUp);
        assert_eq!(edge_of(0x0200), PointerEdge::Other, "WM_MOUSEMOVE");
    }
}
