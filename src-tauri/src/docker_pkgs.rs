use crate::error::{AppError, AppResult};
use crate::net::{http_get_with_settings, proxy_url};
use crate::store;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

/// Docker 离线安装包库（M2）：
/// - 库布局（扁平目录，每包一个）：<dockerPkgRoot>/pkg-<arch>-<kind>-<version>/
///     meta.json + packages/*.rpm|*.deb|docker-*.tgz
/// - compose 插件独立存放：<dockerPkgRoot>/compose/<arch>/docker-compose
/// - 导入：本地 rpm/deb/tgz/docker-compose 文件（文件名自动解析 arch/version）
/// - 在线下载：官方静态包（download.docker.com）+ compose 插件（GitHub）
/// - 构建时按 服务器arch + docker版本 + OS包系 匹配打进 docker-offline/

// ==================== 数据结构 ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerPkgMeta {
    pub arch: String,
    /// rpm | deb | static
    pub kind: String,
    pub docker_version: String,
    pub files: Vec<String>,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerPkgEntry {
    pub id: String,
    pub arch: String,
    pub kind: String,
    pub docker_version: String,
    pub dir: String,
    pub size_bytes: u64,
    pub files: Vec<String>,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposePluginStatus {
    pub arch: String,
    pub installed: bool,
    pub file: String,
    pub size_bytes: u64,
}

fn pkg_root(app: &AppHandle) -> AppResult<PathBuf> {
    let s = store::effective_storage(app)?;
    let p = PathBuf::from(s.docker_pkg_root);
    fs::create_dir_all(&p)?;
    Ok(p)
}

// ==================== 文件名解析 ====================

/// 从 rpm/deb/tgz/compose 文件名解析 (kind, arch, version)
/// 例：docker-ce-27.5.1-1.el8.x86_64.rpm / docker-ce_27.5.1-1~debian12_arm64.deb /
///     docker-27.5.1.tgz / docker-compose-linux-x86_64
pub fn parse_pkg_filename(name: &str) -> Option<(String, String, String)> {
    let lower = name.to_lowercase();
    if lower.starts_with("docker-compose") {
        let arch = if lower.contains("x86_64") || lower.contains("amd64") {
            "amd64"
        } else if lower.contains("aarch64") || lower.contains("arm64") {
            "arm64"
        } else {
            return None
        };
        return Some(("compose".into(), arch.into(), String::new()))
    }
    if lower.starts_with("docker-ce-") && lower.ends_with(".rpm") {
        // docker-ce-27.5.1-1.el8.x86_64.rpm
        let arch = if lower.contains(".x86_64.rpm") {
            "amd64"
        } else if lower.contains(".aarch64.rpm") {
            "arm64"
        } else {
            return None
        };
        let version = name
            .strip_prefix("docker-ce-")?
            .split('-')
            .next()?
            .to_string();
        return Some(("rpm".into(), arch.into(), version))
    }
    if (lower.starts_with("docker-ce_") || lower.starts_with("docker-ce ")) && lower.ends_with(".deb") {
        let arch = if lower.contains("_amd64.deb") || lower.contains("amd64.deb") {
            "amd64"
        } else if lower.contains("_arm64.deb") || lower.contains("_armhf.deb") {
            "arm64"
        } else {
            return None
        };
        let version = name
            .strip_prefix("docker-ce_")?
            .split(['-', '_', '~'])
            .next()?
            .to_string();
        return Some(("deb".into(), arch.into(), version))
    }
    if lower.starts_with("docker-") && lower.ends_with(".tgz") {
        // 官方静态包不带 arch（按下载目录区分），arch 由调用方指定
        let version = name
            .strip_prefix("docker-")?
            .strip_suffix(".tgz")?
            .to_string();
        if version.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Some(("static".into(), String::new(), version))
        }
        return None
    }
    // containerd.io 等依赖包：归入 rpm/deb 但无版本信息（作为附属文件由导入流程归组）
    None
}

/// OS 族 → 包系偏好（rpm 系优先静态包兜底）
pub fn os_pkg_class(os_family: &str) -> &'static str {
    match os_family {
        "ubuntu" | "debian" | "deepin" => "deb",
        // 麒麟/UOS/openEuler/CentOS/RHEL/Rocky/中标 等 el 系
        _ => "rpm",
    }
}

// ==================== 库操作 ====================

