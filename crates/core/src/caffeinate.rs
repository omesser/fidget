//! One macOS `caffeinate` child for this launch.
//!
//! `-w` is mandatory. A macOS child is reparented to `launchd` when fidget
//! dies, so a crash would leave the Mac awake without it.

/// The empty flag set cannot be saved or spawned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Empty;

impl Empty {
    pub const REASON: &'static str = "At least one caffeinate flag has to stay on.";
}

/// One `caffeinate` assertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    Display,
    Idle,
    Disk,
    System,
}

/// The one map from flag to its `caffeinate` letter, in argv order.
const LETTERS: [(Flag, char); 4] = [
    (Flag::Display, 'd'),
    (Flag::Idle, 'i'),
    (Flag::Disk, 'm'),
    (Flag::System, 's'),
];

/// A non-empty set of flags. One bit per [`Flag`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertedFlags(u8);

impl AssertedFlags {
    pub fn new(flags: impl IntoIterator<Item = Flag>) -> Result<Self, Empty> {
        let bits = flags.into_iter().fold(0, |bits, flag| bits | bit(flag));
        if bits == 0 {
            Err(Empty)
        } else {
            Ok(Self(bits))
        }
    }

    /// `-dims`, the default.
    pub fn all() -> Self {
        Self(LETTERS.iter().fold(0, |bits, (flag, _)| bits | bit(*flag)))
    }

    pub fn contains(self, flag: Flag) -> bool {
        self.0 & bit(flag) != 0
    }

    fn letters(self) -> impl Iterator<Item = char> {
        LETTERS
            .into_iter()
            .filter_map(move |(flag, letter)| self.contains(flag).then_some(letter))
    }

    /// The flag cluster Settings shows. `-dims`.
    pub fn preview(self) -> String {
        std::iter::once('-').chain(self.letters()).collect()
    }

    pub fn argv(self, pid: u32) -> Argv {
        Argv { flags: self, pid }
    }

    /// How Chat, the prompt line and Settings name what Caffeinate is blocking:
    /// the flag cluster (`-dims`) on macOS, the inhibitors on Linux.
    pub fn blocking_label(self) -> String {
        if cfg!(target_os = "linux") {
            LinuxPlan::of(self).describe()
        } else {
            self.preview()
        }
    }
}

/// What a flag set asks Linux to block. Pure, so every OS tests it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinuxPlan {
    /// The logind `what`: Idle is `idle`, System is `sleep`.
    pub logind: Option<&'static str>,
    /// Display is a screen saver inhibit.
    pub display: bool,
}

impl LinuxPlan {
    /// Disk has no Linux equivalent, so it adds nothing.
    pub fn of(flags: AssertedFlags) -> Self {
        let logind = match (flags.contains(Flag::Idle), flags.contains(Flag::System)) {
            (true, true) => Some("idle:sleep"),
            (true, false) => Some("idle"),
            (false, true) => Some("sleep"),
            (false, false) => None,
        };
        Self {
            logind,
            display: flags.contains(Flag::Display),
        }
    }

    pub fn is_empty(self) -> bool {
        self.logind.is_none() && !self.display
    }

    /// The Settings Command row, the Chat note and the prompt line.
    pub fn describe(self) -> String {
        let mut parts = Vec::new();
        if let Some(what) = self.logind {
            parts.push(format!("logind inhibit {what}"));
        }
        if self.display {
            parts.push("screen saver inhibit".to_string());
        }
        parts.join(" + ")
    }
}

fn bit(flag: Flag) -> u8 {
    1 << flag as u8
}

/// Closed argv. There is no utility slot, because `caffeinate` ignores `-w`
/// when a utility is also present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Argv {
    flags: AssertedFlags,
    pid: u32,
}

impl Argv {
    /// The flags alone, for a `Spawner` that takes no argv.
    pub fn flags(self) -> AssertedFlags {
        self.flags
    }

