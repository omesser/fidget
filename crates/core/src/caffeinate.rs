//! One macOS `caffeinate` child for this launch.
//!
//! `-w` is mandatory. A macOS child is reparented to `launchd` when fidget
//! dies, so a crash would leave the Mac awake without it.

/// Display, idle, disk, and system sleep, in `caffeinate(8)` letter order.
const DISPLAY: u8 = 0b0001;
const IDLE: u8 = 0b0010;
const DISK: u8 = 0b0100;
const SYSTEM: u8 = 0b1000;

/// The empty flag set cannot be saved or spawned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Empty;

impl Empty {
    pub const REASON: &'static str = "At least one caffeinate flag has to stay on.";
}

/// A non-empty subset of `-d -i -m -s`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertedFlags {
    mask: std::num::NonZeroU8,
}

impl AssertedFlags {
    pub fn try_from_bits(
        display: bool,
        idle: bool,
        disk: bool,
        system: bool,
    ) -> Result<Self, Empty> {
        let mut mask = 0u8;
        if display {
            mask |= DISPLAY;
        }
        if idle {
            mask |= IDLE;
        }
        if disk {
            mask |= DISK;
        }
        if system {
            mask |= SYSTEM;
        }
        std::num::NonZeroU8::new(mask)
            .map(|mask| Self { mask })
            .ok_or(Empty)
    }

    /// Letters in canonical order, with no leading dash. `dims`.
    pub fn letters(self) -> String {
        let mut out = String::new();
        for (bit, letter) in [(DISPLAY, 'd'), (IDLE, 'i'), (DISK, 'm'), (SYSTEM, 's')] {
            if self.mask.get() & bit != 0 {
                out.push(letter);
            }
        }
        out
    }

    /// The flag cluster Settings shows. `-dims`.
    pub fn preview(self) -> String {
        format!("-{}", self.letters())
    }

    pub fn argv(self, pid: u32) -> Argv {
        Argv { flags: self, pid }
    }
}

/// Closed argv. There is no utility slot, because `caffeinate` ignores `-w`
/// when a utility is also present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Argv {
    flags: AssertedFlags,
    pid: u32,
}

impl Argv {
    /// One token per letter, then `-w`, then the pid.
    pub fn args(self) -> Vec<String> {
        let mut args: Vec<String> = self
            .flags
            .letters()
            .chars()
            .map(|letter| format!("-{letter}"))
            .collect();
        args.push("-w".to_string());
        args.push(self.pid.to_string());
        args
    }
}

/// Level of the hold. Absence of the feature is `None` at the prompt, not a
/// third variant here.
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
            Fact::On(flags) => format!("caffeinate: on ({})", flags.preview()),
        }
    }
}

/// A chat edge. Not a tool result and not the prompt line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    On(AssertedFlags),
    Off,
    /// The child left while the hold was still wanted and this owner had not
    /// started a kill.
    Exited,
    Failed(String),
}

impl Note {
    pub fn text(&self) -> String {
        match self {
            Note::On(flags) => format!(
                "☕ Caffeinate on: keeping your Mac awake ({})",
                flags.preview()
            ),
            Note::Off => "Caffeinate off.".to_string(),
            Note::Exited => "Caffeinate off. It exited on its own.".to_string(),
            Note::Failed(_) => "Caffeinate did not start.".to_string(),
        }
    }
}

/// Cup sends [`Request::Toggle`]. The tool sends [`Request::Set`]. Quit sends
/// [`Request::Release`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Toggle,
    Set(bool),
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    Absent,
    Alive,
    Exited,
}

/// Process boundary. `spawn` fails when a child is already held, so a second
/// start cannot hide the first.
pub trait Spawner {
    fn spawn(&mut self, argv: &Argv) -> Result<(), String>;
    fn kill(&mut self);
    fn poll(&mut self) -> Poll;
}

/// One `caffeinate` tool answer. `error` is what the caller reports; `fact`
/// is the hold after the call, including a failed start.
pub struct ToolAnswer {
    pub fact: Fact,
    pub error: Option<String>,
}

