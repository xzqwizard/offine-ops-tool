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
    if b["storageSchemaVersion"] != 1 {
        return Err(AppError::Invalid(
            "旧基线缺少稳定存储/镜像契约，不能安全增量升级；请先备份并按迁移说明生成新全量部署"
                .into(),
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
    let cat = crate::catalog::overlay(crate::catalog::preset()?, p);
    let mut identities = std::collections::HashSet::new();
    for server in b["servers"].as_array().into_iter().flatten() {
        let contracts = server["instances"]
            .as_array()
            .ok_or_else(|| AppError::Invalid("基线缺实例存储契约".into()))?;
        for old in contracts {
            let id = old["instanceId"]
                .as_str()
                .ok_or_else(|| AppError::Invalid("基线实例身份无效".into()))?;
            if !identities.insert(id)
                || old["serverId"] != server["serverId"]
                || old["dataDir"].as_str() != Some(id)
                || !old["dataVolume"].is_string()
                || !old["storageCompatibility"].is_string()
                || !old["runtimeReference"].is_string()
            {
                return Err(AppError::Invalid("基线实例契约损坏或存在重复身份".into()));
            }
        }
    }
    for inst in &p.instances {
        if let Some(old) = b["servers"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|s| s["instances"].as_array().into_iter().flatten())
            .find(|i| i["instanceId"].as_str() == Some(&inst.id))
        {
            let volume = cat
                .templates
                .iter()
                .find(|t| t.id == inst.template_id)
                .map(|t| t.data_volume.as_str())
                .unwrap_or("/data");
            if old["serverId"].as_str() != Some(&inst.server_id)
                || old["dataDir"].as_str() != Some(&inst.id)
                || old["dataVolume"].as_str() != Some(volume)
                || old["storageCompatibility"].as_str()
                    != Some(crate::render_context::storage_compatibility(inst).as_str())
            {
                return Err(AppError::Invalid(format!("实例 {} 的服务器/持久化挂载/版本兼容性已变化，需先备份并执行显式数据迁移，不能直接增量升级", inst.instance_name)));
            }
        }
    }
    Ok(())
}
