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

    // 测试/示例二进制补嵌 common-controls v6 清单：
    // 主 bin 由 tauri_build 嵌入完整资源；tests/examples 产物缺清单时加载器
    // 绑定 comctl32 5.82，TaskDialogIndirect 等导入缺失 → 进程以
    // STATUS_ENTRYPOINT_NOT_FOUND 静默失败。
    // 实现：windres 将最小清单编译为 COFF 目标文件，仅注入 test/example 目标
    // （cargo:rustc-link-arg-tests 仅在存在显式/自动发现的 test 目标时合法）。
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let out = std::path::Path::new(&out_dir);
        let manifest = out.join("test-manifest.xml");
        let rc = out.join("test-manifest.rc");
        let obj = out.join("test-manifest.o");
        let xml = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\" manifestVersion=\"1.0\">\n",
            "  <dependency>\n",
            "    <dependentAssembly>\n",
            "      <assemblyIdentity type=\"win32\" name=\"Microsoft.Windows.Common-Controls\" ",
            "version=\"6.0.0.0\" processorArchitecture=\"*\" publicKeyToken=\"6595b64144ccf1df\" language=\"*\"/>\n",
            "    </dependentAssembly>\n",
            "  </dependency>\n",
            "</assembly>\n"
        );
        if std::fs::write(&manifest, xml).is_ok() {
            let _ = std::fs::write(&rc, "1 24 \"test-manifest.xml\"\n");
            let wr = std::process::Command::new("x86_64-w64-mingw32-windres")
                .arg("-i").arg(&rc)
                .arg("-O").arg("coff")
                .arg("-o").arg(&obj)
                .current_dir(out)
                .status();
            if matches!(wr, Ok(s) if s.success()) {
                let p = obj.to_string_lossy().replace('\\', "/");
                println!("cargo:rustc-link-arg-tests={p}");
                println!("cargo:rustc-link-arg-examples={p}");
            }
        }
    }

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
