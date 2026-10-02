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
    pub config_digest: String,
    pub content_sha256: String,
    pub expected_digest: String,
    pub packed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildServerResult {
    pub server_id: String,
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
        "scripts/upgrade.sh.j2",
        include_str!("../../engine-templates/scripts/upgrade.sh.j2"),
    ),
    (
        "scripts/runtime.sh.j2",
        include_str!("../../engine-templates/scripts/runtime.sh.j2"),
    ),
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
    env.add_filter("shq", |v: String| shell_quote(&v));
    env.add_filter("jsonstr", |v: String| {
        serde_json::to_string(&v).unwrap_or_default()
    });
    for (name, src) in TEMPLATES {
        env.add_template(name, src)
            .map_err(|e| AppError::Serialize(format!("模板 {name} 解析失败: {e}")))?;
    }
    Ok(env)
}

pub fn render(env: &Environment<'_>, name: &str, ctx: &serde_json::Value) -> AppResult<String> {
    if name == "compose/docker-compose.yml.j2" {
        return crate::compose::from_context(ctx);
    }
    let tpl = env
        .get_template(name)
        .map_err(|e| AppError::Serialize(format!("模板 {name} 缺失: {e}")))?;
    tpl.render(ctx)
        .map_err(|e| AppError::Serialize(format!("模板 {name} 渲染失败: {e}")))
}

// ==================== 工具函数 ====================

