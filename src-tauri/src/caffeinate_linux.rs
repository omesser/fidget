//! The inhibitors this process takes on Linux.
//!
//! Idle and System are one `org.freedesktop.login1` inhibitor. Logind keeps it
//! while any copy of the fd it returns is open, and the kernel closes every fd
//! when the process ends. Display is an `org.freedesktop.ScreenSaver` cookie,
//! which the screen saver drops when this connection closes. So a crash,
//! `kill -9` or Quit's `process::exit` (which skips `Drop`) leaves nothing
//! behind, and this side needs no `HELD_PID`.
//!
//! The bus calls block for up to [`TIMEOUT`] each, so a worker thread makes
//! them. `spawn` and `kill` only send it an order, and `alive` reads what it
//! answered, the way the macOS side reads a child's exit.

use std::os::fd::OwnedFd;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use fidget_core::caffeinate::{Argv, LinuxPlan as Plan, Spawner};
use zbus::blocking::{connection, Connection};
use zbus::zvariant::OwnedObjectPath;

/// Who and why, as `systemd-inhibit --list` prints them.
pub const WHO: &str = "Fidget";
pub const WHY: &str = "Caffeinate";

/// Why the cup and the menu row are disabled when no inhibitor service answers.
pub const UNAVAILABLE: &str =
    "Caffeinate needs systemd-logind, elogind or a desktop screen saver service, and none answered.";

const LOGIN1: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const SCREEN_SAVER: &str = "org.freedesktop.ScreenSaver";
const SCREEN_SAVER_PATH: &str = "/org/freedesktop/ScreenSaver";
const PORTAL: &str = "org.freedesktop.portal.Desktop";
/// `org.freedesktop.portal.Inhibit` flag for "idle", which covers blanking.
const PORTAL_IDLE: u32 = 8;

/// A bus that does not answer gives up after this, per call.
const TIMEOUT: Duration = Duration::from_secs(2);

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

/// The display inhibitor and the session connection it lives on.
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

/// Every inhibitor one spawn took.
struct Inhibitors {
    logind: Option<OwnedFd>,
    screen: Option<Screen>,
}

impl Inhibitors {
    fn release(self) {
        drop(self.logind);
        if let Some(screen) = self.screen {
            screen.release();
        }
    }
}

/// Every inhibitor the plan asks for, or none. A part that fails drops the
/// parts already taken, and the error names the inhibitor that failed.
fn take_all<L, S>(
    plan: Plan,
    logind: impl FnOnce(&str) -> Result<L, String>,
    screen: impl FnOnce() -> Result<S, String>,
) -> Result<(Option<L>, Option<S>), String> {
    let logind_fd = match plan.logind {
        Some(what) => {
            Some(logind(what).map_err(|error| format!("logind inhibit {what}: {error}"))?)
        }
        None => None,
    };
    let display = match plan.display {
        true => Some(screen().map_err(|error| format!("screen saver inhibit: {error}"))?),
        false => None,
    };
    Ok((logind_fd, display))
}

/// Releases what a `Take` took. Runs on the worker.
type Release = Box<dyn FnOnce() + Send>;
/// Takes every inhibitor a plan asks for. Runs on the worker.
type Take = Box<dyn FnMut(Plan) -> Result<Release, String> + Send>;

fn take_on_the_bus(plan: Plan) -> Result<Release, String> {
    let (logind, screen) = take_all(
        plan,
        |what| inhibit_logind(what).map_err(|error| error.to_string()),
        || Screen::inhibit().map_err(|error| error.to_string()),
    )?;
    Ok(Box::new(move || Inhibitors { logind, screen }.release()))
}

/// Each order carries the spawn it belongs to, so a late answer about an old
/// spawn cannot end a newer one.
enum Order {
    Take(u64, Plan),
    Release(u64),
}

enum Answer {
    Taken,
    Refused(u64, String),
    Released(u64),
}

