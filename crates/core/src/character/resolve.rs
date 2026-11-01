//! Check declared Animations against art, and Behaviors against each other.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use crate::overlay::AlphaMask;

use super::manifest::{Declared, DeclaredAnimation, DeclaredBehavior};
use super::{
    Animation, Art, Behavior, Facing, PackageBytes, ALPHA_THRESHOLD, MAX_CHARACTER_PIXELS,
    MAX_FRAME_SIDE, SHOWN_LOOP_BEHAVIORS,
};

/// Check every declared Animation against package art and decode what passes.
/// Headers before masks, so over-budget art is refused before a mask is built.
/// A bad frame is rejected here, not after the loader has already called the Character valid.
pub(super) fn resolve_animations(
    package: &PackageBytes,
    declared: BTreeMap<String, DeclaredAnimation>,
    facing: Facing,
    frame_facing: &BTreeMap<String, Facing>,
    errors: &mut Vec<String>,
) -> (BTreeMap<String, Animation>, BTreeMap<String, Art>) {
    let mut animations = BTreeMap::new();
    let mut art: BTreeMap<String, Art> = BTreeMap::new();
    // One mask per distinct frame, exactly as the renderer holds them: a frame
    // two Animations share is charged once. The animation name is kept so a
    // decode error still names the declaration, not only the file.
    let mut charged: BTreeMap<String, String> = BTreeMap::new();
    let mut pixels: u64 = 0;

    for (name, declaration) in declared {
        let mut frame_size = None;

        for frame in &declaration.frames {
            let Some(bytes) = package.get(frame) else {
                errors.push(format!(
                    "animation {name:?} frame {frame:?} is not in the package"
                ));
                continue;
            };
            // Header first, pixels second: the header says how big the frame
            // claims to be for a few dozen bytes of bounded work, so a frame
            // over the size bound is rejected before anything inflates it.
            match art_size(bytes) {
                Err(why) => errors.push(format!(
                    "animation {name:?} frame {frame:?} is not readable art: {why}"
                )),
                Ok(size) if size.0 > MAX_FRAME_SIDE || size.1 > MAX_FRAME_SIDE => {
                    errors.push(format!(
                        "animation {name:?} frame {frame:?} is {}x{}, \
                         and no side of a frame may be over {MAX_FRAME_SIDE} pixels",
                        size.0, size.1
                    ));
                }
                Ok(size) => {
                    if let Entry::Vacant(slot) = charged.entry(frame.clone()) {
                        slot.insert(name.clone());
                        pixels += u64::from(size.0) * u64::from(size.1);
                    }
                    match frame_size {
                        None => frame_size = Some(size),
                        Some(first) if first != size => errors.push(format!(
                            "animation {name:?} frame {frame:?} is {}x{}, \
                             and its first frame is {}x{}; every frame is one size",
                            size.0, size.1, first.0, first.1
                        )),
                        Some(_) => {}
                    }
                }
            }
        }

        // A half-checked Animation is never handed out: any error at all makes
        // `load` return the errors instead of a Character.
        if let Some(frame_size) = frame_size {
            animations.insert(
                name,
                Animation {
                    frames: declaration.frames,
                    frame_size,
                    fps: declaration.fps,
                    looping: declaration.looping,
                    variants: Vec::new(),
                    left_strip: None,
                    weight: declaration.weight,
                },
            );
        }
    }

    if pixels > MAX_CHARACTER_PIXELS {
        errors.push(format!(
            "the package's frames are {pixels} pixels in all, over the \
             {MAX_CHARACTER_PIXELS}-pixel limit"
        ));
        // Past the budget the package is refused, and decoding on regardless
        // would build the very masks the bound exists to refuse. Headers
        // already named every frame; that is the whole of this path.
        return (animations, art);
    }

    for (frame, animation) in charged {
        let bytes = &package[&frame];
        match AlphaMask::from_png(bytes, ALPHA_THRESHOLD) {
            Ok(mask) => {
                let facing = frame_facing.get(&frame).copied().unwrap_or(facing);
                art.insert(
                    frame,
                    Art {
                        png: bytes.clone(),
                        mask,
                        facing,
                    },
                );
            }
            Err(why) => errors.push(format!(
                "animation {animation:?} frame {frame:?} is not readable art: {why}"
            )),
        }
    }

    (animations, art)
}

