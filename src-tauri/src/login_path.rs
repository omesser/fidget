//! The `PATH` a Harness and its `npx` need when launchd or a desktop file
//! started Fidget with the system's short one (#1436).

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{IsTerminal, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(5);

const MARK: &str = "__FIDGET_LOGIN_PATH__";

/// Where a launcher's own folders end and the system's begin.
const SYSTEM_DIRS: &[&str] = &["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"];

/// Homebrew, version-manager shims and per-user installers, for when the login
/// shell does not answer.
const USUAL_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];
const USUAL_HOME_DIRS: &[&str] = &[
    ".local/bin",
    ".cargo/bin",
    ".volta/bin",
    ".bun/bin",
    ".local/share/mise/shims",
    ".asdf/shims",
];

#[derive(Debug)]
enum Failure {
    Spawn(std::io::Error),
    TimedOut(Duration),
    Unmarked,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Spawn(error) => write!(f, "could not run: {error}"),
            Failure::TimedOut(after) => write!(f, "timed out after {}s", after.as_secs()),
            Failure::Unmarked => write!(f, "printed no PATH"),
        }
    }
}

/// Merges the login shell's `PATH` into ours, so every child Fidget spawns
/// finds what a new terminal finds. A terminal launch already has it.
pub fn adopt() {
    if std::io::stdin().is_terminal() {
        fidget::eprintln_and_log!("path: started from a terminal, kept its PATH");
        return;
    }
    let shell = login_shell();
    let home = fidget_core::memory::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let current = std::env::var_os("PATH").unwrap_or_default();
    let started = Instant::now();
    let (path, how) = match login_shell_path(&shell, &home, &current, TIMEOUT) {
        Ok(found) => (
            merged(&current, &found),
            format!(
                "{} answered in {} ms",
                shell.display(),
                started.elapsed().as_millis()
            ),
        ),
        Err(why) => (
            merged(&current, &usual_dirs(&home)),
            format!("{} {why}, added the usual install folders", shell.display()),
        ),
    };
    std::env::set_var("PATH", &path);
    fidget::eprintln_and_log!("path: {how}: {}", path.to_string_lossy());
}

fn login_shell() -> PathBuf {
    match std::env::var_os("SHELL") {
        Some(shell) if !shell.is_empty() => PathBuf::from(shell),
        _ if cfg!(target_os = "macos") => PathBuf::from("/bin/zsh"),
        _ => PathBuf::from("/bin/sh"),
    }
}

/// `-l` reads `.zprofile`, or bash's `.bash_profile` or `.profile`, and `-i`
/// adds zsh's `.zshrc`. Bash reads `.bashrc` only when one of those sources
/// it, as the Debian, Ubuntu and Fedora defaults do. A session of its own, so
/// an interactive shell never takes a terminal. Done at the second mark rather
/// than at end of output, because a job the rc files leave running keeps the
/// pipe open.
fn login_shell_path(
    shell: &Path,
    home: &Path,
    start: &OsStr,
    timeout: Duration,
) -> Result<OsString, Failure> {
    let mut command = Command::new(shell);
    command
        .args(["-l", "-i", "-c"])
        .arg(format!("echo {MARK}; printenv PATH; echo {MARK}"))
        .env("HOME", home)
        .env("PATH", start)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: `setsid` is async-signal-safe and touches no memory of ours.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn().map_err(Failure::Spawn)?;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut seen = Vec::new();
        let mut chunk = [0; 4096];
        while let Ok(read @ 1..) = stdout.read(&mut chunk) {
            seen.extend_from_slice(&chunk[..read]);
            if let Some(path) = marked_path(&seen) {
                let _ = tx.send(path);
                return;
            }
        }
    });
    let found = rx.recv_timeout(timeout);
    // SAFETY: the child is not reaped yet, so its pid is still the group
    // `setsid` made, and no other process can hold it.
    unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
    let _ = child.wait();
    match found {
        Ok(path) => Ok(path),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Failure::TimedOut(timeout)),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Failure::Unmarked),
    }
}

/// The text between the first two marks, so whatever the rc files print
/// around it does not count.
fn marked_path(stdout: &[u8]) -> Option<OsString> {
    let rest = &stdout[mark_at(stdout)? + MARK.len()..];
    let path = rest[..mark_at(rest)?].trim_ascii();
    (!path.is_empty()).then(|| OsStr::from_bytes(path).to_os_string())
}

