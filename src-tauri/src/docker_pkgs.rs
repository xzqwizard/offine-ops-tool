use crate::error::{AppError, AppResult};
use crate::net::http_get_with_settings;
use crate::store;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

/// Docker 离线安装包库（M2）：
/// - 库布局（扁平目录，每包一个）：<dockerPkgRoot>/pkg-<arch>-<kind>-<version>/
///   meta.json + packages/*.rpm|*.deb|docker-*.tgz
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
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub imported_at: String,
    #[serde(default)]
    pub os_family: String,
    #[serde(default)]
    pub os_version: String,
    #[serde(default)]
    pub sha256: std::collections::BTreeMap<String, String>,
    /// Exact engine compatibility of native dependency groups.
    #[serde(default)]
    pub engine_version: String,
    /// filename -> engine | cli | containerd | dependency
    #[serde(default)]
    pub components: std::collections::BTreeMap<String, String>,
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
    pub os_family: String,
    pub os_version: String,
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

/// 从 rpm/deb/tgz/compose 文件名解析 (kind, arch, version, is_aux)
/// 例：docker-ce-27.5.1-1.el8.x86_64.rpm / docker-ce-cli-27.5.1-1.el8.x86_64.rpm（附件）/
///     docker-27.5.1.tgz / docker-compose-linux-x86_64
/// is_aux=true 表示 docker-ce-cli/buildx/containerd 等依赖包：归入同架构 deps 组
/// （与主包版本无关），构建时与命中主包合并拷贝
pub fn parse_pkg_filename(name: &str) -> Option<(String, String, String, bool)> {
    let lower = name.to_lowercase();
    // 注意顺序：docker-compose-plugin-*.rpm 是包管理器插件包（aux），
    // 必须先走 rpm/deb 分支；独立二进制 compose 形如 docker-compose-linux-x86_64（无扩展名）
    if lower.starts_with("docker-compose") && !lower.ends_with(".rpm") && !lower.ends_with(".deb") {
        let arch = if lower.contains("x86_64") || lower.contains("amd64") {
            "amd64"
        } else if lower.contains("aarch64") || lower.contains("arm64") {
            "arm64"
        } else if lower.contains("loongarch64") {
            "loongarch64"
        } else if lower.contains("mips64el") {
            "mips64el"
        } else if lower.contains("sw64") {
            "sw64"
        } else {
            return None;
        };
        return Some(("compose".into(), arch.into(), String::new(), false));
    }
    // 附件包：docker-ce-cli / docker-buildx-plugin / docker-scan-plugin / containerd.io / fuse-overlayfs 等
    let aux_prefixes = [
        "docker-ce-cli-",
        "docker-ce-cli_",
        "docker-buildx-plugin-",
        "docker-buildx-plugin_",
        "docker-scan-plugin-",
        "docker-compose-plugin-",
        "docker-compose-plugin_",
        "containerd.io-",
        "containerd.io_",
        "fuse-overlayfs-",
    ];
    if lower.ends_with(".rpm") || lower.ends_with(".deb") {
        let arch = if lower.contains(".x86_64.rpm") || lower.contains("amd64.deb") {
            "amd64"
        } else if lower.contains(".aarch64.rpm") || lower.contains("arm64.deb") {
            "arm64"
        } else if lower.contains("loongarch64") {
            "loongarch64"
        } else if lower.contains("mips64el") {
            "mips64el"
        } else if lower.contains("sw64") {
            "sw64"
        } else {
            return None;
        };
        for pfx in aux_prefixes {
            if lower.starts_with(pfx) {
                return Some(("aux".into(), arch.into(), String::new(), true));
            }
        }
        if lower.starts_with("docker-ce-") {
            let version = name
                .strip_prefix("docker-ce-")?
                .split('-')
                .next()?
                .to_string();
            if version
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                return Some(("rpm".into(), arch.into(), version, false));
            }
            return None;
        }
        if lower.starts_with("docker-ce_") {
            let version = name
                .strip_prefix("docker-ce_")?
                .split(['-', '_', '~'])
                .next()?
                .to_string();
            if version
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                return Some(("deb".into(), arch.into(), version, false));
            }
            return None;
        }
        // 其它 rpm/deb（如信创源 docker-engine-x.y.z）尝试提取首位为数字的版本段
        let stem = name
            .trim_end_matches(|c: char| c != '.' && !c.is_ascii_digit())
            .to_string();
        let _ = stem;
        return None;
    }
    if lower.starts_with("docker-") && lower.ends_with(".tgz") {
        // 官方静态包不带 arch（按下载目录区分），arch 由调用方指定
        let version = name
            .strip_prefix("docker-")?
            .strip_suffix(".tgz")?
            .to_string();
        if version.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Some(("static".into(), String::new(), version, false));
        }
        return None;
    }
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
        if !dir.is_dir()
            || !dir
                .file_name()
                .map(|n| n.to_string_lossy().starts_with("pkg-"))
                .unwrap_or(false)
        {
            continue;
        }
        let meta_path = dir.join("meta.json");
        let Ok(raw) = fs::read_to_string(&meta_path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<DockerPkgMeta>(&raw) else {
            continue;
        };
        let size = dir_size(&dir.join("packages"));
        out.push(DockerPkgEntry {
            id: dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            arch: meta.arch,
            kind: meta.kind,
            docker_version: meta.docker_version,
            dir: dir.to_string_lossy().into_owned(),
            size_bytes: size,
            files: meta.files,
            imported_at: meta.imported_at,
            os_family: meta.os_family,
            os_version: meta.os_version,
        });
    }
    out.sort_by(|a, b| {
        (b.arch.clone(), b.docker_version.clone()).cmp(&(a.arch.clone(), a.docker_version.clone()))
    });
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
    // 路径穿越与根目录防护：canonicalize 规范化 .. 后，
    // 必须是库根的直接子目录（不能删库根本身，也不能越界）
    let root = pkg_root(&app)?;
    let p_canon = p
        .canonicalize()
        .map_err(|_| AppError::Invalid(format!("目录不存在: {dir}")))?;
    let root_canon = root.canonicalize()?;
    if p_canon.parent() != Some(root_canon.as_path()) {
        return Err(AppError::Invalid("仅允许删除安装包库内的包组目录".into()));
    }
    let _lock = crate::io_util::lock(&root.join(".packages.lock"))?;
    fs::remove_dir_all(&p_canon)?;
    crate::app_ops::__audit(&app, "delete_docker_pkg", &dir);
    Ok(())
}

