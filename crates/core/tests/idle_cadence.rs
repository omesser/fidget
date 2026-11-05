//! The Engine's own idle: a calm loop, a seeded gap, one gesture pass.
//!
//! Asserts the frame `Character::draw` shows. The beat names that frame.

use std::collections::BTreeMap;

use fidget_core::character::{
    load, Behavior, Character, IdleBeat, Primitive, CHARACTER_MANIFEST_FILE, REQUIRED_ANIMATIONS,
};
use fidget_core::engine::{
    BehaviorProposal, Engine, Point, Rect, State, Verb, Window, WorldSnapshot,
};

const FRAME: &[u8] = include_bytes!("fixtures/alpha-2x2.png");

fn package(manifest: &str) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for name in REQUIRED_ANIMATIONS {
        files.insert(format!("{name}-0.png"), FRAME.to_vec());
    }
    files.insert(
        CHARACTER_MANIFEST_FILE.to_string(),
        manifest.as_bytes().to_vec(),
    );
    files
}

/// Base `idle`, a two-frame `sway` loop, and a two-frame `spin` gesture.
/// `every` is `idle_gesture_every_s` when set. `spin_fps` is the gesture's cadence.
fn character(every: Option<&str>, spin_fps: u32, sway_weight: u32) -> Character {
    let mut manifest = String::from("name = \"Blip\"\n");
    if let Some(every) = every {
        manifest.push_str(every);
        manifest.push('\n');
    }
    for name in REQUIRED_ANIMATIONS {
        if name == "idle" {
            manifest.push_str(
                "[animations.idle]\nframes = [\"idle-0.png\", \"idle-0.png\"]\nfps = 8\nweight = 1\n",
            );
        } else {
            manifest.push_str(&format!(
                "[animations.{name}]\nframes = [\"{name}-0.png\"]\n"
            ));
        }
    }
    manifest.push_str(&format!(
        "[animations.sway]\nframes = [\"idle-0.png\", \"idle-0.png\"]\n\
         fps = 8\nvariant_of = \"idle\"\nweight = {sway_weight}\n\
         [animations.spin]\nframes = [\"idle-0.png\", \"idle-0.png\"]\n\
         fps = {spin_fps}\ngesture_of = \"idle\"\nweight = 1\n"
    ));
    load(&package(&manifest)).unwrap_or_else(|errors| panic!("loads: {errors:?}"))
}

fn world(elapsed_ms: u32) -> WorldSnapshot {
    WorldSnapshot {
        displays: vec![Rect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        }],
        elapsed_ms,
        ..WorldSnapshot::default()
    }
}

fn engine_for(character: &Character, seed: u64) -> Engine {
    Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(character.behaviors.clone())
        .with_variant_seed(seed)
        .with_idle_plan(character.idle_plan())
}

fn shown(character: &Character, frame: &fidget_core::engine::Frame) -> (String, usize) {
    let drawn = character
        .draw(
            frame.animation,
            frame.animation_ms,
            frame.ring_choice(),
            frame.facing,
        )
        .expect("the package draws");
    (drawn.animation.to_string(), drawn.index)
}

fn behavior(primitives: Vec<Primitive>, idle_type: bool) -> Behavior {
    use fidget_core::character::Step;
    Behavior {
        steps: primitives.into_iter().map(Step::Primitive).collect(),
        then: None,
        weight: 10,
        trigger: None,
        not_in_manifest: false,
        idle_type,
    }
}

fn propose(name: &str) -> WorldSnapshot {
    WorldSnapshot {
        proposal: Some(BehaviorProposal {
            behavior: name.into(),
            dialogue: None,
            direction: None,
        }),
        ..world(100)
    }
}

/// Tick until the family is idle, then return that frame.
fn first_idle(engine: &mut Engine) -> fidget_core::engine::Frame {
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if frame.animation == "idle" {
            return frame;
        }
    }
    panic!("the sprite never idled");
}

#[test]
fn an_idle_stretch_loops_before_its_first_gesture() {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    let mut engine = engine_for(&character, 1);
    let frame = first_idle(&mut engine);
    assert!(
        matches!(frame.idle_beat, Some(IdleBeat::Loop(_))),
        "the stretch opens on a loop: {:?}",
        frame.idle_beat
    );
    let (name, index) = shown(&character, &frame);
    assert_ne!(name, "spin", "the gesture waits out the gap");
    assert_eq!(index, 0, "the loop starts at its first frame");
}

