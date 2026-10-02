use crate::error::AppResult;
use serde_json::{json, Value};

/// Serialize YAML values, never interpolate user values into YAML text.
pub fn from_context(ctx: &Value) -> AppResult<String> {
    let mut services = serde_json::Map::new();
    for inst in ctx["instances"].as_array().into_iter().flatten() {
        let name = inst["instance_name"].as_str().unwrap_or_default();
        let mut service = json!({"image":inst["image"],"hostname":name.replace('_', "-").to_ascii_lowercase(),"restart":"unless-stopped","pull_policy":"never","labels":{"offlinepreops.project":ctx["project_id"],"offlinepreops.server":ctx["server_id"]},"volumes":[{"type":"bind","source":format!("./data/{name}"),"target":inst["data_volume"].as_str().unwrap_or("/data").replace('$', "$$")} ]});
        let ports:Vec<_>=inst["ports"].as_array().into_iter().flatten().filter(|p|p["expose"]==true).map(|p|json!({"target":p["container"],"published":p["host"].to_string(),"protocol":p["protocol"],"host_ip":ctx["server"]["ip"]})).collect();
        if !ports.is_empty() {
            service["ports"] = json!(ports);
        }
        let mut env = serde_json::Map::new();
        for e in inst["env"].as_array().into_iter().flatten() {
            env.insert(
                e["k"].as_str().unwrap_or_default().into(),
                json!(e["v"].as_str().unwrap_or_default().replace('$', "$$")),
            );
        }
        if !env.is_empty() {
            service["environment"] = json!(env);
        }
        if inst["command"].as_array().is_some_and(|a| !a.is_empty()) {
            service["command"] = json!(inst["command"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c
                    .as_str()
                    .unwrap_or_default()
                    .replace("$$", "$")
                    .replace('$', "$$"))
                .collect::<Vec<_>>());
        }
        if inst["data_user"].is_string() {
            service["user"] = inst["data_user"].clone();
        }
        if inst["depends"].as_array().is_some_and(|a| !a.is_empty()) {
            service["depends_on"] = inst["depends"].clone();
        }
        services.insert(name.into(), service);
    }
    serde_yaml::to_string(&json!({"name":ctx["stack_name"],"services":services}))
        .map_err(|e| crate::error::AppError::Serialize(e.to_string()))
}
