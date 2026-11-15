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

fn spoken_for(source: AnimationSource) -> WorldSnapshot {
    WorldSnapshot {
        speaking_for: Some(source),
        ..snapshot(100)
    }
}

/// Fallback Talk is whoever the sprite speaks for, on the floor and on a
/// Perch, and keeps one clock across a change of speaker.
#[test]
fn fallback_talk_is_the_speakers_and_its_clock_runs_across_a_change_of_speaker() {
    let mut engine = a_resting_sprite();
    let replying = engine.tick(&spoken_for(AnimationSource::AiRequested));
    assert_eq!(
        (replying.animation, replying.animation_source),
        ("talk", AnimationSource::AiRequested)
    );
    let pill = engine.tick(&spoken_for(AnimationSource::UserInvoked));
    assert_eq!(
        (pill.animation, pill.animation_source),
        ("talk", AnimationSource::UserInvoked)
    );
    assert_eq!(
        pill.animation_ms,
        replying.animation_ms + 100,
        "a new speaker is not a new Talk"
    );
}

#[test]
fn speaking_leaves_active_playback_and_a_fall_alone() {
    let mut engine = a_resting_sprite();
    let poked = engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Poke],
        ..spoken_for(AnimationSource::AiRequested)
    });
    assert_eq!(
        (poked.animation, poked.animation_source),
        ("react", AnimationSource::UserInvoked)
    );

    let mut falling = Engine::new(Point { x: 100.0, y: 0.0 });
    let frame = falling.tick(&spoken_for(AnimationSource::AiRequested));
    assert_eq!(
        (frame.animation, frame.animation_source),
        ("fall", AnimationSource::Engine)
    );
}

#[test]
fn a_turn_beat_is_ai_feedback_with_no_behavior_requested() {
    let mut engine = a_resting_sprite();
    engine.cue_moment(Moment::Working);
    let beat = engine.tick(&snapshot(100));
    assert_eq!(beat.playing_behavior, None);
    assert_eq!(beat.animation_source, AnimationSource::AiFeedback);
}

#[test]
fn a_cursor_reaction_is_user_invoked() {
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_cursor_reactions(CursorReaction::React, CursorReaction::Indifferent);
    let far = WorldSnapshot {
        cursor: Point { x: 900.0, y: 0.0 },
        ..snapshot(100)
    };
    let feet = settle(&mut engine, &far).position;
    let near = engine.tick(&WorldSnapshot {
        cursor: Point {
            x: feet.x + 100.0,
            y: feet.y,
        },
        ..snapshot(100)
    });
    assert_eq!(
        (near.animation, near.animation_source),
        ("react", AnimationSource::UserInvoked)
    );
}

/// Sitting down on a Perch is the Engine settling the sprite; only once the
/// quiet stretch gives way to the unrequested idle loop is it idle life.
#[test]
fn a_quiet_perch_is_the_engine_until_the_idle_loop_takes_over() {
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 }).with_sleep_after_s((60, 60));
    let sitting = settle(&mut engine, &perch(50.0, 400.0));
    assert_eq!(
        (sitting.state, sitting.animation, sitting.animation_source),
        (State::Perched, "sit", AnimationSource::Engine)
    );
    let idling = engine.tick(&WorldSnapshot {
        elapsed_ms: PERCHED_IDLE_AFTER_MS,
        ..perch(50.0, 400.0)
    });
    assert_eq!(
        (idling.animation, idling.animation_source),
        ("idle", AnimationSource::IdleLoop)
    );
}

#[test]
fn do_not_disturb_settling_a_walk_is_the_engine() {
    let mut engine = a_resting_sprite();
    engine.tick(&proposed_by("walk", AnimationSource::AiRequested));
    engine.set_do_not_disturb(true);
    let settled = engine.tick(&snapshot(100));
    assert_eq!(
        (settled.animation, settled.animation_source),
        ("sit", AnimationSource::Engine)
    );
}

