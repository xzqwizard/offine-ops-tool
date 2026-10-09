use crate::error::{AppError, AppResult};
use crate::models::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "settings.json";
const PROJECT_FILE: &str = "project.json";

// ==================== 路径解析 ====================

/// 软件所在目录（便携式布局）：数据跟随程序目录而非用户目录，
/// 整个目录拷走即迁移。开发模式下位于 target/<profile>/。
fn app_base_dir() -> AppResult<PathBuf> {
    let exe =
        std::env::current_exe().map_err(|e| AppError::Io(format!("无法定位程序位置: {e}")))?;
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

// ==================== 旧版数据迁移（用户目录 → 软件目录 data/） ====================

static MIGRATED: OnceLock<()> = OnceLock::new();

/// 首次访问数据前执行一次旧版迁移（幂等、非破坏性：只复制不覆盖、不删除旧数据）。
/// get_or_init 保证并发安全：首个调用者执行迁移，其余等待完成后直接返回。
/// 旧位置：Windows %APPDATA%/<identifier>/（settings.json、projects/、cache/）
pub fn ensure_migrated(app: &AppHandle) {
    MIGRATED.get_or_init(|| {
        if let Err(e) = migrate_legacy_data(app).and_then(|_| migrate_credentials_at_rest(app)) {
            eprintln!("[迁移] 旧版数据迁移失败（不影响使用，可手动复制）: {e}");
        }
    });
}

fn migrate_legacy_data(app: &AppHandle) -> AppResult<()> {
    let legacy = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Io(format!("无法定位旧版数据目录: {e}")))?;
    if !legacy.is_dir() {
        return Ok(()); // 无旧数据
    }
    let new_data = app_base_dir()?.join("data");
    fs::create_dir_all(&new_data)?;

    // 1) 设置：仅当新位置尚无 settings.json 时迁移
    let legacy_settings = legacy.join(SETTINGS_FILE);
    let new_settings = new_data.join(SETTINGS_FILE);
    if legacy_settings.is_file() && !new_settings.is_file() {
        fs::copy(&legacy_settings, &new_settings)?;
    }

    // 2) 方案与镜像缓存：文件级合并且不覆盖已存在文件
    merge_dir_recursive(&legacy.join("projects"), &new_data.join("projects"))?;
    merge_dir_recursive(&legacy.join("cache"), &new_data.join("cache"))?;
    Ok(())
}

/// 递归合并目录：src 中存在而 dst 中不存在的文件复制过去；已存在一律跳过。
/// 返回 (复制文件数, 复制字节数)。src 不存在时返回 (0, 0)。
pub fn merge_dir_recursive(src: &Path, dst: &Path) -> AppResult<(usize, u64)> {
    if !src.is_dir() {
        return Ok((0, 0));
    }
    fs::create_dir_all(dst)?;
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let name = entry.file_name();
        // 跳过临时/下载中文件与日志
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str.ends_with(".downloading") {
            continue;
        }
        let d = dst.join(&name);
        if entry.file_type()?.is_symlink() {
            continue;
        }
        if s.is_dir() {
            let (f, b) = merge_dir_recursive(&s, &d)?;
            files += f;
            bytes += b;
        } else if !d.exists() {
            fs::copy(&s, &d)?;
            files += 1;
            bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok((files, bytes))
}

/// 合并设置覆盖项与默认值，得到实际生效的存储路径
pub fn effective_storage(app: &AppHandle) -> AppResult<StorageInfo> {
    ensure_migrated(app);
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
        config_dir: config_dir(app)?.to_string_lossy().into_owned(),
    })
}

// ==================== 应用设置 ====================

pub fn settings_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(config_dir(app)?.join(SETTINGS_FILE))
}

pub fn load_settings(app: &AppHandle) -> AppResult<AppSettings> {
    ensure_migrated(app);
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let raw = fs::read_to_string(&path)?;
    let mut value: AppSettings = serde_json::from_str(&raw)?;
    if let Some(p) = &mut value.proxy {
        if let Some(pw) = &mut p.password {
            *pw = crate::credentials::reveal(pw)?;
        }
    }
    Ok(value)
}

