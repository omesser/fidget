//! The last few seconds of an Instance, for the wake that answers a verb.
//!
//! The Engine reacts to a verb at once, so by the time the Director hears it
//! the sprite is already awake, or off the wall. `Recency` keeps what came
//! before: the state and Behavior the verb found, how many times in a row the
//! same verb arrived, and the short run of states and Behaviors behind it.
//! Kept by the Shell, one per Instance, fed once per tick, and read when a
//! wake is built. A wake gets no timestamps or ages: the window is short
//! enough to mean now.

use std::collections::VecDeque;
use std::time::Duration;

use super::Happened;
use crate::engine::{Frame, State, Verb};
use crate::possession::Move;

/// How far back the window reaches, and how long a verb's snapshot or a
/// streak lasts without a wake to read it.
pub const WINDOW: Duration = Duration::from_secs(12);

/// The most steps the window hands a wake. Keeps the line near 20 tokens.
pub const STEPS: usize = 5;

/// What the sprite was doing at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doing {
    pub state: State,
    pub behavior: Option<String>,
}

/// One change the sprite went through: into a State, into a Behavior, or a
/// move the user drove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    State(State),
    Behavior(String),
    Driven(Move),
}

/// What a wake is told about the last few seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lately {
    /// What the verb this wake answers found the sprite doing.
    pub was: Option<Doing>,
    /// The Behavior playing as the wake is built.
    pub doing: Option<String>,
    /// How many times in a row the wake's verb arrived, this one included.
    /// Zero when the wake answers no verb.
    pub streak: u32,
    /// The steps inside `WINDOW`, oldest first, at most `STEPS`.
    pub steps: Vec<Step>,
    /// Every move a possession drove and how often, first driven first.
    /// Filled only for the wake answering its end.
    pub driven: Vec<(Move, u32)>,
}

/// One Instance's last few seconds. Fed by `tick` and `heard`, read by `take`.
#[derive(Debug, Default)]
pub struct Recency {
    /// What the last tick ended with, which is what a verb this tick finds.
    last: Option<Doing>,
    /// Whether the last tick had verbs. A held Grab or an open menu sends one
    /// every tick, and only the first is what the gesture found.
    gesturing: bool,
    /// What the latest gesture found, and how long ago. Spent by a wake.
    was: Option<(Doing, Duration)>,
    /// The verb the Director last heard, how many in a row, and how long ago.
    streak: Option<(Happened, u32, Duration)>,
    /// Steps with their ages, oldest first.
    steps: VecDeque<(Step, Duration)>,
    /// The moves driven since possession started, outside `WINDOW`: a
    /// release sums up the whole possession.
    driven: Vec<(Move, u32)>,
}

impl Recency {
    /// One Engine tick. `verbs` are the ones this tick handed the Engine,
    /// `frame` what it made of them.
    pub fn tick(&mut self, elapsed: Duration, verbs: &[Verb], frame: &Frame) {
        for (_, age) in self.steps.iter_mut() {
            *age += elapsed;
        }
        self.steps.retain(|(_, age)| *age <= WINDOW);
        if let Some((_, age)) = &mut self.was {
            *age += elapsed;
        }
        self.was.take_if(|(_, age)| *age > WINDOW);
        if let Some((_, _, age)) = &mut self.streak {
            *age += elapsed;
        }

        if !verbs.is_empty() && !self.gesturing {
            self.was = self.last.clone().map(|found| (found, Duration::ZERO));
        }
        self.gesturing = !verbs.is_empty();

        // Ahead of the State it leads to: a launch is airborne the same tick.
        if let Some(moved) = frame.driven {
            self.step(Step::Driven(moved));
            match self.driven.iter_mut().find(|(seen, _)| *seen == moved) {
                Some((_, times)) => *times += 1,
                None => self.driven.push((moved, 1)),
            }
        }

        let now = Doing {
            state: frame.state,
            behavior: frame.playing_behavior.clone(),
        };
        let before = self.last.replace(now.clone());
        if before.as_ref().map(|doing| doing.state) != Some(now.state) {
            self.step(Step::State(now.state));
        }
        if let Some(name) = now.behavior {
            if before.and_then(|doing| doing.behavior).as_ref() != Some(&name) {
                self.step(Step::Behavior(name));
            }
        }
    }

    /// A possession started. Its release sums up its own moves only, even when
    /// a later verb took the last release's wake.
    pub fn possessed(&mut self) {
        self.driven.clear();
    }

    /// A verb the Director will hear, as `touched` names it. Counts the streak.
    pub fn heard(&mut self, happened: &Happened) {
        let count = match &self.streak {
            Some((last, count, age)) if last == happened && *age <= WINDOW => count + 1,
            _ => 1,
        };
        self.streak = Some((happened.clone(), count, Duration::ZERO));
    }

