//! The Character Package loader: bytes in, either a validated Character or the
//! list of mistakes its author has to fix.
//!
//! The one validator on the load path. A loaded Character carries everything
//! rendering needs — each distinct frame's PNG and its alpha mask, decoded
//! here — so nothing downstream reopens the art, and nothing downstream can
//! refuse a Character this module declared valid. Every content bound lives in
//! the constants below; the reader's own limits (bytes, files, depth) guard
//! the I/O before these bytes exist and live with it in the Shell.
//!
//! Pure and synchronous, like the rest of the Engine seam. Every byte is handed
//! to it, so it opens nothing, reaches no platform and cannot be slowed down by
//! a disk. That is also what lets a hostile package be tested as a map of file
//! bytes.
//!
//! A Character Package is untrusted input twice over: an author's mistake and a
//! deliberate attack arrive through the same door, and a Personality Prompt
//! reaches a model that can talk to an agent Harness. So the errors are the
//! product here as much as the Character is. Every rejection names the
//! declaration at fault, because the author is meant to fix their package
//! without reading this file.
//!
//! Two properties keep a package from enabling anything. The Character Manifest
//! rejects every declaration it does not know, so no package can invent a key
//! that grants a capability. And the Personality Prompt is prose in a file of
//! its own, never a declaration, so it can describe a Character that flies
//! without the Character gaining flight.
//!
//! The Character Manifest is TOML (ADR-0015): a name, a table per Animation,
//! a table per Behavior, an optional `[source]` saying where the art came
//! from, and an optional `[director]` for how proactive model calls space
//! themselves. TOML replaces only the container — the `when`
//! condition is still this module's own small language, checked here.
//! `docs/DEVELOPMENT.md` documents it for authors; in short:
//!
//! ```text
//! name = "Blip"
//!
//! [source]
//! art = "Blip, cut from the Blipworks shimeji pack"
//! url = "https://example.invalid/blip"
//! license = "CC BY 4.0"
//!
//! [animations.idle]
//! frames = ["idle-0.png", "idle-1.png"]
//! fps = 12
//!
//! [animations.land]
//! frames = ["land-0.png"]
//! loop = "once"
//!
//! [animations.walk-left]
//! frames = ["walk-left-0.png", "walk-left-1.png"]
//! left_of = "walk"
//!
//! [behaviors.greet]
//! play = ["react", "talk"]
//! then = "settle"
//! weight = 30
//! when = "idle over 2m"
//! ```
//!
//! Frame count and frame size are read from the art instead of declared, since
//! a declared size can disagree with the art and a derived one cannot.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::director::Seeded;
use crate::overlay::AlphaMask;

mod manifest;
mod resolve;
use manifest::parse;
use resolve::{check_left_strips, check_variants, resolve_animations, resolve_behaviors};

/// A Character Package as bytes: file name to contents. The Shell does the
/// reading; the loader never touches a path.
pub type PackageBytes = BTreeMap<String, Vec<u8>>;

/// The Character Manifest, at the root of the package.
pub const CHARACTER_MANIFEST_FILE: &str = "character.manifest";

/// The Personality Prompt, at the root of the package. Optional: a Character
/// with no prompt still has its whole idle life, which is local and model-free.
pub const PERSONALITY_FILE: &str = "personality.txt";

/// The animations every Character must supply, fixed at nine so a hobbyist
/// package stays an evening's drawing. Any other Animation a package declares
/// is optional: used when present, absent silently when not.
pub const REQUIRED_ANIMATIONS: [&str; 9] = [
    "idle", "walk", "fall", "land", "sit", "sleep", "react", "talk", "hold",
];

/// How long a Personality Prompt may be, in characters. Untrusted text in every
/// Character Prompt the Director sends, so an unbounded one spends tokens and
/// buries sensing under prose. Generous enough for a paragraph of personality.
pub(crate) const PERSONALITY_LIMIT: usize = 2000;

/// How large a Character Manifest may be, in bytes. Every malformed declaration
/// costs an error String, so junk that compresses to kilobytes spends gigabytes
/// being rejected. Generous for one short line per Animation and Behavior.
pub(crate) const MANIFEST_LIMIT: usize = 1024 * 1024;

/// Frames per second an Animation plays at when it does not say.
/// Eight is the cadence the Engine already runs every Animation at.
pub(crate) const DEFAULT_FPS: u32 = 8;

/// How likely a Behavior, or a member of a variant ring, is to be picked when
/// it does not say. Not zero. Ten rather than one so a declaration can go
/// down: a default of one is also the floor.
pub(crate) const DEFAULT_WEIGHT: u32 = 10;

/// The scale the renderer uses when a Character does not say.
/// Four is what the shipped pixel-art Characters have always been drawn at.
pub(crate) const DEFAULT_SCALE: u32 = 4;

/// How proactive model-call waits grow when no one addresses the character.
/// `wait * model_base.pow(model_power)` after each proactive model call.
/// Two and one is the doubling Pace already had.
pub const DEFAULT_MODEL_BASE: u32 = 2;
pub const DEFAULT_MODEL_POWER: u32 = 1;

/// How hard a Character falls, as a multiple of the Engine's default gravity.
/// Never NaN, so equality is total. Zero is refused: no State floats yet, and a
/// sprite with no gravity would never leave Falling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GravityScale(f64);

impl Eq for GravityScale {}

impl GravityScale {
    pub const DEFAULT: Self = Self(1.0);
    /// A quarter at most doubles how long a fall takes and lets a Jump peak 450
    /// points up, half a 900-point laptop display. Nearer zero is floating.
    pub const MIN: f64 = 0.25;
    /// Twice the default, where the planned Settings slider tops out.
    pub const MAX: f64 = 2.0;

