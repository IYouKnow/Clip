use clipper23_engine::EngineStatus;

/// Returns the current engine status. Wired to the real engine as the
/// capture/encode pipeline comes online.
#[tauri::command]
fn get_status() -> EngineStatus {
    EngineStatus::default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
