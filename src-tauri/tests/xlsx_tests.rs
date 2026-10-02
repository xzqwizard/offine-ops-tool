//! 集成测试：端口矩阵 xlsx 导出
use offline_preops_tool_lib::models::*;
use offline_preops_tool_lib::xlsx_export::export_port_matrix_xlsx;

fn sample_project() -> Project {
    Project {
        schema_version: 1,
        id: "proj-test".into(),
        name: String::new() + "测试方案",
        customer: String::new(),
        created_at: "2026-09-29T10:00:00Z".into(),
        updated_at: "2026-09-29T12:00:00Z".into(),
        build_config: BuildConfig::default(),
        servers: vec![
            ServerInfo {
                id: "s1".into(),
                name: String::new() + "应用01",
                hostname: "app-01".into(),
                ip: "10.0.0.1".into(),
                os_family: "kylin".into(),
                os_version: "V10".into(),
                arch: "amd64".into(),
                bits: 64,
                cpu_cores: 8,
                memory_gb: 16,
                disk_system_gb: 100,
                disk_data_gb: 500,
                docker_version: "27.5.1".into(),
                docker_data_root: "/data/docker".into(),
                deploy_base_dir: "/opt/stack".into(),
            },
            ServerInfo {
                id: "s2".into(),
                name: String::new() + "数据库01",
                hostname: "db-01".into(),
                ip: "10.0.0.2".into(),
                os_family: "uos".into(),
                os_version: "20".into(),
                arch: "arm64".into(),
                bits: 64,
                cpu_cores: 16,
                memory_gb: 64,
                disk_system_gb: 100,
                disk_data_gb: 1000,
                docker_version: "27.5.1".into(),
                docker_data_root: "/data/docker".into(),
                deploy_base_dir: "/opt/stack".into(),
            },
        ],
        instances: vec![
            MiddlewareInstance {
                id: "i1".into(),
                server_id: "s1".into(),
                template_id: "redis-7".into(),
                image: "redis:7.2.5".into(),
                digest: String::new(),
                instance_name: "redis".into(),
                params: Default::default(),
                ports: vec![PortBinding {
                    name: "redis".into(),
                    host: 6379,
                    container: 6379,
                    protocol: "tcp".into(),
                    expose: true,
                }],
                local_image_tar: String::new(),
            },
            MiddlewareInstance {
                id: "i2".into(),
                server_id: "s2".into(),
                template_id: "mysql-8.0".into(),
                image: "mysql:8.0.42".into(),
                digest: String::new(),
                instance_name: "mysql".into(),
                params: Default::default(),
                ports: vec![PortBinding {
                    name: "mysql".into(),
                    host: 3306,
                    container: 3306,
                    protocol: "tcp".into(),
                    expose: true,
                }],
                local_image_tar: String::new(),
            },
        ],
        registry: None,
        template_snapshots: vec![],
        network_rules: vec![NetworkRule {
            id: "r1".into(),
            from_server_id: "s1".into(),
            to_server_id: "s2".into(),
            to_port: 3306,
            protocol: "tcp".into(),
            description: String::new() + "应用访问数据库",
        }],
    }
}

#[test]
fn export_creates_valid_xlsx() {
    let out = std::env::temp_dir().join(format!("opost-matrix-{}.xlsx", std::process::id()));
    let path =
        export_port_matrix_xlsx(sample_project(), out.to_string_lossy().into_owned()).unwrap();
    let meta = std::fs::metadata(&path).unwrap();
    assert!(meta.len() > 3000, "xlsx 过小: {} bytes", meta.len());
    // xlsx 是 zip 容器，校验魔数 PK
    let mut head = [0u8; 2];
    use std::io::Read;
    std::fs::File::open(&path)
        .unwrap()
        .read_exact(&mut head)
        .unwrap();
    assert_eq!(&head, b"PK", "非 zip/xlsx 容器");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn export_rejects_bad_extension() {
    let err = export_port_matrix_xlsx(sample_project(), "C:/nope/xx.txt".into());
    assert!(err.is_err());
}
