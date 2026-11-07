//! Integration tests for shipped Character Packages.
//!
//! These tests validate at the workspace layout seam: that the on-disk
//! character packages in `characters/` can be loaded by the core crate's
//! `character::load` parser and meet the contracts the engine depends on.
//!
//! The seam matters because the parser and the packages are separate: one
//! change can break the other. A broken shipped package is a shipping defect,
//! not a unit test failure. These tests catch that before release.
//!
//! Character-specific tests cover constraints unique to that character. Every
//! shipped character's load and required animations are covered by the shell
//! crate's integration test.

use fidget_core::character::{
    self, CursorReaction, IdleBeat, Primitive, RingChoice, REQUIRED_ANIMATIONS,
};
use fidget_core::engine::{BehaviorProposal, Engine, Point, Rect, Window, WorldSnapshot};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;

/// Read a Character Package directory in the workspace root into the same
/// `PackageBytes` map `character::load` takes.
fn package_bytes(name: &str) -> BTreeMap<String, Vec<u8>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core is in crates/")
        .parent()
        .expect("crates/ is in workspace");
    let package_dir = root.join("characters").join(name);

    let mut files = BTreeMap::new();
    collect(&package_dir, &package_dir, &mut files)
        .unwrap_or_else(|e| panic!("{}: {}", package_dir.display(), e));
    files
}

fn load_package(name: &str) -> Result<character::Character, Vec<String>> {
    character::load(&package_bytes(name))
}

/// Walk a package directory into the path→bytes map `character::load` takes.
fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, files)?;
        } else {
            let name = path
                .strip_prefix(root)
                .expect("every walked path starts at the root")
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.insert(name, std::fs::read(&path)?);
        }
    }
    Ok(())
}

fn assert_required_animations(character: &character::Character) {
    for required in REQUIRED_ANIMATIONS {
        assert!(
            character.animations.contains_key(required),
            "{} missing required animation: {required:?}",
            character.name
        );
    }
}

/// Every Character Package directory in the workspace root.
fn shipped_packages() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core is in crates/")
        .parent()
        .expect("crates/ is in workspace")
        .join("characters");

    let mut names: Vec<String> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .map(|entry| entry.expect("characters/ is readable").path())
        .filter(|path| path.join("character.manifest").is_file())
        .map(|path| {
            path.file_name()
                .expect("a package directory has a name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "characters/ holds no Character Package");
    names
}

/// `[source]` is optional to the loader and required of anything shipped here.
/// The gallery publishes a package's attribution, so one of ours that declares
/// nothing publishes art with no word on where it came from. Nothing else enforces this.
#[test]
fn every_shipped_package_declares_where_its_art_came_from() {
    for name in shipped_packages() {
        let character = load_package(&name).unwrap_or_else(|e| panic!("{name}: {e:#?}"));
        assert!(
            character.source.is_some(),
            "{name} declares no [source]; the gallery would publish its art with no \
             word on where it came from or what covers it"
        );
    }
}

/// Entry test: package parses, has personality, and walk is the shipped 20-frame loop.
#[test]
fn timber_wolf_package_loads_with_all_required_animations() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    assert_eq!(character.name, "Timber Wolf");
    assert!(
        !character.personality.is_empty(),
        "Timber Wolf has a personality prompt"
    );

    assert_required_animations(&character);

    let walk = &character.animations["walk"];
    assert_eq!(
        walk.frames.len(),
        20,
        "walk must be the shipped 20-frame two-step loop"
    );
}

#[test]
fn timber_wolf_behaviors_compose_existing_primitives() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    assert!(
        !character.behaviors.is_empty(),
        "Timber Wolf declares behaviors"
    );

    assert!(
        character.behaviors.contains_key("patrol"),
        "patrol behavior exists"
    );
    assert!(
        character.behaviors.contains_key("engage"),
        "engage behavior exists for weapon-raise emote"
    );

    let engage = &character.behaviors["engage"];
    assert!(
        engage.primitives.len() >= 2,
        "engage composes multiple primitives"
    );
    assert!(
        engage.plays(character::Primitive::React),
        "engage includes react (weapon raise)"
    );

    assert!(
        character.behaviors.contains_key("pursue"),
        "pursue behavior exists for walk-toward-cursor"
    );
    let pursue = &character.behaviors["pursue"];
    assert!(
        pursue.plays(character::Primitive::Chase),
        "pursue includes Chase primitive for walk-toward-cursor movement"
    );
}

#[test]
fn timber_wolf_uses_scale_1_for_captured_frames() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    assert_eq!(
        character.scale, 1,
        "Timber Wolf uses scale 1: frames are ~148px captures on a 176x160 canvas like Jotaro, not tiny pixel art"
    );
}

/// A pixel the eye can see, rather than the feathered edge of the matte.
const VISIBLE: u8 = 32;

/// How many visible pixels a row needs before it counts as a foot on the
/// floor, so a stray speck left by the matte cannot ground a frame.
const FOOT_PIXELS: usize = 3;

/// The canvas size and alpha channel of one frame PNG, row-major.
fn frame_alpha(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
    let (width, height, pixels) = frame_rgba(bytes);
    (width, height, pixels.iter().map(|p| p[3]).collect())
}

/// The first and last rows carrying visible pixels, and how many the last row
/// carries — the sprite's silhouette reduced to where it starts and stands.
fn silhouette(width: usize, height: usize, alpha: &[u8]) -> (usize, usize, usize) {
    let mut top = None;
    let mut bottom = 0;
    let mut bottom_pixels = 0;

    for row in 0..height {
        let visible = alpha[row * width..(row + 1) * width]
            .iter()
            .filter(|&&a| a >= VISIBLE)
            .count();
        if visible > 0 {
            top = top.or(Some(row));
            bottom = row;
            bottom_pixels = visible;
        }
    }

    (top.expect("frame is not blank"), bottom, bottom_pixels)
}

/// Every frame of a package, in `(animation, frame name, bytes)` order.
fn frames_of(name: &str) -> Vec<(String, String, Vec<u8>)> {
    let character = load_package(name).unwrap_or_else(|e| panic!("{name} is valid: {e:?}"));
    let files = package_bytes(name);

    let mut frames = Vec::new();
    for (animation, declared) in &character.animations {
        for frame in &declared.frames {
            let bytes = files
                .get(frame)
                .unwrap_or_else(|| panic!("{animation} declares {frame}, which the package holds"))
                .clone();
            frames.push((animation.clone(), frame.clone(), bytes));
        }
    }
    frames
}

/// Mean x of cyan cheek-light pixels in an RGBA PNG (Buddy Bot status LED).
fn cyan_cheek_light_x(bytes: &[u8]) -> f32 {
    let (width, _, rgba) = frame_rgba(bytes);
    let mut sx = 0.0f32;
    let mut n = 0.0f32;
    for (i, px) in rgba.iter().enumerate() {
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        if a > 128 && b > 150 && r < 120 && g > 80 {
            sx += (i % width) as f32;
            n += 1.0;
        }
    }
    assert!(n > 0.0, "expected cyan status-light pixels");
    sx / n
}

/// Mean squared RGB error between a frame and its horizontal mirror.
/// Directional art scores high; a left-right symmetric sprite would score near 0.
fn mse_to_horizontal_flip(width: usize, height: usize, rgba: &[[u8; 4]]) -> f64 {
    let mut sum = 0.0f64;
    let mut n = 0.0f64;
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let j = y * width + (width - 1 - x);
            if rgba[i][3] == 0 && rgba[j][3] == 0 {
                continue;
            }
            for (left, right) in rgba[i][..3].iter().zip(rgba[j][..3].iter()) {
                let d = *left as f64 - *right as f64;
                sum += d * d;
                n += 1.0;
            }
        }
    }
    assert!(n > 0.0, "frame has visible pixels to compare");
    sum / n
}

