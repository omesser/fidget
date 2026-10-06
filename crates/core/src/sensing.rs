//! The Free sensing tier: frontmost application, how long it and a few earlier
//! fronts stayed, idle, and the civil clock — none of it costing a permission
//! prompt. Titles, screen content and the clipboard are absent by construction.

use std::time::{Duration, SystemTime};

/// What the operating system will report about the person using it without
/// asking them for anything.
pub trait ActivitySource {
    /// The frontmost application's name, or `None` when nothing is frontmost or
    /// the platform will not say.
    fn frontmost_application(&self) -> Option<String>;

    /// How long since the last keyboard, mouse or trackpad input anywhere on the
    /// machine. Zero while the user is typing.
    fn idle(&self) -> Duration;

    /// True when the machine is not showing anything: displays asleep, lid
    /// closed. A session Director does not wake for this (ADR-0008).
    fn displays_asleep(&self) -> bool;
}

/// Wall-clock time, behind a trait so that nothing else reads it directly and
/// tests never depend on what time it happens to be.
pub trait Clock {
    fn now(&self) -> SystemTime;
    /// Local civil clock, read once so the day cannot turn between the parts:
    /// weekday 0–6 from Sunday, hour 0–23, minute 0–59.
    fn local_time(&self) -> (u8, u8, u8);
}

/// The system clock. The only implementation that reads the real one.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn local_time(&self) -> (u8, u8, u8) {
        system_local_time()
    }
}

/// Local weekday, hour and minute on this machine. `std` has no civil clock,
/// so libc `localtime_r` is the OS answer; the non-unix build reports UTC
/// until a local clock exists there.
#[cfg(unix)]
fn system_local_time() -> (u8, u8, u8) {
    // SAFETY: `time` and `localtime_r` write through pointers to locals that
    // outlive the call, and both returns are checked before `tm` is read. Every
    // field of `libc::tm` is an integer or a raw pointer, so all-zero is valid.
    unsafe {
        let mut t: libc::time_t = 0;
        if libc::time(&mut t) == -1 {
            return (0, 0, 0);
        }
        let mut tm = std::mem::zeroed::<libc::tm>();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return (0, 0, 0);
        }
        (tm.tm_wday as u8, tm.tm_hour as u8, tm.tm_min as u8)
    }
}

#[cfg(not(unix))]
fn system_local_time() -> (u8, u8, u8) {
    let secs = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let minutes = (secs / 60) % (24 * 60);
    // 1970-01-01 was a Thursday.
    let weekday = (secs / 86_400 + 4) % 7;
    (weekday as u8, (minutes / 60) as u8, (minutes % 60) as u8)
}

/// One read of the Free tier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub frontmost_application: Option<String>,
    /// How long the frontmost application has been in front. Zero when
    /// nothing is.
    pub frontmost_for: Duration,
    /// Up to `BEFORE_LIMIT` applications that were in front earlier, newest
    /// first, each with how long it stayed there.
    pub before: Vec<(String, Duration)>,
    /// Whether the frontmost application differs from the previous read, so a
    /// caller need not remember the previous name. The first read of a run counts
    /// as a change, so a Director woken at startup has something to react to.
    pub switched: bool,
    pub idle: Duration,
    /// When this read was taken, from the `Clock`.
    pub at: SystemTime,
    /// Local weekday, 0–6 from Sunday.
    pub weekday: u8,
    /// Local civil hour, 0–23. Time of day for the Director, not UTC.
    pub hour: u8,
    /// Local civil minute, 0–59.
    pub minute: u8,
    /// True when the displays are asleep. Session wakes stay quiet then.
    pub displays_asleep: bool,
}

/// How many earlier front applications a read carries.
pub const BEFORE_LIMIT: usize = 3;

/// Reads the Free tier, and remembers which applications were in front and for
/// how long, so a caller need not keep that history itself.
#[derive(Default)]
pub struct FreeTier {
    previous: Option<String>,
    /// When `previous` came to the front. None before the first read.
    since: Option<SystemTime>,
    before: Vec<(String, Duration)>,
}

