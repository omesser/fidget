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
    // A gap under 5 seconds is a test seam on the Engine, not a manifest.
    // The loader rejects it.
    if let Some(every) = every.filter(|line| !line.contains("[1,")) {
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
    // `spin` is an idle-gesture and keeps that name. `sway` stays a calm loop.
    manifest.push_str(&format!(
        "[animations.sway]\nframes = [\"idle-0.png\", \"idle-0.png\"]\n\
         fps = 8\nvariant_of = \"idle\"\nweight = {sway_weight}\n\
         [animations.spin]\nframes = [\"idle-0.png\", \"idle-0.png\"]\n\
         fps = {spin_fps}\nvariant_of = \"idle-gesture\"\nweight = 1\n"
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
    paced(character, seed, None)
}

/// `every` is a gap the loader would refuse, so the test sets it on the Engine.
fn paced(character: &Character, seed: u64, every: Option<(u32, u32)>) -> Engine {
    let engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(character.behaviors.clone())
        .with_variant_seed(seed)
        .with_idle_plan(character.idle_plan());
    match every {
        Some(range) => engine.with_idle_gesture_every_s(range),
        None => engine,
    }
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

fn a_poke() -> WorldSnapshot {
    WorldSnapshot {
        verbs: vec![Verb::Poke],
        ..world(100)
    }
}

/// Nothing is left playing and the sprite shows its idle.
fn assert_back_to_idle(character: &Character, frame: &fidget_core::engine::Frame) {
    assert_eq!(frame.playing_behavior, None);
    assert_eq!(frame.playing_gesture, None);
    assert_eq!(shown(character, frame).0, "idle");
}

fn behavior(primitives: Vec<Primitive>, idle_type: bool) -> Behavior {
    Behavior {
        primitives: primitives.into_iter().collect(),
        animation: None,
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
        matches!(frame.idle_beat, Some(IdleBeat::IdleLoop(_))),
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
    let mut engine = paced(&character, 1, Some((1, 1)));
    let _ = first_idle(&mut engine);

    let mut spin = Vec::new();
    let mut back = None;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        assert_eq!(frame.animation, "idle", "still the same idle stretch");
        let drawn = shown(&character, &frame);
        if drawn.0 == "spin" {
            assert!(
                matches!(frame.idle_beat, Some(IdleBeat::IdleGesture(_))),
                "a gesture names itself: {:?}",
                frame.idle_beat
            );
            assert!(back.is_none(), "a second gesture started: {spin:?}");
            spin.push(drawn.1);
        } else if !spin.is_empty() {
            assert!(
                matches!(frame.idle_beat, Some(IdleBeat::IdleLoop(_))),
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
fn gap_until_gesture(character: &Character, seed: u64, every: Option<(u32, u32)>) -> u32 {
    let mut engine = paced(character, seed, every);
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
        gap_until_gesture(&character, 1, Some((1, 1))),
        1_000,
        "one second of loop before the gesture"
    );
}

#[test]
fn a_gap_stays_inside_the_declared_range() {
    let character = character(Some("idle_gesture_every_s = [1, 3]"), 1, 0);
    for seed in [1_u64, 2, 9, 40] {
        let held_ms = gap_until_gesture(&character, seed, Some((1, 3)));
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
        let held_ms = gap_until_gesture(&character, seed, None);
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
        let mut engine = paced(&character, seed, Some((1, 3)));
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
    let mut manifest = String::from("name = \"Blip\"\n");
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
        .with_idle_plan(character.idle_plan())
        .with_idle_gesture_every_s((1, 1));
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
        .with_idle_plan(character.idle_plan())
        .with_idle_gesture_every_s((1, 1));
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
    let poked = engine.tick(&a_poke());
    assert_eq!(poked.animation, "react", "the poke takes the screen");

    let mut saw_talk = false;
    let mut looped = false;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if frame.animation == "talk" {
            saw_talk = true;
        }
        if frame.animation == "idle" {
            assert!(matches!(frame.idle_beat, Some(IdleBeat::IdleLoop(_))));
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
    let mut engine = paced(&character, 1, Some((1, 1)));
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
                Some(IdleBeat::IdleLoop(_)) => saw_loop = true,
                Some(IdleBeat::IdleGesture(_)) => {
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
        .with_idle_plan(character.idle_plan())
        .with_idle_gesture_every_s((1, 1));
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
        "the behavior's idle looped the animation: {indices:?}"
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
        .with_idle_plan(character.idle_plan())
        .with_idle_gesture_every_s((1, 1));
    let _ = first_idle(&mut engine);
    engine.tick(&propose("linger"));
    let started = engine.tick(&propose("muse"));
    assert_eq!(started.playing_primitive, Some(Primitive::Idle));
    assert!(
        matches!(started.idle_beat, Some(IdleBeat::IdleLoop(_))),
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
            Some(IdleBeat::IdleGesture(_)) => {
                assert_eq!(shown(&character, &frame).0, "spin");
                saw_gesture = true;
            }
            Some(IdleBeat::IdleLoop(_)) if saw_gesture => back = true,
            Some(IdleBeat::IdleLoop(_)) => {}
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
    engine.tick(&a_poke());
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
    let mut engine = paced(&character, 1, Some((1, 1)));
    let _ = first_idle(&mut engine);
    engine.set_sleep_after_s((1, 1));
    engine.set_sleep_for_s((1, 1));
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
        matches!(woken.idle_beat, Some(IdleBeat::IdleLoop(_))),
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
fn a_gap_outside_five_to_sixty_seconds_is_rejected_by_name() {
    for written in ["[4, 10]", "[1, 1]", "[10, 61]", "[60, 90]"] {
        let errors = load(&package(&format!(
            "name = \"Blip\"\nidle_gesture_every_s = {written}\n"
        )))
        .expect_err("outside 5 to 60");
        assert!(
            errors.iter().any(|error| {
                error.contains("idle_gesture_every_s") && error.contains("5 to 60")
            }),
            "{written}: {errors:?}"
        );
    }
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
fn a_gesture_of_key_is_rejected_by_name() {
    let manifest = format!(
        "{}[animations.spin]\nframes = [\"idle-0.png\"]\ngesture_of = \"idle\"\n",
        manifest_naming("idle", "")
    );
    let errors = load(&package(&manifest)).expect_err("gesture_of is not a key");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("gesture_of") && error.contains("unknown")),
        "{errors:?}"
    );
}

#[test]
fn an_idle_gesture_keeps_its_name_without_a_base_animation() {
    let manifest = format!(
        "{}[animations.spin]\nframes = [\"idle-0.png\"]\nvariant_of = \"idle-gesture\"\n",
        manifest_naming("idle", "")
    );
    load(&package(&manifest)).expect("idle-gesture is a category, not an animation's name");
}

fn chained_behaviors_character(behaviors: &str) -> Character {
    let mut manifest = String::from("name = \"Then test\"\n");
    for name in REQUIRED_ANIMATIONS {
        manifest.push_str(&format!(
            "[animations.{name}]\nframes = [\"{name}-0.png\"]\n"
        ));
    }
    manifest.push_str(
        "[animations.spin]\nframes = [\"idle-0.png\", \"idle-0.png\"]\nfps = 2\nvariant_of = \"idle-gesture\"\n",
    );
    manifest.push_str(behaviors);
    load(&package(&manifest)).unwrap_or_else(|errors| panic!("loads: {errors:?}"))
}

#[test]
fn primitive_then_chain_draws_each_behavior_before_the_named_gesture() {
    let character = chained_behaviors_character(
        r#"
[behaviors.a]
play = ["react"]
then = "b"
[behaviors.b]
play = ["talk"]
then = "c"
[behaviors.c]
play = ["spin"]
"#,
    );
    let mut engine = engine_for(&character, 1);
    first_idle(&mut engine);
    let a = engine.tick(&propose("a"));
    assert_eq!(shown(&character, &a), ("react".into(), 0), "A draws first");
    let b = engine.tick(&world(600));
    assert_eq!(shown(&character, &b), ("talk".into(), 0), "B follows A");
    let c = engine.tick(&world(600));
    assert_eq!(c.playing_behavior.as_deref(), Some("c"), "C starts: {c:?}");
    assert_eq!(c.playing_gesture.as_deref(), Some("spin"));
    assert_eq!(shown(&character, &c), ("spin".into(), 0));
    let last = engine.tick(&world(500));
    assert_eq!(shown(&character, &last), ("spin".into(), 1));
    let complete = engine.tick(&world(500));
    assert_back_to_idle(&character, &complete);
}

#[test]
fn longer_then_chain_crosses_animations_and_finishes_on_primitives() {
    let character = chained_behaviors_character(
        r#"
[animations.bow]
frames = ["idle-0.png", "idle-0.png"]
fps = 2
variant_of = "idle-gesture"
[behaviors.a]
play = ["react", "talk"]
then = "b"
[behaviors.b]
play = ["sit", "react"]
then = "c"
[behaviors.c]
play = ["talk"]
then = "d"
[behaviors.d]
play = ["react"]
then = "e"
[behaviors.e]
play = ["sit"]
then = "spin"
[behaviors.spin]
play = ["spin"]
then = "bow"
[behaviors.bow]
play = ["bow"]
then = "finish"
[behaviors.finish]
play = ["react", "sit"]
"#,
    );
    let mut engine = engine_for(&character, 1);
    first_idle(&mut engine);
    assert_eq!(shown(&character, &engine.tick(&propose("a"))).0, "react");
    for expected in ["talk", "sit", "react", "talk", "react", "sit"] {
        let frame = engine.tick(&world(600));
        assert_eq!(shown(&character, &frame).0, expected);
    }
    for gesture in ["spin", "bow"] {
        let frame = engine.tick(&world(if gesture == "spin" { 600 } else { 500 }));
        assert_eq!(frame.playing_behavior.as_deref(), Some(gesture));
        assert_eq!(frame.playing_gesture.as_deref(), Some(gesture));
        assert_eq!(shown(&character, &frame), (gesture.into(), 0));
        assert_eq!(
            shown(&character, &engine.tick(&world(500))),
            (gesture.into(), 1)
        );
    }
    let finish = engine.tick(&world(500));
    assert_eq!(finish.playing_behavior.as_deref(), Some("finish"));
    assert_eq!(shown(&character, &finish).0, "react");
    assert_eq!(shown(&character, &engine.tick(&world(600))).0, "sit");
    let complete = engine.tick(&world(600));
    assert_back_to_idle(&character, &complete);
}

#[test]
fn gesture_reached_through_then_honors_latest_proposal_and_poke_cancellation() {
    let character = chained_behaviors_character(
        r#"
[behaviors.a]
play = ["react"]
then = "b"
[behaviors.b]
play = ["talk"]
then = "c"
[behaviors.c]
play = ["spin"]
then = "after"
[behaviors.after]
play = ["talk"]
[behaviors.earlier]
play = ["react"]
[behaviors.latest]
play = ["sit"]
"#,
    );
    for poke in [false, true] {
        let mut engine = engine_for(&character, 1);
        first_idle(&mut engine);
        engine.tick(&propose("a"));
        engine.tick(&world(600));
        let c = engine.tick(&world(600));
        assert_eq!(c.playing_behavior.as_deref(), Some("c"));
        assert_eq!(shown(&character, &c), ("spin".into(), 0));
        for proposal in ["earlier", "latest"] {
            let held = engine.tick(&propose(proposal));
            assert_eq!(held.playing_behavior.as_deref(), Some("c"));
            assert_eq!(shown(&character, &held).0, "spin");
        }
        if poke {
            let poked = engine.tick(&a_poke());
            assert_eq!(shown(&character, &poked).0, "react");
            for _ in 0..20 {
                let frame = engine.tick(&world(100));
                assert_eq!(
                    frame.playing_behavior, None,
                    "poke cancels the chain and slot"
                );
                assert_eq!(frame.playing_gesture, None);
                assert_ne!(shown(&character, &frame).0, "sit");
            }
        } else {
            let latest = engine.tick(&world(800));
            assert_eq!(latest.playing_behavior.as_deref(), Some("latest"));
            assert_eq!(latest.playing_gesture, None);
            assert_eq!(shown(&character, &latest).0, "sit");
            for _ in 0..20 {
                let frame = engine.tick(&world(100));
                assert_ne!(frame.playing_behavior.as_deref(), Some("earlier"));
                assert_ne!(frame.playing_behavior.as_deref(), Some("after"));
                assert_eq!(frame.playing_gesture, None);
            }
        }
    }
}

#[test]
fn primitive_only_then_chain_finishes_without_replaying() {
    let character = chained_behaviors_character(
        r#"
[behaviors.a]
play = ["react"]
then = "b"
[behaviors.b]
play = ["talk"]
then = "c"
[behaviors.c]
play = ["sit"]
"#,
    );
    let mut engine = engine_for(&character, 1);
    first_idle(&mut engine);
    assert_eq!(shown(&character, &engine.tick(&propose("a"))).0, "react");
    assert_eq!(shown(&character, &engine.tick(&world(600))).0, "talk");
    assert_eq!(shown(&character, &engine.tick(&world(600))).0, "sit");
    for _ in 0..3 {
        let complete = engine.tick(&world(600));
        assert_back_to_idle(&character, &complete);
    }
}

#[test]
fn poke_during_flattened_primitives_cancels_the_gesture_continuation() {
    let character = chained_behaviors_character(
        r#"
[behaviors.a]
play = ["talk"]
then = "b"
[behaviors.b]
play = ["sit"]
then = "c"
[behaviors.c]
play = ["spin"]
"#,
    );
    let mut engine = engine_for(&character, 1);
    first_idle(&mut engine);
    assert_eq!(shown(&character, &engine.tick(&propose("a"))).0, "talk");
    assert_eq!(shown(&character, &engine.tick(&world(600))).0, "sit");
    let poked = engine.tick(&a_poke());
    assert_eq!(shown(&character, &poked).0, "react");
    for _ in 0..20 {
        let frame = engine.tick(&world(100));
        assert_eq!(frame.playing_behavior, None);
        assert_eq!(frame.playing_gesture, None);
        assert_ne!(shown(&character, &frame).0, "spin");
    }
}

#[test]
fn a_behavior_on_a_wall_spends_the_awake_budget_before_landing() {
    let character = character(Some("idle_gesture_every_s = [30, 30]"), 1, 0);
    let mut behaviors = BTreeMap::new();
    behaviors.insert(
        "ponder".to_string(),
        behavior(vec![Primitive::Talk; 5], false),
    );
    let mut engine = Engine::new(Point {
        x: 900.0,
        y: 4_000.0,
    })
    .with_behaviors(behaviors)
    .with_idle_plan(character.idle_plan());
    let wall = WorldSnapshot {
        displays: vec![Rect {
            x: 0.0,
            y: 0.0,
            width: 1_000.0,
            height: 8_000.0,
        }],
        ..world(100)
    };
    engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Grab],
        cursor: Point {
            x: 900.0,
            y: 4_000.0,
        },
        ..wall.clone()
    });
    let climbing = engine.tick(&WorldSnapshot {
        verbs: vec![Verb::Throw {
            velocity: Point { x: 1_000.0, y: 0.0 },
        }],
        ..wall.clone()
    });
    assert_eq!(climbing.state, State::Climbing);
    engine.set_sleep_after_s((2, 2));
    let mut frame = engine.tick(&WorldSnapshot {
        proposal: propose("ponder").proposal,
        ..wall.clone()
    });
    assert_eq!(frame.playing_behavior.as_deref(), Some("ponder"));
    let mut behavior_ticks = 0;
    for _ in 0..40 {
        if frame.playing_behavior.is_none() {
            break;
        }
        assert_eq!(
            frame.state,
            State::Climbing,
            "finishes on the wall: {frame:?}"
        );
        assert_eq!(shown(&character, &frame).0, "talk");
        behavior_ticks += 1;
        frame = engine.tick(&wall);
    }
    assert_eq!(
        behavior_ticks, 30,
        "the full three-second Behavior finishes"
    );
    assert!(frame.playing_behavior.is_none());
    let landing = WorldSnapshot {
        displays: vec![Rect {
            x: 0.0,
            y: frame.position.y - 10.0,
            width: 1_000.0,
            height: 100.0,
        }],
        ..world(100)
    };
    let mut saw_land = false;
    for _ in 0..20 {
        frame = engine.tick(&landing);
        if frame.playing_primitive == Some(Primitive::Land) {
            saw_land = true;
        } else if saw_land {
            assert_eq!(
                frame.state,
                State::Asleep,
                "the wall Behavior already spent the awake budget: {frame:?}"
            );
            return;
        }
    }
    panic!("the sprite never finished landing: {frame:?}");
}

#[test]
fn awake_behavior_time_counts_in_air_but_unrelated_falling_does_not() {
    let character = character(Some("idle_gesture_every_s = [30, 30]"), 1, 0);
    for playing_behavior in [false, true] {
        let mut behaviors = BTreeMap::new();
        behaviors.insert(
            "ponder".to_string(),
            behavior(vec![Primitive::Talk; 5], false),
        );
        let mut engine = Engine::new(Point { x: 500.0, y: 100.0 })
            .with_behaviors(behaviors)
            .with_idle_plan(character.idle_plan())
            .with_sleep_after_s((2, 2));
        let air = WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1_000.0,
                height: 1_000_000.0,
            }],
            proposal: playing_behavior.then(|| propose("ponder").proposal.unwrap()),
            ..world(100)
        };
        let mut frame = engine.tick(&air);
        for tick in 0..30 {
            assert_eq!(frame.state, State::Falling);
            if playing_behavior {
                assert_eq!(frame.playing_behavior.as_deref(), Some("ponder"));
                assert_eq!(shown(&character, &frame).0, "talk");
            } else {
                assert_eq!(frame.playing_behavior, None);
            }
            frame = engine.tick(&WorldSnapshot {
                proposal: None,
                ..air.clone()
            });
            if tick == 29 {
                assert!(frame.playing_behavior.is_none());
            }
        }
        let landing = WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1_000.0,
                height: frame.position.y + 10.0,
            }],
            ..world(100)
        };
        let landed = engine.tick(&landing);
        assert_eq!(landed.playing_primitive, Some(Primitive::Land));
        let rested = engine.tick(&WorldSnapshot {
            elapsed_ms: 600,
            ..landing
        });
        assert_eq!(
            rested.state,
            if playing_behavior {
                State::Asleep
            } else {
                State::Grounded
            },
            "only the awake Behavior spends the air interval: {rested:?}"
        );
    }
}

#[test]
fn sleep_does_not_spend_the_next_awake_stretch() {
    let character = character(Some("idle_gesture_every_s = [30, 30]"), 1, 0);
    let mut engine = engine_for(&character, 1);
    first_idle(&mut engine);
    engine.set_sleep_after_s((3, 3));
    engine.set_sleep_for_s((10, 10));
    assert_eq!(engine.tick(&world(2_900)).state, State::Grounded);
    assert_eq!(engine.tick(&world(100)).state, State::Asleep);
    assert_eq!(engine.tick(&world(9_900)).state, State::Asleep);
    let woken = engine.tick(&world(100));
    assert_eq!(woken.state, State::Grounded);
    assert!(matches!(woken.idle_beat, Some(IdleBeat::IdleLoop(_))));
    assert_eq!(engine.tick(&world(2_900)).state, State::Grounded);
    assert_eq!(engine.tick(&world(100)).state, State::Asleep);
}

#[test]
fn a_deferred_proposal_waits_out_the_whole_gesture_pass() {
    // `spin` is two frames at 1 fps: a two-second pass.
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    let mut behaviors = character.behaviors.clone();
    behaviors.insert("wave".to_string(), behavior(vec![Primitive::Talk], false));
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(behaviors)
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan())
        .with_idle_gesture_every_s((1, 1));
    let _ = first_idle(&mut engine);
    let mut start = None;
    for _ in 0..80 {
        let frame = engine.tick(&world(100));
        if shown(&character, &frame).0 == "spin" {
            start = Some(frame.animation_ms);
            break;
        }
    }
    let start = start.expect("the stretch reached the gesture");
    let held = engine.tick(&propose("wave"));
    assert_eq!(shown(&character, &held).0, "spin", "the proposal is held");

    let mut last = (held.animation_ms, shown(&character, &held).1);
    let mut talked = false;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if frame.animation == "talk" {
            talked = true;
            break;
        }
        let (name, index) = shown(&character, &frame);
        assert_eq!(name, "spin", "nothing else plays before the line");
        last = (frame.animation_ms, index);
    }
    assert!(talked, "the held line plays when the pass ends");
    assert!(
        last.0 + 100 >= 2_000,
        "the pass ran its full two seconds from {start} ms, not cut at {} ms",
        last.0
    );
    assert_eq!(last.1, 1, "the gesture's last frame shows before the line");
}

