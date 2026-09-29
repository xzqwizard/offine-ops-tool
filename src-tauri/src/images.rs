use crate::engine::crane_command;
use crate::error::{AppError, AppResult};
use crate::store;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{copy, Read, Write};
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    pub registry: String,
    pub repo: String,
    pub tag: String,
    pub digest: Option<String>,
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
    let (base, digest) = match s.split_once('@') {
        Some((b, d)) => (b, Some(d.to_string())),
        None => (s, None),
    };
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
    if repo.is_empty() {
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
fn candidates(settings: &crate::models::AppSettings, r: &ImageRef) -> Vec<String> {
    let suffix = match &r.digest {
        Some(d) => format!("{}@{d}", r.repo),
        None => format!("{}:{}", r.repo, r.tag),
    };
    if r.registry == "docker.io" {
        let mut hosts: Vec<String> = Vec::new();
        for m in &settings.registry_mirrors {
            let host = m.trim().trim_end_matches('/').to_string();
            if host.is_empty() {
                continue;
            }
            let host = if host == "docker.io" { "docker.io".into() } else { host };
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        if hosts.is_empty() {
            hosts.push("docker.io".into());
        }
        hosts.iter().map(|h| format!("{h}/{suffix}")).collect()
    } else {
        vec![format!("{}/{suffix}", r.registry)]
    }
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
pub async fn inspect_image(app: AppHandle, image: String) -> AppResult<ImageInspect> {
    tauri::async_runtime::spawn_blocking(move || inspect_image_sync(&app, &image))
        .await
        .map_err(|e| AppError::Io(format!("查询任务异常: {e}")))?
}

fn inspect_image_sync(app: &AppHandle, image: &str) -> AppResult<ImageInspect> {
    let settings = store::load_settings(app)?;
    let r = parse_reference(image)?;
    let mut errors: Vec<String> = Vec::new();

    for cand in candidates(&settings, &r) {
        // 1) manifest：判断是否 index 并收集 platform
        match run_crane(app, &["manifest", "--platform", "all", &cand]) {
            Ok(o) if o.status.success() => {
                let body = String::from_utf8_lossy(&o.stdout).into_owned();
                if let Ok(idx) = serde_json::from_str::<RawIndex>(&body) {
                    if !idx.manifests.is_empty() {
                        let arches: Vec<String> = idx
                            .manifests
                            .iter()
                            .filter_map(|m| m.platform.as_ref())
                            .filter(|p| !p.os.is_empty() && !p.architecture.is_empty())
                            .map(|p| {
                                match &p.variant {
                                    Some(v) if !v.is_empty() => {
                                        format!("{}/{}:{}", p.os, p.architecture, v)
                                    }
                                    _ => format!("{}/{}", p.os, p.architecture),
                                }
                            })
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
                        if let Ok(cfg) =
                            serde_json::from_str::<RawConfig>(&String::from_utf8_lossy(&cfg_out.stdout))
                        {
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

/// 查询仓库 tag 列表（部分镜像源不代理 tags 接口，自动回退，后台线程执行）
#[tauri::command]
pub async fn list_image_tags(app: AppHandle, image: String) -> AppResult<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || list_image_tags_sync(&app, &image))
        .await
        .map_err(|e| AppError::Io(format!("tag 查询任务异常: {e}")))?
}

fn list_image_tags_sync(app: &AppHandle, image: &str) -> AppResult<Vec<String>> {
    let settings = store::load_settings(app)?;
    let r = parse_reference(image)?;
    let mut errors: Vec<String> = Vec::new();
    for cand in candidates(&settings, &r) {
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
fn sanitize_ref(r: &ImageRef, os: &str, arch: &str) -> String {
    // digest 优先（版本唯一）；否则用 tag；registry 前缀区分私有源
    let version = r.digest.as_deref().unwrap_or(&r.tag);
    let registry = if r.registry == "docker.io" { "hub" } else { &r.registry };
    let raw = format!("{registry}_{}-{version}_{os}_{arch}", r.repo.replace('/', "_"));
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn cache_file_for(app: &AppHandle, image: &str, os: &str, arch: &str) -> AppResult<PathBuf> {
    let r = parse_reference(image)?;
    Ok(cache_dir(app)?.join(format!("{}.tar", sanitize_ref(&r, os, arch))))
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
    if !p_canon.starts_with(&dir_canon) {
        return Err(AppError::Invalid("仅允许删除镜像缓存目录内的文件".into()));
    }
    fs::remove_file(&p_canon)?;
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
    let _ = app.emit("image-pull", serde_json::json!({ "status": status, "detail": detail }));
}

/// 拉取镜像到缓存（跨架构 docker-archive，后台线程执行）。命中缓存直接返回。
#[tauri::command]
pub async fn pull_image(app: AppHandle, image: String, arch: String) -> AppResult<PullResult> {
    tauri::async_runtime::spawn_blocking(move || pull_image_sync(&app, &image, &arch))
        .await
        .map_err(|e| AppError::Io(format!("拉取任务异常: {e}")))?
}

fn pull_image_sync(app: &AppHandle, image: &str, arch: &str) -> AppResult<PullResult> {
    let os = "linux";
    let final_path = cache_file_for(app, image, os, arch)?;
    if final_path.is_file() {
        return Ok(PullResult {
            reference: image.to_string(),
            platform: format!("{os}/{arch}"),
            cache_file: final_path.to_string_lossy().into_owned(),
            size_bytes: final_path.metadata().map(|m| m.len()).unwrap_or(0),
            digest: String::new(),
            source: "cache".into(),
            cached: true,
            elapsed_ms: 0,
        });
    }
    let result = pull_image_inner(app, image, os, arch, &final_path)?;
    Ok(result)
}

pub fn pull_image_inner(
    app: &AppHandle,
    image: &str,
    os: &str,
    arch: &str,
    final_path: &Path,
) -> AppResult<PullResult> {
    let settings = store::load_settings(app)?;
    let r = parse_reference(image)?;
    let start = Instant::now();
    let dir = final_path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.downloading",
        final_path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut errors: Vec<String> = Vec::new();
    let mut ok_source = String::new();

    for cand in candidates(&settings, &r) {
        emit(app, "pulling", &format!("{} → {os}/{arch}", cand));
        let platform = format!("{os}/{arch}");
        match run_crane(app, &["pull", "--platform", &platform, "--format=tarball", &cand, &tmp.to_string_lossy()]) {
            Ok(o) if o.status.success() => {
                ok_source = cand.clone();
                break;
            }
            Ok(o) => errors.push(format!(
                "{cand}: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => errors.push(format!("{cand}: {e}")),
        }
        let _ = fs::remove_file(&tmp);
    }

    if ok_source.is_empty() {
        emit(app, "error", &format!("拉取失败: {}", errors.join("；")));
        return Err(AppError::Io(format!(
            "镜像拉取失败（已尝试全部镜像源）：{}",
            errors.join("；")
        )));
    }

    // RepoTags 改写为用户原始引用，保证 docker load 后与 compose 一致
    emit(app, "rewrite", "规范化镜像标签（RepoTags）…");
    let rewrite_tmp = dir.join(format!(
        ".{}.rewriting",
        final_path.file_name().unwrap_or_default().to_string_lossy()
    ));
    rewrite_docker_archive(&tmp, &rewrite_tmp, image)?;
    let _ = fs::remove_file(&tmp);
    fs::rename(&rewrite_tmp, final_path)?;

    let digest = query_digest(app, &ok_source).unwrap_or_default();
    let size = final_path.metadata().map(|m| m.len()).unwrap_or(0);
    let meta = serde_json::json!({
        "reference": image,
        "platform": format!("{os}/{arch}"),
        "digest": digest,
        "source": ok_source,
        "pulledAt": store::now_rfc3339(),
        "sizeBytes": size,
    });
    let _ = fs::write(
        final_path.with_extension("tar.meta.json"),
        serde_json::to_string_pretty(&meta)?,
    );

    let elapsed = start.elapsed().as_millis() as u64;
    emit(
        app,
        "done",
        &format!("{} ({os}/{arch}) 完成，{:.1} MB，{}ms", image, size as f64 / 1048576.0, elapsed),
    );
    Ok(PullResult {
        reference: image.to_string(),
        platform: format!("{os}/{arch}"),
        cache_file: final_path.to_string_lossy().into_owned(),
        size_bytes: size,
        digest,
        source: ok_source,
        cached: false,
        elapsed_ms: elapsed,
    })
}

/// 改写 docker-archive 中的 manifest.json RepoTags 为规范引用
fn rewrite_docker_archive(src: &Path, dst: &Path, canonical_ref: &str) -> AppResult<()> {
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
            let mut data = Vec::new();
            copy(&mut entry, &mut data)?;
            // 修正 header 大小并重算校验和（entry 读取后 size 不变，直接使用）
            header.set_size(data.len() as u64);
            header.set_cksum();
            builder.append_data(&mut header, path, data.as_slice())?;
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
