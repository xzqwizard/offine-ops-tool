use crate::error::{AppError, AppResult};
use crate::models::AppSettings;
use crate::store;
use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// 统一的外网访问入口：所有 HTTP 请求走系统 curl（自动应用代理设置），
/// 所有外部子进程（crane 等）注入标准代理环境变量。

/// 解析生效的代理 URL，如 `http://127.0.0.1:7890`、`socks5h://user:pass@host:port`。
/// 未启用或配置不完整返回 None（直连）。
pub fn proxy_url(settings: &AppSettings) -> Option<String> {
    let p = settings.proxy.as_ref()?;
    if !p.enabled {
        return None
    }
    if p.host.trim().is_empty() || p.port == 0 {
        return None
    }
    // socks5h：DNS 解析也走代理（防污染）；http 代理同 URL 形式
    let scheme = match p.scheme.as_str() {
        "socks5" => "socks5h",
        _ => "http",
    };
    match (&p.username, &p.password) {
        (Some(u), Some(pw)) if !u.trim().is_empty() => Some(format!(
            "{scheme}://{}:{}@{}:{}",
            u.trim(),
            pw,
            p.host.trim(),
            p.port
        )),
        _ => Some(format!("{scheme}://{}:{}", p.host.trim(), p.port)),
    }
}

/// 供子进程（crane 等）使用的代理环境变量
pub fn proxy_envs(settings: &AppSettings) -> Vec<(String, String)> {
    let mut envs = Vec::new();
    if let Some(url) = proxy_url(settings) {
        for key in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "https_proxy", "http_proxy", "all_proxy"] {
            envs.push((key.to_string(), url.clone()));
        }
        let no_proxy = settings
            .proxy
            .as_ref()
            .and_then(|p| p.no_proxy.clone())
            .unwrap_or_else(|| "localhost,127.0.0.1,::1".into());
        envs.push(("NO_PROXY".into(), no_proxy.clone()));
        envs.push(("no_proxy".into(), no_proxy));
    }
    envs
}

/// HTTP GET（curl），自动应用代理与超时。返回响应体。

pub fn http_get_with_settings(settings: &AppSettings, url: &str, timeout_secs: u64) -> AppResult<String> {
    let mut args: Vec<String> = vec![
        "-sSL".into(),
        "--max-time".into(),
        timeout_secs.to_string(),
    ];
    if let Some(proxy) = proxy_url(settings) {
        args.push("-x".into());
        args.push(proxy);
        if let Some(p) = settings.proxy.as_ref() {
            if let Some(np) = p.no_proxy.as_ref() {
                if !np.trim().is_empty() {
                    args.push("--noproxy".into());
                    args.push(np.trim().into());
                }
            }
        }
    }
    args.push(url.into());
    let output = std::process::Command::new("curl")
        .args(&args)
        .output()
        .map_err(|e| AppError::Io(format!("调用系统 curl 失败: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut msg = stderr.trim().to_string();
        if msg.is_empty() {
            msg = format!("curl 退出码 {:?}", output.status.code());
        }
        return Err(AppError::Io(msg));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityResult {
    pub target: String,
    pub ok: bool,
    pub status: Option<u16>,
    pub latency_ms: u64,
    pub error: String,
}

fn probe(settings: &AppSettings, url: &str) -> ConnectivityResult {
    let start = Instant::now();
    // 用 -o /dev/null 只取状态码，避免拉取响应体
    let mut args: Vec<String> = vec![
        "-sS".into(),
        "-o".into(),
        "NUL".into(),
        "-w".into(),
        "%{http_code}".into(),
        "--max-time".into(),
        "15".into(),
    ];
    if let Some(proxy) = proxy_url(settings) {
        args.push("-x".into());
        args.push(proxy);
    }
    args.push(url.into());
    let out = std::process::Command::new("curl").args(&args).output();
    let latency = start.elapsed().as_millis() as u64;
    match out {
        Ok(o) if o.status.success() => {
            let code = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let status = code.parse::<u16>().ok();
            // 401 是 registry 正常的认证质询；2xx/3xx/401 都算可达
            let ok = matches!(status, Some(200..=399) | Some(401));
            ConnectivityResult {
                target: url.into(),
                ok,
                status,
                latency_ms: latency,
                error: if ok { String::new() } else { format!("HTTP {code}") },
            }
        }
        Ok(o) => ConnectivityResult {
            target: url.into(),
            ok: false,
            status: None,
            latency_ms: latency,
            error: String::from_utf8_lossy(&o.stderr).trim().to_string(),
        },
        Err(e) => ConnectivityResult {
            target: url.into(),
            ok: false,
            status: None,
            latency_ms: latency,
            error: e.to_string(),
        },
    }
}

/// 网络连通性测试报告
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestNetworkReport {
    /// 本次测试实际使用的代理（None=直连）
    pub proxy_used: Option<String>,
    pub results: Vec<ConnectivityResult>,
}

fn emit_test(app: &AppHandle, target: &str, state: &str, result: Option<&ConnectivityResult>) {
    let _ = app.emit(
        "net-test",
        serde_json::json!({ "target": target, "state": state, "result": result }),
    );
}

/// 网络连通性测试（后台线程执行，逐目标推送 net-test 事件）：
/// - 代理通道探针（启用代理时，验证代理本身可用；google.com 对节点分流最敏感）
/// - auth.docker.io（拉取 Docker Hub 镜像的 token 端点，最贴近实际拉取链路）
/// - registry-1.docker.io（Docker Hub Registry API）
/// - 国内镜像源 daocloud（直连兜底通道）
/// settings 参数：前端传入界面当前配置（未保存也可测）；缺省读已保存设置。
#[tauri::command]
pub async fn test_network(
    app: AppHandle,
    settings: Option<AppSettings>,
) -> AppResult<TestNetworkReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = match settings {
            Some(s) => s,
            None => store::load_settings(&app)?,
        };
        let proxy_used = proxy_url(&settings);
        let mut targets: Vec<&str> = Vec::new();
        if proxy_used.is_some() {
            targets.push("https://www.google.com/");
        }
        targets.push("https://auth.docker.io/token");
        targets.push("https://registry-1.docker.io/v2/");
        targets.push("https://docker.m.daocloud.io/v2/");
        let mut results = Vec::new();
        for url in targets {
            emit_test(&app, url, "running", None);
            let r = probe(&settings, url);
            emit_test(&app, url, "done", Some(&r));
            results.push(r);
        }
        Ok(TestNetworkReport { proxy_used, results })
    })
    .await
    .map_err(|e| AppError::Io(format!("测试任务异常: {e}")))?
}
