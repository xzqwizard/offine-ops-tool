use crate::error::AppResult;
use serde::{Deserialize, Serialize};

/// 中间件目录（M0：内嵌随应用发布；M1 支持远端目录更新）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFile {
    pub version: u32,
    pub updated_at: String,
    pub templates: Vec<MiddlewareTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareTemplate {
    pub id: String,
    pub display_name: String,
    pub category: String,
    pub default_image: String,
    #[serde(default)]
    pub recommended_tags: Vec<String>,
    #[serde(default)]
    pub supported_arches: Vec<String>,
    #[serde(default)]
    pub ports: Vec<TemplatePort>,
    #[serde(default)]
    pub env_hints: Vec<EnvHint>,
    #[serde(default)]
    pub data_volume: String,
    /// 数据卷属主（非 root 镜像必须，如 ES 为 "1000:1000"、bitnami 为 "1001:1001"）
    #[serde(default)]
    pub data_user: Option<String>,
    /// compose command 覆盖（如 redis 官方镜像设置密码：sh -c 'redis-server --requirepass "$REDIS_PASSWORD"'）
    #[serde(default)]
    pub command: Vec<String>,
    /// 同机依赖的其它中间件 templateId（compose depends_on；跨机依赖用端口矩阵）
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub health_check: Option<HealthCheck>,
    /// 健康检查超时秒数（deploy.sh 等待时长，默认 60）
    #[serde(default)]
    pub health_timeout_sec: Option<u32>,
    #[serde(default)]
    pub min_memory_gb: f64,
    /// 内核参数要求（如 ES 的 vm.max_map_count>=262144），驱动 precheck
    #[serde(default)]
    pub kernel_reqs: Vec<String>,
    #[serde(default)]
    pub remark: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePort {
    pub name: String,
    pub container: u32,
    pub default_host: u32,
    #[serde(default = "default_tcp")]
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvHint {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    /// exec | tcp
    pub r#type: String,
    #[serde(default)]
    pub cmd: Vec<String>,
}

fn default_tcp() -> String {
    "tcp".into()
}

const PRESET: &str = include_str!("../../catalog/preset.json");

/// 内置目录（builder 复用）
pub fn preset() -> AppResult<CatalogFile> {
    Ok(serde_json::from_str(PRESET)?)
}

// ==================== 自定义中间件 + 远程目录 ====================

use crate::error::AppError;
use crate::store;
use std::path::PathBuf;
use tauri::AppHandle;

/// 自定义目录文件（用户保存的"我的中间件" + 拉取的远程目录），自定义条目覆盖内置同 id
fn custom_catalog_path(app: &AppHandle) -> AppResult<PathBuf> {
    let s = store::effective_storage(app)?;
    Ok(PathBuf::from(s.projects_root).join("../custom-catalog.json"))
}

fn load_custom(app: &AppHandle) -> CatalogFile {
    custom_catalog_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_custom(app: &AppHandle, cat: &CatalogFile) -> AppResult<()> {
    let p = custom_catalog_path(app)?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&p, serde_json::to_string_pretty(cat)?)?;
    Ok(())
}

use std::fs;

/// 目录 = 内置 + 自定义（自定义同 id 覆盖内置；远程目录条目在拉取时合入自定义）
#[tauri::command]
pub fn list_catalog(app: AppHandle) -> AppResult<CatalogFile> {
    let mut merged = preset()?;
    let custom = load_custom(&app);
    for t in custom.templates {
        if let Some(builtin) = merged.templates.iter_mut().find(|b| b.id == t.id) {
            *builtin = t.clone(); // 覆盖
        } else {
            merged.templates.push(t);
        }
    }
    if !custom.updated_at.is_empty() {
        merged.updated_at = custom.updated_at;
    }
    Ok(merged)
}

/// 保存自定义中间件条目（手动输入的镜像存为"我的中间件"；同 id 覆盖）
#[tauri::command]
pub fn save_custom_template(
    app: AppHandle,
    template: MiddlewareTemplate,
    category: String,
) -> AppResult<CatalogFile> {
    let mut t = template;
    if !category.trim().is_empty() {
        t.category = category.trim().to_string();
    }
    let tid = t.id.clone();
    let mut custom = load_custom(&app);
    if let Some(existing) = custom.templates.iter_mut().find(|x| x.id == t.id) {
        *existing = t.clone();
    } else {
        custom.templates.push(t);
    }
    custom.updated_at = store::now_rfc3339();
    save_custom(&app, &custom)?;
    crate::app_ops::__audit(&app, "save_custom_template", &tid);
    list_catalog(app)
}

/// 删除自定义条目（仅自定义目录内存在的 id 可删）
#[tauri::command]
pub fn delete_custom_template(app: AppHandle, id: String) -> AppResult<CatalogFile> {
    let mut custom = load_custom(&app);
    let before = custom.templates.len();
    custom.templates.retain(|t| t.id != id);
    if custom.templates.len() == before {
        return Err(AppError::Invalid(format!("「{id}」不是自定义条目（内置条目不可删除）")));
    }
    save_custom(&app, &custom)?;
    list_catalog(app)
}

/// 从远程 URL 拉取目录更新（校验可解析后合入自定义层，覆盖同 id 条目）
#[tauri::command]
pub async fn fetch_remote_catalog(app: AppHandle, url: String) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = store::load_settings(&app)?;
        let raw = crate::net::http_get_with_settings(&settings, url.trim(), 30)?;
        let remote: CatalogFile = serde_json::from_str(&raw)
            .map_err(|e| AppError::Serialize(format!("远程目录格式无效: {e}")))?;
        let mut custom = load_custom(&app);
        let mut added = 0u32;
        let mut updated = 0u32;
        for t in remote.templates {
            let id = t.id.clone();
            if custom.templates.iter().any(|x| x.id == id) {
                updated += 1;
            } else {
                added += 1;
            }
            custom.templates.retain(|x| x.id != id);
            custom.templates.push(t);
        }
        custom.updated_at = store::now_rfc3339();
        custom.version = remote.version;
        save_custom(&app, &custom)?;
        let msg = format!("目录更新成功：新增 {added} 项，更新 {updated} 项");
        crate::app_ops::__audit(&app, "fetch_remote_catalog", &msg);
        Ok(msg)
    })
    .await
    .map_err(|e| AppError::Io(format!("任务异常: {e}")))?
}