/// A proposal held for a pass owns nothing until it takes the screen, and
/// then it is its own producer's, not the pass's.
#[test]
fn a_deferred_proposal_takes_the_source_only_when_it_takes_the_screen() {
    use crate::character::{IdleMember, IdlePlan};
    let mut behaviors = declared_behaviors();
    behaviors.insert(
        "groom".to_string(),
        Behavior {
            primitives: vec![],
            animation: Some("grooming".to_string()),
            then: None,
            weight: DEFAULT_WEIGHT,
            trigger: None,
            not_in_manifest: false,
            idle_type: false,
        },
    );
    let member = |name: &str, frames| IdleMember {
        name: name.into(),
        weight: 1,
        frames,
        fps: 6,
    };
    let mut engine = a_resting_sprite().with_behaviors(behaviors);
    engine.set_idle_plan(IdlePlan {
        loops: vec![member("idle", 1)],
        gestures: vec![member("grooming", 6)],
        digits: Vec::new(),
        react: None,
        possessed: None,
        idle_gesture_every_s: (10, 30),
        sleep_after_s: DEFAULT_SLEEP_AFTER_S,
        sleep_for_s: DEFAULT_SLEEP_FOR_S,
    });
    let grooming = engine.tick(&proposed_by("groom", AnimationSource::LocalDirector));
    assert_eq!(grooming.animation_source, AnimationSource::LocalDirector);
    let held = engine.tick(&proposed_by("greet", AnimationSource::AiRequested));
    assert_eq!(
        (held.playing_gesture.as_deref(), held.animation_source),
        (Some("grooming"), AnimationSource::LocalDirector)
    );
    let greeted = (0..12)
        .map(|_| engine.tick(&snapshot(100)))
        .find(|frame| frame.playing_gesture.is_none())
        .expect("the pass ends");
    assert_eq!(
        (greeted.animation, greeted.animation_source),
        ("react", AnimationSource::AiRequested)
    );
}

/// The swat ends a Chase, so it is that Chase's: the source is captured
/// before the stop drops the owner. A timeout swats nothing.
#[test]
fn a_chase_that_arrives_swats_for_whoever_started_it_and_a_timeout_does_not() {
    for source in [AnimationSource::AiMcp, AnimationSource::LocalDirector] {
        let mut engine = a_resting_sprite();
        let feet = engine.feet();
        engine.play(&[Primitive::Chase], source);
        let arrived = engine.tick(&WorldSnapshot {
            cursor: feet,
            ..snapshot(100)
        });
        assert_eq!(
            (arrived.animation, arrived.animation_source),
            ("react", source)
        );
    }

    let mut engine = a_resting_sprite();
    let feet = engine.feet();
    engine.play(&[Primitive::Chase], AnimationSource::AiMcp);
    let away = WorldSnapshot {
        cursor: Point {
            x: feet.x + 600.0,
            y: feet.y,
        },
        elapsed_ms: CHASE_TIMEOUT_MS,
        ..snapshot(100)
    };
    let gave_up = engine.tick(&away);
    assert_ne!(gave_up.animation, "react", "{gave_up:?}");
}

#[test]
fn a_digit_pass_is_user_invoked_and_nothing_continues_after_it() {
    use crate::character::{IdleMember, IdlePlan};
    let mut engine = a_resting_sprite();
    engine.set_idle_plan(IdlePlan {
        loops: vec![IdleMember {
            name: "idle".into(),
            weight: 1,
            frames: 1,
            fps: 1,
        }],
        gestures: Vec::new(),
        digits: vec![IdleMember {
            name: "wave".into(),
            weight: 1,
            frames: 3,
            fps: 6,
        }],
        react: None,
        possessed: None,
        idle_gesture_every_s: (10, 30),
        sleep_after_s: DEFAULT_SLEEP_AFTER_S,
        sleep_for_s: DEFAULT_SLEEP_FOR_S,
    });
    engine.set_possessed(true);
    settle(&mut engine, &snapshot(100));
    let pressed = engine.tick(&WorldSnapshot {
        possession: Intent {
            play: Some(1),
            ..Intent::default()
        },
        ..snapshot(100)
    });
    assert_eq!(
        (pressed.art_name(), pressed.animation_source),
        ("wave", AnimationSource::UserInvoked)
    );
    let after = settle(&mut engine, &snapshot(100));
    assert_eq!(
        (after.playing_behavior, after.playing_gesture),
        (None, None)
    );
}

/// Standing the sprite somewhere ends the move and the walk, so their
/// owners go with them.
#[test]
fn standing_the_sprite_elsewhere_ends_the_moves_provenance() {
    let mut engine = a_resting_sprite();
    engine
        .start_move(
            MoveRequest::To(GlobalX::new(500.0).unwrap()),
            AnimationSource::AiMcp,
        )
        .expect("a grounded sprite takes a move");
    engine.tick(&snapshot(100));
    engine.stand_at(Point { x: 300.0, y: 800.0 });
    let stood = engine.tick(&snapshot(100));
    assert_eq!(
        (stood.animation, stood.animation_source),
        ("idle", AnimationSource::IdleLoop)
    );
}

