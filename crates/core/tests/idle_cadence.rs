//! The Engine's own idle: a still rest, then one pass of a ring member.
//!
//! Asserts the frame `Character::draw` shows. The beat is how the Engine
//! names that frame; the pixels are the check.

use std::collections::BTreeMap;

use fidget_core::character::{
    load, Behavior, Character, Primitive, CHARACTER_MANIFEST_FILE, REQUIRED_ANIMATIONS,
};
use fidget_core::engine::{BehaviorProposal, Engine, Point, Rect, Verb, WorldSnapshot};

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

/// Base `idle` plus a two-frame `spin` variant. `idle_body` is the rest of
/// the idle table (weight, fps, rest_ms). `spin_fps` is the gesture's cadence.
fn character(idle_body: &str, spin_fps: u32) -> Character {
    let mut manifest = String::from("name = \"Blip\"\n");
    for name in REQUIRED_ANIMATIONS {
        if name == "idle" {
            manifest.push_str(&format!(
                "[animations.idle]\nframes = [\"idle-0.png\"]\n{idle_body}"
            ));
        } else {
            manifest.push_str(&format!(
                "[animations.{name}]\nframes = [\"{name}-0.png\"]\n"
            ));
        }
    }
    manifest.push_str(&format!(
        "[animations.spin]\nframes = [\"idle-0.png\", \"idle-0.png\"]\n\
         fps = {spin_fps}\nvariant_of = \"idle\"\nweight = 1\n"
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
        .with_idle_ring(character.idle_ring())
}

/// `(drawn animation, index)` for one tick, which is the picture on screen.
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

/// Tick until the family is idle, then return that frame. The fall and the
/// landing are not the stretch under test.
fn first_idle(engine: &mut Engine) -> fidget_core::engine::Frame {
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if frame.animation == "idle" {
            return frame;
        }
    }
    panic!("the sprite never idled");
}

fn idle_body(weight: u32, rest: &str) -> String {
    format!("weight = {weight}\nrest_ms = {rest}\n")
}

#[test]
fn an_idle_stretch_rests_before_its_first_gesture() {
    let character = character(&idle_body(0, "[400, 400]"), 1);
    let mut engine = engine_for(&character, 1);
    let frame = first_idle(&mut engine);
    assert_eq!(frame.animation, "idle", "the family stays idle");
    assert_eq!(
        shown(&character, &frame),
        ("idle".to_string(), 0),
        "the stretch opens on base idle frame 0, not the weighted gesture"
    );
}

#[test]
fn a_gesture_plays_one_pass_and_the_stretch_returns_to_rest() {
    let character = character(&idle_body(0, "[400, 400]"), 1);
    let mut engine = engine_for(&character, 1);
    let _ = first_idle(&mut engine);

    let mut spin = Vec::new();
    let mut back = None;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        assert_eq!(frame.animation, "idle", "still the same idle stretch");
        let drawn = shown(&character, &frame);
        if drawn.0 == "spin" {
            assert!(back.is_none(), "a second gesture started: {spin:?}");
            spin.push(drawn.1);
        } else if !spin.is_empty() {
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
    assert_eq!(
        back.expect("the stretch returned to rest"),
        ("idle".to_string(), 0)
    );
}

#[test]
fn one_idle_stretch_changes_the_drawn_member_without_leaving_idle() {
    let character = character(&idle_body(0, "[200, 200]"), 1);
    let mut engine = engine_for(&character, 3);
    let mut drawn_names = Vec::new();
    let mut families = Vec::new();
    let frame = first_idle(&mut engine);
    families.push(frame.animation);
    drawn_names.push(shown(&character, &frame).0);
    for _ in 0..30 {
        let frame = engine.tick(&world(100));
        if frame.animation != "idle" {
            break;
        }
        families.push(frame.animation);
        drawn_names.push(shown(&character, &frame).0);
        if drawn_names.iter().any(|name| name == "spin")
            && drawn_names.last().is_some_and(|name| name == "idle")
            && drawn_names.iter().filter(|name| *name == "idle").count() > 1
        {
            break;
        }
    }

    assert!(families.iter().all(|name| *name == "idle"));
    assert!(
        drawn_names.windows(2).any(|pair| pair == ["idle", "spin"]),
        "rest gave way to a gesture inside the stretch: {drawn_names:?}"
    );
    assert!(
        drawn_names.windows(2).any(|pair| pair == ["spin", "idle"]),
        "the gesture gave way to rest inside the stretch: {drawn_names:?}"
    );
}

/// Milliseconds from the start of idle until the first gesture frame, counted
/// on the same 100ms ticks the stretch is played with.
fn rest_until_gesture(character: &Character, seed: u64) -> u32 {
    let mut engine = engine_for(character, seed);
    let mut held_ms = 0;
    for _ in 0..80 {
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
fn a_fixed_rest_holds_the_base_for_that_long() {
    let character = character(&idle_body(0, "[400, 400]"), 1);
    assert_eq!(
        rest_until_gesture(&character, 1),
        400,
        "rest_ms = [400, 400] is 400ms of base idle before the gesture"
    );
}

#[test]
fn a_rest_stays_inside_the_declared_range() {
    let character = character(&idle_body(0, "[300, 900]"), 1);
    for seed in [1_u64, 2, 9, 40] {
        let held_ms = rest_until_gesture(&character, seed);
        assert!(
            (300..=900).contains(&held_ms),
            "seed {seed} rested {held_ms}ms, outside [300, 900]"
        );
    }
}

#[test]
fn a_behaviors_idle_loops_the_member_it_drew() {
    let character = character(&idle_body(0, "[400, 400]"), 8);
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(BTreeMap::from([(
            "ponder".to_string(),
            Behavior {
                primitives: vec![Primitive::Idle],
                then: None,
                weight: 10,
                trigger: None,
                not_in_manifest: false,
            },
        )]))
        .with_variant_seed(1)
        .with_idle_ring(character.idle_ring());
    let _ = first_idle(&mut engine);

    engine.tick(&WorldSnapshot {
        proposal: Some(BehaviorProposal {
            behavior: "ponder".into(),
            dialogue: None,
            direction: None,
        }),
        ..world(100)
    });

    let mut indices = Vec::new();
    for _ in 0..12 {
        let frame = engine.tick(&world(100));
        if frame.playing_primitive != Some(Primitive::Idle) {
            break;
        }
        let drawn = shown(&character, &frame);
        assert_eq!(drawn.0, "spin", "the behavior keeps the member it drew");
        indices.push(drawn.1);
    }

    let wrapped = indices.windows(3).any(|triple| triple == [0, 1, 0]);
    assert!(wrapped, "the behavior's idle looped the strip: {indices:?}");
}

fn interrupt_then_rest(interrupt: WorldSnapshot) {
    let character = character(&idle_body(0, "[400, 400]"), 1);
    let mut engine = engine_for(&character, 1);
    let _ = first_idle(&mut engine);
    for _ in 0..20 {
        let frame = engine.tick(&world(100));
        if shown(&character, &frame).0 == "spin" {
            break;
        }
    }
    engine.tick(&interrupt);
    let mut idle_again = Vec::new();
    for _ in 0..20 {
        let frame = engine.tick(&world(100));
        if frame.animation != "idle" {
            continue;
        }
        idle_again.push(shown(&character, &frame));
        if idle_again.len() == 3 {
            break;
        }
    }
    assert_eq!(
        idle_again,
        vec![
            ("idle".to_string(), 0),
            ("idle".to_string(), 0),
            ("idle".to_string(), 0),
        ],
        "the new stretch opens on rest, not the gesture it left"
    );
}

#[test]
fn a_behaviors_idle_starts_at_the_first_frame_even_mid_gesture() {
    let character = character(&idle_body(0, "[400, 400]"), 1);
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(BTreeMap::from([(
            "ponder".to_string(),
            Behavior {
                primitives: vec![Primitive::Idle],
                then: None,
                weight: 10,
                trigger: None,
                not_in_manifest: false,
            },
        )]))
        .with_variant_seed(1)
        .with_idle_ring(character.idle_ring());
    let _ = first_idle(&mut engine);
    let mut saw_second = false;
    for _ in 0..40 {
        let frame = engine.tick(&world(100));
        if shown(&character, &frame) == ("spin".to_string(), 1) {
            saw_second = true;
            break;
        }
    }
    assert!(saw_second, "the gesture reached its second frame");

    let started = engine.tick(&WorldSnapshot {
        proposal: Some(BehaviorProposal {
            behavior: "ponder".into(),
            dialogue: None,
            direction: None,
        }),
        ..world(100)
    });
    assert_eq!(started.playing_primitive, Some(Primitive::Idle));
    assert_eq!(
        shown(&character, &started),
        ("spin".to_string(), 0),
        "the Behavior's member starts at its first frame, not mid-gesture"
    );
}

#[test]
fn a_poke_returns_the_next_idle_to_rest() {
    interrupt_then_rest(WorldSnapshot {
        verbs: vec![Verb::Poke],
        ..world(100)
    });
}

#[test]
fn a_talk_returns_the_next_idle_to_rest() {
    let character = character(&idle_body(0, "[400, 400]"), 1);
    let mut engine = engine_for(&character, 1);
    let _ = first_idle(&mut engine);
    for _ in 0..20 {
        let frame = engine.tick(&world(100));
        if shown(&character, &frame).0 == "spin" {
            break;
        }
    }
    for _ in 0..3 {
        let frame = engine.tick(&WorldSnapshot {
            locomotion_frozen: true,
            ..world(100)
        });
        assert_eq!(frame.animation, "talk");
    }
    let mut idle_again = Vec::new();
    for _ in 0..10 {
        let frame = engine.tick(&world(100));
        if frame.animation != "idle" {
            continue;
        }
        idle_again.push(shown(&character, &frame));
        if idle_again.len() == 3 {
            break;
        }
    }
    assert_eq!(
        idle_again,
        vec![
            ("idle".to_string(), 0),
            ("idle".to_string(), 0),
            ("idle".to_string(), 0),
        ],
        "after the line, idle starts on rest"
    );
}

#[test]
fn the_same_seed_rests_and_gestures_alike() {
    let body = idle_body(1, "[200, 2000]");
    let beats = |seed: u64| {
        let character = character(&body, 1);
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
fn a_reversed_rest_range_is_rejected_by_name() {
    let errors = load(&package(&manifest_naming("idle", "rest_ms = [900, 300]\n")))
        .expect_err("reversed range");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("rest_ms") && error.contains("idle")),
        "{errors:?}"
    );
}

#[test]
fn rest_ms_on_a_walk_is_rejected_by_name() {
    let errors = load(&package(&manifest_naming("walk", "rest_ms = [100, 200]\n")))
        .expect_err("rest_ms is idle only");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("rest_ms") && error.contains("walk")),
        "{errors:?}"
    );
}
