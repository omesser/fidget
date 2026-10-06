//! The roster of Character Instances: spawn, dismiss, list.
//!
//! One Engine per Instance. They differ in name, position, and Behavior. Memory is shared.

use crate::character::Character;
use crate::display::Whereabouts;
use crate::engine::{BehaviorProposal, Engine, Frame, Point, Rect, WorldSnapshot};
use std::collections::BTreeMap;

/// A stable identifier for one Character Instance.
/// A uuid v4, so ids never collide across process restarts.
pub type InstanceId = String;

/// How long an Instance Prompt may be, in characters.
/// Untrusted text in every opening turn. Two authored layers share this bound,
/// so moving one without the other doubles the worst case (ADR-0012).
pub const INSTANCE_PROMPT_LIMIT: usize = crate::character::PERSONALITY_LIMIT;

/// `text` as an Instance Prompt, or why it cannot be one.
/// Refused rather than cut: silent shortening would drop words still visible
/// in the box. Counted after trim, because trimmed is what is stored and sent.
pub fn instance_prompt(text: &str) -> Result<String, String> {
    let text = text.trim();
    let length = text.chars().count();
    if length > INSTANCE_PROMPT_LIMIT {
        return Err(format!(
            "The Instance Prompt is {length} characters, over the \
             {INSTANCE_PROMPT_LIMIT}-character limit"
        ));
    }
    Ok(text.to_string())
}

/// One Instance asked for at launch: Character, name, and last-written prompt.
/// A request, not an Instance. New fields default so an older settings file
/// still loads.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstanceSpec {
    pub character: String,
    pub name: String,
    /// Id this Instance ran under last time. `None` if it has not run.
    /// The Instance Prompt hangs off this id, so a fresh uuid every launch
    /// would lose the text (ADR-0012).
    #[serde(default)]
    pub id: Option<InstanceId>,
    /// The Instance Prompt the user wrote for it. Empty by default.
    #[serde(default)]
    pub prompt: String,
}

impl InstanceSpec {
    /// A spec for an Instance that has not run: no id to restore, and no
    /// Instance Prompt written against one.
    pub fn fresh(character: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            character: character.into(),
            name: name.into(),
            ..Self::default()
        }
    }
}

/// Read Instances to run from one configuration string.
/// Grammar is `character:name`, comma separated, name optional. Empty, unset,
/// or a stray comma all mean the default single Instance, not a failed start.
pub fn parse_specs(raw: &str) -> Result<Vec<InstanceSpec>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            // A colon the user typed is a name they meant to give, so an empty
            // half is a typo to report rather than a default to guess at. No
            // colon at all is the shorthand: the package name is the name.
            let (character, name) = match entry.split_once(':') {
                Some((character, name)) => (character.trim(), name.trim()),
                None => (entry, entry),
            };
            if character.is_empty() || name.is_empty() {
                return Err(format!(
                    "{entry:?} is not a Character and a name. Write character:name, \
                     or the Character alone to name it after its package"
                ));
            }
            Ok(InstanceSpec::fresh(character, name))
        })
        .collect()
}

/// One spawned character: a Character plus a user-given name and a stable id.
pub struct Instance {
    pub id: InstanceId,
    pub name: String,
    character_name: String,
    /// This Instance's own layer of the Character Prompt, empty until the user
    /// writes one. Here rather than beside the Character, because it is what
    /// makes two Instances of one Character differ in voice (ADR-0012).
    prompt: String,
    engine: Engine,
    pending: Option<BehaviorProposal>,
}

impl Instance {
    /// Tick this Instance's Engine forward.
    pub fn tick(&mut self, snapshot: &WorldSnapshot) -> Frame {
        if let Some(proposal) = self.pending.take() {
            // Last write this frame wins: a queued Expression proposal
            // displaces a Director proposal already on the snapshot.
            let mut snapshot = snapshot.clone();
            snapshot.proposal = Some(proposal);
            self.engine.tick(&snapshot)
        } else {
            self.engine.tick(snapshot)
        }
    }

    /// Enqueue a BehaviorProposal to be applied on the next tick.
    pub fn enqueue(&mut self, proposal: BehaviorProposal) {
        self.pending = Some(proposal);
    }

    /// The Character this Instance is running.
    pub fn character_name(&self) -> &str {
        &self.character_name
    }