fn walking_for(source: AnimationSource) -> Engine {
    let mut engine = a_resting_sprite();
    engine.tick(&proposed_by("walk", source));
    let residual = (0..10)
        .map(|_| engine.tick(&snapshot(100)))
        .find(|frame| frame.playing_primitive.is_none() && frame.animation == "walk")
        .expect("the stride carries on past the Primitive");
    assert_eq!(residual.animation_source, source);
    engine
}

/// A startle over a walk is the user's while it shows. It stops nothing, so
/// the walk it covered comes back as its own owner's, not as the startle's.
#[test]
fn a_walk_returns_as_its_own_after_an_expression_over_it() {
    let mut engine = walking_for(AnimationSource::LocalDirector)
        .with_cursor_reactions(CursorReaction::React, CursorReaction::Indifferent);
    let feet = engine.feet();
    let startled = engine.tick(&WorldSnapshot {
        cursor: Point {
            x: feet.x + 100.0,
            y: feet.y,
        },
        ..snapshot(100)
    });
    assert_eq!(
        (startled.animation, startled.animation_source),
        ("react", AnimationSource::UserInvoked)
    );
    let resumed = (0..10)
        .map(|_| {
            engine.tick(&WorldSnapshot {
                cursor: Point {
                    x: feet.x + 100.0,
                    y: feet.y,
                },
                ..snapshot(100)
            })
        })
        .find(|frame| frame.playing_primitive.is_none())
        .expect("the startle ends");
    assert_eq!(
        (resumed.animation, resumed.animation_source),
        ("walk", AnimationSource::LocalDirector)
    );
}

/// A Poke stops the walk, so nothing it played revives that walk's owner.
#[test]
fn an_expression_that_stops_a_walk_does_not_revive_its_owner() {
    let mut engine = walking_for(AnimationSource::AiRequested);
    engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Poke],
        ..snapshot(100)
    });
    let after = settle(&mut engine, &snapshot(100));
    assert_eq!(
        (after.animation, after.animation_source),
        ("idle", AnimationSource::IdleLoop)
    );
}

#[test]
fn unknown_and_refused_proposals_leave_the_source_alone() {
    let mut engine = a_resting_sprite();
    let unknown = engine.tick(&proposed_by("nope", AnimationSource::AiRequested));
    assert_eq!(unknown.animation_source, AnimationSource::IdleLoop);

    // Mid-cooldown after a Poke, a walk is refused and the react stays the user's.
    engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Poke],
        ..snapshot(100)
    });
    let refused = engine.tick(&proposed_by("walk", AnimationSource::AiRequested));
    assert_eq!(refused.refused.as_deref(), Some("walk"));
    assert_eq!(
        (refused.animation, refused.animation_source),
        ("react", AnimationSource::UserInvoked)
    );
}

#[test]
fn a_refused_move_takes_no_source() {
    let mut engine = a_resting_sprite();
    let feet = engine.feet();
    let refused = engine.start_move(
        MoveRequest::To(GlobalX::new(feet.x).unwrap()),
        AnimationSource::AiMcp,
    );
    assert!(refused.is_err());
    let frame = engine.tick(&snapshot(100));
    assert_eq!(frame.animation_source, AnimationSource::IdleLoop);
}

#[test]
fn a_verb_that_interrupts_a_chase_swats_for_nobody() {
    let mut engine = a_resting_sprite();
    let feet = engine.feet();
    engine.play(&[Primitive::Chase], AnimationSource::AiMcp);
    let poked = engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Poke],
        cursor: Point {
            x: feet.x + 400.0,
            y: feet.y,
        },
        ..snapshot(100)
    });
    assert_eq!(
        (poked.animation, poked.animation_source),
        ("react", AnimationSource::UserInvoked)
    );
}

#[test]
fn perched_fallback_talk_is_the_speakers() {
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 }).with_sleep_after_s((60, 60));
    settle(&mut engine, &perch(50.0, 400.0));
    let held = engine.tick(&WorldSnapshot {
        speaking_for: Some(AnimationSource::Onboarding),
        ..perch(50.0, 400.0)
    });
    assert_eq!(
        (held.state, held.animation, held.animation_source),
        (State::Perched, "talk", AnimationSource::Onboarding)
    );
}

/// A Character switch ends what was playing, not the stride under way: the
/// velocity survives it, and so does the owner that set it going.
#[test]
fn a_character_switch_keeps_the_owner_of_a_walk_it_does_not_stop() {
    let mut engine = walking_for(AnimationSource::LocalDirector);
    engine.retarget(
        declared_behaviors(),
        CursorReaction::Indifferent,
        CursorReaction::Indifferent,
    );
    let after = engine.tick(&snapshot(100));
    assert_eq!(
        (after.animation, after.animation_source),
        ("walk", AnimationSource::LocalDirector)
    );
}
