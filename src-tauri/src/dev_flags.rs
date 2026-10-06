//! Live values for the development switches, so a toggle lands without a relaunch.
//!
//! A window cannot flip a switch that reads its own env var; one static per switch lets a read site load without a lock.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::harness;
use crate::model;
use crate::settings::Settings;

/// One boolean development switch and the variable that can own it.
pub struct Flag {
    var: &'static str,
    on: AtomicBool,
}

impl Flag {
    const fn new(var: &'static str) -> Self {
        Self {
            var,
            on: AtomicBool::new(false),
        }
    }

    /// The environment variable this switch answers to. The settings window
    /// names it in a frozen row's label.
    pub fn var(&self) -> &'static str {
        self.var
    }

    /// `Relaxed` is enough: a trace switch has nothing to synchronise with,
    /// and the read sites want the value, not an ordering against it.
    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// What the settings window shows, and what `seed` stores.
    pub fn in_force(&self, persisted: bool) -> bool {
        self.env_value().unwrap_or(persisted)
    }

    /// What the exported variable says, if it is exported.
    ///
    /// `model::env_switch` holds the vocabulary so a Development switch and the Director's switch answer to the same words.
    fn env_value(&self) -> Option<bool> {
        model::env_switch(self.var)
    }

    /// Load the switch from `persisted`, with an exported variable winning.
    fn seed(&self, persisted: bool) {
        self.on.store(self.in_force(persisted), Ordering::Relaxed);
    }
}

pub static TRACE_FRAMES: Flag = Flag::new("FIDGET_TRACE_FRAMES");
pub static TRACE_HITTEST: Flag = Flag::new("FIDGET_TRACE_HITTEST");
pub static TRACE_DIRECTOR: Flag = Flag::new("FIDGET_TRACE_DIRECTOR");
pub static TRACE_ENGINE: Flag = Flag::new("FIDGET_TRACE_ENGINE");
/// Windows overlay reinforce debug logging. Windows only.
#[cfg(windows)]
pub static DEBUG_REINFORCE: Flag = Flag::new("FIDGET_DEBUG_REINFORCE");
pub static DEBUG_IPC: Flag = Flag::new("FIDGET_DEBUG_IPC");
/// Capture exclusion setting. macOS and Windows support it via platform APIs;
/// Linux has no exclusion API (ADR-0024) but the setting and UI row are present.
pub static CAPTURABLE: Flag = Flag::new("FIDGET_CAPTURABLE");
/// Blank-AI mode. Named from `model` rather than spelled again here:
/// it is a Director variable, and the row that freezes on it names the same
/// string the Director's other knobs do.
pub static DIRECTOR_BLANK: Flag = Flag::new(model::BLANK);

/// Completer timeout, turn ceiling, and first ambient wait, as the variable or the file gives them.
///
/// Zero is unset: a blank or non-numeric field, and none of those zeros is a value worth telling apart from absent.
static TIMEOUT_SECS: AtomicU64 = AtomicU64::new(0);
static MAX_TOKENS: AtomicU32 = AtomicU32::new(0);
static WAKE_SECS: AtomicU64 = AtomicU64::new(0);
static AUTH_RETRY_SECS: AtomicU64 = AtomicU64::new(0);
static HARNESS_TURN_TIMEOUT_SECS: AtomicU64 = AtomicU64::new(0);

/// How hard the Completer is asked to think. A `Mutex<String>` because the
/// value is any string a host takes, not a number. Blank is unset, and
/// unset is omitted rather than stored as a default level.
static REASONING_EFFORT: Mutex<String> = Mutex::new(String::new());

/// Where the stdio MCP server is, as the variable or the file gives it.
///
/// A `Mutex` rather than an atomic because a path is not a number.
static MCP_BIN: Mutex<String> = Mutex::new(String::new());

/// The Model API timeout in force, in seconds.
pub fn director_timeout_secs() -> Option<u64> {
    let secs = TIMEOUT_SECS.load(Ordering::Relaxed);
    (secs > 0).then_some(secs)
}