fn work(mut take: Take, orders: Receiver<Order>, answers: Sender<Answer>) {
    let mut taken: Option<Release> = None;
    for order in orders {
        let answer = match order {
            Order::Take(spawn, plan) => match take(plan) {
                Ok(release) => {
                    taken = Some(release);
                    Answer::Taken
                }
                Err(reason) => Answer::Refused(spawn, reason),
            },
            Order::Release(spawn) => {
                if let Some(release) = taken.take() {
                    release();
                }
                Answer::Released(spawn)
            }
        };
        if answers.send(answer).is_err() {
            break;
        }
    }
    if let Some(release) = taken.take() {
        release();
    }
}

pub struct OsSpawner {
    orders: Sender<Order>,
    answers: Receiver<Answer>,
    /// The latest spawn's number.
    spawn: u64,
    /// Asked for and not yet refused or released.
    up: bool,
    releasing: bool,
    failure: Option<String>,
}

impl OsSpawner {
    pub fn new() -> Self {
        start_probe();
        Self::with(Box::new(take_on_the_bus))
    }

    fn with(take: Take) -> Self {
        let (orders, worker_orders) = mpsc::channel();
        let (worker_answers, answers) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("caffeinate-inhibit".to_string())
            .spawn(move || work(take, worker_orders, worker_answers));
        Self {
            orders,
            answers,
            spawn: 0,
            up: false,
            releasing: false,
            failure: None,
        }
    }

    fn hear(&mut self, answer: Answer) {
        match answer {
            Answer::Taken => {}
            Answer::Refused(spawn, reason) if spawn == self.spawn => {
                self.up = false;
                self.failure = Some(reason);
            }
            Answer::Released(spawn) if spawn == self.spawn => self.up = false,
            Answer::Refused(..) | Answer::Released(_) => {}
        }
    }
}

impl Drop for OsSpawner {
    /// The worker releases when the order channel closes.
    fn drop(&mut self) {
        self.kill();
    }
}

impl Spawner for OsSpawner {
    /// Returns once the worker has the order. A refusal comes back through
    /// `alive` and `failure`.
    fn spawn(&mut self, argv: &Argv) -> Result<(), String> {
        if self.up {
            return Err("the inhibitors are already taken".to_string());
        }
        let plan = Plan::of(argv.flags());
        if plan.is_empty() {
            return Err("no flag maps to a Linux inhibitor".to_string());
        }
        self.spawn += 1;
        self.orders
            .send(Order::Take(self.spawn, plan))
            .map_err(|_| "the caffeinate worker stopped".to_string())?;
        self.up = true;
        self.releasing = false;
        self.failure = None;
        Ok(())
    }

    /// Asks once. `alive` stays true until the worker says it let go.
    fn kill(&mut self) {
        if self.up && !self.releasing {
            self.releasing = self.orders.send(Order::Release(self.spawn)).is_ok();
            if !self.releasing {
                self.up = false;
            }
        }
    }

    fn alive(&mut self) -> bool {
        while let Ok(answer) = self.answers.try_recv() {
            self.hear(answer);
        }
        self.up
    }

    fn failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fidget_core::caffeinate::{AssertedFlags, Flag};
    use std::sync::mpsc::SyncSender;

    fn flags(set: &[Flag]) -> AssertedFlags {
        AssertedFlags::new(set.iter().copied()).expect("a flag")
    }

    fn dims() -> Argv {
        AssertedFlags::all().argv(std::process::id())
    }

    /// After `hear_next` the tests read `up`, not `alive`: `alive` would
    /// also take answers the test has yet to wait for.
    impl OsSpawner {
        /// Blocks for the worker's next answer. Tests only.
        fn hear_next(&mut self) {
            let answer = self.answers.recv().expect("the worker answers");
            self.hear(answer);
        }
    }

    /// A take that blocks until the test opens the gate, then reports its
    /// release on `released`.
    fn gated(gate: Receiver<Result<(), String>>, released: SyncSender<()>) -> Take {
        Box::new(move |_| {
            gate.recv().expect("the test opens the gate")?;
            let released = released.clone();
            Ok(Box::new(move || {
                let _ = released.send(());
            }) as Release)
        })
    }

    #[test]
    fn disk_alone_takes_no_inhibitor() {
        let mut spawner = OsSpawner::with(Box::new(|_| Err("never asked".to_string())));
        let err = spawner
            .spawn(&flags(&[Flag::Disk]).argv(std::process::id()))
            .expect_err("no inhibitor");
        assert!(!err.is_empty());
        assert!(!spawner.alive());
    }