#[test]
fn a_gesture_plays_one_pass_and_the_stretch_returns_to_a_loop() {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    let mut engine = engine_for(&character, 1);
    let _ = first_idle(&mut engine);

    let mut spin = Vec::new();
    let mut back = None;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        assert_eq!(frame.animation, "idle", "still the same idle stretch");
        let drawn = shown(&character, &frame);
        if drawn.0 == "spin" {
            assert!(
                matches!(frame.idle_beat, Some(IdleBeat::Gesture(_))),
                "a gesture names itself: {:?}",
                frame.idle_beat
            );
            assert!(back.is_none(), "a second gesture started: {spin:?}");
            spin.push(drawn.1);
        } else if !spin.is_empty() {
            assert!(
                matches!(frame.idle_beat, Some(IdleBeat::Loop(_))),
                "the pass hands back to a loop"
            );
            back = Some(drawn);
            break;
        }
    }

    let mut saw_second = false;
    for index in &spin {
        if *index == 1 {
            saw_second = true;
        }
        if saw_second && *index == 0 {
            panic!("the gesture wrapped instead of ending: {spin:?}");
        }
    }
    assert!(saw_second, "both frames of the one pass played: {spin:?}");
    let (name, index) = back.expect("the stretch returned to a loop");
    assert_ne!(name, "spin");
    assert_eq!(index, 0, "the next loop starts at frame 0");
}

/// Milliseconds from the start of idle until the gesture is on screen.
fn gap_until_gesture(character: &Character, seed: u64) -> u32 {
    let mut engine = engine_for(character, seed);
    let mut held_ms = 0;
    for _ in 0..400 {
        let frame = engine.tick(&world(100));
        if frame.animation != "idle" {
            continue;
        }
        held_ms += 100;
        if shown(character, &frame).0 == "spin" {
            return held_ms;
        }
    }
    panic!("no gesture after {held_ms}ms");
}

#[test]
fn a_fixed_gap_holds_the_loop_for_that_long() {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    assert_eq!(
        gap_until_gesture(&character, 1),
        1_000,
        "one second of loop before the gesture"
    );
}

#[test]
fn a_gap_stays_inside_the_declared_range() {
    let character = character(Some("idle_gesture_every_s = [1, 3]"), 1, 0);
    for seed in [1_u64, 2, 9, 40] {
        let held_ms = gap_until_gesture(&character, seed);
        assert!(
            (1_000..=3_000).contains(&held_ms),
            "seed {seed} waited {held_ms}ms, outside 1 to 3 seconds"
        );
    }
}

#[test]
fn the_default_gap_stays_inside_ten_to_thirty_seconds() {
    let character = character(None, 1, 0);
    for seed in [1_u64, 4, 11] {
        let held_ms = gap_until_gesture(&character, seed);
        assert!(
            (10_000..=30_000).contains(&held_ms),
            "seed {seed} waited {held_ms}ms, outside the default"
        );
    }
}

#[test]
fn the_same_seed_loops_and_gestures_alike() {
    let beats = |seed: u64| {
        let character = character(Some("idle_gesture_every_s = [1, 3]"), 1, 1);
        let mut engine = engine_for(&character, seed);
        let frame = first_idle(&mut engine);
        let mut drawn = vec![shown(&character, &frame)];
        for _ in 0..40 {
            drawn.push(shown(&character, &engine.tick(&world(100))));
        }
        drawn
    };

    assert_eq!(beats(7), beats(7), "one seed replays");
    let varied: Vec<_> = [1_u64, 2, 3, 8, 9, 40].map(beats).into_iter().collect();
    assert!(
        varied.iter().any(|sequence| sequence != &varied[0]),
        "two seeds do not share a stretch"
    );
}

#[test]
fn no_gestures_do_not_burn_the_seed() {
    let mut manifest = String::from("name = \"Blip\"\nidle_gesture_every_s = [1, 3]\n");
    for name in REQUIRED_ANIMATIONS {
        let weight = if name == "idle" { "weight = 1\n" } else { "" };
        manifest.push_str(&format!(
            "[animations.{name}]\nframes = [\"{name}-0.png\"]\n{weight}"
        ));
    }
    manifest.push_str(
        "[animations.sway]\nframes = [\"idle-0.png\", \"idle-0.png\"]\n\
         variant_of = \"idle\"\nweight = 1\n",
    );
    let character = load(&package(&manifest)).unwrap_or_else(|errors| panic!("{errors:?}"));

    let member_after = |seed: u64, idle_ticks: u32| {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
            .with_behaviors(BTreeMap::from([(
                "ponder".to_string(),
                behavior(vec![Primitive::Idle], false),
            )]))
            .with_variant_seed(seed)
            .with_idle_plan(character.idle_plan());
        let _ = first_idle(&mut engine);
        for _ in 0..idle_ticks {
            engine.tick(&world(100));
        }
        engine.tick(&propose("ponder"));
        let frame = engine.tick(&world(100));
        shown(&character, &frame).0
    };

    let mut seen = BTreeMap::new();
    for seed in [1_u64, 2, 7, 9, 40] {
        let early = member_after(seed, 0);
        assert_eq!(
            early,
            member_after(seed, 30),
            "seed {seed} rolled a gap with nothing to play"
        );
        *seen.entry(early).or_insert(0) += 1;
    }
    assert!(
        seen.len() > 1,
        "the loop ring can land on either member: {seen:?}"
    );
}

