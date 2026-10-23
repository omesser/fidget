//! The inhibitors this process holds on Linux.
//!
//! Idle and System are one `org.freedesktop.login1` inhibitor. Logind keeps it
//! while any copy of the fd it returns is open, and the kernel closes every fd
//! when the process ends. Display is an `org.freedesktop.ScreenSaver` cookie,
//! which the screen saver drops when this connection closes. So a crash,
//! `kill -9` or Quit's `process::exit` (which skips `Drop`) leaves nothing
//! behind, and this side needs no `HELD_PID`.

use std::os::fd::OwnedFd;
use std::sync::OnceLock;
use std::time::Duration;

use fidget_core::caffeinate::{Argv, AssertedFlags, Flag, Spawner};
use zbus::blocking::{connection, Connection};
use zbus::zvariant::OwnedObjectPath;

/// Who and why, as `systemd-inhibit --list` prints them.
pub const WHO: &str = "Fidget";
pub const WHY: &str = "Caffeinate";

/// Why the cup and the menu row are disabled when nothing can hold.
pub const UNAVAILABLE: &str =
    "Caffeinate needs systemd-logind, elogind or a desktop screen saver service, and none answered.";

const LOGIN1: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const SCREEN_SAVER: &str = "org.freedesktop.ScreenSaver";
const SCREEN_SAVER_PATH: &str = "/org/freedesktop/ScreenSaver";
const PORTAL: &str = "org.freedesktop.portal.Desktop";
/// `org.freedesktop.portal.Inhibit` flag for "idle", which covers blanking.
const PORTAL_IDLE: u32 = 8;

/// A bus that does not answer must not stall the frame loop for long.
const TIMEOUT: Duration = Duration::from_secs(2);

/// What a flag set asks Linux to block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The logind `what`: Idle is `idle`, System is `sleep`.
    pub logind: Option<&'static str>,
    /// Display is a screen saver inhibit.
    pub display: bool,
}

impl Plan {
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

    /// The Command row in Settings.
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

/// The disabled reason, once the startup probe found neither logind nor a
/// screen saver. `None` while the probe runs or when either answered.
pub fn unavailable() -> Option<&'static str> {
    PROBE.get().copied().flatten()
}

static PROBE: OnceLock<Option<&'static str>> = OnceLock::new();

/// Off the frame loop, because a bus that is slow to answer would stall it.
fn start_probe() {
    if PROBE.get().is_some() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("caffeinate-probe".to_string())
        .spawn(|| {
            let system = reachable(Connection::system, &[LOGIN1]);
            let session = reachable(Connection::session, &[SCREEN_SAVER, PORTAL]);
            let _ = PROBE.set((!system && !session).then_some(UNAVAILABLE));
        });
}

/// Whether the bus has any of `names`, running or activatable.
fn reachable(bus: fn() -> zbus::Result<Connection>, names: &[&str]) -> bool {
    let Ok(conn) = bus() else {
        return false;
    };
    let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return false;
    };
    let activatable: Vec<String> = proxy
        .list_activatable_names()
        .map(|names| names.into_iter().map(|name| name.to_string()).collect())
        .unwrap_or_default();
    names.iter().any(|name| {
        activatable.iter().any(|known| known == name)
            || zbus::names::BusName::try_from(*name)
                .ok()
                .and_then(|bus_name| proxy.name_has_owner(bus_name).ok())
                .unwrap_or(false)
    })
}

fn with_timeout(builder: zbus::Result<connection::Builder<'static>>) -> zbus::Result<Connection> {
    builder?.method_timeout(TIMEOUT).build()
}

/// The fd logind returns. zvariant dups it with `F_DUPFD_CLOEXEC`, so a
/// Harness spawned later does not inherit it. The reply's own copy lacks the
/// flag until the reply drops, a window of one function call.
fn inhibit_logind(what: &str) -> zbus::Result<OwnedFd> {
    let conn = with_timeout(connection::Builder::system())?;
    let reply = conn.call_method(
        Some(LOGIN1),
        LOGIN1_PATH,
        Some("org.freedesktop.login1.Manager"),
        "Inhibit",
        &(what, WHO, WHY, "block"),
    )?;
    let fd: zbus::zvariant::OwnedFd = reply.body().deserialize()?;
    Ok(fd.into())
}