    /// Whether this Instance is in Do Not Disturb.
    /// Per Instance, not per app: silencing every character because one was told
    /// to sit quietly would make the setting mean something else.
    pub fn do_not_disturb(&self) -> bool {
        self.engine.do_not_disturb()
    }

    /// Toggle Do Not Disturb for this Instance.
    pub fn set_do_not_disturb(&mut self, enabled: bool) {
        self.engine.set_do_not_disturb(enabled)
    }

    /// Where this Instance's feet are.
    pub fn feet(&self) -> Point {
        self.engine.feet()
    }

    /// Which display this Instance is on, and where its feet sit there.
    pub fn whereabouts(&self, frames: &[Rect], usable: &[Rect]) -> Whereabouts {
        Whereabouts::locate(self.feet(), frames, usable)
    }

    /// Stand this Instance at `feet`, dropping any motion it had.
    pub fn stand_at(&mut self, feet: Point) {
        self.engine.stand_at(feet);
    }

    /// Switch this Instance to another Character without moving it.
    pub fn retarget(&mut self, character: &Character) {
        self.character_name = character.name.clone();
        self.engine.retarget(
            character.behaviors.clone(),
            character.near_reaction,
            character.rush_reaction,
        );
    }

    /// The name the user gave, which is what the menu and settings print.
    pub fn rename(&mut self, name: String) {
        self.name = name;
    }

    /// This Instance's own layer of the Character Prompt.
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// Take a new Instance Prompt. Bounded at the save surface by
    /// `instance_prompt`, which is where a refusal can still be read.
    pub fn set_prompt(&mut self, prompt: String) {
        self.prompt = prompt;
    }
}

/// Whether a name is the one an Instance gets when nobody named it.
/// Display name and package id differ in case and separators, so both squash.
/// An exact comparison would never match the spelling the app runs.
fn unnamed(name: &str, character: &str) -> bool {
    let squash = |text: &str| -> String {
        text.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect()
    };
    squash(name) == squash(character)
}

/// The name a lone Instance should wear for `character`.
/// Keep this Character's own default, any spelling. Replace a leftover default
/// from a previous Character (`bmo` on Timber Wolf). A typed name survives.
pub fn adopted_name<'a, I>(name: &str, character: &str, known: I) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    if unnamed(name, character) {
        return name.to_string();
    }
    if known.into_iter().any(|n| unnamed(name, n)) {
        return character.to_string();
    }
    name.to_string()
}

/// The roster of Character Instances.
#[derive(Default)]
pub struct Roster {
    instances: BTreeMap<InstanceId, Instance>,
    /// Display names of installed Characters. A leftover default squashes to
    /// one of these and not to the current Character. Without the catalog that
    /// leftover looks chosen.
    known_names: Vec<String>,
}

impl Roster {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the catalog `adopted_name` needs to see a leftover default.
    pub fn set_known_names(&mut self, names: impl IntoIterator<Item = impl Into<String>>) {
        self.known_names = names.into_iter().map(Into::into).collect();
    }

    /// Spawn a Character Instance with the given name at the given position.
    /// Borrowed, not owned: the art is unused here, and taking by value would
    /// copy every frame per character.
    pub fn spawn(&mut self, character: &Character, name: String, position: Point) -> InstanceId {
        self.restore(character, name, position, None, String::new())
    }

    /// Spawn and restore a prior id and Instance Prompt.
    /// `spawn` is this with neither. A new id every launch would lose the
    /// prompt that hangs off the old one (ADR-0012).
    pub fn restore(
        &mut self,
        character: &Character,
        name: String,
        position: Point,
        id: Option<InstanceId>,
        prompt: String,
    ) -> InstanceId {
        let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let engine = Engine::new(position)
            .with_behaviors(character.behaviors.clone())
            // How much room a Perch near the top of a display has to leave,
            // which is this Character's own height rather than a guess at the
            // tallest one anybody ships. #395.
            .with_sprite_height(character.sprite_height())
            .with_cursor_reactions(character.near_reaction, character.rush_reaction)
            // The id is this Instance's one random number, so two characters of
            // one Character do not draw the same idle variants at the same moments.
            .with_variant_seed(variant_seed(&id));
        let name = if self.instances.is_empty() {
            adopted_name(
                &name,
                &character.name,
                self.known_names.iter().map(String::as_str),
            )
        } else {
            name
        };
        let instance = Instance {
            id: id.clone(),
            name,
            character_name: character.name.clone(),
            prompt,
            engine,
            pending: None,
        };
        self.instances.insert(id.clone(), instance);
        id
    }