impl ToolAnswer {
    pub fn json(self) -> Result<serde_json::Value, String> {
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(match self.fact {
            Fact::Off => serde_json::json!({ "on": false }),
            Fact::On(flags) => serde_json::json!({ "on": true, "flags": flags.preview() }),
        })
    }
}

/// The frame loop's hold, as `dispatch` sees it. `None` reads.
pub trait CaffeinateHandle {
    fn call(&mut self, on: Option<bool>) -> ToolAnswer;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tick {
    pub fact: Fact,
    pub notes: Vec<Note>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Intent {
    Off,
    On,
}

/// What to do once the killed child is actually gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Again {
    /// User turned the hold off, or quit. `note` is false for quit.
    Off { flags: AssertedFlags, note: bool },
    /// Flag change. Spawn this set when the previous child has exited.
    Restart(AssertedFlags),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Running { flags: AssertedFlags },
    Stopping { again: Again },
}

/// The only mutator of the hold.
pub struct Machine<S: Spawner> {
    spawner: S,
    intent: Intent,
    phase: Phase,
}

impl<S: Spawner> Machine<S> {
    pub fn new(spawner: S) -> Self {
        Self {
            spawner,
            intent: Intent::Off,
            phase: Phase::Idle,
        }
    }

    /// Apply requests, then reconcile the one child to the resulting intent.
    pub fn tick(
        &mut self,
        flags: Result<AssertedFlags, Empty>,
        pid: u32,
        requests: &[Request],
    ) -> Tick {
        let poll = self.spawner.poll();
        let mut notes = Vec::new();
        let mut error = None;

        let resumed = self.finish_stop(poll, &mut notes, &mut error, pid, true);

        let was = self.intent;
        let mut quiet = false;
        for request in requests {
            match request {
                Request::Release => {
                    self.intent = Intent::Off;
                    quiet = true;
                }
                Request::Set(true) => match flags {
                    Ok(_) => self.intent = Intent::On,
                    Err(_) => {
                        self.intent = Intent::Off;
                        error = Some(Empty::REASON.to_string());
                    }
                },
                Request::Set(false) => self.intent = Intent::Off,
                Request::Toggle => match self.intent {
                    Intent::On => self.intent = Intent::Off,
                    Intent::Off => match flags {
                        Ok(_) => self.intent = Intent::On,
                        Err(_) => error = Some(Empty::REASON.to_string()),
                    },
                },
            }
        }
        if self.intent == Intent::On {
            quiet = false;
        }
        let rose = was == Intent::Off && self.intent == Intent::On;
        let asked_on = requests
            .iter()
            .any(|request| matches!(request, Request::Set(true) | Request::Toggle));

        if !resumed {
            self.classify_dead(poll, flags, quiet, asked_on, &mut notes);
        }
        self.reconcile(flags, pid, rose, quiet, &mut notes, &mut error);
        self.finish_stop(poll, &mut notes, &mut error, pid, false);

        Tick {
            fact: self.fact(),
            notes,
            error,
        }
    }

    fn fact(&self) -> Fact {
        match self.phase {
            Phase::Idle => Fact::Off,
            Phase::Running { flags } => Fact::On(flags),
            Phase::Stopping {
                again: Again::Off { flags, .. } | Again::Restart(flags),
            } => Fact::On(flags),
        }
    }

    /// Returns whether this call spawned the next child. That poll still
    /// describes the child that just exited, so the caller must not treat it
    /// as the new child's death.
    fn finish_stop(
        &mut self,
        poll: Poll,
        notes: &mut Vec<Note>,
        error: &mut Option<String>,
        pid: u32,
        retry_kill: bool,
    ) -> bool {
        let Phase::Stopping { again } = self.phase else {
            return false;
        };
        if !matches!(poll, Poll::Exited | Poll::Absent) {
            if retry_kill {
                self.spawner.kill();
            }
            return false;
        }
        self.phase = Phase::Idle;
        match again {
            Again::Off { note, .. } => {
                self.intent = Intent::Off;
                if note {
                    notes.push(Note::Off);
                }
                false
            }
            Again::Restart(flags) => {
                self.spawn_running(flags, pid, false, notes, error);
                true
            }
        }
    }

