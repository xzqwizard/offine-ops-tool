//! 镜像纯函数逻辑的手动验证（cargo run --example verify_images_logic）
//! 同时有集成测试覆盖；此示例用于手动核对标准引用解析。
use offline_preops_tool_lib::images::{parse_reference, rewrite_repo_tags_for_test};

fn main() {
    // parse_reference
    let cases = [
        ("nginx", ("docker.io", "library/nginx", "latest")),
        ("nginx:1.25.3", ("docker.io", "library/nginx", "1.25.3")),
        (
            "quay.io/prometheus/prometheus:v2.53.0",
            ("quay.io", "prometheus/prometheus", "v2.53.0"),
        ),
    ];
    for (input, (reg, repo, tag)) in cases {
        let r = parse_reference(input).unwrap();
        assert_eq!(r.registry, reg, "registry of {input}");
        assert_eq!(r.repo, repo, "repo of {input}");
        assert_eq!(r.tag, tag, "tag of {input}");
        println!("[ok] parse {input} -> {reg}/{repo}:{tag}");
    }
    let digest = format!("sha256:{}", "a".repeat(64));
    let r = parse_reference(&format!("registry.example.cn:5000/gov/app@{digest}")).unwrap();
    assert_eq!(r.registry, "registry.example.cn:5000");
    assert_eq!(r.digest.as_deref(), Some(digest.as_str()));
    println!("[ok] parse registry with port + digest");

    // rewrite_repo_tags
    let body = String::new()
        + "[{\"Config\":\"c1\",\"RepoTags\":[\"docker.m.daocloud.io/library/alpine:3.20\"],"
        + "\"Layers\":[\"l1\"]}]";
    let out = rewrite_repo_tags_for_test(&body, "docker.io/library/alpine:3.20");
    assert!(out.contains("docker.io/library/alpine:3.20"));
    assert!(!out.contains("daocloud"));
    assert!(out.contains("l1"));
    println!("[ok] rewrite RepoTags mirror -> canonical");

    println!("全部验证通过");
}