/// Every `[frame_facing]` entry names a frame some Animation plays, so a typo
/// cannot leave the real frame quietly facing the default.
pub(super) fn check_frame_facing(declared: &Declared, errors: &mut Vec<String>) {
    for frame in declared.frame_facing.keys() {
        let mut played = declared.animations.values().flat_map(|a| &a.frames);
        if !played.any(|f| f == frame) {
            errors.push(format!(
                "frame_facing names {frame:?}, which no animation plays"
            ));
        }
    }
}

/// Validate every `variant_of` and hand back the (variant, base) pairs.
/// A variant is more art for an Animation the engine already plays, never a new
/// Behavior. A held last frame would stall; a variant of a variant nests a ring.
pub(super) fn check_variants(
    declared: &BTreeMap<String, DeclaredAnimation>,
    errors: &mut Vec<String>,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for (name, animation) in declared {
        let Some(base) = &animation.variant_of else {
            continue;
        };
        match declared.get(base) {
            _ if base == name => errors.push(format!("animation {name:?} is a variant_of itself")),
            None => errors.push(format!(
                "animation {name:?} is a variant_of {base:?}, which is not declared"
            )),
            Some(target) if target.variant_of.is_some() => errors.push(format!(
                "animation {name:?} is a variant_of {base:?}, itself a variant; \
                 variants ring one base"
            )),
            Some(target) if !target.looping || !animation.looping => errors.push(format!(
                "animation {name:?} and its base {base:?} must both loop \"forever\"; \
                 a drawn variant plays until the engine asks for something else"
            )),
            Some(_) => pairs.push((name.clone(), base.clone())),
        }
    }
    pairs
}

/// Validate every `left_of` and hand back the (strip, base) pairs.
/// A left strip is the same walk drawn the other way, so the renderer swaps
/// strips instead of mirroring a cheek mark. A different length would play two animations.
pub(super) fn check_left_strips(
    declared: &BTreeMap<String, DeclaredAnimation>,
    errors: &mut Vec<String>,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut taken: BTreeMap<&str, &str> = BTreeMap::new();
    for (name, animation) in declared {
        let Some(base) = &animation.left_of else {
            continue;
        };
        match declared.get(base) {
            _ if base == name => errors.push(format!("animation {name:?} is a left_of itself")),
            None => errors.push(format!(
                "animation {name:?} is a left_of {base:?}, which is not declared"
            )),
            Some(target) if target.left_of.is_some() => errors.push(format!(
                "animation {name:?} is a left_of {base:?}, itself a left strip; \
                 one strip faces each way"
            )),
            Some(target) if target.frames.len() != animation.frames.len() => errors.push(format!(
                "animation {name:?} has {} frames, and its base {base:?} has {}; \
                 a left strip is the same length as the animation it faces",
                animation.frames.len(),
                target.frames.len()
            )),
            Some(_) => match taken.insert(base.as_str(), name.as_str()) {
                Some(first) => errors.push(format!(
                    "animation {name:?} is a left_of {base:?}, which {first:?} already faces; \
                     one strip faces each way"
                )),
                None => pairs.push((name.clone(), base.clone())),
            },
        }
    }
    pairs
}

/// One frame's dimensions, from the PNG header alone. Bounded work whatever
/// the file claims, and it never inflates a compressed image, so size bounds
/// run before `AlphaMask::from_png` decodes a pixel.
fn art_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .map_err(|e| e.to_string())?;
    let info = reader.info();
    Ok((info.width, info.height))
}

/// Weight-0 Behaviors for each `ASKABLE_PRIMITIVES` entry whose name is free
/// and whose Primitive no declared Behavior already plays. Author entries win.
/// Exclusions (land/hold/talk/idle) live on that table's comment.
fn ensure_default_behaviors(declared: &mut BTreeMap<String, DeclaredBehavior>) {
    for &(name, primitive) in super::ASKABLE_PRIMITIVES {
        if declared.contains_key(name) {
            continue;
        }
        let already_played = declared
            .values()
            .any(|behavior| behavior.primitives.contains(&primitive));
        if already_played {
            continue;
        }
        declared.insert(
            name.to_string(),
            DeclaredBehavior {
                primitives: vec![primitive],
                then: None,
                weight: 0,
                trigger: None,
            },
        );
    }
}