impl FreeTier {
    /// Read the source and the clock once. Remembers which applications were
    /// in front and for how long; idle and the civil clock need no memory.
    pub fn read(&mut self, source: &dyn ActivitySource, clock: &dyn Clock) -> Activity {
        let at = clock.now();
        let frontmost_application = source.frontmost_application();
        let switched = frontmost_application != self.previous;
        if switched {
            // A spell with nothing in front is not an application to report.
            if let (Some(left), Some(since)) = (self.previous.take(), self.since) {
                let stayed = at.duration_since(since).unwrap_or_default();
                self.before.insert(0, (left, stayed));
                self.before.truncate(BEFORE_LIMIT);
            }
            self.since = Some(at);
        }
        self.previous = frontmost_application.clone();
        let frontmost_for = match (&frontmost_application, self.since) {
            (Some(_), Some(since)) => at.duration_since(since).unwrap_or_default(),
            _ => Duration::ZERO,
        };

        let (weekday, hour, minute) = clock.local_time();
        Activity {
            frontmost_application,
            frontmost_for,
            before: self.before.clone(),
            switched,
            idle: source.idle(),
            at,
            weekday,
            hour,
            minute,
            displays_asleep: source.displays_asleep(),
        }
    }
}

/// Hand-written fake: it reports exactly what it was set to, so a test can
/// describe a desktop as a literal and change it between reads.
#[cfg(test)]
#[derive(Default)]
pub struct FakeActivitySource {
    pub frontmost_application: Option<String>,
    pub idle: Duration,
    pub displays_asleep: bool,
}

#[cfg(test)]
impl ActivitySource for FakeActivitySource {
    fn frontmost_application(&self) -> Option<String> {
        self.frontmost_application.clone()
    }

    fn idle(&self) -> Duration {
        self.idle
    }

    fn displays_asleep(&self) -> bool {
        self.displays_asleep
    }
}

/// Hand-written fake: the time is whatever the test says it is.
#[cfg(test)]
pub struct FakeClock {
    pub now: SystemTime,
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
}

