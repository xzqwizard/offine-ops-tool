pub mod builder;
pub mod build_history;
pub mod catalog;
mod commands;
pub mod docker_pkgs;
pub mod docker_versions;
mod engine;
mod error;
pub mod images;
pub mod models;
mod net;
pub mod store;
pub mod xlsx_export;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::create_project,
            commands::save_project,
            commands::load_project,
            commands::clone_project,
            commands::delete_project,
            commands::get_settings,
            commands::save_settings,
            commands::get_storage_info,
            commands::get_disk_space,
            commands::backup_app_data,
            commands::restore_app_data,
            catalog::list_catalog,
            builder::build_offline_package,
            docker_versions::list_docker_versions,
            net::test_network,
            engine::engine_status,
            engine::engine_install,
            images::inspect_image,
            images::list_image_tags,
            images::list_image_cache,
            images::delete_cached_image,
            images::pull_image,
            docker_pkgs::list_docker_pkgs,
            docker_pkgs::list_compose_plugins,
            docker_pkgs::delete_docker_pkg,
            docker_pkgs::import_docker_pkgs,
            docker_pkgs::download_docker_static,
            docker_pkgs::download_compose_plugin,
            xlsx_export::export_port_matrix_xlsx,
            build_history::list_build_history,
            build_history::delete_build,
            build_history::open_dir_in_explorer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
