use crate::error::{AppError, AppResult};
use crate::models::Project;
use crate::store;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;

/// 应用级操作：方案快照恢复 / 缓存引用治理 / 单方案导入导出 / 审计日志

// ==================== 审计日志 ====================

const AUDIT_FILE: &str = "audit.log";

fn audit_log(app: &AppHandle, action: &str, detail: &str) {
    let Ok(storage) = store::effective_storage(app) else { return };
    let line = format!(
        "{}\t{}\t{}\n",
        store::now_rfc3339(),
        action,
        detail.replace(['\n', '\r'], " ")
    );
    let p = PathBuf::from(storage.log_root).join(AUDIT_FILE);
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::OpenOptions::new().create(true).append(true).open(&p).and_then(|mut f| {
        use std::io::Write as _;
        f.write_all(line.as_bytes())
    });
}

#[tauri::command]
pub fn list_audit_log(app: AppHandle) -> AppResult<Vec<String>> {
    let storage = store::effective_storage(&app)?;
    let p = PathBuf::from(storage.log_root).join(AUDIT_FILE);
    if !p.is_file() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&p)?;
    let mut lines: Vec<String> = raw.lines().map(String::from).collect();
    let n = lines.len();
    lines.drain(..n.saturating_sub(200)); // 最近 200 条
    lines.reverse();
    Ok(lines)
}

// ==================== 从历史构建恢复方案（G2） ====================

/// 读取某次构建时的方案快照，另存为新方案（原名 + 后缀，不动当前方案）
#[tauri::command]
pub fn restore_project_from_build(app: AppHandle, dir: String) -> AppResult<Project> {
    let p = PathBuf::from(&dir);
    // 路径校验：必须是构建产物目录（两层结构，含 build-manifest.json）
    let root = PathBuf::from(store::effective_storage(&app)?.artifact_root);
    let p_canon = p
        .canonicalize()
        .map_err(|_| AppError::Invalid(format!("目录不存在: {dir}")))?;
    let root_canon = root.canonicalize()?;
    if p_canon.parent().and_then(|p| p.parent()) != Some(root_canon.as_path()) {
        return Err(AppError::Invalid("仅允许从构建产物目录恢复".into()));
    }
    let snapshot = p_canon.join("project-snapshot.json");
    if !snapshot.is_file() {
        return Err(AppError::NotFound("该构建没有方案快照（旧版本构建）".into()));
    }
    let raw = fs::read_to_string(&snapshot)?;
    let mut proj: Project = serde_json::from_str(&raw)?;
    let build_id = p_canon
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    proj.id = store::new_project_id();
    proj.name = format!("{}（自 {} 恢复）", proj.name, build_id);
    let now = store::now_rfc3339();
    proj.created_at = now.clone();
    proj.updated_at = now;
    store::save_project(&app, &proj)?;
    audit_log(&app, "restore_project_from_build", &format!("{} → {}", build_id, proj.name));
    Ok(proj)
}

// ==================== 单方案导出 / 导入（G4） ====================

/// 导出单方案为 .oppjson（含服务器/实例/规则/私有仓库配置——含敏感字段，注意文件保管）
#[tauri::command]
pub fn export_project(app: AppHandle, id: String, output_path: String) -> AppResult<String> {
    let proj = store::load_project(&app, &id)?;
    let raw = serde_json::to_string_pretty(&proj)?;
    fs::write(&output_path, raw)?;
    audit_log(&app, "export_project", &proj.name);
    Ok(format!("已导出「{}」（{}）→ {output_path}", proj.name, proj.servers.len()))
}

/// 导入 .oppjson：生成新 id 保存（重名自动加后缀，IP 保留原值可再编辑）
#[tauri::command]
pub fn import_project(app: AppHandle, input_path: String) -> AppResult<Project> {
    let raw = fs::read_to_string(&input_path)
        .map_err(|e| AppError::Io(format!("读取文件失败: {e}")))?;
    let mut proj: Project = serde_json::from_str(&raw)
        .map_err(|e| AppError::Serialize(format!("文件不是有效的方案导出（{e}）")))?;
    // 与现有方案重名时加后缀
    let existing = store::list_projects(&app)?;
    if existing.iter().any(|p| p.name == proj.name) {
        proj.name = format!("{}（导入）", proj.name);
    }
    proj.id = store::new_project_id();
    let now = store::now_rfc3339();
    proj.created_at = now.clone();
    proj.updated_at = now;
    store::save_project(&app, &proj)?;
    audit_log(&app, "import_project", &proj.name);
    Ok(proj)
}

// ==================== 镜像缓存引用治理（G3） ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    pub file: String,
    pub reference: String,
    pub platform: String,
    pub size_bytes: u64,
    /// 引用该镜像的方案名列表
    pub referenced_by: Vec<String>,
}

#[tauri::command]
pub fn analyze_cache_usage(app: AppHandle) -> AppResult<Vec<CacheUsage>> {
    let cache = crate::images::list_image_cache(app.clone())?;
    let projects = store::list_projects(&app)?;
    let mut loaded: Vec<(String, Project)> = Vec::new();
    for s in &projects {
        if let Ok(p) = store::load_project(&app, &s.id) {
            loaded.push((s.name.clone(), p));
        }
    }
    // 引用匹配两侧归一化（parse_reference）：缓存 meta 存的是拉取时的原始输入串，
    // "mysql:8.0" 与 "docker.io/library/mysql:8.0" 是同一缓存文件，裸字符串相等会误判未引用
    let norm = |img: &str| -> Option<(String, String, String)> {
        crate::images::parse_reference(img).ok().map(|r| (r.registry, r.repo, r.tag))
    };
    let mut out = Vec::new();
    for c in cache {
        let c_norm = norm(&c.reference);
        let referenced_by: Vec<String> = loaded
            .iter()
            .filter(|(_, p)| {
                p.instances.iter().any(|i| {
                    i.image == c.reference
                        || (c_norm.is_some()
                            && norm(&i.image).map_or(false, |n| Some(n) == c_norm))
                })
            })
            .map(|(name, _)| name.clone())
            .collect();
        out.push(CacheUsage {
            file: c.file,
            reference: c.reference,
            platform: c.platform,
            size_bytes: c.size_bytes,
            referenced_by,
        });
    }
    Ok(out)
}

/// 一键清理未被任何方案引用的缓存镜像，返回 (清理数, 释放字节, 失败明细)
#[tauri::command]
pub fn purge_unref_cache(app: AppHandle) -> AppResult<(u32, u64, Vec<String>)> {
    let usage = analyze_cache_usage(app.clone())?;
    let mut freed = 0u64;
    let mut count = 0u32;
    let mut errors: Vec<String> = Vec::new();
    for u in usage.iter().filter(|u| u.referenced_by.is_empty()) {
        match crate::images::delete_cached_image(app.clone(), u.file.clone()) {
            Ok(()) => {
                freed += u.size_bytes;
                count += 1;
            }
            Err(e) => errors.push(format!("{}: {e}", u.reference)),
        }
    }
    if count > 0 || !errors.is_empty() {
        audit_log(
            &app,
            "purge_unref_cache",
            &format!("{count} 项释放 {freed} 字节，失败 {} 项", errors.len()),
        );
    }
    Ok((count, freed, errors))
}

/// 内部审计入口（catalog 等模块复用）
pub fn __audit(app: &AppHandle, action: &str, detail: &str) {
    audit_log(app, action, detail);
}