/// 3/4-right walk: the near ear sits on the viewer's right and extends the
/// opaque bbox, so dark facial features (eyes) land left of silhouette mid.
/// Horizontally flipping the frame reverses the inequality — that is the
/// regression against re-flipping walk art the engine already mirrors.
fn dark_eyes_left_of_silhouette_mid(width: usize, height: usize, rgba: &[[u8; 4]]) -> (f32, f32) {
    let mut x_min = width;
    let mut x_max = 0usize;
    for (i, px) in rgba.iter().enumerate() {
        if px[3] > 128 {
            let x = i % width;
            x_min = x_min.min(x);
            x_max = x_max.max(x);
        }
    }
    assert!(x_min <= x_max, "walk frame has an opaque silhouette");
    let sil_mid = (x_min + x_max) as f32 / 2.0;

    let y0 = height * 30 / 108;
    let y1 = height * 55 / 108;
    let mut sx = 0.0f32;
    let mut n = 0.0f32;
    for (i, px) in rgba.iter().enumerate() {
        let y = i / width;
        if y < y0 || y > y1 {
            continue;
        }
        let (r, g, b, a) = (px[0] as u16, px[1] as u16, px[2] as u16, px[3]);
        let lum = (r + g + b) / 3;
        if a > 128 && lum < 50 {
            sx += (i % width) as f32;
            n += 1.0;
        }
    }
    assert!(
        n > 10.0,
        "expected dark eye pixels in the head band, got {n}"
    );
    (sx / n, sil_mid)
}

// Clearance belongs to the art: Buddy Bot hovers, while Duck has planted feet.
// Fall/grab animations can reserve space below curled feet independently of standing.
#[test]
fn shipped_frames_keep_their_authored_floor_clearance() {
    let profiles = BTreeMap::from([
        ("black-mage", (0, 0..=0)),
        ("bmo", (1, 0..=2)),
        ("buddy-bot", (20, 0..=25)),
        ("cat", (9, 0..=3)),
        ("jotaro-kujo", (2, 0..=2)),
        ("nim", (1, 0..=1)),
        ("rubber-duck", (0, 0..=0)),
        ("timber-wolf", (0, 1..=20)),
        ("trump", (8, 0..=6)),
        ("tux", (5, 0..=1)),
        ("tuxedo-bean", (6, 0..=0)),
    ]);
    for package in shipped_packages() {
        let (grounded_max, airborne) = profiles
            .get(package.as_str())
            .unwrap_or_else(|| panic!("{package} needs an authored floor-clearance profile"));
        for (animation, frame, bytes) in frames_of(&package) {
            let (width, height, alpha) = frame_alpha(&bytes);
            let bottom = alpha
                .chunks_exact(width)
                .rposition(|row| {
                    row.iter()
                        .filter(|&&a| a >= character::ALPHA_THRESHOLD)
                        .count()
                        >= FOOT_PIXELS
                })
                .unwrap_or_else(|| panic!("{package}/{frame} has no solid foot contact"));
            let gap = height - 1 - bottom;
            let permitted = if matches!(animation.as_str(), "fall" | "grab") {
                airborne.clone()
            } else {
                0..=*grounded_max
            };
            assert!(permitted.contains(&gap),
                "{package}/{frame} ({animation}) has {gap}px floor clearance; expected {permitted:?}");
        }
    }
}

#[test]
fn rubber_duck_keeps_its_smaller_standing_silhouette() {
    let character = load_package("rubber-duck").expect("Rubber Duck package is valid");
    let files = package_bytes("rubber-duck");
    let tallest = character.animations["idle"]
        .frames
        .iter()
        .map(|frame| {
            let (width, height, alpha) = frame_alpha(&files[frame]);
            let rows: Vec<_> = (0..height)
                .filter(|&y| {
                    alpha[y * width..(y + 1) * width]
                        .iter()
                        .any(|&a| a >= character::ALPHA_THRESHOLD)
                })
                .collect();
            (rows.last().unwrap() - rows[0] + 1) * character.scale as usize
        })
        .max()
        .unwrap();
    assert!(
        (102..=108).contains(&tallest),
        "the standing silhouette must be 10–15% below 120px, got {tallest}px"
    );
}

#[test]
fn rubber_duck_jumps_above_the_floor_with_its_grounded_frames() {
    let character = load_package("rubber-duck").expect("Rubber Duck package is valid");
    let world = WorldSnapshot {
        displays: vec![Rect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        }],
        elapsed_ms: 100,
        ..WorldSnapshot::default()
    };
    let mut engine =
        Engine::new(Point { x: 100.0, y: 0.0 }).with_behaviors(character.behaviors.clone());
    for _ in 0..40 {
        engine.tick(&world);
    }
    let floor = engine.tick(&world).position.y;
    engine.tick(&WorldSnapshot {
        proposal: Some(BehaviorProposal {
            behavior: "jump".into(),
            dialogue: None,
            direction: None,
        }),
        ..world.clone()
    });

    let mut peak = floor;
    let mut landed = None;
    for _ in 0..40 {
        let frame = engine.tick(&world);
        peak = peak.min(frame.position.y);
        character
            .draw(
                frame.animation,
                frame.animation_ms,
                frame.ring_choice(),
                frame.facing,
            )
            .expect("the actual duck package draws every jump phase");
        if peak < floor && frame.position.y == floor {
            landed = Some(frame);
            break;
        }
    }
    assert!(
        floor - peak > 50.0,
        "the duck clears the floor: {floor} -> {peak}"
    );
    assert_eq!(landed.expect("the duck lands").animation, "land");
}

/// The beat names an idle-gesture, so draw plays that animation from frame 0.
/// A weighed idle draw stays on the calm loop.
fn assert_idle_gesture(character: &character::Character, name: &str) {
    let idle = &character.animations["idle"];
    assert!(
        !idle.variants.iter().any(|variant| variant == name),
        "{name} is outside the calm loop"
    );
    assert_eq!(
        character.animations[name].variant_of.as_deref(),
        Some("idle-gesture"),
        "{name} keeps its name and is classified idle-gesture"
    );
    let animation = &character.animations[name];
    let beat = IdleBeat::IdleGesture(name.to_string());
    for index in 0..animation.frames.len() {
        let elapsed = (index as u32 * 1000).div_ceil(animation.fps);
        let drawn = character
            .draw("idle", elapsed, RingChoice::Beat(&beat), 1.0)
            .unwrap_or_else(|| panic!("{name} draws"));
        assert_eq!(drawn.animation, name);
        assert_eq!(drawn.index, index, "{name} plays from frame 0");
    }
    let weighed = character.draw("idle", 0, 0, 1.0).expect("idle draws");
    assert_eq!(
        weighed.animation, "idle",
        "a weighed idle stays on the loop"
    );
}

#[test]
fn rubber_duck_plays_thinking_and_looking_as_idle_gestures() {
    let character = load_package("rubber-duck").expect("Rubber Duck package is valid");
    assert_eq!(
        character.art.len(),
        74,
        "all drawn source cells are available"
    );
    assert_eq!(character.animations["idle"].frames.len(), 7);

    for (name, source_frames) in [
        ("wave", 6),
        ("thinking", 6),
        ("pondering", 6),
        ("look-right", 8),
        ("look-left", 8),
    ] {
        assert_eq!(
            character.animations[name]
                .frames
                .iter()
                .collect::<BTreeSet<_>>()
                .len(),
            source_frames
        );
        assert_idle_gesture(&character, name);
    }
}

/// Every listed idle-gesture plays from frame 0, and the left walk draws its
/// own poses rather than mirroring.
fn assert_idle_gestures_and_drawn_left_walk(package: &str, art: usize, gestures: &[(&str, usize)]) {
    let character = load_package(package).unwrap_or_else(|e| panic!("{package} is valid: {e:?}"));
    assert_eq!(character.art.len(), art, "all source rows are retained");
    for &(name, count) in gestures {
        assert_eq!(character.animations[name].frames.len(), count);
        assert_idle_gesture(&character, name);
    }
    let left = character.draw("walk", 0, 0, -1.0).expect("left walk draws");
    assert_eq!(left.animation, "walk-left");
    assert!(!left.mirrored, "{package} uses the drawn left-facing poses");
}