fn chained(primitives: Vec<Primitive>, idle_type: bool, then: Option<&str>) -> Behavior {
    Behavior {
        then: then.map(str::to_string),
        ..behavior(primitives, idle_type)
    }
}

/// Whether each stretch of Idle Primitives from a proposal plays through the
/// Engine's idle machine, in the order they play.
fn idle_ownership(extra: &[(&str, Behavior)], proposed: &str) -> Vec<bool> {
    let character = character(None, 1, 0);
    let mut behaviors = character.behaviors.clone();
    behaviors.extend(extra.iter().map(|(n, b)| (n.to_string(), b.clone())));
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(behaviors)
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());
    let _ = first_idle(&mut engine);
    let mut frame = engine.tick(&propose(proposed));
    let mut seen = Vec::new();
    for _ in 0..60 {
        if frame.playing_primitive.is_none() {
            break;
        }
        if frame.playing_primitive == Some(Primitive::Idle) {
            let typed = frame.idle_beat.is_some();
            if seen.last() != Some(&typed) {
                seen.push(typed);
            }
        }
        frame = engine.tick(&world(100));
    }
    seen
}

#[test]
fn each_behavior_in_a_then_chain_keeps_its_own_idle_type() {
    let typed_after_ordinary = [
        ("a", chained(vec![Primitive::Talk], false, Some("b"))),
        ("b", chained(vec![Primitive::Idle], true, None)),
    ];
    assert_eq!(idle_ownership(&typed_after_ordinary, "b"), vec![true]);
    assert_eq!(
        idle_ownership(&typed_after_ordinary, "a"),
        vec![true],
        "an idle-typed Behavior reached through then still idles its own way"
    );

    let ordinary_after_typed = [
        ("a", chained(vec![Primitive::Idle], true, Some("b"))),
        ("b", chained(vec![Primitive::Idle], false, None)),
    ];
    assert_eq!(idle_ownership(&ordinary_after_typed, "b"), vec![false]);
    assert_eq!(
        idle_ownership(&ordinary_after_typed, "a"),
        vec![true, false],
        "the first Behavior's type does not leak into the next"
    );

    let longer = [
        ("a", chained(vec![Primitive::Idle], false, Some("b"))),
        ("b", chained(vec![Primitive::Idle], true, Some("c"))),
        ("c", chained(vec![Primitive::Idle], true, Some("d"))),
        ("d", chained(vec![Primitive::Idle], false, None)),
    ];
    assert_eq!(
        idle_ownership(&longer, "a"),
        vec![false, true, false,],
        "a longer chain switches type at each boundary"
    );
}