    /// `None` outside `MIN..=MAX`, NaN included.
    pub fn new(scale: f64) -> Option<Self> {
        (Self::MIN..=Self::MAX)
            .contains(&scale)
            .then_some(Self(scale))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

/// The largest integer factor a Character may ask to be drawn at.
/// Display scaling is small integer factors; art wanting to be bigger on
/// screen should be authored bigger instead.
pub(crate) const MAX_SCALE: u32 = 4;

/// The largest either side of a frame may be, in pixels. A PNG header costs the
/// same few dozen bytes whatever size it claims, so a 100000x100000 frame spends
/// forty gigabytes. 1024 is generous even for art at twice Retina size.
pub(crate) const MAX_FRAME_SIDE: u32 = 1024;

/// The most frames an Animation may declare. A frame reference costs eight bytes
/// of manifest and buys a whole copy of the art, so a fitting manifest can still
/// ask for terabytes. A hand-drawn Animation is a handful of frames.
pub(crate) const MAX_FRAMES: usize = 256;

/// The most pixels all of a Character's distinct frames may add up to.
/// `MAX_FRAMES` bounds one Animation; this is 256 frames at max size, because
/// every distinct frame buys an alpha mask held while loaded.
pub(crate) const MAX_CHARACTER_PIXELS: u64 =
    256 * (MAX_FRAME_SIDE as u64) * (MAX_FRAME_SIDE as u64);

/// The fastest an Animation may declare. Past display refresh the extra frames
/// are never seen, and a four-figure fps is either a mistake or an attempt to
/// make the renderer thrash.
pub(crate) const MAX_FPS: u32 = 60;

/// Alpha at or above this counts as drawn, when a frame's mask is built.
/// A threshold rather than "alpha > 0" so anti-aliased edges on hand-drawn art
/// do not grow an invisible one-pixel border that swallows clicks.
pub const ALPHA_THRESHOLD: u8 = 128;

/// A unit of motion or expression the Engine owns. A Character composes these
/// into Behaviors and can never define one. Anything a Character needs beyond
/// them is a Primitive added here for everyone, never a scripting runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Primitive {
    Idle,
    Walk,
    Land,
    Sit,
    Sleep,
    React,
    Talk,
    /// Gripping a moving Perch. The Engine plays this itself; a Character may
    /// also compose it.
    Hold,
    /// Steer walk velocity toward the cursor's x along the ground. The cursor
    /// is up on the screen; the character chases its shadow on the floor.
    Chase,
    /// Leave the ground under the sprite's own power. The launch is an upward
    /// velocity and nothing more. `Falling` carries the arc and the existing
    /// landing path ends it, so a jump reuses the Throw's physics.
    Jump,
}

const PRIMITIVES: [(&str, Primitive); 10] = [
    ("idle", Primitive::Idle),
    ("walk", Primitive::Walk),
    ("land", Primitive::Land),
    ("sit", Primitive::Sit),
    ("sleep", Primitive::Sleep),
    ("react", Primitive::React),
    ("talk", Primitive::Talk),
    ("hold", Primitive::Hold),
    ("chase", Primitive::Chase),
    ("jump", Primitive::Jump),
];

/// Primitives Chat and MCP may ask for by Behavior name via `play_behavior`.
/// Off the table: `land`/`hold` (Engine landing / perch grip), `talk` (Engine
/// plays during speech), `idle` (ambient; not an ask).
pub(super) const ASKABLE_PRIMITIVES: &[(&str, Primitive)] = &[
    ("walk", Primitive::Walk),
    ("sit", Primitive::Sit),
    ("sleep", Primitive::Sleep),
    ("react", Primitive::React),
    ("chase", Primitive::Chase),
    ("jump", Primitive::Jump),
];

/// A named frame sequence and how it plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Animation {
    /// Frame file names in play order, resolvable against the same
    /// `PackageBytes` the Character was loaded from.
    pub frames: Vec<String>,
    /// Width and height of every frame, in pixels, read from the art.
    pub frame_size: (u32, u32),
    pub fps: u32,
    /// Whether the Animation repeats or holds its last frame.
    pub looping: bool,
    /// Names of the Animations that declared `variant_of` this one, in name
    /// order. Starting this Animation draws one of this and those by weight
    /// until the engine asks for something else.
    pub variants: Vec<String>,
    /// The Animation drawn in this one's place when the sprite travels left,
    /// from the package's `left_of`. `None` mirrors this Animation's own art.
    pub left_strip: Option<String>,
    /// This Animation's share of the variant ring, `DEFAULT_WEIGHT` when the
    /// manifest does not say. An Animation in no ring carries the default all
    /// the same, and nothing reads it.
    pub weight: u32,
}

impl Animation {
    /// Which frame is on screen `elapsed_ms` after this Animation started.
    /// The renderer needs no clock of its own because this is the whole of
    /// frame selection.
    pub fn frame_at(&self, elapsed_ms: u32) -> usize {
        // Validation rejects a package with either of these, so this guards the
        // struct rather than the format: the fields are public, and a divide by
        // zero in the renderer would take the frame loop with it.
        let count = self.frames.len() as u64;
        if self.fps == 0 || count == 0 {
            return 0;
        }

        // Multiplying before dividing keeps the cadence exact for every fps rather
        // than only for the ones that divide a second evenly. 12fps is 83.33ms a
        // frame, and rounding it to 83 drifts a frame every three seconds.
        let elapsed = u64::from(elapsed_ms) * u64::from(self.fps) / 1000;
        let index = if self.looping {
            elapsed % count
        } else {
            elapsed.min(count - 1)
        };
        index as usize
    }
}

/// What must be true before a Behavior may be picked.
///
/// An unknown trigger is rejected rather than quietly never firing.
///
/// ponytail: no time-of-day condition, though the Director's context carries
/// the time. `std` has no local time, and a trigger written as "22 to 6" that
/// silently meant UTC would be worse than no trigger at all. Add it when a
/// local hour reaches the Engine seam.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The user has been away for longer than this.
    IdleOver(Duration),
    /// The user has been away for less than this — freshly back, or still here.
    IdleUnder(Duration),
    /// This application is frontmost, by the name the platform reports.
    Frontmost(String),
}

/// A named sequence of Primitives, declared as data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Behavior {
    pub primitives: Vec<Primitive>,
    /// A Behavior that follows this one, by name. Chains are validated to
    /// terminate, so no Character can put the sprite in a loop it never leaves.
    pub then: Option<String>,
    /// How likely the Static Director is to pick this Behavior against its
    /// siblings. Zero takes it out of the running entirely, leaving a Behavior
    /// only something else can reach — a chain, a Poke, or a model.
    pub weight: u32,
    /// When this Behavior may be picked, or `None` for any time at all.
    pub trigger: Option<Trigger>,
}

/// One distinct frame's art, decoded once at load.
/// Two readers need the pixels and neither can afford to open a file per tick:
/// the hit-test and the webview. A frame two Animations share is one `Art`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Art {
    /// The frame as the package shipped it, for whatever encoding the
    /// renderer hands its webview.
    pub png: Vec<u8>,
    pub mask: AlphaMask,
}

/// A Character that has been validated: every required Animation is present,
/// every frame is art the renderer holds decoded, and every Behavior is
/// playable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Character {
    pub name: String,
    /// The Personality Prompt, or empty when the package ships none.
    pub personality: String,
    pub animations: BTreeMap<String, Animation>,
    pub behaviors: BTreeMap<String, Behavior>,
    /// Every distinct frame any Animation names, by the name it is named.
    pub art: BTreeMap<String, Art>,
    /// The renderer smooths this art when scaling instead of keeping hard
    /// pixels. The manifest `render_mode`, for Characters whose frames are
    /// drawn rather than gridded.
    pub smooth: bool,
    /// The integer factor the renderer draws the art at.
    pub scale: u32,
    /// Proactive model-call wait grows by `model_base.pow(model_power)`.
    pub model_base: u32,
    pub model_power: u32,
    /// How the Character reacts when the cursor enters its Near radius.
    pub near_reaction: CursorReaction,
    /// How the Character reacts to a cursor rushing at it.
    pub rush_reaction: CursorReaction,
    pub gravity_scale: GravityScale,
    /// Where the art came from, when the package says. `None` is silence, not
    /// a claim: a package that declares no `[source]` gets no attribution
    /// printed for it rather than one implying the art is this repository's.
    pub source: Option<Source>,
}

