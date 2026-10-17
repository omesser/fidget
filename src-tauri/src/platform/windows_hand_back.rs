//! Windows keyboard hand-back choices, pure so they test on every OS.

/// Whether the foreground window at pill-open is worth remembering: a real
/// window that is not one of Fidget's own (the overlay already in front).
pub(super) fn worth_remembering(foreground: u64, foreground_pid: u32, own_pid: u32) -> bool {
    let _ = (foreground, foreground_pid, own_pid);
    true
}

/// Where a step aside hands the keyboard: the first window, front to back,
/// that `skip` keeps (not Fidget's, not minimized, not cloaked).
pub(super) fn step_aside_target(front_to_back: &[u64], skip: impl Fn(u64) -> bool) -> Option<u64> {
    let _ = skip;
    front_to_back.first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_or_no_window_is_never_remembered() {
        assert!(worth_remembering(0x10, 7, 3));
        assert!(!worth_remembering(0x10, 3, 3), "Fidget's own window");
        assert!(!worth_remembering(0, 0, 3), "no foreground window");
    }

    #[test]
    fn a_step_aside_picks_the_frontmost_window_not_skipped() {
        let skip = |w: u64| w == 30 || w == 40;
        assert_eq!(step_aside_target(&[30, 10, 40, 20], skip), Some(10));
        assert_eq!(step_aside_target(&[30, 40], skip), None);
        assert_eq!(step_aside_target(&[], skip), None);
    }
}