/// The display hold and the session connection it lives on.
enum Screen {
    Saver {
        conn: Connection,
        cookie: u32,
    },
    Portal {
        conn: Connection,
        request: OwnedObjectPath,
    },
}

impl Screen {
    /// The screen saver first, which GNOME and KDE honor on X11 and Wayland,
    /// then the portal where no screen saver service runs.
    fn inhibit() -> zbus::Result<Self> {
        let conn = with_timeout(connection::Builder::session())?;
        let saver = conn.call_method(
            Some(SCREEN_SAVER),
            SCREEN_SAVER_PATH,
            Some(SCREEN_SAVER),
            "Inhibit",
            &(WHO, WHY),
        );
        match saver {
            Ok(reply) => {
                let cookie: u32 = reply.body().deserialize()?;
                Ok(Screen::Saver { conn, cookie })
            }
            Err(saver_error) => {
                let options: std::collections::HashMap<&str, zbus::zvariant::Value> =
                    [("reason", zbus::zvariant::Value::from(WHY))].into();
                let reply = conn
                    .call_method(
                        Some(PORTAL),
                        "/org/freedesktop/portal/desktop",
                        Some("org.freedesktop.portal.Inhibit"),
                        "Inhibit",
                        &("", PORTAL_IDLE, options),
                    )
                    .map_err(|portal_error| {
                        zbus::Error::Failure(format!(
                            "screen saver: {saver_error}; portal: {portal_error}"
                        ))
                    })?;
                let request: OwnedObjectPath = reply.body().deserialize()?;
                Ok(Screen::Portal { conn, request })
            }
        }
    }

    /// Dropping the connection would release it too; asking first releases
    /// it now rather than when the bus notices.
    fn release(self) {
        match self {
            Screen::Saver { conn, cookie } => {
                let _ = conn.call_method(
                    Some(SCREEN_SAVER),
                    SCREEN_SAVER_PATH,
                    Some(SCREEN_SAVER),
                    "UnInhibit",
                    &(cookie,),
                );
            }
            Screen::Portal { conn, request } => {
                let _ = conn.call_method(
                    Some(PORTAL),
                    &request,
                    Some("org.freedesktop.portal.Request"),
                    "Close",
                    &(),
                );
            }
        }
    }
}

struct Held {
    logind: Option<OwnedFd>,
    screen: Option<Screen>,
}

impl Held {
    fn release(self) {
        drop(self.logind);
        if let Some(screen) = self.screen {
            screen.release();
        }
    }
}

pub struct OsSpawner {
    held: Option<Held>,
}

impl OsSpawner {
    pub fn new() -> Self {
        start_probe();
        Self { held: None }
    }
}

impl Drop for OsSpawner {
    fn drop(&mut self) {
        self.kill();
    }
}

impl Spawner for OsSpawner {
    /// Holds what it can. One missing service is logged, not fatal; nothing
    /// held at all is the error the tool and the Chat note report.
    fn spawn(&mut self, argv: &Argv) -> Result<(), String> {
        if self.held.is_some() {
            return Err("an inhibitor is already held".to_string());
        }
        let plan = Plan::of(argv.flags());
        if plan.logind.is_none() && !plan.display {
            return Err("no flag maps to a Linux inhibitor".to_string());
        }
        let mut errors = Vec::new();
        let logind = plan.logind.and_then(|what| {
            inhibit_logind(what)
                .map_err(|error| errors.push(format!("logind: {error}")))
                .ok()
        });
        let screen = plan
            .display
            .then(|| Screen::inhibit().map_err(|error| errors.push(error.to_string())))
            .and_then(Result::ok);
        if logind.is_none() && screen.is_none() {
            return Err(format!("No inhibitor could be held: {}", errors.join("; ")));
        }
        if !errors.is_empty() {
            eprintln!("caffeinate: held in part: {}", errors.join("; "));
        }
        self.held = Some(Held { logind, screen });
        Ok(())
    }

