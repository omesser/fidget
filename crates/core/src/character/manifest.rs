//! TOML parse of a Character Manifest, yielding `Declared*` for `load`.

use std::collections::BTreeMap;
use std::time::Duration;

use toml_edit::{Document, Item};

use super::{
    CursorReaction, GravityScale, Primitive, Source, Trigger, CHARACTER_MANIFEST_FILE, DEFAULT_FPS,
    DEFAULT_WEIGHT, MAX_FPS, MAX_FRAMES, MAX_SCALE, PRIMITIVES,
};

/// A Character Manifest as written, before its declarations are checked
/// against the art and against each other.
#[derive(Default)]
pub(super) struct Declared {
    pub(super) name: Option<String>,
    pub(super) smooth: Option<bool>,
    pub(super) scale: Option<u32>,
    pub(super) model_base: Option<u32>,
    pub(super) model_power: Option<u32>,
    pub(super) near_reaction: Option<CursorReaction>,
    pub(super) rush_reaction: Option<CursorReaction>,
    pub(super) gravity_scale: Option<GravityScale>,
    pub(super) source: Option<Source>,
    pub(super) animations: BTreeMap<String, DeclaredAnimation>,
    pub(super) behaviors: BTreeMap<String, DeclaredBehavior>,
}

pub(super) struct DeclaredAnimation {
    pub(super) frames: Vec<String>,
    pub(super) fps: u32,
    pub(super) looping: bool,
    pub(super) variant_of: Option<String>,
    /// The Animation this one is the left-facing strip of, when the package
    /// draws its leftward travel rather than letting the renderer mirror it.
    pub(super) left_of: Option<String>,
    /// This Animation's share of its variant ring, against its fellow
    /// members'. `DEFAULT_WEIGHT` when undeclared, exactly as on a Behavior.
    pub(super) weight: u32,
}

pub(super) struct DeclaredBehavior {
    pub(super) primitives: Vec<Primitive>,
    pub(super) then: Option<String>,
    pub(super) weight: u32,
    pub(super) trigger: Option<Trigger>,
}

