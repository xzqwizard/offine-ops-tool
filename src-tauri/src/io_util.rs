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
    f.lock_exclusive()?;
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
    std::io::copy(&mut reader, &mut writer)?;
    writer.sync_all()?;
    fs::rename(tmp, dst)?;
    Ok(())
}
