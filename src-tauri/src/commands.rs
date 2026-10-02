use crate::error::AppResult;
use crate::models::*;
use crate::store;
use tauri::AppHandle;

#[tauri::command]
pub fn list_projects(app: AppHandle) -> AppResult<Vec<ProjectSummary>> {
    store::list_projects(&app)
}

/// 新建方案：生成 id 与时间戳，落盘后返回完整对象
#[tauri::command]
pub fn create_project(app: AppHandle, name: String, customer: String) -> AppResult<Project> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(crate::error::AppError::Invalid("方案名称不能为空".into()));
    }
    let now = store::now_rfc3339();
    let project = Project {
        schema_version: SCHEMA_VERSION,
        id: store::new_project_id(),
        name,
        customer: customer.trim().to_string(),
        created_at: now.clone(),
        updated_at: now,
        build_config: BuildConfig::default(),
        servers: Vec::new(),
        instances: Vec::new(),
        network_rules: Vec::new(),
        registry: None,
    };
    store::save_project(&app, &project)?;
    Ok(project)
}

/// 保存方案：服务端刷新 updated_at 后整体写入
#[tauri::command]
pub fn save_project(app: AppHandle, mut project: Project) -> AppResult<Project> {
    if project.name.trim().is_empty() {
        return Err(crate::error::AppError::Invalid("方案名称不能为空".into()));
    }
    store::validate_id(&project.id)?;
    project.updated_at = store::now_rfc3339();
    store::save_project(&app, &project)?;
    Ok(project)
}

#[tauri::command]
pub fn load_project(app: AppHandle, id: String) -> AppResult<Project> {
    store::load_project(&app, &id)
}

/// 复制方案：全部服务器/实例/规则保留，服务器重新分配 id 并清空 IP（待填），
/// 实例与访问规则的服务器引用同步重映射
#[tauri::command]
pub fn clone_project(app: AppHandle, id: String, new_name: String) -> AppResult<Project> {
    let name = new_name.trim().to_string();
    if name.is_empty() {
        return Err(crate::error::AppError::Invalid("新方案名称不能为空".into()));
    }
    let mut p = store::load_project(&app, &id)?;
    let now = store::now_rfc3339();
    p.id = store::new_project_id();
    p.name = name;
    p.created_at = now.clone();
    p.updated_at = now;

    let mut id_map = std::collections::HashMap::new();
    for s in &mut p.servers {
        let old = s.id.clone();
        s.id = format!("srv-{}", uuid::Uuid::new_v4().simple());
        id_map.insert(old, s.id.clone());
        s.ip = String::new(); // 新环境 IP 待填
    }
    for i in &mut p.instances {
        if let Some(n) = id_map.get(&i.server_id) {
            i.server_id = n.clone();
        }
    }
    let stale = p.network_rules.is_empty();
    for r in &mut p.network_rules {
        if let Some(n) = id_map.get(&r.from_server_id) {
            r.from_server_id = n.clone();
        }
        if let Some(n) = id_map.get(&r.to_server_id) {
            r.to_server_id = n.clone();
        }
    }
    let _ = stale;
    store::save_project(&app, &p)?;
    Ok(p)
}

