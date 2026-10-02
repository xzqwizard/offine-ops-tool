//! 集成测试：构建器模板渲染与工具函数、Docker 版本解析。
//!
//! ⚠ 本包测试统一放在 tests/ 目录（不再使用 lib 内 #[cfg(test)]）：
//! lib 单元测试二进制无法注入 common-controls v6 清单（build.rs 的
//! rustc-link-arg-tests 仅覆盖显式 test 目标），在 windows-gnu 工具链下
//! 链接 GUI 符号后启动即 STATUS_ENTRYPOINT_NOT_FOUND。详见 build.rs。

use offline_preops_tool_lib::builder::{image_tag_of, render, sanitize, template_env, TEMPLATES};
use offline_preops_tool_lib::docker_versions::{parse_docker_versions, version_key};

/// 样例上下文：覆盖模板引用的全部变量，防止模板变量缺失/语法错误逃逸
fn sample_ctx() -> serde_json::Value {
    serde_json::json!({
        "project_name": "测试方案",
        "project_id":"proj-test","server_id":"srv-test","firewall_chain":"OPO_test","baseline_build_id":"b-old",
        "build_id": "b-test-0001",
        "generated_at": "2026-09-29T12:00:00+08:00",
        "server": {
            "name": "应用服务器01", "arch": "arm64",
            "os_family": "kylin", "os_version": "V10 SP3",
            "ip": "10.10.1.11", "docker_version": "27.5.1",
            "docker_data_root": "/data/docker", "deploy_base_dir": "/opt/stack"
        },
        "instances": [{
            "instance_name": "mysql", "image": "docker.io/library/mysql:8.0.42",
            "env": [ { "k": "MYSQL_ROOT_PASSWORD", "v": "p@ss" } ],
            "ports": [
                { "host": 3306, "container": 3306, "protocol": "tcp", "expose": true },
                { "host": 33060, "container": 33060, "protocol": "tcp", "expose": false }
            ],
            "data_volume": "/var/lib/mysql",
            "data_user": null,
            "command": [],
            "health_cmd": "'sh' '-c' 'curl -s -o /dev/null http://127.0.0.1/'",
            "health_tcp": null,
            "health_timeout": 90,
            "ports_csv": "3306→3306",
            "backup_hint": "mysyqldump ...",
            "min_memory_gb": 1.0
        },
        {
            "instance_name": "redis", "image": "docker.io/library/redis:7.2.5",
            "env": [ { "k": "REDIS_PASSWORD", "v": "p'a$x" } ],
            "ports": [{ "host": 6379, "container": 6379, "protocol": "tcp", "expose": true }],
            "data_volume": "/data",
            "data_user": null,
            "command": ["sh", "-c", "redis-server --requirepass \"$$REDIS_PASSWORD\""],
            "health_cmd": null,
            "health_tcp": 6379,
            "health_timeout": 60,
            "ports_csv": "6379→6379",
            "backup_hint": "",
            "min_memory_gb": 0.5
        }],
        "images": [{ "file": "images/mysql_8.0.42.tar", "reference": "docker.io/library/mysql:8.0.42" }],
        "services_csv": "mysql",
        "exposed_ports_csv": "3306",
        "allowed_rules": [{ "from_name": "应用服务器01", "from_ip": "10.10.1.11", "to_port": 3306, "protocol": "tcp", "description": "测试" }],
        "local_ports": [{ "host": 3306, "container": 3306, "protocol": "tcp", "instance_name": "mysql", "sources": "10.10.1.21" }],
        "uname_arch": "aarch64",
        "dir_name": "app-01_arm64",
        "stack_name": "test-project",
        "mem_need_gb": 2,
        "disk_need_mb": 15360,
        "kernel_reqs": ["vm.max_map_count>=262144"],
        "kernel_reqs_raw": "vm.max_map_count>=262144",
        "logical_backups": [
            "{ docker exec mysql sh -c 'mysqldump -uroot -p\"$MYSQL_ROOT_PASSWORD\" --single-transaction --all-databases' | gzip > \"$BACKUP_DIR/$TS/mysql.sql.gz\"; } || { rm -f \"$BACKUP_DIR/$TS/mysql.sql.gz\"; echo \"  [!] mysql 逻辑备份失败\"; }",
            "docker exec redis sh -c 'redis-cli -a \"$REDIS_PASSWORD\" BGSAVE' >/dev/null 2>&1 || true; sleep 2; cp x y 2>/dev/null || echo \"  [!] redis RDB 失败\""
        ],
    })
}

