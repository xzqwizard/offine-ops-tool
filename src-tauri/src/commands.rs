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
        template_snapshots: vec![],
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
    let target = if p.exists() {
        p
    } else {
        // 目标不存在时向上找存在的祖先（盘符根一定存在）
        p.ancestors()
            .skip(1)
            .find(|a| a.exists())
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

// ==================== Portable backup ====================
#[tauri::command]
pub fn backup_app_data(app: AppHandle, output_path: String) -> AppResult<String> {
    let _storage_lock = store::storage_lock(&app)?;
    let s = store::effective_storage(&app)?;
    let _lock = store::data_lock(&app)?;
    let r = crate::backup::backup(&s, std::path::Path::new(&output_path))?;
    crate::app_ops::__audit(&app, "backup_app_data", &output_path);
    Ok(r)
}
#[tauri::command]
pub fn restore_app_data(app: AppHandle, backup_path: String) -> AppResult<String> {
    let _storage_lock = store::storage_lock(&app)?;
    let s = store::effective_storage(&app)?;
    let _lock = store::data_lock(&app)?;
    let r = crate::backup::restore(&s, std::path::Path::new(&backup_path))?;
    crate::app_ops::__audit(&app, "restore_app_data", &backup_path);
    Ok(r)
}
#[tauri::command]
pub fn validate_project(
    app: AppHandle,
    project: Project,
) -> AppResult<Vec<crate::validation::ValidationIssue>> {
    let cat = crate::catalog::for_project(&app, &project)?;
    Ok(crate::validation::issues(&project, &cat, true))
}