/// An idle-typed `a` whose Idle starts the Engine's gesture just before it
/// ends, then `next`. With `wave` held mid-gesture or not, returns what showed
/// from the gesture's start: (drawn name, animation ms, behavior) per tick.
fn gesture_across_a_then(next: Behavior, hold_wave: bool) -> Vec<(String, u32, Option<String>)> {
    gesture_across(
        chained(vec![Primitive::Idle], true, Some("b")),
        next,
        hold_wave,
    )
}

/// The same, for any idle-typed `a` and its `b`.
fn gesture_across(
    a: Behavior,
    next: Behavior,
    hold_wave: bool,
) -> Vec<(String, u32, Option<String>)> {
    let character = character(Some("idle_gesture_every_s = [1, 1]"), 1, 0);
    let mut behaviors = character.behaviors.clone();
    behaviors.insert("a".into(), a);
    behaviors.insert("b".into(), next);
    behaviors.insert("wave".into(), behavior(vec![Primitive::Talk], false));
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(behaviors)
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan())
        .with_idle_gesture_every_s((1, 1));
    let _ = first_idle(&mut engine);
    for _ in 0..8 {
        let _ = engine.tick(&world(100));
    }
    let mut frame = engine.tick(&propose("a"));
    assert_eq!(frame.behavior.as_deref(), Some("a"));
    let mut seen = Vec::new();
    let mut held = false;
    for _ in 0..60 {
        let (name, _) = shown(&character, &frame);
        if name == "spin" || !seen.is_empty() {
            seen.push((name.clone(), frame.animation_ms, frame.behavior.clone()));
            if frame.animation == "talk" {
                break;
            }
        }
        let world = if hold_wave && name == "spin" && !held {
            held = true;
            propose("wave")
        } else {
            world(100)
        };
        frame = engine.tick(&world);
    }
    seen
}