#[tauri::command]
pub fn list_docker_pkgs(app: AppHandle) -> AppResult<Vec<DockerPkgEntry>> {
    let root = pkg_root(&app)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(&root)?.flatten() {
        let dir = entry.path();
        if !dir.is_dir() || !dir.file_name().map(|n| n.to_string_lossy().starts_with("pkg-")).unwrap_or(false) {
            continue;
        }
        let meta_path = dir.join("meta.json");
        let Ok(raw) = fs::read_to_string(&meta_path) else { continue };
        let Ok(meta) = serde_json::from_str::<DockerPkgMeta>(&raw) else { continue };
        let size = dir_size(&dir.join("packages"));
        out.push(DockerPkgEntry {
            id: dir.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            arch: meta.arch,
            kind: meta.kind,
            docker_version: meta.docker_version,
            dir: dir.to_string_lossy().into_owned(),
            size_bytes: size,
            files: meta.files,
            imported_at: meta.imported_at,
        });
    }
    out.sort_by(|a, b| (b.arch.clone(), b.docker_version.clone()).cmp(&(a.arch.clone(), a.docker_version.clone())));
    Ok(out)
}

#[tauri::command]
pub fn list_compose_plugins(app: AppHandle) -> AppResult<Vec<ComposePluginStatus>> {
    let root = pkg_root(&app)?;
    let mut out = Vec::new();
    let compose_dir = root.join("compose");
    if compose_dir.is_dir() {
        for arch_dir in fs::read_dir(&compose_dir)?.flatten() {
            let f = arch_dir.path().join("docker-compose");
            if f.is_file() {
                let arch_name = arch_dir.file_name().to_string_lossy().into_owned();
                out.push(ComposePluginStatus {
                    arch: arch_name,
                    installed: true,
                    file: f.to_string_lossy().into_owned(),
                    size_bytes: f.metadata().map(|m| m.len()).unwrap_or(0),
                });
            }
        }
    }
    for arch in ["amd64", "arm64", "loongarch64"] {
        if !out.iter().any(|c| c.arch == arch) {
            out.push(ComposePluginStatus {
                arch: arch.into(),
                installed: false,
                file: String::new(),
                size_bytes: 0,
            });
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn delete_docker_pkg(app: AppHandle, dir: String) -> AppResult<()> {
    let p = PathBuf::from(&dir);
    let root = pkg_root(&app)?;
    if !p.starts_with(&root) || !p.is_dir() {
        return Err(AppError::Invalid("非法的安装包目录".into()));
    }
    fs::remove_dir_all(&p)?;
    Ok(())
}

/// 导入本地安装包文件（多选）：
/// - rpm/deb：按文件名解析 arch/version 归组（解析失败的作为附件归入 default 组）
/// - docker-*.tgz：静态包，arch 由参数指定
/// - docker-compose-*：compose 插件
/// 返回导入结果摘要
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub created_dirs: Vec<String>,
    pub compose_installed: Vec<String>,
    pub skipped: Vec<String>,
}

#[tauri::command]
pub async fn import_docker_pkgs(
    app: AppHandle,
    paths: Vec<String>,
    default_arch: String,
) -> AppResult<ImportResult> {
    tauri::async_runtime::spawn_blocking(move || import_docker_pkgs_sync(&app, &paths, &default_arch))
        .await
        .map_err(|e| AppError::Io(format!("导入任务异常: {e}")))?
}

fn import_docker_pkgs_sync(app: &AppHandle, paths: &[String], default_arch: &str) -> AppResult<ImportResult> {
    let root = pkg_root(app)?;
    let mut result = ImportResult {
        created_dirs: vec![],
        compose_installed: vec![],
        skipped: vec![],
    };

    for path_str in paths {
        let src = PathBuf::from(path_str);
        if !src.is_file() {
            result.skipped.push(format!("{}（不是文件）", path_str));
            continue;
        }
        let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match parse_pkg_filename(&name) {
            Some((kind, arch, _version)) if kind == "compose" => {
                let dst = root.join("compose").join(&arch);
                fs::create_dir_all(&dst)?;
                fs::copy(&src, dst.join("docker-compose"))?;
                result.compose_installed.push(format!("{name} → {arch}"));
            }
            Some((kind, arch, version)) => {
                let arch = if arch.is_empty() { default_arch.to_string() } else { arch };
                let version = if version.is_empty() { "unknown".into() } else { version };
                let id = format!("pkg-{arch}-{kind}-{version}");
                let dir = root.join(&id);
                let pkgs_dir = dir.join("packages");
                fs::create_dir_all(&pkgs_dir)?;
                fs::copy(&src, pkgs_dir.join(&name))?;
                // 更新 meta（合并同组文件）
                let mut meta = fs::read_to_string(dir.join("meta.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str::<DockerPkgMeta>(&s).ok())
                    .unwrap_or(DockerPkgMeta {
                        arch: arch.clone(),
                        kind: kind.clone(),
                        docker_version: version.clone(),
                        files: vec![],
                        imported_at: store::now_rfc3339(),
                    });
                if !meta.files.contains(&name) {
                    meta.files.push(name.clone());
                }
                fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;
                if !result.created_dirs.contains(&id) {
                    result.created_dirs.push(id);
                }
            }
            None => {
                // containerd.io 等附属包：归入 default_arch 的 rpm/deb 附属组
                let kind = if name.ends_with(".rpm") { "rpm" } else if name.ends_with(".deb") { "deb" } else {
                    result.skipped.push(format!("{name}（无法识别）"));
                    continue;
                };
                let id = format!("pkg-{}-{}-deps", default_arch, kind);
                let dir = root.join(&id);
                let pkgs_dir = dir.join("packages");
                fs::create_dir_all(&pkgs_dir)?;
                fs::copy(&src, pkgs_dir.join(&name))?;
                let mut meta = fs::read_to_string(dir.join("meta.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str::<DockerPkgMeta>(&s).ok())
                    .unwrap_or(DockerPkgMeta {
                        arch: default_arch.into(),
                        kind: kind.into(),
                        docker_version: "deps".into(),
                        files: vec![],
                        imported_at: store::now_rfc3339(),
                    });
                if !meta.files.contains(&name) {
                    meta.files.push(name);
                }
                fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;
                if !result.created_dirs.contains(&id) {
                    result.created_dirs.push(id);
                }
            }
        }
    }
    Ok(result)
}

// ==================== 在线下载 ====================

fn emit(app: &AppHandle, step: &str, detail: &str) {
    let _ = app.emit("docker-pkg-download", serde_json::json!({ "step": step, "detail": detail }));
}

/// 在线下载 Docker 官方静态包（amd64/arm64，走代理）
#[tauri::command]
pub async fn download_docker_static(app: AppHandle, arch: String, version: String) -> AppResult<DockerPkgEntry> {
    tauri::async_runtime::spawn_blocking(move || download_docker_static_sync(&app, &arch, &version))
        .await
        .map_err(|e| AppError::Io(format!("下载任务异常: {e}")))?
}

fn download_docker_static_sync(app: &AppHandle, arch: &str, version: &str) -> AppResult<DockerPkgEntry> {
    let settings = store::load_settings(app)?;
    let dir = match arch {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        other => {
            return Err(AppError::Invalid(format!(
                "架构 {other} 无官方静态包，请从信创源下载后导入"
            )))
        }
    };
    let url = format!("https://download.docker.com/linux/static/stable/{dir}/docker-{version}.tgz");
    emit(app, "download", &format!("下载 {url}"));
    let tmp = std::env::temp_dir().join(format!("docker-{arch}-{version}.tgz.downloading"));
    let mut curl_args: Vec<String> = vec!["-sSL".into(), "--max-time".into(), "600".into()];
    if let Some(p) = proxy_url(&settings) {
        curl_args.push("-x".into());
        curl_args.push(p);
    }
    curl_args.push("-o".into());
    curl_args.push(tmp.to_string_lossy().into_owned());
    curl_args.push(url.clone());
    let out = std::process::Command::new("curl")
        .args(&curl_args)
        .output()
        .map_err(|e| AppError::Io(format!("curl 下载失败: {e}")))?;
    if !out.status.success() {
        return Err(AppError::Io(format!(
            "下载失败: {}（检查网络/代理）",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    // tgz 内为 docker/ 目录，保持原样（install-docker.sh 会解压处理）
    import_docker_pkgs_sync(
        app,
        &[tmp.to_string_lossy().into_owned()],
        arch,
    )?;
    let _ = fs::remove_file(&tmp);
    let id = format!("pkg-{arch}-static-{version}");
    let dir_path = pkg_root(app)?.join(&id);
    let meta = fs::read_to_string(dir_path.join("meta.json"))?;
    let meta: DockerPkgMeta = serde_json::from_str(&meta)?;
    Ok(DockerPkgEntry {
        id,
        arch: meta.arch,
        kind: meta.kind,
        docker_version: meta.docker_version,
        dir: dir_path.to_string_lossy().into_owned(),
        size_bytes: dir_size(&dir_path.join("packages")),
        files: meta.files,
        imported_at: meta.imported_at,
    })
}

/// 在线下载 docker compose v2 插件（GitHub Releases，按 arch，走代理）
#[tauri::command]
pub async fn download_compose_plugin(app: AppHandle, arch: String) -> AppResult<ComposePluginStatus> {
    tauri::async_runtime::spawn_blocking(move || download_compose_plugin_sync(&app, &arch))
        .await
        .map_err(|e| AppError::Io(format!("下载任务异常: {e}")))?
}

fn download_compose_plugin_sync(app: &AppHandle, arch: &str) -> AppResult<ComposePluginStatus> {
    let settings = store::load_settings(app)?;
    let asset_arch = match arch {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        other => {
            return Err(AppError::Invalid(format!(
                "架构 {other} 无官方 compose 插件发行，请手动导入"
            )))
        }
    };
    emit(app, "version", "获取 compose 最新版本号…");
    let rel = http_get_with_settings(
        &settings,
        "https://api.github.com/repos/docker/compose/releases/latest",
        20,
    )?;
    let tag: String = serde_json::from_str::<serde_json::Value>(&rel)?
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim_start_matches('v')
        .to_string();
    if tag.is_empty() {
        return Err(AppError::Io("未解析到 compose 版本号".into()));
    }
    let url = format!(
        "https://github.com/docker/compose/releases/download/v{tag}/docker-compose-linux-{asset_arch}"
    );
    emit(app, "download", &format!("下载 {url}"));
    let root = pkg_root(app)?;
    let dst_dir = root.join("compose").join(arch);
    fs::create_dir_all(&dst_dir)?;
    let tmp = dst_dir.join(".downloading");
    let mut curl_args: Vec<String> = vec!["-sSL".into(), "--max-time".into(), "300".into()];
    if let Some(p) = proxy_url(&settings) {
        curl_args.push("-x".into());
        curl_args.push(p);
    }
    curl_args.push("-o".into());
    curl_args.push(tmp.to_string_lossy().into_owned());
    curl_args.push(url);
    let out = std::process::Command::new("curl")
        .args(&curl_args)
        .output()
        .map_err(|e| AppError::Io(format!("curl 下载失败: {e}")))?;
    if !out.status.success() {
        return Err(AppError::Io(format!(
            "下载失败: {}（检查网络/代理）",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    fs::rename(&tmp, dst_dir.join("docker-compose"))?;
    let file = dst_dir.join("docker-compose");
    Ok(ComposePluginStatus {
        arch: arch.into(),
        installed: true,
        file: file.to_string_lossy().into_owned(),
        size_bytes: file.metadata().map(|m| m.len()).unwrap_or(0),
    })
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                total += dir_size(&e.path());
            } else if let Ok(md) = e.metadata() {
                total += md.len();
            }
        }
    }
    total
}

// ==================== 构建期匹配（builder 调用） ====================

/// 按服务器匹配安装包目录：arch 精确 + docker 版本精确 + 包系（rpm/deb 按偏好，static 兜底）
pub fn pick_pkg_dir(app: &AppHandle, arch: &str, docker_version: &str, os_family: &str) -> Option<PathBuf> {
    let root = pkg_root(app).ok()?;
    let preferred = os_pkg_class(os_family);
    let mut fallback: Option<PathBuf> = None;
    for entry in fs::read_dir(&root).ok()?.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let id = dir.file_name()?.to_string_lossy().into_owned();
        if !id.starts_with("pkg-") {
            continue;
        }
        // id 形如 pkg-<arch>-<kind>-<version>[-deps]
        let parts: Vec<&str> = id.trim_start_matches("pkg-").splitn(3, '-').collect();
        if parts.len() < 3 {
            continue;
        }
        let (p_arch, p_kind, p_version) = (parts[0], parts[1], parts[2]);
        if p_arch != arch || p_version != docker_version || p_kind == "deps" {
            continue;
        }
        if p_kind == preferred {
            return Some(dir); // 精确命中
        }
        if p_kind == "static" {
            fallback = Some(dir);
        }
    }
    fallback
}

/// compose 插件路径（按 arch）
pub fn compose_plugin_path(app: &AppHandle, arch: &str) -> Option<PathBuf> {
    let root = pkg_root(app).ok()?;
    let f = root.join("compose").join(arch).join("docker-compose");
    f.is_file().then_some(f)
}