fn mark_at(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(MARK.len())
        .position(|window| window == MARK.as_bytes())
}

/// The inherited folders before its first system folder, then the shell's,
/// then the rest of the inherited ones, each once. A login shell rebuilds
/// `PATH` system first (Debian's `/etc/profile`, macOS `path_helper`), so
/// without the split a launcher's own folder would lose to `/usr/bin`. An
/// empty entry, which would mean the working directory, is dropped.
fn merged(inherited: &OsStr, shell: &OsStr) -> OsString {
    let inherited: Vec<PathBuf> = std::env::split_paths(inherited).collect();
    let system_at = inherited
        .iter()
        .position(|dir| SYSTEM_DIRS.iter().any(|system| dir == Path::new(system)))
        .unwrap_or(inherited.len());
    let (leading, rest) = inherited.split_at(system_at);
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in leading
        .iter()
        .cloned()
        .chain(std::env::split_paths(shell))
        .chain(rest.iter().cloned())
    {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).expect("folders split from a PATH join back")
}

fn usual_dirs(home: &Path) -> OsString {
    let dirs = USUAL_DIRS
        .iter()
        .map(PathBuf::from)
        .chain(USUAL_HOME_DIRS.iter().map(|dir| home.join(dir)))
        .filter(|dir| dir.is_dir());
    std::env::join_paths(dirs).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU32, Ordering};

    const LAUNCHD_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
    const FAKE: &str = "fidget-fake-harness";
    const RERUN: &str = "FIDGET_LOGIN_PATH_RERUN";
    const PROGRAM: &str = "FIDGET_LOGIN_PATH_PROGRAM";

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "fidget-login-path-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).expect("temp home is creatable");
            Self(dir)
        }

        fn write(&self, name: &str, body: &str) {
            fs::write(self.0.join(name), body).expect("rc file is writable");
        }

        fn fake_harness_in(&self, dir: &str) -> PathBuf {
            self.fake_in(dir, FAKE)
        }

        fn fake_in(&self, dir: &str, name: &str) -> PathBuf {
            let bin = self.0.join(dir);
            fs::create_dir_all(&bin).expect("bin dir is creatable");
            let program = bin.join(name);
            fs::write(&program, format!("#!/bin/sh\necho fake {name} ran\n"))
                .expect("fake is writable");
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
                .expect("fake is executable");
            bin
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn rerun(name: &str) -> [String; 4] {
        let module = module_path!();
        let rest = module.split_once("::").map_or(module, |(_, rest)| rest);
        [
            format!("{rest}::{name}"),
            "--exact".into(),
            "--test-threads=1".into(),
            "--nocapture".into(),
        ]
    }

    /// Runs only when re-executed: Fidget's startup step, then a spawn by bare
    /// name, the Harness unless the test names another program.
    #[test]
    fn startup_child() {
        if std::env::var_os(RERUN).is_none() {
            return;
        }
        adopt();
        let path = std::env::var_os("PATH").unwrap_or_default();
        println!("path={}", path.to_string_lossy());
        let program = std::env::var_os(PROGRAM).unwrap_or_else(|| FAKE.into());
        match Command::new(&program).output() {
            Ok(output) => {
                print!("{}", String::from_utf8_lossy(&output.stdout));
                std::process::exit(0);
            }
            Err(error) => {
                print!("spawn {}: {error}", program.to_string_lossy());
                std::process::exit(3);
            }
        }
    }

    /// The test binary, started with a cleared environment besides `HOME`,
    /// `SHELL` and `PATH`, then spawning `program` by bare name.
    fn launched(
        shell: &str,
        home: &TempHome,
        path: &OsStr,
        stdin: Stdio,
        program: &str,
    ) -> (Option<i32>, String) {
        let output = Command::new(std::env::current_exe().expect("test binary"))
            .args(rerun("startup_child"))
            .env_clear()
            .env(RERUN, "1")
            .env(PROGRAM, program)
            .env("HOME", &home.0)
            .env("SHELL", shell)
            .env("PATH", path)
            .stdin(stdin)
            .output()
            .expect("rerun starts");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }

    /// Started the way launchd starts Fidget.app: no terminal and the short
    /// `PATH`.
    fn launched_from_finder(shell: &str, home: &TempHome) -> (Option<i32>, String) {
        launched(shell, home, OsStr::new(LAUNCHD_PATH), Stdio::null(), FAKE)
    }

    /// A pseudo-terminal's follower end, as a terminal hands its stdin to a
    /// program, and the leader that keeps it open.
    fn terminal() -> (OwnedFd, OwnedFd) {
        let (mut leader, mut follower) = (-1, -1);
        // SAFETY: both fds are written on success only; null name, termios
        // and window size are allowed.
        let opened = unsafe {
            libc::openpty(
                &mut leader,
                &mut follower,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(opened, 0, "openpty: {}", std::io::Error::last_os_error());
        // SAFETY: openpty just opened both, and nothing else owns them.
        unsafe { (OwnedFd::from_raw_fd(leader), OwnedFd::from_raw_fd(follower)) }
    }

    fn launchd_finds_fake() -> bool {
        Command::new(FAKE)
            .env_clear()
            .env("PATH", LAUNCHD_PATH)
            .output()
            .is_ok()
    }

    #[test]
    fn only_the_text_between_the_marks_is_the_path() {
        let noisy = format!("Welcome back!\n{MARK}\n/opt/homebrew/bin:/usr/bin\n{MARK}\nbye\n");
        assert_eq!(
            marked_path(noisy.as_bytes()),
            Some(OsString::from("/opt/homebrew/bin:/usr/bin"))
        );
        let one_mark = format!("Welcome back!\n{MARK}\n/opt/homebrew/bin:/usr/bin\n");
        assert_eq!(marked_path(one_mark.as_bytes()), None);
        assert_eq!(marked_path(b"/opt/homebrew/bin:/usr/bin\n"), None);
    }

    #[test]
    fn the_shell_path_leads_and_launchd_folders_appear_once() {
        let once = merged(
            OsStr::new(LAUNCHD_PATH),
            OsStr::new("/opt/homebrew/bin:/usr/bin:/bin"),
        );
        assert_eq!(
            once,
            OsString::from("/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin")
        );
        assert_eq!(
            merged(&once, OsStr::new("/opt/homebrew/bin:/usr/bin:/bin")),
            once
        );
    }

    #[test]
    fn a_launchers_folders_before_the_system_ones_stay_ahead_of_the_shells() {
        let debian = OsStr::new("/usr/local/bin:/usr/bin:/bin:/usr/local/games:/usr/games");
        let once = merged(
            OsStr::new("/tmp/out/bin:/usr/bin:/bin:/usr/sbin:/sbin"),
            debian,
        );
        assert_eq!(
            once,
            OsString::from(
                "/tmp/out/bin:/usr/local/bin:/usr/bin:/bin:/usr/local/games:/usr/games:/usr/sbin:/sbin"
            )
        );
        assert_eq!(merged(&once, debian), once);

        let path_helper = OsStr::new(
            "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/me/bin:/opt/homebrew/bin",
        );
        assert_eq!(
            merged(OsStr::new("/Users/me/bin:/usr/bin:/bin"), path_helper),
            OsString::from(
                "/Users/me/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"
            )
        );
    }

    #[test]
    fn an_empty_or_system_free_launcher_path_merges_without_a_working_directory_entry() {
        assert_eq!(
            merged(OsStr::new(""), OsStr::new("/opt/homebrew/bin:/usr/bin")),
            OsString::from("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(
            merged(OsStr::new("/a::/b"), OsStr::new("/usr/bin:/a")),
            OsString::from("/a:/b:/usr/bin")
        );
    }

    #[test]
    fn a_harness_bash_profile_puts_on_path_runs_after_a_finder_launch() {
        let home = TempHome::new("bash");
        let bin = home.fake_harness_in("tools/bin");
        home.write(
            ".bash_profile",
            &format!(
                "echo rc says hello\nexport PATH=\"{}:$PATH\"\necho rc says bye\n",
                bin.display()
            ),
        );
        assert!(!launchd_finds_fake(), "launchd's PATH has no {FAKE}");
        let (code, stdout) = launched_from_finder("/bin/bash", &home);
        assert_eq!(code, Some(0), "stdout: {stdout}");
        assert!(
            stdout.contains("fake fidget-fake-harness ran"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn a_launchers_leading_folder_still_wins_when_the_shell_rebuilds_path_system_first() {
        assert!(
            Path::new("/usr/bin/true").exists(),
            "a real true in /usr/bin is what the fake must beat"
        );
        let home = TempHome::new("leading");
        let lead = home.fake_in("out/bin", "true");
        home.write(
            ".bash_profile",
            "export PATH=\"/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin\"\n",
        );
        let path = format!("{}:{LAUNCHD_PATH}", lead.display());
        let (code, stdout) = launched("/bin/bash", &home, OsStr::new(&path), Stdio::null(), "true");
        assert_eq!(code, Some(0), "stdout: {stdout}");
        assert!(stdout.contains("fake true ran"), "stdout: {stdout}");
    }

    #[test]
    fn a_terminal_launch_keeps_its_path_and_reads_no_rc_file() {
        let home = TempHome::new("terminal");
        let bin = home.fake_harness_in("tools/bin");
        let read = home.0.join("bash_profile-was-read");
        home.write(
            ".bash_profile",
            &format!(
                "touch \"{}\"\nexport PATH=\"{}:$PATH\"\n",
                read.display(),
                bin.display()
            ),
        );

        let (_leader, follower) = terminal();
        let (code, stdout) = launched(
            "/bin/bash",
            &home,
            OsStr::new(LAUNCHD_PATH),
            Stdio::from(follower),
            FAKE,
        );
        assert_eq!(code, Some(3), "stdout: {stdout}");
        assert!(
            stdout.contains(&format!("path={LAUNCHD_PATH}\n")),
            "stdout: {stdout}"
        );
        assert!(!read.exists(), "a terminal launch ran .bash_profile");

        let (code, stdout) = launched_from_finder("/bin/bash", &home);
        assert_eq!(code, Some(0), "stdout: {stdout}");
        assert!(
            stdout.contains("fake fidget-fake-harness ran"),
            "stdout: {stdout}"
        );
        assert!(read.exists(), "a null-stdin launch skipped .bash_profile");
    }

    #[test]
    fn a_job_the_rc_file_leaves_running_does_not_hold_up_startup() {
        let home = TempHome::new("background");
        let bin = home.fake_harness_in("tools/bin");
        home.write(
            ".bash_profile",
            &format!("sleep 60 &\nexport PATH=\"{}:$PATH\"\n", bin.display()),
        );
        let started = Instant::now();
        let (code, stdout) = launched_from_finder("/bin/bash", &home);
        let took = started.elapsed();
        assert_eq!(code, Some(0), "stdout: {stdout}");
        assert!(
            stdout.contains("fake fidget-fake-harness ran"),
            "stdout: {stdout}"
        );
        assert!(took < Duration::from_secs(3), "startup took {took:?}");
    }

    #[test]
    fn a_harness_zshrc_puts_on_path_runs_after_a_finder_launch() {
        if !Path::new("/bin/zsh").exists() {
            eprintln!("no /bin/zsh here; macOS CI runs this");
            return;
        }
        let home = TempHome::new("zsh");
        let bin = home.fake_harness_in("tools/bin");
        home.write(
            ".zshrc",
            &format!(
                "echo rc says hello\nexport PATH=\"{}:$PATH\"\n",
                bin.display()
            ),
        );
        assert!(!launchd_finds_fake(), "launchd's PATH has no {FAKE}");
        let (code, stdout) = launched_from_finder("/bin/zsh", &home);
        assert_eq!(code, Some(0), "stdout: {stdout}");
        assert!(
            stdout.contains("fake fidget-fake-harness ran"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn a_hanging_rc_file_still_finds_the_usual_install_folders_in_time() {
        let home = TempHome::new("hang");
        home.fake_harness_in(".local/bin");
        home.write(".bash_profile", "sleep 60\n");
        assert!(!launchd_finds_fake(), "launchd's PATH has no {FAKE}");
        let started = Instant::now();
        let (code, stdout) = launched_from_finder("/bin/bash", &home);
        let took = started.elapsed();
        assert_eq!(code, Some(0), "stdout: {stdout}");
        assert!(
            stdout.contains("fake fidget-fake-harness ran"),
            "stdout: {stdout}"
        );
        assert!(took < Duration::from_secs(15), "startup took {took:?}");
    }
}
