//! The menu bar icon. Same menu as the sprite's right-click.
//!
//! One definition (`menu::describe`) and two entry points, so a row cannot
//! exist on the sprite and not here. WindowPet's tray is the reference for
//! putting settings and quit on a menu bar icon; this is our menu, not theirs.

use crate::menu::{self, MenuDescription};
use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;

/// Put the shared menu on a menu bar icon.
///
/// Left-click opens the same menu as right-click so settings is reachable without the sprite.
pub fn install(
    app: &AppHandle,
    description: &MenuDescription,
) -> Result<tauri::tray::TrayIcon, tauri::Error> {
    let menu = menu::build(app, description)?;

    let tray_icon_bytes = include_bytes!("../icons/tray.png");
    let decoded = image::load_from_memory(tray_icon_bytes).expect("Failed to decode tray icon PNG");
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let tray_icon = Image::new_owned(rgba.into_raw(), width, height);

    let icon = TrayIconBuilder::new()
        .menu(&menu)
        .icon(tray_icon)
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
) -> Result<(), tauri::Error> {
    let menu = menu::build(app, description)?;
    tray.set_menu(Some(menu))
}
