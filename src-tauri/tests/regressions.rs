use offline_preops_tool_lib::{
    backup, builder, catalog, credentials, docker_pkgs, image_archive, images, io_util, models::*,
    validation,
};
use serde_json::json;
use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

fn project() -> Project {
    serde_json::from_value(json!({"schemaVersion":1,"id":"proj-test","name":"测试方案","createdAt":"now","updatedAt":"now","servers":[{"id":"srv-test","name":"测试服务器","ip":"10.0.0.1","arch":"amd64","bits":64,"osFamily":"ubuntu","osVersion":"22.04","memoryGb":8}],"instances":[{"id":"inst-test","serverId":"srv-test","templateId":"custom","image":"demo:v1","instanceName":"demo","ports":[{"name":"tcp","host":8080,"container":80,"expose":true,"protocol":"tcp"}]}]})).unwrap()
}
fn temp() -> io_util::Cleanup {
    let path = std::env::temp_dir().join(format!(
        "preops-regression-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&path).unwrap();
    io_util::Cleanup(path)
}
fn storage(root: &Path) -> StorageInfo {
    StorageInfo {
        config_dir: root.join("config").to_string_lossy().into(),
        projects_root: root.join("projects").to_string_lossy().into(),
        image_cache_root: root.join("cache").to_string_lossy().into(),
        docker_pkg_root: root.join("pkgs").to_string_lossy().into(),
        artifact_root: root.join("dist").to_string_lossy().into(),
        log_root: root.join("logs").to_string_lossy().into(),
    }
}

#[test]
fn build_validation_rejects_bad_structures_and_accepts_udp_and_unpublished_ports() {
    let cat = catalog::preset().unwrap();
    let mut p = project();
    validation::require(&p, &cat, true).unwrap();
    p.servers[0].ip = "999.999.1.1".into();
    assert!(validation::require(&p, &cat, true).is_err());
    p.servers[0].ip.clear();
    validation::require(&p, &cat, false).unwrap();
    assert!(validation::require(&p, &cat, true).is_err());
    p.servers[0].ip = "10.0.0.1".into();
    let mut udp = p.instances[0].ports[0].clone();
    udp.protocol = "udp".into();
    p.instances[0].ports.push(udp);
    let mut hidden = p.instances[0].ports[0].clone();
    hidden.expose = false;
    p.instances[0].ports.push(hidden);
    validation::require(&p, &cat, true).unwrap();
    p.instances[0].ports[1].protocol = "tcp".into();
    assert!(validation::require(&p, &cat, true).is_err());
    p = project();
    p.instances[0].instance_name = "服务".into();
    assert!(validation::require(&p, &cat, true).is_err());
    p = project();
    p.instances[0].template_id = "missing".into();
    assert!(validation::require(&p, &cat, false).is_err());
    p = project();
    p.build_config.sign = true;
    assert!(validation::require(&p, &cat, true).is_err());
}

#[test]
fn catalog_presets_are_valid_and_credential_checks_follow_each_database() {
    let cat = catalog::preset().unwrap();
    for t in &cat.templates {
        validation::validate_template(t).unwrap();
    }
    let minio = cat.templates.iter().find(|t| t.id == "minio").unwrap();
    assert_eq!(
        minio.command,
        vec!["server", "/data", "--console-address", ":9001"]
    );
    let mut p = project();
    p.instances[0].template_id = "mysql-8.0".into();
    p.instances[0]
        .params
        .insert("MYSQL_ROOT_PASSWORD".into(), json!("safe-password"));
    p.instances[0]
        .params
        .insert("MYSQL_USER".into(), json!("app"));
    assert!(validation::require(&p, &cat, true).is_err());
    p.instances[0]
        .params
        .insert("MYSQL_PASSWORD".into(), json!("pass"));
    validation::require(&p, &cat, true).unwrap();
    let mut portable = p.clone();
    credentials::strip_project(&mut portable, &cat);
    assert!(!portable.instances[0]
        .params
        .contains_key("MYSQL_ROOT_PASSWORD"));
    portable.template_snapshots = vec![minio.clone()];
    let overlay = catalog::overlay(catalog::CatalogFile::default(), &portable);
    assert_eq!(overlay.templates[0].command, minio.command);
}

#[test]
fn cache_keys_isolate_ambiguous_repositories_sources_and_platforms() {
    let key = |image, arch, source: &str| {
        images::cache_key(image, "linux", arch, &[source.into()]).unwrap()
    };
    assert_ne!(
        key("team/app:v1", "amd64", "registry-a"),
        key("team_app:v1", "amd64", "registry-a")
    );
    assert_ne!(
        key("team/app:v1", "amd64", "registry-a"),
        key("team/app:v1", "amd64", "registry-b")
    );
    assert_ne!(
        key("team/app:v1", "amd64", "registry-a"),
        key("team/app:v1", "arm64", "registry-a")
    );
    assert_eq!(
        key("nginx:1", "amd64", "hub"),
        key("docker.io/library/nginx:1", "amd64", "hub")
    );
    assert!(images::parse_reference("demo@sha256:bad").is_err());
}

fn archive(path: &Path, arch: &str, tag: &str, with_layer: bool) {
    let layer = b"example-layer";
    let config=json!({"architecture":arch,"os":"linux","rootfs":{"diff_ids":[format!("sha256:{}",io_util::hash(std::str::from_utf8(layer).unwrap()))]}}).to_string();
    let name = format!("{}.json", io_util::hash(&config));
    let manifest = json!([{"Config":name,"RepoTags":[tag],"Layers":["layer.tar"]}]).to_string();
    let mut tar = tar::Builder::new(File::create(path).unwrap());
    for (name, body) in [
        (name.as_str(), config.as_bytes()),
        ("manifest.json", manifest.as_bytes()),
        ("layer.tar", layer.as_slice()),
    ] {
        if name == "layer.tar" && !with_layer {
            continue;
        }
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        tar.append_data(&mut h, name, body).unwrap();
    }
    tar.finish().unwrap();
}
#[test]
fn image_archive_checks_tag_platform_layers_and_actual_file_sha() {
    let t = temp();
    let file = t.0.join("image.tar");
    archive(&file, "amd64", "demo:v1", true);
    let valid = image_archive::verify(&file, "demo:v1", "linux", "amd64").unwrap();
    assert_eq!(valid.sha256, io_util::sha256_file(&file).unwrap());
    assert!(image_archive::verify(&file, "demo:v2", "linux", "amd64").is_err());
    assert!(image_archive::verify(&file, "demo:v1", "linux", "arm64").is_err());
    archive(&file, "amd64", "demo:v1", false);
    assert!(image_archive::verify(&file, "demo:v1", "linux", "amd64").is_err());
    archive(&file, "loong64", "demo:v1", true);
    image_archive::verify(&file, "demo:v1", "linux", "loongarch64").unwrap();
    fs::write(&file, b"invalid").unwrap();
    assert!(image_archive::verify(&file, "demo:v1", "linux", "amd64").is_err());
}

#[test]
fn baseline_uses_stable_server_identity_platform_and_current_content() {
    let p = project();
    let b = json!({"schemaVersion":2,"status":"complete","projectId":p.id,"servers":[{"serverId":"srv-test","arch":"amd64","name":"old-name","osFamily":p.servers[0].os_family,"osVersion":p.servers[0].os_version,"dockerVersion":p.servers[0].docker_version,"dockerDataRoot":p.servers[0].docker_data_root,"deployBaseDir":p.servers[0].deploy_base_dir,"ip":p.servers[0].ip,"images":[{"reference":"demo:v1","contentSha256":"content-a"}]}]});
    builder::validate_baseline(&b, &p).unwrap();
    let set = builder::baseline_image_set(&b, "srv-test", "amd64");
    assert!(set.contains("demo:v1@content-a"));
    assert!(!set.contains("demo:v1@content-b"));
    assert!(builder::baseline_image_set(&b, "srv-test", "arm64").is_empty());
    let mut p = p;
    p.servers[0].arch = "arm64".into();
    assert!(builder::validate_baseline(&b, &p).is_err());
    let mut legacy = b;
    legacy["schemaVersion"] = json!(1);
    assert!(builder::validate_baseline(&legacy, &p).is_err());
}

#[test]
fn backup_creates_missing_project_dirs_preserves_current_roots_and_scrubs_secrets() {
    let t = temp();
    let source = storage(&t.0.join("source"));
    fs::create_dir_all(&source.config_dir).unwrap();
    let settings = AppSettings {
        projects_root: Some("Z:/old-machine/projects".into()),
        ..Default::default()
    };
    fs::write(
        Path::new(&source.config_dir).join("settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let mut p = project();
    p.registry = Some(RegistryConfig {
        url: "registry.example.org".into(),
        username: "user".into(),
        password: "private-secret".into(),
    });
    p.instances[0]
        .params
        .insert("PASSWORD".into(), json!("database-secret"));
    let path = Path::new(&source.projects_root)
        .join(&p.id)
        .join("project.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(&p).unwrap()).unwrap();
    let zip = t.0.join("backup.zip");
    backup::backup(&source, &zip).unwrap();
    let target = storage(&t.0.join("target"));
    backup::restore(&target, &zip).unwrap();
    let restored: Project = serde_json::from_slice(
        &fs::read(Path::new(&target.projects_root).join("proj-test/project.json")).unwrap(),
    )
    .unwrap();
    assert!(restored.registry.unwrap().password.is_empty());
    assert!(!restored.instances[0].params.contains_key("PASSWORD"));
    let restored: AppSettings = serde_json::from_slice(
        &fs::read(Path::new(&target.config_dir).join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.projects_root, None);
}

#[test]
fn corrupt_restore_cannot_overwrite_any_live_file() {
    let t = temp();
    let s = storage(&t.0);
    fs::create_dir_all(&s.config_dir).unwrap();
    let path = Path::new(&s.config_dir).join("settings.json");
    fs::write(&path, b"existing").unwrap();
    let zip = t.0.join("bad.zip");
    let mut w = zip::ZipWriter::new(File::create(&zip).unwrap());
    let opt = zip::write::SimpleFileOptions::default();
    w.start_file("settings.json", opt).unwrap();
    w.write_all(&serde_json::to_vec(&AppSettings::default()).unwrap())
        .unwrap();
    w.start_file("projects/proj-test/project.json", opt)
        .unwrap();
    w.write_all(b"corrupt").unwrap();
    w.finish().unwrap();
    assert!(backup::restore(&s, &zip).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"existing");
    let a = t.0.join("a");
    assert!(backup::replace_transaction(&[
        (a.clone(), b"first".to_vec()),
        (a.clone(), b"second".to_vec())
    ])
    .is_err());
    assert!(!a.exists());
}

#[test]
fn package_parsing_handles_deb_aux_and_rejects_32_bit_arm_as_arm64() {
    assert!(
        docker_pkgs::parse_pkg_filename("docker-ce-cli_27.5.1-1_amd64.deb")
            .unwrap()
            .3
    );
    assert!(
        docker_pkgs::parse_pkg_filename("docker-compose-plugin_2.1_arm64.deb")
            .unwrap()
            .3
    );
    assert!(docker_pkgs::parse_pkg_filename("docker-ce_27.5.1_armhf.deb").is_none());
    assert_eq!(
        docker_pkgs::parse_pkg_filename("docker-ce-27.5.1.el8.loongarch64.rpm")
            .unwrap()
            .1,
        "loongarch64"
    );
}

#[test]
fn native_package_selection_requires_exact_os_roles_versions_and_hashes() {
    let t = temp();
    let root = t.0.join("packages");
    fs::create_dir_all(&root).unwrap();
    let add = |dir: &str, file: &str, version: &str, role: &str| {
        let path = root.join(dir).join("packages");
        fs::create_dir_all(&path).unwrap();
        let pkg = path.join(file);
        fs::write(&pkg, b"!<arch>\n").unwrap();
        let sha = std::collections::BTreeMap::from([(file, io_util::sha256_file(&pkg).unwrap())]);
        let components = std::collections::BTreeMap::from([(file, role)]);
        let meta = json!({"arch":"amd64","kind":"deb","dockerVersion":version,
            "engineVersion":"27.5.1","osFamily":"ubuntu","osVersion":"24.04",
            "files":[file],"sha256":sha,"components":components});
        fs::write(
            root.join(dir).join("meta.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
        pkg
    };
    let engine = add("engine", "docker-ce.deb", "27.5.1", "engine");
    add("cli", "docker-cli.deb", "deps", "cli");
    assert!(docker_pkgs::pick_pkg_dirs_at(&root, "amd64", "27.5.1", "ubuntu", "24.04").is_err());
    add("containerd", "containerd.deb", "deps", "containerd");
    assert_eq!(
        docker_pkgs::pick_pkg_dirs_at(&root, "amd64", "27.5.1", "ubuntu", "24.04")
            .unwrap()
            .len(),
        3
    );
    assert!(docker_pkgs::pick_pkg_dirs_at(&root, "amd64", "27.5.1", "ubuntu", "22.04").is_err());
    assert!(docker_pkgs::pick_pkg_dirs_at(&root, "arm64", "27.5.1", "ubuntu", "24.04").is_err());
    fs::write(engine, b"!<arch>\nmodified").unwrap();
    assert!(docker_pkgs::pick_pkg_dirs_at(&root, "amd64", "27.5.1", "ubuntu", "24.04").is_err());
}

#[test]
fn dependency_cycle_duplicate_hostname_and_server_removal_are_rejected() {
    let mut cat = catalog::preset().unwrap();
    for (id, dep) in [("a", "b"), ("b", "a")] {
        cat.templates.push(
            serde_json::from_value(json!({
                "id":id,"displayName":id,"category":"custom","defaultImage":"demo:v1",
                "dependsOn":[dep],"dataVolume":"/data"
            }))
            .unwrap(),
        );
    }
    let mut p = project();
    p.instances[0].template_id = "a".into();
    let mut other = p.instances[0].clone();
    other.id = "inst-b".into();
    other.instance_name = "service_b".into();
    other.template_id = "b".into();
    other.ports[0].host = 8081;
    p.instances.push(other);
    assert!(validation::issues(&p, &cat, true)
        .iter()
        .any(|i| i.code == "dependencyCycle"));
    p.instances[1].template_id = "custom".into();
    assert!(!validation::issues(&p, &cat, true)
        .iter()
        .any(|i| i.code == "dependencyCycle"));
    let mut server = p.servers[0].clone();
    server.id = "srv-b".into();
    server.ip = "10.0.0.2".into();
    server.name = "second".into();
    server.hostname = "same-host".into();
    p.servers[0].hostname = "same-host".into();
    p.servers.push(server);
    assert!(validation::require(&p, &cat, true).is_err());
    assert!(images::same_reference(
        "nginx:1",
        "docker.io/library/nginx:1"
    ));
    assert!(!images::same_reference(
        "nginx:1",
        "docker.io/library/nginx:2"
    ));
}

#[cfg(windows)]
#[test]
fn os_credentials_round_trip_unicode_and_special_chars() {
    let secret = "密码 '$() 中文\n🙂";
    let protected = credentials::protect(secret).unwrap();
    assert!(!protected.contains(secret));
    assert_eq!(credentials::reveal(&protected).unwrap(), secret);
    let mut p = project();
    p.instances[0]
        .params
        .insert("APP_SECRET".into(), json!(secret));
    credentials::protect_project(&mut p, &catalog::preset().unwrap()).unwrap();
    assert!(p.instances[0].params["APP_SECRET"]
        .as_str()
        .unwrap()
        .starts_with("dpapi:"));
    credentials::reveal_project(&mut p).unwrap();
    assert_eq!(p.instances[0].params["APP_SECRET"], json!(secret));
}
