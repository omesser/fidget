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

#[cfg(any(test, not(target_os = "macos")))]
fn under_cargo_target(exe: &Path) -> bool {
    let parts: Vec<_> = exe.components().collect();
    let profile = |name: &std::ffi::OsStr| name == "debug" || name == "release";
    // A cross build is `target/<triple>/debug`, not `target/debug`.
    parts
        .windows(2)
        .any(|window| window[0].as_os_str() == "target" && profile(window[1].as_os_str()))
        || parts
            .windows(3)
            .any(|window| window[0].as_os_str() == "target" && profile(window[2].as_os_str()))
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

    #[cfg(windows)]
    #[test]
    fn a_windows_target_triple_build_is_a_checkout() {
        for exe in [
            r"C:\work\fidget\target\debug\fidget.exe",
            r"C:\work\fidget\target\x86_64-pc-windows-msvc\debug\fidget.exe",
            r"C:\work\fidget\target\x86_64-pc-windows-msvc\release\fidget.exe",
        ] {
            assert!(!exe_is_bundled(Path::new(exe)), "{exe}");
        }
        assert!(exe_is_bundled(Path::new(
            r"C:\Program Files\Fidget\fidget.exe"
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
}
