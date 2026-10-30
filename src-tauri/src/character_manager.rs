//! Import and manage user Character ZIPs without requiring a shell environment.
//!
//! Packaging and validation happen on a worker; the native file chooser uses
//! the non-blocking dialog API so it cannot stall Fidget's desktop companion.

use std::fs;
use std::path::Path;

use serde::Serialize;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::package;
use crate::SettingsState;

#[derive(Serialize)]
pub struct ManagedCharacter {
    pub id: String,
    pub name: String,
    pub removable: bool,
}

/// Only user-owned archives are removable. Bundled and external-path packages
/// remain visible in the normal Character picker, but are never removed here.
#[tauri::command]
pub async fn character_manager_list(
    app: tauri::AppHandle,
) -> Result<Vec<ManagedCharacter>, String> {
    tauri::async_runtime::spawn_blocking(move || managed_characters(&app))
        .await
        .map_err(|error| format!("Character Manager: {error}"))?
}

fn managed_characters(app: &tauri::AppHandle) -> Result<Vec<ManagedCharacter>, String> {
    let state = app
        .try_state::<SettingsState>()
        .ok_or("Character Manager: Fidget is still starting")?;
    let selected = state
        .settings
        .lock()
        .map_err(|_| "Character Manager: settings unavailable")?
        .character
        .clone();
    let in_use: Vec<String> = state
        .instances
        .lock()
        .map_err(|_| "Character Manager: instances unavailable")?
        .iter()
        .map(|instance| instance.character.clone())
        .collect();
    let mut result = Vec::new();
    for entry in fs::read_dir(package::user_characters_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !package::safe_package_id(id) {
            continue;
        }
        let Ok(bytes) = package::read(&path) else {
            continue;
        };
        let Ok(character) = fidget_core::character::load(&bytes) else {
            continue;
        };
        let removable = selected != character.name && !in_use.contains(&character.name);
        result.push(ManagedCharacter {
            id: id.to_string(),
            name: character.name,
            removable,
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

/// Import from a native picker. Cancel is not an error. No JS-supplied path is
/// accepted, and the source archive stays untouched.
#[tauri::command]
pub async fn character_manager_import(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Fidget character ZIP", &["zip"])
        .pick_file(move |path| {
            let _ = sender.send(path);
        });

    let chosen = receiver
        .await
        .map_err(|_| "Character Manager: file picker did not return")?;
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let source = chosen
        .into_path()
        .map_err(|error| format!("Character Manager: {error}"))?;
    let name = tauri::async_runtime::spawn_blocking(move || import_archive(&app, &source))
        .await
        .map_err(|error| format!("Character Manager: {error}"))??;
    Ok(Some(name))
}

fn import_archive(app: &tauri::AppHandle, source: &Path) -> Result<String, String> {
    if !source.is_file()
        || !source
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        return Err("Choose a .zip character package".to_string());
    }
    let bytes = package::read_import_archive(source)?;
    let character = fidget_core::character::load(&bytes)
        .map_err(|errors| format!("Character package is invalid:\n{}", errors.join("\n")))?;

    // Matching display names have one runtime identity. Refuse a shadow copy
    // rather than install one that the existing cache will never select.
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("characters"));
    for existing in package::installed(&package::search_paths(bundled)) {
        if let Ok(files) = package::read(&existing) {
            if let Ok(other) = fidget_core::character::load(&files) {
                if other.name.eq_ignore_ascii_case(&character.name) {
                    return Err(format!("{} is already installed", character.name));
                }
            }
        }
    }
    package::save_user_archive(&bytes, &character.name)?;
    Ok(character.name)
}

/// Refuse to uninstall a selected or running Character, and never take a path
/// from the webview. The package ID is a restricted file stem, not a filename.
#[tauri::command]
pub async fn character_manager_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || remove_archive(&app, &id))
        .await
        .map_err(|error| format!("Character Manager: {error}"))?
}

fn remove_archive(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    if !package::safe_package_id(id) {
        return Err("Invalid character package ID".to_string());
    }
    let path = package::user_characters_dir().join(format!("{id}.zip"));
    let character =
        fidget_core::character::load(&package::read(&path).map_err(|error| error.to_string())?)
            .map_err(|errors| errors.join("\n"))?;
    let state = app
        .try_state::<SettingsState>()
        .ok_or("Fidget is still starting")?;
    let selected = state
        .settings
        .lock()
        .map_err(|_| "Settings unavailable")?
        .character
        == character.name;
    let running = state
        .instances
        .lock()
        .map_err(|_| "Instances unavailable")?
        .iter()
        .any(|instance| instance.character == character.name);
    if selected || running {
        return Err(format!(
            "{} is selected or running. Switch to another Character and restart Fidget first.",
            character.name
        ));
    }
    fs::remove_file(path).map_err(|error| format!("Could not remove character: {error}"))
}