/// The turn ceiling in force, in tokens.
pub fn director_max_tokens() -> Option<u32> {
    let cap = MAX_TOKENS.load(Ordering::Relaxed);
    (cap > 0).then_some(cap)
}

/// The first ambient wait in force, in seconds.
pub fn director_wake_secs() -> Option<u64> {
    let secs = WAKE_SECS.load(Ordering::Relaxed);
    (secs > 0).then_some(secs)
}

/// The Harness auth-retry interval in force, in seconds. Zero is unset for the
/// reason a zero timeout is: retrying with no wait at all would hammer a child
/// that has said it is not signed in.
pub fn harness_auth_retry_secs() -> Option<u64> {
    let secs = AUTH_RETRY_SECS.load(Ordering::Relaxed);
    (secs > 0).then_some(secs)
}

/// The Harness turn timeout in force, in seconds. Zero is unset for the
/// reason a zero Model API timeout is: a turn with no budget cannot finish.
pub fn harness_turn_timeout_secs() -> Option<u64> {
    let secs = HARNESS_TURN_TIMEOUT_SECS.load(Ordering::Relaxed);
    (secs > 0).then_some(secs)
}

/// The reasoning effort in force, if one is set.
pub fn director_reasoning_effort() -> Option<String> {
    let effort = REASONING_EFFORT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (!effort.is_empty()).then(|| effort.clone())
}

/// The stdio MCP server path in force, if one is set.
pub fn mcp_bin() -> Option<PathBuf> {
    let path = MCP_BIN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (!path.is_empty()).then(|| PathBuf::from(path.as_str()))
}

fn flag_vars() -> Vec<&'static str> {
    vec![
        TRACE_FRAMES.var(),
        TRACE_HITTEST.var(),
        TRACE_DIRECTOR.var(),
        TRACE_ENGINE.var(),
        #[cfg(windows)]
        DEBUG_REINFORCE.var(),
        CAPTURABLE.var(),
        DIRECTOR_BLANK.var(),
    ]
}

/// Every variable naming a switch, the Director's included, for the launch
/// check that each holds a value `model::env_switch` can read.
pub fn switch_vars() -> Vec<&'static str> {
    std::iter::once(model::ENABLED).chain(flag_vars()).collect()
}

/// Every variable a Development row answers to.
///
/// `with_env` clears these under the test env lock. The Director's switch is omitted: clearing it twice would undo a caller-set value.
#[cfg(test)]
pub(crate) fn test_vars() -> Vec<&'static str> {
    flag_vars()
        .into_iter()
        .chain([
            model::TIMEOUT_SECS,
            model::MAX_TOKENS,
            model::REASONING_EFFORT,
            model::WAKE_SECS,
            harness::AUTH_RETRY_SECS,
            harness::TURN_TIMEOUT_SECS,
            harness::MCP_BIN,
            harness::CWD,
        ])
        .collect()
}

