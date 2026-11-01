//! The State machine, in one place: the only writer of State in the Engine.
//! Verbs move it before physics and Contacts after, and `tick` stores nothing
//! but what these two functions return, so no transition lives anywhere else.

use super::{State, Surface, Verb};

/// What the sprite's body met while `Engine::integrate` moved it.
#[derive(Clone, Copy)]
pub enum Contact {
    /// Nothing underfoot: still in the air, or the footing is gone.
    Airborne,
    /// Came down on a surface at the end of a fall.
    Landed(Surface),
    /// Put on top of a window that had come to contain it. Not a landing:
    /// the sprite never left the ground it thought it had.
    Lifted(Surface),
    /// Still standing exactly where it stood.
    Standing,
    /// Ran into something the sprite cannot pass sideways: a screen edge or the
    /// side of the Dock. Contains side (-1.0 left, 1.0 right); the sprite's x
    /// is the contact x.
    Wall { side: f64 },
    /// Met the usable top while rising or climbing.
    Ceiling,
}

/// The hand picked the sprite up or let it go. A continued Grab is not an
/// edge: the verb repeats every held tick, and a slow release emits no verb,
/// so the cue cannot key on one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    PickedUp,
    LetGo,
}

/// State after the verbs, whether they woke the sprite, and the [`Edge`] when
/// the hand picked it up or let go. Reported here so a caller does not
/// re-derive them. Possession wakes it: the keys move a sprite on its feet.
pub fn on_verbs(state: State, verbs: &[Verb], possessed: bool) -> (State, bool, Option<Edge>) {
    let woke = state == State::Asleep && (!verbs.is_empty() || possessed);

    if verbs.contains(&Verb::Grab) {
        let edge = match state {
            State::Dragged => None,
            _ => Some(Edge::PickedUp),
        };
        return (State::Dragged, woke, edge);
    }
    let (state, edge) = match state {
        // Let go. Thrown or simply dropped, what follows is a fall.
        State::Dragged => (State::Falling, Some(Edge::LetGo)),
        // Woken. Whether it is still standing on anything is settled by
        // falling, the same as any other loss of footing.
        _ if woke => (State::Falling, None),
        other => (other, None),
    };
    (state, woke, edge)
}

/// Where this tick's physics leaves the sprite. `rested` is whether it
/// has been untouched and still for long enough to nod off.
pub fn on_contact(state: State, contact: Option<Contact>, rested: bool) -> State {
    match contact {
        // Physics had nothing to report: a held sprite, or a climb still
        // under way.
        None => state,
        Some(Contact::Wall { .. }) => State::Climbing,
        Some(Contact::Airborne | Contact::Ceiling) => State::Falling,
        Some(Contact::Landed(surface) | Contact::Lifted(surface)) => match surface {
            Surface::Floor => State::Grounded,
            Surface::Perch => State::Perched,
        },
        Some(Contact::Standing) if rested => State::Asleep,
        Some(Contact::Standing) => state,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Point, State, Verb};
    use super::on_verbs;

    #[test]
    fn a_grab_is_picked_up_once_and_a_repeat_is_not_an_edge() {
        let (state, woke, edge) = on_verbs(State::Grounded, &[Verb::Grab], false);
        assert_eq!(
            (state, woke, edge),
            (State::Dragged, false, Some(super::Edge::PickedUp))
        );

        let (state, woke, edge) = on_verbs(State::Dragged, &[Verb::Grab], false);
        assert_eq!((state, woke, edge), (State::Dragged, false, None));

        let (state, woke, edge) = on_verbs(State::Asleep, &[Verb::Grab], false);
        assert_eq!(
            (state, woke, edge),
            (State::Dragged, true, Some(super::Edge::PickedUp))
        );
    }

    #[test]
    fn letting_go_without_a_verb_is_the_edge_and_a_throw_is_the_same_edge() {
        let (state, woke, edge) = on_verbs(State::Dragged, &[], false);
        assert_eq!(
            (state, woke, edge),
            (State::Falling, false, Some(super::Edge::LetGo))
        );

        let thrown = Verb::Throw {
            velocity: Point {
                x: 100.0,
                y: -200.0,
            },
        };
        let (state, woke, edge) = on_verbs(State::Dragged, &[thrown], false);
        assert_eq!(
            (state, woke, edge),
            (State::Falling, false, Some(super::Edge::LetGo))
        );
    }

    #[test]
    fn a_verb_that_is_not_a_grab_has_no_dragged_edge() {
        let (state, woke, edge) = on_verbs(State::Grounded, &[Verb::Poke], false);
        assert_eq!((state, woke, edge), (State::Grounded, false, None));

        let (state, woke, edge) = on_verbs(State::Asleep, &[], true);
        assert_eq!((state, woke, edge), (State::Falling, true, None));
    }
}
