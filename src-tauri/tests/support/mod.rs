use std::{path::PathBuf, process::Command};

pub fn bash() -> String {
    let mut candidates = vec![];
    if let Some(p) = std::env::var_os("PREOPS_BASH") {
        candidates.push(PathBuf::from(p));
    }
    if cfg!(windows) {
        for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(key) {
                candidates.push(PathBuf::from(root).join("Git/usr/bin/bash.exe"));
            }
        }
        if let Some(root) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(root).join("Programs/Git/usr/bin/bash.exe"));
        }
    } else {
        candidates.push(PathBuf::from("/bin/bash"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            // Git for Windows commonly exposes cmd/ or bin/ on PATH. Discover the
            // real Bash beside them, including installations outside Program Files.
            if cfg!(windows)
                && dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.eq_ignore_ascii_case("cmd") || name.eq_ignore_ascii_case("bin")
                    })
            {
                if let Some(root) = dir.parent() {
                    candidates.push(root.join("usr/bin/bash.exe"));
                }
            }
            candidates.push(dir.join(if cfg!(windows) { "bash.exe" } else { "bash" }));
        }
    }
    candidates
        .into_iter()
        .find(|p| {
            p.is_file()
                && Command::new(p)
                    .arg("-c")
                    // Git's bin/bash.exe launcher prepends system directories to PATH,
                    // hiding fixture commands such as id/docker. Require the real Bash.
                    .arg("test \"$PATH\" = /usr/bin:/bin && command -v tar sha256sum awk >/dev/null && uname -s")
                    .env("PATH", "/usr/bin:/bin")
                    .output()
                    .is_ok_and(|o| {
                        let kernel = String::from_utf8_lossy(&o.stdout);
                        o.status.success()
                            && (!cfg!(windows)
                                || kernel.starts_with("MINGW")
                                || kernel.starts_with("MSYS"))
                    })
        })
        .expect("Bash 测试未执行：Windows 需 Git usr/bin/bash.exe/coreutils（bin/bash.exe 启动器会重排 PATH，WSL 启动器不兼容 Windows 路径），或设置 PREOPS_BASH；不能静默跳过")
        .to_string_lossy()
        .into_owned()
}
