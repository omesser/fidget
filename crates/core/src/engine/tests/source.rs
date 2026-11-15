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