fn assert_whole_gesture_then(seen: &[(String, u32, Option<String>)], talker: &str) {
    let spin: Vec<_> = seen.iter().take_while(|s| s.0 == "spin").collect();
    assert!(!spin.is_empty(), "the gesture started inside a: {seen:?}");
    assert!(
        spin.windows(2).all(|w| w[1].1 > w[0].1),
        "one gesture pass, never restarted: {seen:?}"
    );
    assert!(
        spin.last().unwrap().1 + 100 >= 2_000,
        "the gesture ran its whole pass: {seen:?}"
    );
    let after = &seen[spin.len()];
    assert_eq!(after.0, "talk", "then the line: {seen:?}");
    assert_eq!(after.2.as_deref(), Some(talker), "{seen:?}");
}

fn talk_b() -> Behavior {
    chained(vec![Primitive::Talk], false, None)
}

#[test]
fn a_gesture_begun_inside_an_idle_typed_behavior_finishes_before_its_then() {
    let seen = gesture_across_a_then(talk_b(), false);
    let spin = seen.iter().take_while(|s| s.0 == "spin").count();
    assert!(spin > 0 && seen[spin - 1].1 + 100 >= 2_000, "{seen:?}");
    // One tick of the loop at most, while the queue reaches its boundary.
    let spin = spin + usize::from(seen[spin].0 == "idle");
    assert_eq!(seen[spin].0, "talk", "then `b` starts: {seen:?}");
}

