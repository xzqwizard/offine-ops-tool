//! 集成测试：旧版数据迁移的目录合并逻辑
use offline_preops_tool_lib::store::merge_dir_recursive;
use std::fs;
use std::path::PathBuf;

fn temp_root(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "opost-migrate-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn merge_copies_missing_and_keeps_existing() {
    let root = temp_root("basic");
    let src = root.join("src");
    let dst = root.join("dst");
    fs::create_dir_all(src.join("a")).unwrap();
    fs::create_dir_all(dst.join("a")).unwrap();
    fs::write(src.join("a/old.txt"), b"old-content").unwrap();
    fs::write(src.join("a/keep.txt"), b"new-src").unwrap();
    fs::create_dir_all(src.join("b")).unwrap();
    fs::write(src.join("b/new.txt"), b"new-file").unwrap();
    fs::write(dst.join("a/old.txt"), b"existing-dst").unwrap(); // 已存在 → 不覆盖

    let (files, _bytes) = merge_dir_recursive(&src, &dst).unwrap();

    assert_eq!(files, 2); // keep.txt + new.txt
    assert_eq!(fs::read_to_string(dst.join("a/old.txt")).unwrap(), "existing-dst");
    assert_eq!(fs::read_to_string(dst.join("a/keep.txt")).unwrap(), "new-src");
    assert_eq!(fs::read_to_string(dst.join("b/new.txt")).unwrap(), "new-file");
    // 源目录保持不变（非破坏性）
    assert!(src.join("a/old.txt").is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn merge_missing_src_is_noop() {
    let root = temp_root("noop");
    let dst = root.join("dst");
    let (files, bytes) = merge_dir_recursive(&root.join("no-such-src"), &dst).unwrap();
    assert_eq!((files, bytes), (0, 0));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn merge_skips_temp_files() {
    let root = temp_root("temp");
    let src = root.join("src");
    let dst = root.join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.tar.downloading"), b"x").unwrap();
    fs::write(src.join(".hidden"), b"x").unwrap();
    fs::write(src.join("good.tar"), b"y").unwrap();

    let (files, _) = merge_dir_recursive(&src, &dst).unwrap();

    assert_eq!(files, 1); // 仅 good.tar
    assert!(!dst.join("a.tar.downloading").exists());
    assert!(!dst.join(".hidden").exists());
    assert!(dst.join("good.tar").is_file());
    let _ = fs::remove_dir_all(&root);
}