#[test]
fn all_templates_parse_and_render() {
    let env = template_env().expect("模板解析失败");
    let ctx = sample_ctx();
    for (name, _) in TEMPLATES {
        let out = render(&env, name, &ctx).unwrap_or_else(|e| panic!("模板 {name} 渲染失败: {e}"));
        assert!(!out.trim().is_empty(), "模板 {name} 渲染结果为空");
    }
}

#[test]
fn compose_encodes_literals_and_protocols() {
    let env = template_env().unwrap();
    let ctx = sample_ctx();
    let yaml = render(&env, "compose/docker-compose.yml.j2", &ctx).unwrap();
    let v: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(
        v["services"]["mysql"]["image"].as_str(),
        Some("docker.io/library/mysql:8.0.42")
    );
    assert_eq!(
        v["services"]["redis"]["environment"]["REDIS_PASSWORD"].as_str(),
        Some("p'a$$x")
    );
    assert_eq!(
        v["services"]["mysql"]["ports"].as_sequence().unwrap().len(),
        1
    );
    assert_eq!(
        v["services"]["mysql"]["ports"][0]["protocol"].as_str(),
        Some("tcp")
    );
    assert_eq!(
        v["services"]["mysql"]["ports"][0]["host_ip"].as_str(),
        Some("10.10.1.11")
    );
    assert_eq!(
        v["services"]["mysql"]["pull_policy"].as_str(),
        Some("never")
    );
}

#[test]
fn every_script_is_valid_bash_and_upgrade_is_registered() {
    let env = template_env().unwrap();
    let ctx = sample_ctx();
    assert!(TEMPLATES.iter().any(|(n, _)| *n == "scripts/upgrade.sh.j2"));
    let bash = if cfg!(windows) {
        "D:/Environment/Git/usr/bin/bash.exe"
    } else {
        "/bin/bash"
    };
    if !std::path::Path::new(bash).is_file() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("preops-syntax-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let _cleanup = offline_preops_tool_lib::io_util::Cleanup(dir.clone());
    for (name, _) in TEMPLATES.iter().filter(|(n, _)| n.ends_with(".sh.j2")) {
        let file = dir.join(name.replace('/', "-"));
        let script = render(&env, name, &ctx).unwrap();
        assert!(!script.contains('\r'), "{name} contains CRLF line endings");
        std::fs::write(&file, script).unwrap();
        let out = std::process::Command::new(bash)
            .arg("-n")
            .arg(file)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn sanitize_keeps_unicode_names() {
    // 中文保留（政务命名常态），分隔符替换为 -
    assert_eq!(sanitize("应用 服务器/01"), "应用-服务器-01");
    assert_eq!(sanitize("app-01_生产"), "app-01_生产");
    assert_eq!(sanitize("---"), "unnamed");
    assert!(sanitize(&"应用服务".repeat(30)).len() <= 64);
    assert_eq!(sanitize(".."), "unnamed");
    assert_eq!(sanitize("CON"), "unnamed");
}

#[test]
fn image_tag_extraction() {
    assert_eq!(image_tag_of("docker.io/library/mysql:8.0.42"), "8.0.42");
    // 官方镜像短引用（无 registry 前缀）——旧实现曾误判为 latest
    assert_eq!(image_tag_of("mysql:8.0"), "8.0");
    assert_eq!(image_tag_of("registry.cn:5000/app/img"), "latest");
    assert_eq!(image_tag_of("nginx"), "latest");
}

#[test]
fn docker_versions_parse_sorted_desc_and_dedup() {
    let html = String::new()
        + "<a href=\"docker-27.5.1.tgz\">x</a>\n"
        + "<a href=\"docker-28.0.1.tgz\">x</a>\n"
        + "<a href=\"docker-27.5.1.tgz\">x</a>\n"
        + "<a href=\"docker-24.0.9.tgz\">x</a>\n"
        + "<a href=\"docker-17.03.1-ce.tgz\">x</a>\n";
    let v = parse_docker_versions(&html);
    assert_eq!(v, vec!["28.0.1", "27.5.1", "24.0.9"]);
}

#[test]
fn docker_version_key_numeric_compare() {
    assert!(version_key("27.5.1") > version_key("9.03.0"));
    assert_eq!(version_key("24.0"), (24, 0, 0));
}
