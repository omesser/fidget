//! Login item for a packaged build.
//!
//! A checkout binary is not an app a login item can start, so nothing here
//! registers one unless `exe_is_bundled` says this process is.

use std::path::Path;

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// Register or remove the OS login item. No-op for a checkout binary.
pub fn sync(app: &AppHandle, wanted: bool) {
    if !bundled_exe() {
        return;
    }
    let manager = app.autolaunch();
    let result = if wanted {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(why) = result {
        fidget::eprintln_and_log!("launch at login: {why}");
    }
}

/// `current_exe` when it can be read. A failure is not a bundle: registering
/// a login item with no path would point at nothing.
pub fn bundled_exe() -> bool {
    std::env::current_exe().is_ok_and(|exe| exe_is_bundled(&exe))
}

/// An installed app. On macOS that is a `.app`. Elsewhere it is any binary
/// that is not a `cargo` build under `target/debug` or `target/release`.
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

#[cfg(not(target_os = "macos"))]
fn under_cargo_target(exe: &Path) -> bool {
    let mut parts = exe.components();
    while let Some(component) = parts.next() {
        if component.as_os_str() != "target" {
            continue;
        }
        if let Some(next) = parts.next() {
            let name = next.as_os_str();
            if name == "debug" || name == "release" {
                return true;
            }
        }
    }
    false
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
}
