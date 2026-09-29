#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::App::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
