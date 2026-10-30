//! Import Characters from a ZIP or folder into Fidget's own imports folder, and
//! remove them from there. A folder is packed into a ZIP first, so both take
//! one validate-and-save path, and neither source is ever written or deleted.
//!
//! Packaging and validation happen on a worker; the native file chooser uses
//! the non-blocking dialog API so it cannot stall Fidget's desktop companion.

use std::path::Path;

use serde::Serialize;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::package;
use crate::SettingsState;

#[derive(Serialize)]
pub struct ImportedCharacter {
    pub id: String,
    pub name: String,
    pub removable: bool,
}

/// Only user-owned archives are removable. Bundled and external-path packages
/// remain visible in the normal Character picker, but are never removed here.
#[tauri::command]
pub async fn character_import_list(
    app: tauri::AppHandle,
) -> Result<Vec<ImportedCharacter>, String> {
    tauri::async_runtime::spawn_blocking(move || imported_characters(&app))
        .await
        .map_err(|error| error.to_string())?
}

fn imported_characters(app: &tauri::AppHandle) -> Result<Vec<ImportedCharacter>, String> {
    let state = app
        .try_state::<SettingsState>()
        .ok_or("Fidget is still starting")?;
    let selected = state
        .settings
        .lock()
        .map_err(|_| "Settings unavailable")?
        .character
        .clone();
    let in_use: Vec<String> = state
        .instances
        .lock()
        .map_err(|_| "Instances unavailable")?
        .iter()
        .map(|instance| instance.character.clone())
        .collect();
    Ok(package::imports(&package::imported_characters_dir())
        .into_iter()
        .map(|(id, character)| ImportedCharacter {
            id,
            removable: selected != character.name && !in_use.contains(&character.name),
            name: character.name,
        })
        .collect())
}

/// Import from a native picker. Cancel is not an error. No JS-supplied path is
/// accepted, and the source archive stays untouched.
#[tauri::command]
pub async fn character_import_zip(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Fidget character ZIP", &["zip"])
        .pick_file(move |path| {
            let _ = sender.send(path);
        });

    let chosen = receiver
        .await
        .map_err(|_| "The file picker did not return")?;
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let source = chosen.into_path().map_err(|error| error.to_string())?;
    let name = tauri::async_runtime::spawn_blocking(move || import_archive(&app, &source))
        .await
        .map_err(|error| error.to_string())??;
    Ok(Some(name))
}

/// Import a folder laid out like a Character ZIP, from a native folder picker.
#[tauri::command]
pub async fn character_import_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |path| {
        let _ = sender.send(path);
    });

    let chosen = receiver
        .await
        .map_err(|_| "The folder picker did not return")?;
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let source = chosen.into_path().map_err(|error| error.to_string())?;
    let name = tauri::async_runtime::spawn_blocking(move || import_folder(&app, &source))
        .await
        .map_err(|error| error.to_string())??;
    Ok(Some(name))
}

/// The packed ZIP is a temp file, removed when this returns.
fn import_folder(app: &tauri::AppHandle, source: &Path) -> Result<String, String> {
    let mut archive = tempfile::Builder::new()
        .suffix(".zip")
        .tempfile()
        .map_err(|e| format!("Cannot start character import: {e}"))?;
    package::pack_import_directory(source, archive.as_file_mut())?;
    import_archive(app, archive.path())
}

fn import_archive(app: &tauri::AppHandle, source: &Path) -> Result<String, String> {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("characters"));
    let installed = package::installed(&package::search_paths(bundled));
    package::import_package(&package::imported_characters_dir(), &installed, source)
}

/// Refuse to uninstall a selected or running Character, and never take a path
/// from the webview. The package ID is a restricted file stem, not a filename.
#[tauri::command]
pub async fn character_import_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || forget_import(&app, &id))
        .await
        .map_err(|error| error.to_string())?
}

/// Deletes only Fidget's managed copy in the imports folder, never the user's source ZIP or folder.
fn forget_import(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let state = app
        .try_state::<SettingsState>()
        .ok_or("Fidget is still starting")?;
    package::forget_import(&package::imported_characters_dir(), id, |name| {
        let selected = state
            .settings
            .lock()
            .map_err(|_| "Settings unavailable")?
            .character
            == name;
        let running = state
            .instances
            .lock()
            .map_err(|_| "Instances unavailable")?
            .iter()
            .any(|instance| instance.character == name);
        if selected || running {
            return Err(format!(
                "{name} is selected or running. Switch to another Character and restart Fidget first."
            ));
        }
        Ok(())
    })
}
