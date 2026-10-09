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
        fs::write(
            &pkg,
            deb_fixture(
                match role {
                    "engine" => "docker-ce",
                    "cli" => "docker-ce-cli",
                    _ => "containerd.io",
                },
                "27.5.1-1",
                "amd64",
            ),
        )
        .unwrap();
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
    let extra = root.join("engine/packages/undeclared.deb");
    fs::write(&extra, b"!<arch>\n").unwrap();
    assert!(docker_pkgs::pick_pkg_dirs_at(&root, "amd64", "27.5.1", "ubuntu", "24.04").is_err());
    fs::remove_file(extra).unwrap();
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
    assert!(
        p.instances[0].params["APP_SECRET"]["$offlinePreOpsSecret"]["ciphertext"]
            .as_str()
            .unwrap()
            .starts_with("dpapi:")
    );
    credentials::reveal_project(&mut p).unwrap();
    assert_eq!(p.instances[0].params["APP_SECRET"], json!(secret));
}

#[cfg(windows)]
#[test]
fn literal_credential_prefix_and_nonsecret_text_round_trip_with_legacy_compatibility() {
    for secret in [
        "dpapi:literal-password",
        "dpapi:v1:not-ciphertext",
        "dpapi:",
        "普通密码",
    ] {
        let mut p = project();
        p.registry = Some(RegistryConfig {
            url: "registry.example.org".into(),
            username: "user".into(),
            password: secret.into(),
        });
        p.instances[0]
            .params
            .insert("APP_PASSWORD".into(), json!(secret));
        p.instances[0]
            .params
            .insert("COMMENT".into(), json!("dpapi:plain-comment"));
        credentials::protect_project(&mut p, &catalog::preset().unwrap()).unwrap();
        let persisted = serde_json::to_vec(&p).unwrap();
        let mut p: Project = serde_json::from_slice(&persisted).unwrap();
        credentials::reveal_project(&mut p).unwrap();
        assert_eq!(p.registry.unwrap().password, secret);
        assert_eq!(p.instances[0].params["APP_PASSWORD"], json!(secret));
        assert_eq!(
            p.instances[0].params["COMMENT"],
            json!("dpapi:plain-comment")
        );
    }
    let protected =
        credentials::protect("legacy secret")
            .unwrap()
            .replacen("dpapi:v1:", "dpapi:", 1);
    assert_eq!(credentials::reveal(&protected).unwrap(), "legacy secret");
    let mut p = project();
    p.instances[0]
        .params
        .insert("APP_SECRET".into(), json!(protected));
    credentials::migrate_project(&mut p, &catalog::preset().unwrap()).unwrap();
    let migrated = serde_json::to_string(&p).unwrap();
    credentials::migrate_project(&mut p, &catalog::preset().unwrap()).unwrap();
    assert_eq!(serde_json::to_string(&p).unwrap(), migrated);
    credentials::reveal_project(&mut p).unwrap();
    assert_eq!(p.instances[0].params["APP_SECRET"], json!("legacy secret"));
}

fn deb_fixture(package: &str, version: &str, arch: &str) -> Vec<u8> {
    let gzip_tar = |name: &str, data: &[u8]| {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut head = tar::Header::new_gnu();
        head.set_size(data.len() as u64);
        head.set_mode(0o644);
        head.set_cksum();
        tar.append_data(&mut head, name, data).unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    };
    let control = format!(
        "Package: {package}\nVersion: {version}\nArchitecture: {arch}\nDescription: test fixture\n"
    );
    let mut result = b"!<arch>\n".to_vec();
    for (name, body) in [
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar.gz", gzip_tar("./control", control.as_bytes())),
        (
            "data.tar.gz",
            gzip_tar("./usr/share/test", b"fixture payload"),
        ),
    ] {
        result.extend(
            format!(
                "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
                format!("{name}/"),
                0,
                0,
                0,
                "100644",
                body.len()
            )
            .as_bytes(),
        );
        result.extend(&body);
        if body.len() % 2 != 0 {
            result.push(b'\n');
        }
    }
    result
}

#[test]
fn native_control_metadata_and_truncated_packages_are_checked() {
    let t = temp();
    let pkg = t.0.join("native.deb");
    let good = deb_fixture("docker-ce", "27.5.1-1", "amd64");
    fs::write(&pkg, &good).unwrap();
    let info = docker_pkgs::verify_native(&pkg, "deb").unwrap();
    assert_eq!(
        (
            info.name.as_str(),
            info.version.as_str(),
            info.arch.as_str()
        ),
        ("docker-ce", "27.5.1-1", "amd64")
    );
    fs::write(&pkg, &good[..good.len() - 5]).unwrap();
    assert!(docker_pkgs::verify_native(&pkg, "deb").is_err());
    fs::write(&pkg, b"!<arch>\n").unwrap();
    assert!(docker_pkgs::verify_native(&pkg, "deb").is_err());
    let mut overflow = good;
    overflow[56..66].copy_from_slice(b"9999999999");
    fs::write(&pkg, overflow).unwrap();
    assert!(docker_pkgs::verify_native(&pkg, "deb").is_err());
}

#[test]
fn conflicting_material_basenames_cannot_overwrite_each_other() {
    let t = temp();
    for (group, sha) in [("a", "first"), ("b", "second")] {
        let dir = t.0.join(group);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("meta.json"), serde_json::to_vec(&json!({"arch":"amd64","kind":"deb","dockerVersion":"27.5.1","files":["same.deb"],"sha256":{"same.deb":sha}})).unwrap()).unwrap();
    }
    assert!(docker_pkgs::package_inventory(&[t.0.join("a"), t.0.join("b")]).is_err());
}

fn valid_elf() -> Vec<u8> {
    let mut bytes = vec![0; 128];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes[68..72].copy_from_slice(&5u32.to_le_bytes());
    bytes[96..104].copy_from_slice(&128u64.to_le_bytes());
    bytes[104..112].copy_from_slice(&128u64.to_le_bytes());
    bytes
}

#[test]
fn compose_requires_complete_elf_and_persisted_integrity_metadata() {
    let t = temp();
    let file = t.0.join("docker-compose");
    let elf = valid_elf();
    fs::write(&file, &elf[..20]).unwrap();
    assert!(docker_pkgs::verify_elf(&file, "amd64").is_err());
    fs::write(&file, &elf).unwrap();
    docker_pkgs::verify_elf(&file, "amd64").unwrap();
    assert!(docker_pkgs::verify_compose_at(&t.0, "amd64").is_err());
    let meta = json!({"arch":"amd64","version":"2.40.3","source":"local-test","sizeBytes":elf.len(),"sha256":io_util::sha256_file(&file).unwrap()});
    fs::write(t.0.join("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
    docker_pkgs::verify_compose_at(&t.0, "amd64").unwrap();
    let mut damaged = elf;
    damaged[127] = 42;
    fs::write(&file, damaged).unwrap();
    assert!(docker_pkgs::verify_compose_at(&t.0, "amd64").is_err());
}
