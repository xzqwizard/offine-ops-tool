use crate::engine::crane_command;
use crate::error::{AppError, AppResult};
use crate::store;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// 镜像查询/拉取/缓存（M1）：
/// - 引用解析与镜像源改写（docker.io 引用按设置中的镜像源顺序回退）
/// - crane 实现：ls(manifest tags) / manifest(架构矩阵) / config(单架构) / pull(docker-archive)
/// - 缓存：<镜像缓存根>/docker-archives/<规范化引用>_<os>_<arch>.tar（+ .meta.json）
/// - RepoTags 改写：mirror 拉取的 tar 内 RepoTags 为 mirror 地址，load 后与 compose
///   引用不一致 → 拉取完成后统一改写为用户原始引用

// ==================== 引用解析 ====================

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    pub registry: String,
    pub repo: String,
    pub tag: String,
    pub digest: Option<String>,
}

pub fn same_reference(a: &str, b: &str) -> bool {
    matches!((parse_reference(a), parse_reference(b)), (Ok(a), Ok(b)) if a == b)
}

impl ImageRef {
    /// 完整引用（不含 digest）
    pub fn repo_with_tag(&self) -> String {
        format!("{}:{}", self.repo, self.tag)
    }
}

pub fn parse_reference(input: &str) -> AppResult<ImageRef> {
    let s = input.trim();
    if s.is_empty() {
        return Err(AppError::Invalid("镜像引用不能为空".into()));
    }
    if s.starts_with('-')
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/@-".contains(c))
    {
        return Err(AppError::Invalid("镜像引用包含非法字符".into()));
    }
    let (base, digest) = match s.split_once('@') {
        Some((b, d)) => (b, Some(d.to_string())),
        None => (s, None),
    };
    if base.is_empty() || digest.as_ref().is_some_and(|d| !valid_digest(d)) {
        return Err(AppError::Invalid(
            "镜像 digest 必须为 sha256:64位十六进制".into(),
        ));
    }
    // tag：最后一个冒号且位于最后一个斜杠之后
    let mut tag = "latest".to_string();
    let mut name = base.to_string();
    if let Some(idx) = base.rfind(':') {
        if idx > base.rfind('/').unwrap_or(0) {
            name = base[..idx].to_string();
            tag = base[idx + 1..].to_string();
        }
    }
    // registry：首段含 . / : 或为 localhost
    let (registry, repo) = match name.split_once('/') {
        Some((first, rest))
            if first.contains('.') || first.contains(':') || first == "localhost" =>
        {
            (first.to_string(), rest.to_string())
        }
        _ => (
            "docker.io".to_string(),
            if name.contains('/') {
                name.clone()
            } else {
                format!("library/{name}")
            },
        ),
    };
    if repo.is_empty()
        || tag.is_empty()
        || repo
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(AppError::Invalid(format!("无法解析镜像引用: {input}")));
    }
    Ok(ImageRef {
        registry,
        repo,
        tag,
        digest,
    })
}