#[test]
fn a_proposal_during_a_gesture_plays_when_the_pass_ends() {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    let mut behaviors = character.behaviors.clone();
    behaviors.insert("wave".to_string(), behavior(vec![Primitive::Talk], false));
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(behaviors)
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());
    let _ = first_idle(&mut engine);

    let mut proposed = false;
    let mut played_after = false;
    for _ in 0..50 {
        if !proposed {
            let frame = engine.tick(&world(100));
            if shown(&character, &frame).0 == "spin" {
                let held = engine.tick(&propose("wave"));
                assert_eq!(held.behavior, None, "the pass is not cut");
                assert_ne!(held.playing_primitive, Some(Primitive::Talk));
                assert_eq!(shown(&character, &held).0, "spin");
                proposed = true;
            }
            continue;
        }
        let frame = engine.tick(&world(100));
        if frame.animation == "talk" {
            played_after = true;
            break;
        }
    }
    assert!(proposed, "the stretch reached the gesture");
    assert!(played_after, "the deferred line plays when the pass ends");
}

#[test]
fn a_poke_during_a_gesture_still_wins() {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    let mut behaviors = character.behaviors.clone();
    behaviors.insert("wave".to_string(), behavior(vec![Primitive::Talk], false));
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(behaviors)
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());
    let _ = first_idle(&mut engine);
    let mut saw_spin = false;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if shown(&character, &frame).0 == "spin" {
            saw_spin = true;
            break;
        }
    }
    assert!(saw_spin, "the gesture was on screen");

    let held = engine.tick(&propose("wave"));
    assert_eq!(held.animation, "idle", "the proposal waits");
    let poked = engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Poke],
        ..world(100)
    });
    assert_eq!(poked.animation, "react", "the poke takes the screen");

    let mut saw_talk = false;
    let mut looped = false;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if frame.animation == "talk" {
            saw_talk = true;
        }
        if frame.animation == "idle" {
            assert!(matches!(frame.idle_beat, Some(IdleBeat::Loop(_))));
            assert_eq!(shown(&character, &frame).1, 0);
            looped = true;
            break;
        }
    }
    assert!(!saw_talk, "the deferred line does not play after the poke");
    assert!(looped, "idle returns on a loop");
}

#[test]
fn perched_idle_uses_the_same_loop_and_gesture() {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    let perch = WorldSnapshot {
        windows: vec![Window {
            id: 1,
            rect: Rect {
                x: 0.0,
                y: 400.0,
                width: 800.0,
                height: 200.0,
            },
        }],
        ..world(100)
    };
    let mut engine = engine_for(&character, 1);
    let mut saw_sit = false;
    let mut saw_loop = false;
    let mut saw_gesture = false;
    for _ in 0..80 {
        let frame = engine.tick(&perch);
        if frame.state == State::Perched && frame.animation == "sit" {
            saw_sit = true;
            assert!(frame.idle_beat.is_none(), "sit is not the idle machine");
        }
        if frame.animation == "idle" {
            match &frame.idle_beat {
                Some(IdleBeat::Loop(_)) => saw_loop = true,
                Some(IdleBeat::Gesture(_)) => {
                    assert!(saw_loop, "the gesture follows a loop");
                    saw_gesture = true;
                    break;
                }
                None => panic!("perched idle left the machine"),
            }
        }
    }
    assert!(saw_sit, "the perch sits before it idles");
    assert!(saw_loop && saw_gesture, "then the grounded machine");
}

