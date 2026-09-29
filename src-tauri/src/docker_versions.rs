use crate::error::{AppError, AppResult};
use crate::net::http_get_with_settings;
use crate::store;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// Docker 版本列表缓存：(arch, 抓取时间, 版本列表)
static CACHE: Mutex<Option<(String, Instant, Vec<String>)>> = Mutex::new(None);

const CACHE_TTL: Duration = Duration::from_secs(600);

/// 从 Docker 官方静态包目录抓取可用版本列表（倒序，自动应用代理）
/// https://download.docker.com/linux/static/stable/{x86_64|aarch64}/
#[tauri::command]
pub fn list_docker_versions(app: AppHandle, arch: String) -> AppResult<Vec<String>> {
    let dir = match arch.as_str() {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        other => {
            return Err(AppError::Invalid(format!(
                "架构 {other} 无 Docker 官方静态包（需从信创源获取安装包），请手动填写版本"
            )))
        }
    };

    {
        let cache = CACHE.lock().unwrap();
        if let Some((cached_arch, at, versions)) = cache.as_ref() {
            if cached_arch == &arch && at.elapsed() < CACHE_TTL {
                return Ok(versions.clone());
            }
        }
    }

    let settings = store::load_settings(&app)?;
    let url = format!("https://download.docker.com/linux/static/stable/{dir}/");
    let body = http_get_with_settings(&settings, &url, 20)?;
    let versions = parse_docker_versions(&body);
    if versions.is_empty() {
        return Err(AppError::Io(
            "官方源未解析到任何版本（页面结构可能已变化）".into(),
        ));
    }

    *CACHE.lock().unwrap() = Some((arch, Instant::now(), versions.clone()));
    Ok(versions)
}

/// 从目录索引 HTML 中解析 docker-<版本>.tgz 文件名，版本倒序去重
fn parse_docker_versions(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find("docker-") {
        rest = &rest[pos + "docker-".len()..];
        let end = match rest.find(".tgz") {
            Some(e) => e,
            None => break,
        };
        let candidate = &rest[..end];
        // 版本仅允许数字与点（排除 17.03.x-ce 等历史命名之外的非版本文件）
        if !candidate.is_empty() && candidate.chars().all(|c| c.is_ascii_digit() || c == '.') {
            out.push(candidate.to_string());
        }
    }
    out.sort_by(|a, b| version_key(b).cmp(&version_key(a)));
    out.dedup();
    out
}

fn version_key(v: &str) -> (u64, u64, u64) {
    let mut parts = [0u64; 3];
    for (i, p) in v.split('.').take(3).enumerate() {
        parts[i] = p.parse().unwrap_or(0);
    }
    (parts[0], parts[1], parts[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    // 历史"布局敏感崩溃"已定案：并非工具链 bug，而是 WebView2Loader.dll 未随
    // deps/examples 子目录的测试二进制落位导致进程启动失败（build.rs 已自动
    // 复制修复，详见 build.rs 注释）。parse 测试曾因排障被移除，M2 可补回。
    // 单元测试布局受限于本机工具链的历史问题（test 目标缺 common-controls v6
    // 清单），保持最小集；parse 测试位于 tests/images_tests.rs（build.rs 注入
    // 清单的显式 test 目标，可任意扩展）。
    #[test]
    fn version_key_numeric_compare() {
        assert!(version_key("27.5.1") > version_key("9.03.0"));
        assert_eq!(version_key("24.0"), (24, 0, 0));
    }
}