#[test]
fn tux_draws_each_idle_gesture_and_its_left_waddle() {
    assert_idle_gestures_and_drawn_left_walk(
        "tux",
        57,
        &[
            ("restless-sitting", 6),
            ("look-around", 6),
            ("checking-work", 6),
            ("recover-card", 8),
            ("waddle-in-place", 6),
        ],
    );
}

#[test]
fn tuxedo_bean_draws_each_idle_gesture_and_its_left_walk() {
    assert_idle_gestures_and_drawn_left_walk(
        "tuxedo-bean",
        57,
        &[
            ("look-around", 6),
            ("nod-off", 8),
            ("review", 6),
            ("wave", 4),
        ],
    );
}

/// Every row of the `tuxedo-bean` sheet reaches the screen without a model:
/// as an idle-gesture, or through a Behavior the Static Director can pick.
/// Row 2, the left walk, is checked by the test above.
#[test]
fn tuxedo_bean_plays_every_sheet_row_without_a_model() {
    let character = load_package("tuxedo-bean").expect("Tuxedo Bean package is valid");
    for (row, name) in [
        (0, "look-around"),
        (3, "wave"),
        (5, "nod-off"),
        (8, "review"),
    ] {
        assert!(
            character.animations.get(name).is_some_and(|animation| {
                animation.variant_of.as_deref() == Some("idle-gesture")
            }),
            "row {row} ({name}) keeps its name inside the idle-gesture family"
        );
    }
    for (row, primitive) in [
        (1, Primitive::Walk),
        (4, Primitive::Jump),
        (6, Primitive::Idle),
        (7, Primitive::Talk),
    ] {
        assert!(
            character
                .behaviors
                .values()
                .any(|behavior| behavior.weight > 0 && behavior.plays(primitive)),
            "row {row} ({primitive:?}) needs a Behavior the Static Director can pick"
        );
    }
}

/// How far right of the cap's middle Jotaro's gold badge sits, in pixels. The
/// badge is on the cap's front, so it sits right of middle when he faces right.
fn jotaro_badge_offset(bytes: &[u8]) -> f32 {
    let (width, height, rgba) = frame_rgba(bytes);
    let alpha: Vec<u8> = rgba.iter().map(|p| p[3]).collect();
    let (top, bottom, _) = silhouette(width, height, &alpha);
    let cap = top..top + (bottom - top) * 3 / 10;
    let (mut x_min, mut x_max, mut badge, mut n) = (width, 0, 0.0f32, 0.0f32);
    for row in cap {
        for x in 0..width {
            let [r, g, b, a] = rgba[row * width + x];
            if a > 128 {
                (x_min, x_max) = (x_min.min(x), x_max.max(x));
                if r > 170 && g > 120 && b < 90 {
                    badge += x as f32;
                    n += 1.0;
                }
            }
        }
    }
    assert!(n > 20.0, "the cap badge shows");
    badge / n - (x_min + x_max) as f32 / 2.0
}

/// How far left of BMO's screen its dark side panel sits, in pixels. Facing
/// right, the lettered side panel shows on the left of the screen.
fn bmo_panel_offset(bytes: &[u8]) -> f32 {
    let (width, _, rgba) = frame_rgba(bytes);
    let centroid = |color: [u8; 3]| {
        let (mut sum, mut n) = (0.0f32, 0.0f32);
        for (i, [r, g, b, a]) in rgba.iter().enumerate() {
            let near = [*r, *g, *b]
                .iter()
                .zip(color)
                .all(|(p, c)| p.abs_diff(c) <= 18);
            if *a > 128 && near {
                sum += (i % width) as f32;
                n += 1.0;
            }
        }
        assert!(n > 200.0, "BMO's {color:?} shows");
        sum / n
    };
    centroid([222, 255, 239]) - centroid([55, 103, 91])
}

/// Every frame of `names` as drawn for both ways of travel faces that way,
/// left animation included. `faces_right` reads the art: positive past `margin`
/// when the PNG faces right, negative past it when it faces left.
fn assert_drawn_facing_the_travel(
    package: &str,
    names: &[&str],
    faces_right: fn(&[u8]) -> f32,
    margin: f32,
) {
    let character = load_package(package).unwrap_or_else(|e| panic!("{package}: {e:?}"));
    let files = package_bytes(package);
    for &name in names {
        let animation = &character.animations[name];
        // A variant ring's base draws only on some draws; find one.
        let base = (0..)
            .find(|&seed| character.draw(name, 0, seed, 1.0).expect("draws").animation == name)
            .expect("the base draws");
        for index in 0..animation.frames.len() {
            let at = ((index as u32 * 2 + 1) * 500) / animation.fps;
            for travel in [1.0, -1.0] {
                let drawn = character.draw(name, at, base, travel).expect("draws");
                assert_eq!(drawn.index, index, "{package} {name} reaches frame {index}");
                let frame = &character.animations[drawn.animation].frames[index];
                let offset = faces_right(&files[frame]);
                let shown = if drawn.mirrored { -offset } else { offset };
                assert!(
                    shown * travel as f32 > margin,
                    "{package} {frame} drawn for travel {travel} faces it, offset {shown}"
                );
            }
        }
    }
}

/// How far right of the duck's middle its orange beak sits, in pixels. The
/// feet are orange too, so only the top six tenths are read.
fn duck_beak_offset(bytes: &[u8]) -> f32 {
    let (width, height, rgba) = frame_rgba(bytes);
    let alpha: Vec<u8> = rgba.iter().map(|p| p[3]).collect();
    let (top, bottom, _) = silhouette(width, height, &alpha);
    let (mut x_min, mut x_max, mut beak, mut n) = (width, 0, 0.0f32, 0.0f32);
    for row in top..top + (bottom - top) * 6 / 10 {
        for x in 0..width {
            let [r, g, b, a] = rgba[row * width + x];
            if a > 128 {
                (x_min, x_max) = (x_min.min(x), x_max.max(x));
                if r > 200 && (91..180).contains(&g) && b < 90 {
                    beak += x as f32;
                    n += 1.0;
                }
            }
        }
    }
    assert!(n > 10.0, "the beak shows");
    beak / n - (x_min + x_max) as f32 / 2.0
}

/// How far left of the mech's middle Timber Wolf's missile racks sit, in
/// pixels: the top of the silhouette is the racks, mounted behind the cockpit.
fn timber_wolf_rack_offset(bytes: &[u8]) -> f32 {
    let (width, height, rgba) = frame_rgba(bytes);
    let alpha: Vec<u8> = rgba.iter().map(|p| p[3]).collect();
    let (top, bottom, _) = silhouette(width, height, &alpha);
    let (mut x_min, mut x_max) = (width, 0);
    let (mut racks, mut n) = (0.0f32, 0.0f32);
    for row in 0..height {
        for x in 0..width {
            if alpha[row * width + x] > 128 {
                (x_min, x_max) = (x_min.min(x), x_max.max(x));
                if row < top + (bottom - top) * 15 / 100 {
                    racks += x as f32;
                    n += 1.0;
                }
            }
        }
    }
    (x_min + x_max) as f32 / 2.0 - racks / n
}