    /// The tokens `Command` is given.
    pub fn args(self) -> Vec<String> {
        let flags = self.flags.letters().map(|letter| format!("-{letter}"));
        flags
            .chain(["-w".to_string(), self.pid.to_string()])
            .collect()
    }
}

/// Whether Caffeinate is on, and with which flags. Absence of the feature is
/// `None` at the prompt, not a third variant here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fact {
    Off,
    On(AssertedFlags),
}

impl Fact {
    pub fn on(self) -> bool {
        matches!(self, Fact::On(_))
    }

    /// `caffeinate: off` or `caffeinate: on (-dims)`.
    pub fn prompt_line(self) -> String {
        match self {
            Fact::Off => "caffeinate: off".to_string(),
            Fact::On(flags) => format!("caffeinate: on ({})", flags.blocking_label()),
        }
    }
}

/// A Chat note. Not a tool result and not the prompt line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    On(AssertedFlags),
    Off,
    /// The child left while Caffeinate was still on and nobody killed it.
    Exited,
    Failed,
    /// A `Spawner` that works off the caller's thread could not turn Caffeinate
    /// on.
    Lost(String),
}

impl Note {
    pub fn text(&self) -> String {
        match self {
            Note::On(flags) => format!(
                "☕ Caffeinate on: keeping your computer awake ({})",
                flags.blocking_label()
            ),
            Note::Off => "Caffeinate off.".to_string(),
            Note::Exited => "Caffeinate off. It exited on its own.".to_string(),
            Note::Failed => "Caffeinate did not start.".to_string(),
            Note::Lost(reason) => format!("Caffeinate off. Could not take {reason}"),
        }
    }
}

/// Chat and the menu send [`Request::Toggle`]. The tool sends [`Request::Set`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Toggle,
    Set(bool),
}

/// Process boundary. `spawn` fails while Caffeinate is already on, so a second
/// start cannot hide the first.
pub trait Spawner {
    fn spawn(&mut self, argv: &Argv) -> Result<(), String>;
    fn kill(&mut self);
    /// Whether the child or the inhibitors are still in effect. Reaps a child
    /// that has left.
    fn alive(&mut self) -> bool;
    /// Why `alive` just reported off: Caffeinate never came on. Only a `Spawner`
    /// whose `spawn` returns before the OS answers has one.
    fn failure(&mut self) -> Option<String> {
        None
    }
}

/// The frame loop's Caffeinate, as `dispatch` sees it. `None` reads. `Err` is a
/// failed start or an empty flag set.
pub trait CaffeinateHandle {
    fn call(&mut self, on: Option<bool>) -> Result<Fact, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tick {
    pub fact: Fact,
    pub notes: Vec<Note>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Running(AssertedFlags),
    /// SIGTERM sent. Caffeinate still reads `flags`. Once the child is gone it
    /// spawns `flags` if `restart`, or turns off with a note.
    Stopping {
        flags: AssertedFlags,
        restart: bool,
    },
}

/// The only thing that turns Caffeinate on or off.
pub struct Machine<S: Spawner> {
    spawner: S,
    pid: u32,
    wanted: bool,
    phase: Phase,
}

impl<S: Spawner> Machine<S> {
    /// `pid` is the process the child waits on with `-w`.
    pub fn new(spawner: S, pid: u32) -> Self {
        Self {
            spawner,
            pid,
            wanted: false,
            phase: Phase::Idle,
        }
    }

