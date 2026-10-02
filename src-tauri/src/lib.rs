pub mod app_ops;
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
            catalog::save_custom_template,
            catalog::delete_custom_template,
            catalog::fetch_remote_catalog,
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
            app_ops::list_audit_log,
            app_ops::restore_project_from_build,
            app_ops::export_project,
            app_ops::import_project,
            app_ops::analyze_cache_usage,
            app_ops::purge_unref_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ==================== CLI 构建支持 ====================

/// CLI 模式构建（main.rs --build 调用）：初始化最小 Tauri 应用获取路径上下文，
/// 结果写入 <cwd>/build-cli-result.txt，随后退出进程
pub fn run_cli_build(project_id: &str) -> String {
    let code = project_id.to_string();
    tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let code2 = code.clone();
            let result = std::thread::spawn(move || {
                let proj = match store::load_project(&handle, &code2) {
                    Ok(p) => p,
                    Err(e) => return format!("[cli] 加载方案失败: {e}"),
                };
                match builder::build_inner(&handle, &proj, true, None) {
                    Ok(r) => format!(
                        "[cli] 构建完成: {}
[cli] 产物: {}
[cli] 服务器: {} 台",
                        r.build_id, r.output_dir, r.servers.len()
                    ),
                    Err(e) => format!("[cli] 构建失败: {e}"),
                }
            })
            .join()
            .unwrap_or_else(|_| "[cli] 构建线程异常".into());
            let _ = std::fs::write("build-cli-result.txt", &result);
            println!("{result}");
            std::process::exit(0);
        })
        .run(tauri::generate_context!())
        .expect("cli build failed");
    unreachable!()
}
