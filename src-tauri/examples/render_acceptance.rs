use offline_preops_tool_lib::{
    builder, catalog, image_archive, images, io_util, models::Project, render_context, validation,
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 10 && args.len() != 11 {
        return Err("usage: render_acceptance DIR NGINX_TAR HOST_IP DOCKER_VERSION DOCKER_ROOT BUILD_ID BASELINE_ID HOST_PORT POSTGRES_TAR".into());
    }
    let root = PathBuf::from(&args[1]);
    let image_tar = Path::new(&args[2]);
    let image = "docker.io/library/nginx:1.27.2";
    let postgres_image = "docker.io/library/postgres:16.4";
    let port: u16 = args[8].parse()?;
    let project_id =
        std::env::var("PREOPS_ACCEPTANCE_PROJECT_ID").unwrap_or_else(|_| "proj-acceptance".into());
    let os_family =
        std::env::var("PREOPS_ACCEPTANCE_OS_FAMILY").unwrap_or_else(|_| "ubuntu".into());
    let os_version = match std::env::var("PREOPS_ACCEPTANCE_OS_VERSION") {
        Ok(version) => version,
        Err(_) => std::fs::read_to_string("/etc/os-release")?
            .lines()
            .find_map(|line| {
                line.strip_prefix("VERSION_ID=")
                    .map(|v| v.trim_matches('"').to_string())
            })
            .ok_or("VERSION_ID missing")?,
    };
    let deploy_base_dir = if let Ok(path) = std::env::var("PREOPS_ACCEPTANCE_DEPLOY_BASE") {
        path
    } else if cfg!(windows) {
        "/tmp/preops-acceptance".to_string()
    } else {
        root.parent()
            .ok_or("root has no parent")?
            .to_string_lossy()
            .into_owned()
    };
    let mut project: Project = serde_json::from_value(json!({
        "schemaVersion": 1, "id":project_id, "name":"接受测试", "createdAt":"ci", "updatedAt":"ci",
        "servers":[
            {"id":"srv-acceptance","name":"目标主机","ip":args[3],"arch":"amd64","bits":64,"osFamily":os_family,"osVersion":os_version,"memoryGb":8,"dockerVersion":args[4],"dockerDataRoot":args[5],"deployBaseDir":deploy_base_dir},
            {"id":"srv-allowed","name":"允许来源","ip":"172.30.252.10","arch":"amd64","bits":64,"osFamily":os_family,"osVersion":os_version,"memoryGb":8,"dockerVersion":args[4]}
        ],
        "instances":[
            {"id":"inst-nginx","serverId":"srv-acceptance","templateId":"nginx-1.27","image":image,"instanceName":"web","ports":[{"name":"http","host":port,"container":80,"protocol":"tcp","expose":true}]},
            {"id":"inst-postgres","serverId":"srv-acceptance","templateId":"postgresql-16","image":postgres_image,"instanceName":"db","params":{"POSTGRES_USER":"review_user","POSTGRES_PASSWORD":"review_password","POSTGRES_DB":"review_db"},"ports":[]},
            {"id":"inst-source","serverId":"srv-allowed","templateId":"custom","image":image,"instanceName":"source","ports":[]}
        ],
        "networkRules":[{"id":"rule-allowed","fromServerId":"srv-allowed","toServerId":"srv-acceptance","toPort":port,"protocol":"tcp"}]
    }))?;
    if args.get(10).is_some_and(|name| name == "rename") {
        project.instances[1].instance_name = "db-renamed".into();
    }
    let catalog = catalog::preset()?;
    validation::require(&project, &catalog, true)?;
    project.template_snapshots = catalog
        .templates
        .iter()
        .filter(|t| t.id == "nginx-1.27" || t.id == "postgresql-16")
        .cloned()
        .collect();
    let is_upgrade = !args[7].is_empty();
    for dir in [
        "scripts",
        "stack/data/inst-nginx",
        "stack/data/inst-postgres",
        "docs",
        "images",
        "docker-offline",
    ] {
        fs::create_dir_all(root.join(dir))?;
    }
    let mut ctx = render_context::server_context(
        &project,
        &project.servers[0],
        &catalog,
        &args[6],
        "ci",
        "acceptance",
        Some(&args[7]),
    );
    ctx["disk_need_mb"] = json!(512);
    for (index, (src, reference, dest)) in [
        (image_tar, image, "images/nginx.tar"),
        (Path::new(&args[9]), postgres_image, "images/postgres.tar"),
    ]
    .iter()
    .enumerate()
    {
        let verified = image_archive::verify(src, reference, "linux", "amd64")?;
        let runtime_reference = images::delivery_reference(&verified.config_digest)?;
        ctx["instances"][index]["image"] = json!(runtime_reference);
        if !is_upgrade {
            images::rewrite_archive(src, &root.join(dest), &runtime_reference)?;
        }
    }
    ctx["images"] = if is_upgrade {
        json!([])
    } else {
        json!([
            {"file":"images/nginx.tar","reference":image},
            {"file":"images/postgres.tar","reference":postgres_image}
        ])
    };
    let env = builder::template_env()?;
    let scripts = if is_upgrade {
        vec!["upgrade", "ops", "precheck", "apply-firewall", "runtime"]
    } else {
        vec!["deploy", "ops", "precheck", "apply-firewall", "runtime"]
    };
    for name in scripts {
        fs::write(
            root.join(format!("scripts/{name}.sh")),
            builder::render(&env, &format!("scripts/{name}.sh.j2"), &ctx)?,
        )?;
    }
    let mut compose = builder::render(&env, "compose/docker-compose.yml.j2", &ctx)?;
    if std::env::var("PREOPS_ACCEPTANCE_RESOURCE_LIMIT").as_deref() == Ok("1") {
        let mut value: serde_yaml::Value = serde_yaml::from_str(&compose)?;
        for (_, service) in value["services"]
            .as_mapping_mut()
            .ok_or("services missing")?
        {
            service["mem_limit"] = serde_yaml::Value::String("256m".into());
            service["cpus"] = serde_yaml::Value::Number(serde_yaml::Number::from(0.25));
        }
        compose = serde_yaml::to_string(&value)?;
    }
    fs::write(root.join("stack/docker-compose.yml"), compose)?;
    for (src, dest) in [
        ("docs/README.md.j2", "README.md"),
        ("docs/OPS-GUIDE.md.j2", "docs/OPS-GUIDE.md"),
        ("docs/PORT-MATRIX.md.j2", "docs/PORT-MATRIX.md"),
    ] {
        fs::write(root.join(dest), builder::render(&env, src, &ctx)?)?;
    }
    if !is_upgrade {
        fs::write(
            root.join("docker-offline/install-docker.sh"),
            "#!/bin/bash\nset -e\ndocker compose version >/dev/null\n",
        )?;
    }
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(
            &json!({"schemaVersion":2,"storageSchemaVersion":1,"projectId":project.id,"serverId":project.servers[0].id,"buildId":args[6],"baselineBuildId":args[7],"status":"complete","instances":render_context::instance_contracts(&ctx)}),
        )?,
    )?;
    fs::write(
        root.join("deployment.env"),
        format!(
            "PROJECT_ID='{}'\nSERVER_ID='srv-acceptance'\nBUILD_ID='{}'\nBASELINE_BUILD_ID='{}'\n",
            project.id, args[6], args[7]
        ),
    )?;
    let mut sums = Vec::new();
    fn collect(
        root: &Path,
        path: &Path,
        sums: &mut Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                collect(root, &path, sums)?;
            } else if path.file_name().and_then(|s| s.to_str()) != Some("SHA256SUMS") {
                sums.push(format!(
                    "{}  {}",
                    io_util::sha256_file(&path)?,
                    path.strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/")
                ));
            }
        }
        Ok(())
    }
    collect(&root, &root, &mut sums)?;
    sums.sort();
    fs::write(root.join("SHA256SUMS"), sums.join("\n") + "\n")?;
    Ok(())
}