    /// Apply requests, then reconcile the one child to what they want.
    pub fn tick(&mut self, flags: Result<AssertedFlags, Empty>, requests: &[Request]) -> Tick {
        let alive = self.spawner.alive();
        let failure = self.spawner.failure();
        let mut tick = Tick {
            fact: Fact::Off,
            notes: Vec::new(),
            error: None,
        };
        let restarted = self.finish_stop(alive, &mut tick);

        let was = self.wanted;
        for request in requests {
            let want = match request {
                Request::Toggle => !self.wanted,
                Request::Set(on) => *on,
            };
            if want && flags.is_err() {
                tick.error = Some(Empty::REASON.to_string());
            }
            self.wanted = want && flags.is_ok();
        }
        let announce = !was && self.wanted;
        let asked_on = requests
            .iter()
            .any(|request| matches!(request, Request::Set(true) | Request::Toggle));

        // `alive` described the old child if a restart just spawned a new one.
        if let (false, false, Phase::Running(spawned)) = (restarted, alive, self.phase) {
            self.phase = Phase::Idle;
            let changed = flags.is_ok_and(|next| next != spawned);
            if !self.wanted {
                tick.notes.push(Note::Off);
            } else if let Some(reason) = failure {
                tick.notes.push(Note::Lost(reason));
                self.wanted = false;
            } else if self.wanted && !changed && !asked_on {
                tick.notes.push(Note::Exited);
                self.wanted = false;
            }
        }

        match (self.wanted, self.phase, flags) {
            (true, Phase::Idle, Ok(flags)) => self.spawn(flags, announce, &mut tick),
            (true, Phase::Idle, Err(Empty)) => {
                self.wanted = false;
                tick.error = Some(Empty::REASON.to_string());
            }
            (false, Phase::Running(flags), _) => {
                self.spawner.kill();
                self.phase = Phase::Stopping {
                    flags,
                    restart: false,
                };
            }
            (true, Phase::Running(spawned), Ok(next)) if next != spawned => {
                self.spawner.kill();
                self.phase = Phase::Stopping {
                    flags: next,
                    restart: true,
                };
            }
            (true, Phase::Stopping { restart: false, .. }, Ok(next)) => {
                self.phase = Phase::Stopping {
                    flags: next,
                    restart: true,
                };
            }
            (
                false,
                Phase::Stopping {
                    flags,
                    restart: true,
                    ..
                },
                _,
            ) => {
                self.phase = Phase::Stopping {
                    flags,
                    restart: false,
                };
            }
            _ => {}
        }

        tick.fact = match self.phase {
            Phase::Idle => Fact::Off,
            Phase::Running(flags) | Phase::Stopping { flags, .. } => Fact::On(flags),
        };
        tick
    }

    /// Retries SIGTERM while the child is alive. Returns whether this call
    /// spawned the next child.
    fn finish_stop(&mut self, alive: bool, tick: &mut Tick) -> bool {
        let Phase::Stopping { flags, restart } = self.phase else {
            return false;
        };
        if alive {
            self.spawner.kill();
            return false;
        }
        self.phase = Phase::Idle;
        if restart {
            self.spawn(flags, false, tick);
            return true;
        }
        self.wanted = false;
        tick.notes.push(Note::Off);
        false
    }

    fn spawn(&mut self, flags: AssertedFlags, announce: bool, tick: &mut Tick) {
        match self.spawner.spawn(&flags.argv(self.pid)) {
            Ok(()) => {
                self.phase = Phase::Running(flags);
                self.wanted = true;
                if announce {
                    tick.notes.push(Note::On(flags));
                }
            }
            Err(reason) => {
                self.phase = Phase::Idle;
                self.wanted = false;
                tick.notes.push(Note::Failed);
                tick.error = Some(reason);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        running: bool,
        next: Option<bool>,
        spawns: Vec<Vec<String>>,
        kills: usize,
        fail: bool,
        failure: Option<String>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                running: false,
                next: None,
                spawns: Vec::new(),
                kills: 0,
                fail: false,
                failure: None,
            }
        }
    }

    impl Spawner for Fake {
        fn spawn(&mut self, argv: &Argv) -> Result<(), String> {
            if self.running {
                return Err("a child is already running".to_string());
            }
            if self.fail {
                return Err("caffeinate missing".to_string());
            }
            self.running = true;
            self.spawns.push(argv.args());
            Ok(())
        }

        fn kill(&mut self) {
            if self.running {
                self.kills += 1;
            }
        }

        fn alive(&mut self) -> bool {
            let alive = self.next.take().unwrap_or(self.running);
            self.running &= alive;
            alive
        }