    /// What the wake answering `happened` is told. Spends the verb's snapshot,
    /// so the next wake reads the next verb's.
    pub fn take(&mut self, happened: &Happened) -> Lately {
        let streak = match &self.streak {
            Some((last, count, _)) if last == happened => *count,
            _ => 0,
        };
        let skip = self.steps.len().saturating_sub(STEPS);
        let driven = match happened {
            Happened::PossessionEnd => std::mem::take(&mut self.driven),
            _ => Vec::new(),
        };
        Lately {
            driven,
            was: self.was.take().map(|(found, _)| found),
            doing: self.last.as_ref().and_then(|now| now.behavior.clone()),
            streak,
            steps: self
                .steps
                .iter()
                .skip(skip)
                .map(|(step, _)| step.clone())
                .collect(),
        }
    }

    /// Consecutive repeats collapse, so a flicker is one step.
    fn step(&mut self, step: Step) {
        if self.steps.back().map(|(newest, _)| newest) != Some(&step) {
            self.steps.push_back((step, Duration::ZERO));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::character::{Behavior, Primitive};
    use crate::director::{follow_up, Context};
    use crate::engine::{BehaviorProposal, Engine, Point, Rect, WorldSnapshot};
    use crate::possession::{Arrow, Intent, Key, Possession};

    /// An Instance as the frame loop runs one: the Engine ticks, `Recency`
    /// hears every tick, and a wake reads it.
    struct Instance {
        engine: Engine,
        recency: Recency,
        frame: Frame,
    }

    impl Instance {
        fn at(x: f64, y: f64) -> Self {
            let walk = Behavior {
                primitives: vec![Primitive::Walk],
                then: None,
                weight: 1,
                trigger: None,
            };
            let mut engine = Engine::new(Point { x, y })
                .with_behaviors(BTreeMap::from([("walk".to_string(), walk)]));
            let mut instance = Self {
                frame: engine.tick(&snapshot(0, vec![])),
                engine,
                recency: Recency::default(),
            };
            // Long enough that landing has left the window.
            instance.wait(13_000);
            instance
        }

        fn tick(&mut self, snapshot: WorldSnapshot) {
            self.frame = self.engine.tick(&snapshot);
            self.recency.tick(
                Duration::from_millis(u64::from(snapshot.elapsed_ms)),
                &snapshot.verbs,
                &self.frame,
            );
        }

        fn wait(&mut self, ms: u32) {
            for _ in 0..ms / 100 {
                self.tick(snapshot(100, vec![]));
            }
        }

        fn verb(&mut self, verb: Verb) {
            self.tick(snapshot(100, vec![verb]));
        }

        /// A click, then the double-click interval, then the Director hears it.
        fn poke(&mut self) {
            self.verb(Verb::Poke);
            self.wait(400);
            self.recency.heard(&Happened::Poke);
        }

        fn wake(&mut self, happened: Happened) -> String {
            follow_up(&Context {
                state: self.frame.state,
                lately: self.recency.take(&happened),
                happened,
                ..Context::quiet()
            })
        }
    }

    fn snapshot(elapsed_ms: u32, verbs: Vec<Verb>) -> WorldSnapshot {
        WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 800.0,
            }],
            elapsed_ms,
            verbs,
            ..WorldSnapshot::default()
        }
    }

    fn floor(prompt: &str) -> String {
        format!("{prompt}recent: (none)\ntime: Sun 00:00\n")
    }

    #[test]
    fn poked_awake_the_wake_says_it_was_asleep() {
        let mut sprite = Instance::at(100.0, 0.0);
        sprite.tick(snapshot(60_000, vec![]));
        sprite.wait(30_000);

        sprite.poke();

        assert_eq!(
            sprite.wake(Happened::Poke),
            format!(
                "{}state: idle\nstanding on: nothing\n",
                floor("what just happened: poked\nwas: asleep\n")
            )
        );
    }

    #[test]
    fn each_poke_in_a_row_is_counted_and_a_pause_starts_over() {
        let mut sprite = Instance::at(100.0, 0.0);
        let mut heard = Vec::new();
        for pause in [3_000, 3_000, 13_000, 0] {
            sprite.poke();
            let wake = sprite.wake(Happened::Poke);
            heard.push(wake.lines().next().unwrap().to_string());
            sprite.wait(pause);
        }

        assert_eq!(
            heard,
            [
                "what just happened: poked",
                "what just happened: poked (2 in a row)",
                "what just happened: poked (3 in a row)",
                "what just happened: poked",
            ]
        );
    }

    #[test]
    fn pulled_off_the_wall_mid_climb_the_wake_says_it_was_climbing() {
        let mut sprite = Instance::at(900.0, 400.0);
        sprite.tick(WorldSnapshot {
            cursor: Point { x: 900.0, y: 400.0 },
            ..snapshot(100, vec![Verb::Grab])
        });
        sprite.verb(Verb::Throw {
            velocity: Point { x: 2000.0, y: 0.0 },
        });
        sprite.wait(1_000);

        sprite.verb(Verb::Grab);
        sprite.recency.heard(&Happened::Grab);

        assert_eq!(
            sprite.wake(Happened::Grab),
            format!(
                "{}state: held\nlately: held → climbing → held\nstanding on: nothing\n",
                floor("what just happened: picked up\nwas: climbing\n")
            )
        );
    }

    #[test]
    fn a_poke_ends_a_behavior_and_the_wake_says_which() {
        let mut sprite = Instance::at(300.0, 0.0);
        sprite.tick(WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "walk".to_string(),
                dialogue: None,
            }),
            ..snapshot(100, vec![])
        });
        sprite.wait(500);
        let asked = sprite.wake(Happened::Chat("where to?".to_string()));

        sprite.poke();

        assert_eq!(
            asked,
            format!(
                "{}state: idle\ndoing: walk\nstanding on: nothing\nthey said: where to?\n",
                floor("what just happened: spoken to\n")
            )
        );
        assert_eq!(
            sprite.wake(Happened::Poke),
            format!(
                "{}state: idle\nstanding on: nothing\n",
                floor("what just happened: poked\nwas: idle, doing walk\n")
            )
        );
    }

    #[test]
    fn a_release_hears_the_drive_marked_and_summed() {
        let mut sprite = Instance::at(500.0, 0.0);
        sprite.engine.set_possessed(true);
        let keys = |walk, launch| WorldSnapshot {
            possession: Intent {
                walk,
                launch,
                ..Intent::default()
            },
            ..snapshot(100, vec![])
        };
        let tap = {
            let mut space = Possession::default();
            space.key(Key::Space, true);
            space.key(Key::Space, false);
            space.tick(0).launch
        };
        for _ in 0..3 {
            sprite.tick(keys(Some(Arrow::Left), None));
        }
        sprite.tick(keys(Some(Arrow::Right), None));
        sprite.tick(keys(None, None));
        sprite.tick(keys(Some(Arrow::Left), None));
        sprite.tick(keys(None, tap));
        sprite.wait(2_000);
        sprite.engine.set_possessed(false);

        assert_eq!(
            sprite.wake(Happened::PossessionEnd),
            format!(
                "{}state: idle\ndriven: walk-left ×2, walk-right ×1, jump ×1\n\
                 lately: walk-right (driven) → walk-left (driven) → jump (driven) → \
                 falling → idle\nstanding on: nothing\n",
                floor("what just happened: released from possession\n")
            )
        );
        assert!(
            !sprite.wake(Happened::PossessionEnd).contains("×"),
            "the summary is spent by the wake that reads it"
        );
    }

    #[test]
    fn a_release_no_wake_read_stays_out_of_the_next_sum() {
        let mut sprite = Instance::at(500.0, 0.0);
        let walk = |arrow| WorldSnapshot {
            possession: Intent {
                walk: Some(arrow),
                ..Intent::default()
            },
            ..snapshot(100, vec![])
        };
        sprite.engine.set_possessed(true);
        sprite.recency.possessed();
        sprite.tick(walk(Arrow::Left));
        sprite.engine.set_possessed(false);
        sprite.wake(Happened::Grab);

        sprite.engine.set_possessed(true);
        sprite.recency.possessed();
        sprite.tick(walk(Arrow::Right));
        sprite.engine.set_possessed(false);

        assert!(
            sprite
                .wake(Happened::PossessionEnd)
                .contains("\ndriven: walk-right ×1\n"),
            "only the second possession's walk"
        );
    }

    #[test]
    fn a_verb_no_wake_read_is_forgotten_with_the_window() {
        let mut sprite = Instance::at(100.0, 0.0);
        sprite.tick(snapshot(60_000, vec![]));
        sprite.wait(30_000);
        sprite.verb(Verb::Poke);

        sprite.wait(13_000);

        assert_eq!(
            sprite.wake(Happened::Proactive),
            format!(
                "{}state: idle\nstanding on: nothing\n",
                floor("what just happened: time passed\n")
            )
        );
    }

    #[test]
    fn a_busy_window_hands_the_wake_its_last_five_steps() {
        let mut sprite = Instance::at(900.0, 400.0);
        for _ in 0..3 {
            sprite.tick(WorldSnapshot {
                cursor: sprite.frame.position,
                ..snapshot(100, vec![Verb::Grab])
            });
            sprite.verb(Verb::Throw {
                velocity: Point { x: 2000.0, y: 0.0 },
            });
            sprite.wait(500);
        }

        assert_eq!(
            sprite.wake(Happened::Proactive),
            format!(
                "{}state: climbing\nlately: climbing → held → climbing → held → climbing\nstanding on: nothing\n",
                floor("what just happened: time passed\n")
            )
        );
    }
}
