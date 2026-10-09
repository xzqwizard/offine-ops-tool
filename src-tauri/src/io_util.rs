use crate::error::{AppError, AppResult};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Cross-process lock. Keep the handle alive for the entire read/modify/write transaction.
pub fn lock(path: &Path) -> AppResult<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    let start = std::time::Instant::now();
    loop {
        crate::tasks::check()?;
        match f.try_lock_exclusive() {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed().as_secs() > 300 {
                    return Err(AppError::Io(
                        "等待材料/存储锁超过 300 秒，请稍后重试".into(),
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(f)
}

pub fn unique_sibling(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4().simple()
    ))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = unique_sibling(path);
    let result = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

pub fn sha256_file(path: &Path) -> AppResult<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        crate::tasks::check()?;
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

pub fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

pub fn require_space(path: &Path, bytes: u64) -> AppResult<()> {
    let p = path
        .ancestors()
        .find(|p| p.exists())
        .ok_or_else(|| AppError::Invalid("无有效磁盘路径".into()))?;
    let free = fs2::available_space(p)?;
    if free < bytes {
        return Err(AppError::Invalid(format!(
            "{} 磁盘不足：需 {} MB，可用 {} MB",
            path.display(),
            bytes / 1048576,
            free / 1048576
        )));
    }
    Ok(())
}

/// Deletes only this guard's unique staging path, including on early error.
pub struct Cleanup(pub PathBuf);
pub struct CheckedWriter<W>(pub W);
impl<W: Write> Write for CheckedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        crate::tasks::check().map_err(std::io::Error::other)?;
        self.0.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.0.is_dir() {
            let _ = fs::remove_dir_all(&self.0);
        } else {
            let _ = fs::remove_file(&self.0);
        }
    }
}

pub fn copy_atomic(src: &Path, dst: &Path) -> AppResult<()> {
    if let Some(p) = dst.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = unique_sibling(dst);
    let _cleanup = Cleanup(tmp.clone());
    let mut reader = File::open(src)?;
    let mut writer = File::create(&tmp)?;
    let mut buf = vec![0; 1024 * 1024];
    loop {
        crate::tasks::check()?;
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
    }
    writer.sync_all()?;
    fs::rename(tmp, dst)?;
    Ok(())
}

/// Publish files and their metadata together under the caller's library lock.
/// If publication fails, restore the previous directory and retain it on rollback failure.
pub fn commit_directory(stage: &Path, dst: &Path) -> AppResult<()> {
    let backup = unique_sibling(dst);
    let had_old = dst.exists();
    if had_old {
        fs::rename(dst, &backup)?;
    }
    if let Err(e) = fs::rename(stage, dst) {
        if had_old {
            if let Err(rollback) = fs::rename(&backup, dst) {
                return Err(AppError::Io(format!(
                    "材料落位失败: {e}；旧材料恢复失败: {rollback}，保留于 {}",
                    backup.display()
                )));
            }
        }
        return Err(e.into());
    }
    if had_old {
        // Publication is committed; leftover backup cleanup must not report a failed write.
        let _ = fs::remove_dir_all(backup);
    }
    Ok(())
}