        fn failure(&mut self) -> Option<String> {
            self.failure.take()
        }
    }

    fn dims() -> AssertedFlags {
        AssertedFlags::all()
    }

    fn drive(
        machine: &mut Machine<Fake>,
        flags: Result<AssertedFlags, Empty>,
        requests: &[Request],
        alive: bool,
    ) -> Tick {
        machine.spawner.next = Some(alive);
        machine.tick(flags, requests)
    }

    #[test]
    fn argv_is_flag_tokens_then_wait_pid() {
        let args = dims().argv(42).args();
        assert_eq!(
            args,
            vec!["-d", "-i", "-m", "-s", "-w", "42"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(dims().preview(), "-dims");
        assert_eq!(
            AssertedFlags::new([Flag::System, Flag::Display])
                .unwrap()
                .preview(),
            "-ds"
        );
        assert_eq!(AssertedFlags::new([Flag::Disk]).unwrap().preview(), "-m");
        assert_eq!(AssertedFlags::new([]), Err(Empty));
    }

    #[test]
    fn prompt_and_note_text_are_the_issue_sentences() {
        assert_eq!(Fact::Off.prompt_line(), "caffeinate: off");
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(Fact::On(dims()).prompt_line(), "caffeinate: on (-dims)");
            assert_eq!(
                Note::On(dims()).text(),
                "☕ Caffeinate on: keeping your computer awake (-dims)"
            );
        }
        assert_eq!(Note::Off.text(), "Caffeinate off.");
        assert_eq!(Note::Exited.text(), "Caffeinate off. It exited on its own.");
        assert_eq!(Note::Failed.text(), "Caffeinate did not start.");
        assert_eq!(
            Note::Lost("logind inhibit sleep: Access denied".to_string()).text(),
            "Caffeinate off. Could not take logind inhibit sleep: Access denied"
        );
    }

    /// Linux names the inhibitors, the same words as the Settings Command row.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_notes_and_prompt_name_the_inhibitors() {
        let named = "logind inhibit idle:sleep + screen saver inhibit";
        assert_eq!(dims().blocking_label(), named);
        assert_eq!(
            Fact::On(dims()).prompt_line(),
            format!("caffeinate: on ({named})")
        );
        assert_eq!(
            Note::On(dims()).text(),
            format!("☕ Caffeinate on: keeping your computer awake ({named})")
        );
    }

    fn flags(set: &[Flag]) -> AssertedFlags {
        AssertedFlags::new(set.iter().copied()).expect("a flag")
    }

    #[test]
    fn idle_and_system_are_one_logind_inhibitor_and_display_the_screen_saver() {
        assert_eq!(
            LinuxPlan::of(dims()),
            LinuxPlan {
                logind: Some("idle:sleep"),
                display: true
            }
        );
        assert_eq!(LinuxPlan::of(flags(&[Flag::Idle])).logind, Some("idle"));
        assert_eq!(LinuxPlan::of(flags(&[Flag::System])).logind, Some("sleep"));
        assert_eq!(
            LinuxPlan::of(flags(&[Flag::Display])),
            LinuxPlan {
                logind: None,
                display: true
            }
        );
        assert!(LinuxPlan::of(flags(&[Flag::Disk])).is_empty());
    }

    #[test]
    fn the_linux_plan_names_each_inhibitor() {
        assert_eq!(
            LinuxPlan::of(dims()).describe(),
            "logind inhibit idle:sleep + screen saver inhibit"
        );
        assert_eq!(
            LinuxPlan::of(flags(&[Flag::System])).describe(),
            "logind inhibit sleep"
        );
        assert_eq!(
            LinuxPlan::of(flags(&[Flag::Display])).describe(),
            "screen saver inhibit"
        );
    }

