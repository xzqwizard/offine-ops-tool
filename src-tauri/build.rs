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
}
