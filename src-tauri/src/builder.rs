use crate::catalog;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::store;
use flate2::write::GzEncoder;
use flate2::Compression;
use minijinja::Environment;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

// ==================== 构建结果 ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildImageInfo {
    pub reference: String,
    pub file: String,
    pub sha256: Option<String>,
    pub size_bytes: u64,
    /// 镜像 digest（来自方案锁定或拉取元数据，产物审计用）
    pub digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildServerResult {
    pub name: String,
    pub arch: String,
    pub dir_name: String,
    pub package_file: Option<String>,
    pub size_bytes: u64,
    pub images: Vec<BuildImageInfo>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildResult {
    pub build_id: String,
    pub output_dir: String,
    pub generated_at: String,
    pub servers: Vec<BuildServerResult>,
    pub warnings: Vec<String>,
}

// ==================== 模板注册 ====================

pub const TEMPLATES: &[(&str, &str)] = &[
    (
        "scripts/precheck.sh.j2",
        include_str!("../../engine-templates/scripts/precheck.sh.j2"),
    ),
    (
        "scripts/deploy.sh.j2",
        include_str!("../../engine-templates/scripts/deploy.sh.j2"),
    ),
    (
        "scripts/ops.sh.j2",
        include_str!("../../engine-templates/scripts/ops.sh.j2"),
    ),
    (
        "scripts/apply-firewall.sh.j2",
        include_str!("../../engine-templates/scripts/apply-firewall.sh.j2"),
    ),
    (
        "install/install-docker.sh.j2",
        include_str!("../../engine-templates/install/install-docker.sh.j2"),
    ),
    (
        "compose/docker-compose.yml.j2",
        include_str!("../../engine-templates/compose/docker-compose.yml.j2"),
    ),
    (
        "docs/README.md.j2",
        include_str!("../../engine-templates/docs/README.md.j2"),
    ),
    (
        "docs/OPS-GUIDE.md.j2",
        include_str!("../../engine-templates/docs/OPS-GUIDE.md.j2"),
    ),
    (
        "docs/PORT-MATRIX.md.j2",
        include_str!("../../engine-templates/docs/PORT-MATRIX.md.j2"),
    ),
];

pub fn template_env() -> AppResult<Environment<'static>> {
    let mut env = Environment::new();
    for (name, src) in TEMPLATES {
        env.add_template(name, src)
            .map_err(|e| AppError::Serialize(format!("模板 {name} 解析失败: {e}")))?;
    }
    Ok(env)
}

pub fn render(env: &Environment<'_>, name: &str, ctx: &serde_json::Value) -> AppResult<String> {
    let tpl = env
        .get_template(name)
        .map_err(|e| AppError::Serialize(format!("模板 {name} 缺失: {e}")))?;
    tpl.render(ctx)
        .map_err(|e| AppError::Serialize(format!("模板 {name} 渲染失败: {e}")))
}

// ==================== 工具函数 ====================

/// shell 单引号包裹（元素内 ' 转义为 '\''）
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 读取 docker-archive tar 内 manifest.json 的 RepoTags 列表
fn read_tar_repo_tags(path: &Path) -> AppResult<Vec<String>> {
    let f = File::open(path)?;
    let mut archive = tar::Archive::new(BufReader::new(f));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().replace('\\', "/");
        if name == "manifest.json" {
            let mut body = String::new();
            std::io::Read::read_to_string(&mut entry, &mut body)?;
            let v: serde_json::Value = serde_json::from_str(&body)?;
            let mut out = Vec::new();
            if let Some(arr) = v.as_array() {
                for item in arr {
                    if let Some(tags) = item.get("RepoTags").and_then(|t| t.as_array()) {
                        for t in tags {
                            if let Some(s) = t.as_str() {
                                out.push(s.to_string());
                            }
                        }
                    }
                }
            }
            return Ok(out);
        }
    }
    Ok(vec![])
}

/// 实例镜像的 digest：方案锁定值优先，其次缓存 meta 里的拉取 digest
fn image_digest(app: &AppHandle, inst: &crate::models::MiddlewareInstance, src: &Path) -> String {
    if !inst.digest.is_empty() {
        return inst.digest.clone();
    }
    let meta = src.with_extension("tar.meta.json");
    if let Ok(raw) = fs::read_to_string(&meta) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(d) = v.get("digest").and_then(|x| x.as_str()) {
                return d.to_string();
            }
        }
    }
    let _ = app;
    String::new()
}

/// compose 项目名：仅 [a-z0-9_-] 且字母数字开头（compose 强制规则，
/// 中文/大写/点号会导致现场 `docker compose config` 报错）
fn compose_name(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
            out.push(c)
        } else if c.is_ascii_uppercase() {
            out.push(c.to_ascii_lowercase())
        }
        // 中文与其它字符丢弃
    }
    let t = out.trim_matches('-').to_string();
    if t.is_empty() || !t.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) {
        "stack".into()
    } else {
        t
    }
}