/// Check that every Behavior can be played to an end. A chain that returns to
/// a Behavior already run would hold the sprite forever. The walk is iterative
/// and remembers what it walked, so a deep package is rejected rather than crashing the stack.
pub(super) fn resolve_behaviors(
    mut declared: BTreeMap<String, DeclaredBehavior>,
    errors: &mut Vec<String>,
) -> BTreeMap<String, Behavior> {
    ensure_default_behaviors(&mut declared);

    for (name, declaration) in &declared {
        if let Some(next) = &declaration.then {
            if !declared.contains_key(next) {
                errors.push(format!(
                    "behavior {name:?} follows {next:?}, \
                     which the package does not declare"
                ));
            }
        }
    }

    let mut walked: BTreeSet<&str> = BTreeSet::new();
    for start in declared.keys() {
        let mut path: Vec<&str> = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut current = start.as_str();

        loop {
            if walked.contains(current) {
                break;
            }
            if !seen.insert(current) {
                errors.push(format!(
                    "behavior {current:?} cannot terminate: {}",
                    loop_path(&path, current)
                ));
                break;
            }
            path.push(current);

            match declared.get(current).and_then(|next| next.then.as_deref()) {
                // A dangling `then` is reported above and stops the walk here.
                Some(next) if declared.contains_key(next) => current = next,
                _ => break,
            }
        }

        // Everything just walked either ends or is part of a loop already
        // reported, so no later chain has to walk it again.
        walked.extend(path);
    }

    declared
        .into_iter()
        .map(|(name, declaration)| {
            (
                name,
                Behavior {
                    primitives: declaration.primitives,
                    then: declaration.then,
                    weight: declaration.weight,
                    trigger: declaration.trigger,
                },
            )
        })
        .collect()
}

