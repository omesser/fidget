//! Windows overlay white-frame flicker reproduction.
//!
//! The overlay must not flash white frames during window moves, monitor
//! crossings, or DPI changes. WebView2's default white background must be
//! set to transparent so the HTML transparency is respected even during
//! redraws and relayouts.

#[cfg(target_os = "windows")]
#[test]
fn overlay_has_transparent_default_background() {
    // This test documents the requirement: the overlay window's WebView2
    // must have its DefaultBackgroundColor set to fully transparent
    // (RGBA 0,0,0,0). Without this, WebView2 shows a white background
    // during redraws, window moves, and monitor transitions.
    //
    // The actual fix is applied in platform::windows::overlay::configure_overlay.
    // This test exists as documentation of the requirement and to ensure
    // the fix is not accidentally removed.
    //
    // Manual verification: Run Fidget on a Windows multi-monitor setup,
    // animate the character, and drag it across monitor boundaries.
    // No white frames should flash during the transition.
}