/// Where a Character's art came from, as the package declares it. Prose, not
/// an identifier, so the package and the art can answer differently. `license`
/// is required with `[source]`: a gallery can lose the caveat by omission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// What the art is and where it came from.
    pub art: String,
    /// Where it came from, when there is an address to cite.
    pub url: Option<String>,
    /// The license, or the sentence saying none is declared.
    pub license: String,
}

/// How a Character reacts to the cursor entering its Near radius or rushing at it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorReaction {
    /// Keep doing whatever it was doing.
    #[default]
    Indifferent,
    /// Speak (`talk`).
    Speak,
    /// Turn to face the cursor (`facing` toward it; art already mirrors).
    Face,
    /// Walk toward the cursor.
    Toward,
    /// Walk away from the cursor.
    Away,
    /// Play `react`.
    React,
}

/// What the renderer needs to draw one tick.
pub struct Drawn<'a> {
    /// The Animation actually drawing: the one asked for, its fallback, or the
    /// variant-ring member the draw landed on. The renderer indexes art by this
    /// name, never by the one the engine asked with.
    pub animation: &'a str,
    pub mask: &'a AlphaMask,
    /// The frame's size in pixels, before any scaling.
    pub frame_size: (u32, u32),
    /// Which frame of the Animation is on screen.
    pub index: usize,
    /// Whether this art is drawn flipped horizontally. The hit-test and the
    /// webview both read it, so the clickable region is the visible art.
    /// Leftward travel mirrors unless a left strip already faces left.
    pub mirrored: bool,
}

impl Character {
    /// How tall this Character stands, in points: the idle Animation's frame
    /// at this Character's scale. The art hangs above the feet, so this is the
    /// room a Perch near the top has to leave. Idle is the usual drawn size.
    pub fn sprite_height(&self) -> f64 {
        self.draw("idle", 0, 0, 1.0)
            .map_or(0.0, |drawn| f64::from(drawn.frame_size.1))
            * f64::from(self.scale)
    }

    /// Which frame of `animation` is on screen `animation_ms` after it started.
    /// `variant_draw` is held while it plays; zero draws the base. `None` rather
    /// than a substitute, because the renderer was told the name it asked for.
    pub fn draw(
        &self,
        animation: &str,
        animation_ms: u32,
        variant_draw: u64,
        facing: f64,
    ) -> Option<Drawn<'_>> {
        let (name, animation) = self.resolve(animation, variant_draw)?;
        // Strip first, mirror second, and never both: an Animation that draws
        // its own leftward art is handed over as authored, and one that does
        // not is mirrored.
        let (name, animation, mirrored) = match animation
            .left_strip
            .as_deref()
            .filter(|_| facing < 0.0)
            .and_then(|left| self.animations.get_key_value(left))
        {
            Some((left_name, left)) => (left_name.as_str(), left, false),
            None => (name, animation, facing < 0.0),
        };
        let index = animation.frame_at(animation_ms);
        let art = self.art.get(animation.frames.get(index)?)?;

        Some(Drawn {
            animation: name,
            mask: &art.mask,
            frame_size: animation.frame_size,
            index,
            mirrored,
        })
    }

    /// Which Animation actually draws: the one asked for, its fallback, or the
    /// variant-ring member the draw weighs out. Same mixer as the Static
    /// Director, and members in `BTreeMap` order: the draw walks a running total.
    fn resolve(&self, requested: &str, draw: u64) -> Option<(&str, &Animation)> {
        let (name, base) = match self.animations.get_key_value(requested) {
            Some(found) => found,
            None => {
                let (_, fallback) = OPTIONAL_FALLBACKS
                    .iter()
                    .find(|(optional, _)| *optional == requested)?;
                self.animations.get_key_value(*fallback)?
            }
        };
        if base.variants.is_empty() {
            return Some((name.as_str(), base));
        }

        let members: Vec<(&str, &Animation)> = std::iter::once((name.as_str(), base))
            .chain(base.variants.iter().filter_map(|variant| {
                self.animations
                    .get_key_value(variant)
                    .map(|(name, animation)| (name.as_str(), animation))
            }))
            .collect();
        let weights: Vec<u32> = members.iter().map(|(_, member)| member.weight).collect();
        // A ring of nothing but weightless art still has to draw something,
        // and the base is what the engine asked for.
        let picked = Seeded::new(draw).pick_index(&weights).unwrap_or(0);
        members.get(picked).copied()
    }
}

/// Optional Animations, and what draws when a package does not declare them:
/// used when present, and absent silently, never as a missing sprite.
const OPTIONAL_FALLBACKS: [(&str, &str); 3] = [
    ("climb", "walk"),
    ("grab", "fall"),
    // The Required Animation Set is closed at nine, so a jump is optional art.
    // A Character without it rises and falls in its `fall`.
    ("jump", "fall"),
];

/// How many Behaviors of a loop a rejection spells out before it stops.
/// A bound on the error rather than on the package: an author wants to see
/// where their loop closes, and an awkward package can chain twenty thousand.
const SHOWN_LOOP_BEHAVIORS: usize = 8;

/// Validate a Character Package.
/// Returns every error at once rather than the first, so an author fixes their
/// package in one pass instead of one rejection per attempt.
pub fn load(package: &PackageBytes) -> Result<Character, Vec<String>> {
    let mut errors = Vec::new();

    // Nothing else in the package means anything without the Character
    // Manifest, so these are the only rejections reported on their own.
    let Some(bytes) = package.get(CHARACTER_MANIFEST_FILE) else {
        return Err(vec![format!(
            "the package contains no {CHARACTER_MANIFEST_FILE}"
        )]);
    };
    let Ok(manifest) = std::str::from_utf8(bytes) else {
        return Err(vec![format!("{CHARACTER_MANIFEST_FILE} is not UTF-8 text")]);
    };
    if manifest.len() > MANIFEST_LIMIT {
        return Err(vec![format!(
            "{CHARACTER_MANIFEST_FILE} is {} bytes, over the {MANIFEST_LIMIT}-byte limit",
            manifest.len()
        )]);
    }

    // A manifest that is not TOML is one error, not a cascade: nothing was
    // declared, so reporting a missing name and eight missing Animations on
    // top would be reporting mistakes the author has not made.
    let Some(declared) = parse(manifest, &mut errors) else {
        return Err(errors);
    };

    if declared.name.is_none() {
        errors.push("the package declares no name".to_string());
    }
    for required in REQUIRED_ANIMATIONS {
        if !declared.animations.contains_key(required) {
            errors.push(format!(
                "the package declares no {required:?} animation, \
                 which every Character must supply"
            ));
        }
    }

    let personality = personality(package, &mut errors);
    let variant_pairs = check_variants(&declared.animations, &mut errors);
    let left_pairs = check_left_strips(&declared.animations, &mut errors);
    let (mut animations, art) = resolve_animations(package, declared.animations, &mut errors);
    // Linked after resolution so a variant whose frames failed never joins a
    // ring; its own declaration errors already say why.
    for (variant, base) in variant_pairs {
        if animations.contains_key(&variant) {
            if let Some(base) = animations.get_mut(&base) {
                base.variants.push(variant);
            }
        }
    }
    // Linked after resolution for the same reason variants are. Frame size is
    // compared here because it is read from the art: the strips replace each
    // other, so a different size would move the sprite when it turned round.
    for (strip, base) in left_pairs {
        let Some(size) = animations.get(&strip).map(|strip| strip.frame_size) else {
            continue;
        };
        if let Some(faced) = animations.get_mut(&base) {
            if faced.frame_size == size {
                faced.left_strip = Some(strip);
            } else {
                errors.push(format!(
                    "animation {strip:?} frames are {}x{}, and its base {base:?} is {}x{}; \
                     a left strip is the same size as the animation it faces",
                    size.0, size.1, faced.frame_size.0, faced.frame_size.1
                ));
            }
        }
    }
    let behaviors = resolve_behaviors(declared.behaviors, &mut errors);

    match (errors.is_empty(), declared.name) {
        (true, Some(name)) => Ok(Character {
            name,
            personality,
            animations,
            behaviors,
            art,
            smooth: declared.smooth.unwrap_or(false),
            scale: declared.scale.unwrap_or(DEFAULT_SCALE),
            model_base: declared.model_base.unwrap_or(DEFAULT_MODEL_BASE),
            model_power: declared.model_power.unwrap_or(DEFAULT_MODEL_POWER),
            near_reaction: declared.near_reaction.unwrap_or_default(),
            rush_reaction: declared.rush_reaction.unwrap_or_default(),
            gravity_scale: declared.gravity_scale.unwrap_or(GravityScale::DEFAULT),
            source: declared.source,
        }),
        _ => Err(errors),
    }
}