    /// Dismiss the Instance with the given id.
    ///
    /// Memory is untouched.
    pub fn dismiss(&mut self, id: &str) -> Option<Instance> {
        self.instances.remove(id)
    }

    /// List all Instances: id and name.
    pub fn list(&self) -> Vec<(InstanceId, String)> {
        self.instances
            .iter()
            .map(|(id, instance)| (id.clone(), instance.name.clone()))
            .collect()
    }

    /// Get a mutable reference to an Instance by id.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Instance> {
        self.instances.get_mut(id)
    }

    /// Get a reference to an Instance by id.
    pub fn get(&self, id: &str) -> Option<&Instance> {
        self.instances.get(id)
    }

    /// Switch one Instance's Character. False when the id is unknown.
    /// A lone unnamed Instance takes the new Character's name. A leftover
    /// default does too. A typed name, or any name with several characters up, stays.
    pub fn retarget(&mut self, id: &str, character: &Character) -> bool {
        let alone = self.instances.len() == 1;
        match self.instances.get_mut(id) {
            Some(instance) => {
                if alone {
                    let known: Vec<&str> = std::iter::once(instance.character_name.as_str())
                        .chain(self.known_names.iter().map(String::as_str))
                        .collect();
                    let next = adopted_name(&instance.name, &character.name, known);
                    instance.rename(next);
                }
                instance.retarget(character);
                true
            }
            None => false,
        }
    }

    /// Rename one Instance. False when the id is unknown.
    pub fn rename(&mut self, id: &str, name: String) -> bool {
        match self.instances.get_mut(id) {
            Some(instance) => {
                instance.rename(name);
                true
            }
            None => false,
        }
    }

    /// Write one Instance's own prompt layer. False when the id is unknown.
    pub fn set_prompt(&mut self, id: &str, prompt: String) -> bool {
        match self.instances.get_mut(id) {
            Some(instance) => {
                instance.set_prompt(prompt);
                true
            }
            None => false,
        }
    }
}

/// How `id` seeds this Instance's variant ring.
/// A minted or restored uuid uses its first half. Anything else is hashed, so
/// two hand-edited ids do not lockstep the way a constant would.
fn variant_seed(id: &str) -> u64 {
    match uuid::Uuid::parse_str(id) {
        Ok(uuid) => uuid.as_u64_pair().0,
        // FNV-1a, which is five characters of arithmetic and needs no crate.
        Err(_) => id.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        }),
    }
}

impl crate::tools::ExpressionHandle for Roster {
    fn enqueue(&mut self, instance_id: &str, proposal: BehaviorProposal) -> bool {
        match self.instances.get_mut(instance_id) {
            Some(instance) => {
                instance.enqueue(proposal);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::{
        Animation, Behavior, Character, CursorReaction, Primitive, DEFAULT_MODEL_BASE,
        DEFAULT_MODEL_POWER, DEFAULT_WEIGHT,
    };
    use crate::engine::{Point, Rect, Verb};
    use std::collections::BTreeMap;

    /// A minimal test Character with the Required Animation Set.
    fn test_character(name: &str) -> Character {
        let mut animations = BTreeMap::new();
        let required = [
            "idle", "walk", "fall", "land", "sit", "sleep", "react", "talk", "hold",
        ];
        for anim in required {
            animations.insert(
                anim.to_string(),
                Animation {
                    frames: vec![format!("{anim}-0.png")],
                    frame_size: (32, 32),
                    fps: 8,
                    looping: true,
                    variants: Vec::new(),
                    left_strip: None,
                    weight: DEFAULT_WEIGHT,
                },
            );
        }

        let mut behaviors = BTreeMap::new();
        behaviors.insert(
            "wave".to_string(),
            Behavior {
                primitives: vec![Primitive::React],
                then: None,
                weight: 1,
                trigger: None,
            },
        );
        behaviors.insert(
            "sleep".to_string(),
            Behavior {
                primitives: vec![Primitive::Sleep],
                then: None,
                weight: 1,
                trigger: None,
            },
        );

        Character {
            name: name.to_string(),
            personality: format!("A test character named {name}"),
            animations,
            behaviors,
            art: BTreeMap::new(),
            smooth: false,
            scale: 1,
            model_base: DEFAULT_MODEL_BASE,
            model_power: DEFAULT_MODEL_POWER,
            near_reaction: CursorReaction::default(),
            rush_reaction: CursorReaction::default(),
            source: None,
        }
    }

    fn test_snapshot() -> WorldSnapshot {
        WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            }],
            windows: vec![],
            cursor: Point { x: 100.0, y: 100.0 },
            verbs: vec![],
            poke_settled: false,
            elapsed_ms: 16,
            proposal: None,
            poll_generation: 0,
            composing: false,
            locomotion_frozen: false,
        }
    }

