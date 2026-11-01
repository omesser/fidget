//! The menu bar icon. Same menu as the sprite's right-click.
//!
//! One definition (`menu::describe`) and two entry points, so a row cannot
//! exist on the sprite and not here. WindowPet's tray is the reference for
//! putting settings and quit on a menu bar icon; this is our menu, not theirs.

use crate::menu::{self, MenuDescription};
use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;

/// Pixels decoded at compile time. The path is relative to `src-tauri`.
const TRAY_ICON: Image<'static> = tauri::include_image!("icons/tray.png");

/// Put the shared menu on a menu bar icon.
///
/// Left-click opens the same menu as right-click so settings is reachable without the sprite.
pub fn install(
    app: &AppHandle,
    description: &MenuDescription,
    quit_generation: u64,
) -> Result<tauri::tray::TrayIcon, tauri::Error> {
    let menu = menu::build(app, description, quit_generation)?;

    let icon = TrayIconBuilder::new()
        .menu(&menu)
        .icon(TRAY_ICON)
        .show_menu_on_left_click(true)
        .tooltip("fidget")
        .build(app)?;

    #[cfg(target_os = "macos")]
    if let Err(why) = crate::platform::tune_tray_icon(&icon) {
        eprintln!("tray: tune icon: {why}");
    }

    Ok(icon)
}

/// Rebuild the tray menu after a toggle, so the checkboxes match the Engine.
pub fn refresh(
    tray: &tauri::tray::TrayIcon,
    app: &AppHandle,
    description: &MenuDescription,
    quit_generation: u64,
) -> Result<(), tauri::Error> {
    let menu = menu::build(app, description, quit_generation)?;
    tray.set_menu(Some(menu))
}

#[cfg(test)]
mod tests {
    use super::TRAY_ICON;

    #[test]
    fn tray_icon_is_a_64px_white_mark_on_transparent_corners() {
        let icon = TRAY_ICON;
        assert_eq!((icon.width(), icon.height()), (64, 64));
        assert_eq!(icon.rgba().len(), 64 * 64 * 4);
        let pixel = |x: usize, y: usize| &icon.rgba()[(y * 64 + x) * 4..][..4];
        for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
            assert_eq!(pixel(x, y)[3], 0, "corner ({x},{y}) is transparent");
        }
        assert_eq!(pixel(32, 32), [255, 255, 255, 255]);
    }
}
