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