#[cfg(test)]
impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        self.now
    }

    fn local_time(&self) -> (u8, u8, u8) {
        (self.weekday, self.hour, self.minute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn source(frontmost: &str) -> FakeActivitySource {
        FakeActivitySource {
            frontmost_application: Some(frontmost.to_string()),
            idle: Duration::ZERO,
            displays_asleep: false,
        }
    }

    fn stopped_clock() -> FakeClock {
        FakeClock {
            now: UNIX_EPOCH,
            weekday: 0,
            hour: 0,
            minute: 0,
        }
    }

    #[test]
    fn switching_application_signals_a_change_on_that_read_and_not_the_next() {
        let mut source = source("Terminal");
        let mut tier = FreeTier::default();

        let first = tier.read(&source, &stopped_clock());
        assert_eq!(first.frontmost_application.as_deref(), Some("Terminal"));
        assert!(first.switched, "the first sight of an application is news");

        let again = tier.read(&source, &stopped_clock());
        assert_eq!(again.frontmost_application.as_deref(), Some("Terminal"));
        assert!(!again.switched, "the same application is not a switch");

        source.frontmost_application = Some("Safari".to_string());
        let switched = tier.read(&source, &stopped_clock());
        assert_eq!(switched.frontmost_application.as_deref(), Some("Safari"));
        assert!(switched.switched, "Terminal to Safari is a switch");

        let settled = tier.read(&source, &stopped_clock());
        assert!(!settled.switched, "a switch is signalled once, not held");
    }

    /// The empty desktop, and the platform that will not say. Losing the
    /// frontmost application is as much a change as gaining one, and getting it
    /// back afterwards must not be silent.
    #[test]
    fn nothing_frontmost_is_a_change_from_something_and_back_again() {
        let mut source = source("Terminal");
        let mut tier = FreeTier::default();
        tier.read(&source, &stopped_clock());

        source.frontmost_application = None;
        let lost = tier.read(&source, &stopped_clock());
        assert_eq!(lost.frontmost_application, None);
        assert!(
            lost.switched,
            "losing the frontmost application is a change"
        );

        let still_nothing = tier.read(&source, &stopped_clock());
        assert!(!still_nothing.switched, "nothing, twice, is not a change");

        source.frontmost_application = Some("Terminal".to_string());
        let regained = tier.read(&source, &stopped_clock());
        assert!(regained.switched, "coming back is a change");
    }

    #[test]
    fn idle_grows_while_input_stops_and_returns_to_zero_on_the_next_input() {
        let mut source = source("Terminal");
        let mut tier = FreeTier::default();

        assert_eq!(tier.read(&source, &stopped_clock()).idle, Duration::ZERO);

        source.idle = Duration::from_secs(30);
        assert_eq!(
            tier.read(&source, &stopped_clock()).idle,
            Duration::from_secs(30)
        );

        source.idle = Duration::from_secs(15 * 60);
        assert_eq!(
            tier.read(&source, &stopped_clock()).idle,
            Duration::from_secs(900)
        );

        // The user touches the keyboard.
        source.idle = Duration::ZERO;
        assert_eq!(
            tier.read(&source, &stopped_clock()).idle,
            Duration::ZERO,
            "the next input ends the idle spell"
        );
    }

    /// A read is stamped from the `Clock` it was given, never from the real one,
    /// which is what lets a test about the time of day be a test about a
    /// literal.
    #[test]
    fn a_read_is_stamped_by_the_clock_it_was_given() {
        // 2023-11-14T22:13:20Z, chosen for being nothing like now.
        let clock = FakeClock {
            now: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            weekday: 2,
            hour: 22,
            minute: 13,
        };

        let read = FreeTier::default().read(&source("Terminal"), &clock);

        assert_eq!(read.at, UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        assert_eq!(
            (read.weekday, read.hour, read.minute),
            (2, 22, 13),
            "local, not derived from `at`"
        );
    }

    fn at_minute(minute: u64) -> FakeClock {
        FakeClock {
            now: UNIX_EPOCH + Duration::from_secs(minute * 60),
            weekday: 0,
            hour: 0,
            minute: 0,
        }
    }

    /// Each application that leaves the front is kept with how long it
    /// stayed, newest first, and only the last few are kept.
    #[test]
    fn earlier_front_applications_are_kept_newest_first_with_their_stay() {
        let mut source = source("Terminal");
        let mut tier = FreeTier::default();
        tier.read(&source, &at_minute(0));

        let settled = tier.read(&source, &at_minute(41));
        assert_eq!(settled.frontmost_for, Duration::from_secs(41 * 60));
        assert!(settled.before.is_empty(), "nothing has left the front yet");

        for (minute, next) in [(41, "Safari"), (44, "Mail"), (50, "Slack"), (51, "Notes")] {
            source.frontmost_application = Some(next.to_string());
            tier.read(&source, &at_minute(minute));
        }
        let read = tier.read(&source, &at_minute(53));

        assert_eq!(read.frontmost_for, Duration::from_secs(2 * 60));
        let minutes: Vec<(&str, u64)> = read
            .before
            .iter()
            .map(|(name, stayed)| (name.as_str(), stayed.as_secs() / 60))
            .collect();
        assert_eq!(
            minutes,
            [("Slack", 1), ("Mail", 6), ("Safari", 3)],
            "Terminal's 41 minutes fell off the end"
        );
    }

    #[test]
    fn a_spell_with_nothing_in_front_is_not_kept() {
        let mut source = source("Terminal");
        let mut tier = FreeTier::default();
        tier.read(&source, &at_minute(0));

        source.frontmost_application = None;
        let nothing = tier.read(&source, &at_minute(5));
        assert_eq!(nothing.frontmost_for, Duration::ZERO);

        source.frontmost_application = Some("Safari".to_string());
        let read = tier.read(&source, &at_minute(9));
        assert_eq!(
            read.before,
            [("Terminal".to_string(), Duration::from_secs(300))]
        );
    }

    #[test]
    fn displays_asleep_is_read_from_the_source() {
        let mut source = source("Terminal");
        source.displays_asleep = true;
        assert!(
            FreeTier::default()
                .read(&source, &stopped_clock())
                .displays_asleep
        );
    }
}