#[test]
fn jotaro_idle_draws_at_the_same_standing_scale_as_his_walk() {
    let character = load_package("jotaro-kujo").expect("Jotaro package is valid");
    let idle = &character.animations["idle"];
    for index in 0..idle.frames.len() {
        let at = (index as u32 * 1000).div_ceil(idle.fps);
        let drawn = character.draw("idle", at, 0, 1.0).expect("idle draws");
        assert_eq!(drawn.index, index);
        let (width, height, pixels) = drawn.mask.raw();
        let mut rows = pixels.chunks_exact(width as usize);
        let top = rows
            .clone()
            .position(|row| row.contains(&true))
            .expect("idle is visible");
        let bottom = rows
            .clone()
            .rposition(|row| row.contains(&true))
            .expect("idle is visible");
        let feet = rows
            .rposition(|row| row.iter().filter(|&&solid| solid).count() >= FOOT_PIXELS)
            .expect("idle has solid feet") as i32;
        let shown_height = (bottom - top + 1) as i32 * character.scale as i32;
        assert!(
            (107..=111).contains(&shown_height),
            "idle frame {index} stands {shown_height}px tall; Jotaro's upright walk is 108-109px"
        );
        assert!(
            height - 1 - feet <= 2,
            "idle frame {index} must keep its feet within 2px of the canvas floor"
        );
    }
}

/// Jump draws `fall`, and Jotaro's fall frames face left while his walk faces
/// right. The manifest says so, and the renderer mirrors each to the travel.
#[test]
fn jotaro_falls_facing_the_way_he_walks() {
    assert_drawn_facing_the_travel("jotaro-kujo", &["walk", "fall"], jotaro_badge_offset, 2.0);
}

/// BMO's fall frames are drawn turned the other way from its walk.
#[test]
fn bmo_falls_facing_the_way_it_walks() {
    assert_drawn_facing_the_travel("bmo", &["walk", "fall"], bmo_panel_offset, 2.0);
}

/// The duck rests facing left and walks facing right, with a drawn left walk.
/// `look-left` and `look-right` turn on purpose and are left out.
#[test]
fn rubber_duck_faces_the_way_it_travels() {
    let resting = [
        "idle",
        "wave",
        "thinking",
        "pondering",
        "talk",
        "jump",
        "sleep",
    ];
    let names: Vec<&str> = resting.iter().copied().chain(["walk"]).collect();
    assert_drawn_facing_the_travel("rubber-duck", &names, duck_beak_offset, 2.0);
}

/// Timber Wolf sleeps with its cockpit pointing left and walks to the right.
/// Some walk frames sit near upright, hence the small margin.
#[test]
fn timber_wolf_sleeps_facing_the_way_it_walks() {
    assert_drawn_facing_the_travel(
        "timber-wolf",
        &["walk", "sleep"],
        timber_wolf_rack_offset,
        0.5,
    );
}

#[test]
fn rubber_duck_uses_its_drawn_left_walk() {
    let character = load_package("rubber-duck").expect("Rubber Duck package is valid");
    let left = character.draw("walk", 0, 0, -1.0).expect("walk draws");
    assert_eq!(left.animation, "walk-left");
    assert!(!left.mirrored);
    assert_eq!(character.animations["walk-left"].frames.len(), 8);
}

#[test]
fn rubber_duck_dozes_most_of_the_way_through_its_sleep_cycle() {
    let character = load_package("rubber-duck").expect("Rubber Duck package is valid");
    let sleep = &character.animations["sleep"];
    assert!(sleep.looping);
    assert_eq!(sleep.frames.iter().collect::<BTreeSet<_>>().len(), 8);
    let closed = sleep
        .frames
        .iter()
        .filter(|frame| matches!(frame.as_str(), "frames/doze-3.png" | "frames/doze-4.png"))
        .count();
    assert!(
        closed * 5 >= sleep.frames.len() * 4,
        "at least 80% of sleep stays on the closed-eye poses"
    );
}

/// One canvas for the package, silhouette heavy beside Jotaro's 110px
/// without dwarfing the desktop.
#[test]
fn timber_wolf_frames_share_one_canvas_at_a_desktop_scale() {
    let frames = frames_of("timber-wolf");
    let mut canvas = None;
    let mut tallest = 0;

    for (animation, frame, bytes) in &frames {
        let (width, height, alpha) = frame_alpha(bytes);
        let size = (width, height);
        match canvas {
            None => canvas = Some(size),
            Some(first) => assert_eq!(
                size, first,
                "{animation} frame {frame} is {width}x{height}, and the package's other \
                 frames are {}x{}",
                first.0, first.1
            ),
        }

        let (top, bottom, _) = silhouette(width, height, &alpha);
        tallest = tallest.max(bottom - top + 1);
    }

    assert!(
        (120..=152).contains(&tallest),
        "the tallest Timber Wolf pose is {tallest}px, and a 75-ton mech reads between \
         120px (taller than Jotaro's 110px) and 152px (short of dwarfing the desktop)"
    );
}

/// Decode RGBA8 pixels from a frame PNG.
fn frame_rgba(bytes: &[u8]) -> (usize, usize, Vec<[u8; 4]>) {
    let mut reader = png::Decoder::new(Cursor::new(bytes))
        .read_info()
        .expect("every frame is a PNG");
    let info = reader.info();
    let (width, height) = (info.width as usize, info.height as usize);
    assert_eq!(info.color_type, png::ColorType::Rgba, "frames carry alpha");
    assert_eq!(
        info.bit_depth,
        png::BitDepth::Eight,
        "frames are 8 bits a channel"
    );
    let mut buf = vec![0; reader.output_buffer_size().expect("frame fits in memory")];
    let frame = reader.next_frame(&mut buf).expect("frame decodes");
    let pixels = buf[..frame.buffer_size()]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect();
    (width, height, pixels)
}

/// Mean absolute RGB distance over pixels visible in either frame.
/// Near-duplicate frontals from the turn clip score ~1-3; distinct poses score >> 15.
fn mean_rgb_distance(a: &[[u8; 4]], b: &[[u8; 4]]) -> f64 {
    assert_eq!(a.len(), b.len(), "frames share one canvas");
    let mut sum = 0.0;
    let mut n = 0.0;
    for (pa, pb) in a.iter().zip(b.iter()) {
        let va = pa[3] >= VISIBLE;
        let vb = pb[3] >= VISIBLE;
        if !(va || vb) {
            continue;
        }
        let (ra, ga, ba) = if va {
            (pa[0] as f64, pa[1] as f64, pa[2] as f64)
        } else {
            (0.0, 0.0, 0.0)
        };
        let (rb, gb, bb) = if vb {
            (pb[0] as f64, pb[1] as f64, pb[2] as f64)
        } else {
            (0.0, 0.0, 0.0)
        };
        sum += ((ra - rb).abs() + (ga - gb).abs() + (ba - bb).abs()) / 3.0;
        n += 1.0;
    }
    if n == 0.0 {
        0.0
    } else {
        sum / n
    }
}

/// Idle, land, sit, sleep, hold, react and talk must each be their own
/// art — not byte-identical copies, and not near-duplicate frontals (mean RGB
/// ~1-3) that look like one shared stand. Sit must also read as a hunker:
/// clearly shorter silhouette than idle.
#[test]
fn timber_wolf_poses_are_not_copies_of_each_other() {
    let posed = ["idle", "land", "sit", "sleep", "hold", "react", "talk"];
    let frames: Vec<_> = frames_of("timber-wolf")
        .into_iter()
        .filter(|(animation, _, _)| posed.contains(&animation.as_str()))
        .collect();

    // Byte-identity still fails exact copies.
    for (i, (animation, frame, bytes)) in frames.iter().enumerate() {
        for (other_animation, other_frame, other_bytes) in &frames[i + 1..] {
            if animation == other_animation {
                continue;
            }
            assert!(
                bytes != other_bytes,
                "{animation} frame {frame} is the same art as {other_animation}                  frame {other_frame}"
            );
        }
    }

    // First frame of each pose vs every other pose: near-duplicate frontals
    // from the turn clip scored mean RGB ~1-3; distinct art clears 15+.
    const MIN_MEAN_RGB: f64 = 12.0;
    let first: Vec<_> = posed
        .iter()
        .map(|anim| {
            let (frame, bytes) = frames
                .iter()
                .find(|(a, _, _)| a == anim)
                .map(|(_, f, b)| (f.clone(), b.clone()))
                .unwrap_or_else(|| panic!("{anim} has a first frame"));
            let (_, _, rgba) = frame_rgba(&bytes);
            ((*anim).to_string(), frame, rgba)
        })
        .collect();

    for (i, (animation, frame, rgba)) in first.iter().enumerate() {
        for (other_animation, other_frame, other_rgba) in &first[i + 1..] {
            let d = mean_rgb_distance(rgba, other_rgba);
            assert!(
                d >= MIN_MEAN_RGB,
                "{animation} frame {frame} is a near-copy of {other_animation}                  frame {other_frame} (mean RGB distance {d:.2}, need >= {MIN_MEAN_RGB})"
            );
        }
    }

    // Sit is a hunker: silhouette at least 12px shorter than idle.
    let idle_h = {
        let bytes = &frames
            .iter()
            .find(|(a, f, _)| a == "idle" && f.ends_with("idle-0.png"))
            .expect("idle-0")
            .2;
        let (w, h, alpha) = frame_alpha(bytes);
        let (top, bottom, _) = silhouette(w, h, &alpha);
        bottom - top + 1
    };
    let sit_h = {
        let bytes = &frames
            .iter()
            .find(|(a, f, _)| a == "sit" && f.ends_with("sit-0.png"))
            .expect("sit-0")
            .2;
        let (w, h, alpha) = frame_alpha(bytes);
        let (top, bottom, _) = silhouette(w, h, &alpha);
        bottom - top + 1
    };
    assert!(
        sit_h + 12 <= idle_h,
        "sit silhouette is {sit_h}px and idle is {idle_h}px; sit must hunker at least 12px lower"
    );
}

