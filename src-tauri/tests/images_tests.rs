//! 集成测试：镜像引用解析与 RepoTags 改写。
//! 放在显式 test 目标中（build.rs 会为 test 目标注入 common-controls v6
//! 清单，规避 comctl32 5.82 绑定导致的启动期 ENTRYPOINT_NOT_FOUND）。
use offline_preops_tool_lib::images::{parse_reference, rewrite_repo_tags_for_test};

#[test]
fn parse_reference_docker_hub_shorthand() {
    let r = parse_reference("nginx").unwrap();
    assert_eq!(r.registry, "docker.io");
    assert_eq!(r.repo, "library/nginx");
    assert_eq!(r.tag, "latest");

    let r = parse_reference("nginx:1.25.3").unwrap();
    assert_eq!(r.repo, "library/nginx");
    assert_eq!(r.tag, "1.25.3");
}

#[test]
fn parse_reference_explicit_registry() {
    let r = parse_reference("quay.io/prometheus/prometheus:v2.53.0").unwrap();
    assert_eq!(r.registry, "quay.io");
    assert_eq!(r.repo, "prometheus/prometheus");
    assert_eq!(r.tag, "v2.53.0");

    let r = parse_reference("registry.example.cn:5000/gov/app@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    assert_eq!(r.registry, "registry.example.cn:5000");
    assert_eq!(r.repo, "gov/app");
    assert_eq!(
        r.digest.as_deref(),
        Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
}

#[test]
fn rewrite_repo_tags_replaces_mirror_ref() {
    let body = String::new()
        + "[{\"Config\":\"c1\",\"RepoTags\":[\"docker.m.daocloud.io/library/alpine:3.20\"],"
        + "\"Layers\":[\"l1\"]}]";
    let out = rewrite_repo_tags_for_test(&body, "docker.io/library/alpine:3.20");
    assert!(out.contains("docker.io/library/alpine:3.20"));
    assert!(!out.contains("daocloud"));
    assert!(out.contains("l1"));
}
