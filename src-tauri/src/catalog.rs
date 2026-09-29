use crate::error::AppResult;
use serde::{Deserialize, Serialize};

/// 中间件目录（M0：内嵌随应用发布；M1 支持远端目录更新）
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// compose command 覆盖（如 redis 官方镜像设置密码：sh -c 'redis-server --requirepass "$$REDIS_PASSWORD"'）
    #[serde(default)]
    pub command: Vec<String>,
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

#[tauri::command]
pub fn list_catalog() -> AppResult<CatalogFile> {
    preset()
}