pub fn save_settings(app: &AppHandle, settings: &AppSettings) -> AppResult<()> {
    let _storage_lock = storage_lock(app)?;
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
    ]
    .into_iter()
    .flatten()
    {
        if !p.trim().is_empty() {
            fs::create_dir_all(p)?;
        }
    }
    let _lock = data_lock(app)?;
    let mut persisted = settings.clone();
    for m in &mut persisted.registry_mirrors {
        *m = crate::validation::normalize_host(m)?;
    }
    if let Some(p) = &mut persisted.proxy {
        if p.enabled
            && (p.port == 0
                || p.host.is_empty()
                || !["http", "socks5"].contains(&p.scheme.as_str()))
        {
            return Err(AppError::Invalid("代理配置无效".into()));
        }
        if let Some(pw) = &mut p.password {
            *pw = crate::credentials::protect(pw)?;
        }
    }
    crate::io_util::atomic_write(&path, serde_json::to_string_pretty(&persisted)?.as_bytes())?;
    crate::app_ops::__audit(app, "save_settings", "settings");
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
    if ok
        && ![
            "con", "prn", "nul", "aux", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ]
        .contains(&id.to_ascii_lowercase().as_str())
    {
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let p = entry.path().join(PROJECT_FILE);
        if !p.is_file() {
            continue;
        }
        let raw = fs::read_to_string(&p)?;
        let proj: Project = serde_json::from_str(&raw)
            .map_err(|e| AppError::Invalid(format!("方案文件 {} 损坏: {e}", p.display())))?;
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
    let mut p: Project = serde_json::from_str(&fs::read_to_string(path)?)?;
    crate::credentials::reveal_project(&mut p)?;
    Ok(p)
}

pub fn data_lock(app: &AppHandle) -> AppResult<std::fs::File> {
    crate::io_util::lock(&config_dir(app)?.join(".data.lock"))
}
pub fn storage_lock(app: &AppHandle) -> AppResult<std::fs::File> {
    crate::io_util::lock(&config_dir(app)?.join(".storage.lock"))
}

pub fn save_project(app: &AppHandle, project: &Project) -> AppResult<Project> {
    let cat = crate::catalog::for_project(app, project)?;
    crate::validation::require(project, &cat, false)?;
    let mut frozen = crate::catalog::freeze(app, project)?;
    frozen.instances = project.instances.clone();
    frozen.registry = project.registry.clone();
    let _lock = data_lock(app)?;
    let path = project_path(app, &project.id)?;
    let mut p = frozen;
    if let Some(r) = &mut p.registry {
        if !r.url.is_empty() {
            r.url = crate::validation::normalize_host(&r.url)?;
        }
    }
    let canonical = p.clone();
    crate::credentials::protect_project(&mut p, &cat)?;
    crate::io_util::atomic_write(&path, serde_json::to_string_pretty(&p)?.as_bytes())?;
    crate::app_ops::__audit(app, "save_project", &project.id);
    Ok(canonical)
}

pub fn delete_project(app: &AppHandle, id: &str) -> AppResult<()> {
    let _lock = data_lock(app)?;
    let dir = project_dir(app, id)?;
    if !dir.exists() {
        return Err(AppError::NotFound(format!("方案 {id} 不存在")));
    }
    fs::remove_dir_all(dir)?;
    crate::app_ops::__audit(app, "delete_project", id);
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

fn migrate_credentials_at_rest(app: &AppHandle) -> AppResult<()> {
    let data = config_dir(app)?;
    let _lock = crate::io_util::lock(&data.join(".data.lock"))?;
    let mut settings = AppSettings::default();
    let path = data.join(SETTINGS_FILE);
    if path.exists() {
        settings = serde_json::from_slice(&fs::read(&path)?)?;
        if let Some(p) = &mut settings.proxy {
            if let Some(pw) = &mut p.password {
                if !pw.is_empty() && !pw.starts_with("dpapi:v1:") {
                    *pw = crate::credentials::protect(&crate::credentials::reveal(pw)?)?;
                    crate::io_util::atomic_write(
                        &path,
                        serde_json::to_string_pretty(&settings)?.as_bytes(),
                    )?;
                }
            }
        }
    }
    let mut catalog = crate::catalog::preset()?;
    let custom_path = data.join("custom-catalog.json");
    if custom_path.exists() {
        let mut custom: crate::catalog::CatalogFile =
            serde_json::from_slice(&fs::read(&custom_path)?)?;
        crate::catalog::validate_catalog(&custom)?;
        let before = serde_json::to_vec(&custom)?;
        crate::catalog::scrub_catalog_defaults(&mut custom);
        if serde_json::to_vec(&custom)? != before {
            crate::io_util::atomic_write(
                &custom_path,
                serde_json::to_string_pretty(&custom)?.as_bytes(),
            )?;
        }
        for t in custom.templates {
            catalog.templates.retain(|x| x.id != t.id);
            catalog.templates.push(t);
        }
    }
    let root = override_or(&settings.projects_root, data.join("projects"));
    if root.is_dir() {
        for e in fs::read_dir(root)? {
            let e = e?;
            if !e.file_type()?.is_dir() {
                continue;
            }
            let path = e.path().join(PROJECT_FILE);
            if !path.exists() {
                continue;
            }
            let mut p: Project = serde_json::from_slice(&fs::read(&path)?)?;
            let before = serde_json::to_vec(&p)?;
            let cat = crate::catalog::overlay(catalog.clone(), &p);
            crate::credentials::migrate_project(&mut p, &cat)?;
            if serde_json::to_vec(&p)? != before {
                crate::io_util::atomic_write(&path, serde_json::to_string_pretty(&p)?.as_bytes())?;
            }
        }
    }
    Ok(())
}
