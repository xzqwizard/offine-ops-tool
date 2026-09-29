use crate::error::{AppError, AppResult};
use crate::models::*;
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;

const SETTINGS_FILE: &str = "settings.json";
const PROJECT_FILE: &str = "project.json";

// ==================== 路径解析 ====================

/// 软件所在目录（便携式布局）：数据跟随程序目录而非用户目录，
/// 整个目录拷走即迁移。开发模式下位于 target/<profile>/。
fn app_base_dir() -> AppResult<PathBuf> {
    let exe = std::env::current_exe()
        .map_err(|e| AppError::Io(format!("无法定位程序位置: {e}")))?;
    exe.parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| AppError::Io("无法取得程序所在目录".into()))
}

/// 配置文件目录：<软件目录>/data/
fn config_dir(_app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app_base_dir()?.join("data"))
}

/// 未配置覆盖时的默认存储根（全部在软件所在目录的 data/ 下，便携可迁移）
struct DefaultRoots {
    projects_root: PathBuf,
    image_cache_root: PathBuf,
    docker_pkg_root: PathBuf,
    artifact_root: PathBuf,
    log_root: PathBuf,
}

fn default_roots(_app: &AppHandle) -> AppResult<DefaultRoots> {
    let data = app_base_dir()?.join("data");
    Ok(DefaultRoots {
        projects_root: data.join("projects"),
        image_cache_root: data.join("cache"),
        docker_pkg_root: data.join("cache").join("docker-pkgs"),
        artifact_root: data.join("dist"),
        log_root: data.join("logs"),
    })
}

fn override_or(ov: &Option<String>, default: PathBuf) -> PathBuf {
    match ov {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => default,
    }
}

/// 合并设置覆盖项与默认值，得到实际生效的存储路径
pub fn effective_storage(app: &AppHandle) -> AppResult<StorageInfo> {
    let settings = load_settings(app)?;
    let d = default_roots(app)?;
    Ok(StorageInfo {
        projects_root: override_or(&settings.projects_root, d.projects_root)
            .to_string_lossy()
            .into_owned(),
        image_cache_root: override_or(&settings.image_cache_root, d.image_cache_root)
            .to_string_lossy()
            .into_owned(),
        docker_pkg_root: override_or(&settings.docker_pkg_root, d.docker_pkg_root)
            .to_string_lossy()
            .into_owned(),
        artifact_root: override_or(&settings.artifact_root, d.artifact_root)
            .to_string_lossy()
            .into_owned(),
        log_root: override_or(&settings.log_root, d.log_root)
            .to_string_lossy()
            .into_owned(),
        config_dir: config_dir(app)?
            .to_string_lossy()
            .into_owned(),
    })
}

// ==================== 应用设置 ====================

fn settings_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(config_dir(app)?.join(SETTINGS_FILE))
}

pub fn load_settings(app: &AppHandle) -> AppResult<AppSettings> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_settings(app: &AppHandle, settings: &AppSettings) -> AppResult<()> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // 校验覆盖路径可创建（提前暴露权限/盘符问题）
    for p in [
        &settings.projects_root,
        &settings.image_cache_root,
        &settings.docker_pkg_root,
        &settings.artifact_root,
        &settings.log_root,
    ] {
        if let Some(p) = p {
            if !p.trim().is_empty() {
                fs::create_dir_all(p)?;
            }
        }
    }
    let raw = serde_json::to_string_pretty(settings)?;
    fs::write(&path, raw)?;
    Ok(())
}

// ==================== 方案存取 ====================

fn projects_root(app: &AppHandle) -> AppResult<PathBuf> {
    let s = effective_storage(app)?;
    Ok(PathBuf::from(s.projects_root))
}

fn project_dir(app: &AppHandle, id: &str) -> AppResult<PathBuf> {
    validate_id(id)?;
    Ok(projects_root(app)?.join(id))
}

fn project_path(app: &AppHandle, id: &str) -> AppResult<PathBuf> {
    Ok(project_dir(app, id)?.join(PROJECT_FILE))
}

/// 方案 id 仅允许字母数字与 _ -，防止路径穿越
pub fn validate_id(id: &str) -> AppResult<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(AppError::Invalid(format!("非法的方案 id: {id}")))
    }
}

pub fn list_projects(app: &AppHandle) -> AppResult<Vec<ProjectSummary>> {
    let root = projects_root(app)?;
    let mut out = Vec::new();
    let entries = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Ok(out), // 目录不存在视为空
    };
    for entry in entries.flatten() {
        let p = entry.path().join(PROJECT_FILE);
        if !p.is_file() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&p) else { continue };
        let Ok(proj) = serde_json::from_str::<Project>(&raw) else { continue };
        out.push(ProjectSummary {
            id: proj.id,
            name: proj.name,
            customer: proj.customer,
            updated_at: proj.updated_at,
            server_count: proj.servers.len(),
            instance_count: proj.instances.len(),
        });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

pub fn load_project(app: &AppHandle, id: &str) -> AppResult<Project> {
    let path = project_path(app, id)?;
    if !path.exists() {
        return Err(AppError::NotFound(format!("方案 {id} 不存在")));
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_project(app: &AppHandle, project: &Project) -> AppResult<()> {
    let dir = project_dir(app, &project.id)?;
    fs::create_dir_all(&dir)?;
    let raw = serde_json::to_string_pretty(project)?;
    // 先写临时文件再原子重命名，避免写一半损坏
    let tmp = dir.join(format!("{PROJECT_FILE}.tmp"));
    fs::write(&tmp, raw)?;
    fs::rename(&tmp, dir.join(PROJECT_FILE))?;
    Ok(())
}

pub fn delete_project(app: &AppHandle, id: &str) -> AppResult<()> {
    let dir = project_dir(app, id)?;
    if !dir.exists() {
        return Err(AppError::NotFound(format!("方案 {id} 不存在")));
    }
    fs::remove_dir_all(dir)?;
    Ok(())
}

/// 供创建/保存时生成时间戳
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 供创建时生成 id（proj-<uuid>）
pub fn new_project_id() -> String {
    format!("proj-{}", uuid::Uuid::new_v4().simple())
}
