use crate::{
    error::{AppError, AppResult},
    io_util,
    models::{AppSettings, StorageInfo},
    store,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Emitter};

/// Reversible copy-and-switch: original roots remain intact, settings publish last.
#[tauri::command]
pub async fn migrate_storage(
    app: AppHandle,
    settings: AppSettings,
    task_id: Option<String>,
) -> AppResult<StorageInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = crate::tasks::Session::begin(task_id)?;
        let _lock = store::storage_lock(&app)?;
        let settings = store::normalize_settings(&settings)?;
        let current = store::effective_storage(&app)?;
        let data = Path::new(&current.config_dir);
        let changes = [
            (&current.projects_root, &settings.projects_root, data.join("projects")),
            (&current.image_cache_root, &settings.image_cache_root, data.join("cache")),
            (&current.docker_pkg_root, &settings.docker_pkg_root, data.join("cache/docker-pkgs")),
            (&current.artifact_root, &settings.artifact_root, data.join("dist")),
            (&current.log_root, &settings.log_root, data.join("logs")),
        ];
        // Freeze project/catalog writers while copying, but acquire data lock after settings reads.
        let data_lock = store::data_lock(&app)?;
        let paths: Vec<_> = changes.into_iter().map(|(src, configured, default)| {
            (PathBuf::from(src), configured.as_ref().map(PathBuf::from).unwrap_or(default))
        }).collect();
        copy_roots(&paths, |src, dst, bytes| {
            let _ = app.emit("storage-migration", serde_json::json!({"taskId":crate::tasks::id(),"step":"copy","detail":format!("{} → {}（约 {} MB；保留旧目录）", src.display(), dst.display(), bytes/1048576)}));
        }, || {
            drop(data_lock); // settings publication acquires this same data lock.
            store::save_settings_locked(&app, &settings)
        })?;
        store::effective_storage(&app)
    }).await.map_err(|e| AppError::Io(format!("存储迁移任务异常: {e}")))?
}
fn resolved(path: &Path) -> AppResult<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(AppError::Invalid("迁移路径必须为绝对路径".into()));
    }
    let parent = path
        .ancestors()
        .find(|p| p.exists())
        .ok_or_else(|| AppError::Invalid("迁移路径不可访问".into()))?;
    Ok(parent.canonicalize()?.join(
        path.strip_prefix(parent)
            .map_err(|_| AppError::Invalid("迁移路径无效".into()))?,
    ))
}
fn key(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}
fn transient(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    [".lock", ".downloading", ".tmp", ".staging"]
        .iter()
        .any(|s| name.ends_with(s))
}
fn excluded(path: &Path, exclusions: &[PathBuf]) -> bool {
    transient(path) || exclusions.iter().any(|p| key(path).starts_with(key(p)))
}
fn tree_size(src: &Path, exclusions: &[PathBuf]) -> AppResult<u64> {
    crate::tasks::check()?;
    if !src.exists() {
        return Ok(0);
    }
    let mut size: u64 = 0;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if excluded(&entry.path(), exclusions) {
            continue;
        }
        if entry.file_type()?.is_symlink() {
            return Err(AppError::Invalid(
                "迁移不接受符号链接，请先整理源目录".into(),
            ));
        }
        size = size.saturating_add(if entry.file_type()?.is_dir() {
            tree_size(&entry.path(), exclusions)?
        } else if entry.file_type()?.is_file() {
            entry.metadata()?.len()
        } else {
            return Err(AppError::Invalid("迁移含不支持的文件类型".into()));
        });
    }
    Ok(size)
}
fn copy_tree(src: &Path, dst: &Path, exclusions: &[PathBuf]) -> AppResult<()> {
    fs::create_dir_all(dst)?;
    if !src.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(src)? {
        crate::tasks::check()?;
        let entry = entry?;
        if excluded(&entry.path(), exclusions) {
            continue;
        }
        let name = entry.file_name();
        let dest = dst.join(&name);
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(AppError::Invalid("迁移不接受符号链接".into()));
        }
        if kind.is_dir() {
            copy_tree(&entry.path(), &dest, exclusions)?;
        } else if kind.is_file() {
            io_util::copy_atomic(&entry.path(), &dest)?;
            if io_util::sha256_file(&entry.path())? != io_util::sha256_file(&dest)? {
                return Err(AppError::Invalid("源文件发生变化或复制校验失败".into()));
            }
        } else {
            return Err(AppError::Invalid("迁移含不支持的文件类型".into()));
        }
    }
    Ok(())
}
pub fn copy_root(src: &Path, dst: &Path) -> AppResult<()> {
    copy_roots(
        &[(src.to_path_buf(), dst.to_path_buf())],
        |_, _, _| {},
        || Ok(()),
    )
}
fn empty_target(dst: &Path) -> AppResult<()> {
    if dst.exists() && (!dst.is_dir() || fs::read_dir(dst)?.next().is_some()) {
        return Err(AppError::Invalid(format!(
            "目标 {} 必须为空目录，避免覆盖已有数据；可直接切换到已有目录",
            dst.display()
        )));
    }
    Ok(())
}

