//! Login item for a packaged build.
//!
//! A checkout binary is not an app a login item can start, so `sync` does
//! not register one.

use std::path::Path;

use tauri::{AppHandle, Manager};

/// Register or remove the OS login item. No-op for a checkout binary, and
/// when the plugin did not register.
pub fn sync(app: &AppHandle, wanted: bool) {
    if !process_is_bundled() {
        return;
    }
    let Some(manager) = app.try_state::<tauri_plugin_autostart::AutoLaunchManager>() else {
        fidget::eprintln_and_log!("launch at login: plugin is not registered");
        return;
    };
    let result = if wanted {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(why) = result {
        fidget::eprintln_and_log!("launch at login: {why}");
    }
}

/// Whether this process is an installed app a login item can start. A failed
/// `current_exe` is not one: a login item would point at nothing.
pub fn process_is_bundled() -> bool {
    std::env::current_exe().is_ok_and(|exe| exe_is_bundled(&exe))
}

/// Whether `exe` is an installed app a login item can start.
pub fn exe_is_bundled(exe: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        exe.ancestors()
            .any(|path| path.extension().is_some_and(|ext| ext == "app"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        !under_cargo_target(exe)
    }
}

/// Whether `exe` sits in a Cargo `target` directory. Empty and `.` segments
/// are skipped, as `Path::components` does, and `\` separates only on Windows.
#[cfg(any(test, not(target_os = "macos")))]
fn under_cargo_target(exe: &Path) -> bool {
    under_cargo_target_split(exe, cfg!(windows))
}

#[cfg(any(test, not(target_os = "macos")))]
fn under_cargo_target_split(exe: &Path, backslash_separates: bool) -> bool {
    let path = exe.to_string_lossy();
    let parts: Vec<_> = path
        .split(|c| c == '/' || (backslash_separates && c == '\\'))
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let profile = |name: &str| name == "debug" || name == "release";
    // A cross build is `target/<triple>/debug`, not `target/debug`.
    parts
        .windows(2)
        .any(|window| window[0] == "target" && profile(window[1]))
        || parts
            .windows(3)
            .any(|window| window[0] == "target" && profile(window[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn a_bundle_can_launch_at_login_and_a_checkout_cannot() {
        assert!(exe_is_bundled(Path::new(
            "/Applications/Fidget.app/Contents/MacOS/fidget"
        )));
        assert!(!exe_is_bundled(Path::new(
            "/work/fidget/target/debug/fidget"
        )));
        assert!(!exe_is_bundled(Path::new(
            "/work/fidget/target/release/fidget"
        )));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_bundle_can_launch_at_login_and_a_checkout_cannot() {
        assert!(exe_is_bundled(Path::new("/usr/bin/fidget")));
        assert!(!exe_is_bundled(Path::new(
            "/work/fidget/target/debug/fidget"
        )));
        assert!(!exe_is_bundled(Path::new(
            "/work/fidget/target/release/fidget"
        )));
    }

    #[test]
    fn a_target_triple_build_is_still_a_checkout() {
        assert!(under_cargo_target(Path::new(
            "/work/fidget/target/x86_64-unknown-linux-gnu/debug/fidget"
        )));
        assert!(under_cargo_target(Path::new(
            "/work/fidget/target/x86_64-pc-windows-msvc/release/fidget.exe"
        )));
        assert!(!under_cargo_target(Path::new("/usr/bin/fidget")));
    }

    #[test]
    fn empty_and_dot_segments_do_not_hide_a_checkout() {
        for checkout in [
            "/work/fidget/target//debug/fidget",
            "/work/fidget/target/./debug/fidget",
            "/work/fidget/target/x/./debug/fidget",
        ] {
            assert!(under_cargo_target(Path::new(checkout)), "{checkout}");
        }
    }

    #[test]
    fn a_backslash_is_part_of_a_name_except_on_windows() {
        let path = Path::new(r"/a\target/debug/fidget");
        assert!(!under_cargo_target_split(path, false));
        assert!(under_cargo_target_split(path, true));
    }

    #[test]
    fn windows_paths_split_on_backslashes() {
        let under_cargo_target = |path: &Path| under_cargo_target_split(path, true);
        for checkout in [
            r"C:\work\fidget\target\debug\fidget.exe",
            r"C:\work\fidget\target\release\fidget.exe",
            r"C:\work\fidget\target\x86_64-pc-windows-msvc\debug\fidget.exe",
            r"C:\work\fidget\target\x86_64-pc-windows-msvc\release\fidget.exe",
            r"target\release\fidget.exe",
        ] {
            assert!(under_cargo_target(Path::new(checkout)), "{checkout}");
        }
        for installed in [
            r"C:\Program Files\Fidget\fidget.exe",
            r"C:\Users\me\AppData\Local\Fidget\fidget.exe",
            r"D:\Portable\Fidget\fidget.exe",
            r"C:\Users\me\Downloads\release\fidget.exe",
        ] {
            assert!(!under_cargo_target(Path::new(installed)), "{installed}");
        }
    }
}