    /// Releases at once, so the next tick reads it as gone.
    fn kill(&mut self) {
        if let Some(held) = self.held.take() {
            held.release();
        }
    }

    fn alive(&mut self) -> bool {
        self.held.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(set: &[Flag]) -> AssertedFlags {
        AssertedFlags::new(set.iter().copied()).expect("a flag")
    }

    #[test]
    fn idle_and_system_are_one_logind_inhibitor_and_display_the_screen_saver() {
        assert_eq!(
            Plan::of(AssertedFlags::all()),
            Plan {
                logind: Some("idle:sleep"),
                display: true
            }
        );
        assert_eq!(Plan::of(flags(&[Flag::Idle])).logind, Some("idle"));
        assert_eq!(Plan::of(flags(&[Flag::System])).logind, Some("sleep"));
        assert_eq!(
            Plan::of(flags(&[Flag::Display])),
            Plan {
                logind: None,
                display: true
            }
        );
        assert_eq!(
            Plan::of(flags(&[Flag::Disk])),
            Plan {
                logind: None,
                display: false
            }
        );
    }

    #[test]
    fn the_command_row_names_each_inhibitor() {
        assert_eq!(
            Plan::of(AssertedFlags::all()).describe(),
            "logind inhibit idle:sleep + screen saver inhibit"
        );
        assert_eq!(
            Plan::of(flags(&[Flag::System])).describe(),
            "logind inhibit sleep"
        );
        assert_eq!(
            Plan::of(flags(&[Flag::Display])).describe(),
            "screen saver inhibit"
        );
    }

    #[test]
    fn disk_alone_holds_nothing() {
        let mut spawner = OsSpawner { held: None };
        let err = spawner
            .spawn(&flags(&[Flag::Disk]).argv(std::process::id()))
            .expect_err("no inhibitor");
        assert!(!err.is_empty());
        assert!(!spawner.alive());
    }

    /// Who holds a logind inhibitor now.
    fn logind_holders(conn: &Connection) -> Vec<String> {
        let reply = conn
            .call_method(
                Some(LOGIN1),
                LOGIN1_PATH,
                Some("org.freedesktop.login1.Manager"),
                "ListInhibitors",
                &(),
            )
            .expect("ListInhibitors");
        let list: Vec<(String, String, String, String, u32, u32)> =
            reply.body().deserialize().expect("inhibitor list");
        list.into_iter().map(|(_, who, ..)| who).collect()
    }

    /// Against a real logind, so it is opt-in: `cargo test -p fidget
    /// caffeinate_linux -- --ignored`, as a user logind lets inhibit.
    /// Releasing is asynchronous on logind's side, so it waits for the
    /// `BlockInhibited` change rather than polling.
    #[test]
    #[ignore = "needs a system bus with logind"]
    fn a_real_logind_lists_the_hold_until_it_is_killed() {
        let conn = Connection::system().expect("system bus");
        let changes = zbus::blocking::MessageIterator::for_match_rule(
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .path(LOGIN1_PATH)
                .expect("path")
                .interface("org.freedesktop.DBus.Properties")
                .expect("interface")
                .member("PropertiesChanged")
                .expect("member")
                .build(),
            &conn,
            None,
        )
        .expect("match rule");

        let mut spawner = OsSpawner::new();
        spawner
            .spawn(&AssertedFlags::all().argv(std::process::id()))
            .expect("held");
        assert!(logind_holders(&conn).iter().any(|who| who == WHO));
        print_inhibitors("held");

        spawner.kill();
        assert!(!spawner.alive());
        changes
            .into_iter()
            .find(|_| !logind_holders(&conn).iter().any(|who| who == WHO))
            .expect("released")
            .expect("a PropertiesChanged signal");
        print_inhibitors("killed");
    }

    /// What `systemd-inhibit --list` says, for `--nocapture` runs.
    fn print_inhibitors(when: &str) {
        if let Ok(out) = std::process::Command::new("systemd-inhibit")
            .arg("--list")
            .output()
        {
            println!("{when}:\n{}", String::from_utf8_lossy(&out.stdout));
        }
    }
}
