use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

// ==================== 方案（Project） ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub customer: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub build_config: BuildConfig,
    #[serde(default)]
    pub servers: Vec<ServerInfo>,
    #[serde(default)]
    pub instances: Vec<MiddlewareInstance>,
    #[serde(default)]
    pub network_rules: Vec<NetworkRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildConfig {
    /// tar.gz | zip | tar | dir
    pub package_format: String,
    /// 镜像 tar 是否参与二次压缩（默认否，docker save 层已压缩）
    pub recompress_images: bool,
    /// 是否 GPG 签名
    pub sign: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            package_format: "tar.gz".into(),
            recompress_images: false,
            sign: false,
        }
    }
}

/// 方案列表条目（不含明细，避免读取大文件时反序列化全量字段）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub customer: String,
    pub updated_at: String,
    pub server_count: usize,
    pub instance_count: usize,
}

// ==================== 服务器 ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub ip: String,
    /// kylin | uos | openeuler | centos | rhel | rocky | ubuntu | debian | deepin | neokylin ...
    #[serde(default = "default_os_family")]
    pub os_family: String,
    #[serde(default)]
    pub os_version: String,
    /// amd64 | arm64 | loongarch64 | mips64el | sw64
    pub arch: String,
    pub bits: u32,
    #[serde(default)]
    pub cpu_cores: u32,
    #[serde(default)]
    pub memory_gb: u32,
    #[serde(default)]
    pub disk_system_gb: u32,
    #[serde(default)]
    pub disk_data_gb: u32,
    #[serde(default = "default_docker_version")]
    pub docker_version: String,
    #[serde(default = "default_docker_data_root")]
    pub docker_data_root: String,
    #[serde(default = "default_deploy_base_dir")]
    pub deploy_base_dir: String,
}

fn default_os_family() -> String {
    "kylin".into()
}
fn default_docker_version() -> String {
    "27.5.1".into()
}
fn default_docker_data_root() -> String {
    "/data/docker".into()
}
fn default_deploy_base_dir() -> String {
    "/opt/stack".into()
}

// ==================== 中间件实例 ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortBinding {
    pub name: String,
    pub host: u32,
    pub container: u32,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default = "default_true")]
    pub expose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareInstance {
    pub id: String,
    pub server_id: String,
    /// 目录条目 id；手动输入镜像时为 "custom"
    pub template_id: String,
    /// 完整镜像引用（registry/repo:tag，可含 @sha256:）
    pub image: String,
    #[serde(default)]
    pub digest: String,
    /// 容器/服务名（compose service name）
    pub instance_name: String,
    /// 模板驱动的自由参数（端口外的密码、目录、内存限额等）
    #[serde(default)]
    pub params: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub ports: Vec<PortBinding>,
    /// M0：本地镜像 tar 绝对路径（docker save 产物，构建时直接复制）
    #[serde(default)]
    pub local_image_tar: String,
}

fn default_protocol() -> String {
    "tcp".into()
}
fn default_true() -> bool {
    true
}

// ==================== 网络访问规则 ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRule {
    pub id: String,
    pub from_server_id: String,
    pub to_server_id: String,
    pub to_port: u32,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub description: String,
}

// ==================== 应用设置（存储根可自定义） ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub schema_version: u32,
    /// None = 使用默认值；设置后为覆盖路径
    pub projects_root: Option<String>,
    pub image_cache_root: Option<String>,
    pub docker_pkg_root: Option<String>,
    pub artifact_root: Option<String>,
    pub log_root: Option<String>,
    /// 镜像源列表（有序，失败自动切换）
    pub registry_mirrors: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            projects_root: None,
            image_cache_root: None,
            docker_pkg_root: None,
            artifact_root: None,
            log_root: None,
            registry_mirrors: vec![
                "docker.io".into(),
                "docker.m.daocloud.io".into(),
                "docker.1ms.run".into(),
            ],
        }
    }
}

/// 解析后的实际存储路径（返回给前端展示与校验）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub projects_root: String,
    pub image_cache_root: String,
    pub docker_pkg_root: String,
    pub artifact_root: String,
    pub log_root: String,
    pub config_dir: String,
}
