//! Who owns the Animation on screen, read off real Frames.
use super::*;

#[test]
fn a_sprite_resting_on_the_floor_is_its_idle_loop() {
    let mut engine = a_resting_sprite();
    let frame = engine.tick(&snapshot(100));
    assert_eq!(frame.animation, "idle");
    assert_eq!(frame.animation_source, AnimationSource::IdleLoop);
}

#[test]
fn a_poke_is_user_invoked_and_its_react_hands_back_to_the_idle_loop() {
    let mut engine = a_resting_sprite();
    let poked = engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Poke],
        ..snapshot(100)
    });
    assert_eq!(
        (poked.animation, poked.animation_source),
        ("react", AnimationSource::UserInvoked)
    );
    let after = settle(&mut engine, &snapshot(100));
    assert_eq!(
        (after.animation, after.animation_source),
        ("idle", AnimationSource::IdleLoop)
    );
}

fn proposed_by(behavior: &str, source: AnimationSource) -> WorldSnapshot {
    WorldSnapshot {
        proposal: Some(BehaviorProposal {
            behavior: behavior.to_string(),
            dialogue: None,
            direction: None,
            source,
        }),
        ..snapshot(100)
    }
}

/// The same Behavior from two producers reads as two causes, through the
/// whole chain and its continuation.
#[test]
fn the_same_behavior_reports_whoever_proposed_it_through_its_continuation() {
    for source in [AnimationSource::LocalDirector, AnimationSource::AiRequested] {
        let mut engine = a_resting_sprite();
        let started = engine.tick(&proposed_by("greet", source));
        assert_eq!(
            (started.animation, started.animation_source),
            ("react", source)
        );
        let chain: Vec<_> = (0..24)
            .map(|_| engine.tick(&snapshot(100)))
            .map(|frame| (frame.animation, frame.animation_source))
            .collect();
        assert!(
            chain
                .iter()
                .filter(|(animation, _)| *animation != "idle")
                .all(|(_, owner)| *owner == source),
            "greet then settle is one decision: {chain:?}"
        );
        assert_eq!(chain.last(), Some(&("idle", AnimationSource::IdleLoop)));
    }
}

#[test]
fn an_accepted_move_walks_as_whoever_asked_for_it() {
    let mut engine = a_resting_sprite();
    engine
        .start_move(
            MoveRequest::To(GlobalX::new(500.0).unwrap()),
            AnimationSource::AiMcp,
        )
        .expect("a grounded sprite takes a move");
    let walking = engine.tick(&snapshot(100));
    assert_eq!(
        (walking.animation, walking.animation_source),
        ("walk", AnimationSource::AiMcp)
    );
    let arrived = settle(&mut engine, &snapshot(100));
    assert_eq!(
        (arrived.animation, arrived.animation_source),
        ("idle", AnimationSource::IdleLoop)
    );
}

/// The hand is the user's. Once it lets go, physics owns the fall and the
/// landing, though the user caused them.
#[test]
fn a_grab_is_the_users_and_the_fall_after_it_is_the_engines() {
    let mut engine = a_resting_sprite();
    let held = carry(&mut engine, Point { x: 300.0, y: 300.0 }, &snapshot(100));
    assert_eq!(
        (held.state, held.animation_source),
        (State::Dragged, AnimationSource::UserInvoked)
    );
    let falling = engine.tick(&snapshot(16));
    assert_eq!(
        (falling.state, falling.animation_source),
        (State::Falling, AnimationSource::Engine)
    );
    let landed = (0..80)
        .map(|_| engine.tick(&snapshot(16)))
        .find(|frame| frame.animation == "land")
        .expect("the drop lands");
    assert_eq!(landed.animation_source, AnimationSource::Engine);
}

#[test]
fn a_possessed_stride_is_the_users() {
    let mut engine = a_resting_sprite();
    engine.set_possessed(true);
    let walking = engine.tick(&WorldSnapshot {
        possession: Intent {
            walk: Some(Walk::Right),
            ..Intent::default()
        },
        ..snapshot(100)
    });
    assert_eq!(
        (walking.animation, walking.animation_source),
        ("walk", AnimationSource::UserInvoked)
    );
}

/// The walk outlives its Primitive under its own velocity, and keeps the
/// owner that set it going rather than reading as idle life.
#[test]
fn a_walk_that_outlives_its_primitive_keeps_its_owner() {
    let mut engine = a_resting_sprite();
    engine.tick(&proposed_by("walk", AnimationSource::LocalDirector));
    let residual = (0..10)
        .map(|_| engine.tick(&snapshot(100)))
        .find(|frame| frame.playing_primitive.is_none() && frame.animation == "walk")
        .expect("the stride carries on past the Primitive");
    assert_eq!(residual.animation_source, AnimationSource::LocalDirector);
}
