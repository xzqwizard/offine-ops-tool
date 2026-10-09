use crate::{
    catalog::{CatalogFile, MiddlewareTemplate},
    error::{AppError, AppResult},
    models::*,
    store,
};
use serde::Serialize;
use std::{collections::HashSet, net::Ipv4Addr};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub code: String,
    pub level: String,
    pub message: String,
}
fn issue(out: &mut Vec<ValidationIssue>, rule: &str, message: String) {
    out.push(ValidationIssue {
        code: rule.into(),
        level: "error".into(),
        message,
    });
}
pub fn env_key(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && c.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}
pub fn service_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
pub fn linux_path(s: &str) -> bool {
    s.starts_with('/')
        && s != "/"
        && !s.split('/').any(|p| p == "..")
        && !s.chars().any(|c| c.is_control() || c == '\0')
}
pub fn normalize_host(s: &str) -> AppResult<String> {
    let s = s
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if s.is_empty()
        || s.starts_with('-')
        || s.contains("..")
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c))
    {
        return Err(AppError::Invalid(format!("仓库地址应为 host[:port]: {s}")));
    }
    Ok(s.to_ascii_lowercase())
}
pub fn validate_template(t: &MiddlewareTemplate) -> AppResult<()> {
    if t.id.is_empty()
        || t.id.len() > 128
        || !t
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return Err(AppError::Invalid("模板 ID 无效".into()));
    }
    crate::images::parse_reference(&t.default_image)?;
    if !t.data_volume.is_empty() && !linux_path(&t.data_volume) {
        return Err(AppError::Invalid("模板数据卷必须为绝对 Linux 路径".into()));
    }
    if t.env_hints.iter().any(|e| !env_key(&e.key))
        || t.ports.iter().any(|p| {
            !(1..=65535).contains(&p.container)
                || !(1..=65535).contains(&p.default_host)
                || !["tcp", "udp"].contains(&p.protocol.as_str())
        })
    {
        return Err(AppError::Invalid("模板环境变量或端口无效".into()));
    }
    if t.data_user
        .as_ref()
        .is_some_and(|u| u.is_empty() || !u.chars().all(|c| c.is_ascii_digit() || c == ':'))
    {
        return Err(AppError::Invalid("数据属主必须为数字 UID[:GID]".into()));
    }
    if t.health_check.as_ref().is_some_and(|h| {
        !["tcp", "exec"].contains(&h.r#type.as_str()) || (h.r#type == "exec" && h.cmd.is_empty())
    }) {
        return Err(AppError::Invalid("健康检查类型/命令无效".into()));
    }
    if t.kernel_reqs.iter().any(|r| {
        r.split_once(">=").is_none_or(|(k, v)| {
            k.is_empty()
                || !k
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                || v.is_empty()
                || !v.chars().all(|c| c.is_ascii_digit())
        })
    }) {
        return Err(AppError::Invalid(
            "内核要求格式必须为 sysctl.key>=数字".into(),
        ));
    }
    if !t.min_memory_gb.is_finite()
        || t.min_memory_gb < 0.0
        || t.health_timeout_sec.is_some_and(|s| s == 0 || s > 3600)
    {
        return Err(AppError::Invalid("模板资源/健康检查超时无效".into()));
    }
    Ok(())
}
pub fn issues(p: &Project, cat: &CatalogFile, build: bool) -> Vec<ValidationIssue> {
    let mut out = vec![];
    if p.schema_version != SCHEMA_VERSION
        || store::validate_id(&p.id).is_err()
        || p.name.trim().is_empty()
        || p.name.chars().any(char::is_control)
    {
        issue(&mut out, "project", "方案版本、ID 或名称无效".into());
    }
    if !["dir", "tar.gz"].contains(&p.build_config.package_format.as_str()) || p.build_config.sign {
        issue(
            &mut out,
            "buildConfig",
            "只支持 dir/tar.gz；签名功能尚未实现，不能启用".into(),
        );
    }
    let mut ids = HashSet::new();
    let mut addresses = HashSet::new();
    let mut server_names = HashSet::new();
    let mut hostnames = HashSet::new();
    if crate::catalog::validate_catalog(cat).is_err() {
        issue(&mut out, "catalog", "目录含无效或重复模板".into());
    }
    let mut snapshot_ids = HashSet::new();
    if p.template_snapshots
        .iter()
        .any(|t| !snapshot_ids.insert(&t.id) || validate_template(t).is_err())
    {
        issue(&mut out, "snapshots", "冻结模板无效或 ID 重复".into());
    }
    for s in &p.servers {
        if store::validate_id(&s.id).is_err() || !ids.insert(&s.id) {
            issue(
                &mut out,
                "serverId",
                format!("服务器 {} ID 无效或重复", s.name),
            );
        }
        if [&s.name, &s.hostname, &s.os_family, &s.os_version, &s.ip]
            .iter()
            .any(|v| v.chars().any(char::is_control))
        {
            issue(
                &mut out,
                "serverText",
                format!("服务器 {} 字段含控制字符", s.name),
            );
        }
        if s.name.trim().is_empty()
            || !server_names.insert(s.name.trim().to_lowercase())
            || (!s.hostname.is_empty()
                && (!hostnames.insert(s.hostname.to_lowercase())
                    || s.hostname.len() > 253
                    || !s.hostname.split('.').all(|label| {
                        !label.is_empty()
                            && label.len() <= 63
                            && !label.starts_with('-')
                            && !label.ends_with('-')
                            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                    })))
            || !["amd64", "arm64", "loongarch64", "mips64el", "sw64"].contains(&s.arch.as_str())
            || s.bits != 64
        {
            issue(
                &mut out,
                "server",
                format!("服务器 {} 名称/架构/位数无效", s.name),
            );
        }
        if (build || !s.ip.is_empty())
            && (s.ip.parse::<Ipv4Addr>().is_err() || !addresses.insert(&s.ip))
        {
            issue(
                &mut out,
                "ip",
                format!("服务器 {} 需要唯一、合法的 IPv4 地址", s.name),
            );
        }
        if !linux_path(&s.deploy_base_dir)
            || !linux_path(&s.docker_data_root)
            || s.deploy_base_dir
                .split('/')
                .any(|v| v.is_empty() && s.deploy_base_dir == "/")
        {
            issue(
                &mut out,
                "path",
                format!("服务器 {} 数据/部署目录无效", s.name),
            );
        }
        if s.docker_version.is_empty()
            || !s
                .docker_version
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.')
            || (build && (s.os_family.is_empty() || s.os_version.is_empty()))
        {
            issue(
                &mut out,
                "os",
                format!("服务器 {} OS/Docker 版本未填写或无效", s.name),
            );
        }
    }
    if build {
        for s in &p.servers {
            if !p.instances.iter().any(|i| i.server_id == s.id) {
                issue(
                    &mut out,
                    "serverEmpty",
                    format!("服务器 {} 没有服务，无法生成可部署的 Compose 包", s.name),
                );
            }
            let need: f64 = p
                .instances
                .iter()
                .filter(|i| i.server_id == s.id)
                .map(|i| {
                    cat.templates
                        .iter()
                        .find(|t| t.id == i.template_id)
                        .map_or(0.5, |t| t.min_memory_gb)
                })
                .sum();
            if (s.memory_gb as f64) < need {
                issue(
                    &mut out,
                    "memory",
                    format!(
                        "服务器 {} 内存 {}GB 低于最低需求 {need}GB",
                        s.name, s.memory_gb
                    ),
                );
            }
        }
    }
    let mut instance_ids = HashSet::new();
    let mut names = HashSet::new();
    let mut ports = HashSet::new();
    for i in &p.instances {
        if store::validate_id(&i.id).is_err()
            || !instance_ids.insert(&i.id)
            || !ids.contains(&i.server_id)
            || !service_name(&i.instance_name)
            || !names.insert((
                &i.server_id,
                i.instance_name.replace('_', "-").to_ascii_lowercase(),
            ))
        {
            issue(
                &mut out,
                "instance",
                format!("实例 {} 的 ID/服务器引用/服务名无效或重复", i.instance_name),
            );
        }
        if crate::images::parse_reference(&i.image).is_err()
            || (!i.digest.is_empty() && !crate::images::valid_digest(&i.digest))
        {
            issue(
                &mut out,
                "image",
                format!("实例 {} 镜像引用或摘要无效", i.instance_name),
            );
        }
        if i.params.iter().any(|(k, v)| {
            !env_key(k)
                || (!v.is_string() && !v.is_number() && !v.is_boolean() && !v.is_null())
                || v.as_str().is_some_and(|v| v.contains('\0'))
        }) {
            issue(
                &mut out,
                "env",
                format!("实例 {} 环境变量键/值无效", i.instance_name),
            );
        }
        for port in &i.ports {
            if !["tcp", "udp"].contains(&port.protocol.as_str())
                || !(1..=65535).contains(&port.container)
                || (port.expose
                    && (!(1..=65535).contains(&port.host)
                        || !ports.insert((&i.server_id, port.host, &port.protocol))))
            {
                issue(
                    &mut out,
                    "port",
                    format!("实例 {} 端口无效或冲突", i.instance_name),
                );
            }
        }
        if i.template_id != "custom" {
            match cat.templates.iter().find(|t| t.id == i.template_id) {
                None => issue(
                    &mut out,
                    "template",
                    format!("实例 {} 的模板 {} 不存在", i.instance_name, i.template_id),
                ),
                Some(t) => {
                    if let Err(e) = validate_template(t) {
                        issue(&mut out, "template", e.to_string());
                    }
                    if build {
                        let arch = p
                            .servers
                            .iter()
                            .find(|s| s.id == i.server_id)
                            .map(|s| s.arch.as_str())
                            .unwrap_or("");
                        if !t.supported_arches.is_empty()
                            && !t.supported_arches.iter().any(|a| {
                                crate::image_archive::normalize_arch(a)
                                    == crate::image_archive::normalize_arch(arch)
                            })
                        {
                            issue(
                                &mut out,
                                "arch",
                                format!("{} 模板不支持 {arch}", i.instance_name),
                            );
                        }
                        for h in &t.env_hints {
                            if h.required
                                && i.params
                                    .get(&h.key)
                                    .is_none_or(|v| v.is_null() || v.as_str() == Some(""))
                            {
                                issue(
                                    &mut out,
                                    "required",
                                    format!("{} 缺少参数 {}", i.instance_name, h.key),
                                );
                            }
                        }
                        if t.health_check.as_ref().is_some_and(|h| h.r#type == "tcp")
                            && !i.ports.iter().any(|p| p.expose && p.protocol == "tcp")
                        {
                            issue(
                                &mut out,
                                "health",
                                format!("{} 的 TCP 健康检查需要发布 TCP 端口", i.instance_name),
                            );
                        }
                        let param =
                            |key: &str| i.params.get(key).and_then(|v| v.as_str()).unwrap_or("");
                        if i.template_id.starts_with("mysql-")
                            && (param("MYSQL_USER").is_empty()
                                != param("MYSQL_PASSWORD").is_empty())
                        {
                            issue(
                                &mut out,
                                "mysqlCredentials",
                                format!("{} 业务账号和密码需成组填写", i.instance_name),
                            );
                        }
                        if i.template_id == "minio"
                            && (param("MINIO_ROOT_USER").len() < 3
                                || param("MINIO_ROOT_PASSWORD").len() < 8)
                        {
                            issue(
                                &mut out,
                                "minioCredentials",
                                format!(
                                    "{} MinIO 账号至少 3 字符，密码至少 8 字符",
                                    i.instance_name
                                ),
                            );
                        }
                        if i.template_id == "nacos-2" {
                            let http = i.ports.iter().find(|p| p.container == 8848 && p.expose);
                            let grpc = i.ports.iter().find(|p| p.container == 9848 && p.expose);
                            if http.zip(grpc).is_none_or(|(h, g)| g.host != h.host + 1000) {
                                issue(
                                    &mut out,
                                    "nacosGrpc",
                                    format!(
                                        "{} Nacos HTTP/gRPC 宿主端口必须相差 1000",
                                        i.instance_name
                                    ),
                                );
                            }
                        }
                        for dep in &t.depends_on {
                            if p.instances
                                .iter()
                                .filter(|o| {
                                    o.server_id == i.server_id
                                        && o.template_id == *dep
                                        && o.id != i.id
                                })
                                .count()
                                != 1
                            {
                                issue(
                                    &mut out,
                                    "depends",
                                    format!(
                                        "{} 同机依赖 {dep} 必须恰好有一个候选实例",
                                        i.instance_name
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    if build {
        let mut remaining: HashSet<&str> = p.instances.iter().map(|i| i.id.as_str()).collect();
        loop {
            let ready: Vec<_> = p
                .instances
                .iter()
                .filter(|i| remaining.contains(i.id.as_str()))
                .filter(|i| {
                    cat.templates
                        .iter()
                        .find(|t| t.id == i.template_id)
                        .is_none_or(|t| {
                            t.depends_on.iter().all(|dep| {
                                !p.instances.iter().any(|o| {
                                    o.server_id == i.server_id
                                        && o.template_id == *dep
                                        && remaining.contains(o.id.as_str())
                                })
                            })
                        })
                })
                .map(|i| i.id.as_str())
                .collect();
            if ready.is_empty() {
                break;
            }
            for id in ready {
                remaining.remove(id);
            }
        }
        if !remaining.is_empty() {
            issue(
                &mut out,
                "dependencyCycle",
                "同机服务依赖成环，无法启动".into(),
            );
        }
    }
    let mut rules = HashSet::new();
    for r in &p.network_rules {
        if store::validate_id(&r.id).is_err()
            || !rules.insert(&r.id)
            || !ids.contains(&r.from_server_id)
            || !ids.contains(&r.to_server_id)
            || !["tcp", "udp"].contains(&r.protocol.as_str())
            || !ports.contains(&(&r.to_server_id, r.to_port, &r.protocol))
        {
            issue(
                &mut out,
                "network",
                format!("访问规则 {} 引用、协议或目标发布端口无效", r.id),
            );
        }
    }
    if let Some(reg) = &p.registry {
        if (build || !reg.url.is_empty()) && normalize_host(&reg.url).is_err() {
            issue(&mut out, "registry", "私有仓库地址无效".into());
        }
    }
    if build && (p.servers.is_empty() || p.instances.is_empty()) {
        issue(&mut out, "empty", "构建至少需要一台服务器和一个实例".into());
    }
    out
}
pub fn require(p: &Project, cat: &CatalogFile, build: bool) -> AppResult<()> {
    let out = issues(p, cat, build);
    if out.is_empty() {
        Ok(())
    } else {
        Err(AppError::Invalid(
            out.iter()
                .map(|i| i.message.as_str())
                .collect::<Vec<_>>()
                .join("；"),
        ))
    }
}