/// Read the Character Manifest. `None` means it is not TOML at all: one error,
/// not a guess past the first syntax mistake, which would report mistakes the
/// author has not made.
pub(super) fn parse(manifest: &str, errors: &mut Vec<String>) -> Option<Declared> {
    let mut declared = Declared::default();

    let document = match Document::parse(manifest) {
        Ok(document) => document,
        Err(error) => {
            let at = error
                .span()
                .map(|span| format!(" at line {}", line_of(manifest, span.start)))
                .unwrap_or_default();
            // The parser's message can be as terse as "duplicate key", so the
            // offending text is quoted after it when the span has any.
            let offender = match wrote(manifest, error.span()) {
                Some(text) if !text.is_empty() => format!(" ({text})"),
                _ => String::new(),
            };
            errors.push(format!(
                "{CHARACTER_MANIFEST_FILE} is not TOML{at}: {}{offender}",
                error.message()
            ));
            return None;
        }
    };

    for (key, item) in document.iter() {
        match key {
            "name" => match item.as_str() {
                Some("") => errors.push("\"name\" is empty".to_string()),
                Some(name) => declared.name = Some(name.to_string()),
                None => errors.push(format!(
                    "\"name\" is {}, and must be text, as name = \"Blip\"",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "animations" => match item.as_table_like() {
                Some(table) => {
                    for (name, item) in table.iter() {
                        if let Some(animation) = parse_animation(name, item, manifest, errors) {
                            declared.animations.insert(name.to_string(), animation);
                        }
                    }
                }
                None => errors.push(
                    "\"animations\" is not a set of tables; an Animation reads \
                     [animations.idle] with its frames, fps and loop below"
                        .to_string(),
                ),
            },
            "behaviors" => match item.as_table_like() {
                Some(table) => {
                    for (name, item) in table.iter() {
                        if let Some(behavior) = parse_behavior(name, item, manifest, errors) {
                            declared.behaviors.insert(name.to_string(), behavior);
                        }
                    }
                }
                None => errors.push(
                    "\"behaviors\" is not a set of tables; a Behavior reads \
                     [behaviors.greet] with its play, then, weight and when below"
                        .to_string(),
                ),
            },
            "render_mode" => match item.as_str() {
                Some("pixelated") => declared.smooth = Some(false),
                Some("smooth") => declared.smooth = Some(true),
                _ => errors.push(format!(
                    "\"render_mode\" is {}, and must be \"pixelated\" or \"smooth\"",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "scale" => match item.as_integer() {
                Some(scale) if (1..=i64::from(MAX_SCALE)).contains(&scale) => {
                    declared.scale = Some(scale as u32)
                }
                _ => errors.push(format!(
                    "\"scale\" is {}, and must be a whole number from 1 to {MAX_SCALE}",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            // TOML reads `1` as an integer and `1.0` as a float; an author
            // writing either means the same multiple.
            "gravity" => match item
                .as_float()
                .or_else(|| item.as_integer().map(|whole| whole as f64))
                .and_then(GravityScale::new)
            {
                Some(scale) => declared.gravity_scale = Some(scale),
                None => errors.push(format!(
                    "\"gravity\" is {}, and must be a number from {} to {}, \
                     where 1 is the default fall",
                    wrote(manifest, item.span()).unwrap_or("?"),
                    GravityScale::MIN,
                    GravityScale::MAX
                )),
            },
            "source" => parse_source(item, manifest, &mut declared, errors),
            "director" => parse_director(item, manifest, &mut declared, errors),
            "cursor" => parse_cursor(item, manifest, &mut declared, errors),
            other => errors.push(format!(
                "unknown declaration {other:?}; a Character Manifest declares \
                 name, render_mode, scale, gravity, source, animations, \
                 behaviors, director and cursor"
            )),
        }
    }

    Some(declared)
}

/// `[source]`: where the art came from, for `scripts/make-character-gallery.py`.
/// The Engine does not read it. Validated anyway: the parser knows every
/// declaration or none.
fn parse_source(item: &Item, manifest: &str, declared: &mut Declared, errors: &mut Vec<String>) {
    let Some(table) = item.as_table_like() else {
        errors.push(
            "\"source\" is not a table; it reads [source] with \
             art, url and license below"
                .to_string(),
        );
        return;
    };

    let mut art = None;
    let mut url = None;
    let mut license = None;

    for (key, item) in table.iter() {
        match key {
            "art" => match item.as_str() {
                Some(text) if !text.trim().is_empty() => art = Some(text.to_string()),
                _ => errors.push(format!(
                    "\"source.art\" is {}, and must say what the art is \
                     and where it came from",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            // A page renders this as a link, so the scheme is refused here
            // rather than trusted there: the manifest is data, and javascript:
            // in an href is the whole reason to check.
            "url" => match item.as_str() {
                Some(text) if text.starts_with("https://") || text.starts_with("http://") => {
                    url = Some(text.to_string())
                }
                _ => errors.push(format!(
                    "\"source.url\" is {}, and must be an http or https address",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "license" => match item.as_str() {
                Some(text) if !text.trim().is_empty() => license = Some(text.to_string()),
                _ => errors.push(format!(
                    "\"source.license\" is {}, and must name the license \
                     or say that none is declared",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            other => errors.push(format!(
                "unknown declaration source.{other:?}; [source] declares \
                 art, url and license"
            )),
        }
    }

    // Asking the table rather than the parsed value keeps a key that is
    // present but malformed to the one error its own arm already pushed.
    if !table.contains_key("art") {
        errors.push(
            "[source] declares no art; say what the art is and where it \
             came from, as art = \"Blip, cut from the Blipworks pack\""
                .to_string(),
        );
    }
    // Silence about a license reads as permission to whoever publishes the
    // art, so the key is required. A pack often declares nothing; the author
    // still has to write that, not leave the field out.
    if !table.contains_key("license") {
        errors.push(
            "[source] declares no license; name it, or say that none is \
             declared, as license = \"None declared\""
                .to_string(),
        );
    }

    if let (Some(art), Some(license)) = (art, license) {
        declared.source = Some(Source { art, url, license });
    }
}

/// `[director]`: how proactive model calls space themselves.
fn parse_director(item: &Item, manifest: &str, declared: &mut Declared, errors: &mut Vec<String>) {
    let Some(table) = item.as_table_like() else {
        errors.push(
            "\"director\" is not a table; it reads [director] with \
             model_base and model_power below"
                .to_string(),
        );
        return;
    };

    for (key, item) in table.iter() {
        match key {
            "model_base" => match item.as_integer() {
                Some(base) if base >= 1 && u32::try_from(base).is_ok() => {
                    declared.model_base = Some(base as u32)
                }
                _ => errors.push(format!(
                    "\"director.model_base\" is {}, and must be a whole number from 1",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "model_power" => match item.as_integer() {
                Some(power) if u32::try_from(power).is_ok() => {
                    declared.model_power = Some(power as u32)
                }
                _ => errors.push(format!(
                    "\"director.model_power\" is {}, and must be a whole number from 0",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            other => errors.push(format!(
                "unknown declaration director.{other:?}; [director] declares \
                 model_base and model_power"
            )),
        }
    }
}

/// `[cursor]`: how the Character reacts to cursor proximity.
fn parse_cursor(item: &Item, manifest: &str, declared: &mut Declared, errors: &mut Vec<String>) {
    let Some(table) = item.as_table_like() else {
        errors.push(
            "\"cursor\" is not a table; it reads [cursor] with \
             near_reaction and rush_reaction below"
                .to_string(),
        );
        return;
    };

    for (key, item) in table.iter() {
        match key {
            "near_reaction" => match parse_cursor_reaction(item.as_str()) {
                Some(reaction) => declared.near_reaction = Some(reaction),
                None => errors.push(format!(
                    "\"cursor.near_reaction\" is {}, and must be one of: \
                     indifferent, speak, face, toward, away, react",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "rush_reaction" => match parse_cursor_reaction(item.as_str()) {
                Some(reaction) => declared.rush_reaction = Some(reaction),
                None => errors.push(format!(
                    "\"cursor.rush_reaction\" is {}, and must be one of: \
                     indifferent, speak, face, toward, away, react",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            other => errors.push(format!(
                "unknown declaration cursor.{other:?}; [cursor] declares \
                 near_reaction and rush_reaction"
            )),
        }
    }
}

fn parse_cursor_reaction(value: Option<&str>) -> Option<CursorReaction> {
    match value? {
        "indifferent" => Some(CursorReaction::Indifferent),
        "speak" => Some(CursorReaction::Speak),
        "face" => Some(CursorReaction::Face),
        "toward" => Some(CursorReaction::Toward),
        "away" => Some(CursorReaction::Away),
        "react" => Some(CursorReaction::React),
        _ => None,
    }
}

fn parse_animation(
    name: &str,
    item: &Item,
    manifest: &str,
    errors: &mut Vec<String>,
) -> Option<DeclaredAnimation> {
    let Some(table) = item.as_table_like() else {
        errors.push(format!(
            "animation {name:?} is not a table; an Animation reads \
             [animations.{name}] with its frames, fps and loop below"
        ));
        return None;
    };

    let mut frames = None;
    let mut fps = DEFAULT_FPS;
    let mut looping = true;
    let mut variant_of = None;
    let mut left_of = None;
    let mut weight = DEFAULT_WEIGHT;

    for (key, item) in table.iter() {
        match key {
            "frames" => frames = frame_list(name, item, manifest, errors),
            "variant_of" => match item.as_str() {
                Some(base) if !base.is_empty() => variant_of = Some(base.to_string()),
                _ => errors.push(format!(
                    "variant_of for animation {name:?} is {}, and must name \
                     an animation, as variant_of = \"idle\"",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "left_of" => match item.as_str() {
                Some(base) if !base.is_empty() => left_of = Some(base.to_string()),
                _ => errors.push(format!(
                    "left_of for animation {name:?} is {}, and must name \
                     an animation, as left_of = \"walk\"",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "fps" => match item.as_integer() {
                Some(declared) if (1..=i64::from(MAX_FPS)).contains(&declared) => {
                    fps = declared as u32;
                }
                Some(declared) => errors.push(format!(
                    "fps for animation {name:?} is {declared}, and must be 1 to {MAX_FPS}"
                )),
                None => errors.push(format!(
                    "fps for animation {name:?} is {}, which is not a whole number",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "loop" => match item.as_str() {
                Some("forever") => looping = true,
                Some("once") => looping = false,
                _ => errors.push(format!(
                    "loop mode for animation {name:?} is {}, \
                     and must be \"forever\" or \"once\"",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "weight" => match item.as_integer().and_then(|w| u32::try_from(w).ok()) {
                Some(declared) => weight = declared,
                None => errors.push(format!(
                    "weight for animation {name:?} is {}, which is not a whole number",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            other => errors.push(format!(
                "animation {name:?} declares unknown {other:?}; \
                 an Animation declares frames, fps, loop, variant_of, left_of and weight"
            )),
        }
    }

    // The table rather than `frames`: `frame_list` already reports an empty
    // list, and every other refusal leaves the key present, so this asks only
    // whether the author wrote one at all.
    if !table.contains_key("frames") {
        errors.push(format!("animation {name:?} declares no frames"));
    }
    // No usable frame list — rejected above or by `frame_list` — means no
    // Animation, however sound its fps and loop mode were.
    let frames = frames?;
    Some(DeclaredAnimation {
        frames,
        fps,
        looping,
        variant_of,
        left_of,
        weight,
    })
}

/// An Animation's `frames` list, counted before the names are copied, so a
/// long list is rejected for its length alone.
fn frame_list(
    name: &str,
    item: &Item,
    manifest: &str,
    errors: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Some(list) = item.as_array() else {
        errors.push(format!(
            "frames for animation {name:?} is {}, and must be a list of \
             frame files, as frames = [\"idle-0.png\"]",
            wrote(manifest, item.span()).unwrap_or("?")
        ));
        return None;
    };

    if list.len() > MAX_FRAMES {
        errors.push(format!(
            "animation {name:?} declares {} frames, \
             and an Animation may have at most {MAX_FRAMES}",
            list.len()
        ));
        return None;
    }

    let mut frames = Vec::new();
    for frame in list.iter() {
        match frame.as_str() {
            Some(file) => frames.push(file.to_string()),
            None => {
                errors.push(format!(
                    "animation {name:?} declares the frame {}, which is not a file name",
                    wrote(manifest, frame.span()).unwrap_or("?")
                ));
                return None;
            }
        }
    }
    // The list written empty; `parse_animation` reports the key never written.
    if frames.is_empty() {
        errors.push(format!("animation {name:?} declares no frames"));
        return None;
    }
    Some(frames)
}

fn parse_behavior(
    name: &str,
    item: &Item,
    manifest: &str,
    errors: &mut Vec<String>,
) -> Option<DeclaredBehavior> {
    let Some(table) = item.as_table_like() else {
        errors.push(format!(
            "behavior {name:?} is not a table; a Behavior reads \
             [behaviors.{name}] with its play, then, weight and when below"
        ));
        return None;
    };

    let mut primitives = Vec::new();
    let mut then = None;
    let mut weight = DEFAULT_WEIGHT;
    let mut trigger = None;

    for (key, item) in table.iter() {
        match key {
            "play" => primitives = play_list(name, item, manifest, errors),
            "then" => match item.as_str() {
                Some(next) if !next.is_empty() => then = Some(next.to_string()),
                _ => errors.push(format!(
                    "then for behavior {name:?} is {}, and must name one Behavior, \
                     as then = \"settle\"",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "weight" => match item.as_integer().and_then(|w| u32::try_from(w).ok()) {
                Some(declared) => weight = declared,
                None => errors.push(format!(
                    "weight for behavior {name:?} is {}, which is not a whole number",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            "when" => match item.as_str().and_then(parse_trigger) {
                Some(condition) => trigger = Some(condition),
                None => errors.push(format!(
                    "when for behavior {name:?} is {}, which is not a condition; \
                     a condition reads \"idle over 2m\", \"idle under 30s\" or \"app Safari\"",
                    wrote(manifest, item.span()).unwrap_or("?")
                )),
            },
            other => errors.push(format!(
                "behavior {name:?} declares unknown {other:?}; \
                 a Behavior declares play, then, weight and when"
            )),
        }
    }

    Some(DeclaredBehavior {
        primitives,
        then,
        weight,
        trigger,
    })
}

/// A Behavior's `play` list. A word that is not a Primitive is reported and
/// dropped rather than abandoning the declaration, so the rest is still
/// checked.
fn play_list(name: &str, item: &Item, manifest: &str, errors: &mut Vec<String>) -> Vec<Primitive> {
    let Some(list) = item.as_array() else {
        errors.push(format!(
            "play for behavior {name:?} is {}, and must be a list of \
             Primitives, as play = [\"react\", \"talk\"]",
            wrote(manifest, item.span()).unwrap_or("?")
        ));
        return Vec::new();
    };

    let mut primitives = Vec::new();
    for word in list.iter() {
        let primitive = word.as_str().and_then(|word| {
            PRIMITIVES
                .iter()
                .find_map(|(known, primitive)| (*known == word).then_some(*primitive))
        });
        match primitive {
            Some(primitive) => primitives.push(primitive),
            None => errors.push(format!(
                "behavior {name:?} declares {}, which is not a Primitive; \
                 the Primitives are {}",
                wrote(manifest, word.span()).unwrap_or("?"),
                PRIMITIVES
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
    primitives
}

/// The line a byte offset falls on, for a syntax error to name.
fn line_of(manifest: &str, offset: usize) -> usize {
    let offset = offset.min(manifest.len());
    manifest.as_bytes()[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

/// What the author wrote, quoted back at them when its type is wrong. The
/// manifest's own bytes rather than a re-rendering, so the author sees text
/// they can search their file for. `None` when the parser kept no span.
fn wrote(manifest: &str, span: Option<std::ops::Range<usize>>) -> Option<&str> {
    span.and_then(|span| manifest.get(span)).map(str::trim)
}

/// The application name is the rest of the line rather than one word.
/// "Google Chrome" is what the platform reports, and an author writes what
/// they see.
fn parse_trigger(value: &str) -> Option<Trigger> {
    let (condition, rest) = value.split_once(char::is_whitespace)?;
    let rest = rest.trim();

    match condition {
        "idle" => {
            let (comparison, duration) = rest.split_once(char::is_whitespace)?;
            let duration = parse_duration(duration.trim())?;
            match comparison {
                "over" => Some(Trigger::IdleOver(duration)),
                "under" => Some(Trigger::IdleUnder(duration)),
                _ => None,
            }
        }
        "app" => (!rest.is_empty()).then(|| Trigger::Frontmost(rest.to_string())),
        _ => None,
    }
}

/// A span as a count and a unit, as `30s` or `2m`. Stripped as a suffix, not
/// split at the last byte: the last byte of "2\u{043c}" is the middle of a
/// character, and splitting untrusted text there would panic.
fn parse_duration(text: &str) -> Option<Duration> {
    let (count, seconds_each) = match (text.strip_suffix('s'), text.strip_suffix('m')) {
        (Some(count), _) => (count, 1),
        (_, Some(count)) => (count, 60),
        _ => return None,
    };
    count
        .parse::<u64>()
        .ok()?
        .checked_mul(seconds_each)
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::tests::{assert_names, declaring, errors, load_manifest};
    use crate::character::REQUIRED_ANIMATIONS;

    #[test]
    fn a_weight_that_is_not_a_whole_number_is_rejected() {
        let manifest = format!(
            "{}[behaviors.greet]\nplay = [\"react\"]\nweight = \"lots\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let rejected = errors(load_manifest(&manifest));

        assert_eq!(
            rejected,
            vec!["weight for behavior \"greet\" is \"lots\", \
                 which is not a whole number"
                .to_string()]
        );

        // TOML integers can be negative, and a weight is a count.
        let negative = format!(
            "{}[behaviors.greet]\nplay = [\"react\"]\nweight = -3\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let rejected = errors(load_manifest(&negative));

        assert_eq!(
            rejected,
            vec!["weight for behavior \"greet\" is -3, \
                 which is not a whole number"
                .to_string()]
        );
    }

    #[test]
    fn a_trigger_that_is_not_a_condition_is_rejected_with_the_conditions() {
        let manifest = format!(
            "{}[behaviors.greet]\nplay = [\"react\"]\nwhen = \"weather rain\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_names(&errors, "\"weather rain\"");
        assert_names(&errors, "idle over 2m");
        assert_names(&errors, "app Safari");
    }

    /// Hostile input: a duration whose last byte is the middle of a character.
    /// Splitting it off by byte would panic and take the loader with it.
    #[test]
    fn a_duration_that_is_not_ascii_is_rejected_rather_than_crashing() {
        let manifest = format!(
            "{}[behaviors.nap]\nplay = [\"sit\"]\nwhen = \"idle over 2\u{043c}\"\n",
            declaring(&REQUIRED_ANIMATIONS)
        );

        assert_names(&errors(load_manifest(&manifest)), "nap");
    }

    #[test]
    fn an_unknown_primitive_is_rejected_by_name() {
        let manifest = format!(
            "{}[behaviors.greet]\nplay = [\"talk\", \"pounce\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors,
            vec![
                "behavior \"greet\" declares \"pounce\", which is not a Primitive; \
                 the Primitives are idle, walk, land, sit, sleep, react, talk, hold, chase, jump"
                    .to_string()
            ],
            "the author is told the offending word and what they may write instead"
        );
    }

    #[test]
    fn an_unknown_declaration_is_rejected_by_name() {
        // Before the tables: a root key written after one would land inside it.
        let manifest = format!(
            "capability = \"screen_recording\"\n{}",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors,
            vec![
                "unknown declaration \"capability\"; a Character Manifest declares \
                 name, render_mode, scale, gravity, source, animations, \
                 behaviors, director and cursor"
                    .to_string()
            ],
            "no package can invent a declaration, so none can grant itself anything"
        );
    }

    #[test]
    fn a_director_backoff_that_is_not_a_whole_number_from_one_is_rejected() {
        let manifest = format!(
            "{}\n[director]\nmodel_base = 0\nmodel_power = 1\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));
        assert_names(&errors, "model_base");
    }

    #[test]
    fn a_render_mode_that_is_neither_option_is_rejected_by_its_text() {
        let manifest = format!(
            "render_mode = \"blurry\"\n{}",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let errors = errors(load_manifest(&manifest));
        assert_eq!(
            errors,
            vec![
                "\"render_mode\" is \"blurry\", and must be \"pixelated\" or \"smooth\""
                    .to_string()
            ],
        );
    }

    #[test]
    fn a_scale_outside_its_bounds_is_rejected_by_its_text() {
        let manifest = format!("scale = 9\n{}", declaring(&REQUIRED_ANIMATIONS));
        let errors = errors(load_manifest(&manifest));
        assert_eq!(
            errors,
            vec![format!(
                "\"scale\" is 9, and must be a whole number from 1 to {MAX_SCALE}"
            )],
        );
    }

    /// A syntax mistake is one error naming its line, never a cascade: past it
    /// the parser would be guessing, and a guess would report mistakes the
    /// author has not made.
    #[test]
    fn a_manifest_that_is_not_toml_is_rejected_with_its_line() {
        let manifest = format!("{}animation idle\n", declaring(&REQUIRED_ANIMATIONS));
        let errors = errors(load_manifest(&manifest));

        assert_eq!(
            errors.len(),
            1,
            "one syntax error, one message: {errors:#?}"
        );
        // Nine required Animations at two lines each follow the name line.
        assert_names(&errors, "character.manifest is not TOML at line 20");
    }

    #[test]
    fn an_animation_with_no_frames_is_rejected_by_name() {
        // An empty list and no list at all are the same mistake to an author.
        for wave in ["[animations.wave]\nframes = []\n", "[animations.wave]\n"] {
            let empty = errors(load_manifest(&format!(
                "{}{wave}",
                declaring(&REQUIRED_ANIMATIONS)
            )));
            assert_eq!(
                empty,
                vec!["animation \"wave\" declares no frames".to_string()],
                "the author is told which Animation has no art"
            );
        }
    }

    /// A missing `frames` key and an empty list both say `declares no frames`,
    /// and only the key's presence keeps them to one error. Other refusals
    /// leave the key, so a name check would still pass if that error followed.
    #[test]
    fn a_frames_list_refused_for_another_reason_is_not_also_called_no_frames() {
        let over_the_bound = vec!["\"wave-0.png\""; MAX_FRAMES + 1].join(", ");
        for frames in [
            "frames = \"wave-0.png\"".to_string(),
            "frames = [7]".to_string(),
            format!("frames = [{over_the_bound}]"),
        ] {
            let refused = errors(load_manifest(&format!(
                "{}[animations.wave]\n{frames}\n",
                declaring(&REQUIRED_ANIMATIONS)
            )));

            assert_eq!(refused.len(), 1, "one mistake, one error: {refused:#?}");
            assert!(
                !refused[0].contains("declares no frames"),
                "the frames key is written, so the missing-key error must not fire: {refused:#?}"
            );
        }
    }

    /// TOML itself refuses a duplicate key, so a declaration written twice is
    /// a syntax error naming the key rather than a check of this module's.
    #[test]
    fn an_animation_or_behavior_declared_twice_is_rejected_by_name() {
        let twice = errors(load_manifest(&format!(
            "{}[animations.idle]\nframes = [\"idle-0.png\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        )));
        assert_names(&twice, "is not TOML");
        assert_names(&twice, "idle");

        let twice = errors(load_manifest(&format!(
            "{}[behaviors.greet]\nplay = [\"talk\"]\n[behaviors.greet]\nplay = [\"sit\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        )));
        assert_names(&twice, "is not TOML");
        assert_names(&twice, "greet");
    }

    /// Hostile input: a frame name is a few manifest bytes and a full copy of
    /// the art in the renderer, so an unbounded count dies on allocation. Both
    /// sides of the bound are checked so it cannot drift by one.
    #[test]
    fn an_animation_with_more_frames_than_the_bound_is_rejected_by_name() {
        let repeat = |count: usize| {
            format!(
                "{}[animations.wave]\nframes = [{}]\n",
                declaring(&REQUIRED_ANIMATIONS),
                vec!["\"wave-0.png\""; count].join(", ")
            )
        };

        let character = load_manifest(&repeat(MAX_FRAMES)).expect("the bound itself loads");
        assert_eq!(character.animations["wave"].frames.len(), MAX_FRAMES);

        let over = errors(load_manifest(&repeat(MAX_FRAMES + 1)));
        assert_names(&over, "wave");
        assert_names(&over, &format!("{} frames", MAX_FRAMES + 1));
    }

    /// None of these is guessed at, ignored, or allowed to panic.
    #[test]
    fn nonsense_declarations_are_each_rejected_by_name() {
        let manifest = format!(
            "{}\
             [animations.wave]\n\
             frames = \"wave-0.png\"\n\
             mirrored = true\n\
             [behaviors.chase]\n\
             play = \"walk\"\n\
             then = 3\n\
             [behaviors.pounce]\n\
             play = [\"react\", 7]\n\
             when = 6\n\
             [behaviors.\"фыр\"]\n\
             play = [[]]\n",
            declaring(&REQUIRED_ANIMATIONS)
        );

        let errors = errors(load_manifest(&manifest));

        // The whole set, not a count and a prefix: messages reading only
        // "wrong type" would satisfy a structural check while telling the
        // author nothing about what to change.
        assert_eq!(
            errors,
            vec![
                "frames for animation \"wave\" is \"wave-0.png\", and must be a list of \
                 frame files, as frames = [\"idle-0.png\"]"
                    .to_string(),
                "animation \"wave\" declares unknown \"mirrored\"; an Animation declares \
                 frames, fps, loop, variant_of, left_of and weight"
                    .to_string(),
                "play for behavior \"chase\" is \"walk\", and must be a list of Primitives, \
                 as play = [\"react\", \"talk\"]"
                    .to_string(),
                "then for behavior \"chase\" is 3, and must name one Behavior, \
                 as then = \"settle\""
                    .to_string(),
                "behavior \"pounce\" declares 7, which is not a Primitive; the Primitives \
                 are idle, walk, land, sit, sleep, react, talk, hold, chase, jump"
                    .to_string(),
                "when for behavior \"pounce\" is 6, which is not a condition; a condition \
                 reads \"idle over 2m\", \"idle under 30s\" or \"app Safari\""
                    .to_string(),
                "behavior \"фыр\" declares [], which is not a Primitive; the Primitives \
                 are idle, walk, land, sit, sleep, react, talk, hold, chase, jump"
                    .to_string(),
            ],
            "each nonsense declaration is rejected by name, saying what is wrong"
        );
    }

    #[test]
    fn an_unplayable_fps_or_loop_mode_is_rejected_by_name() {
        // Each case asks for the Animation at fault and what is wrong with
        // it, so a message that says only "fps" cannot pass.
        for (declaration, wanted) in [
            (
                "fps = 0",
                &["animation \"idle\"", "is 0", "must be 1 to 60"][..],
            ),
            ("fps = \"soon\"", &["animation \"idle\"", "\"soon\""]),
            ("fps = 240", &["animation \"idle\"", "is 240"]),
            ("fps = 3.5", &["animation \"idle\"", "3.5", "whole number"]),
            ("loop = \"maybe\"", &["animation \"idle\"", "\"maybe\""]),
        ] {
            let manifest = declaring(&REQUIRED_ANIMATIONS).replace(
                "frames = [\"idle-0.png\"]",
                &format!("frames = [\"idle-0.png\"]\n{declaration}"),
            );
            let errors = errors(load_manifest(&manifest));
            for offender in wanted {
                assert_names(&errors, offender);
            }
        }
    }
}
