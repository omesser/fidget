//! The `PATH` a Harness and its `npx` need when launchd or a desktop file
//! started Fidget with the system's short one (#1436).

use std::ffi::{OsStr, OsString};

const MARK: &str = "__FIDGET_LOGIN_PATH__";

/// Puts the login shell's `PATH` in front of ours, so every child Fidget
/// spawns finds what a new terminal finds. A terminal launch already has it.
pub fn adopt() {}

/// The text between the first two marks, so whatever the rc files print
/// around it does not count.
fn marked_path(stdout: &[u8]) -> Option<OsString> {
    let _ = stdout;
    None
}

/// `first`'s folders in order, then `then`'s that `first` lacks.
fn merged(first: &OsStr, then: &OsStr) -> OsString {
    let _ = first;
    then.to_os_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    const LAUNCHD_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
    const FAKE: &str = "fidget-fake-harness";
    const RERUN: &str = "FIDGET_LOGIN_PATH_RERUN";

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
            let bin = self.0.join(dir);
            fs::create_dir_all(&bin).expect("bin dir is creatable");
            let program = bin.join(FAKE);
            fs::write(&program, "#!/bin/sh\necho fake harness ran\n").expect("fake is writable");
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

    /// Runs only when re-executed: Fidget's startup step, then the Harness
    /// spawn by bare name.
    #[test]
    fn startup_child() {
        if std::env::var_os(RERUN).is_none() {
            return;
        }
        adopt();
        match Command::new(FAKE).output() {
            Ok(output) => {
                print!("{}", String::from_utf8_lossy(&output.stdout));
                std::process::exit(0);
            }
            Err(error) => {
                print!("spawn {FAKE}: {error}");
                std::process::exit(3);
            }
        }
    }

    /// The test binary, started the way launchd starts Fidget.app: no
    /// terminal, the short `PATH`, and only `HOME` and `SHELL` besides.
    fn launched_from_finder(shell: &str, home: &TempHome) -> (Option<i32>, String) {
        let output = Command::new(std::env::current_exe().expect("test binary"))
            .args(rerun("startup_child"))
            .env_clear()
            .env(RERUN, "1")
            .env("HOME", &home.0)
            .env("SHELL", shell)
            .env("PATH", LAUNCHD_PATH)
            .stdin(Stdio::null())
            .output()
            .expect("rerun starts");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
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
            OsStr::new("/opt/homebrew/bin:/usr/bin:/bin"),
            OsStr::new(LAUNCHD_PATH),
        );
        assert_eq!(
            once,
            OsString::from("/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin")
        );
        assert_eq!(merged(&once, OsStr::new(LAUNCHD_PATH)), once);
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
        assert!(stdout.contains("fake harness ran"), "stdout: {stdout}");
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
        assert!(stdout.contains("fake harness ran"), "stdout: {stdout}");
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
        assert!(stdout.contains("fake harness ran"), "stdout: {stdout}");
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
        assert!(stdout.contains("fake harness ran"), "stdout: {stdout}");
        assert!(took < Duration::from_secs(15), "startup took {took:?}");
    }
}