#[test]
fn a_proposal_held_inside_an_idle_typed_behavior_outranks_its_then() {
    assert_whole_gesture_then(&gesture_across_a_then(talk_b(), true), "wave");
}

#[test]
fn a_proposal_held_inside_an_idle_typed_behavior_outranks_an_animation_then() {
    let animation_b = Behavior {
        animation: Some("spin".into()),
        ..behavior(vec![], false)
    };
    assert_whole_gesture_then(&gesture_across_a_then(animation_b, true), "wave");
}

#[test]
fn a_gesture_inside_a_multi_primitive_idle_typed_behavior_finishes_before_its_talk() {
    let a = || chained(vec![Primitive::Idle, Primitive::Talk], true, None);
    let unused = || behavior(vec![], false);
    let seen = gesture_across(a(), unused(), false);
    let spin = seen.iter().take_while(|s| s.0 == "spin").count();
    assert!(spin > 0 && seen[spin - 1].1 + 100 >= 2_000, "{seen:?}");
    // One tick of the loop at most, while the queue reaches its boundary.
    let spin = spin + usize::from(seen[spin].0 == "idle");
    assert_eq!(seen[spin].0, "talk", "then a's own Talk: {seen:?}");
    assert_whole_gesture_then(&gesture_across(a(), unused(), true), "wave");
}

#[test]
fn a_gesture_inside_a_flattened_same_type_chain_finishes_before_the_next_primitive() {
    let a = || chained(vec![Primitive::Idle], true, Some("b"));
    let typed_b = || chained(vec![Primitive::Talk], true, None);
    let seen = gesture_across(a(), typed_b(), false);
    let spin = seen.iter().take_while(|s| s.0 == "spin").count();
    assert!(spin > 0 && seen[spin - 1].1 + 100 >= 2_000, "{seen:?}");
    // One tick of the loop at most, while the queue reaches its boundary.
    let spin = spin + usize::from(seen[spin].0 == "idle");
    assert_eq!(seen[spin].0, "talk", "then b's Talk: {seen:?}");
    assert_whole_gesture_then(&gesture_across(a(), typed_b(), true), "wave");
}