/// 名称转文件/目录安全的片段：保留 Unicode 字母数字（含中文）与 - _ .，
/// 其余替换为 '-'；控制为可控长度。中文服务器/实例名是常态，不能折叠为空。
pub fn sanitize(s: &str) -> String {
    let mut out: String = s
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    out.truncate(64);
    let t = out.trim_matches('-').to_string();
    if t.is_empty() {
        "unnamed".into()
    } else {
        t
    }
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let mut f = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(md) = e.metadata() {
                total += md.len();
            }
        }
    }
    total
}

/// 递归计算目录内全部文件 sha256，写 SHA256SUMS（相对路径）
fn write_sha256_sums(dir: &Path) -> AppResult<()> {
    let mut lines: Vec<String> = Vec::new();
    collect_sums(dir, dir, &mut lines)?;
    lines.sort();
    let mut out = File::create(dir.join("SHA256SUMS"))?;
    out.write_all(lines.join("\n").as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

fn collect_sums(root: &Path, dir: &Path, lines: &mut Vec<String>) -> AppResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "SHA256SUMS" {
            continue;
        }
        if p.is_dir() {
            collect_sums(root, &p, lines)?;
        } else {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            let sum = sha256_file(&p)?;
            lines.push(format!("{sum}  {rel}"));
        }
    }
    Ok(())
}

fn uname_of(arch: &str) -> &'static str {
    match arch {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        other => match other {
            "loongarch64" => "loongarch64",
            "mips64el" => "mips64el",
            _ => "unknown",
        },
    }
}

/// 各中间件的逻辑备份提示（写入手册第 4 章）。
/// 输出路径统一用绝对备份目录 <deploy_base_dir>/backup（ops.sh backup 的同一路径）；
/// 密码类一律在容器内引用环境变量（宿主 shell 无这些变量）。
fn backup_hint(template_id: &str, deploy_base_dir: &str) -> String {
    let b = format!("{deploy_base_dir}/backup");
    match template_id {
        "mysql-8.0" => format!(
            "```bash\nmkdir -p {b}\ndocker exec mysql sh -c 'mysqldump -uroot -p\"$MYSQL_ROOT_PASSWORD\" --single-transaction --all-databases' | gzip > {b}/mysql-$(date +%F).sql.gz\n```"
        ),
        "postgresql-16" => format!(
            "```bash\nmkdir -p {b}\ndocker exec pgsql sh -c 'pg_dumpall -U postgres' | gzip > {b}/pgsql-$(date +%F).sql.gz\n```"
        ),
        "redis-7" => format!(
            "```bash\nmkdir -p {b}\ndocker exec redis sh -c 'redis-cli -a \"$REDIS_PASSWORD\" BGSAVE'\ncp stack/data/redis/dump.rdb {b}/redis-$(date +%F).rdb\n```"
        ),
        "mongodb-7" => format!(
            "```bash\nmkdir -p {b}\ndocker exec mongo sh -c 'mongodump --archive --gzip' > {b}/mongo-$(date +%F).archive.gz\n```"
        ),
        _ => String::new(),
    }
}

/// 渲染前字段白名单校验（阻止脚本/compose 注入与损坏）：
/// - 实例名：字母数字开头 + [A-Za-z0-9_-]（进入 bash 与 compose 服务名，严格）
/// - 镜像引用：[A-Za-z0-9._:/@-]
/// - 服务器名/IP/主机名/目录/参数值：禁止控制字符（换行/回车）与反引号
fn validate_render_fields(project: &Project) -> AppResult<()> {
    let mut errors: Vec<String> = Vec::new();
    for inst in &project.instances {
        let name_ok = !inst.instance_name.is_empty()
            && inst.instance_name.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
            && inst.instance_name[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !name_ok {
            errors.push(format!("实例名「{}」不合法（仅允许字母数字开头，含 _ -）", inst.instance_name));
        }
        if !inst.image.chars().all(|c| c.is_ascii_alphanumeric() || "._:/@-".contains(c)) {
            errors.push(format!("实例「{}」的镜像引用含非法字符: {}", inst.instance_name, inst.image));
        }
        for (k, v) in &inst.params {
            let s = match v {
                serde_json::Value::String(s) => s.as_str(),
                _ => continue, // 非字符串值序列化无换行风险
            };
            if s.contains('\n') || s.contains('\r') || s.contains('`') {
                errors.push(format!(
                    "实例「{}」参数 {k} 含换行/反引号（会破坏生成的编排文件）",
                    inst.instance_name
                ));
            }
        }
    }
    for s in &project.servers {
        for (label, v) in [
            ("服务器名", s.name.as_str()),
            ("主机名", s.hostname.as_str()),
            ("IP", s.ip.as_str()),
            ("部署目录", s.deploy_base_dir.as_str()),
            ("Docker 数据目录", s.docker_data_root.as_str()),
        ] {
            if v.contains('\n') || v.contains('\r') || v.contains('`') || v.contains('"') {
                errors.push(format!("服务器「{}」的{label}含换行/引号/反引号: {v}", s.name));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::Invalid(format!(
            "方案内容含不安全字符，已阻止构建：\n- {}",
            errors.join("\n- ")
        )))
    }
}

// ==================== 主流程 ====================

#[tauri::command]
pub async fn build_offline_package(
    app: AppHandle,
    project: Project,
    auto_pull: Option<bool>,
    baseline_build_id: Option<String>,
) -> AppResult<BuildResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let auto_pull = auto_pull.unwrap_or(true);
        let r = match &baseline_build_id {
            Some(b) if !b.is_empty() => build_upgrade(&app, &project, auto_pull, b),
            _ => build(&app, &project, auto_pull),
        };
        match &r {
            Ok(res) => emit(&app, "done", &format!("构建完成: {}", res.build_id)),
            Err(e) => emit(&app, "error", &e.to_string()),
        }
        r
    })
    .await
    .map_err(|e| AppError::Io(format!("构建任务异常: {e}")))?
}

