use crate::{
    catalog::CatalogFile,
    error::{AppError, AppResult},
    io_util,
    models::{AppSettings, Project, StorageInfo},
    validation,
};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn backup(storage: &StorageInfo, output: &Path) -> AppResult<String> {
    let tmp = io_util::unique_sibling(output);
    let _cleanup = io_util::Cleanup(tmp.clone());
    if let Some(p) = output.parent() {
        fs::create_dir_all(p)?;
    }
    let mut zip = zip::ZipWriter::new(File::create(&tmp)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let config = Path::new(&storage.config_dir);
    let mut settings: AppSettings = if config.join("settings.json").exists() {
        serde_json::from_slice(&fs::read(config.join("settings.json"))?)?
    } else {
        AppSettings::default()
    };
    if let Some(proxy) = &mut settings.proxy {
        proxy.password = None;
    }
    zip.start_file("settings.json", options)
        .map_err(|e| AppError::Io(e.to_string()))?;
    zip.write_all(&serde_json::to_vec_pretty(&settings)?)?;
    let mut catalog = crate::catalog::preset()?;
    if config.join("custom-catalog.json").is_file() {
        let raw = fs::read(config.join("custom-catalog.json"))?;
        let mut custom: CatalogFile = serde_json::from_slice(&raw)?;
        crate::catalog::validate_catalog(&custom)?;
        crate::catalog::scrub_catalog_defaults(&mut custom);
        for t in &custom.templates {
            catalog.templates.retain(|x| x.id != t.id);
            catalog.templates.push(t.clone());
        }
        let raw = serde_json::to_vec_pretty(&custom)?;
        zip.start_file("custom-catalog.json", options)
            .map_err(|e| AppError::Io(e.to_string()))?;
        zip.write_all(&raw)?;
    }
    let mut count = 0;
    let projects = Path::new(&storage.projects_root);
    if projects.is_dir() {
        for e in fs::read_dir(projects)? {
            let e = e?;
            if !e.file_type()?.is_dir() || e.file_type()?.is_symlink() {
                continue;
            }
            let path = e.path().join("project.json");
            if !path.is_file() {
                continue;
            }
            let mut p: Project = serde_json::from_slice(&fs::read(path)?)?;
            crate::store::validate_id(&p.id)?;
            let c = crate::catalog::overlay(catalog.clone(), &p);
            p.template_snapshots = c
                .templates
                .iter()
                .filter(|t| p.instances.iter().any(|i| i.template_id == t.id))
                .cloned()
                .collect();
            crate::credentials::strip_project(&mut p, &c);
            zip.start_file(format!("projects/{}/project.json", p.id), options)
                .map_err(|e| AppError::Io(e.to_string()))?;
            zip.write_all(&serde_json::to_vec_pretty(&p)?)?;
            count += 1;
        }
    }
    zip.finish()
        .map_err(|e| AppError::Io(e.to_string()))?
        .sync_all()?;
    fs::rename(tmp, output)?;
    Ok(format!(
        "已备份 {count} 个方案、设置及自定义目录；仓库/代理及中间件密码已排除"
    ))
}

/// All files are parsed before any live file changes. Stage on each target filesystem;
/// rollback every completed replacement on failure. Storage roots always belong to this machine.
pub fn restore(storage: &StorageInfo, backup: &Path) -> AppResult<String> {
    let mut zip =
        zip::ZipArchive::new(File::open(backup)?).map_err(|e| AppError::Invalid(e.to_string()))?;
    if zip.len() > 10000 {
        return Err(AppError::Invalid("备份条目过多".into()));
    }
    let mut files: Vec<(PathBuf, Vec<u8>)> = vec![];
    let mut seen = HashSet::new();
    let mut total = 0;
    let mut cat = crate::catalog::preset()?;
    let mut projects = vec![];
    let mut settings = None;
    for n in 0..zip.len() {
        let mut entry = zip
            .by_index(n)
            .map_err(|e| AppError::Invalid(e.to_string()))?;
        let name = entry.name().to_string();
        if entry.enclosed_name().is_none()
            || name.contains('\\')
            || name.contains(':')
            || name.split('/').any(|c| c == "..")
            || entry.is_symlink()
            || !seen.insert(name.clone())
        {
            return Err(AppError::Invalid(format!("备份含不安全/重复条目 {name}")));
        }
        if entry.is_dir() {
            continue;
        }
        total += entry.size();
        if entry.size() > 32 * 1024 * 1024 || total > 256 * 1024 * 1024 {
            return Err(AppError::Invalid("备份数据超出限制".into()));
        }
        let mut raw = vec![];
        entry.read_to_end(&mut raw)?;
        match name.as_str() {
            "settings.json" => {
                let mut s: AppSettings = serde_json::from_slice(&raw)?;
                let current_path = Path::new(&storage.config_dir).join("settings.json");
                let current: AppSettings = if current_path.exists() {
                    serde_json::from_slice(&fs::read(current_path)?)?
                } else {
                    AppSettings::default()
                };
                s.projects_root = current.projects_root;
                s.image_cache_root = current.image_cache_root;
                s.docker_pkg_root = current.docker_pkg_root;
                s.artifact_root = current.artifact_root;
                s.log_root = current.log_root;
                if s.schema_version != crate::models::SCHEMA_VERSION {
                    return Err(AppError::Invalid("设置版本不受支持".into()));
                }
                for m in &mut s.registry_mirrors {
                    *m = validation::normalize_host(m)?;
                }
                if let Some(proxy) = &mut s.proxy {
                    proxy.password = None;
                }
                settings = Some(s);
            }
            "custom-catalog.json" => {
                let c: CatalogFile = serde_json::from_slice(&raw)?;
                let mut ids = HashSet::new();
                for t in &c.templates {
                    validation::validate_template(t)?;
                    if !ids.insert(&t.id) {
                        return Err(AppError::Invalid("目录模板 ID 重复".into()));
                    }
                }
                for t in &c.templates {
                    cat.templates.retain(|x| x.id != t.id);
                    cat.templates.push(t.clone());
                }
                files.push((Path::new(&storage.config_dir).join(&name), raw));
            }
            _ => {
                let parts: Vec<_> = name.split('/').collect();
                if parts.len() != 3 || parts[0] != "projects" || parts[2] != "project.json" {
                    return Err(AppError::Invalid(format!("备份含未知文件 {name}")));
                }
                crate::store::validate_id(parts[1])?;
                let mut p: Project = serde_json::from_slice(&raw)?;
                if p.id != parts[1] {
                    return Err(AppError::Invalid("方案目录与 ID 不符".into()));
                }
                if let Some(reg) = &mut p.registry {
                    reg.password.clear();
                }
                projects.push(p);
            }
        }
    }
    let s = settings.ok_or_else(|| AppError::Invalid("备份缺少 settings.json".into()))?;
    files.push((
        Path::new(&storage.config_dir).join("settings.json"),
        serde_json::to_vec_pretty(&s)?,
    ));
    for mut p in projects {
        let effective = crate::catalog::overlay(cat.clone(), &p);
        crate::credentials::strip_project(&mut p, &effective);
        validation::require(&p, &crate::catalog::overlay(cat.clone(), &p), false)?;
        files.push((
            Path::new(&storage.projects_root)
                .join(&p.id)
                .join("project.json"),
            serde_json::to_vec_pretty(&p)?,
        ));
    }
    replace_transaction(&files)?;
    Ok(format!(
        "已恢复 {} 个文件到本机当前存储目录；请重新填写仓库/代理及中间件密码",
        files.len()
    ))
}

pub fn replace_transaction(files: &[(PathBuf, Vec<u8>)]) -> AppResult<()> {
    let mut targets = HashSet::new();
    for (p, _) in files {
        if !targets.insert(p) {
            return Err(AppError::Invalid("恢复目标重复".into()));
        }
    }
    let mut pending = vec![];
    let mut cleanups = vec![];
    for (path, raw) in files {
        let parent = path
            .parent()
            .ok_or_else(|| AppError::Invalid("目标无父目录".into()))?;
        fs::create_dir_all(parent)?;
        if parent.ancestors().any(|p| {
            p.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        }) {
            return Err(AppError::Invalid("恢复目标含符号链接".into()));
        }
        let tmp = io_util::unique_sibling(path);
        cleanups.push(io_util::Cleanup(tmp.clone()));
        io_util::atomic_write(&tmp, raw)?;
        pending.push((path.clone(), tmp, io_util::unique_sibling(path)));
    }
    let mut replaced = vec![];
    let result: AppResult<()> = (|| {
        for (path, tmp, old) in &pending {
            let existed = path.exists();
            if existed {
                fs::rename(path, old)?;
            }
            replaced.push((path.clone(), old.clone(), existed));
            fs::rename(tmp, path)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        let mut failed = vec![];
        for (path, old, existed) in replaced.iter().rev() {
            if path.exists() && fs::remove_file(path).is_err() {
                failed.push(path.display().to_string());
                continue;
            }
            if *existed && fs::rename(old, path).is_err() {
                failed.push(old.display().to_string());
            }
        }
        if !failed.is_empty() {
            return Err(AppError::Io(format!(
                "恢复失败 {e}；自动回退未完成，保留文件：{}",
                failed.join(",")
            )));
        }
        return Err(e);
    }
    for (_, old, existed) in replaced {
        if existed {
            fs::remove_file(old)?;
        }
    }
    Ok(())
}