/// 导入本地安装包文件（多选）：
/// - rpm/deb：按文件名解析 arch/version 归组（解析失败的作为附件归入 default 组）
/// - docker-*.tgz：静态包，arch 由参数指定
/// - docker-compose-*：compose 插件
///
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
    tauri::async_runtime::spawn_blocking(move || {
        import_docker_pkgs_sync(&app, &paths, &default_arch)
    })
    .await
    .map_err(|e| AppError::Io(format!("导入任务异常: {e}")))?
}

fn import_docker_pkgs_sync(
    app: &AppHandle,
    paths: &[String],
    default_arch: &str,
) -> AppResult<ImportResult> {
    let root = pkg_root(app)?;
    let _lock = crate::io_util::lock(&root.join(".packages.lock"))?;
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
        let name = src
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let sidecar = src.with_file_name(format!("{name}.pkg.json"));
        let declared: Option<DockerPkgMeta> = if sidecar.exists() {
            Some(serde_json::from_slice(&fs::read(&sidecar)?)?)
        } else {
            None
        };
        let parsed = declared
            .as_ref()
            .map(|m| {
                (
                    m.kind.clone(),
                    m.arch.clone(),
                    m.docker_version.clone(),
                    m.docker_version == "deps",
                )
            })
            .or_else(|| parse_pkg_filename(&name));
        match parsed {
            Some((kind, arch, _v, _aux)) if kind == "compose" => {
                verify_elf(&src, &arch)?;
                let dst = root.join("compose").join(&arch);
                fs::create_dir_all(&dst)?;
                crate::io_util::atomic_write(&dst.join("docker-compose"), &fs::read(&src)?)?;
                result.compose_installed.push(format!("{name} → {arch}"));
            }
            Some((kind, arch, version, is_aux)) => {
                let arch = if arch.is_empty() {
                    default_arch.to_string()
                } else {
                    arch
                };
                let kind = if kind == "aux" {
                    // 附件包归入按架构的依赖组（rpm/deb 由扩展名决定），与主包版本无关
                    if name.ends_with(".deb") {
                        "deb"
                    } else {
                        "rpm"
                    }
                } else {
                    kind.as_str()
                };
                let version = if is_aux || version.is_empty() {
                    "deps".into()
                } else {
                    version
                };
                if !["amd64", "arm64", "loongarch64", "mips64el", "sw64"].contains(&arch.as_str())
                    || !["static", "rpm", "deb"].contains(&kind)
                {
                    return Err(AppError::Invalid("安装包架构或类型无效".into()));
                }
                if version.is_empty()
                    || version != "deps" && !version.chars().all(|c| c.is_ascii_digit() || c == '.')
                {
                    return Err(AppError::Invalid("安装包版本无效".into()));
                }
                if kind == "static" {
                    verify_static(&src, &arch)?;
                } else {
                    verify_native(&src, kind)?;
                }
                let sha = crate::io_util::sha256_file(&src)?;
                if kind != "static"
                    && declared.as_ref().and_then(|m| m.sha256.get(&name)) != Some(&sha)
                {
                    result
                        .skipped
                        .push(format!("{name}：侧车文件需声明并匹配 sha256[{name}]"));
                    continue;
                }
                let os_family = declared
                    .as_ref()
                    .map(|m| m.os_family.clone())
                    .unwrap_or_default();
                let os_version = declared
                    .as_ref()
                    .map(|m| m.os_version.clone())
                    .unwrap_or_default();
                if kind != "static" && (os_family.is_empty() || os_version.is_empty()) {
                    result.skipped.push(format!("{name}：rpm/deb 需同名 .pkg.json 声明 osFamily/osVersion，避免跨发行版混装"));
                    continue;
                }
                let engine_version = declared
                    .as_ref()
                    .map(|m| m.engine_version.clone())
                    .unwrap_or_default();
                let component = declared
                    .as_ref()
                    .and_then(|m| m.components.get(&name))
                    .cloned()
                    .unwrap_or_default();
                if kind != "static"
                    && (engine_version.is_empty()
                        || !engine_version
                            .chars()
                            .all(|c| c.is_ascii_digit() || c == '.')
                        || !["engine", "cli", "containerd", "dependency"]
                            .contains(&component.as_str())
                        || (version != "deps" && version != engine_version))
                {
                    result.skipped.push(format!(
                        "{name}：需声明兼容 engineVersion 以及 components[{name}] 的组件角色"
                    ));
                    continue;
                }
                let id = if is_aux {
                    format!("pkg-{arch}-{kind}-deps")
                } else {
                    format!("pkg-{arch}-{kind}-{version}")
                };
                let id = if kind == "static" {
                    id
                } else {
                    format!(
                        "{id}-{}",
                        &crate::io_util::hash(&format!(
                            "{os_family}/{os_version}/{engine_version}"
                        ))[..12]
                    )
                };
                let dir = root.join(&id);
                let pkgs_dir = dir.join("packages");
                fs::create_dir_all(&pkgs_dir)?;
                crate::io_util::copy_atomic(&src, &pkgs_dir.join(&name))?;
                // 更新 meta（合并同组文件）
                let mut meta = fs::read_to_string(dir.join("meta.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str::<DockerPkgMeta>(&s).ok())
                    .unwrap_or(DockerPkgMeta {
                        arch: arch.clone(),
                        kind: kind.to_string(),
                        docker_version: version.clone(),
                        files: vec![],
                        imported_at: store::now_rfc3339(),
                        os_family,
                        os_version,
                        sha256: std::collections::BTreeMap::new(),
                        engine_version,
                        components: std::collections::BTreeMap::new(),
                    });
                meta.sha256.insert(name.clone(), sha);
                if kind != "static" {
                    meta.components.insert(name.clone(), component);
                }
                if !meta.files.contains(&name) {
                    meta.files.push(name.clone());
                }
                crate::io_util::atomic_write(
                    &dir.join("meta.json"),
                    serde_json::to_string_pretty(&meta)?.as_bytes(),
                )?;
                if !result.created_dirs.contains(&id) {
                    result.created_dirs.push(id);
                }
            }
            None => {
                result.skipped.push(format!("{name}（无法识别，支持 docker-ce*/docker-ce-cli*/containerd.io*/docker-*.tgz/docker-compose-*）"));
            }
        }
    }
    crate::app_ops::__audit(
        app,
        "import_docker_pkgs",
        &format!(
            "{} 个组，{} 个插件，{} 个跳过",
            result.created_dirs.len(),
            result.compose_installed.len(),
            result.skipped.len()
        ),
    );
    Ok(result)
}

// ==================== 在线下载 ====================

fn emit(app: &AppHandle, step: &str, detail: &str) {
    let _ = app.emit(
        "docker-pkg-download",
        serde_json::json!({ "step": step, "detail": detail }),
    );
}

/// 在线下载 Docker 官方静态包（amd64/arm64，走代理）
#[tauri::command]
pub async fn download_docker_static(
    app: AppHandle,
    arch: String,
    version: String,
) -> AppResult<DockerPkgEntry> {
    tauri::async_runtime::spawn_blocking(move || download_docker_static_sync(&app, &arch, &version))
        .await
        .map_err(|e| AppError::Io(format!("下载任务异常: {e}")))?
}

fn download_docker_static_sync(
    app: &AppHandle,
    arch: &str,
    version: &str,
) -> AppResult<DockerPkgEntry> {
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
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(AppError::Invalid("Docker 版本无效".into()));
    }
    let task_dir =
        std::env::temp_dir().join(format!("preops-docker-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&task_dir)?;
    let _cleanup = crate::io_util::Cleanup(task_dir.clone());
    let tmp = task_dir.join(format!("docker-{version}.tgz"));
    crate::net::download(&settings, &url, &tmp, 600)?;
    // tgz 内为 docker/ 目录，保持原样（install-docker.sh 会解压处理）
    let imported = import_docker_pkgs_sync(app, &[tmp.to_string_lossy().into_owned()], arch)?;
    let id = format!("pkg-{arch}-static-{version}");
    if !imported.created_dirs.contains(&id) {
        return Err(AppError::Invalid(format!(
            "下载未成功导入：{:?}",
            imported.skipped
        )));
    }
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
        os_family: meta.os_family,
        os_version: meta.os_version,
    })
}

