适用于 Windows 10/11 x64。

- `*-setup.exe`：推荐使用的安装包，内置 Microsoft WebView2 离线安装程序，安装时无需联网。
- `*-portable.zip`：便携程序，完整解压到可写目录后运行 `OfflinePreOpsTool.exe`。电脑需已安装 WebView2 Runtime；缺少时先运行本版本安装包。默认配置和数据位于程序旁的 `data/` 目录；在设置中覆盖存储根后，相关数据使用指定目录。
- `SHA256SUMS.txt`：安装包和 ZIP 的 SHA256 校验值。Windows 可用 `Get-FileHash <文件路径> -Algorithm SHA256` 核对。

程序内置中间件目录和部署脚本模板；crane、Docker 安装材料和业务镜像需在应用中另行准备。安装包内置的 WebView2 用于桌面程序界面，与应用生成的 Linux 离线部署包无关。

当前安装包未做 Windows 代码签名。版本升级前建议通过设置中的备份功能保存应用数据。

应用数据备份不含镜像、安装材料、构建产物或现场业务数据；备份/导出排除被识别的密码，但仍可能含项目名称、地址、用户名和路径。换机器或 Windows 用户后需重新填写密码。部署包包含实际运行参数与密码，请保护其访问权限。

使用指南见 [项目 README](https://github.com/xzqwizard/offine-ops-tool#readme)，分享日志或示例前请阅读 [数据与隐私说明](https://github.com/xzqwizard/offine-ops-tool/blob/main/docs/数据与隐私.md)。
