//! Leaving the keyboard where it was when Fidget launches.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};

/// The frontmost app's pid, or `None` when that is Fidget or nothing. Main
/// thread.
pub fn other_app_in_front() -> Option<i32> {
    let workspace = AnyClass::get(c"NSWorkspace").expect("AppKit defines NSWorkspace");
    // SAFETY: plain NSWorkspace / NSRunningApplication getters.
    let pid: i32 = unsafe {
        let shared: *mut AnyObject = msg_send![workspace, sharedWorkspace];
        let front: *mut AnyObject = msg_send![shared, frontmostApplication];
        if front.is_null() {
            return None;
        }
        msg_send![front, processIdentifier]
    };
    (pid != std::process::id() as i32).then_some(pid)
}

/// Activate `pid` again after Fidget's first window showed. Main thread.
///
/// AppKit holds a launch's activation until the app's first window shows, then
/// takes the keyboard from a frontmost parent. No window flag, show call or
/// activation policy stopped that on macOS 27.2, so the launch gives it back.
pub fn give_front_back(pid: i32) {
    let running = AnyClass::get(c"NSRunningApplication").expect("AppKit defines it");
    // SAFETY: a class getter and one action on the object it returned.
    unsafe {
        let previous: *mut AnyObject =
            msg_send![running, runningApplicationWithProcessIdentifier: pid];
        if !previous.is_null() {
            let _: bool = msg_send![previous, activateWithOptions: 0usize];
        }
    }
}