    /// The frame loop calls `spawn` and `kill`; a bus that takes its 2 s
    /// timeout must not block either.
    #[test]
    fn spawn_and_kill_return_while_the_bus_is_still_busy() {
        let (open, gate) = mpsc::channel();
        let (released_tx, released) = mpsc::sync_channel(1);
        let mut spawner = OsSpawner::with(gated(gate, released_tx));
        spawner.spawn(&dims()).expect("ordered");
        assert!(spawner.alive(), "up while the worker is taking it");
        assert!(spawner.spawn(&dims()).is_err(), "one spawn at a time");
        spawner.kill();
        assert!(spawner.alive(), "up until the worker lets go");
        open.send(Ok(())).expect("gate");
        spawner.hear_next();
        assert!(spawner.up, "still on, release not heard yet");
        spawner.hear_next();
        assert!(!spawner.up);
        released.recv().expect("released on the worker");
        assert_eq!(spawner.failure(), None);
    }

    #[test]
    fn a_refusal_turns_caffeinate_off_and_names_the_inhibitor() {
        let (open, gate) = mpsc::channel();
        let (released_tx, _released) = mpsc::sync_channel(1);
        let mut spawner = OsSpawner::with(gated(gate, released_tx));
        spawner.spawn(&dims()).expect("ordered");
        open.send(Err("screen saver inhibit: no service".to_string()))
            .expect("gate");
        spawner.hear_next();
        assert!(!spawner.up);
        assert_eq!(
            spawner.failure().as_deref(),
            Some("screen saver inhibit: no service")
        );
        assert_eq!(spawner.failure(), None, "said once");
    }

    /// The answer about a refused spawn must not end the spawn after it.
    #[test]
    fn a_late_answer_about_an_old_spawn_leaves_the_new_one_up() {
        let (open, gate) = mpsc::channel();
        let (released_tx, _released) = mpsc::sync_channel(1);
        let mut spawner = OsSpawner::with(gated(gate, released_tx));
        spawner.spawn(&dims()).expect("first");
        spawner.kill();
        open.send(Err("logind inhibit idle:sleep: denied".to_string()))
            .expect("gate");
        spawner.hear_next();
        assert!(!spawner.up, "refused");
        spawner.spawn(&dims()).expect("second");
        spawner.hear_next();
        assert!(spawner.up, "the first spawn's answer is not this one's");
    }

    /// A partial take fails whole: the Chat note says "keeping your computer
    /// awake", which keeping only some would make untrue.
    #[test]
    fn a_failed_part_drops_the_part_already_taken() {
        struct Token(SyncSender<()>);
        impl Drop for Token {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (dropped_tx, dropped) = mpsc::sync_channel(1);
        let taken = take_all(
            Plan::of(AssertedFlags::all()),
            |what| {
                assert_eq!(what, "idle:sleep");
                Ok(Token(dropped_tx))
            },
            || Err::<(), _>("no service".to_string()),
        );
        assert_eq!(
            taken.err().as_deref(),
            Some("screen saver inhibit: no service")
        );
        dropped.try_recv().expect("the logind part was released");

        let refused = take_all(
            Plan::of(AssertedFlags::all()),
            |_| Err::<(), _>("denied".to_string()),
            || -> Result<(), String> { panic!("not asked after logind failed") },
        );
        assert_eq!(
            refused.err().as_deref(),
            Some("logind inhibit idle:sleep: denied")
        );
    }

    /// The `who` of every logind inhibitor now.
    fn logind_inhibitor_whos(conn: &Connection) -> Vec<String> {
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
    fn a_real_logind_lists_the_inhibitor_until_it_is_released() {
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
        spawner.spawn(&dims()).expect("ordered");
        spawner.hear_next();
        assert!(spawner.alive(), "taken: {:?}", spawner.failure());
        assert!(logind_inhibitor_whos(&conn).iter().any(|who| who == WHO));
        print_inhibitors("taken");

        spawner.kill();
        spawner.hear_next();
        assert!(!spawner.alive());
        changes
            .into_iter()
            .find(|_| !logind_inhibitor_whos(&conn).iter().any(|who| who == WHO))
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