#[tauri::command]
pub fn delete_project(app: AppHandle, id: String) -> AppResult<()> {
    store::delete_project(&app, &id)
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppResult<AppSettings> {
    store::load_settings(&app)
}

/// 保存设置并返回解析后的实际存储路径（便于前端立即展示）
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> AppResult<StorageInfo> {
    store::save_settings(&app, &settings)?;
    store::effective_storage(&app)
}

#[tauri::command]
pub fn get_storage_info(app: AppHandle) -> AppResult<StorageInfo> {
    store::effective_storage(&app)
}

// ==================== 磁盘空间 ====================

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpaceInfo {
    pub path: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// 查询存储根所在盘的剩余空间（构建前预检用）
#[tauri::command]
pub fn get_disk_space(_app: AppHandle, path: String) -> AppResult<DiskSpaceInfo> {
    use fs2::{available_space, total_space};
    let p = std::path::PathBuf::from(path.trim());
    let target = if p.exists() { p } else {
        // 目标不存在时向上找存在的祖先（盘符根一定存在）
        p.ancestors().skip(1).find(|a| a.exists())
            .ok_or_else(|| crate::error::AppError::Invalid(format!("路径无效: {path}")))?
            .to_path_buf()
    };
    let free = available_space(&target)
        .map_err(|e| crate::error::AppError::Io(format!("查询磁盘空间失败: {e}")))?;
    let total = total_space(&target).unwrap_or(0);
    Ok(DiskSpaceInfo {
        path: path.trim().to_string(),
        free_bytes: free,
        total_bytes: total,
    })
}

// ==================== 工具数据备份（方案/设置随包迁移，不含镜像缓存与产物） ====================

/// 备份工具数据（全部方案 + 设置；cache/dist 体积大且可再生，排除）。
/// 尊重存储设置：projects 用 effective_storage 的实际根（用户可能改到其他盘）。
#[tauri::command]
pub fn backup_app_data(app: AppHandle, output_path: String) -> AppResult<String> {
    let storage = store::effective_storage(&app)?;
    let projects_root = std::path::PathBuf::from(&storage.projects_root);
    let settings_file = std::path::PathBuf::from(&storage.config_dir).join("settings.json");
    let file = std::fs::File::create(&output_path)
        .map_err(|e| crate::error::AppError::Io(format!("创建备份文件失败: {e}")))?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut count = 0u32;
    if settings_file.is_file() {
        zip.start_file("settings.json", options)
            .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
        std::io::copy(&mut std::fs::File::open(&settings_file)?, &mut zip)?;
        count += 1;
    }
    if projects_root.is_dir() {
        add_dir_to_zip(&mut zip, &projects_root, "projects", options)?;
        count += 1;
    }
    zip.finish().map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    Ok(format!("已备份 {count} 项 → {output_path}"))
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    dir: &std::path::Path,
    prefix: &str,
    options: zip::write::SimpleFileOptions,
) -> AppResult<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        if path.is_dir() {
            add_dir_to_zip(zip, &path, &name, options)?;
        } else {
            zip.start_file(&name, options)
                .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
            std::io::copy(&mut std::fs::File::open(&path)?, zip)?;
        }
    }
    Ok(())
}

/// 从备份恢复（覆盖式；恢复后需重启应用生效）。
/// 恢复目标同样按 effective_storage 解析（与备份对称）。
#[tauri::command]
pub fn restore_app_data(app: AppHandle, backup_path: String) -> AppResult<String> {
    let storage = store::effective_storage(&app)?;
    let settings_file = std::path::PathBuf::from(&storage.config_dir).join("settings.json");
    let projects_root = std::path::PathBuf::from(&storage.projects_root);
    let file = std::fs::File::open(&backup_path)
        .map_err(|e| crate::error::AppError::Io(format!("打开备份失败: {e}")))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| crate::error::AppError::Io(format!("备份文件格式错误: {e}")))?;
    let mut count = 0u32;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        // 路径穿越防护：条目必须是纯相对路径——
        // 拒绝 ..、反斜杠、绝对路径（join 绝对路径会整体替换 base）
        let name = entry.name().to_string();
        if name.contains("..") || name.contains('\\') || std::path::Path::new(&name).is_absolute() {
            continue;
        }
        let (out_path, parent): (std::path::PathBuf, Option<std::path::PathBuf>) =
            match name.strip_prefix("projects/") {
                Some(rest) => {
                    (projects_root.join(rest), Some(projects_root.clone()))
                }
                None if name == "settings.json" => {
                    (settings_file.clone(), settings_file.parent().map(|p| p.to_path_buf()))
                }
                None => continue, // 未知条目跳过
            };
        if let Some(parent) = parent {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        count += 1;
    }
    Ok(format!("已恢复 {count} 个文件（重启应用后生效）"))
}
