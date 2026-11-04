//! Glass's blur on macOS: a native `NSVisualEffectView` behind the webview.
//! The CSS `backdrop-filter` blur drops from Settings about 1 s after another
//! Fidget window closes, such as Chat closing while Settings is open (#1854).
//! Focus loss alone keeps it. The best read, unproven, is that WebKit's
//! backdrop layer stops sampling. AppKit's view sits outside WebKit's layers
//! and is held active.

#[cfg(any(target_os = "macos", test))]
use tauri::utils::config::WindowEffectsConfig;
#[cfg(any(target_os = "macos", test))]
use tauri::window::{Effect, EffectState, EffectsBuilder};

/// `.chat-ui-glass body`'s `border-radius` in chat-ui.css, so the blur ends
/// at the panel's rounded corners.
#[cfg(any(target_os = "macos", test))]
const PANEL_RADIUS: f64 = 18.0;

/// The window effect a Chat UI design wears: Glass's blur, held `Active`.
/// Left unset, the state is `FollowsWindowActiveState`, which drops the blur
/// whenever the window is inactive.
#[cfg(any(target_os = "macos", test))]
fn effects(chat_ui: &str) -> Option<WindowEffectsConfig> {
    (chat_ui == "glass").then(|| {
        EffectsBuilder::new()
            .effect(Effect::HudWindow)
            .state(EffectState::Active)
            .radius(PANEL_RADIUS)
            .build()
    })
}

/// Put on or take off Glass's blur to match `chat_ui`. Nothing off macOS:
/// Windows keeps its CSS blur, which cannot see the desktop, so Windows
/// tints harder like Linux.
pub(crate) fn wear(window: &tauri::WebviewWindow, chat_ui: &str) {
    #[cfg(target_os = "macos")]
    if let Err(why) = window.set_effects(effects(chat_ui)) {
        eprintln!("glass: {} blur: {why}", window.label());
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, chat_ui);
}

/// Whether the window labelled `label` wears the Chat UI: every Chat
/// (`chat_label`) and Settings.
fn wears_chat_ui(label: &str) -> bool {
    label == "settings" || label.starts_with("chat-")
}

/// Put on or take off Glass's blur on every open window that wears the
/// Chat UI, after a live UI Theme switch.
pub(crate) fn wear_all(app: &tauri::AppHandle, chat_ui: &str) {
    use tauri::Manager;
    for (label, window) in app.webview_windows() {
        if wears_chat_ui(&label) {
            wear(&window, chat_ui);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{effects, wears_chat_ui};
    use tauri::window::EffectState;

    #[test]
    fn glass_blur_is_held_active() {
        let glass = effects("glass").expect("Glass wears a window effect");
        assert_eq!(glass.state, Some(EffectState::Active));
    }

    #[test]
    fn designs_that_paint_their_own_panel_wear_no_effect() {
        for design in ["minimal", "terminal", "", "Glass"] {
            assert_eq!(effects(design), None, "{design:?}");
        }
    }

    #[test]
    fn chats_and_settings_wear_the_chat_ui() {
        for label in ["settings", "chat-0", "chat-7f3a"] {
            assert!(wears_chat_ui(label), "{label:?}");
        }
        for label in ["overlay-0", "main", "chat", "settings-1"] {
            assert!(!wears_chat_ui(label), "{label:?}");
        }
    }
}