/// Stage every changed root first, then publish copies and settings together.
/// Nested cache/package roots are copied once when their relative layout is preserved.
pub fn copy_roots(
    paths: &[(PathBuf, PathBuf)],
    mut progress: impl FnMut(&Path, &Path, u64),
    switch: impl FnOnce() -> AppResult<()>,
) -> AppResult<()> {
    let paths: Vec<_> = paths
        .iter()
        .map(|(a, b)| Ok((resolved(a)?, resolved(b)?)))
        .collect::<AppResult<_>>()?;
    let changed: Vec<_> = paths.iter().filter(|(a, b)| key(a) != key(b)).collect();
    for (src, dst) in &changed {
        for (other_src, _) in &paths {
            if key(other_src).starts_with(key(dst)) || key(dst).starts_with(key(other_src)) {
                return Err(AppError::Invalid("迁移目标不能与任何原存储目录重叠".into()));
            }
        }
        for (other_src, other_dst) in &paths {
            if src == other_src {
                continue;
            }
            if key(other_dst).starts_with(key(dst)) {
                let suffix = key(other_src)
                    .strip_prefix(key(src))
                    .map(Path::to_path_buf)
                    .ok();
                if suffix
                    .as_ref()
                    .is_none_or(|s| key(dst.join(s).as_path()) != key(other_dst))
                {
                    return Err(AppError::Invalid(
                        "不同存储目标不能重叠；嵌套目录需保持原有相对布局".into(),
                    ));
                }
            }
        }
    }
    let mut plans = vec![];
    for (src, dst) in &changed {
        let covered = changed.iter().any(|(a, b)| {
            a != src
                && key(src)
                    .strip_prefix(key(a))
                    .is_ok_and(|suffix| key(b.join(suffix).as_path()) == key(dst))
        });
        if covered {
            continue;
        }
        empty_target(dst)?;
        let exclusions: Vec<_> = paths
            .iter()
            .filter(|(a, b)| {
                a != src
                    && key(a)
                        .strip_prefix(key(src))
                        .is_ok_and(|suffix| key(dst.join(suffix).as_path()) != key(b))
            })
            .map(|(a, _)| a.clone())
            .collect();
        let bytes = tree_size(src, &exclusions)?;
        plans.push((src.clone(), dst.clone(), exclusions, bytes));
    }
    // Conservative per-volume peak: staged copies plus a small transaction allowance.
    for (_, dst, _, _) in &plans {
        let volume = |p: &Path| {
            key(p)
                .components()
                .take(if cfg!(windows) { 2 } else { 1 })
                .collect::<PathBuf>()
        };
        let bytes = plans
            .iter()
            .filter(|(_, p, _, _)| volume(p) == volume(dst))
            .fold(128 * 1024 * 1024u64, |n, (_, _, _, b)| n.saturating_add(*b));
        io_util::require_space(dst, bytes)?;
    }
    let mut stages = vec![];
    for (src, dst, exclusions, bytes) in &plans {
        progress(src, dst, *bytes);
        let stage = io_util::Cleanup(io_util::unique_sibling(dst));
        copy_tree(src, &stage.0, exclusions)?;
        stages.push(stage);
    }
    crate::tasks::check()?;
    for (_, dst, _, _) in &plans {
        empty_target(dst)?;
    }
    let mut published: Vec<(PathBuf, bool)> = vec![];
    let result = (|| {
        for ((_, dst, _, _), stage) in plans.iter().zip(&stages) {
            let existed = dst.exists();
            io_util::commit_directory(&stage.0, dst)?;
            published.push((dst.clone(), existed));
        }
        switch()
    })();
    if let Err(error) = result {
        let mut retained = vec![];
        for (dst, existed) in published.into_iter().rev() {
            if fs::remove_dir_all(&dst).is_err() || existed && fs::create_dir_all(&dst).is_err() {
                retained.push(dst.display().to_string());
            }
        }
        if !retained.is_empty() {
            return Err(AppError::Io(format!(
                "{error}；原数据仍保留；未能清理复制目录 {}，重试前请检查",
                retained.join("、")
            )));
        }
        return Err(error);
    }
    Ok(())
}
