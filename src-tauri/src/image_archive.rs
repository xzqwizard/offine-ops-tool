use crate::{
    error::{AppError, AppResult},
    images::{load_reference, parse_reference},
    io_util,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Write},
    path::{Component, Path},
};

#[derive(Debug)]
pub struct VerifiedImage {
    pub sha256: String,
    pub config_digest: String,
    pub size: u64,
}
struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        crate::tasks::check().map_err(std::io::Error::other)?;
        self.0.update(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn bad(s: &str) -> AppError {
    AppError::Invalid(s.into())
}
pub fn normalize_arch(s: &str) -> &str {
    match s {
        "loong64" => "loongarch64",
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => s,
    }
}

/// Validate manifest, layer availability/checksums and config platform without buffering layers.
pub fn verify(path: &Path, reference: &str, os: &str, arch: &str) -> AppResult<VerifiedImage> {
    let mut archive = tar::Archive::new(File::open(path)?);
    let mut names = HashSet::new();
    let mut hashes = HashMap::new();
    let mut manifest = None;
    for entry in archive.entries()? {
        crate::tasks::check()?;
        let mut e = entry?;
        let p = e.path()?.into_owned();
        if p.components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
            || p.to_string_lossy().contains('\\')
            || (!e.header().entry_type().is_file() && !e.header().entry_type().is_dir())
        {
            return Err(bad("镜像 tar 包含不安全路径或链接"));
        }
        let name = p.to_string_lossy().trim_start_matches("./").to_string();
        if names.len() > 100000 || !names.insert(name.clone()) {
            return Err(bad("镜像 tar 含重复或过多条目"));
        }
        if name == "manifest.json" {
            if e.size() > 8 * 1024 * 1024 {
                return Err(bad("manifest 过大"));
            }
            let mut body = String::new();
            e.read_to_string(&mut body)?;
            manifest = Some(body);
        } else if e.header().entry_type().is_file() {
            let mut h = HashWriter(Sha256::new());
            std::io::copy(&mut e, &mut h)?;
            let sum = hex::encode(h.0.finalize());
            if name.starts_with("blobs/sha256/") && name.rsplit('/').next() != Some(&sum) {
                return Err(bad("镜像 blob SHA256 不匹配"));
            }
            hashes.insert(name, sum);
        }
    }
    let raw = manifest.ok_or_else(|| bad("不是完整 docker save tar：缺 manifest.json"))?;
    let m: Value = serde_json::from_str(&raw)?;
    let items = m
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or_else(|| bad("每个镜像 tar 必须恰好包含一个镜像"))?;
    let item = &items[0];
    let tags = item["RepoTags"]
        .as_array()
        .ok_or_else(|| bad("镜像缺 RepoTags"))?;
    let expected = load_reference(reference)?;
    let r = parse_reference(reference)?;
    let full = format!("{}/{}:{}", r.registry, r.repo, r.tag);
    if !tags.iter().any(|t| {
        t.as_str().is_some_and(|tag| {
            crate::images::same_reference(tag, &expected)
                || crate::images::same_reference(tag, &full)
        })
    }) {
        return Err(bad("tar 标签与实例镜像引用不一致"));
    }
    let config = item["Config"]
        .as_str()
        .ok_or_else(|| bad("镜像缺 Config"))?;
    let digest = hashes.get(config).ok_or_else(|| bad("镜像缺配置文件"))?;
    if config
        .strip_suffix(".json")
        .is_some_and(|s| s.len() == 64 && s != digest)
    {
        return Err(bad("镜像配置 SHA256 不匹配"));
    }
    let mut archive = tar::Archive::new(File::open(path)?);
    let mut c = None;
    for entry in archive.entries()? {
        let mut e = entry?;
        if e.path()?.to_string_lossy().trim_start_matches("./") == config {
            if e.size() > 8 * 1024 * 1024 {
                return Err(bad("配置文件过大"));
            }
            let mut body = String::new();
            e.read_to_string(&mut body)?;
            c = Some(serde_json::from_str::<Value>(&body)?);
            break;
        }
    }
    let c = c.ok_or_else(|| bad("镜像配置无效"))?;
    if c["os"].as_str() != Some(os)
        || normalize_arch(c["architecture"].as_str().unwrap_or_default()) != normalize_arch(arch)
    {
        return Err(bad("镜像平台与目标服务器不一致"));
    }
    let layers = item["Layers"]
        .as_array()
        .ok_or_else(|| bad("镜像缺 Layers"))?;
    if layers
        .iter()
        .any(|l| l.as_str().is_none_or(|s| !hashes.contains_key(s)))
    {
        return Err(bad("镜像缺少声明的数据层"));
    }
    if let Some(diffs) = c["rootfs"]["diff_ids"].as_array() {
        if diffs.len() != layers.len() {
            return Err(bad("镜像 rootfs 与 Layers 数量不一致"));
        }
        for (layer, diff) in layers.iter().zip(diffs) {
            let layer = layer.as_str().unwrap_or_default();
            // Newer docker-save exports uncompressed layers under OCI blob paths.
            let media_type = item["LayerSources"][format!("sha256:{}", hashes[layer])]["mediaType"]
                .as_str()
                .unwrap_or_default();
            if (layer.ends_with(".tar") || media_type.ends_with(".tar"))
                && diff.as_str() != Some(&format!("sha256:{}", hashes[layer]))
            {
                return Err(bad("镜像数据层与 rootfs 校验和不一致"));
            }
        }
    } else {
        return Err(bad("镜像配置缺少 rootfs.diff_ids"));
    }
    Ok(VerifiedImage {
        sha256: io_util::sha256_file(path)?,
        config_digest: format!("sha256:{digest}"),
        size: path.metadata()?.len(),
    })
}