#[test]
fn a_behaviors_idle_loops_the_loop_ring_only() {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 1);
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(BTreeMap::from([(
            "ponder".to_string(),
            behavior(vec![Primitive::Idle], false),
        )]))
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());
    let _ = first_idle(&mut engine);
    engine.tick(&propose("ponder"));

    let mut indices = Vec::new();
    for _ in 0..12 {
        let frame = engine.tick(&world(100));
        if frame.playing_primitive != Some(Primitive::Idle) {
            break;
        }
        assert!(
            frame.idle_beat.is_none(),
            "a plain idle step does not run the machine"
        );
        let drawn = shown(&character, &frame);
        assert_ne!(drawn.0, "spin", "the gesture stays out of the loop ring");
        assert!(
            drawn.0 == "idle" || drawn.0 == "sway",
            "the step stays on the loop ring: {}",
            drawn.0
        );
        indices.push(drawn.1);
    }
    assert!(
        indices.windows(3).any(|triple| triple == [0, 1, 0]),
        "the behavior's idle looped the strip: {indices:?}"
    );
}

#[test]
fn an_idle_typed_behavior_uses_the_loop_and_gesture_machine() {
    // 8 fps makes the two-frame pass 250ms, inside the four idle steps.
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 8, 0);
    // Talk first so the machine starts fresh, then an idle-typed step long
    // enough to cross the one-second gap.
    let mut behaviors = character.behaviors.clone();
    behaviors.insert("linger".to_string(), behavior(vec![Primitive::Talk], false));
    behaviors.insert("muse".to_string(), behavior(vec![Primitive::Idle; 4], true));
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(behaviors)
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());
    let _ = first_idle(&mut engine);
    engine.tick(&propose("linger"));
    let started = engine.tick(&propose("muse"));
    assert_eq!(started.playing_primitive, Some(Primitive::Idle));
    assert!(
        matches!(started.idle_beat, Some(IdleBeat::Loop(_))),
        "the idle step opens on a loop: {:?}",
        started.idle_beat
    );

    let mut saw_gesture = false;
    let mut back = false;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if frame.playing_behavior.as_deref() != Some("muse") {
            break;
        }
        match &frame.idle_beat {
            Some(IdleBeat::Gesture(_)) => {
                assert_eq!(shown(&character, &frame).0, "spin");
                saw_gesture = true;
            }
            Some(IdleBeat::Loop(_)) if saw_gesture => back = true,
            Some(IdleBeat::Loop(_)) => {}
            None => panic!("the idle-typed step left the machine"),
        }
    }
    assert!(
        saw_gesture && back,
        "the step plays one gesture, then a loop"
    );
}

fn rested(character: &Character, seed: u64) -> Engine {
    let mut engine = engine_for(character, seed);
    let _ = first_idle(&mut engine);
    engine.set_sleep_after_s((1, 1));
    engine.set_sleep_for_s((1, 1));
    engine
}

#[test]
fn sleep_waits_until_the_behavior_ends() {
    let character = character(Some("idle_gesture_every_s = [30, 30]"), 1, 0);
    let mut behaviors = character.behaviors.clone();
    behaviors.insert(
        "ponder".to_string(),
        behavior(vec![Primitive::Idle; 3], false),
    );
    let mut engine = rested(&character, 1).with_behaviors(behaviors);
    engine.tick(&propose("ponder"));

    let mut during = false;
    let mut asleep_after = false;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if frame.playing_behavior.as_deref() == Some("ponder") {
            assert_ne!(frame.state, State::Asleep, "the behavior finishes first");
            during = true;
        } else if during {
            assert_eq!(frame.state, State::Asleep, "then it sleeps");
            asleep_after = true;
            break;
        }
    }
    assert!(
        during && asleep_after,
        "the behavior ran and sleep followed it"
    );
}

#[test]
fn a_proposal_does_not_reset_sleep_and_a_poke_does() {
    let character = character(Some("idle_gesture_every_s = [30, 30]"), 1, 0);
    let mut behaviors = character.behaviors.clone();
    behaviors.insert("wave".to_string(), behavior(vec![Primitive::Talk], false));

    let mut engine = rested(&character, 1).with_behaviors(behaviors);
    engine.tick(&world(500));
    engine.tick(&propose("wave"));
    let mut asleep = false;
    for _ in 0..20 {
        let frame = engine.tick(&world(100));
        if frame.state == State::Asleep {
            asleep = true;
            break;
        }
    }
    assert!(asleep, "the line did not put the clock back");

    let mut engine = rested(&character, 1);
    engine.tick(&world(500));
    engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Poke],
        ..world(100)
    });
    let later = engine.tick(&world(500));
    assert_ne!(later.state, State::Asleep, "a poke starts the clock over");
}