/// TAC laser is an idle-gesture (scan-1..3; scan-0 is a dupe of idle-0).
/// Sit stays a hunker. Perched sit then idle is #311.
#[test]
fn timber_wolf_declares_tac_laser_scan_as_idle_gesture() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    assert!(
        character.animations.contains_key("scan"),
        "the TAC sweep keeps the name scan"
    );
    let scan = &character.animations["scan"];
    assert_eq!(scan.frames.len(), 4, "scan is a four-frame TAC sweep");
    assert_idle_gesture(&character, "scan");
    assert!(
        character.animations["sit"].variants.is_empty(),
        "sit is not where the laser lives"
    );
    assert!(
        matches!(
            character.behaviors["scan"].animation.as_deref(),
            Some("scan")
        ),
        "scan plays the TAC sweep by name"
    );
    for name in [
        "frames/scan-1.png",
        "frames/scan-2.png",
        "frames/scan-3.png",
    ] {
        assert!(
            scan.frames.iter().any(|f| f == name),
            "scan includes {name}"
        );
    }
    assert!(
        scan.frames.iter().all(|f| f != "frames/scan-0.png"),
        "scan-0 is a dupe of idle-0 and must not be referenced"
    );

    let drawn = character
        .draw("scan", 0, 0, 1.0)
        .expect("scan draws when asked for by name");
    assert_eq!(drawn.animation, "scan");

    let engage = &character.behaviors["engage"];
    assert!(
        engage.plays(character::Primitive::Idle),
        "engage still stands on the calm loop before it raises weapons"
    );
    assert!(
        engage.plays(character::Primitive::React),
        "engage still raises weapons"
    );

    assert!(character.behaviors.contains_key("patrol"), "patrol stays");
    let pursue = &character.behaviors["pursue"];
    assert!(
        pursue.plays(character::Primitive::Chase),
        "pursue still walks toward the cursor"
    );
}

/// One display, big enough for a sprite to land on and walk about.
const DISPLAY: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1000.0,
    height: 800.0,
};

/// The art Timber Wolf draws each time a patrol reaches its idle Primitive,
/// over `stretches` of them, for one Instance's variant seed.
///
/// Through the Engine rather than by calling `draw` with a chosen number,
/// because the property under test is that the two agree: the Engine decides
/// when a draw happens, and a `draw` called directly cannot disagree with
/// itself.
fn patrol_idle_art(character: &character::Character, seed: u64, stretches: usize) -> Vec<String> {
    let ground = WorldSnapshot {
        displays: vec![DISPLAY],
        elapsed_ms: 100,
        ..WorldSnapshot::default()
    };
    let mut engine = Engine::new(Point { x: 500.0, y: 0.0 })
        .with_behaviors(character.behaviors.clone())
        .with_variant_seed(seed)
        .with_idle_plan(character.idle_plan());

    let mut art = Vec::new();
    let mut idling = false;
    let mut playing = true;
    // A bound rather than a loop that hangs the suite: one patrol is three
    // Primitive turns and the walk it coasts out of, well inside a hundred
    // ticks. The landing before the first one costs a handful more.
    for _ in 0..40 + 100 * stretches {
        let frame = engine.tick(&WorldSnapshot {
            // Proposed only when nothing is playing: a proposal restarts the
            // chain, so one every tick would never let patrol reach its idle.
            proposal: (!playing).then(|| BehaviorProposal {
                behavior: "patrol".to_string(),
                dialogue: None,
                direction: None,
            }),
            ..ground.clone()
        });
        playing = frame.playing_primitive.is_some();
        let on_idle = frame.playing_primitive == Some(character::Primitive::Idle);
        if on_idle && !idling {
            art.push(
                character
                    .draw(
                        frame.animation,
                        frame.animation_ms,
                        frame.ring_choice(),
                        frame.facing,
                    )
                    .expect("Timber Wolf draws the idle it is playing")
                    .animation
                    .to_string(),
            );
            if art.len() == stretches {
                break;
            }
        }
        idling = on_idle;
    }
    art
}

/// A Behavior's `idle` plays the calm loop. `scan` is an idle-gesture, so it
/// stays out of that step.
#[test]
fn timber_wolf_behavior_idle_stays_on_the_calm_loop() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    let art = patrol_idle_art(&character, 0x5EED, 40);

    assert_eq!(art.len(), 40, "forty patrols reached their idle: {art:?}");
    assert!(
        art.iter().all(|name| name == "idle"),
        "the behavior loops the calm idle, not the scan gesture: {art:?}"
    );
}

/// The same seed and Character draw the same idle art in the same order.
#[test]
fn the_same_seed_draws_timber_wolfs_idle_in_the_same_order() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    let art = patrol_idle_art(&character, 7, 16);

    assert_eq!(
        art,
        patrol_idle_art(&character, 7, 16),
        "same seed, same art"
    );
    assert!(
        art.iter().all(|name| name == "idle"),
        "the patrol's idle step is the calm loop: {art:?}"
    );
}

/// A quiet perch sits, then the calm idle loop. `scan` is a gesture, so the
/// opening of perched idle is the base loop.
#[test]
fn timber_wolf_perched_idle_opens_on_the_calm_loop() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");
    let perch = WorldSnapshot {
        displays: vec![DISPLAY],
        windows: vec![Window {
            id: 1,
            rect: Rect {
                x: 50.0,
                y: 400.0,
                width: 300.0,
                height: 200.0,
            },
        }],
        elapsed_ms: 100,
        ..WorldSnapshot::default()
    };

    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(character.behaviors.clone())
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());
    let mut opened = None;
    for _ in 0..80 {
        let frame = engine.tick(&perch);
        if frame.animation != "idle" {
            continue;
        }
        assert!(matches!(frame.idle_beat, Some(IdleBeat::IdleLoop(_))));
        let drawn = character
            .draw(
                frame.animation,
                frame.animation_ms,
                frame.ring_choice(),
                frame.facing,
            )
            .expect("draws");
        opened = Some(drawn.animation.to_string());
        assert_eq!(drawn.index, 0, "the loop starts at frame 0");
        break;
    }
    assert_eq!(
        opened.as_deref(),
        Some("idle"),
        "perched idle opens on the calm loop"
    );
}

/// A patrol mech tracks a near contact and raises a weapon on a rush.
#[test]
fn timber_wolf_declares_cursor_reactions() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    assert_eq!(
        character.near_reaction,
        CursorReaction::Face,
        "the cursor entering the Near radius turns the mech to track it"
    );
    assert_eq!(
        character.rush_reaction,
        CursorReaction::React,
        "a cursor rushing the chassis plays react, the weapon raise"
    );
}

