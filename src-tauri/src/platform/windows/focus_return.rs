//! Keyboard hand-back for Windows when a quick message pill closes.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsIconic, IsWindow, SetForegroundWindow,
};

use crate::pill_focus::{hand_back, HandBack};

/// The foreground window when a pill took the keyboard; 0 is none.
static FOREGROUND_BEFORE_PILL: AtomicIsize = AtomicIsize::new(0);

/// Remember the foreground window before a pill takes the keyboard.
pub fn remember_foreground(hwnd: HWND) {
    FOREGROUND_BEFORE_PILL.store(hwnd as isize, Ordering::Relaxed);

    if crate::dev_flags::TRACE_BUBBLE.is_on() {
        eprintln!("overlay: remembered foreground hwnd={:#x}", hwnd as usize);
    }
}

/// Next eligible window to step aside to, from visible_windows filtering.
fn next_window_in_front(_overlay: HWND) -> Option<HWND> {
    // TODO: implement using window_source::visible_windows / counts_for_desktop
    None
}

/// The overlay's last pill closed: give the foreground back.
pub fn hand_back_keyboard(overlay: HWND) -> Result<(), String> {
    let previous = FOREGROUND_BEFORE_PILL.swap(0, Ordering::Relaxed) as HWND;

    // SAFETY: Win32 queries on window handles; IsWindow vets the remembered one.
    unsafe {
        let foreground_before = GetForegroundWindow();
        let holding = foreground_before == overlay;
        let alive = !previous.is_null() && IsWindow(previous) != 0;

        let decision = hand_back(holding, alive);

        if crate::dev_flags::TRACE_BUBBLE.is_on() {
            eprintln!(
                "overlay: hand_back decision={decision:?} holding={holding} alive={alive}"
            );
        }

        match decision {
            HandBack::Restore => {
                SetForegroundWindow(previous);
            }
            HandBack::StepAside => {
                if let Some(next) = next_window_in_front(overlay) {
                    SetForegroundWindow(next);
                }
            }
            HandBack::Stay => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_aside_skips_our_own_pid() {
        // This test should pass once next_window_in_front is implemented
        assert!(true, "TODO: test that step-aside filters our own PID");
    }

    #[test]
    fn step_aside_skips_cloaked_windows() {
        // This test should pass once next_window_in_front is implemented
        assert!(true, "TODO: test that step-aside filters cloaked windows");
    }

    #[test]
    fn step_aside_skips_minimized_windows() {
        // This test should pass once next_window_in_front is implemented
        assert!(true, "TODO: test that step-aside filters minimized windows");
    }
}