fn emit(app: &AppHandle, step: &str, detail: &str) {
    let _ = app.emit("build-progress", json!({ "step": step, "detail": detail }));
}

fn build(app: &AppHandle, project: &Project, auto_pull: bool) -> AppResult<BuildResult> {
    build_inner(app, project, auto_pull, None)
}

/// 增量升级包：以 baseline 构建的 manifest 为基线，
/// 仅打包"新增/版本变更"的镜像与新编排，附 upgrade.sh（带回退）
fn build_upgrade(
    app: &AppHandle,
    project: &Project,
    auto_pull: bool,
    baseline_build_id: &str,
) -> AppResult<BuildResult> {
    let baseline = crate::build_history::read_baseline(app, baseline_build_id, &project.id)?;
    // 校验基线归属同一方案
    let b_project = baseline["projectId"].as_str().unwrap_or_default();
    if b_project != project.id {
        return Err(AppError::Invalid(
            "基线构建不属于当前方案，无法对比生成升级包".into(),
        ));
    }
    emit(
        app,
        "baseline",
        &format!("以构建 {baseline_build_id} 为基线生成升级包"),
    );
    build_inner(app, project, auto_pull, Some((&baseline, baseline_build_id)))
}

/// 基线镜像集合：每服务器一个 HashSet<"image@digest-or-tag">
fn baseline_image_set(
    baseline: &serde_json::Value,
    server_name: &str,
) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    if let Some(servers) = baseline["servers"].as_array() {
        for s in servers {
            if s["name"].as_str() != Some(server_name) {
                continue;
            }
            if let Some(imgs) = s["images"].as_array() {
                for i in imgs {
                    let reference = i["reference"].as_str().unwrap_or_default();
                    let digest = i["digest"].as_str().unwrap_or_default();
                    set.insert(if digest.is_empty() {
                        reference.to_string()
                    } else {
                        format!("{reference}@{digest}")
                    });
                }
            }
        }
    }
    set
}

