use crate::error::{AppError, AppResult};
use crate::store;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 构建历史管理：扫描产物根下各项目的 build-manifest.json，
/// 提供列表 / 删除 / 打开目录，并作为增量升级包的基线数据源。

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHistoryEntry {
    pub build_id: String,
    pub project_id: String,
    pub project_name: String,
    pub generated_at: String,
    /// full | upgrade
    pub kind: String,
    pub servers: Vec<BuildHistoryServer>,
    pub dir: String,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHistoryServer {
    pub name: String,
    pub arch: String,
    pub dir_name: String,
    pub package_file: Option<String>,
    pub size_bytes: u64,
    pub images: Vec<BuildHistoryImage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHistoryImage {
    pub reference: String,
    pub digest: String,
    pub file: String,
}

fn artifact_root(app: &AppHandle) -> AppResult<PathBuf> {
    let s = store::effective_storage(app)?;
    Ok(PathBuf::from(s.artifact_root))
}

#[tauri::command]
pub fn list_build_history(app: AppHandle) -> AppResult<Vec<BuildHistoryEntry>> {
    let root = artifact_root(&app)?;
    let mut out = Vec::new();
    let projects = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for proj_dir in projects.flatten() {
        let pdir = proj_dir.path();
        if !pdir.is_dir() {
            continue;
        }
        let builds = match fs::read_dir(&pdir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for bdir in builds.flatten() {
            let bpath = bdir.path();
            if !bpath.is_dir() {
                continue;
            }
            let mf = bpath.join("build-manifest.json");
            let Ok(raw) = fs::read_to_string(&mf) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { continue };
            let servers: Vec<BuildHistoryServer> = v["servers"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|s| BuildHistoryServer {
                            name: s["name"].as_str().unwrap_or_default().into(),
                            arch: s["arch"].as_str().unwrap_or_default().into(),
                            dir_name: s["dirName"].as_str().unwrap_or_default().into(),
                            package_file: s["packageFile"].as_str().map(String::from),
                            size_bytes: s["sizeBytes"].as_u64().unwrap_or(0),
                            images: s["images"]
                                .as_array()
                                .map(|imgs| {
                                    imgs.iter()
                                        .map(|i| BuildHistoryImage {
                                            reference: i["reference"].as_str().unwrap_or_default().into(),
                                            digest: i["digest"].as_str().unwrap_or_default().into(),
                                            file: i["file"].as_str().unwrap_or_default().into(),
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push(BuildHistoryEntry {
                build_id: v["buildId"].as_str().unwrap_or_default().into(),
                project_id: v["projectId"].as_str().unwrap_or_default().into(),
                project_name: v["projectName"].as_str().unwrap_or_default().into(),
                generated_at: v["generatedAt"].as_str().unwrap_or_default().into(),
                kind: v["kind"].as_str().unwrap_or("full").into(),
                total_size_bytes: servers.iter().map(|s| s.size_bytes).sum(),
                dir: bpath.to_string_lossy().into_owned(),
                servers,
            });
        }
    }
    out.sort_by(|a, b| b.generated_at.cmp(&a.generated_at));
    Ok(out)
}

#[tauri::command]
pub fn delete_build(app: AppHandle, dir: String) -> AppResult<()> {
    let p = PathBuf::from(&dir);
    let root = artifact_root(&app)?;
    let p_canon = p
        .canonicalize()
        .map_err(|_| AppError::Invalid(format!("目录不存在: {dir}")))?;
    let root_canon = root.canonicalize()?;
    if p_canon.parent().and_then(|p| p.parent()) != Some(root_canon.as_path()) {
        return Err(AppError::Invalid("仅允许删除构建产物目录".into()));
    }
    fs::remove_dir_all(&p_canon)?;
    Ok(())
}

/// 资源管理器打开目录（现场交付常用）
#[tauri::command]
pub fn open_dir_in_explorer(dir: String) -> AppResult<()> {
    let p = PathBuf::from(&dir);
    if !p.is_dir() {
        return Err(AppError::Invalid(format!("目录不存在: {dir}")));
    }
    std::process::Command::new("explorer")
        .arg(p.as_os_str())
        .spawn()
        .map_err(|e| AppError::Io(format!("打开资源管理器失败: {e}")))?;
    Ok(())
}

/// 读取基线构建的 manifest（升级包对比用）。
/// 不按当前方案名定位目录（方案改名后目录名与当前名不一致），
/// 而是全库扫描 build-manifest.json 匹配 projectId+buildId。
pub fn read_baseline(
    app: &AppHandle,
    baseline_build_id: &str,
    project_id: &str,
) -> AppResult<serde_json::Value> {
    // buildId 净化（命令入参，防路径穿越）
    if baseline_build_id.is_empty()
        || !baseline_build_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::Invalid(format!("非法的构建号: {baseline_build_id}")));
    }
    let root = artifact_root(app)?;
    if let Ok(projects) = fs::read_dir(&root) {
        for proj in projects.flatten() {
            let pdir = proj.path();
            if !pdir.is_dir() {
                continue;
            }
            let mf = pdir.join(baseline_build_id).join("build-manifest.json");
            let Ok(raw) = fs::read_to_string(&mf) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { continue };
            if v["projectId"].as_str() == Some(project_id) {
                return Ok(v);
            }
        }
    }
    Err(AppError::NotFound(format!(
        "基线构建 {baseline_build_id} 不存在（若方案已改名或构建已删除，请重新选择基线）"
    )))
}