/// Load every switch from `settings`, with an exported variable winning.
///
/// Called at startup and on each applied patch, so re-read the environment every time and keep precedence in one place.
pub fn seed(settings: &Settings) {
    TRACE_FRAMES.seed(settings.trace_frames);
    TRACE_HITTEST.seed(settings.trace_hittest);
    TRACE_DIRECTOR.seed(settings.trace_director);
    TRACE_ENGINE.seed(settings.trace_engine);
    #[cfg(windows)]
    DEBUG_REINFORCE.seed(false);
    CAPTURABLE.seed(settings.capturable);
    DIRECTOR_BLANK.seed(settings.director_blank);
    TIMEOUT_SECS.store(
        model::env_or_file(model::TIMEOUT_SECS, &settings.director_timeout_secs)
            .trim()
            .parse()
            .unwrap_or(0),
        Ordering::Relaxed,
    );
    MAX_TOKENS.store(
        model::env_or_file(model::MAX_TOKENS, &settings.director_max_tokens)
            .trim()
            .parse()
            .unwrap_or(0),
        Ordering::Relaxed,
    );
    WAKE_SECS.store(
        model::env_or_file(model::WAKE_SECS, &settings.director_wake_secs)
            .trim()
            .parse()
            .unwrap_or(0),
        Ordering::Relaxed,
    );
    AUTH_RETRY_SECS.store(
        model::env_or_file(harness::AUTH_RETRY_SECS, &settings.harness_auth_retry_secs)
            .trim()
            .parse()
            .unwrap_or(0),
        Ordering::Relaxed,
    );
    HARNESS_TURN_TIMEOUT_SECS.store(
        model::env_or_file(
            harness::TURN_TIMEOUT_SECS,
            &settings.harness_turn_timeout_secs,
        )
        .trim()
        .parse()
        .unwrap_or(0),
        Ordering::Relaxed,
    );
    *REASONING_EFFORT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        model::env_or_file(model::REASONING_EFFORT, &settings.director_reasoning_effort)
            .trim()
            .to_string();
    *MCP_BIN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        model::env_or_file(harness::MCP_BIN, &settings.mcp_bin)
            .trim()
            .to_string();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vocabulary `model::env_switch` holds, seen through a switch: the
    /// same words the Director's own variable answers to.
    #[test]
    fn an_exported_switch_reads_the_shared_vocabulary() {
        // `with_env` is the whole test binary's env lock; a second mutex would
        // not serialise against it, and concurrent setenv is undefined.
        model::tests::with_env(None, None, None, || {
            let persisted = Settings {
                trace_frames: true,
                ..Settings::default()
            };
            for (exported, on) in [
                (None, true),
                (Some("1"), true),
                (Some("on"), true),
                (Some("true"), true),
                (Some("0"), false),
                (Some("off"), false),
                (Some("no"), false),
                // An expansion that produced nothing is a mistake, not an
                // override, and a word no switch knows is not an off. Both
                // leave the file holding the switch.
                (Some(""), true),
                (Some("banana"), true),
            ] {
                match exported {
                    Some(value) => std::env::set_var(TRACE_FRAMES.var(), value),
                    None => std::env::remove_var(TRACE_FRAMES.var()),
                }
                seed(&persisted);
                assert_eq!(TRACE_FRAMES.is_on(), on, "exported {exported:?}");
            }
            std::env::remove_var(TRACE_FRAMES.var());
        });
    }

    #[test]
    fn seeding_takes_the_env_over_the_file() {
        model::tests::with_env(None, None, None, || {
            let off = Settings {
                trace_hittest: false,
                ..Settings::default()
            };
            std::env::set_var(TRACE_HITTEST.var(), "1");
            seed(&off);
            std::env::remove_var(TRACE_HITTEST.var());
            assert!(TRACE_HITTEST.is_on(), "the exported variable wins");

            seed(&off);
            assert!(!TRACE_HITTEST.is_on(), "the file wins with no variable set");
        });
    }

    #[test]
    fn a_patched_flag_moves_what_is_on_reports() {
        model::tests::with_env(None, None, None, || {
            seed(&Settings {
                trace_director: true,
                ..Settings::default()
            });
            assert!(TRACE_DIRECTOR.is_on());
            assert!(model::tracing(), "model::tracing reads the live flag");

            seed(&Settings::default());
            assert!(!TRACE_DIRECTOR.is_on());
        });
    }

    /// The row writes the switch a Director reads when it is built, and
    /// an exported variable outranks it like every other switch.
    #[test]
    fn a_patched_blank_switch_moves_what_the_director_reads() {
        model::tests::with_env(None, None, None, || {
            seed(&Settings {
                director_blank: true,
                ..Settings::default()
            });
            assert!(DIRECTOR_BLANK.is_on());
            assert!(model::blank(), "model::blank reads the live flag");

            std::env::set_var(model::BLANK, "0");
            seed(&Settings {
                director_blank: true,
                ..Settings::default()
            });
            std::env::remove_var(model::BLANK);
            assert!(!model::blank(), "the exported variable wins");

            seed(&Settings::default());
            assert!(!DIRECTOR_BLANK.is_on(), "off is the shipped answer");
        });
    }

    #[test]
    fn a_blank_number_is_unset() {
        model::tests::with_env(None, None, None, || {
            seed(&Settings {
                director_timeout_secs: String::new(),
                director_max_tokens: "not a number".to_string(),
                ..Settings::default()
            });
            assert_eq!(director_timeout_secs(), None);
            assert_eq!(director_max_tokens(), None);

            seed(&Settings {
                director_timeout_secs: "45".to_string(),
                director_max_tokens: "300".to_string(),
                ..Settings::default()
            });
            assert_eq!(director_timeout_secs(), Some(45));
            assert_eq!(director_max_tokens(), Some(300));
        });
    }

    /// The effort is a string, so blank is the only unset there is: no parse
    /// can reject it, and nothing validates it against a list of levels.
    #[test]
    fn a_blank_effort_is_unset_and_anything_else_is_kept() {
        model::tests::with_env(None, None, None, || {
            seed(&Settings::default());
            assert_eq!(director_reasoning_effort(), None);

            seed(&Settings {
                director_reasoning_effort: "  ".to_string(),
                ..Settings::default()
            });
            assert_eq!(director_reasoning_effort(), None, "whitespace is blank");

            for typed in ["high", "max", "banana"] {
                seed(&Settings {
                    director_reasoning_effort: format!(" {typed} "),
                    ..Settings::default()
                });
                assert_eq!(director_reasoning_effort(), Some(typed.to_string()));
            }
            seed(&Settings::default());
        });
    }

    /// One knob's `(env var, file setter, reader, file value, env value)`.
    /// Every non-boolean knob shares this precedence, so the loop below
    /// runs the same steps per row instead of repeating a test per knob.
    struct Precedence {
        var: &'static str,
        set_file: fn(&str) -> Settings,
        read: fn() -> Option<String>,
        file_value: &'static str,
        env_value: &'static str,
    }

    const PRECEDENCE: &[Precedence] = &[
        Precedence {
            var: harness::AUTH_RETRY_SECS,
            set_file: |v| Settings {
                harness_auth_retry_secs: v.to_string(),
                ..Settings::default()
            },
            read: || harness_auth_retry_secs().map(|secs| secs.to_string()),
            file_value: "5",
            env_value: "1",
        },
        Precedence {
            var: harness::TURN_TIMEOUT_SECS,
            set_file: |v| Settings {
                harness_turn_timeout_secs: v.to_string(),
                ..Settings::default()
            },
            read: || harness_turn_timeout_secs().map(|secs| secs.to_string()),
            file_value: "45",
            env_value: "90",
        },
        Precedence {
            var: harness::MCP_BIN,
            set_file: |v| Settings {
                mcp_bin: v.to_string(),
                ..Settings::default()
            },
            read: || mcp_bin().map(|path| path.display().to_string()),
            file_value: "/tmp/from-the-file",
            env_value: "/tmp/from-the-env",
        },
        Precedence {
            var: model::REASONING_EFFORT,
            set_file: |v| Settings {
                director_reasoning_effort: v.to_string(),
                ..Settings::default()
            },
            read: director_reasoning_effort,
            file_value: "medium",
            env_value: "xhigh",
        },
    ];

    #[test]
    fn an_exported_variable_outranks_the_file() {
        for row in PRECEDENCE {
            model::tests::with_env(None, None, None, || {
                seed(&Settings::default());
                assert_eq!((row.read)(), None, "blank is unset: {}", row.var);

                let file = (row.set_file)(row.file_value);
                seed(&file);
                assert_eq!(
                    (row.read)(),
                    Some(row.file_value.to_string()),
                    "the file wins with no variable set: {}",
                    row.var
                );

                std::env::set_var(row.var, row.env_value);
                seed(&file);
                std::env::remove_var(row.var);
                assert_eq!(
                    (row.read)(),
                    Some(row.env_value.to_string()),
                    "exported {} wins",
                    row.var
                );

                seed(&file);
                assert_eq!(
                    (row.read)(),
                    Some(row.file_value.to_string()),
                    "the file returns once the variable is gone: {}",
                    row.var
                );
            });
        }
    }
}