#[test]
fn a_chat_line_resets_sleep() {
    let character = character(Some("idle_gesture_every_s = [30, 30]"), 1, 0);
    let mut engine = rested(&character, 1);
    engine.tick(&world(500));
    engine.tick(&WorldSnapshot {
        chat: true,
        ..world(100)
    });
    let later = engine.tick(&world(500));
    assert_ne!(
        later.state,
        State::Asleep,
        "a line to the AI starts the clock over"
    );
}

#[test]
fn a_long_dwell_resets_sleep_without_replacing_the_short_dwell() {
    let character = character(Some("idle_gesture_every_s = [30, 30]"), 1, 0);
    let mut engine = rested(&character, 1);
    let at = engine.tick(&world(100)).position;
    let on_sprite = |elapsed| WorldSnapshot {
        cursor: at,
        elapsed_ms: elapsed,
        ..world(elapsed)
    };
    let noticed = engine.tick(&on_sprite(500));
    assert!(
        noticed.addressed,
        "the 400ms dwell still addresses the Director"
    );
    assert_ne!(noticed.state, State::Asleep);

    engine.tick(&on_sprite(10_000));
    let later = engine.tick(&world(500));
    assert_ne!(
        later.state,
        State::Asleep,
        "ten seconds on the sprite starts the clock over"
    );
}

#[test]
fn an_idle_gesture_finishes_before_sleep() {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    let mut engine = rested(&character, 1);
    let mut saw_second = false;
    let mut asleep_after = false;
    for _ in 0..50 {
        let frame = engine.tick(&world(100));
        if shown(&character, &frame).0 == "spin" {
            assert_ne!(frame.state, State::Asleep, "the pass is not cut");
            if shown(&character, &frame).1 == 1 {
                saw_second = true;
            }
        } else if saw_second && frame.state == State::Asleep {
            asleep_after = true;
            break;
        }
    }
    assert!(
        saw_second && asleep_after,
        "the pass played out, then sleep"
    );
}

#[test]
fn waking_returns_to_an_idle_loop() {
    let character = character(Some("idle_gesture_every_s = [30, 30]"), 1, 0);
    let mut engine = rested(&character, 1);
    assert_eq!(engine.tick(&world(1_000)).state, State::Asleep);
    let woken = engine.tick(&world(1_000));
    assert_eq!(woken.state, State::Grounded, "the sleep timer woke it");
    assert!(
        matches!(woken.idle_beat, Some(IdleBeat::Loop(_))),
        "it wakes into a loop: {:?}",
        woken.idle_beat
    );
    assert_ne!(shown(&character, &woken).0, "spin");
}

fn manifest_naming(tail_for: &str, tail: &str) -> String {
    let mut manifest = String::from("name = \"Blip\"\n");
    for name in REQUIRED_ANIMATIONS {
        let extra = if name == tail_for { tail } else { "" };
        manifest.push_str(&format!(
            "[animations.{name}]\nframes = [\"{name}-0.png\"]\n{extra}"
        ));
    }
    manifest
}

#[test]
fn a_reversed_gap_is_rejected_by_name() {
    let errors = load(&package("name = \"Blip\"\nidle_gesture_every_s = [9, 1]\n"))
        .expect_err("reversed range");
    // The manifest is only the key, so the required animations are missing too.
    // The range error is the one this test is about.
    assert!(
        errors
            .iter()
            .any(|error| error.contains("idle_gesture_every_s")),
        "{errors:?}"
    );
}

#[test]
fn a_gap_that_is_not_a_pair_is_rejected_by_name() {
    let errors =
        load(&package("name = \"Blip\"\nidle_gesture_every_s = 4\n")).expect_err("not a pair");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("idle_gesture_every_s")),
        "{errors:?}"
    );
}

#[test]
fn gesture_of_anything_but_idle_is_rejected_by_name() {
    let manifest = format!(
        "{}[animations.spin]\nframes = [\"idle-0.png\"]\ngesture_of = \"walk\"\n",
        manifest_naming("walk", "")
    );
    let errors = load(&package(&manifest)).expect_err("gesture_of walk");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("gesture_of") && error.contains("walk")),
        "{errors:?}"
    );
}

#[test]
fn gesture_of_a_variant_is_rejected_by_name() {
    let manifest = format!(
        "{}[animations.sway]\nframes = [\"idle-0.png\"]\nvariant_of = \"idle\"\n\
         [animations.spin]\nframes = [\"idle-0.png\"]\ngesture_of = \"sway\"\n",
        manifest_naming("idle", "")
    );
    let errors = load(&package(&manifest)).expect_err("gesture of a variant");
    assert!(
        errors.iter().any(|error| error.contains("spin")
            && error.contains("sway")
            && error.contains("variant")),
        "{errors:?}"
    );
}