/// 在线下载 docker compose v2 插件（GitHub Releases，按 arch，走代理）
#[tauri::command]
pub async fn download_compose_plugin(
    app: AppHandle,
    arch: String,
) -> AppResult<ComposePluginStatus> {
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
    let tmp = dst_dir.join(format!(
        ".{}.downloading",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _cleanup = crate::io_util::Cleanup(tmp.clone());
    let _lock = crate::io_util::lock(&dst_dir.join(".compose.lock"))?;
    crate::net::download(&settings, &url, &tmp, 300)?;
    let checksum = http_get_with_settings(&settings, &format!("{url}.sha256"), 30)?;
    let expected = checksum.split_whitespace().next().unwrap_or("");
    if expected != crate::io_util::sha256_file(&tmp)? {
        return Err(AppError::Invalid("Compose 下载 SHA256 不匹配".into()));
    }
    verify_elf(&tmp, arch)?;
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

/// 按服务器匹配安装包目录（主组 + 同架构依赖组）：
/// arch 精确 + docker 版本精确 + 包系（rpm/deb 按偏好，static 兜底）；
/// 返回的所有组的 packages/ 内容会被合并拷贝（主包 + cli/containerd 等依赖）。
pub fn pick_pkg_dirs(
    app: &AppHandle,
    arch: &str,
    version: &str,
    os_family: &str,
    os_version: &str,
) -> AppResult<Vec<PathBuf>> {
    let root = pkg_root(app)?;
    pick_pkg_dirs_at(&root, arch, version, os_family, os_version)
}
pub fn pick_pkg_dirs_at(
    root: &Path,
    arch: &str,
    version: &str,
    os_family: &str,
    os_version: &str,
) -> AppResult<Vec<PathBuf>> {
    let mut mains = vec![];
    let mut deps = vec![];
    let mut statics = vec![];
    for e in fs::read_dir(root)? {
        let dir = e?.path();
        let Ok(raw) = fs::read(dir.join("meta.json")) else {
            continue;
        };
        let m: DockerPkgMeta = serde_json::from_slice(&raw)?;
        if m.arch != arch {
            continue;
        }
        if m.docker_version != version && m.docker_version != "deps" {
            continue;
        }
        if m.kind != "static"
            && (m.os_family != os_family
                || m.os_version != os_version
                || m.engine_version != version)
        {
            continue;
        }
        validate_group(&dir, &m)?;
        if m.kind == "static" && m.docker_version == version {
            statics.push(dir.clone());
        } else if m.docker_version == version {
            mains.push((dir, m.kind));
        } else {
            deps.push((dir, m.kind));
        }
    }
    mains.sort();
    deps.sort();
    statics.sort();
    for (main, kind) in mains {
        let matching: Vec<_> = deps
            .iter()
            .filter(|(_, k)| *k == kind)
            .map(|(d, _)| d.clone())
            .collect();
        let mut group = vec![main];
        group.extend(matching);
        let mut roles = std::collections::HashSet::new();
        for dir in &group {
            let m: DockerPkgMeta = serde_json::from_slice(&fs::read(dir.join("meta.json"))?)?;
            roles.extend(m.components.values().cloned());
        }
        if ["engine", "cli", "containerd"]
            .iter()
            .all(|role| roles.contains(*role))
        {
            return Ok(group);
        }
    }
    if let Some(s) = statics.first() {
        return Ok(vec![s.clone()]);
    }
    Err(AppError::Invalid(format!("没有完整、兼容 {os_family} {os_version}/{arch} Docker {version} 的安装材料（rpm/deb 需 OS 声明及依赖组）")))
}
fn validate_group(dir: &Path, m: &DockerPkgMeta) -> AppResult<()> {
    if m.files.is_empty() {
        return Err(AppError::Invalid("安装材料组为空".into()));
    }
    for name in &m.files {
        if Path::new(name)
            .file_name()
            .is_none_or(|n| n != name.as_str())
            || name.contains('\\')
        {
            return Err(AppError::Invalid("材料文件名无效".into()));
        }
        let p = dir.join("packages").join(name);
        let sha = crate::io_util::sha256_file(&p)?;
        if m.sha256.get(name) != Some(&sha) {
            return Err(AppError::Invalid(format!(
                "安装材料 {name} 缺校验元数据或校验失败，请重新导入"
            )));
        }
        if m.kind == "static" {
            verify_static(&p, &m.arch)?;
        } else {
            if !["rpm", "deb"].contains(&m.kind.as_str())
                || m.components.get(name).is_none_or(|role| {
                    !["engine", "cli", "containerd", "dependency"].contains(&role.as_str())
                })
            {
                return Err(AppError::Invalid("本机安装材料缺少有效组件角色".into()));
            }
            verify_native(&p, &m.kind)?;
        }
    }
    Ok(())
}

/// compose 插件路径（按 arch）
pub fn compose_plugin_path(app: &AppHandle, arch: &str) -> Option<PathBuf> {
    let root = pkg_root(app).ok()?;
    let f = root.join("compose").join(arch).join("docker-compose");
    verify_elf(&f, arch).is_ok().then_some(f)
}

pub fn verify_elf(path: &Path, arch: &str) -> AppResult<()> {
    use std::io::Read;
    let mut head = [0; 20];
    std::fs::File::open(path)?.read_exact(&mut head)?;
    verify_elf_header(&head, arch)
}
fn verify_elf_header(h: &[u8; 20], arch: &str) -> AppResult<()> {
    let machine = u16::from_le_bytes([h[18], h[19]]);
    let expected = match arch {
        "amd64" => 62,
        "arm64" => 183,
        "loongarch64" => 258,
        "mips64el" => 8,
        "sw64" => 0x9916,
        _ => 0,
    };
    if &h[..4] != b"\x7fELF" || h[4] != 2 || h[5] != 1 || machine != expected || expected == 0 {
        return Err(AppError::Invalid(format!(
            "不是 {arch} 的 64 位 Linux ELF 可执行文件"
        )));
    }
    Ok(())
}
pub fn verify_static(path: &Path, arch: &str) -> AppResult<()> {
    use std::io::Read;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(path)?));
    let mut found = std::collections::HashSet::new();
    for entry in tar.entries()? {
        let mut e = entry?;
        let p = e.path()?.into_owned();
        if p.components().any(|c| {
            !matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) || p.to_string_lossy().contains('\\')
            || !e.header().entry_type().is_file() && !e.header().entry_type().is_dir()
        {
            return Err(AppError::Invalid("静态包含不安全路径/链接".into()));
        }
        if e.header().entry_type().is_file() {
            let mut h = [0; 20];
            e.read_exact(&mut h)?;
            verify_elf_header(&h, arch)?;
            found.insert(p.to_string_lossy().into_owned());
        }
    }
    if [
        "docker",
        "dockerd",
        "containerd",
        "ctr",
        "runc",
        "docker-init",
        "docker-proxy",
        "containerd-shim-runc-v2",
    ]
    .iter()
    .any(|name| !found.contains(&format!("docker/{name}")))
    {
        return Err(AppError::Invalid(
            "静态包缺 Docker/containerd/runc 等必需二进制".into(),
        ));
    }
    Ok(())
}
fn verify_native(path: &Path, kind: &str) -> AppResult<()> {
    use std::io::Read;
    let mut h = [0; 8];
    std::fs::File::open(path)?.read_exact(&mut h)?;
    if kind == "rpm" && h[..4] != [0xed, 0xab, 0xee, 0xdb] || kind == "deb" && &h != b"!<arch>\n" {
        return Err(AppError::Invalid("安装包格式与声明不一致".into()));
    }
    Ok(())
}
