use crate::{builder::shell_quote, catalog, models::*};
use serde_json::json;

/// Pure context construction, shared by package generation and Linux acceptance fixtures.
pub fn server_context(
    project: &Project,
    server: &ServerInfo,
    catalog: &catalog::CatalogFile,
    build_id: &str,
    now: &str,
    dir_name: &str,
    baseline_build_id: Option<&str>,
) -> serde_json::Value {
    let tpl_of = |id: &str| catalog.templates.iter().find(|t| t.id == id);
    let instances: Vec<_> = project
        .instances
        .iter()
        .filter(|i| i.server_id == server.id)
        .collect();
    // ---- 实例渲染上下文 ----
    let inst_ctx: Vec<serde_json::Value> = instances
        .iter()
        .map(|inst| {
            let tpl = tpl_of(&inst.template_id);
            // env 值做 compose 安全转义：' → ''（YAML 单引号），$ → $$（禁止 compose 插值）
            let env_pairs: Vec<serde_json::Value> = inst
                .params
                .iter()
                .map(|(k, v)| {
                    let val = value_to_plain_string(v);
                    json!({ "k": k, "v": val })
                })
                .collect();
            let ports_csv = inst
                .ports
                .iter()
                .map(|p| format!("{}→{}", p.host, p.container))
                .collect::<Vec<_>>()
                .join(",");
            let data_volume = tpl
                .map(|t| t.data_volume.clone())
                .unwrap_or_else(|| "/data".into());
            let data_user = tpl.and_then(|t| t.data_user.clone());
            let command = tpl.map(|t| t.command.clone()).unwrap_or_default();
            let health_cmd: Option<String> = tpl
                .and_then(|t| t.health_check.as_ref())
                .and_then(|h| {
                    if h.r#type == "exec" && !h.cmd.is_empty() {
                        // 逐元素 shell 单引号化：含空格/||/()/$ 的元素（如 sh -c "curl a || wget b"）
                        // 裸拼接会被 deploy.sh 的 bash 解释成自己的运算符，体检必坏
                        Some(
                            h.cmd
                                .iter()
                                .map(|c| shell_quote(c))
                                .collect::<Vec<_>>()
                                .join(" "),
                        )
                    } else {
                        None
                    }
                });
            let health_tcp: Option<u32> = tpl
                .and_then(|t| t.health_check.as_ref())
                .and_then(|h| {
                    if h.r#type == "tcp" {
                        inst.ports.iter().find(|p| p.expose && p.protocol=="tcp").map(|p| p.host)
                    } else {
                        None
                    }
                });
            let health_timeout = tpl.and_then(|t| t.health_timeout_sec).unwrap_or(60);
            // 同机依赖：模板 depends_on 命中的同服务器其它实例（compose depends_on）
            let depends: Vec<String> = tpl
                .map(|t| {
                    t.depends_on
                        .iter()
                        .filter_map(|dep| instances.iter().find(|o| o.template_id == *dep))
                        .map(|o| o.instance_name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            json!({
                "instance_id": inst.id,
                "instance_name": inst.instance_name,
                "template_id": inst.template_id,
                "data_dir": inst.id,
                "storage_compatibility": storage_compatibility(inst),
                "image": crate::images::locked_reference(&inst.image, &inst.digest)
                    .and_then(|r| crate::images::load_reference(&r)).unwrap_or_else(|_|inst.image.clone()),
                "env": env_pairs,
                "ports": inst.ports.iter().map(|p| json!({
                    "host": p.host, "container": p.container,
                    "protocol": p.protocol, "expose": p.expose
                })).collect::<Vec<_>>(),
                "data_volume": data_volume,
                "data_user": data_user,"initialize_html":inst.template_id.starts_with("nginx-"),
                "depends": depends,
                "command": command,
                "health_cmd": health_cmd,
                "health_tcp": health_tcp,
                "health_timeout": health_timeout,
                "ports_csv": ports_csv,
                "backup_hint": "bash scripts/ops.sh backup（停止服务后创建一致性文件快照）",
                "min_memory_gb": tpl.map(|t| t.min_memory_gb).unwrap_or(0.5),
            })
        })
        .collect();

    let services_csv = instances
        .iter()
        .map(|i| i.instance_name.clone())
        .collect::<Vec<_>>()
        .join(",");
    let exposed: Vec<_> = instances
        .iter()
        .flat_map(|i| {
            i.ports
                .iter()
                .filter(|p| p.expose)
                .map(|p| p.host)
                .collect::<Vec<_>>()
        })
        .collect();
    let exposed_ports_csv = exposed
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");

    // 访问规则（指向本机）
    let allowed_rules: Vec<serde_json::Value> = project
        .network_rules
        .iter()
        .filter(|r| r.to_server_id == server.id)
        // 过滤来源 IP 为空的规则：空 address= 会生成非法防火墙命令
        .filter(|r| {
            project
                .servers
                .iter()
                .find(|s| s.id == r.from_server_id)
                .map(|s| !s.ip.trim().is_empty())
                .unwrap_or(false)
        })
        .map(|r| {
            let from = project.servers.iter().find(|s| s.id == r.from_server_id);
            json!({
                "from_name": from.map(|s| s.name.clone()).unwrap_or_default(),
                "from_ip": from.map(|s| s.ip.clone()).unwrap_or_default(),
                "to_port": r.to_port,
                "protocol": r.protocol,
                "description": r.description,
            })
        })
        .collect();

    // 本机端口表（含放行来源）
    let local_ports: Vec<serde_json::Value> = instances
        .iter()
        .flat_map(|i| {
            i.ports
                .iter()
                .filter(|p| p.expose)
                .map(|p| {
                    let sources = project
                        .network_rules
                        .iter()
                        .filter(|r| r.to_server_id == server.id && r.to_port == p.host && r.protocol==p.protocol)
                        .map(|r| {
                            project
                                .servers
                                .iter()
                                .find(|s| s.id == r.from_server_id)
                                .map(|s| s.ip.clone())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    json!({
                        "host": p.host, "container": p.container,
                        "protocol": p.protocol, "instance_name": i.instance_name,
                        "sources": if sources.is_empty() { "（本机/按需）".to_string() } else { sources },
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let mem_need_gb = inst_ctx
        .iter()
        .map(|v| {
            let g = v
                .get("min_memory_gb")
                .and_then(|x| x.as_f64())
                .unwrap_or(0.5);
            g
        })
        .sum::<f64>()
        .ceil() as u32;
    let kernel_reqs: Vec<String> = instances
        .iter()
        .filter_map(|i| tpl_of(&i.template_id).map(|t| t.kernel_reqs.clone()))
        .flatten()
        .collect();
    let kernel_reqs_raw = kernel_reqs.join(",");

    let server_ctx = json!({
        "name": server.name, "arch": server.arch,
        "os_family": server.os_family, "os_version": server.os_version,
        "ip": server.ip, "docker_version": server.docker_version,
        "docker_data_root": server.docker_data_root,
        "deploy_base_dir": server.deploy_base_dir,
    });
    json!({
        "project_id":project.id,"server_id":server.id,"firewall_chain":format!("OPO_{}",&crate::io_util::hash(&format!("{}/{}",project.id,server.id))[..12]),
        "project_name": project.name,
        "build_id": build_id,
        "generated_at": now,
        "server": server_ctx,
        "instances": inst_ctx,
        "services_csv": services_csv,

        "exposed_ports_csv": exposed_ports_csv,
        "allowed_rules": allowed_rules,
        "local_ports": local_ports,
        "uname_arch": uname_of(&server.arch),
        "dir_name": dir_name,
        "stack_name": format!("opo-{}-{}",&crate::io_util::hash(&project.id)[..12],&crate::io_util::hash(&server.id)[..12]),
        "baseline_build_id": baseline_build_id.unwrap_or(""),
        "mem_need_gb": std::cmp::max(mem_need_gb, 1),
        "disk_need_mb": 1024u32, // Replaced by actual material sizes before rendering.
        "kernel_reqs": kernel_reqs,
        "kernel_reqs_raw": kernel_reqs_raw,
    })
}

pub fn storage_compatibility(inst: &MiddlewareInstance) -> String {
    let stateful = [
        "mysql-",
        "postgresql-",
        "mongodb-",
        "redis-",
        "elasticsearch-",
        "rabbitmq-",
        "kafka-",
    ]
    .iter()
    .any(|prefix| inst.template_id.starts_with(prefix));
    if stateful {
        let major = crate::images::parse_reference(&inst.image)
            .ok()
            .and_then(|r| r.tag.split('.').next().map(str::to_string))
            .unwrap_or_default();
        format!("{}:{major}", inst.template_id)
    } else {
        inst.template_id.clone()
    }
}

pub fn instance_contracts(ctx: &serde_json::Value) -> Vec<serde_json::Value> {
    ctx["instances"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            json!({
                "instanceId": i["instance_id"], "instanceName": i["instance_name"],
                "serverId": ctx["server_id"], "templateId": i["template_id"],
                "dataDir": i["data_dir"], "dataVolume": i["data_volume"],
                "storageCompatibility": i["storage_compatibility"], "runtimeReference": i["image"]
            })
        })
        .collect()
}

fn uname_of(arch: &str) -> &'static str {
    match arch {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        other => match other {
            "loongarch64" => "loongarch64",
            "mips64el" => "mips64",
            "sw64" => "sw_64",
            _ => "unknown",
        },
    }
}

fn value_to_plain_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}