/// Under-arm Sketchfab plate must not seal the crook: the desk shows through
/// between torso and inner arm. Window is the left crook on front-facing
/// frames (his right arm). Near-black opaque must not dominate that pocket.
#[test]
fn timber_wolf_underarm_crook_is_not_a_near_black_plate() {
    let files = package_bytes("timber-wolf");
    // Left under-arm pocket on the 176×160 frontals (idle / talk-2 / react / scan).
    const X0: usize = 55;
    const X1: usize = 72;
    const Y0: usize = 70;
    const Y1: usize = 86;

    let frames = [
        "frames/idle-0.png",
        "frames/idle-1.png",
        "frames/talk-2.png",
        "frames/react-0.png",
        "frames/react-1.png",
        "frames/scan-1.png",
        "frames/scan-2.png",
        "frames/scan-3.png",
    ];
    for path in frames {
        let bytes = files
            .get(path)
            .unwrap_or_else(|| panic!("{path} is in the package"));
        let (width, height, rgba) = frame_rgba(bytes);
        assert_eq!((width, height), (176, 160), "{path} stays on the TW canvas");

        let mut transparent = 0usize;
        let mut opaque = 0usize;
        let mut near_black = 0usize;
        for y in Y0..Y1 {
            for x in X0..X1 {
                let px = rgba[y * width + x];
                if px[3] < 8 {
                    transparent += 1;
                    continue;
                }
                if px[3] < VISIBLE {
                    continue;
                }
                opaque += 1;
                let mx = px[0].max(px[1]).max(px[2]);
                if mx <= 22 {
                    near_black += 1;
                }
            }
        }

        assert!(
            transparent >= 140,
            "{path}: left under-arm crook must open to the desk (transparent={transparent}, want >= 140)"
        );
        assert!(
            near_black * 2 <= opaque + transparent / 4,
            "{path}: near-black opaque must not plate-fill the crook \
             (near_black={near_black} opaque={opaque} transparent={transparent})"
        );

        // Outer gun column still solid — remat must not eat the hanging pod.
        let mut outer = 0usize;
        for y in 55..100 {
            for x in 36..50 {
                if rgba[y * width + x][3] >= VISIBLE {
                    outer += 1;
                }
            }
        }
        assert!(
            outer >= 140,
            "{path}: left outer gun/arm column still solid (opaque={outer})"
        );
    }
}

/// Buddy Bot: logo mascot package loads with all nine required animations and
/// the authored frame counts from the Grok Imagine pack.
#[test]
fn buddy_bot_package_loads_with_all_required_animations() {
    let character = load_package("buddy-bot").expect("Buddy Bot package is valid");

    assert_eq!(character.name, "Buddy Bot");
    assert!(
        !character.personality.is_empty(),
        "Buddy Bot has a personality prompt"
    );

    assert_required_animations(&character);

    let expected_frames = [
        ("idle", 16),
        ("walk", 8),
        ("talk", 6),
        ("sleep", 4),
        ("sit", 4),
        ("react", 5),
        ("land", 4),
        ("hold", 4),
        ("fall", 6),
    ];
    for (animation, count) in expected_frames {
        assert_eq!(
            character.animations[animation].frames.len(),
            count,
            "{animation} must carry the shipped {count}-frame loop"
        );
    }
}

#[test]
fn buddy_bot_uses_scale_1_for_authored_desktop_frames() {
    let character = load_package("buddy-bot").expect("Buddy Bot package is valid");

    assert_eq!(
        character.scale, 1,
        "Buddy Bot uses scale 1: 90×90 frames are already desktop-sized; \
         the schema only allows whole-number scale 1–4"
    );
    assert!(
        character.smooth,
        "soft anti-aliased mascot art uses smooth, not pixelated"
    );
}

#[test]
fn buddy_bot_frames_are_90_square_rgba() {
    let frames = frames_of("buddy-bot");
    assert_eq!(
        frames.len(),
        78,
        "the pack ships 78 PNG frames (57 base + 21 idle-gesture frames)"
    );

    for (animation, frame, bytes) in frames {
        let (width, height, alpha) = frame_alpha(&bytes);
        assert_eq!(
            (width, height),
            (90, 90),
            "{animation} frame {frame} must be 90×90"
        );
        assert!(
            alpha.iter().any(|&a| a > 0),
            "{animation} frame {frame} must have visible pixels"
        );
        assert!(
            alpha.contains(&0),
            "{animation} frame {frame} must keep a transparent margin"
        );
    }
}

/// Mood animations are idle-gestures, declared as art rather than Behaviors.
#[test]
fn buddy_bot_declares_idle_life_gestures() {
    let character = load_package("buddy-bot").expect("Buddy Bot package is valid");

    let expected = [("idle-breathe", 8), ("idle-blink", 6), ("idle-listen", 7)];
    for (name, count) in expected {
        assert!(
            character.animations.contains_key(name),
            "{name} animation is declared"
        );
        let anim = &character.animations[name];
        assert_eq!(
            anim.frames.len(),
            count,
            "{name} must carry the shipped {count}-frame animation"
        );
        assert_idle_gesture(&character, name);
        assert!(
            !character.behaviors.contains_key(name),
            "no {name} Behavior; the gesture plays between loops"
        );
    }

    let drawn = character
        .draw("idle-breathe", 0, 0, 1.0)
        .expect("idle-breathe draws when asked for by name");
    assert_eq!(drawn.animation, "idle-breathe");
}

/// Eyes must be opaque black, not punched-through alpha holes (desktop shows through).
#[test]
fn buddy_bot_eyes_are_opaque_black_not_transparent() {
    let files = package_bytes("buddy-bot");
    let idle = files.get("frames/idle-0.png").expect("idle-0");
    let (width, height, rgba) = frame_rgba(idle);
    // Sample the two eye sockets near the face midline (90×90 pack).
    let samples = [(34usize, 33usize), (35, 34), (54, 33), (55, 34)];
    for (x, y) in samples {
        assert!(x < width && y < height, "sample ({x},{y}) in bounds");
        let px = rgba[y * width + x];
        assert_eq!(px[0], 0, "eye R at ({x},{y})");
        assert_eq!(px[1], 0, "eye G at ({x},{y})");
        assert_eq!(px[2], 0, "eye B at ({x},{y})");
        assert_eq!(
            px[3], 255,
            "eye alpha at ({x},{y}) must be opaque, got {}",
            px[3]
        );
    }
}

/// Brow-sensor LEDs on idle: cool cyan-white glow at the two dots above the
/// eyes. Positions shift with the idle bob, so we search the brow band.
/// Mirror-safe alive signal; cheek LED on walk is #345.
#[test]
fn buddy_bot_idle_has_mirror_safe_brow_sensor_leds() {
    let files = package_bytes("buddy-bot");
    for frame in [
        "frames/idle-0.png",
        "frames/idle-breathe-4.png",
        "frames/idle-blink-2.png",
        "frames/idle-listen-3.png",
    ] {
        let bytes = files.get(frame).unwrap_or_else(|| panic!("{frame}"));
        let (width, height, rgba) = frame_rgba(bytes);
        let y0 = height * 16 / 90;
        let y1 = height * 30 / 90;
        let mut left = 0usize;
        let mut right = 0usize;
        for y in y0..=y1 {
            for x in 0..width {
                let px = rgba[y * width + x];
                let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
                if a < 255 || b <= 200 || g <= 180 || b <= r || (b as i16 - r as i16) <= 20 {
                    continue;
                }
                if x < width / 2 {
                    left += 1;
                } else {
                    right += 1;
                }
            }
        }
        assert!(
            left >= 1 && right >= 1,
            "{frame} needs brow-sensor glow on both sides of mid, got left={left} right={right}"
        );
    }
}