/// shell 单引号包裹（元素内 ' 转义为 '\''）
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
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
    while out.len() > 64 {
        out.pop();
    }
    let t = out.trim_matches(['-', '.']).to_string();
    if t.is_empty()
        || [
            "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ]
        .contains(&t.to_ascii_lowercase().split('.').next().unwrap_or(""))
    {
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

// ==================== 主流程 ====================

#[tauri::command]
pub async fn build_offline_package(
    app: AppHandle,
    project: Project,
    auto_pull: Option<bool>,
    baseline_build_id: Option<String>,
    task_id: Option<String>,
) -> AppResult<BuildResult> {
    tauri::async_runtime::spawn_blocking(move || {
        EVENT_SCOPE.with(|v| {
            *v.borrow_mut() = Some((
                project.id.clone(),
                task_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            ))
        });
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

thread_local! { static EVENT_SCOPE: std::cell::RefCell<Option<(String,String)>> = const {std::cell::RefCell::new(None)}; }

fn emit(app: &AppHandle, step: &str, detail: &str) {
    let scope = EVENT_SCOPE.with(|v| v.borrow().clone()).unwrap_or_default();
    let _ = app.emit(
        "build-progress",
        json!({ "step": step, "detail": detail,"projectId":scope.0,"taskId":scope.1 }),
    );
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
    build_inner(
        app,
        project,
        auto_pull,
        Some((&baseline, baseline_build_id)),
    )
}

pub use crate::baseline::{baseline_image_set, validate_baseline};

/// Isolated staging directory is only published after all materials and checksums succeed.
pub fn build_inner(
    app: &AppHandle,
    project: &Project,
    auto_pull: bool,
    upgrade: Option<(&serde_json::Value, &str)>,
) -> AppResult<BuildResult> {
    let _storage_lock = store::storage_lock(app)?;
    let mut project_frozen = catalog::freeze(app, project)?;
    project_frozen.registry = project.registry.clone();
    project_frozen.instances = project.instances.clone();
    crate::validation::require(
        &project_frozen,
        &catalog::for_project(app, &project_frozen)?,
        true,
    )?;
    if let Some((b, _)) = upgrade {
        validate_baseline(b, project)?;
    }
    let storage = store::effective_storage(app)?;
    let project_root = PathBuf::from(storage.artifact_root).join(format!(
        "{}-{}",
        sanitize(&project.name),
        &crate::io_util::hash(&project.id)[..12]
    ));
    fs::create_dir_all(&project_root)?;
    let _lock = crate::io_util::lock(&project_root.join(".build.lock"))?;
    let build_id = format!(
        "b-{}-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let staging = project_root.join(format!(".{build_id}.staging"));
    fs::create_dir_all(&staging)?;
    let _cleanup = crate::io_util::Cleanup(staging.clone());
    match build_staged(
        app,
        &project_frozen,
        auto_pull,
        upgrade,
        &staging,
        &build_id,
    ) {
        Ok(mut r) => {
            let final_dir = project_root.join(&build_id);
            r.output_dir = final_dir.to_string_lossy().into_owned();
            for s in &mut r.servers {
                if let Some(file) = &s.package_file {
                    s.package_file = Some(
                        final_dir
                            .join(Path::new(file).file_name().unwrap_or_default())
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
            let path = staging.join("build-manifest.json");
            let mut index: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
            for (n, s) in r.servers.iter().enumerate() {
                index["servers"][n]["packageFile"] = json!(s.package_file);
            }
            crate::io_util::atomic_write(&path, serde_json::to_string_pretty(&index)?.as_bytes())?;
            fs::rename(&staging, &final_dir)?;
            crate::app_ops::__audit(app, "build_complete", &format!("{} {build_id}", project.id));
            Ok(r)
        }
        Err(e) => {
            let failure = project_root.join(format!("{build_id}-failed"));
            fs::create_dir_all(&failure)?;
            fs::write(
                failure.join("build-manifest.json"),
                serde_json::to_vec_pretty(
                    &json!({"schemaVersion":2,"status":"failed","buildId":build_id,"projectId":project.id,"projectName":project.name,"generatedAt":store::now_rfc3339(),"kind":"failed","error":e.to_string(),"servers":[]}),
                )?,
            )?;
            crate::app_ops::__audit(
                app,
                "build_failed",
                &format!("{} {build_id}: {e}", project.id),
            );
            Err(e)
        }
    }
}

fn build_staged(
    app: &AppHandle,
    project: &Project,
    auto_pull: bool,
    upgrade: Option<(&serde_json::Value, &str)>,
    staging: &Path,
    build_id: &str,
) -> AppResult<BuildResult> {
    let now = store::now_rfc3339();
    let build_id = build_id.to_string();
    let catalog = catalog::for_project(app, project)?;
    let out_root = staging.to_path_buf();

    let env = template_env()?;
    let mut result = BuildResult {
        build_id: build_id.clone(),
        output_dir: out_root.to_string_lossy().into_owned(),
        generated_at: now.clone(),
        servers: Vec::new(),
        warnings: Vec::new(),
    };

    emit(
        app,
        "start",
        &format!(
            "开始构建 {}（{} 台服务器）",
            project.name,
            project.servers.len()
        ),
    );

    if project.servers.is_empty() {
        return Err(AppError::Invalid("方案中没有服务器，无法构建".into()));
    }
    // 渲染前字段白名单校验：这些字段会进入 bash 脚本 / compose YAML，
    // 拒绝引号/换行/$()/反引号等可破坏或注入生成物的字符。
    crate::validation::require(project, &catalog, true)?;

    for server in &project.servers {
        emit(
            app,
            "server",
            &format!("处理服务器: {} ({})", server.name, server.arch),
        );
        let baseline_set = upgrade
            .map(|(b, _)| baseline_image_set(b, &server.id, &server.arch))
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
    matrix.push_str(&format!(
        "> 方案: {} | 构建: {} | 生成: {}\n\n",
        project.name, build_id, now
    ));
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
    // ---- 部署顺序推导（端口访问关系 = 依赖图：被依赖最多的先部署） ----
    let deploy_order = deploy_order(project);
    let order_section = if project.network_rules.is_empty() {
        String::new()
    } else {
        let mut s = String::from("\n## 建议部署顺序（按依赖拓扑排序，被依赖的先装）\n\n");
        for (i, (name, reason)) in deploy_order.iter().enumerate() {
            s.push_str(&format!(
                "{}. **{}**{}\n",
                i + 1,
                name,
                if reason.is_empty() {
                    String::new()
                } else {
                    format!("（{reason}）")
                }
            ));
        }
        s.push_str(
            "\n每台部署完成后，等待其暴露端口就绪（bash scripts/ops.sh status）再部署下一台。\n",
        );
        s
    };
    matrix.push_str(&order_section);
    fs::write(out_root.join("PORT-MATRIX.md"), &matrix)?;

    // ---- 构建报告 ----
    let mut report = String::from("# 构建报告\n\n");
    report.push_str(&format!(
        "- 方案: {}（{}）\n",
        project.name, project.customer
    ));
    report.push_str(&format!("- 构建: {}\n- 时间: {}\n", build_id, now));
    report.push_str(&format!(
        "- 输出目录: {}\n\n## 服务器产物\n\n",
        result.output_dir
    ));
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
    let servers_index:Vec<serde_json::Value>=result.servers.iter().map(|s|json!({"serverId":s.server_id,"name":s.name,"arch":s.arch,"osFamily":project.servers.iter().find(|o|o.id==s.server_id).map(|o|&o.os_family),"osVersion":project.servers.iter().find(|o|o.id==s.server_id).map(|o|&o.os_version),"dockerVersion":project.servers.iter().find(|o|o.id==s.server_id).map(|o|&o.docker_version),"dockerDataRoot":project.servers.iter().find(|o|o.id==s.server_id).map(|o|&o.docker_data_root),"deployBaseDir":project.servers.iter().find(|o|o.id==s.server_id).map(|o|&o.deploy_base_dir),"ip":project.servers.iter().find(|o|o.id==s.server_id).map(|o|&o.ip),"dirName":s.dir_name,"packageFile":s.package_file,"sizeBytes":s.size_bytes,"images":s.images,"changedImageCount":s.images.iter().filter(|i|i.packed).count()})).collect();
    let index = json!({
        "schemaVersion":2,"status":"complete",
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

    // ---- 方案快照（构建即存档：审计回溯 / 从历史构建恢复方案） ----
    fs::write(
        out_root.join("project-snapshot.json"),
        serde_json::to_string_pretty(&catalog::freeze(app, project)?)?,
    )?;

    // ---- 摆渡校验材料：SHA256SUMS.all（各服务器包）+ Windows 侧 verify.bat ----
    let mut sum_lines: Vec<String> = Vec::new();
    for s in &result.servers {
        if let Some(pkg) = &s.package_file {
            let p = PathBuf::from(pkg);
            if p.is_file() {
                if let Ok(sum) = sha256_file(&p) {
                    let name = p.file_name().unwrap_or_default().to_string_lossy();
                    sum_lines.push(format!("{sum}  {name}"));
                }
            }
        }
    }
    if sum_lines.is_empty() && !result.servers.is_empty() {
        result.warnings.push(
            "目录模式（dir）不生成 SHA256SUMS.all/verify.bat 摆渡校验材料：建议使用 tar.gz 交付，或对目录自行计算校验".into(),
        );
    }
    if !sum_lines.is_empty() {
        sum_lines.sort();
        fs::write(out_root.join("SHA256SUMS.all"), sum_lines.join("\n") + "\n")?;
        fs::write(out_root.join("verify.bat"), VERIFY_BAT)?;
    }

    emit(app, "done", &format!("产物输出: {}", result.output_dir));
    Ok(result)
}

/// Windows 侧摆渡校验脚本（certutil，Win10+ 自带；消息用 ASCII 避免 GBK 控制台乱码）
const VERIFY_BAT: &str = r#"@echo off
rem Integrity check for delivery packages (generated by OfflinePreOpsTool)
rem Usage: put this file next to SHA256SUMS.all and the packages, then double click
rem chcp 65001: SHA256SUMS.all is UTF-8 (Chinese server names); without it the
rem ANSI codepage garbles non-ASCII filenames and every entry falsely fails
chcp 65001 >nul
pushd "%~dp0"
setlocal enabledelayedexpansion
set FAILED=0
for /f "tokens=1,*" %%a in (SHA256SUMS.all) do (
  if not exist "%%b" (
    echo [X] %%b : FILE MISSING
    set FAILED=1
  ) else (
    certutil -hashfile "%%b" SHA256 > "%TEMP%\opost_hash.txt" 2>nul
    findstr /i /c:"%%a" "%TEMP%\opost_hash.txt" >nul
    if errorlevel 1 (
      echo [X] %%b : HASH MISMATCH
      set FAILED=1
    ) else (
      echo [OK] %%b
    )
  )
)
del "%TEMP%\opost_hash.txt" >nul 2>&1
popd
if %FAILED%==1 (
  echo.
  echo RESULT: FAILED - re-copy the packages before deployment
  pause
  exit /b 1
) else (
  echo.
  echo RESULT: ALL PASSED
  pause
)
"#;

/// 部署顺序：按访问规则拓扑排序（Kahn，被依赖多者先）。
/// 返回 (服务器名, 原因)；依赖他人的排后（reason=依赖哪些机器就绪），
/// 被依赖的排前（reason=哪些机器依赖它）。
fn deploy_order(project: &Project) -> Vec<(String, String)> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let id_name: HashMap<&str, &str> = project
        .servers
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();
    // 边：from 依赖 to（to 必须先就绪）
    let mut indegree: HashMap<&str, usize> =
        project.servers.iter().map(|s| (s.id.as_str(), 0)).collect();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new(); // to -> [from...]
    for r in &project.network_rules {
        if r.from_server_id == r.to_server_id {
            continue;
        }
        *indegree.entry(r.from_server_id.as_str()).or_insert(0) += 1;
        dependents
            .entry(r.to_server_id.as_str())
            .or_default()
            .push(r.from_server_id.as_str());
    }
    // 先按入度 0（不被任何人依赖……实际是"不依赖任何人"先部署）— Kahn：入度=其依赖数
    // 稳定排序：入度 0 的初始集合按服务器在方案中的顺序（HashMap 迭代序随机，
    // 同一方案两次构建的部署顺序文档不能漂移）
    let mut queue: VecDeque<&str> = project
        .servers
        .iter()
        .map(|s| s.id.as_str())
        .filter(|id| indegree.get(*id).copied().unwrap_or(0) == 0)
        .collect();
    let mut ordered: Vec<&str> = Vec::new();
    while let Some(id) = queue.pop_front() {
        ordered.push(id);
        if let Some(deps) = dependents.get(id) {
            for d in deps {
                if let Some(n) = indegree.get_mut(d) {
                    *n -= 1;
                    if *n == 0 {
                        queue.push_back(d);
                    }
                }
            }
        }
    }
    // 环：剩余按原顺序补齐（规则成环时给出提示）
    let mut seen: HashSet<&str> = ordered.iter().copied().collect();
    let mut result: Vec<(String, String)> = ordered
        .iter()
        .map(|id| {
            let name = id_name.get(id).copied().unwrap_or(*id);
            let depended_by: Vec<&str> = dependents
                .get(id)
                .map(|v| {
                    v.iter()
                        .map(|d| id_name.get(d).copied().unwrap_or(d))
                        .collect()
                })
                .unwrap_or_default();
            let depends_on: Vec<&str> = project
                .network_rules
                .iter()
                .filter(|r| r.from_server_id.as_str() == *id && r.to_server_id != r.from_server_id)
                .filter_map(|r| id_name.get(r.to_server_id.as_str()).copied())
                .collect();
            let reason = if !depended_by.is_empty() {
                format!("{} 依赖本机，优先部署", depended_by.join("、"))
            } else if !depends_on.is_empty() {
                format!("需等 {} 就绪后再部署", depends_on.join("、"))
            } else {
                String::new()
            };
            (name.to_string(), reason)
        })
        .collect();
    for s in &project.servers {
        if !seen.contains(s.id.as_str()) {
            result.push((s.name.clone(), "依赖关系成环，顺序仅供参考".into()));
            seen.insert(s.id.as_str());
        }
    }
    result
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
    let dir_name = if is_upgrade {
        format!(
            "upgrade-{}-{}_{}",
            sanitize(&server.name),
            &crate::io_util::hash(&server.id)[..12],
            server.arch
        )
    } else {
        format!(
            "{}-{}_{}",
            sanitize(&server.name),
            &crate::io_util::hash(&server.id)[..12],
            server.arch
        )
    };
    let sdir = out_root.join(&dir_name);
    // 升级包不需要 docker-offline/docs/firewall（目标机已具备 Docker 环境，避免空目录误导现场）
    let dirs: &[&str] = if is_upgrade {
        &["stack", "images", "scripts", "logs", "docs"]
    } else {
        &[
            "stack",
            "images",
            "scripts",
            "docker-offline/packages",
            "docs",
            "firewall",
            "logs",
        ]
    };
    for d in dirs {
        fs::create_dir_all(sdir.join(d))?;
    }

    let instances: Vec<_> = project
        .instances
        .iter()
        .filter(|i| i.server_id == server.id)
        .collect();

    let base_ctx = crate::render_context::server_context(
        project,
        server,
        catalog,
        build_id,
        now,
        &dir_name,
        baseline_build_id,
    );

    // Resolve and verify every required image before comparing its actual content to the baseline.
    let mut images_info = Vec::new();
    let warnings = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pulled = std::collections::HashSet::new();
    for inst in &instances {
        let locked_ref = crate::images::locked_reference(&inst.image, &inst.digest)?;
        let cache = crate::images::cache_file_for_registry(
            app,
            &locked_ref,
            "linux",
            &server.arch,
            project.registry.as_ref(),
        )?;
        let local = !inst.local_image_tar.trim().is_empty();
        let src = if local {
            PathBuf::from(&inst.local_image_tar)
        } else {
            if auto_pull && pulled.insert(cache.clone()) {
                crate::images::pull_image_inner(
                    app,
                    &locked_ref,
                    "linux",
                    &server.arch,
                    &cache,
                    project.registry.as_ref(),
                )?;
            }
            cache.clone()
        };
        let _image_lock = if !local {
            Some(crate::io_util::lock(&src.with_extension("lock"))?)
        } else {
            None
        };
        if !src.is_file() {
            return Err(AppError::NotFound(format!(
                "{} 缺少必需镜像 tar：{}",
                inst.instance_name,
                src.display()
            )));
        }
        let verify_ref = if local {
            inst.image.as_str()
        } else {
            locked_ref.as_str()
        };
        let verified = crate::image_archive::verify(&src, verify_ref, "linux", &server.arch)?;
        let digest = if local {
            if !inst.digest.is_empty()
                || crate::images::parse_reference(&inst.image)?
                    .digest
                    .is_some()
            {
                return Err(AppError::Invalid("本地 docker-save tar 无法证明仓库 manifest digest，请清除 digest 锁定并以 tar SHA256 审计".into()));
            }
            String::new()
        } else {
            let m: serde_json::Value =
                serde_json::from_slice(&fs::read(src.with_extension("tar.meta.json"))?)?;
            if m["version"] != 2
                || m["sha256"].as_str() != Some(&verified.sha256)
                || !m["reference"]
                    .as_str()
                    .is_some_and(|r| crate::images::same_reference(r, &locked_ref))
                || m["platform"].as_str() != Some(&format!("linux/{}", server.arch))
            {
                return Err(AppError::Invalid(
                    "缓存元数据缺失、不匹配或损坏，请重新拉取".into(),
                ));
            }
            m["digest"]
                .as_str()
                .filter(|d| crate::images::valid_digest(d))
                .ok_or_else(|| AppError::Invalid("缓存缺少实际 digest".into()))?
                .to_string()
        };
        if !seen.insert((inst.image.clone(), verified.sha256.clone())) {
            continue;
        }
        let packed = baseline_set
            .is_none_or(|b| !b.contains(&format!("{}@{}", inst.image, verified.sha256)));
        let file = format!(
            "images/image-{}.tar",
            crate::io_util::hash(&format!("{}/{}", inst.image, verified.sha256))
        );
        if packed {
            crate::io_util::require_space(
                &sdir,
                verified.size.saturating_mul(2) + 128 * 1024 * 1024,
            )?;
            let dst = sdir.join(&file);
            // Rewrite locked digest references to a valid local tag. Compose uses the same tag.
            if !local && locked_ref != inst.image {
                crate::images::rewrite_archive(
                    &src,
                    &dst,
                    &crate::images::load_reference(&inst.image)?,
                )?;
            } else {
                copy_file_with_progress(app, &src, &dst, &inst.instance_name)?;
            }
            crate::image_archive::verify(&dst, &inst.image, "linux", &server.arch)?;
        }
        let delivered_sha = if packed {
            crate::io_util::sha256_file(&sdir.join(&file))?
        } else {
            verified.sha256.clone()
        };
        images_info.push(BuildImageInfo {
            reference: inst.image.clone(),
            file,
            sha256: packed.then_some(delivered_sha),
            size_bytes: verified.size,
            digest,
            config_digest: verified.config_digest,
            content_sha256: verified.sha256,
            expected_digest: inst.digest.clone(),
            packed,
        });
    }

    // ---- Docker 离线安装材料（升级包不需要：目标机已具备 Docker 环境） ----
    if !is_upgrade {
        let _pkg_lock = crate::io_util::lock(
            &PathBuf::from(store::effective_storage(app)?.docker_pkg_root).join(".packages.lock"),
        )?;
        let pkg_dirs = crate::docker_pkgs::pick_pkg_dirs(
            app,
            &server.arch,
            &server.docker_version,
            &server.os_family,
            &server.os_version,
        )?;

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
                crate::io_util::require_space(
                    &sdir,
                    entry.metadata()?.len().saturating_mul(2) + 128 * 1024 * 1024,
                )?;
                fs::copy(
                    entry.path(),
                    sdir.join("docker-offline/packages").join(&name),
                )?;
                copied += 1;
            }
        }
        emit(
            app,
            "docker-pkg",
            &format!(
                "Docker 安装材料：{} 个组（{copied} 个文件）",
                pkg_dirs.len()
            ),
        );
        // compose 插件（独立按 arch 匹配）
        match crate::docker_pkgs::compose_plugin_path(app, &server.arch) {
            Some(p) => {
                fs::copy(
                    &p,
                    sdir.join("docker-offline/packages").join("docker-compose"),
                )?;
            }
            None => {
                return Err(AppError::Invalid(format!(
                    "{} 缺少 {} 的 Compose 插件",
                    server.name, server.arch
                )))
            }
        }
    } // end if !is_upgrade

    // ---- 渲染并写文件 ----
    let mut ctx = base_ctx.clone();
    ctx["images"] = json!(images_info
        .iter()
        .filter(|i| i.packed)
        .map(|i| json!({ "file": i.file, "reference": i.reference }))
        .collect::<Vec<_>>());

    ctx["disk_need_mb"] = json!(
        (images_info.iter().map(|i| i.size_bytes).sum::<u64>() * 3 / 1048576
            + dir_size(&sdir.join("docker-offline")) / 1048576
            + 1024)
    );
    let mut writes: Vec<(&str, PathBuf)> = if is_upgrade {
        vec![
            (
                "compose/docker-compose.yml.j2",
                sdir.join("stack/docker-compose.yml"),
            ),
            ("scripts/upgrade.sh.j2", sdir.join("scripts/upgrade.sh")),
            ("scripts/ops.sh.j2", sdir.join("scripts/ops.sh")),
            ("scripts/precheck.sh.j2", sdir.join("scripts/precheck.sh")),
            (
                "scripts/apply-firewall.sh.j2",
                sdir.join("scripts/apply-firewall.sh"),
            ),
            ("docs/README.md.j2", sdir.join("README.md")),
            ("docs/OPS-GUIDE.md.j2", sdir.join("docs/OPS-GUIDE.md")),
            ("docs/PORT-MATRIX.md.j2", sdir.join("docs/PORT-MATRIX.md")),
        ]
    } else {
        vec![
            (
                "compose/docker-compose.yml.j2",
                sdir.join("stack/docker-compose.yml"),
            ),
            ("scripts/precheck.sh.j2", sdir.join("scripts/precheck.sh")),
            ("scripts/deploy.sh.j2", sdir.join("scripts/deploy.sh")),
            ("scripts/ops.sh.j2", sdir.join("scripts/ops.sh")),
            (
                "scripts/apply-firewall.sh.j2",
                sdir.join("scripts/apply-firewall.sh"),
            ),
            (
                "install/install-docker.sh.j2",
                sdir.join("docker-offline/install-docker.sh"),
            ),
            ("docs/README.md.j2", sdir.join("README.md")),
            ("docs/OPS-GUIDE.md.j2", sdir.join("docs/OPS-GUIDE.md")),
            ("docs/PORT-MATRIX.md.j2", sdir.join("docs/PORT-MATRIX.md")),
        ]
    };
    writes.push(("scripts/runtime.sh.j2", sdir.join("scripts/runtime.sh")));
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

    // ---- manifest.json ----
    let manifest = json!({
        "schemaVersion":2,"projectId":project.id,"serverId":server.id,"baselineBuildId":baseline_build_id,
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
            "ports": i.ports.iter().map(|p| format!("{}:{}/{}", p.host, p.container,p.protocol)).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "generatedAt": now,
    });
    fs::write(
        sdir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    fs::write(
        sdir.join("deployment.env"),
        format!(
            "PROJECT_ID={}\nSERVER_ID={}\nBUILD_ID={}\nBASELINE_BUILD_ID={}\n",
            shell_quote(&project.id),
            shell_quote(&server.id),
            shell_quote(build_id),
            shell_quote(baseline_build_id.unwrap_or(""))
        ),
    )?;
    // ---- SHA256SUMS ----
    write_sha256_sums(&sdir)?;

    // ---- 打包 ----
    let mut sr = BuildServerResult {
        server_id: server.id.clone(),
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
        crate::io_util::require_space(out_root, dir_size(&sdir).saturating_add(128 * 1024 * 1024))?;
        let file = File::create(&pkg_path)?;
        let level = if project.build_config.recompress_images {
            Compression::default()
        } else {
            Compression::fast() // Docker save 层并不保证已压缩，快速 gzip 控制交付体积
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
        if total > 0 && (copied.is_multiple_of(64 * 1024 * 1024) || copied == total) {
            emit(
                app,
                "image",
                &format!(
                    "复制镜像 {label}: {} / {} MB",
                    copied / 1048576,
                    total / 1048576
                ),
            );
        }
    }
    Ok(())
}

pub fn image_tag_of(image: &str) -> String {
    // 复用 images 的标准解析（旧实现 before.contains('/') 条件写反，
    // 官方镜像短引用 mysql:8.0 会被误判为 latest）
    crate::images::parse_reference(image)
        .map(|r| r.tag)
        .unwrap_or_else(|_| "latest".into())
}