    fn classify_dead(
        &mut self,
        poll: Poll,
        flags: Result<AssertedFlags, Empty>,
        quiet: bool,
        asked_on: bool,
        notes: &mut Vec<Note>,
    ) {
        let Phase::Running { flags: spawned } = self.phase else {
            return;
        };
        if !matches!(poll, Poll::Exited | Poll::Absent) {
            return;
        }
        self.phase = Phase::Idle;
        if self.intent == Intent::Off {
            if !quiet {
                notes.push(Note::Off);
            }
            return;
        }
        let changed = matches!(flags, Ok(next) if next != spawned);
        if changed || asked_on {
            return;
        }
        notes.push(Note::Exited);
        self.intent = Intent::Off;
    }

    fn reconcile(
        &mut self,
        flags: Result<AssertedFlags, Empty>,
        pid: u32,
        rose: bool,
        quiet: bool,
        notes: &mut Vec<Note>,
        error: &mut Option<String>,
    ) {
        if self.intent == Intent::On {
            if let Phase::Stopping {
                again: Again::Off { .. },
            } = self.phase
            {
                if let Ok(flags) = flags {
                    self.phase = Phase::Stopping {
                        again: Again::Restart(flags),
                    };
                }
            }
        }

        match (self.intent, self.phase) {
            (Intent::On, Phase::Idle) => match flags {
                Ok(flags) => self.spawn_running(flags, pid, rose, notes, error),
                Err(_) => {
                    self.intent = Intent::Off;
                    *error = Some(Empty::REASON.to_string());
                }
            },
            (Intent::Off, Phase::Running { flags }) => {
                self.spawner.kill();
                self.phase = Phase::Stopping {
                    again: Again::Off {
                        flags,
                        note: !quiet,
                    },
                };
            }
            (Intent::On, Phase::Running { flags: spawned }) => {
                if let Ok(next) = flags {
                    if next != spawned {
                        self.spawner.kill();
                        self.phase = Phase::Stopping {
                            again: Again::Restart(next),
                        };
                    }
                }
            }
            (
                Intent::Off,
                Phase::Stopping {
                    again: Again::Restart(_),
                },
            ) => {
                let flags = match self.phase {
                    Phase::Stopping {
                        again: Again::Restart(flags),
                    } => flags,
                    _ => return,
                };
                self.phase = Phase::Stopping {
                    again: Again::Off {
                        flags,
                        note: !quiet,
                    },
                };
            }
            _ => {}
        }
    }

    fn spawn_running(
        &mut self,
        flags: AssertedFlags,
        pid: u32,
        announce: bool,
        notes: &mut Vec<Note>,
        error: &mut Option<String>,
    ) {
        let argv = flags.argv(pid);
        match self.spawner.spawn(&argv) {
            Ok(()) => {
                self.phase = Phase::Running { flags };
                self.intent = Intent::On;
                if announce {
                    notes.push(Note::On(flags));
                }
            }
            Err(reason) => {
                self.phase = Phase::Idle;
                self.intent = Intent::Off;
                notes.push(Note::Failed(reason.clone()));
                *error = Some(reason);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        held: bool,
        next: Option<Poll>,
        spawns: Vec<Vec<String>>,
        kills: usize,
        fail: bool,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                held: false,
                next: None,
                spawns: Vec::new(),
                kills: 0,
                fail: false,
            }
        }
    }

    impl Spawner for Fake {
        fn spawn(&mut self, argv: &Argv) -> Result<(), String> {
            if self.held {
                return Err("already holding a child".to_string());
            }
            if self.fail {
                return Err("caffeinate missing".to_string());
            }
            self.held = true;
            self.spawns.push(argv.args());
            Ok(())
        }

        fn kill(&mut self) {
            if self.held {
                self.kills += 1;
            }
        }

        fn poll(&mut self) -> Poll {
            let poll =
                self.next
                    .take()
                    .unwrap_or(if self.held { Poll::Alive } else { Poll::Absent });
            if matches!(poll, Poll::Exited | Poll::Absent) {
                self.held = false;
            }
            poll
        }
    }