/// 按设置生成依次尝试的完整引用（仅 docker.io 引用做镜像源回退；私有/其他源原样）
fn candidates(
    settings: &crate::models::AppSettings,
    r: &ImageRef,
    registry: Option<&crate::models::RegistryConfig>,
) -> Vec<String> {
    let suffix = match &r.digest {
        Some(d) => format!("{}@{d}", r.repo),
        None => format!("{}:{}", r.repo, r.tag),
    };
    let mut hosts: Vec<String> = Vec::new();
    // 项目级私有仓库优先
    if let Some(reg) = registry {
        let host = crate::validation::normalize_host(&reg.url).unwrap_or_default();
        if !host.is_empty() && !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    if r.registry == "docker.io" {
        for m in &settings.registry_mirrors {
            let host = crate::validation::normalize_host(m).unwrap_or_default();
            if host.is_empty() {
                continue;
            }
            let host = if host == "docker.io" {
                "docker.io".into()
            } else {
                host
            };
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        if hosts.is_empty() {
            hosts.push("docker.io".into());
        }
        hosts.iter().map(|h| format!("{h}/{suffix}")).collect()
    } else {
        // 显式 registry 引用：私有仓库恰好就是它时由上面的 hosts 命中；否则原样
        let mut out: Vec<String> = hosts.iter().map(|h| format!("{h}/{suffix}")).collect();
        out.push(format!("{}/{suffix}", r.registry));
        out
    }
}

// Credential-specific isolated DOCKER_CONFIG; never writes the global Docker login file.
thread_local! { static PULL_TASK:std::cell::RefCell<String> = const {std::cell::RefCell::new(String::new())}; }
thread_local! { static AUTH_DIR: std::cell::RefCell<Option<PathBuf>> = const {std::cell::RefCell::new(None)}; }
struct AuthSession(PathBuf);
impl Drop for AuthSession {
    fn drop(&mut self) {
        AUTH_DIR.with(|p| *p.borrow_mut() = None);
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn registry_login(
    app: &AppHandle,
    registry: Option<&crate::models::RegistryConfig>,
) -> AppResult<AuthSession> {
    let dir = std::env::temp_dir().join(format!("preops-auth-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&dir)?;
    AUTH_DIR.with(|p| *p.borrow_mut() = Some(dir.clone()));
    let session = AuthSession(dir.clone());
    if let Some(reg) = registry {
        if !reg.username.is_empty() || !reg.password.is_empty() {
            let host = crate::validation::normalize_host(&reg.url)?;
            let mut cmd = crane_command(
                app,
                &[
                    "auth",
                    "login",
                    &host,
                    "-u",
                    &reg.username,
                    "--password-stdin",
                ],
            )?;
            cmd.env("DOCKER_CONFIG", &dir)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = cmd.spawn()?;
            child
                .stdin
                .take()
                .ok_or_else(|| AppError::Io("认证输入失败".into()))?
                .write_all(reg.password.as_bytes())?;
            let out = child.wait_with_output()?;
            if !out.status.success() {
                return Err(AppError::Invalid(format!("私有仓库 {host} 认证失败")));
            }
        }
    }
    Ok(session)
}

// ==================== 查询 ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInspect {
    /// 是否为多架构 index/manifest-list
    pub is_list: bool,
    /// 如 ["linux/amd64", "linux/arm64"]
    pub arches: Vec<String>,
    pub digest: String,
    /// 最终使用的源（哪个镜像源成功）
    pub source: String,
}

fn run_crane(app: &AppHandle, args: &[&str]) -> AppResult<std::process::Output> {
    let mut cmd = crane_command(app, args)?;
    AUTH_DIR.with(|p| {
        if let Some(dir) = p.borrow().as_ref() {
            cmd.env("DOCKER_CONFIG", dir);
        }
    });
    cmd.output()
        .map_err(|e| AppError::Io(format!("执行 crane 失败: {e}")))
}

#[derive(Deserialize)]
struct RawIndex {
    #[serde(default)]
    manifests: Vec<RawIndexEntry>,
}

#[derive(Deserialize)]
struct RawIndexEntry {
    #[serde(default)]
    platform: Option<RawPlatform>,
}

#[derive(Deserialize)]
struct RawPlatform {
    #[serde(default)]
    architecture: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    variant: Option<String>,
}

#[derive(Deserialize)]
struct RawConfig {
    #[serde(default)]
    architecture: String,
    #[serde(default)]
    os: String,
}

/// 查询镜像的架构支持矩阵与 digest（按镜像源顺序尝试，后台线程执行）
#[tauri::command]
pub async fn inspect_image(
    app: AppHandle,
    image: String,
    registry: Option<crate::models::RegistryConfig>,
) -> AppResult<ImageInspect> {
    tauri::async_runtime::spawn_blocking(move || {
        inspect_image_sync(&app, &image, registry.as_ref())
    })
    .await
    .map_err(|e| AppError::Io(format!("查询任务异常: {e}")))?
}

fn inspect_image_sync(
    app: &AppHandle,
    image: &str,
    registry: Option<&crate::models::RegistryConfig>,
) -> AppResult<ImageInspect> {
    let settings = store::load_settings(app)?;
    let _auth = registry_login(app, registry)?;
    let r = parse_reference(image)?;
    let mut errors: Vec<String> = Vec::new();

    for cand in candidates(&settings, &r, registry) {
        // 1) manifest：判断是否 index 并收集 platform
        match run_crane(app, &["manifest", "--platform", "all", &cand]) {
            Ok(o) if o.status.success() => {
                let body = String::from_utf8_lossy(&o.stdout).into_owned();
                if let Ok(idx) = serde_json::from_str::<RawIndex>(&body) {
                    if !idx.manifests.is_empty() {
                        // 过滤 unknown/unknown（Docker buildx 的 attestation/SBOM 证明
                        // 条目，不是可运行平台）并去重
                        use std::collections::BTreeSet;
                        let arches: Vec<String> = idx
                            .manifests
                            .iter()
                            .filter_map(|m| m.platform.as_ref())
                            .filter(|p| !p.os.is_empty() && !p.architecture.is_empty())
                            .filter(|p| p.os != "unknown" && p.architecture != "unknown")
                            .map(|p| match &p.variant {
                                Some(v) if !v.is_empty() => {
                                    format!("{}/{}:{}", p.os, p.architecture, v)
                                }
                                _ => format!("{}/{}", p.os, p.architecture),
                            })
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                        let digest = query_digest(app, &cand)?;
                        return Ok(ImageInspect {
                            is_list: true,
                            arches,
                            digest,
                            source: cand,
                        });
                    }
                }
                // 2) 单架构：从 config 读 os/arch
                if let Ok(cfg_out) = run_crane(app, &["config", &cand]) {
                    if cfg_out.status.success() {
                        if let Ok(cfg) = serde_json::from_str::<RawConfig>(
                            &String::from_utf8_lossy(&cfg_out.stdout),
                        ) {
                            let digest = query_digest(app, &cand)?;
                            return Ok(ImageInspect {
                                is_list: false,
                                arches: vec![format!("{}/{}", cfg.os, cfg.architecture)],
                                digest,
                                source: cand,
                            });
                        }
                    }
                }
                errors.push(format!("{cand}: 响应无法解析"));
            }
            Ok(o) => errors.push(format!(
                "{cand}: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => errors.push(format!("{cand}: {e}")),
        }
    }
    Err(AppError::Io(format!(
        "查询失败（已尝试全部镜像源）：{}",
        errors.join("；")
    )))
}

fn query_digest(app: &AppHandle, reference: &str) -> AppResult<String> {
    let out = run_crane(app, &["digest", reference])?;
    if !out.status.success() {
        return Ok(String::new()); // digest 失败不阻断查询
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 查询镜像在指定平台的层总大小（进度条分母；失败返回 0=不显示进度）
fn total_download_hint(app: &AppHandle, candidates: &[String], platform: &str) -> u64 {
    for cand in candidates {
        if let Ok(o) = run_crane(app, &["manifest", "--platform", platform, cand]) {
            if !o.status.success() {
                continue;
            }
            if let Ok(v) =
                serde_json::from_str::<serde_json::Value>(&String::from_utf8_lossy(&o.stdout))
            {
                if let Some(layers) = v.get("layers").and_then(|l| l.as_array()) {
                    let total: u64 = layers
                        .iter()
                        .filter_map(|l| l.get("size").and_then(|s| s.as_u64()))
                        .sum();
                    if total > 0 {
                        return total;
                    }
                }
            }
        }
    }
    0
}

/// 查询仓库 tag 列表（部分镜像源不代理 tags 接口，自动回退，后台线程执行）
#[tauri::command]
pub async fn list_image_tags(
    app: AppHandle,
    image: String,
    registry: Option<crate::models::RegistryConfig>,
) -> AppResult<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        list_image_tags_sync(&app, &image, registry.as_ref())
    })
    .await
    .map_err(|e| AppError::Io(format!("tag 查询任务异常: {e}")))?
}

fn list_image_tags_sync(
    app: &AppHandle,
    image: &str,
    registry: Option<&crate::models::RegistryConfig>,
) -> AppResult<Vec<String>> {
    let settings = store::load_settings(app)?;
    let _auth = registry_login(app, registry)?;
    let r = parse_reference(image)?;
    let mut errors: Vec<String> = Vec::new();
    for cand in candidates(&settings, &r, registry) {
        let repo_only = cand.split('@').next().unwrap_or(&cand);
        let repo_only = match repo_only.rfind(':') {
            Some(i) if i > repo_only.rfind('/').unwrap_or(0) => &repo_only[..i],
            _ => repo_only,
        };
        match run_crane(app, &["ls", repo_only]) {
            Ok(o) if o.status.success() => {
                let tags: Vec<String> = String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                if !tags.is_empty() {
                    return Ok(tags);
                }
                errors.push(format!("{repo_only}: 空列表"));
            }
            Ok(o) => errors.push(format!(
                "{repo_only}: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => errors.push(format!("{repo_only}: {e}")),
        }
    }
    Err(AppError::Io(format!(
        "tag 列表获取失败（部分镜像源不支持 tags 查询）：{}",
        errors.join("；")
    )))
}

// ==================== 缓存 ====================

fn cache_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let s = store::effective_storage(app)?;
    let d = PathBuf::from(s.image_cache_root).join("docker-archives");
    fs::create_dir_all(&d)?;
    Ok(d)
}

/// 缓存文件名必须包含 registry+repo+tag/digest+平台：不同版本/不同源的同名
/// repo 绝不能共用缓存（否则换 tag 后命中旧 tar，现场 load 到错误版本）。
pub fn valid_digest(s: &str) -> bool {
    s.strip_prefix("sha256:")
        .is_some_and(|v| v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit()))
}
pub fn load_reference(image: &str) -> AppResult<String> {
    let r = parse_reference(image)?;
    if r.digest.is_some() {
        Ok(format!(
            "offline.local/image:{}",
            crate::io_util::hash(image)
        ))
    } else {
        Ok(image.into())
    }
}
pub fn locked_reference(image: &str, digest: &str) -> AppResult<String> {
    let r = parse_reference(image)?;
    if digest.is_empty() {
        return Ok(image.into());
    }
    if !valid_digest(digest) || r.digest.as_ref().is_some_and(|d| d != digest) {
        return Err(AppError::Invalid("引用与锁定 digest 不一致".into()));
    }
    Ok(format!("{}/{}@{}", r.registry, r.repo, digest))
}
pub fn cache_key(image: &str, os: &str, arch: &str, sources: &[String]) -> AppResult<String> {
    let r = parse_reference(image)?;
    Ok(crate::io_util::hash(&serde_json::json!({"version":2,"registry":r.registry,"repo":r.repo,"tag":r.tag,"digest":r.digest,"os":os,"arch":arch,"sources":sources}).to_string()))
}
pub fn cache_file_for(app: &AppHandle, image: &str, os: &str, arch: &str) -> AppResult<PathBuf> {
    cache_file_for_registry(app, image, os, arch, None)
}
pub fn cache_file_for_registry(
    app: &AppHandle,
    image: &str,
    os: &str,
    arch: &str,
    registry: Option<&crate::models::RegistryConfig>,
) -> AppResult<PathBuf> {
    let settings = store::load_settings(app)?;
    let key = cache_key(
        image,
        os,
        arch,
        &candidates(&settings, &parse_reference(image)?, registry),
    )?;
    Ok(cache_dir(app)?.join(format!("v2-{key}.tar")))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedImage {
    pub reference: String,
    pub platform: String,
    pub file: String,
    pub size_bytes: u64,
    pub pulled_at: String,
}

#[tauri::command]
pub fn list_image_cache(app: AppHandle) -> AppResult<Vec<CachedImage>> {
    let dir = cache_dir(&app)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() || !path.to_string_lossy().ends_with(".tar") {
            continue;
        }
        let meta_path = path.with_extension("tar.meta.json");
        let (reference, platform, pulled_at) = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| {
                (
                    v["reference"].as_str().unwrap_or_default().to_string(),
                    v["platform"].as_str().unwrap_or_default().to_string(),
                    v["pulledAt"].as_str().unwrap_or_default().to_string(),
                )
            })
            .unwrap_or_default();
        out.push(CachedImage {
            reference,
            platform,
            file: path.to_string_lossy().into_owned(),
            size_bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
            pulled_at,
        });
    }
    out.sort_by(|a, b| b.pulled_at.cmp(&a.pulled_at));
    Ok(out)
}

#[tauri::command]
pub fn delete_cached_image(app: AppHandle, file: String) -> AppResult<()> {
    let p = PathBuf::from(&file);
    // 路径穿越防护：canonicalize 规范化 .. 后，必须仍位于缓存目录内
    // （Path::starts_with 不规范化 ..，直接比较可被 ..\..\ 绕过）
    let dir = cache_dir(&app)?;
    let p_canon = p
        .canonicalize()
        .map_err(|_| AppError::Invalid(format!("文件不存在: {file}")))?;
    let dir_canon = dir.canonicalize()?;
    if p_canon.parent() != Some(dir_canon.as_path())
        || p_canon.extension().and_then(|e| e.to_str()) != Some("tar")
    {
        return Err(AppError::Invalid("仅允许删除镜像缓存目录内的文件".into()));
    }
    let _lock = crate::io_util::lock(&p_canon.with_extension("lock"))?;
    fs::remove_file(&p_canon)?;
    crate::app_ops::__audit(&app, "delete_cached_image", &file);
    let _ = fs::remove_file(p_canon.with_extension("tar.meta.json"));
    Ok(())
}

// ==================== 拉取 ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullResult {
    pub reference: String,
    pub platform: String,
    pub cache_file: String,
    pub size_bytes: u64,
    pub digest: String,
    pub source: String,
    pub cached: bool,
    pub elapsed_ms: u64,
}

fn emit(app: &AppHandle, status: &str, detail: &str) {
    let _ = app.emit("image-pull", serde_json::json!({ "status": status, "detail": detail,"taskId":PULL_TASK.with(|v|v.borrow().clone()) }));
}

/// 拉取镜像到缓存（跨架构 docker-archive，后台线程执行）。命中缓存直接返回。
#[tauri::command]
pub async fn pull_image(
    app: AppHandle,
    image: String,
    arch: String,
    registry: Option<crate::models::RegistryConfig>,
    task_id: Option<String>,
) -> AppResult<PullResult> {
    tauri::async_runtime::spawn_blocking(move || {
        PULL_TASK.with(|v| {
            *v.borrow_mut() = task_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        });
        pull_image_sync(&app, &image, &arch, registry.as_ref())
    })
    .await
    .map_err(|e| AppError::Io(format!("拉取任务异常: {e}")))?
}

fn pull_image_sync(
    app: &AppHandle,
    image: &str,
    arch: &str,
    registry: Option<&crate::models::RegistryConfig>,
) -> AppResult<PullResult> {
    let final_path = cache_file_for_registry(app, image, "linux", arch, registry)?;
    pull_image_inner(app, image, "linux", arch, &final_path, registry)
}

pub fn pull_image_inner(
    app: &AppHandle,
    image: &str,
    os: &str,
    arch: &str,
    final_path: &Path,
    registry: Option<&crate::models::RegistryConfig>,
) -> AppResult<PullResult> {
    let settings = store::load_settings(app)?;
    let _auth = registry_login(app, registry)?;
    let _lock = crate::io_util::lock(&final_path.with_extension("lock"))?;
    let r = parse_reference(image)?;
    let start = Instant::now();
    let tmp = crate::io_util::unique_sibling(final_path);
    let _clean = crate::io_util::Cleanup(tmp.clone());
    let rewritten = crate::io_util::unique_sibling(final_path);
    let _rewrite_clean = crate::io_util::Cleanup(rewritten.clone());
    let platform = format!("{os}/{arch}");
    let mut errors = vec![];
    for cand in candidates(&settings, &r, registry) {
        // Resolve before pulling: a moving tag cannot change the recorded content underneath us.
        let digest = match query_digest(app, &cand) {
            Ok(d) if valid_digest(&d) => d,
            Ok(_) => {
                errors.push(format!("{cand}: digest 无效"));
                continue;
            }
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };
        if r.digest.as_ref().is_some_and(|d| d != &digest) {
            errors.push(format!("{cand}: 锁定 digest 不匹配"));
            continue;
        }
        let meta_path = final_path.with_extension("tar.meta.json");
        if let Ok(raw) = fs::read_to_string(&meta_path) {
            if let Ok(m) = serde_json::from_str::<serde_json::Value>(&raw) {
                if m["version"] == 2
                    && m["digest"].as_str() == Some(&digest)
                    && m["source"].as_str() == Some(&cand)
                    && m["reference"]
                        .as_str()
                        .is_some_and(|r| same_reference(r, image))
                    && m["platform"].as_str() == Some(&platform)
                {
                    if let Ok(v) = crate::image_archive::verify(final_path, image, os, arch) {
                        if m["sha256"].as_str() == Some(&v.sha256) {
                            return Ok(PullResult {
                                reference: image.into(),
                                platform,
                                cache_file: final_path.to_string_lossy().into_owned(),
                                size_bytes: v.size,
                                digest,
                                source: cand,
                                cached: true,
                                elapsed_ms: start.elapsed().as_millis() as u64,
                            });
                        }
                    }
                }
            }
        }
        let parsed = parse_reference(&cand)?;
        let immutable = format!("{}/{}@{digest}", parsed.registry, parsed.repo);
        let total = total_download_hint(app, std::slice::from_ref(&immutable), &platform);
        crate::io_util::require_space(final_path, total.saturating_mul(3).max(256 * 1024 * 1024))?;
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        let watch_path = tmp.clone();
        let watch_app = app.clone();
        let watch_ref = image.to_string();
        let watch_arch = arch.to_string();
        let pull_task = PULL_TASK.with(|v| v.borrow().clone());
        let watcher = std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(m) = fs::metadata(&watch_path) {
                    let _=watch_app.emit("image-pull-progress",serde_json::json!({"taskId":pull_task,"reference":watch_ref,"arch":watch_arch,"bytes":m.len(),"total":total,"percent":if total>0 {(m.len() as f64/total as f64*100.0).min(99.0)}else{0.0},"speedBps":0}));
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
        emit(app, "pulling", &format!("{immutable} → {platform}"));
        let out = run_crane(
            app,
            &[
                "pull",
                "--platform",
                &platform,
                "--format=tarball",
                &immutable,
                &tmp.to_string_lossy(),
            ],
        );
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = watcher.join();
        match out {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                errors.push(format!("{cand}: {}", String::from_utf8_lossy(&o.stderr)));
                continue;
            }
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        }
        rewrite_archive(&tmp, &rewritten, &load_reference(image)?)?;
        let v = crate::image_archive::verify(&rewritten, image, os, arch)?;
        let meta = serde_json::json!({"version":2,"reference":image,"platform":platform,"digest":digest,"source":cand,"pulledAt":store::now_rfc3339(),"sizeBytes":v.size,"sha256":v.sha256,"configDigest":v.config_digest});
        // A missing meta after a crash is a cache miss, never an accepted half-commit.
        fs::rename(&rewritten, final_path)?;
        crate::io_util::atomic_write(&meta_path, serde_json::to_string_pretty(&meta)?.as_bytes())?;
        crate::app_ops::__audit(app, "pull_image", &format!("{image} {platform} {digest}"));
        emit(app, "done", &format!("{image} {platform} 完成"));
        return Ok(PullResult {
            reference: image.into(),
            platform,
            cache_file: final_path.to_string_lossy().into_owned(),
            size_bytes: v.size,
            digest,
            source: cand,
            cached: false,
            elapsed_ms: start.elapsed().as_millis() as u64,
        });
    }
    Err(AppError::Io(format!("镜像拉取失败：{}", errors.join("；"))))
}

/// 改写 docker-archive 中的 manifest.json RepoTags 为规范引用
pub fn rewrite_archive(src: &Path, dst: &Path, canonical_ref: &str) -> AppResult<()> {
    let f = File::open(src)?;
    let mut archive = tar::Archive::new(f);
    let mut builder = tar::Builder::new(File::create(dst)?);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let name = path.to_string_lossy().replace('\\', "/");
        if name == "manifest.json" {
            let mut body = String::new();
            entry.read_to_string(&mut body)?;
            let rewritten = rewrite_repo_tags(&body, canonical_ref);
            let mut header = tar::Header::new_gnu();
            header.set_size(rewritten.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, "manifest.json", rewritten.as_bytes())?;
        } else if name != "repositories" {
            // docker save 老格式还有 repositories 文件（仅旧版本），保留其余条目原样
            let mut header = entry.header().clone();

            // 修正 header 大小并重算校验和（entry 读取后 size 不变，直接使用）
            header.set_size(entry.size());
            header.set_cksum();
            builder.append_data(&mut header, path, &mut entry)?;
        }
    }
    builder.into_inner()?.flush()?;
    Ok(())
}

/// 集成测试辅助（tests/images_tests.rs 使用）
pub fn rewrite_repo_tags_for_test(body: &str, canonical_ref: &str) -> String {
    rewrite_repo_tags(body, canonical_ref)
}

fn rewrite_repo_tags(body: &str, canonical_ref: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => {
            let mut v = v;
            if let Some(arr) = v.as_array_mut() {
                for item in arr.iter_mut() {
                    if let Some(tags) = item.get_mut("RepoTags") {
                        *tags = serde_json::json!([canonical_ref]);
                    }
                }
            }
            v.to_string()
        }
        Err(_) => body.to_string(),
    }
}
