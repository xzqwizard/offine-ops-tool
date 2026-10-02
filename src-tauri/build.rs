fn main() {
    // 主程序不使用 Tauri 内嵌 manifest（new_without_app_manifest），
    // 改为下方全局注入统一清单：否则仅 bin 有 manifest，而 lib 单元测试/
    // 集成测试/示例产物缺失清单时，加载器绑定 comctl32 5.82，缺少
    // TaskDialogIndirect 等入口 → 进程以 STATUS_ENTRYPOINT_NOT_FOUND 静默失败。
    let target = std::env::var("TARGET").unwrap_or_default();
    let gnu = target.contains("windows-gnu");
    let attrs = if gnu {
        tauri_build::Attributes::default()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest())
    } else {
        tauri_build::Attributes::default()
    };
    tauri_build::try_build(attrs).expect("tauri_build failed");
    if target.contains("windows-msvc") {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }
    if !gnu {
        return;
    }

    // windows-gnu + rust-lld 链接方案：lld 不允许重复资源，而 gcc specs 默认
    // 注入 default-manifest.o 与下方的统一清单冲突 → 通过自定义 specs 去除。
    // 路径由 build script 按当前包位置动态生成（可移植，勿硬编码）。
    // 详见 .cargo/config.toml 说明与本目录 mingw-specs-nodm.txt。
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let specs = std::path::Path::new(&manifest_dir)
        .join(".cargo/mingw-specs-nodm.txt")
        .to_string_lossy()
        .replace('\\', "/");
    println!("cargo:rustc-link-arg=-specs={specs}");

    // 统一清单（common-controls v6 + DPI 感知 + UTF-8 代码页），windres 编译
    // 为 COFF 后全局注入（bin/lib-test/test/example 全部生效，各一份无重复）。
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let out = std::path::Path::new(&out_dir);
        let manifest = out.join("app-manifest.xml");
        let rc = out.join("app-manifest.rc");
        let obj = out.join("app-manifest.o");
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
            let _ = std::fs::write(&rc, "1 24 \"app-manifest.xml\"\n");
            let wr = std::process::Command::new("x86_64-w64-mingw32-windres")
                .arg("-i")
                .arg(&rc)
                .arg("-O")
                .arg("coff")
                .arg("-o")
                .arg(&obj)
                .current_dir(out)
                .status();
            if matches!(wr, Ok(s) if s.success()) {
                let p = obj.to_string_lossy().replace('\\', "/");
                println!("cargo:rustc-link-arg={p}");
            } else {
                panic!("windres 编译统一清单失败（本方案依赖该清单，请确认 MinGW windres 可用）");
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
