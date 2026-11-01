fn main() {
    // tauri-build copies the shipped Characters next to the binary, but cargo
    // reruns this script only for watched paths. Without this, a new package in
    // ../characters never reaches target/debug/characters.
    println!("cargo:rerun-if-changed=../characters");
    // tauri::include_image! reads the tray icon without cargo tracking it.
    println!("cargo:rerun-if-changed=icons/tray.png");
    // Targeting Windows, tauri-build always compiles a resource file and fails
    // without icons/icon.ico, with no opt-out. scripts/make-app-icon.py writes
    // it beside icons/icon.png from the same art.
    tauri_build::build()
}