    fn dims() -> AssertedFlags {
        AssertedFlags::try_from_bits(true, true, true, true).unwrap()
    }

    fn drive(
        machine: &mut Machine<Fake>,
        flags: Result<AssertedFlags, Empty>,
        requests: &[Request],
        poll: Poll,
    ) -> Tick {
        machine.spawner.next = Some(poll);
        let pid = 42;
        machine.tick(flags, pid, requests)
    }

    #[test]
    fn empty_flags_do_not_construct() {
        let error = AssertedFlags::try_from_bits(false, false, false, false).unwrap_err();
        assert_eq!(
            Empty::REASON,
            "At least one caffeinate flag has to stay on."
        );
        assert_eq!(error, Empty);
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
            AssertedFlags::try_from_bits(true, false, false, true)
                .unwrap()
                .preview(),
            "-ds"
        );
    }

    #[test]
    fn prompt_and_note_text_are_the_issue_sentences() {
        assert_eq!(Fact::Off.prompt_line(), "caffeinate: off");
        assert_eq!(Fact::On(dims()).prompt_line(), "caffeinate: on (-dims)");
        assert_eq!(
            Note::On(dims()).text(),
            "☕ Caffeinate on: keeping your Mac awake (-dims)"
        );
        assert_eq!(Note::Off.text(), "Caffeinate off.");
        assert_eq!(Note::Exited.text(), "Caffeinate off. It exited on its own.");
        assert_eq!(
            Note::Failed("x".to_string()).text(),
            "Caffeinate did not start."
        );
    }

    #[test]
    fn set_on_spawns_once_and_a_repeat_does_not() {
        let mut machine = Machine::new(Fake::new());
        let first = drive(
            &mut machine,
            Ok(dims()),
            &[Request::Set(true)],
            Poll::Absent,
        );
        assert_eq!(first.fact, Fact::On(dims()));
        assert_eq!(first.notes, vec![Note::On(dims())]);
        assert_eq!(machine.spawner.spawns.len(), 1);

        let second = drive(&mut machine, Ok(dims()), &[Request::Set(true)], Poll::Alive);
        assert_eq!(second.fact, Fact::On(dims()));
        assert!(second.notes.is_empty());
        assert_eq!(machine.spawner.spawns.len(), 1);
        assert_eq!(machine.spawner.kills, 0);
    }

    #[test]
    fn set_off_stays_on_until_the_child_exits() {
        let mut machine = Machine::new(Fake::new());
        drive(
            &mut machine,
            Ok(dims()),
            &[Request::Set(true)],
            Poll::Absent,
        );

        let stopping = drive(
            &mut machine,
            Ok(dims()),
            &[Request::Set(false)],
            Poll::Alive,
        );
        assert_eq!(stopping.fact, Fact::On(dims()));
        assert!(stopping.notes.is_empty());
        assert_eq!(machine.spawner.kills, 1);

        let done = drive(&mut machine, Ok(dims()), &[], Poll::Exited);
        assert_eq!(done.fact, Fact::Off);
        assert_eq!(done.notes, vec![Note::Off]);
        assert_eq!(machine.spawner.spawns.len(), 1);
    }

    #[test]
    fn a_second_set_off_does_nothing() {
        let mut machine = Machine::new(Fake::new());
        let tick = drive(
            &mut machine,
            Ok(dims()),
            &[Request::Set(false)],
            Poll::Absent,
        );
        assert_eq!(tick.fact, Fact::Off);
        assert!(tick.notes.is_empty());
        assert!(machine.spawner.spawns.is_empty());
        assert_eq!(machine.spawner.kills, 0);
    }