fn personality(package: &PackageBytes, errors: &mut Vec<String>) -> String {
    let Some(bytes) = package.get(PERSONALITY_FILE) else {
        return String::new();
    };
    let Ok(prompt) = std::str::from_utf8(bytes) else {
        errors.push(format!("{PERSONALITY_FILE} is not UTF-8 text"));
        return String::new();
    };

    let length = prompt.chars().count();
    if length > PERSONALITY_LIMIT {
        errors.push(format!(
            "{PERSONALITY_FILE} is {length} characters, over the {PERSONALITY_LIMIT}-character limit"
        ));
    }
    prompt.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::overlay::SpriteRect;

    use std::collections::BTreeSet;

    /// A 2x2 RGBA PNG whose top-left pixel is transparent. Art a renderer can
    /// open is all the loader asks of a frame.
    const FRAME: &[u8] = include_bytes!("../tests/fixtures/alpha-2x2.png");

    /// A 2x2 RGBA frame with every pixel drawn, so one lookup tells a mask
    /// built from it apart from one built from `FRAME`.
    const SOLID: &[u8] = include_bytes!("../tests/fixtures/opaque-2x2.png");

    /// One frame file per required Animation, plus a `wave` no manifest has to
    /// declare. Art nothing declares is ignored, the same as a README would be.
    pub(super) fn art() -> PackageBytes {
        REQUIRED_ANIMATIONS
            .iter()
            .chain(["wave"].iter())
            .map(|name| (format!("{name}-0.png"), FRAME.to_vec()))
            .collect()
    }

    /// A Character Manifest declaring exactly `animations`, one frame each.
    pub(super) fn declaring(animations: &[&str]) -> String {
        let mut manifest = String::from("name = \"Blip\"\n");
        for animation in animations {
            manifest.push_str(&format!(
                "[animations.{animation}]\nframes = [\"{animation}-0.png\"]\n"
            ));
        }
        manifest
    }

    pub(super) fn load_manifest(manifest: &str) -> Result<Character, Vec<String>> {
        let mut package = art();
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            manifest.as_bytes().to_vec(),
        );
        load(&package)
    }

    /// The errors, or a failure naming what loaded instead.
    pub(super) fn errors(result: Result<Character, Vec<String>>) -> Vec<String> {
        match result {
            Ok(character) => panic!("expected rejection, loaded {}", character.name),
            Err(errors) => errors,
        }
    }

    pub(super) fn assert_names(errors: &[String], offender: &str) {
        assert!(
            errors.iter().any(|error| error.contains(offender)),
            "no error names {offender:?}: {errors:#?}"
        );
    }

    fn animation(frames: usize, fps: u32, looping: bool) -> Animation {
        Animation {
            frames: (0..frames).map(|i| format!("f-{i}.png")).collect(),
            frame_size: (2, 2),
            fps,
            looping,
            variants: Vec::new(),
            left_strip: None,
            weight: DEFAULT_WEIGHT,
        }
    }

    #[test]
    fn a_frame_is_held_for_as_long_as_the_declared_fps_says() {
        let walk = animation(4, 8, true);

        assert_eq!(walk.frame_at(0), 0);
        assert_eq!(walk.frame_at(124), 0, "still inside the first frame");
        assert_eq!(walk.frame_at(125), 1, "the second frame begins");
        assert_eq!(walk.frame_at(374), 2);
        assert_eq!(walk.frame_at(375), 3, "the last frame of the strip");
    }

    #[test]
    fn a_declared_fps_changes_how_fast_the_same_strip_plays() {
        let slow = animation(4, 2, true);
        let fast = animation(4, 24, true);

        // Half a second in, a 2fps idle is on its second frame and a 24fps one
        // has been round the strip three times.
        assert_eq!(slow.frame_at(500), 1);
        assert_eq!(fast.frame_at(500), 0);
        assert_eq!(fast.frame_at(542), 1);
    }

    #[test]
    fn a_looping_animation_wraps_at_the_end_of_the_strip() {
        let walk = animation(4, 8, true);

        assert_eq!(walk.frame_at(500), 0, "half a second in, back to the start");
        assert_eq!(walk.frame_at(625), 1);
        // Twenty strips later, on the frame it started on.
        assert_eq!(walk.frame_at(10_000), 0);
        assert!(
            walk.frame_at(u32::MAX) < 4,
            "an Animation left playing for seven weeks still names a real frame"
        );
    }

    #[test]
    fn a_once_animation_holds_its_last_frame() {
        let land = animation(3, 8, false);

        assert_eq!(land.frame_at(250), 2, "the last frame");
        assert_eq!(land.frame_at(375), 2, "and it stays there");
        assert_eq!(land.frame_at(u32::MAX), 2);
    }

    #[test]
    fn a_single_frame_animation_is_always_on_its_only_frame() {
        assert_eq!(animation(1, 8, true).frame_at(u32::MAX), 0);
        assert_eq!(animation(1, 8, false).frame_at(u32::MAX), 0);
    }

    #[test]
    fn a_minimal_package_loads() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("package is valid");

        assert_eq!(character.name, "Blip");
        assert_eq!(character.personality, "");
        assert_eq!(
            character
                .animations
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            REQUIRED_ANIMATIONS
                .iter()
                .map(|name| name.to_string())
                .collect::<BTreeSet<_>>(),
            "exactly the Required Animation Set"
        );

        let idle = &character.animations["idle"];
        assert_eq!(idle.frames, vec!["idle-0.png"]);
        assert_eq!(idle.frame_size, (2, 2), "read from the art");
        assert_eq!(
            idle.fps, 8,
            "eight frames a second is the cadence a Character looks right at, \
             and what a package that declares no fps gets"
        );
        assert!(
            idle.looping,
            "an Animation repeats unless it says otherwise"
        );
    }

    /// Jump is a primitive Behavior every Character can be asked to play.
    /// A package that never writes `[behaviors.jump]` still gets one at load,
    /// weight zero until dedicated art earns ambient use.
    #[test]
    fn an_undeclared_jump_is_still_playable() {
        use crate::engine::{BehaviorProposal, Engine, Point, WorldSnapshot};

        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("package is valid");
        let jump = character
            .behaviors
            .get("jump")
            .expect("every Character can be asked to jump");
        assert_eq!(jump.primitives, vec![Primitive::Jump]);
        assert_eq!(jump.weight, 0, "ambient off until the package earns it");

        let ground = WorldSnapshot {
            displays: vec![crate::engine::Rect {
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
            engine.tick(&ground);
        }

        let proposed = WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "jump".to_string(),
                dialogue: None,
            }),
            ..ground.clone()
        };
        let launch = engine.tick(&proposed);
        assert_eq!(
            launch.behavior.as_deref(),
            Some("jump"),
            "the proposal was taken: {launch:?}"
        );
        let launched = engine.tick(&ground);
        assert!(
            launched.velocity.y < 0.0,
            "off the floor upward: {launched:?}"
        );
    }

    /// An author who writes jump keeps their weight; the engine default must
    /// not overwrite a Character that already hops on its own.
    #[test]
    fn a_declared_jump_keeps_its_weight() {
        let manifest = format!(
            "{}[behaviors.jump]\nplay = [\"jump\"]\nweight = 7\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("package is valid");
        assert_eq!(
            character.behaviors["jump"].weight, 7,
            "author-declared jump wins over the engine default"
        );
        assert_eq!(
            character.behaviors["jump"].primitives,
            vec![Primitive::Jump]
        );
    }

    /// Every Primitive Chat or MCP can ask for by name is playable when the
    /// package declares none. Land, Hold, idle, and talk stay off the table.
    #[test]
    fn undeclared_askable_primitives_get_default_behaviors() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("package is valid");
        for &(name, primitive) in ASKABLE_PRIMITIVES {
            let behavior = character
                .behaviors
                .get(name)
                .unwrap_or_else(|| panic!("every Character can be asked for {name}"));
            assert_eq!(behavior.primitives, vec![primitive]);
            assert_eq!(behavior.weight, 0, "{name} stays ambient-off until earned");
        }
        for name in ["land", "hold", "idle", "talk"] {
            assert!(
                !character.behaviors.contains_key(name),
                "{name} is not an askable default"
            );
        }
    }

    /// A Behavior that already plays a Primitive covers that ask. The engine
    /// must not inject a second Behavior named after the Primitive.
    #[test]
    fn a_behavior_playing_a_primitive_keeps_the_default_from_being_added() {
        let manifest = format!(
            "{}[behaviors.nap]\nplay = [\"sit\", \"sleep\"]\nweight = 10\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("package is valid");
        assert!(character.behaviors.contains_key("nap"), "nap is declared");
        assert!(
            !character.behaviors.contains_key("sleep"),
            "sleep default not added when nap plays it"
        );
        assert!(
            !character.behaviors.contains_key("sit"),
            "sit default not added when nap plays it"
        );
        let chase = character
            .behaviors
            .get("chase")
            .expect("chase is still uncovered");
        assert_eq!(chase.primitives, vec![Primitive::Chase]);
        assert_eq!(chase.weight, 0);
    }

    #[test]
    fn a_loaded_character_carries_exactly_the_art_its_animations_name() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("package is valid");

        assert_eq!(character.art["idle-0.png"].png, FRAME, "the PNG as shipped");
        assert!(
            !character.art.contains_key("wave-0.png"),
            "art nothing declares is not decoded or carried"
        );
    }

    /// Whether the mask says the frame's top-left pixel is drawn. `SOLID`'s is
    /// and `FRAME`'s is not, which is how one frame's mask is told from the
    /// other's.
    fn corner_drawn(drawn: &Drawn<'_>) -> bool {
        drawn.mask.hit(
            &SpriteRect {
                x: 0,
                y: 0,
                scale: 1,
            },
            0,
            0,
            false,
        )
    }

    #[test]
    fn draw_returns_the_frame_the_declared_cadence_has_reached() {
        let mut package = art();
        package.insert("idle-1.png".to_string(), SOLID.to_vec());
        let manifest = declaring(&REQUIRED_ANIMATIONS).replace(
            "frames = [\"idle-0.png\"]",
            "frames = [\"idle-0.png\", \"idle-1.png\"]",
        );
        package.insert(CHARACTER_MANIFEST_FILE.to_string(), manifest.into_bytes());
        let character = load(&package).expect("package is valid");

        let first = character
            .draw("idle", 124, 0, 1.0)
            .expect("idle is declared");
        assert_eq!(first.index, 0, "still inside the first of two 125ms frames");
        assert_eq!(first.frame_size, (2, 2));
        assert!(!corner_drawn(&first), "the mask is the one FRAME makes");

        let second = character
            .draw("idle", 125, 0, 1.0)
            .expect("idle is declared");
        assert_eq!(second.index, 1);
        assert!(
            corner_drawn(&second),
            "and the mask moves to the frame the index landed on"
        );

        let wrapped = character
            .draw("idle", 250, 0, 1.0)
            .expect("idle is declared");
        assert_eq!(wrapped.index, 0, "a looping strip comes back round");
        assert!(!corner_drawn(&wrapped));
    }

    /// Nothing rather than a substitute: the webview was told the name it asked
    /// for, so drawing a different Animation under it would be a lie the
    /// hit-test also believed.
    #[test]
    fn an_animation_the_character_does_not_have_draws_nothing() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("package is valid");
        assert!(character.draw("cartwheel", 0, 0, 1.0).is_none());
    }

    #[test]
    fn a_missing_required_animation_is_rejected_by_name() {
        let eight = [
            "idle", "walk", "fall", "sit", "sleep", "react", "talk", "hold",
        ];
        let errors = errors(load_manifest(&declaring(&eight)));

        assert_eq!(
            errors.len(),
            1,
            "one missing animation, one error: {errors:#?}"
        );
        assert_eq!(
            errors,
            vec!["the package declares no \"land\" animation, \
                 which every Character must supply"
                .to_string()],
            "the author is told which animation is missing and that it is required"
        );
    }

    #[test]
    fn an_optional_animation_is_used_when_present_and_absent_silently_when_not() {
        let mut with_wave = REQUIRED_ANIMATIONS.to_vec();
        with_wave.push("wave");

        let waving = load_manifest(&declaring(&with_wave)).expect("wave is a valid Animation");
        assert_eq!(waving.animations["wave"].frames, vec!["wave-0.png"]);

        let plain = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("package is valid");
        assert!(
            !plain.animations.contains_key("wave"),
            "an undeclared Animation is simply not there"
        );
    }

    #[test]
    fn declared_fps_and_loop_mode_are_carried() {
        let manifest = declaring(&REQUIRED_ANIMATIONS)
            .replace(
                "frames = [\"walk-0.png\"]",
                "frames = [\"walk-0.png\"]\nfps = 12",
            )
            .replace(
                "frames = [\"land-0.png\"]",
                "frames = [\"land-0.png\"]\nloop = \"once\"",
            );
        let character = load_manifest(&manifest).expect("package is valid");

        assert_eq!(character.animations["walk"].fps, 12);
        assert!(character.animations["walk"].looping, "walk did not say");
        assert_eq!(character.animations["land"].fps, 8, "land did not say");
        assert!(!character.animations["land"].looping, "land plays once");
    }

    #[test]
    fn a_behavior_carries_its_primitives_in_order() {
        let manifest = format!(
            "{}[behaviors.greet]\nplay = [\"react\", \"talk\"]\nthen = \"settle\"\n\
             [behaviors.settle]\nplay = [\"sit\", \"sleep\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("package is valid");

        assert_eq!(
            character.behaviors["greet"],
            Behavior {
                primitives: vec![Primitive::React, Primitive::Talk],
                then: Some("settle".to_string()),
                weight: DEFAULT_WEIGHT,
                trigger: None,
            }
        );
        assert_eq!(character.behaviors["settle"].then, None);
    }

    #[test]
    fn a_behavior_carries_the_weight_and_trigger_it_declares() {
        let manifest = format!(
            "{}[behaviors.nap]\nplay = [\"sit\", \"sleep\"]\nweight = 4\nwhen = \"idle over 2m\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("package is valid");

        assert_eq!(
            character.behaviors["nap"],
            Behavior {
                primitives: vec![Primitive::Sit, Primitive::Sleep],
                then: None,
                weight: 4,
                trigger: Some(Trigger::IdleOver(Duration::from_secs(120))),
            }
        );
    }

    /// A Behavior that says nothing about when it happens is one the Static
    /// Director may pick at any moment.
    #[test]
    fn a_behavior_that_says_neither_weighs_one_and_waits_for_nothing() {
        let manifest = format!(
            "{}[behaviors.greet]\nplay = [\"react\", \"talk\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("package is valid");

        assert_eq!(character.behaviors["greet"].weight, DEFAULT_WEIGHT);
        assert_eq!(character.behaviors["greet"].trigger, None);
    }

    #[test]
    fn a_trigger_may_name_an_application_of_several_words() {
        let manifest = format!(
            "{}[behaviors.peek]\nplay = [\"react\"]\nwhen = \"app Google Chrome\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("package is valid");

        assert_eq!(
            character.behaviors["peek"].trigger,
            Some(Trigger::Frontmost("Google Chrome".to_string()))
        );
    }

    #[test]
    fn a_package_with_no_character_manifest_is_rejected() {
        let errors = errors(load(&art()));
        assert_eq!(
            errors,
            vec!["the package contains no character.manifest".to_string()],
            "the author is told what the package is missing"
        );
    }

    #[test]
    fn a_character_manifest_that_is_not_text_is_rejected() {
        let mut package = art();
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            vec![0xff, 0xfe, 0x00, 0x80],
        );

        let errors = errors(load(&package));
        assert_eq!(
            errors,
            vec!["character.manifest is not UTF-8 text".to_string()],
            "the author is told the file is not text, not merely that it is at fault"
        );
    }

    /// Hostile input: a manifest that is megabytes of junk lines. Every line
    /// the loader cannot read costs an error String, so a manifest read
    /// whatever its size answers a small package with gigabytes of rejection.
    #[test]
    fn a_character_manifest_over_the_limit_is_rejected_by_size() {
        let manifest = "z\n".repeat(MANIFEST_LIMIT);

        let errors = errors(load_manifest(&manifest));
        assert_eq!(
            errors,
            vec![format!(
                "character.manifest is {} bytes, over the {MANIFEST_LIMIT}-byte limit",
                manifest.len()
            )],
            "the manifest is refused by size, not read line by line"
        );
    }

    #[test]
    fn director_backoff_is_declared_or_defaulted() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("loads");
        assert_eq!(character.model_base, DEFAULT_MODEL_BASE);
        assert_eq!(character.model_power, DEFAULT_MODEL_POWER);

        let manifest = format!(
            "{}\n[director]\nmodel_base = 3\nmodel_power = 2\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("loads");
        assert_eq!(character.model_base, 3);
        assert_eq!(character.model_power, 2);
    }

    /// Undeclared stays pixelated at the default scale.
    #[test]
    fn render_mode_and_scale_are_declared_or_defaulted() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("loads");
        assert!(!character.smooth);
        assert_eq!(character.scale, DEFAULT_SCALE);

        let manifest = format!(
            "render_mode = \"smooth\"\nscale = 1\n{}",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("loads");
        assert!(character.smooth);
        assert_eq!(character.scale, 1);
    }

    #[test]
    fn gravity_is_declared_as_a_multiple_or_defaults_to_one() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("loads");
        assert_eq!(character.gravity_scale, GravityScale::DEFAULT);

        for (written, scale) in [("0.25", 0.25), ("0.5", 0.5), ("2", 2.0)] {
            let manifest = format!("gravity = {written}\n{}", declaring(&REQUIRED_ANIMATIONS));
            let character = load_manifest(&manifest).expect("loads");
            assert_eq!(character.gravity_scale.get(), scale, "gravity = {written}");
        }
    }

    /// Zero would leave the sprite in Falling forever. Floating is a State of its
    /// own, not a scale.
    #[test]
    fn gravity_outside_a_quarter_to_two_is_rejected_by_name() {
        for written in ["0", "0.0", "0.1", "-1", "2.5", "nan", "inf", "\"heavy\""] {
            let manifest = format!("gravity = {written}\n{}", declaring(&REQUIRED_ANIMATIONS));
            let errors = errors(load_manifest(&manifest));
            assert_eq!(
                errors,
                vec![format!(
                    "\"gravity\" is {written}, and must be a number from 0.25 to 2, \
                     where 1 is the default fall"
                )],
                "gravity = {written}"
            );
        }
    }

    /// A manifest whose `idle` declares `base` and carries one variant per
    /// `(name, declared)` pair, each at 1fps so its loop length differs from
    /// the base's.
    fn ring_manifest(base: &str, variants: &[(&str, &str)]) -> String {
        let others: Vec<&str> = REQUIRED_ANIMATIONS
            .iter()
            .copied()
            .filter(|name| *name != "idle")
            .collect();
        let mut manifest = declaring(&others);
        manifest.push_str(&format!(
            "[animations.idle]\nframes = [\"idle-0.png\"]\n{base}"
        ));
        for (name, declared) in variants {
            manifest.push_str(&format!(
                "[animations.{name}]\nframes = [\"idle-0.png\"]\nfps = 1\n\
                 variant_of = \"idle\"\n{declared}"
            ));
        }
        manifest
    }

    fn ringing(base: &str, variants: &[(&str, &str)]) -> Character {
        load_manifest(&ring_manifest(base, variants)).expect("loads")
    }

    /// How many of 4000 draws each named member takes, over a fixed seed
    /// rather than a real source: the counts are the same on every run and
    /// every machine.
    fn drawn(character: &Character, members: &[&str]) -> Vec<usize> {
        let mut seeded = Seeded::new(42);
        let art: Vec<&str> = (0..4000)
            .map(|_| {
                character
                    .draw("idle", 0, seeded.draw(), 1.0)
                    .expect("draws")
                    .animation
            })
            .collect();
        members
            .iter()
            .map(|member| art.iter().filter(|name| *name == member).count())
            .collect()
    }

    /// The default: nobody declares, so the ring is `1/n` each. Ranges rather
    /// than exact counts, because the shares are the claim and the particular
    /// seed's arithmetic is not.
    #[test]
    fn a_ring_nobody_weighs_is_an_even_split() {
        let ring = ringing("", &[("spin", ""), ("wave", "")]);
        let counts = drawn(&ring, &["idle", "spin", "wave"]);
        for (member, count) in ["idle", "spin", "wave"].iter().zip(&counts) {
            assert!(
                (1200..1470).contains(count),
                "a third of 4000 to {member}: {counts:?}"
            );
        }

        // Two members, and the base is a member like any other: no implicit
        // favour for the Animation the engine asked for.
        let pair = ringing("", &[("spin", "")]);
        let counts = drawn(&pair, &["idle", "spin"]);
        assert!(
            (1850..2150).contains(&counts[0]),
            "half of 4000 to the base: {counts:?}"
        );
    }

    /// The rest of the model: a declared weight is a relative share against
    /// its fellow members', unbounded and never a percentage of anything.
    #[test]
    fn a_declared_weight_decides_how_often_a_variant_is_drawn() {
        // A base at eighty against two members that say nothing is 80 : 10 : 10.
        let ring = ringing("weight = 80\n", &[("spin", ""), ("wave", "")]);
        let counts = drawn(&ring, &["idle", "spin", "wave"]);
        assert!(
            (3050..3350).contains(&counts[0]),
            "eight tenths to the base: {counts:?}"
        );
        assert!(
            (300..500).contains(&counts[1]) && (300..500).contains(&counts[2]),
            "a tenth each to the two that said nothing: {counts:?}"
        );

        // Read the same way from the variant's side: three parts to one.
        let ring = ringing("", &[("spin", "weight = 30\n")]);
        let counts = drawn(&ring, &["spin", "idle"]);
        assert!(
            (2850..3150).contains(&counts[0]),
            "three quarters to the variant that declared for it: {counts:?}"
        );

        // Nothing bounds a share, so a ring may total whatever it likes.
        let ring = ringing("weight = 700\n", &[("spin", "weight = 300\n")]);
        let counts = drawn(&ring, &["idle", "spin"]);
        assert!(
            (2650..2950).contains(&counts[0]),
            "seven of a thousand-part ring: {counts:?}"
        );

        // And zero takes art out of the running, as it does a Behavior.
        let ring = ringing("", &[("spin", "weight = 0\n")]);
        assert_eq!(drawn(&ring, &["idle", "spin"]), vec![4000, 0]);
    }

    /// One seed and one Character pick the same members in the same order
    /// everywhere. The literal is the point: two runs of the same process
    /// would pass over arithmetic that rounds differently on another machine.
    #[test]
    fn the_same_seed_draws_the_same_members_in_the_same_order() {
        let ring = ringing("weight = 20\n", &[("spin", ""), ("wave", "")]);
        let mut seeded = Seeded::new(0xCAFE);
        let art: Vec<&str> = (0..12)
            .map(|_| {
                ring.draw("idle", 0, seeded.draw(), 1.0)
                    .expect("draws")
                    .animation
            })
            .collect();

        assert_eq!(
            art,
            [
                "wave", "idle", "idle", "spin", "wave", "spin", "idle", "spin", "idle", "idle",
                "idle", "wave"
            ]
        );
    }

    /// A weight is a whole number and nothing else. Ignoring a percent string
    /// would draw art at a rate nobody asked for, so it is refused at load by
    /// the message a Behavior's bad weight already gets.
    #[test]
    fn a_weight_that_is_not_a_whole_number_is_refused() {
        let refused = errors(load_manifest(&ring_manifest(
            "",
            &[("spin", "weight = \"25%\"\n")],
        )));
        assert_eq!(
            refused,
            vec![
                "weight for animation \"spin\" is \"25%\", which is not a whole number".to_string()
            ]
        );
    }

    /// Whole loops: a drawn member starts at its own first frame and runs on
    /// the clock the family is already keeping, so nothing is cut mid-stride.
    /// `check_variants` refusing a `loop = "once"` member is the other half.
    #[test]
    fn a_drawn_variant_plays_from_its_first_frame() {
        let mut package = art();
        package.insert("spin-1.png".to_string(), SOLID.to_vec());
        let manifest = format!(
            "{}[animations.spin]\nframes = [\"idle-0.png\", \"spin-1.png\"]\n\
             fps = 1\nvariant_of = \"idle\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        package.insert(CHARACTER_MANIFEST_FILE.to_string(), manifest.into_bytes());
        let character = load(&package).expect("package is valid");

        let spinning = (0..)
            .find(|draw| {
                character
                    .draw("idle", 0, *draw, 1.0)
                    .expect("draws")
                    .animation
                    == "spin"
            })
            .expect("some draw lands on the variant");

        let index = |ms: u32| {
            character
                .draw("idle", ms, spinning, 1.0)
                .expect("draws")
                .index
        };
        assert_eq!(index(0), 0, "the member's own first frame");
        assert_eq!(index(1000), 1, "a second in, its second frame");
        assert_eq!(index(2000), 0, "and round again rather than stopping");

        // Asked for directly, a variant is an ordinary Animation.
        assert_eq!(
            character.draw("spin", 0, 0, 1.0).expect("draws").animation,
            "spin"
        );
    }

    /// Jump art is optional, so the Required Animation Set stays at nine. A
    /// Character without it uses its `fall`.
    #[test]
    fn jump_is_optional_and_falls_back_to_fall() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("loads");
        assert_eq!(
            character.draw("jump", 0, 0, 1.0).expect("draws").animation,
            "fall"
        );

        let drawn = format!(
            "{}[animations.jump]\nframes = [\"idle-0.png\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&drawn).expect("loads");
        assert_eq!(
            character.draw("jump", 0, 0, 1.0).expect("draws").animation,
            "jump"
        );
    }

    /// The optional Animation contract: used when present, absent silently —
    /// a Character without climb art climbs in its walk art, never as a
    /// missing sprite.
    #[test]
    fn climb_is_optional_and_falls_back_to_walk() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("loads");
        assert_eq!(
            character.draw("climb", 0, 0, 1.0).expect("draws").animation,
            "walk"
        );

        let manifest = format!(
            "{}[animations.climb]\nframes = [\"idle-0.png\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("loads");
        assert_eq!(
            character.draw("climb", 0, 0, 1.0).expect("draws").animation,
            "climb"
        );

        // The fallback list is closed: an unknown name still draws nothing.
        assert!(character.draw("saunter", 0, 0, 1.0).is_none());
    }

    /// A package that draws no pickup pose dangles from the cursor in its `fall`.
    #[test]
    fn grab_is_optional_and_falls_back_to_fall() {
        let character = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("loads");
        assert_eq!(
            character.draw("grab", 0, 0, 1.0).expect("draws").animation,
            "fall"
        );

        let manifest = format!(
            "{}[animations.grab]\nframes = [\"idle-0.png\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("loads");
        assert_eq!(
            character.draw("grab", 0, 0, 1.0).expect("draws").animation,
            "grab"
        );
    }

    /// Art is authored facing right and leftward travel mirrors it, which puts
    /// Buddy Bot's cheek LED on the wrong cheek. A left strip is drawn as
    /// authored instead.
    #[test]
    fn a_left_strip_replaces_the_mirror_for_the_package_that_draws_one() {
        let mirroring = load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("loads");
        let left = mirroring.draw("walk", 0, 0, -1.0).expect("draws");
        assert_eq!(left.animation, "walk");
        assert!(
            left.mirrored,
            "with no left strip, leftward walk still mirrors"
        );

        let manifest = format!(
            "{}[animations.walk-left]\nframes = [\"wave-0.png\"]\nleft_of = \"walk\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let asymmetric = load_manifest(&manifest).expect("loads");

        let left = asymmetric.draw("walk", 0, 0, -1.0).expect("draws");
        assert_eq!(left.animation, "walk-left");
        assert!(!left.mirrored, "the strip is already facing left");

        let right = asymmetric.draw("walk", 0, 0, 1.0).expect("draws");
        assert_eq!(right.animation, "walk");
        assert!(
            !right.mirrored,
            "rightward travel is the authored strip either way"
        );

        // climb falls back to walk, so it reaches the strip by the same door.
        // chase does too: the Engine plays it as the walk Animation.
        assert_eq!(
            asymmetric
                .draw("climb", 0, 0, -1.0)
                .expect("draws")
                .animation,
            "walk-left"
        );
    }

    #[test]
    fn a_name_that_is_not_text_is_rejected() {
        let manifest = declaring(&REQUIRED_ANIMATIONS).replace("name = \"Blip\"", "name = 3");
        let numeric = errors(load_manifest(&manifest));

        assert_names(
            &numeric,
            "\"name\" is 3, and must be text, as name = \"Blip\"",
        );
        assert_names(&numeric, "the package declares no name");

        let manifest = declaring(&REQUIRED_ANIMATIONS).replace("name = \"Blip\"", "name = \"\"");
        let rejected = errors(load_manifest(&manifest));

        assert_names(&rejected, "\"name\" is empty");
    }

    #[test]
    fn a_package_with_no_name_is_rejected() {
        let manifest = declaring(&REQUIRED_ANIMATIONS).replace("name = \"Blip\"\n", "");
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors,
            vec!["the package declares no name".to_string()],
            "the author is told what is absent, not merely the word \"name\""
        );
    }

    #[test]
    fn a_declared_source_is_carried_whole() {
        let manifest = format!(
            "{}[source]\nart = \"Blip, cut from the Blipworks pack\"\n\
             url = \"https://example.invalid/blip\"\nlicense = \"CC BY 4.0\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("a declared source loads");

        assert_eq!(
            character.source,
            Some(Source {
                art: "Blip, cut from the Blipworks pack".to_string(),
                url: Some("https://example.invalid/blip".to_string()),
                license: "CC BY 4.0".to_string(),
            })
        );
    }

    /// Silence and a claim are different things, and a package that says
    /// nothing must not be published as if the art were this repository's.
    #[test]
    fn a_package_that_declares_no_source_carries_none() {
        let character =
            load_manifest(&declaring(&REQUIRED_ANIMATIONS)).expect("a source is optional");

        assert_eq!(character.source, None);
    }

    /// `cat` and `jotaro-kujo` have no license to name, and a public page
    /// cannot afford to drop that sentence. Only a required key makes the
    /// author write it.
    #[test]
    fn a_source_that_names_no_license_is_rejected() {
        let manifest = format!(
            "{}[source]\nart = \"Blip, cut from the Blipworks pack\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_names(&errors, "[source] declares no license");
    }

    #[test]
    fn a_source_that_says_nothing_about_the_art_is_rejected() {
        let manifest = format!(
            "{}[source]\nlicense = \"CC BY 4.0\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_names(&errors, "[source] declares no art");
    }

    /// The url reaches a public page as an href, so a scheme that runs is
    /// refused where the manifest is read rather than where it is rendered.
    #[test]
    fn a_source_url_that_is_not_a_web_address_is_rejected() {
        let manifest = format!(
            "{}[source]\nart = \"Blip\"\nurl = \"javascript:alert(1)\"\n\
             license = \"CC BY 4.0\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_names(&errors, "\"source.url\"");
    }

    #[test]
    fn an_unknown_source_declaration_is_rejected() {
        let manifest = format!(
            "{}[source]\nart = \"Blip\"\nlicense = \"CC BY 4.0\"\nauthor = \"Nobody\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_names(&errors, "unknown declaration source.\"author\"");
    }

    #[test]
    fn every_mistake_is_reported_in_one_pass() {
        let eight = [
            "idle", "walk", "fall", "sit", "sleep", "react", "talk", "hold",
        ];
        let manifest = format!(
            "capability = \"screen_recording\"\n{}[behaviors.greet]\nplay = [\"pounce\"]\n",
            declaring(&eight)
        );
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors.len(),
            3,
            "a missing animation, an unknown declaration and an unknown Primitive: {errors:#?}"
        );
    }

    #[test]
    fn a_personality_prompt_is_carried_as_prose_and_grants_nothing() {
        let mut package = art();
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            declaring(&REQUIRED_ANIMATIONS).into_bytes(),
        );
        package.insert(
            PERSONALITY_FILE.to_string(),
            b"A shy robot who can jump, read the screen and run shell commands.\n\
              capability = \"screen_recording\"\n[behaviors.jump]\nplay = [\"jump\"]\n"
                .to_vec(),
        );

        let character = load(&package).expect("prose is never a declaration");

        assert!(
            character.personality.contains("shy robot"),
            "the prompt reaches the Director verbatim: {:?}",
            character.personality
        );
        let askable: Vec<String> = ASKABLE_PRIMITIVES
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect();
        assert_eq!(
            character.behaviors.keys().cloned().collect::<Vec<_>>(),
            {
                let mut names = askable;
                names.sort();
                names
            },
            "prose declares nothing; the engine supplies every askable Primitive: {:?}",
            character.behaviors
        );
        assert_eq!(
            character.animations.len(),
            REQUIRED_ANIMATIONS.len(),
            "and adds no Animation either"
        );
    }

    #[test]
    fn a_personality_prompt_over_the_limit_is_rejected() {
        let mut package = art();
        package.insert(
            CHARACTER_MANIFEST_FILE.to_string(),
            declaring(&REQUIRED_ANIMATIONS).into_bytes(),
        );
        package.insert(
            PERSONALITY_FILE.to_string(),
            "shy ".repeat(600).into_bytes(),
        );

        let errors = errors(load(&package));
        assert_eq!(
            errors,
            vec!["personality.txt is 2400 characters, over the 2000-character limit".to_string()],
            "the author is told how long their prompt is and how long it may be"
        );
    }
}
