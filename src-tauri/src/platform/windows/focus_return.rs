//! Keyboard hand-back on Windows when an overlay's last quick message pill
//! closes (#1577): back to the window in front at pill-open, or a step aside.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
};

use crate::pill_focus::{hand_back, HandBack};

use super::super::windows_hand_back::{step_aside_target, worth_remembering};
use super::window_source::visible_windows;

/// The foreground window when a pill took the keyboard; 0 is none.
static FOREGROUND_BEFORE_PILL: AtomicIsize = AtomicIsize::new(0);

fn owner_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: a plain query; a stale hwnd leaves pid at 0.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

/// Remember `foreground` before the pill takes the keyboard, unless it is
/// Fidget's own (the overlay already in front) or none.
pub fn remember_foreground(foreground: HWND) {
    if worth_remembering(foreground as u64, owner_pid(foreground), std::process::id()) {
        FOREGROUND_BEFORE_PILL.store(foreground as isize, Ordering::Relaxed);
    }
}

/// Fidget's own, minimized, or cloaked (another virtual desktop, a
/// suspended app): no window to step aside to.
fn skipped(hwnd: HWND, own_pid: u32) -> bool {
    let mut cloaked = 0u32;
    // SAFETY: Win32 queries on a window from visible_windows' walk; cloaked
    // is a live u32 the size passed.
    unsafe {
        owner_pid(hwnd) == own_pid
            || IsIconic(hwnd) != 0
            || (DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            ) >= 0
                && cloaked != 0)
    }
}

/// The overlay's last pill closed: give the keyboard back. Main thread.
pub fn hand_back_keyboard(overlay: HWND) -> Result<(), String> {
    let previous = FOREGROUND_BEFORE_PILL.swap(0, Ordering::Relaxed) as HWND;
    // SAFETY: Win32 queries and SetForegroundWindow on window handles;
    // IsWindow vets the remembered one first.
    let (decision, target) = unsafe {
        let holding = GetForegroundWindow() == overlay;
        let alive = !previous.is_null() && IsWindow(previous) != 0;
        let decision = hand_back(holding, alive);
        let target = match decision {
            HandBack::Restore => Some(previous),
            HandBack::StepAside => {
                let own_pid = std::process::id();
                let ids: Vec<u64> = visible_windows(false).iter().map(|w| w.id).collect();
                step_aside_target(&ids, |id| skipped(id as HWND, own_pid)).map(|id| id as HWND)
            }
            HandBack::Stay => None,
        };
        (decision, target)
    };
    // SAFETY: target is a live top-level window from the checks above.
    let result = target.map(|hwnd| unsafe { SetForegroundWindow(hwnd) });
    if crate::dev_flags::TRACE_BUBBLE.is_on() {
        eprintln!(
            "{}",
            hand_back_line(decision, target.map(|h| h as usize), result)
        );
    }
    Ok(())
}

/// The trace a scenario waits on after a pill closes.
fn hand_back_line(decision: HandBack, target: Option<usize>, result: Option<i32>) -> String {
    match (target, result) {
        (Some(hwnd), Some(result)) => {
            format!("overlay: hand back {decision:?} to {hwnd:#x} -> {result}")
        }
        _ => format!("overlay: hand back {decision:?} to none"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hand_back_line_names_the_decision_target_and_result() {
        assert_eq!(
            hand_back_line(HandBack::Restore, Some(0x1a2b), Some(1)),
            "overlay: hand back Restore to 0x1a2b -> 1"
        );
        assert_eq!(
            hand_back_line(HandBack::StepAside, None, None),
            "overlay: hand back StepAside to none"
        );
        assert_eq!(
            hand_back_line(HandBack::Stay, None, None),
            "overlay: hand back Stay to none"
        );
    }
}
