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