/// The loop a chain closes, from where it closes, for the author to read.
fn loop_path(path: &[&str], closes_at: &str) -> String {
    let from = path.iter().position(|name| *name == closes_at).unwrap_or(0);
    let chain: Vec<String> = path[from..]
        .iter()
        .chain([&closes_at])
        .map(|name| format!("{name:?}"))
        .collect();

    if chain.len() > SHOWN_LOOP_BEHAVIORS {
        format!(
            "{} and {} more",
            chain[..SHOWN_LOOP_BEHAVIORS].join(" -> "),
            chain.len() - SHOWN_LOOP_BEHAVIORS
        )
    } else {
        chain.join(" -> ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::tests::{art, assert_names, declaring, errors, load_manifest};
    use crate::character::{load, CHARACTER_MANIFEST_FILE, REQUIRED_ANIMATIONS};

    /// A 2x2 greyscale frame: a readable header with no alpha to mask.
    const GREYSCALE: &[u8] = include_bytes!("../../tests/fixtures/greyscale-2x2.png");

    /// A readable header with no alpha behind it. Rejected here, naming the
    /// frame, because nothing downstream reopens the art to discover it —
    /// this loader is the last thing that can refuse a Character.
    #[test]
    fn a_frame_no_mask_can_be_built_from_is_rejected_by_name() {
        let mut package = art();
        package.insert("sit-0.png".to_string(), GREYSCALE.to_vec());
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            declaring(&REQUIRED_ANIMATIONS).into_bytes(),
        );

        let errors = errors(load(&package));
        assert_names(&errors, "sit-0.png");
        assert_names(&errors, "RGBA");
    }

    #[test]
    fn a_behavior_that_cannot_terminate_is_rejected() {
        let manifest = format!(
            "{}[behaviors.pace]\nplay = [\"walk\"]\nthen = \"turn\"\n\
             [behaviors.turn]\nplay = [\"walk\"]\nthen = \"pace\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors,
            vec!["behavior \"pace\" cannot terminate: \
                 \"pace\" -> \"turn\" -> \"pace\""
                .to_string()],
            "the author is given the whole cycle"
        );
    }

    #[test]
    fn a_behavior_that_follows_itself_is_rejected() {
        let manifest = format!(
            "{}[behaviors.pace]\nplay = [\"walk\"]\nthen = \"pace\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_names(&errors, "cannot terminate");
        assert_names(&errors, "pace");
    }

    #[test]
    fn a_behavior_following_one_that_does_not_exist_is_rejected_by_name() {
        let manifest = format!(
            "{}[behaviors.greet]\nplay = [\"talk\"]\nthen = \"nap\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors,
            vec!["behavior \"greet\" follows \"nap\", \
                 which the package does not declare"
                .to_string()],
            "the author is told which behavior points at what"
        );
    }

    /// Hostile input: a chain deeper than an author would write, ending in a loop.
    /// Recursion would exhaust the stack instead of reporting it. Two thousand
    /// links is past a typical debug stack; the proof is the rejection, not the length of the TOML.
    #[test]
    fn a_very_deep_chain_ending_in_a_loop_is_rejected_rather_than_crashing() {
        const DEPTH: u32 = 2_000;
        let mut manifest = declaring(&REQUIRED_ANIMATIONS);
        for link in 0..DEPTH {
            manifest.push_str(&format!("[behaviors.b{link}]\nthen = \"b{}\"\n", link + 1));
        }
        manifest.push_str(&format!("[behaviors.b{DEPTH}]\nthen = \"b0\"\n"));

        let errors = errors(load_manifest(&manifest));
        assert_names(&errors, "cannot terminate");
    }

    #[test]
    fn a_variant_of_nothing_a_variant_or_a_once_loop_is_rejected_by_name() {
        let manifest = format!(
            "{}\
             [animations.a]\nframes = [\"idle-0.png\"]\nvariant_of = \"dance\"\n\
             [animations.b]\nframes = [\"idle-0.png\"]\nvariant_of = \"idle\"\n\
             [animations.c]\nframes = [\"idle-0.png\"]\nvariant_of = \"b\"\n\
             [animations.d]\nframes = [\"idle-0.png\"]\nloop = \"once\"\nvariant_of = \"walk\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));
        assert_eq!(
            errors,
            vec![
                "animation \"a\" is a variant_of \"dance\", which is not declared".to_string(),
                "animation \"c\" is a variant_of \"b\", itself a variant; \
                 variants ring one base"
                    .to_string(),
                "animation \"d\" and its base \"walk\" must both loop \"forever\"; \
                 a drawn variant plays until the engine asks for something else"
                    .to_string(),
            ],
        );
    }

    /// The wording is the behavior, not the refusal: a variant of itself is also
    /// a variant of a variant, so dropping the self-reference guard still rejects
    /// it with the ring message, which names a mistake the author did not make.
    #[test]
    fn an_animation_that_is_a_variant_of_itself_is_rejected_by_name() {
        let manifest = format!(
            "{}[animations.shimmy]\nframes = [\"idle-0.png\"]\nvariant_of = \"shimmy\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors,
            vec!["animation \"shimmy\" is a variant_of itself".to_string()],
        );
    }

    #[test]
    fn a_frame_that_is_not_in_the_package_is_rejected_by_name() {
        let manifest = declaring(&REQUIRED_ANIMATIONS).replace("sit-0.png", "sit-99.png");
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors,
            vec!["animation \"sit\" frame \"sit-99.png\" is not in the package".to_string()],
            "the author is told which frame of which Animation is absent"
        );
    }

    #[test]
    fn a_frame_that_is_not_readable_art_is_rejected_by_name() {
        let mut package = art();
        package.insert("sit-0.png".to_string(), b"MZ\x90\x00 not a PNG".to_vec());
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            declaring(&REQUIRED_ANIMATIONS).into_bytes(),
        );

        let errors = errors(load(&package));
        assert_names(&errors, "sit-0.png");
        assert_names(&errors, "is not readable art");
    }

    #[test]
    fn an_animation_whose_frames_disagree_on_size_is_rejected() {
        let mut package = art();
        package.insert("walk-1.png".to_string(), png_bytes(3, 3));
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            declaring(&REQUIRED_ANIMATIONS)
                .replace(
                    "frames = [\"walk-0.png\"]",
                    "frames = [\"walk-0.png\", \"walk-1.png\"]",
                )
                .into_bytes(),
        );

        let errors = errors(load(&package));
        assert_eq!(
            errors,
            vec![
                "animation \"walk\" frame \"walk-1.png\" is 3x3, and its first frame \
                 is 2x2; every frame is one size"
                    .to_string()
            ],
            "the author is told both sizes and which frame disagrees"
        );
    }

    /// Hostile input: a header claiming a frame no screen could hold. Nothing
    /// decompresses it here, but a renderer that trusts the declared size
    /// allocates it.
    #[test]
    fn a_frame_too_large_to_be_a_sprite_is_rejected_by_name() {
        let mut package = art();
        package.insert("sit-0.png".to_string(), png_bytes(100_000, 1));
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            declaring(&REQUIRED_ANIMATIONS).into_bytes(),
        );

        let errors = errors(load(&package));
        assert_eq!(
            errors,
            vec![
                "animation \"sit\" frame \"sit-0.png\" is 100000x1, and no side of a \
                 frame may be over 1024 pixels"
                    .to_string()
            ],
            "the author is told which frame is implausible and how big a frame may be"
        );
    }

    /// Hostile input: per-frame and per-Animation caps leave total masks unbounded,
    /// and a shared frame is charged once. The on-budget package is 256 megapixels.
    /// Passing it uses the same sum as the refusal, so that package is not loaded.
    #[test]
    fn a_character_whose_frames_outweigh_the_budget_is_rejected() {
        let frame = png_bytes(MAX_FRAME_SIDE, MAX_FRAME_SIDE);
        let pixels = u64::from(MAX_FRAME_SIDE) * u64::from(MAX_FRAME_SIDE);
        let budget = (MAX_CHARACTER_PIXELS / pixels) as usize;

        // Every required Animation shares one frame, so `count` frames of art
        // are `count` masks however many Animations name them.
        let declaring_distinct = |count: usize| {
            let mut package: PackageBytes = (0..count)
                .map(|i| (format!("big-{i}.png"), frame.clone()))
                .collect();
            let mut manifest = String::from("name = \"Blip\"\n");
            for animation in REQUIRED_ANIMATIONS {
                manifest.push_str(&format!(
                    "[animations.{animation}]\nframes = [\"big-0.png\"]\n"
                ));
            }
            let rest: Vec<String> = (1..count).map(|i| format!("\"big-{i}.png\"")).collect();
            manifest.push_str(&format!(
                "[animations.wave]\nframes = [{}]\n",
                rest.join(", ")
            ));
            package.insert(CHARACTER_MANIFEST_FILE.to_string(), manifest.into_bytes());
            package
        };

        let over = errors(load(&declaring_distinct(budget + 1)));
        assert_eq!(
            over,
            vec![format!(
                "the package's frames are {} pixels in all, over the \
                 {MAX_CHARACTER_PIXELS}-pixel limit",
                (budget as u64 + 1) * pixels
            )],
            "the author is told how much art they declared and how much a Character may hold"
        );
    }

    /// A left strip stands in for its base when the sprite travels left, so a
    /// missing base, a strip of a strip, or a different length — anything that
    /// would play the two differently — is the author's to fix before the package loads.
    #[test]
    fn a_left_strip_of_nothing_itself_a_strip_or_another_length_is_rejected_by_name() {
        let manifest = format!(
            "{}\
             [animations.a]\nframes = [\"idle-0.png\"]\nleft_of = \"dance\"\n\
             [animations.b]\nframes = [\"idle-0.png\"]\nleft_of = \"b\"\n\
             [animations.c]\nframes = [\"idle-0.png\"]\nleft_of = \"walk\"\n\
             [animations.d]\nframes = [\"idle-0.png\"]\nleft_of = \"c\"\n\
             [animations.e]\nframes = [\"idle-0.png\", \"idle-0.png\"]\nleft_of = \"sit\"\n\
             [animations.f]\nframes = [\"idle-0.png\"]\nleft_of = \"walk\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );

        assert_eq!(
            errors(load_manifest(&manifest)),
            vec![
                "animation \"a\" is a left_of \"dance\", which is not declared".to_string(),
                "animation \"b\" is a left_of itself".to_string(),
                "animation \"d\" is a left_of \"c\", itself a left strip; \
                 one strip faces each way"
                    .to_string(),
                "animation \"e\" has 2 frames, and its base \"sit\" has 1; \
                 a left strip is the same length as the animation it faces"
                    .to_string(),
                "animation \"f\" is a left_of \"walk\", which \"c\" already faces; \
                 one strip faces each way"
                    .to_string(),
            ],
            "every mistake at once, each naming the declaration at fault"
        );
    }

    /// The size a strip is drawn at is read from its art rather than declared,
    /// so it is the loader that catches art that would resize the sprite when
    /// it turned round.
    #[test]
    fn a_left_strip_whose_art_is_a_different_size_is_rejected() {
        let mut package = art();
        package.insert("wide-0.png".to_string(), png_bytes(4, 4));
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            format!(
                "{}[animations.walk-left]\nframes = [\"wide-0.png\"]\nleft_of = \"walk\"\n",
                declaring(&REQUIRED_ANIMATIONS)
            )
            .into_bytes(),
        );

        assert_eq!(
            errors(load(&package)),
            vec![
                "animation \"walk-left\" frames are 4x4, and its base \"walk\" is 2x2; \
                 a left strip is the same size as the animation it faces"
                    .to_string()
            ]
        );
    }

    /// A valid PNG of the given size, for the tests the fixture cannot serve.
    /// The encoder is already a dependency of the renderer.
    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header is writable");
        writer
            .write_image_data(&vec![0; (width * height * 4) as usize])
            .expect("image data is writable");
        writer.finish().expect("PNG is finishable");
        bytes
    }
}
