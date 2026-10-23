//! The one `caffeinate` child this process holds.
//!
//! `Child::kill` is SIGKILL. A child that ignores SIGTERM keeps the assertion
//! until `try_wait` says it left; `-w` is what releases it if this process
//! dies first. Quit signals the pid from another thread because
//! `process::exit` skips this thread's `Drop`.

use std::process::{Child, Command};
use std::sync::atomic::{AtomicU32, Ordering};

use fidget_core::caffeinate::{
    Argv, AssertedFlags, CaffeinateHandle, Empty, Fact, Machine, Note, Request, Spawner, Tick,
};

static HELD_PID: AtomicU32 = AtomicU32::new(0);

/// SIGTERM the child, if one is still held. Safe to call from Quit.
pub fn signal_held_child() {
    let pid = HELD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        signal_term(pid);
    }
}

fn signal_term(pid: u32) {
    // SAFETY: SIGTERM to a pid we spawned. A recycled pid is the kernel's
    // to reject; we only store a pid we have not yet reaped.
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
}

fn remember(pid: u32) {
    HELD_PID.store(pid, Ordering::SeqCst);
}

fn forget(pid: u32) {
    let _ = HELD_PID.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst);
}

pub struct OsSpawner {
    program: &'static str,
    child: Option<Child>,
}

impl OsSpawner {
    pub fn new() -> Self {
        Self::program("/usr/bin/caffeinate")
    }

    fn program(program: &'static str) -> Self {
        Self {
            program,
            child: None,
        }
    }
}

impl Drop for OsSpawner {
    fn drop(&mut self) {
        self.kill();
        if let Some(child) = self.child.as_mut() {
            let _ = child.wait();
        }
    }
}

impl Spawner for OsSpawner {
    fn spawn(&mut self, argv: &Argv) -> Result<(), String> {
        if self.child.is_some() {
            return Err("a caffeinate child is already held".to_string());
        }
        let child = Command::new(self.program)
            .args(argv.args())
            .spawn()
            .map_err(|error| error.to_string())?;
        remember(child.id());
        self.child = Some(child);
        Ok(())
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_ref() {
            signal_term(child.id());
        }
    }

    fn alive(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        if let Ok(Some(_)) = child.try_wait() {
            forget(child.id());
            self.child = None;
            return false;
        }
        true
    }
}

/// The frame loop's hold. Notes from tool calls wait here until the frame loop
/// posts them to Chat.
pub struct Hold {
    machine: Machine<OsSpawner>,
    configured: Result<AssertedFlags, Empty>,
    pending: Vec<Note>,
}

impl Hold {
    pub fn new() -> Self {
        Self {
            machine: Machine::new(OsSpawner::new(), std::process::id()),
            configured: AssertedFlags::try_from_bits(true, true, true, true),
            pending: Vec::new(),
        }
    }

    pub fn set_target(&mut self, configured: Result<AssertedFlags, Empty>) {
        self.configured = configured;
    }

    pub fn tick(&mut self, requests: &[Request]) -> Tick {
        let tick = self.machine.tick(self.configured, requests);
        self.pending.extend(tick.notes.iter().cloned());
        tick
    }

    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.pending)
    }
}

impl CaffeinateHandle for Hold {
    fn call(&mut self, on: Option<bool>) -> Result<Fact, String> {
        let requests: Vec<Request> = on.map(Request::Set).into_iter().collect();
        let tick = self.tick(&requests);
        match tick.error {
            Some(error) => Err(error),
            None => Ok(tick.fact),
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn dims() -> Argv {
        AssertedFlags::try_from_bits(true, true, true, true)
            .expect("four flags")
            .argv(std::process::id())
    }

    #[test]
    fn a_missing_program_holds_nothing() {
        let mut spawner = OsSpawner::program("/no/such/caffeinate");
        let err = spawner.spawn(&dims()).expect_err("missing program");
        assert!(!err.is_empty());
        assert!(!spawner.alive());
    }

    #[test]
    fn a_real_caffeinate_leaves_when_signalled() {
        let mut spawner = OsSpawner::new();
        spawner.spawn(&dims()).expect("caffeinate starts");
        assert!(spawner.alive());
        let listed = Command::new("pmset")
            .args(["-g", "assertions"])
            .output()
            .expect("pmset");
        let text = String::from_utf8_lossy(&listed.stdout);
        assert!(
            text.contains("caffeinate"),
            "pmset should name the hold while it is up: {text}"
        );
        let again = spawner.spawn(&dims());
        assert!(again.is_err(), "a second child would hide the first");
        assert!(spawner.alive());
        spawner.kill();
        let mut alive = true;
        for _ in 0..50 {
            alive = spawner.alive();
            if !alive {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive);
        assert_eq!(HELD_PID.load(Ordering::SeqCst), 0);
    }
}