    /// A `Spawner` that works off the caller's thread reports a refusal after
    /// `spawn` returned Ok. Chat hears why, and Caffeinate stays
    /// off.
    #[test]
    fn caffeinate_that_could_not_turn_on_says_why_and_stays_off() {
        let mut machine = Machine::new(Fake::new(), 42);
        drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);
        machine.spawner.failure = Some("logind inhibit sleep: denied".to_string());
        let lost = drive(&mut machine, Ok(dims()), &[], false);
        assert_eq!(lost.fact, Fact::Off);
        assert_eq!(
            lost.notes,
            vec![Note::Lost("logind inhibit sleep: denied".to_string())]
        );
        let after = drive(&mut machine, Ok(dims()), &[], false);
        assert_eq!(after.fact, Fact::Off);
        assert!(after.notes.is_empty());
        assert_eq!(machine.spawner.spawns.len(), 1);
    }

    /// A refusal that lands after the user turned it off is just Off.
    #[test]
    fn a_refusal_while_stopping_is_only_off() {
        let mut machine = Machine::new(Fake::new(), 42);
        drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);
        drive(&mut machine, Ok(dims()), &[Request::Set(false)], true);
        machine.spawner.failure = Some("denied".to_string());
        let done = drive(&mut machine, Ok(dims()), &[], false);
        assert_eq!(done.fact, Fact::Off);
        assert_eq!(done.notes, vec![Note::Off]);
        assert!(machine.spawner.failure.is_none());
    }

    #[test]
    fn set_on_spawns_once_and_a_repeat_does_not() {
        let mut machine = Machine::new(Fake::new(), 42);
        let first = drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);
        assert_eq!(first.fact, Fact::On(dims()));
        assert_eq!(first.notes, vec![Note::On(dims())]);
        assert_eq!(machine.spawner.spawns.len(), 1);

        let second = drive(&mut machine, Ok(dims()), &[Request::Set(true)], true);
        assert_eq!(second.fact, Fact::On(dims()));
        assert!(second.notes.is_empty());
        assert_eq!(machine.spawner.spawns.len(), 1);
        assert_eq!(machine.spawner.kills, 0);
    }

    #[test]
    fn set_off_stays_on_until_the_child_exits() {
        let mut machine = Machine::new(Fake::new(), 42);
        drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);

        let stopping = drive(&mut machine, Ok(dims()), &[Request::Set(false)], true);
        assert_eq!(stopping.fact, Fact::On(dims()));
        assert!(stopping.notes.is_empty());
        assert_eq!(machine.spawner.kills, 1);

        let done = drive(&mut machine, Ok(dims()), &[], false);
        assert_eq!(done.fact, Fact::Off);
        assert_eq!(done.notes, vec![Note::Off]);
        assert_eq!(machine.spawner.spawns.len(), 1);
    }

    #[test]
    fn a_second_set_off_does_nothing() {
        let mut machine = Machine::new(Fake::new(), 42);
        let tick = drive(&mut machine, Ok(dims()), &[Request::Set(false)], false);
        assert_eq!(tick.fact, Fact::Off);
        assert!(tick.notes.is_empty());
        assert!(machine.spawner.spawns.is_empty());
        assert_eq!(machine.spawner.kills, 0);
    }

    #[test]
    fn flag_change_restarts_without_an_off_or_a_second_on() {
        let mut machine = Machine::new(Fake::new(), 42);
        drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);
        let display = AssertedFlags::new([Flag::Display]).unwrap();

        let stopping = drive(&mut machine, Ok(display), &[], true);
        assert_eq!(stopping.fact, Fact::On(display));
        assert!(stopping.notes.is_empty());
        assert_eq!(machine.spawner.kills, 1);
        assert_eq!(machine.spawner.spawns.len(), 1);

        let restarted = drive(&mut machine, Ok(display), &[], false);
        assert_eq!(restarted.fact, Fact::On(display));
        assert!(restarted.notes.is_empty());
        assert_eq!(
            machine.spawner.spawns[1],
            vec!["-d".to_string(), "-w".to_string(), "42".to_string()]
        );
    }

    #[test]
    fn off_in_the_tick_a_restart_spawns_waits_for_the_new_child() {
        let mut machine = Machine::new(Fake::new(), 42);
        drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);
        let display = AssertedFlags::new([Flag::Display]).unwrap();
        drive(&mut machine, Ok(display), &[], true);

        let off = drive(&mut machine, Ok(display), &[Request::Set(false)], false);
        assert_eq!(off.fact, Fact::On(display));
        assert!(off.notes.is_empty());
        assert_eq!(machine.spawner.spawns.len(), 2);

        let done = drive(&mut machine, Ok(display), &[], false);
        assert_eq!(done.fact, Fact::Off);
        assert_eq!(done.notes, vec![Note::Off]);

        let again = drive(&mut machine, Ok(display), &[Request::Set(true)], false);
        assert_eq!(again.fact, Fact::On(display));
        assert_eq!(again.notes, vec![Note::On(display)]);
        assert!(again.error.is_none());
        assert_eq!(machine.spawner.spawns.len(), 3);
    }

    #[test]
    fn a_flag_change_in_the_tick_a_restart_spawns_waits_for_the_new_child() {
        let mut machine = Machine::new(Fake::new(), 42);
        drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);
        let display = AssertedFlags::new([Flag::Display]).unwrap();
        let system = AssertedFlags::new([Flag::System]).unwrap();
        drive(&mut machine, Ok(display), &[], true);

        let changed = drive(&mut machine, Ok(system), &[], false);
        assert_eq!(changed.fact, Fact::On(system));
        assert!(changed.notes.is_empty());
        assert!(changed.error.is_none());

        let restarted = drive(&mut machine, Ok(system), &[], false);
        assert_eq!(restarted.fact, Fact::On(system));
        assert!(restarted.notes.is_empty());
        assert_eq!(
            machine.spawner.spawns[2],
            vec!["-s".to_string(), "-w".to_string(), "42".to_string()]
        );
    }

    #[test]
    fn a_child_that_exits_on_its_own_turns_caffeinate_off() {
        let mut machine = Machine::new(Fake::new(), 42);
        drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);
        let exited = drive(&mut machine, Ok(dims()), &[], false);
        assert_eq!(exited.fact, Fact::Off);
        assert_eq!(exited.notes, vec![Note::Exited]);
        assert_eq!(machine.spawner.kills, 0);
    }

    #[test]
    fn a_failed_spawn_stays_off() {
        let mut fake = Fake::new();
        fake.fail = true;
        let mut machine = Machine::new(fake, 42);
        let tick = drive(&mut machine, Ok(dims()), &[Request::Set(true)], false);
        assert_eq!(tick.fact, Fact::Off);
        assert_eq!(tick.notes, vec![Note::Failed]);
        assert_eq!(tick.error.as_deref(), Some("caffeinate missing"));
        assert!(machine.spawner.spawns.is_empty());
    }

    #[test]
    fn empty_flags_refuse_a_start() {
        let mut machine = Machine::new(Fake::new(), 42);
        let tick = drive(&mut machine, Err(Empty), &[Request::Set(true)], false);
        assert_eq!(tick.fact, Fact::Off);
        assert_eq!(tick.error.as_deref(), Some(Empty::REASON));
        assert!(tick.notes.is_empty());
        assert!(machine.spawner.spawns.is_empty());
    }

    #[test]
    fn toggle_tracks_set() {
        let mut machine = Machine::new(Fake::new(), 42);
        let on = drive(&mut machine, Ok(dims()), &[Request::Toggle], false);
        assert_eq!(on.fact, Fact::On(dims()));
        assert_eq!(on.notes, vec![Note::On(dims())]);

        let off = drive(&mut machine, Ok(dims()), &[Request::Toggle], true);
        assert_eq!(off.fact, Fact::On(dims()));
        assert!(off.notes.is_empty());
        let done = drive(&mut machine, Ok(dims()), &[], false);
        assert_eq!(done.notes, vec![Note::Off]);
        assert_eq!(done.fact, Fact::Off);
    }
}
