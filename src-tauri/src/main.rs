#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// CLI 模式：OfflinePreOpsTool.exe --build <projectId> [--out log.txt]
// 无窗口后台构建（结果写日志文件），供脚本/CI 调用；其余情况正常启动 GUI
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(idx) = args.iter().position(|a| a == "--build") {
        let project_id = args.get(idx + 1).cloned().unwrap_or_default();
        let log_path = args
            .iter()
            .position(|a| a == "--out")
            .and_then(|i| args.get(i + 1))
            .cloned()
            .unwrap_or_else(|| "build-cli.log".into());
        cli_build(&project_id, &log_path);
        return;
    }
    offline_preops_tool_lib::run()
}

fn cli_build(project_id: &str, log_path: &str) {
    use std::io::Write as _;
    let mut log = std::fs::File::create(log_path).expect("无法创建 CLI 构建日志");
    let _ = writeln!(log, "[cli] 构建 {project_id} 开始");
    // 结果与退出码在 run_cli_build 内部处理（进程在 Tauri setup 内退出）
    let code = offline_preops_tool_lib::run_cli_build(project_id, Some(log_path.to_string()));
    std::process::exit(code);
}