/// Walk art must already face right (engine mirrors for left). Idle keeps the
/// cyan cheek LED on the viewer's left. Walk omits that LED; facing is pinned
/// by dark eyes right of silhouette mid after the pack flip. MSE to the mirror
/// stays high so the pose is directional.
#[test]
fn buddy_bot_walk_faces_right_for_engine_mirroring() {
    let files = package_bytes("buddy-bot");
    let idle = files.get("frames/idle-0.png").expect("idle-0");
    let walk = files.get("frames/walk-0.png").expect("walk-0");
    let (idle_w, _, _) = frame_alpha(idle);
    let idle_mid = idle_w as f32 / 2.0;

    let idle_light = cyan_cheek_light_x(idle);
    assert!(
        idle_light < idle_mid,
        "idle cheek light is on the viewer's left, got {idle_light} mid {idle_mid}"
    );

    let (width, height, rgba) = frame_rgba(walk);
    let mse = mse_to_horizontal_flip(width, height, &rgba);
    assert!(
        mse > 500.0,
        "walk must be left-right asymmetric (3/4 pose); MSE to mirror was {mse}"
    );

    // After the Imagine pack's left-facing walk was mirrored, dark eyes sit
    // on the viewer-right of silhouette mid (character looks right).
    let (eye_x, sil_mid) = dark_eyes_left_of_silhouette_mid(width, height, &rgba);
    assert!(
        eye_x > sil_mid,
        "walk must face right: dark eyes right of silhouette mid, got eye_x={eye_x} sil_mid={sil_mid}"
    );

    // Flipping the authored walk must break the facing cue — guards against
    // re-flipping frames the engine already mirrors for leftward travel.
    let mut flipped = rgba.clone();
    for y in 0..height {
        for x in 0..(width / 2) {
            let left = y * width + x;
            let right = y * width + (width - 1 - x);
            flipped.swap(left, right);
        }
    }
    let (flip_eye_x, flip_sil_mid) = dark_eyes_left_of_silhouette_mid(width, height, &flipped);
    assert!(
        flip_eye_x < flip_sil_mid,
        "a horizontally flipped walk must fail the right-facing cue, got eye_x={flip_eye_x} sil_mid={flip_sil_mid}"
    );
}

#[test]
fn buddy_bot_declares_friendly_cursor_reactions() {
    let character = load_package("buddy-bot").expect("Buddy Bot package is valid");

    assert_eq!(
        character.near_reaction,
        CursorReaction::Speak,
        "a friendly mascot greets when the cursor enters Near"
    );
    assert_eq!(
        character.rush_reaction,
        CursorReaction::React,
        "a rush earns a lean-pop react, not flight"
    );
}

#[test]
fn buddy_bot_behaviors_cover_a_helpful_idle_life() {
    let character = load_package("buddy-bot").expect("Buddy Bot package is valid");

    for name in [
        "greet", "help", "fidget", "stroll", "patrol", "settle", "nap",
    ] {
        assert!(
            character.behaviors.contains_key(name),
            "Buddy Bot declares {name}"
        );
    }

    assert_eq!(
        character.behaviors["greet"].then.as_deref(),
        Some("stroll"),
        "greet chains into stroll so a hello is followed by a walk          (BMO greet→patrol / cat greet→inspect)"
    );
    assert_eq!(
        character.behaviors["help"].weight, 20,
        "help stays present but light enough that stroll can win the 30s–1m overlap"
    );
    assert_eq!(
        character.behaviors["stroll"].weight, 30,
        "stroll outranks fidget/help so the mascot actually walks the desk"
    );
    assert!(
        character.behaviors["stroll"].plays(character::Primitive::Walk),
        "stroll is a Walk"
    );
    assert_eq!(
        character.behaviors["settle"].then.as_deref(),
        Some("nap"),
        "settle chains into nap so sitting leads to sleep"
    );
    assert_eq!(
        character.behaviors["nap"].weight, 40,
        "nap outranks late-band stroll/patrol so sleep is visible"
    );
    assert!(
        matches!(
            character.behaviors["nap"].trigger,
            Some(character::Trigger::IdleOver(d)) if d == std::time::Duration::from_secs(120)
        ),
        "nap opens at idle over 2m (was 5m — too rare against walk)"
    );
    assert!(
        character.behaviors["nap"].plays(character::Primitive::Sleep),
        "nap ends on sleep for an empty desk"
    );
}

/// `PRIMITIVE_MS` is the turn, not the animation. The clock runs through that
/// turn, and the Engine's own idle after it starts a loop at frame 0.
#[test]
fn groom_plays_the_full_animation_once_then_the_calm_loop_returns() {
    let character = load_package("cat").expect("cat package is valid");
    let ground = WorldSnapshot {
        displays: vec![DISPLAY],
        elapsed_ms: 100,
        ..WorldSnapshot::default()
    };

    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(character.behaviors.clone())
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());
    let idling = (0..40)
        .map(|_| engine.tick(&ground))
        .find(|frame| frame.animation == "idle" && frame.playing_gesture.is_none())
        .expect("the cat lands and the landing gives way to idling");
    let opening = shown_cat(&character, &idling);
    assert_eq!(
        opening,
        ("idle".to_string(), 0),
        "the calm loop starts at frame 0"
    );
    assert!(matches!(idling.idle_beat, Some(IdleBeat::IdleLoop(_))));

    let mut during = Vec::new();
    let mut after = Vec::new();
    for tick in 0..20 {
        let frame = engine.tick(&WorldSnapshot {
            proposal: (tick == 0).then(|| BehaviorProposal {
                behavior: "groom".to_string(),
                dialogue: None,
                direction: None,
            }),
            ..ground.clone()
        });
        if tick == 0 {
            assert_eq!(frame.behavior.as_deref(), Some("groom"));
        }
        let art = shown_cat(&character, &frame);
        if frame.playing_gesture.as_deref() == Some("grooming") {
            during.push(art.1);
        } else if frame.animation == "idle" && !during.is_empty() {
            after.push(art);
            if after.len() == 4 {
                break;
            }
        }
    }

    assert!(
        during.windows(2).any(|pair| pair[1] == pair[0] + 1),
        "grooming plays the full animation forward: {during:?}"
    );
    assert!(
        !during.windows(2).any(|pair| pair[0] == 5 && pair[1] == 0),
        "grooming does not wrap: {during:?}"
    );
    assert_eq!(
        after.first().map(|art| art.0.as_str()),
        Some("idle"),
        "the calm loop returns: {after:?}"
    );
    assert_eq!(after.first().map(|art| art.1), Some(0));
}

