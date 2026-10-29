//! The inhibitors this process takes on Linux.
//!
//! Idle and System are one `org.freedesktop.login1` inhibitor. Logind keeps it
//! while any copy of the fd it returns is open, and the kernel closes every fd
//! when the process ends. Display is an `org.freedesktop.ScreenSaver` cookie,
//! which the screen saver drops when this connection closes. So a crash,
//! `kill -9` or Quit's `process::exit` (which skips `Drop`) leaves nothing
//! behind, and this side needs no `CHILD_PID`.
//!
//! The bus calls block for up to [`TIMEOUT`] each, so a worker thread makes
//! them. `turn_on` and `turn_off` only send it an order, and `is_on` reads
//! what it answered, the way the macOS side reads a child's exit.

use std::os::fd::OwnedFd;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use fidget_core::caffeinate::linux::Plan;
use fidget_core::caffeinate::{Flags, OsCaffeinate, ALREADY_ON};
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

    /// Dropping the connection would end it too; asking first ends it now
    /// rather than when the bus notices.
    fn uninhibit(self) {
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

/// Every inhibitor one `turn_on` took.
struct Active {
    logind: Option<OwnedFd>,
    screen: Option<Screen>,
}

impl Active {
    fn close(self) {
        drop(self.logind);
        if let Some(screen) = self.screen {
            screen.uninhibit();
        }
    }
}

/// Every inhibitor the plan asks for, or none. A part that fails drops the
/// parts already taken, and the error names the inhibitor that failed.
fn inhibit_all<L, S>(
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

/// Closes what a `TurnOn` took. Runs on the worker.
type TurnOff = Box<dyn FnOnce() + Send>;
/// Takes every inhibitor a plan asks for. Runs on the worker.
type TurnOn = Box<dyn FnMut(Plan) -> Result<TurnOff, String> + Send>;

fn inhibit_on_the_bus(plan: Plan) -> Result<TurnOff, String> {
    let (logind, screen) = inhibit_all(
        plan,
        |what| inhibit_logind(what).map_err(|error| error.to_string()),
        || Screen::inhibit().map_err(|error| error.to_string()),
    )?;
    Ok(Box::new(move || Active { logind, screen }.close()))
}

/// Each order carries the `turn_on` it belongs to, so a late answer about an
/// old one cannot end a newer one.
enum Order {
    On(u64, Plan),
    Off(u64),
}

enum Answer {
    On,
    Refused(u64, String),
    Off(u64),
}

fn work(mut turn_on: TurnOn, orders: Receiver<Order>, answers: Sender<Answer>) {
    let mut active: Option<TurnOff> = None;
    for order in orders {
        let answer = match order {
            Order::On(turn, plan) => match turn_on(plan) {
                Ok(turn_off) => {
                    active = Some(turn_off);
                    Answer::On
                }
                Err(reason) => Answer::Refused(turn, reason),
            },
            Order::Off(turn) => {
                if let Some(turn_off) = active.take() {
                    turn_off();
                }
                Answer::Off(turn)
            }
        };
        if answers.send(answer).is_err() {
            break;
        }
    }
    if let Some(turn_off) = active.take() {
        turn_off();
    }
}

pub struct Inhibitors {
    orders: Sender<Order>,
    answers: Receiver<Answer>,
    /// The latest `turn_on`'s number.
    turn: u64,
    /// Asked for and not yet refused or turned off.
    on: bool,
    turning_off: bool,
    failure: Option<String>,
}

impl Inhibitors {
    pub fn new() -> Self {
        start_probe();
        Self::with(Box::new(inhibit_on_the_bus))
    }

    /// The disabled reason, once the startup probe found neither logind nor a
    /// screen saver. `None` while the probe runs or when either answered.
    pub fn unavailable() -> Option<&'static str> {
        PROBE.get().copied().flatten()
    }

    /// The kernel closes the logind fd and the bus drops the screen saver
    /// cookie when the process ends.
    pub fn on_quit() {}

    fn with(turn_on: TurnOn) -> Self {
        let (orders, worker_orders) = mpsc::channel();
        let (worker_answers, answers) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("caffeinate-inhibit".to_string())
            .spawn(move || work(turn_on, worker_orders, worker_answers));
        Self {
            orders,
            answers,
            turn: 0,
            on: false,
            turning_off: false,
            failure: None,
        }
    }

    fn hear(&mut self, answer: Answer) {
        match answer {
            Answer::On => {}
            Answer::Refused(turn, reason) if turn == self.turn => {
                self.on = false;
                self.failure = Some(reason);
            }
            Answer::Off(turn) if turn == self.turn => self.on = false,
            Answer::Refused(..) | Answer::Off(_) => {}
        }
    }
}

impl Drop for Inhibitors {
    /// The worker turns off when the order channel closes.
    fn drop(&mut self) {
        self.turn_off();
    }
}

impl OsCaffeinate for Inhibitors {
    /// Returns once the worker has the order. A refusal comes back through
    /// `is_on` and `failure`.
    fn turn_on(&mut self, flags: Flags) -> Result<(), String> {
        if self.on {
            return Err(ALREADY_ON.to_string());
        }
        let plan = Plan::of(flags);
        if plan.is_empty() {
            return Err("no flag maps to a Linux inhibitor".to_string());
        }
        self.turn += 1;
        self.orders
            .send(Order::On(self.turn, plan))
            .map_err(|_| "the caffeinate worker stopped".to_string())?;
        self.on = true;
        self.turning_off = false;
        self.failure = None;
        Ok(())
    }

    /// Asks once. `is_on` stays true until the worker says it let go.
    fn turn_off(&mut self) {
        if self.on && !self.turning_off {
            self.turning_off = self.orders.send(Order::Off(self.turn)).is_ok();
            if !self.turning_off {
                self.on = false;
            }
        }
    }

    fn is_on(&mut self) -> bool {
        while let Ok(answer) = self.answers.try_recv() {
            self.hear(answer);
        }
        self.on
    }

    fn failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fidget_core::caffeinate::Flag;
    use std::sync::mpsc::SyncSender;

    fn flags(set: &[Flag]) -> Flags {
        Flags::new(set.iter().copied()).expect("a flag")
    }

    /// After `hear_next` the tests read `on`, not `is_on`, because `is_on`
    /// would also take answers the test has yet to wait for.
    impl Inhibitors {
        /// Blocks for the worker's next answer. Tests only.
        fn hear_next(&mut self) {
            let answer = self.answers.recv().expect("the worker answers");
            self.hear(answer);
        }
    }

    /// A `TurnOn` that blocks until the test opens the gate, then reports its
    /// turn-off on `released`.
    fn gated(gate: Receiver<Result<(), String>>, released: SyncSender<()>) -> TurnOn {
        Box::new(move |_| {
            gate.recv().expect("the test opens the gate")?;
            let released = released.clone();
            Ok(Box::new(move || {
                let _ = released.send(());
            }) as TurnOff)
        })
    }

    #[test]
    fn disk_alone_takes_no_inhibitor() {
        let mut inhibitors = Inhibitors::with(Box::new(|_| Err("never asked".to_string())));
        let err = inhibitors
            .turn_on(flags(&[Flag::Disk]))
            .expect_err("no inhibitor");
        assert!(!err.is_empty());
        assert!(!inhibitors.is_on());
    }

    /// The frame loop calls `turn_on` and `turn_off`. A bus that takes its 2 s
    /// timeout must not block either.
    #[test]
    fn turn_on_and_turn_off_return_while_the_bus_is_still_busy() {
        let (open, gate) = mpsc::channel();
        let (released_tx, released) = mpsc::sync_channel(1);
        let mut inhibitors = Inhibitors::with(gated(gate, released_tx));
        inhibitors.turn_on(Flags::all()).expect("ordered");
        assert!(inhibitors.is_on(), "on while the worker turns on");
        assert!(
            inhibitors.turn_on(Flags::all()).is_err(),
            "one turn_on at a time"
        );
        inhibitors.turn_off();
        assert!(inhibitors.is_on(), "on until the worker turns off");
        open.send(Ok(())).expect("gate");
        inhibitors.hear_next();
        assert!(inhibitors.on, "still on, the turn off not heard yet");
        inhibitors.hear_next();
        assert!(!inhibitors.on);
        released.recv().expect("released on the worker");
        assert_eq!(inhibitors.failure(), None);
    }

    #[test]
    fn a_refusal_turns_caffeinate_off_and_names_the_inhibitor() {
        let (open, gate) = mpsc::channel();
        let (released_tx, _released) = mpsc::sync_channel(1);
        let mut inhibitors = Inhibitors::with(gated(gate, released_tx));
        inhibitors.turn_on(Flags::all()).expect("ordered");
        open.send(Err("screen saver inhibit: no service".to_string()))
            .expect("gate");
        inhibitors.hear_next();
        assert!(!inhibitors.on);
        assert_eq!(
            inhibitors.failure().as_deref(),
            Some("screen saver inhibit: no service")
        );
        assert_eq!(inhibitors.failure(), None, "said once");
    }

    /// The answer about a refused `turn_on` must not end the one after it.
    #[test]
    fn a_late_answer_about_an_old_turn_on_leaves_the_new_one_on() {
        let (open, gate) = mpsc::channel();
        let (released_tx, _released) = mpsc::sync_channel(1);
        let mut inhibitors = Inhibitors::with(gated(gate, released_tx));
        inhibitors.turn_on(Flags::all()).expect("first");
        inhibitors.turn_off();
        open.send(Err("logind inhibit idle:sleep: denied".to_string()))
            .expect("gate");
        inhibitors.hear_next();
        assert!(!inhibitors.on, "refused");
        inhibitors.turn_on(Flags::all()).expect("second");
        inhibitors.hear_next();
        assert!(
            inhibitors.on,
            "the first turn_on's answer is not this one's"
        );
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
        let taken = inhibit_all(
            Plan::of(Flags::all()),
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

        let refused = inhibit_all(
            Plan::of(Flags::all()),
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

    /// Opt-in: `cargo test -p fidget caffeinate::linux -- --ignored`, as a user
    /// logind lets inhibit. Logind releases asynchronously, so it waits for the
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

        let mut inhibitors = Inhibitors::new();
        inhibitors.turn_on(Flags::all()).expect("ordered");
        inhibitors.hear_next();
        assert!(inhibitors.is_on(), "taken: {:?}", inhibitors.failure());
        assert!(logind_inhibitor_whos(&conn).iter().any(|who| who == WHO));
        print_inhibitors("taken");

        inhibitors.turn_off();
        inhibitors.hear_next();
        assert!(!inhibitors.is_on());
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
