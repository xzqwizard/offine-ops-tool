fn main() {
    tauri_build::build();

    // windows-gnu + rust-lld 链接方案：lld 不允许重复资源，而 gcc specs 默认
    // 注入 default-manifest.o 与 Tauri 内嵌清单冲突 → 通过自定义 specs 去除。
    // 路径由 build script 按当前包位置动态生成（可移植，勿硬编码）。
    // 详见 .cargo/config.toml 说明与本目录 mingw-specs-nodm.txt。
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let specs = std::path::Path::new(&manifest_dir)
        .join(".cargo/mingw-specs-nodm.txt")
        .to_string_lossy()
        .replace('\\', "/");
    println!("cargo:rustc-link-arg=-specs={specs}");

    // WebView2Loader.dll 落位修复：webview2-com-sys 只把它拷到 target/<profile> 根，
    // 而 deps/examples 子目录中的测试与示例 exe 动态依赖它，找不到时进程在
    // 启动期静默失败（bash 127 / cargo 显示 STATUS_ENTRYPOINT_NOT_FOUND）。
    // 复制到各子目录（exe 同目录为 DLL 搜索第一优先级）。
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let out_dir = std::path::Path::new(&out_dir);
        // OUT_DIR = target/<profile>/build/<pkg>-<hash>/out
        if let Some(profile_dir) = out_dir.ancestors().nth(3) {
            let src = profile_dir.join("WebView2Loader.dll");
            if src.is_file() {
                for sub in ["deps", "examples"] {
                    let dst = profile_dir.join(sub).join("WebView2Loader.dll");
                    if !dst.exists() {
                        let _ = std::fs::copy(&src, &dst);
                    }
                }
            } else {
                // webview2-com-sys 尚未拷贝时（首次构建顺序），rerun 补一次
                println!("cargo:rerun-if-changed={}", src.display());
            }
        }
    }
}