/// Every idle-gesture animation stars in one Behavior. The proposal draws that
/// animation's first frame, including the family base, which must not reweigh.
#[test]
fn every_idle_gesture_animation_stars_as_its_own_behavior() {
    // Behavior, the animation it plays, that animation's first frame.
    let cases = [
        ("bmo", "skate", "skate", "frames/skate-0.png"),
        ("bmo", "sing", "sing", "frames/sing-0.png"),
        (
            "buddy-bot",
            "listen",
            "idle-listen",
            "frames/idle-listen-0.png",
        ),
        (
            "buddy-bot",
            "breathe",
            "idle-breathe",
            "frames/idle-breathe-0.png",
        ),
        (
            "buddy-bot",
            "blink",
            "idle-blink",
            "frames/idle-blink-0.png",
        ),
        ("cat", "groom", "grooming", "frames/idle-0.png"),
        ("cat", "waiting", "waiting", "frames/waiting-0.png"),
        (
            "rubber-duck",
            "look-left",
            "look-left",
            "frames/look-left-0.png",
        ),
        ("rubber-duck", "wing-wave", "wave", "frames/wave-0.png"),
        (
            "rubber-duck",
            "thinking",
            "thinking",
            "frames/thinking-0.png",
        ),
        (
            "rubber-duck",
            "pondering",
            "pondering",
            "frames/pondering-0.png",
        ),
        (
            "rubber-duck",
            "look-right",
            "look-right",
            "frames/look-right-0.png",
        ),
        ("timber-wolf", "scan", "scan", "frames/scan-1.png"),
        ("trump", "waiting", "waiting", "frames/waiting-0.png"),
        (
            "tux",
            "waddle-in-place",
            "waddle-in-place",
            "frames/waddle-in-place-0.png",
        ),
        (
            "tux",
            "restless-sitting",
            "restless-sitting",
            "frames/idle-0.png",
        ),
        (
            "tux",
            "look-around",
            "look-around",
            "frames/look-around-0.png",
        ),
        (
            "tux",
            "checking-work",
            "checking-work",
            "frames/checking-work-0.png",
        ),
        (
            "tux",
            "recover-card",
            "recover-card",
            "frames/recover-card-0.png",
        ),
        ("tuxedo-bean", "review", "review", "frames/review-0.png"),
        (
            "tuxedo-bean",
            "look-around",
            "look-around",
            "frames/look-around-0.png",
        ),
        ("tuxedo-bean", "nod-off", "nod-off", "frames/nod-off-0.png"),
        ("tuxedo-bean", "hand-wave", "wave", "frames/wave-0.png"),
    ];

    for (package, behavior, animation, first) in cases {
        let character = load_package(package).unwrap_or_else(|e| panic!("{package}: {e:?}"));
        let ground = WorldSnapshot {
            displays: vec![DISPLAY],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        };
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
            .with_behaviors(character.behaviors.clone())
            .with_variant_seed(1)
            .with_idle_plan(character.idle_plan());
        let landed = (0..80).any(|_| {
            let frame = engine.tick(&ground);
            frame.animation == "idle" && frame.playing_gesture.is_none()
        });
        assert!(landed, "{package} never idled");

        let mut saw = false;
        for tick in 0..30 {
            let frame = engine.tick(&WorldSnapshot {
                proposal: (tick == 0).then(|| BehaviorProposal {
                    behavior: behavior.to_string(),
                    dialogue: None,
                    direction: None,
                }),
                ..ground.clone()
            });
            if frame.playing_gesture.as_deref() != Some(animation) {
                continue;
            }
            let drawn = character
                .draw(
                    animation,
                    frame.animation_ms,
                    frame.ring_choice(),
                    frame.facing,
                )
                .unwrap_or_else(|| panic!("{package} {behavior} did not draw"));
            assert_eq!(drawn.animation, animation, "{package} {behavior}");
            assert_eq!(
                drawn.index, 0,
                "{package} {behavior} did not start at frame 0"
            );
            assert_eq!(
                character.animations[animation].frames[0], first,
                "{package} {behavior} starred the wrong animation"
            );
            saw = true;
            break;
        }
        assert!(
            saw,
            "{package} behavior {behavior} never played {animation}"
        );
    }

    let jotaro = load_package("jotaro-kujo").expect("jotaro");
    assert!(
        !jotaro.animations.contains_key("idle-gesture")
            && !jotaro.behaviors.contains_key("coat-flick"),
        "Jotaro's stand is the idle loop, not a gesture"
    );
    assert_eq!(jotaro.animations["idle"].frames.len(), 6);
    assert!(jotaro.animations["idle"]
        .frames
        .iter()
        .all(|frame| frame.starts_with("frames/idle-")));

    for package in shipped_packages() {
        let character = load_package(&package).unwrap_or_else(|e| panic!("{package}: {e:?}"));
        assert!(
            !character.animations.contains_key("idle-gesture"),
            "{package} must not sacrifice an animation's name to the family"
        );
        let family: Vec<&str> = character
            .animations
            .iter()
            .filter(|(_, animation)| animation.variant_of.as_deref() == Some("idle-gesture"))
            .map(|(name, _)| name.as_str())
            .collect();
        for name in &family {
            assert!(
                cases.iter().any(|(_, _, animation, _)| animation == name),
                "{package} {name} has no starring Behavior"
            );
            assert_ne!(*name, "idle");
        }
        for (_, _, animation, _) in cases.iter().filter(|(pkg, ..)| *pkg == package) {
            assert!(
                family.contains(animation),
                "{package} {animation} is not an idle-gesture"
            );
        }
    }

    // New starring wrappers are weight 0: callable via Director/MCP proposal,
    // not a second ambient cadence beside idle_gesture_every_s.
    let buddy = load_package("buddy-bot").expect("buddy");
    for name in ["blink", "breathe", "listen"] {
        let behavior = &buddy.behaviors[name];
        assert_eq!(
            behavior.weight, 0,
            "buddy {name} must be callable-only, not ambient"
        );
        assert_eq!(
            behavior.animation.as_deref(),
            match name {
                "blink" => Some("idle-blink"),
                "breathe" => Some("idle-breathe"),
                "listen" => Some("idle-listen"),
                _ => None,
            },
            "buddy {name} stars its animation"
        );
    }
    let mut context = fidget_core::director::Context::quiet();
    context.activity.idle = std::time::Duration::from_secs(60);
    let mut ambient = BTreeSet::new();
    for seed in 0..400 {
        let mut director =
            fidget_core::director::StaticDirector::new(buddy.behaviors.clone(), seed);
        if let Some(proposal) = director.propose(&context) {
            ambient.insert(proposal.behavior);
        }
    }
    for name in ["blink", "breathe", "listen"] {
        assert!(
            !ambient.contains(name),
            "weight-0 buddy {name} must not fire on ambient cadence: {ambient:?}"
        );
    }
}

fn shown_cat(
    character: &character::Character,
    frame: &fidget_core::engine::Frame,
) -> (String, usize) {
    let name = frame.art_name();
    let drawn = character
        .draw(name, frame.animation_ms, frame.ring_choice(), frame.facing)
        .expect("the cat draws the idle it is playing");
    (drawn.animation.to_string(), drawn.index)
}

/// Black Mage's own idle softly breathes: on its feet with nothing proposed,
/// the drawn image changes well inside five seconds and comes back around.
/// Watched through the Engine at a 16 ms frame, as the renderer draws it.
#[test]
fn black_mage_idle_breathes_rather_than_holding_one_image() {
    let character = load_package("black-mage").expect("Black Mage package is valid");
    let ground = WorldSnapshot {
        displays: vec![DISPLAY],
        elapsed_ms: 16,
        ..WorldSnapshot::default()
    };
    let mut engine = Engine::new(Point { x: 500.0, y: 0.0 })
        .with_behaviors(character.behaviors.clone())
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());

    let mut shown: Vec<String> = Vec::new();
    for _ in 0..2_000 {
        let frame = engine.tick(&ground);
        if !matches!(frame.idle_beat, Some(IdleBeat::IdleLoop(_))) {
            assert!(shown.is_empty(), "the calm loop is not interrupted");
            continue;
        }
        let drawn = character
            .draw(
                frame.animation,
                frame.animation_ms,
                frame.ring_choice(),
                frame.facing,
            )
            .expect("Black Mage draws its idle");
        let (file, _) = character
            .art
            .iter()
            .find(|(_, art)| std::ptr::eq(&art.mask, drawn.mask))
            .expect("the drawn mask is a package frame");
        shown.push(file.clone());
        if shown.len() == 750 {
            break;
        }
    }
    assert_eq!(shown.len(), 750, "twelve seconds of uninterrupted idle");

    let longest = shown
        .chunk_by(|a, b| a == b)
        .map(|run| run.len() * 16)
        .max()
        .unwrap_or_default();
    assert!(
        longest < 2_000,
        "the idle holds one image for {longest} ms; breathing keeps moving"
    );
    let distinct: BTreeSet<_> = shown.iter().collect();
    assert!(distinct.len() >= 2, "the idle shows more than one pose");
    let first = &shown[0];
    let back = shown
        .iter()
        .skip_while(|file| *file == first)
        .any(|file| file == first);
    assert!(back, "the idle returns to its first pose and repeats");
}