fn build_inner(
    app: &AppHandle,
    project: &Project,
    auto_pull: bool,
    upgrade: Option<(&serde_json::Value, &str)>,
) -> AppResult<BuildResult> {
    let now = store::now_rfc3339();
    let build_id = format!("b-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f"));
    let catalog = catalog::preset()?;

    let storage = store::effective_storage(app)?;
    let out_root = PathBuf::from(&storage.artifact_root)
        .join(sanitize(&project.name))
        .join(&build_id);
    fs::create_dir_all(&out_root)?;

    let env = template_env()?;
    let mut result = BuildResult {
        build_id: build_id.clone(),
        output_dir: out_root.to_string_lossy().into_owned(),
        generated_at: now.clone(),
        servers: Vec::new(),
        warnings: Vec::new(),
    };

    emit(app, "start", &format!("开始构建 {}（{} 台服务器）", project.name, project.servers.len()));

    if project.servers.is_empty() {
        return Err(AppError::Invalid("方案中没有服务器，无法构建".into()));
    }
    // 渲染前字段白名单校验：这些字段会进入 bash 脚本 / compose YAML，
    // 拒绝引号/换行/$()/反引号等可破坏或注入生成物的字符。
    validate_render_fields(project)?;

    for server in &project.servers {
        emit(app, "server", &format!("处理服务器: {} ({})", server.name, server.arch));
        let baseline_set = upgrade
            .map(|(b, _)| baseline_image_set(b, &server.name))
            .unwrap_or_default();
        let sr = build_server(
            app,
            project,
            server,
            &catalog,
            &env,
            &out_root,
            &build_id,
            &now,
            auto_pull,
            if upgrade.is_some() {
                Some(&baseline_set)
            } else {
                None
            },
            upgrade.map(|(_, id)| id),
        )?;
        result.warnings.extend(sr.warnings.clone());
        result.servers.push(sr);
    }

    // ---- 全局端口矩阵 ----
    let mut matrix = String::from("# 全网端口矩阵\n\n");
    matrix.push_str(&format!("> 方案: {} | 构建: {} | 生成: {}\n\n", project.name, build_id, now));
    for rule in &project.network_rules {
        let from = project.servers.iter().find(|s| s.id == rule.from_server_id);
        let to = project.servers.iter().find(|s| s.id == rule.to_server_id);
        matrix.push_str(&format!(
            "| {} | → | {}:{} | {} | {} |\n",
            from.map(|s| s.name.as_str()).unwrap_or("?"),
            to.map(|s| s.name.as_str()).unwrap_or("?"),
            rule.to_port,
            rule.protocol,
            rule.description
        ));
    }
    if project.network_rules.is_empty() {
        matrix.push_str("（未定义跨服务器访问规则）\n");
    }
    fs::write(out_root.join("PORT-MATRIX.md"), &matrix)?;

    // ---- 构建报告 ----
    let mut report = String::from("# 构建报告\n\n");
    report.push_str(&format!("- 方案: {}（{}）\n", project.name, project.customer));
    report.push_str(&format!("- 构建: {}\n- 时间: {}\n", build_id, now));
    report.push_str(&format!("- 输出目录: {}\n\n## 服务器产物\n\n", result.output_dir));
    for s in &result.servers {
        report.push_str(&format!(
            "### {} ({})\n- 包: {}\n- 体积: {:.2} MB\n- 镜像: {} 个\n",
            s.name,
            s.arch,
            s.package_file.as_deref().unwrap_or("（目录模式，未打包）"),
            s.size_bytes as f64 / 1048576.0,
            s.images.len()
        ));
        for w in &s.warnings {
            report.push_str(&format!("- ⚠ {w}\n"));
        }
        report.push('\n');
    }
    if !result.warnings.is_empty() {
        report.push_str("## 全局警告\n\n");
        for w in &result.warnings {
            report.push_str(&format!("- ⚠ {w}\n"));
        }
    }
    fs::write(out_root.join("build-report.md"), &report)?;

    // ---- 构建索引（机器可读：升级包对比基线 / 构建历史列表） ----
    // 升级包只含变更镜像，但 manifest 合并基线镜像集（含未变更项），
    // 保证该构建可作为下一轮增量对比的基线
    let servers_index: Vec<serde_json::Value> = result
        .servers
        .iter()
        .map(|s| {
            let mut all: Vec<serde_json::Value> = s
                .images
                .iter()
                .map(|i| json!({ "reference": i.reference, "digest": i.digest, "file": i.file }))
                .collect();
            if let Some((b, _)) = upgrade {
                let baseline_imgs = b["servers"].as_array().and_then(|arr| {
                    arr.iter()
                        .find(|bs| bs["name"].as_str() == Some(s.name.as_str()))
                        .and_then(|bs| bs["images"].as_array().cloned())
                });
                if let Some(bimgs) = baseline_imgs {
                    let changed_refs: std::collections::HashSet<String> = all
                        .iter()
                        .filter_map(|i| i["reference"].as_str().map(String::from))
                        .collect();
                    for bi in bimgs {
                        if let Some(r) = bi["reference"].as_str() {
                            if !changed_refs.contains(r) {
                                all.push(bi.clone());
                            }
                        }
                    }
                }
            }
            json!({
                "name": s.name, "arch": s.arch, "dirName": s.dir_name,
                "packageFile": s.package_file, "sizeBytes": s.size_bytes,
                "images": all,
            })
        })
        .collect();
    let index = json!({
        "buildId": result.build_id,
        "projectId": project.id,
        "projectName": project.name,
        "generatedAt": result.generated_at,
        "kind": if upgrade.is_some() { "upgrade" } else { "full" },
        "baselineBuildId": upgrade.map(|(_, id)| id),
        "servers": servers_index,
    });
    fs::write(
        out_root.join("build-manifest.json"),
        serde_json::to_string_pretty(&index)?,
    )?;

    emit(app, "done", &format!("产物输出: {}", result.output_dir));
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn build_server(
    app: &AppHandle,
    project: &Project,
    server: &ServerInfo,
    catalog: &catalog::CatalogFile,
    env: &Environment<'_>,
    out_root: &Path,
    build_id: &str,
    now: &str,
    auto_pull: bool,
    // 升级模式：该服务器的基线镜像集合（Some=仅打包差异镜像）
    baseline_set: Option<&std::collections::HashSet<String>>,
    // 升级模式的基线构建号
    baseline_build_id: Option<&str>,
) -> AppResult<BuildServerResult> {
    let is_upgrade = baseline_set.is_some();
    let tpl_of = |id: &str| catalog.templates.iter().find(|t| t.id == id);
    let dir_name = if is_upgrade {
        format!("upgrade-{}_{}", sanitize(&server.name), server.arch)
    } else {
        format!("{}_{}", sanitize(&server.name), server.arch)
    };
    let sdir = out_root.join(&dir_name);
    // 升级包不需要 docker-offline/docs/firewall（目标机已具备 Docker 环境，避免空目录误导现场）
    let dirs: &[&str] = if is_upgrade {
        &["stack", "images", "scripts", "logs"]
    } else {
        &["stack", "images", "scripts", "docker-offline/packages", "docs", "firewall", "logs"]
    };
    for d in dirs {
        fs::create_dir_all(sdir.join(d))?;
    }

    let instances: Vec<_> = project
        .instances
        .iter()
        .filter(|i| i.server_id == server.id)
        .collect();

    // ---- 实例渲染上下文 ----
    let inst_ctx: Vec<serde_json::Value> = instances
        .iter()
        .map(|inst| {
            let tpl = tpl_of(&inst.template_id);
            // env 值做 compose 安全转义：' → ''（YAML 单引号），$ → $$（禁止 compose 插值）
            let env_pairs: Vec<serde_json::Value> = inst
                .params
                .iter()
                .map(|(k, v)| {
                    let val = value_to_plain_string(v).replace('\'', "''").replace('$', "$$");
                    json!({ "k": k, "v": val })
                })
                .collect();
            let ports_csv = inst
                .ports
                .iter()
                .map(|p| format!("{}→{}", p.host, p.container))
                .collect::<Vec<_>>()
                .join(",");
            let data_volume = tpl
                .and_then(|t| {
                    if t.data_volume.is_empty() {
                        None
                    } else {
                        Some(t.data_volume.clone())
                    }
                })
                .unwrap_or_else(|| "/data".into());
            let data_user = tpl.and_then(|t| t.data_user.clone());
            let command = tpl.map(|t| t.command.clone()).unwrap_or_default();
            let health_cmd: Option<String> = tpl
                .and_then(|t| t.health_check.as_ref())
                .and_then(|h| {
                    if h.r#type == "exec" && !h.cmd.is_empty() {
                        // 逐元素 shell 单引号化：含空格/||/()/$ 的元素（如 sh -c "curl a || wget b"）
                        // 裸拼接会被 deploy.sh 的 bash 解释成自己的运算符，体检必坏
                        Some(
                            h.cmd
                                .iter()
                                .map(|c| shell_quote(c))
                                .collect::<Vec<_>>()
                                .join(" "),
                        )
                    } else {
                        None
                    }
                });
            let health_tcp: Option<u32> = tpl
                .and_then(|t| t.health_check.as_ref())
                .and_then(|h| {
                    if h.r#type == "tcp" {
                        inst.ports.iter().find(|p| p.expose).map(|p| p.container)
                    } else {
                        None
                    }
                });
            let health_timeout = tpl.and_then(|t| t.health_timeout_sec).unwrap_or(60);
            json!({
                "instance_name": inst.instance_name,
                "image": inst.image,
                "env": env_pairs,
                "ports": inst.ports.iter().map(|p| json!({
                    "host": p.host, "container": p.container,
                    "protocol": p.protocol, "expose": p.expose
                })).collect::<Vec<_>>(),
                "data_volume": data_volume,
                "data_user": data_user,
                "command": command,
                "health_cmd": health_cmd,
                "health_tcp": health_tcp,
                "health_timeout": health_timeout,
                "ports_csv": ports_csv,
                "backup_hint": backup_hint(&inst.template_id, &server.deploy_base_dir),
                "min_memory_gb": tpl.map(|t| t.min_memory_gb).unwrap_or(0.5),
            })
        })
        .collect();

    let services_csv = instances
        .iter()
        .map(|i| i.instance_name.clone())
        .collect::<Vec<_>>()
        .join(",");
    let exposed: Vec<_> = instances
        .iter()
        .flat_map(|i| i.ports.iter().filter(|p| p.expose).map(|p| p.host).collect::<Vec<_>>())
        .collect();
    let exposed_ports_csv = exposed
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");

    // 访问规则（指向本机）
    let allowed_rules: Vec<serde_json::Value> = project
        .network_rules
        .iter()
        .filter(|r| r.to_server_id == server.id)
        // 过滤来源 IP 为空的规则：空 address= 会生成非法防火墙命令
        .filter(|r| {
            project
                .servers
                .iter()
                .find(|s| s.id == r.from_server_id)
                .map(|s| !s.ip.trim().is_empty())
                .unwrap_or(false)
        })
        .map(|r| {
            let from = project.servers.iter().find(|s| s.id == r.from_server_id);
            json!({
                "from_name": from.map(|s| s.name.clone()).unwrap_or_default(),
                "from_ip": from.map(|s| s.ip.clone()).unwrap_or_default(),
                "to_port": r.to_port,
                "protocol": r.protocol,
                "description": r.description,
            })
        })
        .collect();

    // 本机端口表（含放行来源）
    let local_ports: Vec<serde_json::Value> = instances
        .iter()
        .flat_map(|i| {
            i.ports
                .iter()
                .filter(|p| p.expose)
                .map(|p| {
                    let sources = project
                        .network_rules
                        .iter()
                        .filter(|r| r.to_server_id == server.id && r.to_port == p.host)
                        .map(|r| {
                            project
                                .servers
                                .iter()
                                .find(|s| s.id == r.from_server_id)
                                .map(|s| s.ip.clone())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    json!({
                        "host": p.host, "container": p.container,
                        "protocol": p.protocol, "instance_name": i.instance_name,
                        "sources": if sources.is_empty() { "（本机/按需）".to_string() } else { sources },
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let mem_need_gb: u32 = inst_ctx
        .iter()
        .map(|v| {
            let g = v.get("min_memory_gb").and_then(|x| x.as_f64()).unwrap_or(0.5);
            g.ceil() as u32
        })
        .sum();
    let kernel_reqs: Vec<String> = instances
        .iter()
        .filter_map(|i| tpl_of(&i.template_id).map(|t| t.kernel_reqs.clone()))
        .flatten()
        .collect();
    let kernel_reqs_raw = kernel_reqs.join(",");

    let server_ctx = json!({
        "name": server.name, "arch": server.arch,
        "os_family": server.os_family, "os_version": server.os_version,
        "ip": server.ip, "docker_version": server.docker_version,
        "docker_data_root": server.docker_data_root,
        "deploy_base_dir": server.deploy_base_dir,
    });
    let base_ctx = json!({
        "project_name": project.name,
        "build_id": build_id,
        "generated_at": now,
        "server": server_ctx,
        "instances": inst_ctx,
        "services_csv": services_csv,
        "exposed_ports_csv": exposed_ports_csv,
        "allowed_rules": allowed_rules,
        "local_ports": local_ports,
        "uname_arch": uname_of(&server.arch),
        "dir_name": dir_name,
        "stack_name": compose_name(&project.name),
        "baseline_build_id": baseline_build_id.unwrap_or(""),
        "mem_need_gb": std::cmp::max(mem_need_gb, 1),
        "disk_need_mb": 15360u32, // M0 预估：系统 10G + 镜像余量
        "kernel_reqs": kernel_reqs,
        "kernel_reqs_raw": kernel_reqs_raw,
    });

    // ---- 镜像解析与复制：本地 tar > 缓存 > 在线拉取（auto_pull） ----
    // 升级模式：与基线集合比对，未变更的镜像跳过（不装入升级包）
    // 比对键与基线侧对称：digest 优先，否则回退缓存 meta 的拉取 digest，最后裸引用兜底
    let mut images_info: Vec<BuildImageInfo> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for inst in &instances {
        if let Some(base) = baseline_set {
            let cache_probe = crate::images::cache_file_for(app, &inst.image, "linux", &server.arch)
                .unwrap_or_else(|_| PathBuf::from("N/A"));
            let meta_digest = image_digest(app, inst, &cache_probe);
            let key = if meta_digest.is_empty() {
                inst.image.clone()
            } else {
                format!("{}@{}", inst.image, meta_digest)
            };
            // 裸引用兜底：基线时代码未记录 digest（本地 tar 场景）也能匹配
            if base.contains(&key) || base.contains(&inst.image) {
                emit(
                    app,
                    "image",
                    &format!("「{}」与基线一致，跳过（{}）", inst.instance_name, inst.image),
                );
                continue;
            }
        }
        let resolved: Option<std::path::PathBuf> = if !inst.local_image_tar.trim().is_empty() {
            let p = PathBuf::from(inst.local_image_tar.trim());
            if p.is_file() {
                Some(p)
            } else {
                warnings.push(format!(
                    "「{}」本地镜像 tar 不存在: {}",
                    inst.instance_name,
                    p.display()
                ));
                None
            }
        } else {
            let cache = crate::images::cache_file_for(app, &inst.image, "linux", &server.arch)?;
            if cache.is_file() {
                Some(cache)
            } else if auto_pull {
                emit(
                    app,
                    "image",
                    &format!("缓存未命中，在线拉取 {} (linux/{})", inst.image, server.arch),
                );
                match crate::images::pull_image_inner(app, &inst.image, "linux", &server.arch, &cache, project.registry.as_ref())
                {
                    Ok(_) => Some(cache),
                    Err(e) => {
                        warnings.push(format!(
                            "「{}」在线拉取失败（产物不含该镜像，现场需自行 docker load）: {}",
                            inst.instance_name, e
                        ));
                        None
                    }
                }
            } else {
                warnings.push(format!(
                    "「{}」无本地 tar 且缓存未命中（未启用自动拉取），产物不含该镜像",
                    inst.instance_name
                ));
                None
            }
        };
        let Some(src) = resolved else { continue };
        let file_name = format!(
            "{}_{}.tar",
            sanitize(&inst.instance_name),
            sanitize(&image_tag_of(&inst.image))
        );
        let dst = sdir.join("images").join(&file_name);
        copy_file_with_progress(app, &src, &dst, &inst.instance_name)?;
        // 本地导入的 tar 校验 RepoTags 与实例引用一致：不一致时现场 docker load 后
        // compose 按引用找不到镜像（离线无法拉取），提前在构建期暴露
        if !inst.local_image_tar.trim().is_empty() {
            match read_tar_repo_tags(&dst) {
                Ok(tags) if !tags.is_empty() => {
                    if !tags.iter().any(|t| t == &inst.image) {
                        warnings.push(format!(
                            "「{}」本地 tar 的镜像标签 {:?} 与实例引用 {} 不一致：现场 docker load 后 compose 将找不到该镜像，请确认 tar 来源",
                            inst.instance_name, tags, inst.image
                        ));
                    }
                }
                Ok(_) => warnings.push(format!(
                    "「{}」本地 tar 内未解析到 RepoTags，无法校验与引用 {} 的一致性",
                    inst.instance_name, inst.image
                )),
                Err(e) => warnings.push(format!(
                    "「{}」本地 tar 读取失败（{}）：可能不是 docker save 格式",
                    inst.instance_name, e
                )),
            }
        }
        let sha = sha256_file(&dst).ok();
        let size = dst.metadata().map(|m| m.len()).unwrap_or(0);
        images_info.push(BuildImageInfo {
            reference: inst.image.clone(),
            file: format!("images/{file_name}"),
            sha256: sha,
            size_bytes: size,
            digest: image_digest(app, inst, &src),
        });
    }

    // ---- 渲染并写文件 ----
    let mut ctx = base_ctx.clone();
    ctx["images"] = json!(images_info
        .iter()
        .map(|i| json!({ "file": i.file, "reference": i.reference }))
        .collect::<Vec<_>>());

    let writes: Vec<(&str, PathBuf)> = if is_upgrade {
        vec![
            ("compose/docker-compose.yml.j2", sdir.join("stack/docker-compose.yml")),
            ("scripts/upgrade.sh.j2", sdir.join("scripts/upgrade.sh")),
            ("scripts/ops.sh.j2", sdir.join("scripts/ops.sh")),
        ]
    } else {
        vec![
            ("compose/docker-compose.yml.j2", sdir.join("stack/docker-compose.yml")),
            ("scripts/precheck.sh.j2", sdir.join("scripts/precheck.sh")),
            ("scripts/deploy.sh.j2", sdir.join("scripts/deploy.sh")),
            ("scripts/ops.sh.j2", sdir.join("scripts/ops.sh")),
            ("scripts/apply-firewall.sh.j2", sdir.join("scripts/apply-firewall.sh")),
            ("install/install-docker.sh.j2", sdir.join("docker-offline/install-docker.sh")),
            ("docs/README.md.j2", sdir.join("README.md")),
            ("docs/OPS-GUIDE.md.j2", sdir.join("docs/OPS-GUIDE.md")),
            ("docs/PORT-MATRIX.md.j2", sdir.join("docs/PORT-MATRIX.md")),
        ]
    };
    for (tpl, path) in writes {
        let content = render(env, tpl, &ctx)?;
        // compose 渲染产物做 YAML 语法校验：非法 YAML 在现场 compose config 才暴露
        // 就太晚（离线环境排障困难），构建期必须拦截
        if path.to_string_lossy().ends_with("docker-compose.yml") {
            serde_yaml::from_str::<serde_yaml::Value>(&content).map_err(|e| {
                AppError::Serialize(format!("生成的 docker-compose.yml 语法非法: {e}"))
            })?;
        }
        let mut f = File::create(&path)?;
        f.write_all(content.as_bytes())?;
    }

    // ---- Docker 离线安装材料（升级包不需要：目标机已具备 Docker 环境） ----
    if !is_upgrade {
        let pkg_dirs = crate::docker_pkgs::pick_pkg_dirs(app, &server.arch, &server.docker_version, &server.os_family);
    if pkg_dirs.is_empty() {
        warnings.push(format!(
            "「{}」(Docker {} {} 系) 无匹配离线安装包：构建产物不含 Docker 安装材料，deploy.sh 将要求现场已装 Docker 或人工安装",
            server.name, server.docker_version, server.os_family
        ));
    } else {
        let is_static = pkg_dirs
            .iter()
            .any(|d| d.file_name().map(|n| n.to_string_lossy().contains("-static-")).unwrap_or(false));
        // rpm/deb 主包但缺依赖组（cli/containerd）：现场 localinstall 依赖解析会失败
        let deps_found = pkg_dirs.iter().any(|d| d.file_name().map(|n| n.to_string_lossy().contains("-deps")).unwrap_or(false));
        if !is_static && !deps_found && crate::docker_pkgs::os_pkg_class(&server.os_family) != "static" {
            warnings.push(format!(
                "「{}」的 Docker {} 包缺少依赖组（docker-ce-cli/containerd 等）：建议在安装包库一并导入，否则现场离线安装可能因依赖不全失败",
                server.name, server.docker_version
            ));
        }
        let mut copied = 0u32;
        for pkg_dir in &pkg_dirs {
            let pkgs_src = pkg_dir.join("packages");
            for entry in fs::read_dir(&pkgs_src)? {
                let entry = entry?;
                if !entry.path().is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                fs::copy(entry.path(), sdir.join("docker-offline/packages").join(&name))?;
                copied += 1;
            }
        }
        emit(
            app,
            "docker-pkg",
            &format!("Docker 安装材料：{} 个组（{copied} 个文件）", pkg_dirs.len()),
        );
    }
    // compose 插件（独立按 arch 匹配）
    match crate::docker_pkgs::compose_plugin_path(app, &server.arch) {
        Some(p) => {
            fs::copy(&p, sdir.join("docker-offline/packages").join("docker-compose"))?;
        }
        None => {
            warnings.push(format!(
                "「{}」缺 docker compose 插件（{} 架构）：deploy.sh 安装 Docker 后将因缺 compose 中止",
                server.name, server.arch
            ));
        }
    }
    } // end if !is_upgrade

    // ---- manifest.json ----
    let manifest = json!({
        "buildId": build_id,
        "project": { "name": project.name, "customer": project.customer },
        "server": {
            "name": server.name, "arch": server.arch,
            "os": format!("{} {}", server.os_family, server.os_version),
            "dockerVersion": server.docker_version,
        },
        "images": images_info,
        "instances": instances.iter().map(|i| json!({
            "name": i.instance_name, "image": i.image,
            "ports": i.ports.iter().map(|p| format!("{}:{}", p.host, p.container)).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "generatedAt": now,
    });
    fs::write(
        sdir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    // ---- SHA256SUMS ----
    write_sha256_sums(&sdir)?;

    // ---- 打包 ----
    let mut sr = BuildServerResult {
        name: server.name.clone(),
        arch: server.arch.clone(),
        dir_name: dir_name.clone(),
        package_file: None,
        size_bytes: 0,
        images: images_info,
        warnings,
    };

    if project.build_config.package_format != "dir" {
        let pkg_path = out_root.join(format!("{dir_name}.tar.gz"));
        emit(app, "package", &format!("打包: {}", pkg_path.display()));
        let file = File::create(&pkg_path)?;
        let level = if project.build_config.recompress_images {
            Compression::default()
        } else {
            Compression::none() // 镜像层已压缩，store 模式加速
        };
        let gz = GzEncoder::new(file, level);
        let mut tar = tar::Builder::new(gz);
        tar.append_dir_all(&dir_name, &sdir)
            .map_err(|e| AppError::Io(format!("tar 打包失败: {e}")))?;
        tar.into_inner()
            .map_err(|e| AppError::Io(format!("tar 收尾失败: {e}")))?
            .finish()
            .map_err(|e| AppError::Io(format!("gzip 收尾失败: {e}")))?;
        sr.package_file = Some(pkg_path.to_string_lossy().into_owned());
        sr.size_bytes = pkg_path.metadata().map(|m| m.len()).unwrap_or(0);
        fs::remove_dir_all(&sdir)?;
    } else {
        sr.size_bytes = dir_size(&sdir);
    }

    Ok(sr)
}

fn copy_file_with_progress(app: &AppHandle, src: &Path, dst: &Path, label: &str) -> AppResult<()> {
    let mut reader = BufReader::new(File::open(src)?);
    let mut writer = File::create(dst)?;
    let mut buf = [0u8; 4 * 1024 * 1024];
    let mut copied = 0u64;
    let total = src.metadata().map(|m| m.len()).unwrap_or(0);
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        copied += n as u64;
        if total > 0 && (copied % (64 * 1024 * 1024) == 0 || copied == total) {
            emit(
                app,
                "image",
                &format!("复制镜像 {label}: {} / {} MB", copied / 1048576, total / 1048576),
            );
        }
    }
    Ok(())
}

fn value_to_plain_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub fn image_tag_of(image: &str) -> String {
    // 复用 images 的标准解析（旧实现 before.contains('/') 条件写反，
    // 官方镜像短引用 mysql:8.0 会被误判为 latest）
    crate::images::parse_reference(image)
        .map(|r| r.tag)
        .unwrap_or_else(|_| "latest".into())
}

