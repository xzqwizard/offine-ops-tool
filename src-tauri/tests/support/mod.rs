use std::{path::Path, process::Command};

pub fn bash() -> String {
    let mut candidates = vec![];
    if let Ok(p) = std::env::var("PREOPS_BASH") {
        candidates.push(p);
    }
    candidates.extend(
        [
            "D:/Environment/Git/usr/bin/bash.exe",
            "C:/Program Files/Git/bin/bash.exe",
            "C:/Program Files/Git/usr/bin/bash.exe",
            "/bin/bash",
        ]
        .map(str::to_string),
    );
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            candidates.push(
                dir.join(if cfg!(windows) { "bash.exe" } else { "bash" })
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    candidates
        .into_iter()
        .find(|p| {
            Path::new(p).is_file()
                && Command::new(p)
                    .arg("-c")
                    .arg("command -v tar sha256sum awk >/dev/null && uname -s")
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
        .expect("Bash 测试未执行：Windows 需 Git Bash/coreutils（WSL 启动器不兼容 Windows 路径），或设置 PREOPS_BASH；不能静默跳过")
}
