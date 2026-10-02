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
            "env": [ { "k": "REDIS_PASSWORD", "v": "p''a$$x" } ],
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
fn compose_contains_service_and_env() {
    let env = template_env().unwrap();
    let out = render(&env, "compose/docker-compose.yml.j2", &sample_ctx()).unwrap();
    assert!(out.contains("mysql:"));
    // 镜像引用带引号（防 YAML 特殊字符）
    assert!(out.contains("image: \"docker.io/library/mysql:8.0.42\""));
    // env 值用单引号包裹（$ 与 " 不会破坏 compose），值内 ' 转义为 ''
    assert!(out.contains("MYSQL_ROOT_PASSWORD: 'p@ss'"));
    assert!(out.contains("REDIS_PASSWORD: 'p''a$$x'"));
    // 仅 expose=true 的端口进入 ports；33060 不应出现
    assert!(out.contains("\"3306:3306\""));
    assert!(!out.contains("33060"));
    // command 元素单引号包裹：含双引号的 redis requirepass 命令是合法 YAML
    assert!(out.contains("command: ['sh', '-c', 'redis-server --requirepass \"$$REDIS_PASSWORD\"']"));
    // 整体必须是合法 YAML（现场 compose config 前拦截）
    serde_yaml::from_str::<serde_yaml::Value>(&out).expect("compose 渲染结果非法 YAML");
}

#[test]
fn deploy_health_check_references_and_tcp() {
    let env = template_env().unwrap();
    let deploy = render(&env, "scripts/deploy.sh.j2", &sample_ctx()).unwrap();
    // 健康超时透传
    assert!(deploy.contains("MAX=90"));
    // 镜像路径不得双重拼接（img.file 已含 images/ 前缀）
    assert!(deploy.contains("$BASE_DIR/images/mysql_8.0.42.tar"));
    assert!(!deploy.contains("images/images/"));
    // exec 型体检逐元素引用（builder 已 shell_quote）
    assert!(deploy.contains("docker exec \"mysql\" 'sh' '-c'"));
    // tcp 型体检（redis）走 /dev/tcp 探测
    assert!(deploy.contains("</dev/tcp/127.0.0.1/6379"));
    // 生成的脚本本身必须是合法 bash
    let dir = std::env::temp_dir().join(format!("opost-deploychk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("deploy.sh");
    std::fs::write(&f, deploy.replace("\r\n", "\n")).unwrap();
    // 探测 Git Bash（PATH 上的 bash 可能是 WSL，无法访问 C:/ 路径）；不可用则跳过语法检查
    let candidates = [
        "C:/Program Files/Git/usr/bin/bash.exe",
        "C:/Program Files (x86)/Git/usr/bin/bash.exe",
        "D:/Environment/Git/usr/bin/bash.exe",
        "D:/Software/Git/usr/bin/bash.exe",
    ];
    let bash = candidates.iter().find(|p| std::path::Path::new(p).is_file());
    if let Some(bash) = bash {
        let bash_path = f.to_string_lossy().replace('\\', "/");
        let out = std::process::Command::new(bash).args(["-n", &bash_path]).output().unwrap();
        assert!(out.status.success(), "deploy.sh 语法错误: {}", String::from_utf8_lossy(&out.stderr));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ops_sh_renders_and_passes_bash_check() {
    let env = template_env().unwrap();
    let out = render(&env, "scripts/ops.sh.j2", &sample_ctx()).unwrap();
    // 逻辑备份命令必须渲染进 backup
    assert!(out.contains("mysqldump"));
    // report 子命令必须存在且引用的 LOG_DIR 已在头部定义（曾因未定义必现崩溃）
    assert!(out.contains("cmd_report"));
    let defining = out.find("LOG_DIR=").unwrap_or(0);
    let using = out.find("REPORT=").unwrap_or(0);
    assert!(defining < using && defining > 0, "LOG_DIR 定义必须先于使用");
    // bash 语法检查（Git Bash 探测，无则跳过）
    let candidates = [
        "D:/Environment/Git/usr/bin/bash.exe",
        "C:/Program Files/Git/usr/bin/bash.exe",
    ];
    let bash = candidates.iter().find(|p| std::path::Path::new(p).is_file());
    if let Some(bash) = bash {
        let dir = std::env::temp_dir().join(format!("opost-opschk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("ops.sh");
        std::fs::write(&f, out.replace("
", "
")).unwrap();
        let p = f.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        let o = std::process::Command::new(bash).args(["-n", &p]).output().unwrap();
        assert!(o.status.success(), "ops.sh 语法错误: {}", String::from_utf8_lossy(&o.stderr));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn sanitize_keeps_unicode_names() {
    // 中文保留（政务命名常态），分隔符替换为 -
    assert_eq!(sanitize("应用 服务器/01"), "应用-服务器-01");
    assert_eq!(sanitize("app-01_生产"), "app-01_生产");
    assert_eq!(sanitize("---"), "unnamed");
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