    #[test]
    fn flag_change_restarts_without_an_off_or_a_second_on() {
        let mut machine = Machine::new(Fake::new());
        drive(
            &mut machine,
            Ok(dims()),
            &[Request::Set(true)],
            Poll::Absent,
        );
        let display = AssertedFlags::try_from_bits(true, false, false, false).unwrap();

        let stopping = drive(&mut machine, Ok(display), &[], Poll::Alive);
        assert_eq!(stopping.fact, Fact::On(display));
        assert!(stopping.notes.is_empty());
        assert_eq!(machine.spawner.kills, 1);
        assert_eq!(machine.spawner.spawns.len(), 1);

        let restarted = drive(&mut machine, Ok(display), &[], Poll::Exited);
        assert_eq!(restarted.fact, Fact::On(display));
        assert!(restarted.notes.is_empty());
        assert_eq!(
            machine.spawner.spawns[1],
            vec!["-d".to_string(), "-w".to_string(), "42".to_string()]
        );
    }

    #[test]
    fn a_child_that_exits_on_its_own_turns_the_hold_off() {
        let mut machine = Machine::new(Fake::new());
        drive(
            &mut machine,
            Ok(dims()),
            &[Request::Set(true)],
            Poll::Absent,
        );
        let exited = drive(&mut machine, Ok(dims()), &[], Poll::Exited);
        assert_eq!(exited.fact, Fact::Off);
        assert_eq!(exited.notes, vec![Note::Exited]);
        assert_eq!(machine.spawner.kills, 0);
    }

    #[test]
    fn a_failed_spawn_stays_off() {
        let mut fake = Fake::new();
        fake.fail = true;
        let mut machine = Machine::new(fake);
        let tick = drive(
            &mut machine,
            Ok(dims()),
            &[Request::Set(true)],
            Poll::Absent,
        );
        assert_eq!(tick.fact, Fact::Off);
        assert_eq!(
            tick.notes,
            vec![Note::Failed("caffeinate missing".to_string())]
        );
        assert_eq!(tick.error.as_deref(), Some("caffeinate missing"));
        assert!(machine.spawner.spawns.is_empty());
    }

    #[test]
    fn release_kills_and_notes_nothing() {
        let mut machine = Machine::new(Fake::new());
        drive(
            &mut machine,
            Ok(dims()),
            &[Request::Set(true)],
            Poll::Absent,
        );
        let stopping = drive(&mut machine, Ok(dims()), &[Request::Release], Poll::Alive);
        assert_eq!(stopping.fact, Fact::On(dims()));
        assert!(stopping.notes.is_empty());

        let done = drive(&mut machine, Ok(dims()), &[], Poll::Exited);
        assert_eq!(done.fact, Fact::Off);
        assert!(done.notes.is_empty());

        let again = drive(&mut machine, Ok(dims()), &[Request::Release], Poll::Absent);
        assert_eq!(again.fact, Fact::Off);
        assert!(again.notes.is_empty());
        assert_eq!(machine.spawner.spawns.len(), 1);
    }

    #[test]
    fn empty_flags_refuse_a_start() {
        let mut machine = Machine::new(Fake::new());
        let tick = drive(
            &mut machine,
            Err(Empty),
            &[Request::Set(true)],
            Poll::Absent,
        );
        assert_eq!(tick.fact, Fact::Off);
        assert_eq!(tick.error.as_deref(), Some(Empty::REASON));
        assert!(tick.notes.is_empty());
        assert!(machine.spawner.spawns.is_empty());
    }

    #[test]
    fn toggle_tracks_set() {
        let mut machine = Machine::new(Fake::new());
        let on = drive(&mut machine, Ok(dims()), &[Request::Toggle], Poll::Absent);
        assert_eq!(on.fact, Fact::On(dims()));
        assert_eq!(on.notes, vec![Note::On(dims())]);

        let off = drive(&mut machine, Ok(dims()), &[Request::Toggle], Poll::Alive);
        assert_eq!(off.fact, Fact::On(dims()));
        assert!(off.notes.is_empty());
        let done = drive(&mut machine, Ok(dims()), &[], Poll::Exited);
        assert_eq!(done.notes, vec![Note::Off]);
        assert_eq!(done.fact, Fact::Off);
    }

    #[test]
    fn spawn_refuses_a_second_child() {
        let mut fake = Fake::new();
        fake.spawn(&dims().argv(1)).unwrap();
        let error = fake.spawn(&dims().argv(1)).unwrap_err();
        assert_eq!(error, "already holding a child");
    }
}