    #[test]
    fn spawning_an_instance_records_the_given_name_and_a_stable_id() {
        let mut roster = Roster::new();
        let character = test_character("Blip");

        let id = roster.spawn(
            &character,
            "Buddy One".to_string(),
            Point { x: 100.0, y: 100.0 },
        );

        assert!(!id.is_empty(), "the id is not empty");
        assert_eq!(
            roster.list(),
            vec![(id.clone(), "Buddy One".to_string())],
            "the Instance appears in the roster with its name"
        );
    }

    #[test]
    fn an_instance_reports_the_display_its_feet_are_on() {
        let mut roster = Roster::new();
        let character = test_character("Blip");
        let id = roster.spawn(
            &character,
            "Buddy".to_string(),
            Point {
                x: 2000.0,
                y: 100.0,
            },
        );
        let frames = [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            Rect {
                x: 1920.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
        ];

        let here = roster
            .get(&id)
            .expect("the spawned instance")
            .whereabouts(&frames, &frames);
        let place = here.placement().expect("on the right display");

        assert_eq!(place.index(), 1);
        assert_eq!(place.x(), 80.0);
        assert_eq!(place.y(), 100.0);
    }

    #[test]
    fn two_instances_of_the_same_character_play_different_behaviors_independently() {
        let mut roster = Roster::new();
        let character = test_character("Blip");

        let id_a = roster.spawn(
            &character,
            "Buddy A".to_string(),
            Point {
                x: 100.0,
                y: 1000.0,
            },
        );
        let id_b = roster.spawn(
            &character,
            "Buddy B".to_string(),
            Point {
                x: 500.0,
                y: 1000.0,
            },
        );

        let grounded_snapshot = test_snapshot();
        for _ in 0..50 {
            roster.get_mut(&id_a).unwrap().tick(&grounded_snapshot);
            roster.get_mut(&id_b).unwrap().tick(&grounded_snapshot);
        }

        let frame_a_check = roster.get_mut(&id_a).unwrap().tick(&grounded_snapshot);
        let frame_b_check = roster.get_mut(&id_b).unwrap().tick(&grounded_snapshot);

        assert!(
            frame_a_check.state == crate::engine::State::Grounded,
            "Instance A is grounded before behavior proposal, state: {:?}",
            frame_a_check.state
        );
        assert!(
            frame_b_check.state == crate::engine::State::Grounded,
            "Instance B is grounded before behavior proposal, state: {:?}",
            frame_b_check.state
        );

        let mut snapshot_wave = test_snapshot();
        snapshot_wave.proposal = Some(crate::engine::BehaviorProposal {
            behavior: "wave".to_string(),
            dialogue: None,
        });

        let mut snapshot_sleep = test_snapshot();
        snapshot_sleep.proposal = Some(crate::engine::BehaviorProposal {
            behavior: "sleep".to_string(),
            dialogue: None,
        });

        let frame_a = roster.get_mut(&id_a).unwrap().tick(&snapshot_wave);
        let frame_b = roster.get_mut(&id_b).unwrap().tick(&snapshot_sleep);

        assert_eq!(
            frame_a.animation, "react",
            "Instance A plays wave (which starts with react)"
        );
        assert_eq!(
            frame_b.animation, "sleep",
            "Instance B plays sleep independently"
        );
        assert_ne!(
            frame_a.animation, frame_b.animation,
            "the two Instances are playing different animations"
        );
    }

    #[test]
    fn two_instances_are_independently_positionable() {
        let mut roster = Roster::new();
        let character = test_character("Blip");

        let id_a = roster.spawn(
            &character,
            "Buddy A".to_string(),
            Point { x: 100.0, y: 200.0 },
        );
        let id_b = roster.spawn(
            &character,
            "Buddy B".to_string(),
            Point { x: 500.0, y: 600.0 },
        );

        let mut snapshot_a = test_snapshot();
        snapshot_a.verbs = vec![Verb::Grab];
        snapshot_a.cursor = Point { x: 120.0, y: 220.0 };

        let mut snapshot_b = test_snapshot();
        snapshot_b.verbs = vec![Verb::Grab];
        snapshot_b.cursor = Point { x: 520.0, y: 620.0 };

        let frame_a = roster.get_mut(&id_a).unwrap().tick(&snapshot_a);
        let frame_b = roster.get_mut(&id_b).unwrap().tick(&snapshot_b);

        assert_ne!(
            frame_a.position, frame_b.position,
            "the two Instances have different positions"
        );
        assert!(
            (frame_a.position.x - 120.0).abs() < 50.0,
            "Instance A is near its cursor position"
        );
        assert!(
            (frame_b.position.x - 520.0).abs() < 50.0,
            "Instance B is near its cursor position"
        );
    }

    #[test]
    fn listing_returns_both_instances_and_dismissing_one_leaves_the_other() {
        let mut roster = Roster::new();
        let character = test_character("Blip");

        let id_a = roster.spawn(
            &character,
            "Buddy A".to_string(),
            Point { x: 100.0, y: 100.0 },
        );
        let id_b = roster.spawn(
            &character,
            "Buddy B".to_string(),
            Point { x: 500.0, y: 100.0 },
        );

        let list = roster.list();
        assert_eq!(list.len(), 2, "both Instances appear");
        assert!(
            list.iter().any(|(_, name)| name == "Buddy A"),
            "Buddy A is listed"
        );
        assert!(
            list.iter().any(|(_, name)| name == "Buddy B"),
            "Buddy B is listed"
        );

        let dismissed = roster.dismiss(&id_a);
        assert!(dismissed.is_some(), "dismissing returns the Instance");
        assert_eq!(dismissed.unwrap().name, "Buddy A");

        let list = roster.list();
        assert_eq!(list.len(), 1, "one Instance remains");
        assert_eq!(list[0], (id_b.clone(), "Buddy B".to_string()));

        roster.dismiss(&id_b);
        assert_eq!(
            roster.list().len(),
            0,
            "an empty list after dismissing both"
        );
    }

    #[test]
    fn one_spec_names_a_character_and_what_to_call_it() {
        assert_eq!(
            parse_specs("bmo:Blip"),
            Ok(vec![InstanceSpec::fresh("bmo", "Blip")])
        );
    }

    /// Whitespace around either half is a user typing a list, not a Character
    /// whose name begins with a space.
    #[test]
    fn several_specs_are_read_in_order_and_whitespace_is_trimmed() {
        assert_eq!(
            parse_specs(" bmo:Blip ,  jotaro:Jo "),
            Ok(vec![
                InstanceSpec::fresh("bmo", "Blip"),
                InstanceSpec::fresh("jotaro", "Jo"),
            ])
        );
    }

    /// A Character named alone is called after its package. Demanding
    /// `bmo:bmo` would tax the shortest thing anyone will write.
    #[test]
    fn a_character_named_alone_is_called_after_its_package() {
        assert_eq!(
            parse_specs("bmo,jotaro:Jo"),
            Ok(vec![
                InstanceSpec::fresh("bmo", "bmo"),
                InstanceSpec::fresh("jotaro", "Jo"),
            ])
        );
    }

    #[test]
    fn the_same_character_twice_is_two_specs() {
        let specs = parse_specs("bmo:One,bmo:Two").expect("both parse");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].character, specs[1].character);
        assert_ne!(specs[0].name, specs[1].name);
    }

    /// Nothing named is not an error. An unset variable and a trailing comma
    /// are the same request, not a failed start.
    #[test]
    fn nothing_named_asks_for_no_instances_rather_than_failing() {
        assert_eq!(parse_specs(""), Ok(Vec::new()));
        assert_eq!(parse_specs("   "), Ok(Vec::new()));
        assert_eq!(parse_specs(",,"), Ok(Vec::new()));
        assert_eq!(
            parse_specs("bmo:Blip,"),
            Ok(vec![InstanceSpec::fresh("bmo", "Blip")])
        );
    }

    /// A colon the user typed is a name they meant to give, so an empty half is
    /// a typo to report rather than a default to guess at.
    #[test]
    fn a_colon_with_either_half_missing_is_reported() {
        assert!(parse_specs("bmo:").is_err(), "no name after the colon");
        assert!(parse_specs(":Blip").is_err(), "no Character before it");
        assert!(parse_specs("bmo:Blip,:Jo").is_err(), "the second is bad");
    }

    /// The message names the offending entry. A list of several is read back by
    /// finding which one is wrong, and "invalid" alone does not say.
    #[test]
    fn the_error_quotes_the_entry_that_could_not_be_read() {
        let why = parse_specs("bmo:Blip,nope:").expect_err("the second is bad");
        assert!(why.contains("nope:"), "the entry is quoted: {why}");
    }

    /// Switching Character is a new set of Behaviors, not a new body. The id
    /// and the name stay, or settings would lose the character it just renamed.
    #[test]
    fn retargeting_keeps_the_instance_and_changes_the_character() {
        let mut roster = Roster::new();
        let first = test_character("bmo");
        let second = test_character("nim");
        let id = roster.spawn(&first, "Beemo".to_string(), Point { x: 10.0, y: 20.0 });

        assert!(roster.retarget(&id, &second));
        let instance = roster.get(&id).expect("still there");
        assert_eq!(instance.character_name(), "nim");
        assert_eq!(instance.name, "Beemo");
        assert!(!roster.retarget("missing", &second));
    }

    /// Both the display name and the package id count as this Character's
    /// default, because the Shell passes the package id when settings carry only that.
    #[test]
    fn switching_renames_an_instance_that_still_wears_its_characters_name() {
        let wolf = test_character("Timber Wolf");

        let mut roster = Roster::new();
        let id = roster.spawn(
            &test_character("BMO"),
            "BMO".to_string(),
            Point { x: 10.0, y: 20.0 },
        );
        assert!(roster.retarget(&id, &wolf));
        assert_eq!(
            roster.list(),
            vec![(id, "Timber Wolf".to_string())],
            "the display name follows the Character"
        );

        let mut roster = Roster::new();
        let id = roster.spawn(
            &test_character("BMO"),
            "bmo".to_string(),
            Point { x: 10.0, y: 20.0 },
        );
        assert!(roster.retarget(&id, &wolf));
        assert_eq!(
            roster.list(),
            vec![(id, "Timber Wolf".to_string())],
            "the package id is the same default, spelled the way settings spell it"
        );
    }

    /// A name the user typed is the one thing the rule has to protect.
    #[test]
    fn switching_leaves_a_name_the_user_chose() {
        let mut roster = Roster::new();
        let id = roster.spawn(
            &test_character("BMO"),
            "Pip".to_string(),
            Point { x: 10.0, y: 20.0 },
        );

        assert!(roster.retarget(&id, &test_character("Timber Wolf")));
        assert_eq!(roster.list(), vec![(id, "Pip".to_string())]);
    }

    /// With several characters up, names are what tell them apart, and renaming
    /// one on a switch could hand it a name another already answers to.
    #[test]
    fn switching_renames_nothing_when_more_than_one_instance_runs() {
        let mut roster = Roster::new();
        let bmo = test_character("BMO");
        let first = roster.spawn(&bmo, "BMO".to_string(), Point { x: 10.0, y: 20.0 });
        let second = roster.spawn(&bmo, "BMO".to_string(), Point { x: 40.0, y: 20.0 });

        assert!(roster.retarget(&first, &test_character("Timber Wolf")));
        let names: Vec<_> = roster.list().into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, vec!["BMO".to_string(), "BMO".to_string()]);
        assert_eq!(
            roster.get(&first).expect("still there").character_name(),
            "Timber Wolf",
            "the Character still switches"
        );
        assert_eq!(
            roster.get(&second).expect("still there").character_name(),
            "BMO"
        );
    }

    /// Production change that would fail this: returning `name` unchanged
    /// when it is another Character's default. That leftover is
    /// `{ character: "Timber Wolf", name: "bmo" }`.
    #[test]
    fn adopted_name_replaces_another_characters_default() {
        assert_eq!(
            adopted_name("bmo", "Timber Wolf", ["BMO", "Timber Wolf"]),
            "Timber Wolf"
        );
        assert_eq!(
            adopted_name("BMO", "Timber Wolf", ["BMO", "Timber Wolf"]),
            "Timber Wolf"
        );
    }

    #[test]
    fn adopted_name_keeps_this_characters_package_id() {
        assert_eq!(
            adopted_name("bmo", "BMO", ["BMO", "Timber Wolf"]),
            "bmo",
            "this Character's default spelling is not a leftover"
        );
    }

    #[test]
    fn adopted_name_keeps_a_name_the_user_chose() {
        assert_eq!(
            adopted_name("Pip", "Timber Wolf", ["BMO", "Timber Wolf"]),
            "Pip"
        );
    }

    /// Spawn is the launch path, so a leftover `{ character: "Timber Wolf",
    /// name: "bmo" }` has to die here or every restart reprints it.
    #[test]
    fn spawning_a_lone_instance_drops_another_characters_default_name() {
        let mut roster = Roster::new();
        roster.set_known_names(["BMO", "Timber Wolf"]);
        let id = roster.spawn(
            &test_character("Timber Wolf"),
            "bmo".to_string(),
            Point { x: 10.0, y: 20.0 },
        );
        assert_eq!(roster.list(), vec![(id, "Timber Wolf".to_string())]);
    }

    /// A leftover already on the desk still has to rename on a later switch,
    /// because `unnamed` is false once persist has mixed name and Character.
    #[test]
    fn switching_renames_a_default_that_belongs_to_a_different_character() {
        let mut roster = Roster::new();
        roster.set_known_names(["BMO", "Timber Wolf", "Cat"]);
        let id = roster.spawn(
            &test_character("Timber Wolf"),
            "bmo".to_string(),
            Point { x: 10.0, y: 20.0 },
        );
        // Force the leftover onto the Instance so this test does not depend
        // on spawn already having adopted. The persist looks like this.
        assert!(roster.rename(&id, "bmo".to_string()));
        assert!(roster.retarget(&id, &test_character("Cat")));
        assert_eq!(roster.list(), vec![(id, "Cat".to_string())]);
    }

    /// The text hangs off the Instance's id. Minting a fresh uuid every launch
    /// would key it to an id that never comes back (ADR-0012).
    #[test]
    fn a_restored_instance_keeps_its_id_and_the_prompt_written_against_it() {
        let character = test_character("bmo");
        let mut roster = Roster::new();
        let id = roster.spawn(&character, "Beemo".to_string(), Point { x: 10.0, y: 20.0 });
        assert!(roster.set_prompt(&id, "Answer in haiku.".to_string()));

        // What the Shell persists on one launch and reads back on the next.
        let spec = InstanceSpec {
            character: "bmo".to_string(),
            name: "Beemo".to_string(),
            id: Some(id.clone()),
            prompt: roster.get(&id).expect("still there").prompt().to_string(),
        };
        let written = serde_json::to_string(&spec).expect("a spec serialises");
        let read: InstanceSpec = serde_json::from_str(&written).expect("and reads back");

        let mut restarted = Roster::new();
        let again = restarted.restore(
            &character,
            read.name.clone(),
            Point { x: 10.0, y: 20.0 },
            read.id.clone(),
            read.prompt.clone(),
        );

        assert_eq!(again, id, "the same Instance, not a new one beside it");
        assert_eq!(
            restarted.get(&again).expect("spawned").prompt(),
            "Answer in haiku.",
            "the text the user wrote survived the restart"
        );
    }

    /// The text follows the Instance, not the Character (ADR-0012).
    /// Production change that would fail this: clearing the prompt in
    /// `Roster::retarget` beside the Behaviors and the name.
    #[test]
    fn switching_character_keeps_the_instance_prompt() {
        let mut roster = Roster::new();
        let id = roster.spawn(
            &test_character("BMO"),
            "Pip".to_string(),
            Point { x: 10.0, y: 20.0 },
        );
        assert!(roster.set_prompt(&id, "Answer in haiku.".to_string()));

        assert!(roster.retarget(&id, &test_character("Timber Wolf")));

        assert_eq!(
            roster.get(&id).expect("still there").prompt(),
            "Answer in haiku."
        );
    }

    /// The reopened session's opening turn is the new Character's personality
    /// with the text the Instance kept. That is the moment the Director is told both.
    #[test]
    fn a_switched_instance_opens_the_new_character_with_the_text_it_kept() {
        let wolf = test_character("Timber Wolf");
        let mut roster = Roster::new();
        let id = roster.spawn(
            &test_character("BMO"),
            "Pip".to_string(),
            Point { x: 10.0, y: 20.0 },
        );
        assert!(roster.set_prompt(&id, "Answer in haiku.".to_string()));
        assert!(roster.retarget(&id, &wolf));

        // The two authored layers as the Shell hands them to the Director:
        // the new Character's, and this Instance's own.
        let instance = roster.get(&id).expect("still there");
        let opening = crate::director::character_prompt(
            &crate::director::Context {
                activity: crate::sensing::Activity {
                    hour: 9,
                    minute: 0,
                    ..crate::sensing::Activity::quiet()
                },
                personality: wolf.personality.clone(),
                instance_prompt: instance.prompt().to_string(),
                ..crate::director::Context::quiet()
            },
            wolf.behaviors.keys(),
            false,
        );

        assert!(
            opening.contains("A test character named Timber Wolf"),
            "the new Character's own layer: {opening}"
        );
        assert!(
            opening.contains("Answer in haiku."),
            "and the layer the Instance kept: {opening}"
        );
    }

    /// A spec with no id is an Instance that has not run. It gets one minted.
    #[test]
    fn a_spec_with_no_id_is_spawned_under_a_fresh_one() {
        let mut roster = Roster::new();
        let id = roster.restore(
            &test_character("bmo"),
            "Beemo".to_string(),
            Point { x: 10.0, y: 20.0 },
            None,
            String::new(),
        );

        assert!(!id.is_empty(), "an Instance still gets an id");
        assert!(roster.get(&id).expect("spawned").prompt().is_empty());
    }

    /// Production change that would fail this: making the two new fields
    /// required. A settings file written before this feature would stop
    /// deserialising, and the user's whole roster would go with it.
    #[test]
    fn a_settings_file_written_before_the_instance_prompt_still_loads() {
        let spec: InstanceSpec =
            serde_json::from_str(r#"{"character":"bmo","name":"Beemo"}"#).expect("today's file");

        assert_eq!(spec.character, "bmo");
        assert_eq!(spec.name, "Beemo");
        assert_eq!(spec.id, None, "nothing to restore, so an id is minted");
        assert!(spec.prompt.is_empty(), "empty by default (ADR-0012)");
    }

    /// The user's layer has the bound the author's already has, counted in
    /// characters rather than bytes: an accented paragraph is not twice the
    /// prose of a plain one (ADR-0012).
    #[test]
    fn an_instance_prompt_at_the_limit_is_taken_and_one_over_it_is_refused() {
        let at_limit = "é".repeat(INSTANCE_PROMPT_LIMIT);
        assert_eq!(
            instance_prompt(&at_limit),
            Ok(at_limit.clone()),
            "the limit itself is not over it"
        );

        let over = "é".repeat(INSTANCE_PROMPT_LIMIT + 1);
        let why = instance_prompt(&over).expect_err("one character over the limit");
        assert!(
            why.contains(&(INSTANCE_PROMPT_LIMIT + 1).to_string()),
            "a user who pasted a page needs to know how much to cut: {why}"
        );
        assert!(
            why.contains(&INSTANCE_PROMPT_LIMIT.to_string()),
            "and what to cut it to: {why}"
        );
    }

    /// Production change that would fail this: cutting an over-long prompt to
    /// the limit instead of refusing it. Silent truncation loses words the user
    /// typed and shows them nothing.
    #[test]
    fn an_over_long_instance_prompt_is_not_quietly_cut() {
        assert!(instance_prompt(&"a".repeat(INSTANCE_PROMPT_LIMIT + 40)).is_err());
    }

    #[test]
    fn renaming_changes_only_the_name() {
        let mut roster = Roster::new();
        let character = test_character("bmo");
        let id = roster.spawn(&character, "old".to_string(), Point { x: 0.0, y: 0.0 });

        assert!(roster.rename(&id, "new".to_string()));
        assert_eq!(roster.list(), vec![(id, "new".to_string())]);
        assert!(!roster.rename("missing", "x".to_string()));
    }
}
