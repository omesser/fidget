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

use fidget_core::character::{self, CursorReaction, Primitive, REQUIRED_ANIMATIONS};
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
        engage.primitives.contains(&character::Primitive::React),
        "engage includes react (weapon raise)"
    );

    assert!(
        character.behaviors.contains_key("pursue"),
        "pursue behavior exists for walk-toward-cursor"
    );
    let pursue = &character.behaviors["pursue"];
    assert!(
        pursue.primitives.contains(&character::Primitive::Chase),
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
// Fall/grab strips can reserve space below curled feet independently of standing.
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
                frame.variant_draw,
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

#[test]
fn rubber_duck_plays_thinking_and_looking_in_its_idle_rotation() {
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
        assert!(character.animations["idle"]
            .variants
            .contains(&name.to_string()));
        let animation = &character.animations[name];
        assert!(animation.looping, "{name} repeats while idling");
        assert_eq!(
            animation.frames.iter().collect::<BTreeSet<_>>().len(),
            source_frames
        );

        let seed = (0..4096)
            .find(|seed| character.draw("idle", 0, *seed, 1.0).unwrap().animation == name)
            .unwrap_or_else(|| panic!("{name} is unreachable from idle"));
        for index in 0..animation.frames.len() {
            let elapsed = (index as u32 * 1000).div_ceil(animation.fps);
            let drawn = character.draw("idle", elapsed, seed, 1.0).unwrap();
            assert_eq!(drawn.animation, name);
            assert_eq!(drawn.index, index, "{name} reaches its complete loop");
        }
    }
}

/// Every listed idle variant is reachable from idle and plays one complete
/// loop, and the left walk draws its own poses rather than mirroring.
fn assert_idle_loops_and_drawn_left_walk(package: &str, art: usize, loops: &[(&str, usize)]) {
    let character = load_package(package).unwrap_or_else(|e| panic!("{package} is valid: {e:?}"));
    assert_eq!(character.art.len(), art, "all source rows are retained");
    for &(name, count) in loops {
        let animation = &character.animations[name];
        assert_eq!(animation.frames.len(), count);
        let seed = (0..4096)
            .find(|seed| character.draw("idle", 0, *seed, 1.0).unwrap().animation == name)
            .unwrap_or_else(|| panic!("{name} is unreachable while idling"));
        for index in 0..=count {
            let elapsed = (index as u32 * 1000).div_ceil(animation.fps);
            let drawn = character.draw("idle", elapsed, seed, 1.0).unwrap();
            assert_eq!(drawn.animation, name);
            assert_eq!(drawn.index, index % count, "{name} plays one complete loop");
        }
    }
    let left = character.draw("walk", 0, 0, -1.0).expect("left walk draws");
    assert_eq!(left.animation, "walk-left");
    assert!(!left.mirrored, "{package} uses the drawn left-facing poses");
}

#[test]
fn tux_draws_each_idle_loop_and_its_left_waddle() {
    let character = load_package("tux").expect("Tux package is valid");
    assert_eq!(character.art.len(), 57, "all nine source rows are retained");
    for (name, count) in [
        ("look-around", 6),
        ("checking-work", 6),
        ("recover-card", 8),
        ("waddle-in-place", 6),
    ] {
        let animation = &character.animations[name];
        assert_eq!(animation.frames.len(), count);
        let seed = (0..4096)
            .find(|seed| character.draw("idle", 0, *seed, 1.0).unwrap().animation == name)
            .unwrap_or_else(|| panic!("{name} is unreachable while idling"));
        for index in 0..=count {
            let elapsed = (index as u32 * 1000).div_ceil(animation.fps);
            let drawn = character.draw("idle", elapsed, seed, 1.0).unwrap();
            assert_eq!(drawn.animation, name);
            assert_eq!(drawn.index, index % count, "{name} plays one complete loop");
        }
    }
    let left = character.draw("walk", 0, 0, -1.0).expect("left walk draws");
    assert_eq!(left.animation, "walk-left");
    assert!(!left.mirrored, "Tux uses the drawn left-facing poses");
}

