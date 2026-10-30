//! Windows pointer button state via GetAsyncKeyState.
//!
//! Polled each tick beside cursor position. GetAsyncKeyState reads current button
//! state without hooks or events, same seam as macOS CGEventSourceButtonState
//! and X11 XQueryPointer.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetDoubleClickTime, VK_LBUTTON, VK_RBUTTON,
};

use crate::platform::ButtonsDown;

/// Both buttons from GetAsyncKeyState.
/// High bit means pressed. Failure reads as nothing held. The overlay
/// witness in `platform.rs` is the other half of the answer.
pub fn buttons_down() -> ButtonsDown {
    ButtonsDown {
        primary: button_down(VK_LBUTTON.into()),
        secondary: button_down(VK_RBUTTON.into()),
    }
}

fn button_down(vk_button: i32) -> bool {
    // SAFETY: GetAsyncKeyState is documented safe; a bad vk_button yields
    // zero (not pressed), which is the safe answer. High bit means down.
    unsafe { GetAsyncKeyState(vk_button) < 0 }
}

/// The OS double-click interval, in milliseconds.
pub fn double_click_interval_ms() -> Option<u32> {
    let ms = unsafe { GetDoubleClickTime() };
    if ms > 0 {
        Some(ms)
    } else {
        None
    }
}
