mod catalog;
mod commands;
mod error;
mod models;
mod store;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::create_project,
            commands::save_project,
            commands::load_project,
            commands::delete_project,
            commands::get_settings,
            commands::save_settings,
            commands::get_storage_info,
            catalog::list_catalog,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