#[test]
fn tuxedo_bean_draws_each_idle_loop_and_its_left_walk() {
    assert_idle_loops_and_drawn_left_walk(
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
/// as an idle variant, or through a Behavior the Static Director can pick.
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
            character.animations["idle"]
                .variants
                .contains(&name.to_string()),
            "row {row} ({name}) plays while idling"
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
                .any(|behavior| behavior.weight > 0 && behavior.primitives.contains(&primitive)),
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

/// Every walk and fall frame as drawn for both ways of travel faces that way.
/// `faces_right` reads the art: positive when the PNG faces right.
fn assert_walk_and_fall_face_the_travel(package: &str, faces_right: fn(&[u8]) -> f32) {
    let character = load_package(package).unwrap_or_else(|e| panic!("{package}: {e:?}"));
    let files = package_bytes(package);
    for name in ["walk", "fall"] {
        let animation = &character.animations[name];
        for (index, frame) in animation.frames.iter().enumerate() {
            let at = ((index as u32 * 2 + 1) * 500) / animation.fps;
            for travel in [1.0, -1.0] {
                let drawn = character.draw(name, at, 0, travel).expect("draws");
                assert_eq!(drawn.index, index, "{package} {name} reaches {frame}");
                let offset = faces_right(&files[frame]);
                let shown = if drawn.mirrored { -offset } else { offset };
                assert!(
                    shown * travel as f32 > 2.0,
                    "{package} {frame} drawn for travel {travel} faces it, offset {shown}"
                );
            }
        }
    }
}

/// Jump draws `fall`, and Jotaro's fall frames face left while his walk faces
/// right. The manifest says so, and the renderer mirrors each to the travel.
#[test]
fn jotaro_falls_facing_the_way_he_walks() {
    assert_walk_and_fall_face_the_travel("jotaro-kujo", jotaro_badge_offset);
}

/// BMO's fall frames are drawn turned the other way from its walk.
#[test]
fn bmo_falls_facing_the_way_it_walks() {
    assert_walk_and_fall_face_the_travel("bmo", bmo_panel_offset);
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

/// TAC laser: optional idle variant only (scan-1..3; scan-0 is a dupe of
/// idle-0). Not baked into sit — perched sit→idle escape is #311.
#[test]
fn timber_wolf_declares_tac_laser_scan_as_idle_variant() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    assert!(
        character.animations.contains_key("scan"),
        "scan animation is declared"
    );
    let scan = &character.animations["scan"];
    assert_eq!(scan.frames.len(), 4, "scan is a four-frame TAC loop");
    assert!(
        character.animations["idle"]
            .variants
            .contains(&"scan".to_string()),
        "scan is an idle variant"
    );
    assert!(
        character.animations["sit"].variants.is_empty(),
        "sit is not where the laser lives"
    );
    assert!(
        !character.behaviors.contains_key("scan"),
        "no scan Behavior — the variant ring is enough"
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
        engage.primitives.contains(&character::Primitive::Idle),
        "engage steps through idle, which is where a scan can be drawn"
    );
    assert!(
        engage.primitives.contains(&character::Primitive::React),
        "engage still raises weapons"
    );

    assert!(character.behaviors.contains_key("patrol"), "patrol stays");
    let pursue = &character.behaviors["pursue"];
    assert!(
        pursue.primitives.contains(&character::Primitive::Chase),
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
        .with_variant_seed(seed);

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
                        frame.variant_draw,
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

/// The TAC laser is a weighted draw taken when idle starts, so it
/// surfaces inside a Behavior's idle Primitive — a turn of under a second,
/// which is the case an author is most likely to think is broken.
#[test]
fn timber_wolf_scans_inside_a_behaviors_idle_primitive() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    let art = patrol_idle_art(&character, 0x5EED, 40);

    assert_eq!(art.len(), 40, "forty patrols reached their idle: {art:?}");
    assert!(
        art.iter().any(|name| name == "scan"),
        "the laser sweeps on some of them: {art:?}"
    );
    assert!(
        art.iter().filter(|name| *name == "idle").count() * 2 > art.len(),
        "and the base still dominates, because the manifest says so — the sweep \
         declares a quarter of the ring: {art:?}"
    );
}

/// The same seed and Character draw the same members in the same order.
/// A draw is the only thing deciding which member plays, so nothing else pins
/// this.
#[test]
fn the_same_seed_draws_timber_wolfs_variants_in_the_same_order() {
    let character = load_package("timber-wolf").expect("Timber Wolf package is valid");

    let art = patrol_idle_art(&character, 7, 16);

    assert_eq!(
        art,
        patrol_idle_art(&character, 7, 16),
        "same seed, same art"
    );
    assert_ne!(
        art,
        patrol_idle_art(&character, 8, 16),
        "and two Instances of one Character do not idle in lockstep"
    );
}

/// A quiet perch leaves sit for idle, and that start is a draw like any
/// other, whatever `PERCHED_IDLE_AFTER_MS` is.
#[test]
fn timber_wolf_scans_on_the_first_perched_idle_whatever_the_seed() {
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

    // The art on the first idle tick of each Instance's perched rest, however
    // long that rest then lasts: no proposal, no Behavior, only the switch out
    // of sit.
    let first_idle = |seed: u64| {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
            .with_behaviors(character.behaviors.clone())
            .with_variant_seed(seed);
        (0..200)
            .map(|_| engine.tick(&perch))
            .find(|frame| frame.animation == "idle")
            .map(|frame| {
                character
                    .draw(
                        frame.animation,
                        frame.animation_ms,
                        frame.variant_draw,
                        frame.facing,
                    )
                    .expect("draws")
                    .animation
                    .to_string()
            })
            .expect("a quietly perched sprite leaves sit for idle")
    };

    let drawn: BTreeSet<String> = (0..32).map(first_idle).collect();
    assert!(
        drawn.contains("scan"),
        "the laser sweeps on a window: {drawn:?}"
    );
    assert!(
        drawn.contains("idle"),
        "and mostly it only stands: {drawn:?}"
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
        "the pack ships 78 PNG frames (57 base + 21 idle variants)"
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

/// Idle mood variants ring with base idle (BMO / Timber Wolf pattern). Declared
/// as art, not Behaviors — the ring is enough without Director weight.
#[test]
fn buddy_bot_declares_idle_life_variants() {
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
            "{name} must carry the shipped {count}-frame loop"
        );
        assert!(
            character.animations["idle"]
                .variants
                .contains(&name.to_string()),
            "{name} is an idle variant"
        );
        assert!(
            !character.behaviors.contains_key(name),
            "no {name} Behavior — the variant ring is enough"
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
        character.behaviors["stroll"]
            .primitives
            .contains(&character::Primitive::Walk),
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
        character.behaviors["nap"]
            .primitives
            .contains(&character::Primitive::Sleep),
        "nap ends on sleep for an empty desk"
    );
}

/// `PRIMITIVE_MS` is the Engine's turn, not the art's length. Restarting that
/// clock would replay the opening of the strip and never the rest. The test
/// goes through the Engine, because `draw` with a chosen `ms` starts its own.
#[test]
fn a_behaviors_idle_sweeps_every_frame_of_the_strip_the_cat_drew() {
    let character = load_package("cat").expect("cat package is valid");
    let ground = WorldSnapshot {
        displays: vec![DISPLAY],
        elapsed_ms: 100,
        ..WorldSnapshot::default()
    };

    let mut strips = BTreeSet::new();
    for seed in 0..16 {
        let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
            .with_behaviors(character.behaviors.clone())
            .with_variant_seed(seed);
        let idling = (0..40)
            .map(|_| engine.tick(&ground))
            .find(|frame| frame.animation == "idle")
            .expect("the cat lands and the landing gives way to idling");
        assert_eq!(idling.animation_ms, 0, "seed {seed} idles from frame zero");

        // `groom` is a single Idle Primitive over an idle already on screen:
        // the Behavior with nothing to restart.
        let mut swept = BTreeSet::new();
        let mut drew = BTreeSet::new();
        for tick in 0..13 {
            let frame = engine.tick(&WorldSnapshot {
                proposal: (tick == 0).then(|| BehaviorProposal {
                    behavior: "groom".to_string(),
                    dialogue: None,
                }),
                ..ground.clone()
            });
            if tick == 0 {
                assert_eq!(
                    frame.behavior.as_deref(),
                    Some("groom"),
                    "seed {seed} played the Behavior rather than refusing it"
                );
            }
            let art = character
                .draw(
                    frame.animation,
                    frame.animation_ms,
                    frame.variant_draw,
                    frame.facing,
                )
                .expect("the cat draws the idle it is playing");
            drew.insert(art.animation.to_string());
            swept.insert(art.index);
        }

        assert_eq!(
            swept,
            BTreeSet::from([0, 1, 2, 3, 4, 5]),
            "seed {seed} drew {drew:?} end to end"
        );
        strips.extend(drew);
    }

    assert_eq!(
        strips,
        BTreeSet::from(["idle".to_string(), "waiting".to_string()]),
        "and both members of the ring took a turn, so the variant is pinned \
         with the base"
    );
}
