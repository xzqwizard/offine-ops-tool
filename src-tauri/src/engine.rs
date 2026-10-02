use crate::error::{AppError, AppResult};
use crate::net::{http_get_with_settings, proxy_envs};
use crate::store;
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{copy, Read};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

/// 镜像引擎（crane）管理：
/// - 官方 Windows 发行版：go-containerregistry releases（附 checksums.txt）
/// - 安装位置：<镜像缓存根>/bin/crane.exe（随存储根配置，不占 C 盘应用目录）
/// - 所有外网下载自动应用代理设置
///
const RELEASES_API: &str =
    "https://api.github.com/repos/google/go-containerregistry/releases/latest";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: String,
    /// 下载安装的最后错误（界面展示）
    pub last_error: Option<String>,
}

fn bin_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let s = store::effective_storage(app)?;
    Ok(PathBuf::from(s.image_cache_root).join("bin"))
}

pub fn crane_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(bin_dir(app)?.join("crane.exe"))
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

fn host_asset_arch() -> &'static str {
    // 下载适配宿主机的 crane（办公机架构，与目标服务器架构无关）
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "x86_64",
    }
}

/// 探测已安装的 crane（crane version 输出版本号）
#[tauri::command]
pub fn engine_status(app: AppHandle) -> AppResult<EngineStatus> {
    let path = crane_path(&app)?;
    let marker = bin_dir(&app)?.join("crane-version.txt");
    let version = fs::read_to_string(&marker)
        .ok()
        .map(|v| v.trim().to_string());
    let installed = path.is_file();
    Ok(EngineStatus {
        installed,
        version,
        path: path.to_string_lossy().into_owned(),
        last_error: None,
    })
}

fn emit(app: &AppHandle, step: &str, detail: &str) {
    let _ = app.emit(
        "engine-install",
        serde_json::json!({ "step": step, "detail": detail }),
    );
}

/// 下载并安装 crane 引擎（后台线程执行，幂等：已安装同版本则跳过）
#[tauri::command]
pub async fn engine_install(app: AppHandle, force: bool) -> AppResult<EngineStatus> {
    tauri::async_runtime::spawn_blocking(move || engine_install_sync(&app, force))
        .await
        .map_err(|e| AppError::Io(format!("安装任务异常: {e}")))?
}

fn engine_install_sync(app: &AppHandle, force: bool) -> AppResult<EngineStatus> {
    let settings = store::load_settings(app)?;
    let dir = bin_dir(app)?;
    fs::create_dir_all(&dir)?;
    let exe = dir.join("crane.exe");
    let marker = dir.join("crane-version.txt");

    emit(app, "start", "获取 go-containerregistry 最新版本号…");
    let rel_raw = http_get_with_settings(&settings, RELEASES_API, 20)?;
    let rel: GithubRelease = serde_json::from_str(&rel_raw)
        .map_err(|e| AppError::Serialize(format!("解析 GitHub Release 失败: {e}")))?;
    let tag = rel.tag_name;
    let current = fs::read_to_string(&marker)
        .ok()
        .map(|v| v.trim().to_string());
    if !force && current.as_deref() == Some(tag.as_str()) && exe.is_file() {
        emit(app, "done", &format!("已安装最新版 {tag}，跳过"));
        return engine_status_inner(&exe, &marker);
    }

    // 下载 checksums 并校验
    let arch = host_asset_arch();
    let base = format!("https://github.com/google/go-containerregistry/releases/download/{tag}");
    let asset_name = format!("go-containerregistry_Windows_{arch}.tar.gz");
    emit(app, "checksum", "下载官方校验清单…");
    let checksums = http_get_with_settings(&settings, &format!("{base}/checksums.txt"), 20)?;
    let expect = checksums
        .lines()
        .find_map(|l| {
            let (sum, name) = l.split_once("  ")?;
            (name.trim() == asset_name).then_some(sum.trim().to_string())
        })
        .ok_or_else(|| AppError::Io("checksums.txt 中未找到 Windows 资产条目".into()))?;

    emit(app, "download", &format!("下载 {asset_name}（约 15MB）…"));
    let tmp = dir.join(format!(
        ".{asset_name}.{}.downloading",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _cleanup = crate::io_util::Cleanup(tmp.clone());
    let _lock = crate::io_util::lock(&dir.join(".engine.lock"))?;
    crate::net::download(&settings, &format!("{base}/{asset_name}"), &tmp, 300)?;

    emit(app, "verify", "校验 SHA256…");
    let actual = sha256_file(&tmp)?;
    if !actual.eq_ignore_ascii_case(&expect) {
        let _ = fs::remove_file(&tmp);
        return Err(AppError::Invalid(format!(
            "SHA256 校验失败（期望 {expect}，实际 {actual}），已删除下载文件"
        )));
    }

    emit(app, "extract", "解压 crane.exe…");
    let exe_tmp = crate::io_util::unique_sibling(&exe);
    let _exe_cleanup = crate::io_util::Cleanup(exe_tmp.clone());
    extract_crane(&tmp, &exe_tmp)?;

    // 原子替换
    fs::rename(&exe_tmp, &exe)?;
    let _ = fs::remove_file(&tmp);
    crate::io_util::atomic_write(&marker, tag.as_bytes())?;

    emit(app, "done", &format!("crane {tag} 安装完成"));
    engine_status_inner(&exe, &marker)
}

fn engine_status_inner(exe: &Path, marker: &Path) -> AppResult<EngineStatus> {
    let out = std::process::Command::new(exe).arg("version").output();
    let version = match out {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        _ => fs::read_to_string(marker).ok().map(|v| v.trim().into()),
    };
    Ok(EngineStatus {
        installed: exe.is_file(),
        version,
        path: exe.to_string_lossy().into_owned(),
        last_error: None,
    })
}

/// 从 tar.gz 中提取 crane.exe（保持可执行位由 Windows 忽略）
fn extract_crane(archive: &Path, dest: &Path) -> AppResult<()> {
    let f = File::open(archive)?;
    let gz = GzDecoder::new(f);
    let mut tar = tar::Archive::new(gz);
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.eq_ignore_ascii_case("crane.exe") {
            let mut out = File::create(dest)?;
            copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(AppError::Io("压缩包中未找到 crane.exe".into()))
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let mut f = File::open(path)?;
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

/// 构造已注入代理环境变量的 crane 命令（供 images 模块复用）
pub fn crane_command(app: &AppHandle, args: &[&str]) -> AppResult<std::process::Command> {
    let settings = store::load_settings(app)?;
    let exe = crane_path(app)?;
    if !exe.is_file() {
        return Err(AppError::NotFound(
            "镜像引擎 crane 未安装，请先在「镜像库」页面下载安装".into(),
        ));
    }
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(args);
    for (k, v) in proxy_envs(&settings) {
        cmd.env(k, v);
    }
    Ok(cmd)
}
