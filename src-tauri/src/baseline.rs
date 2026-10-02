use crate::{
    error::{AppError, AppResult},
    models::Project,
};
pub fn baseline_image_set(
    baseline: &serde_json::Value,
    server_id: &str,
    arch: &str,
) -> std::collections::HashSet<String> {
    baseline["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["serverId"].as_str() == Some(server_id) && s["arch"].as_str() == Some(arch))
        .flat_map(|s| s["images"].as_array().into_iter().flatten())
        .filter_map(|i| {
            Some(format!(
                "{}@{}",
                i["reference"].as_str()?,
                i["contentSha256"].as_str()?
            ))
        })
        .collect()
}

pub fn validate_baseline(b: &serde_json::Value, p: &Project) -> AppResult<()> {
    if b["schemaVersion"] != 2
        || b["status"] != "complete"
        || b["projectId"].as_str() != Some(&p.id)
    {
        return Err(AppError::Invalid(
            "基线必须为当前方案的完整、成功构建；旧版本基线需重新全量构建".into(),
        ));
    }
    if b["servers"]
        .as_array()
        .is_none_or(|servers| servers.len() != p.servers.len())
    {
        return Err(AppError::Invalid(
            "基线服务器集合已变化，请重新全量构建".into(),
        ));
    }
    for s in &p.servers {
        if !b["servers"].as_array().into_iter().flatten().any(|bs| {
            bs["serverId"].as_str() == Some(&s.id)
                && bs["arch"].as_str() == Some(&s.arch)
                && bs["osFamily"].as_str() == Some(&s.os_family)
                && bs["osVersion"].as_str() == Some(&s.os_version)
                && bs["dockerVersion"].as_str() == Some(&s.docker_version)
                && bs["dockerDataRoot"].as_str() == Some(&s.docker_data_root)
                && bs["deployBaseDir"].as_str() == Some(&s.deploy_base_dir)
                && bs["ip"].as_str() == Some(&s.ip)
        }) {
            return Err(AppError::Invalid(format!(
                "服务器 {} 身份/架构/OS/Docker/存储路径/IP 与基线不匹配，请全量构建",
                s.name
            )));
        }
    }
    Ok(())
}
