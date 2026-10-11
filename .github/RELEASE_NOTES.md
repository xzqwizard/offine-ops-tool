适用于 Windows 10/11 x64。

- `*-setup.exe`：推荐使用的安装包，内置 Microsoft WebView2 离线安装程序，安装时无需联网。
- `*-portable.zip`：便携程序，完整解压后运行 `OfflinePreOpsTool.exe`。电脑需已安装 WebView2 Runtime；缺少时先运行本版本安装包。便携程序仍使用应用设置中的数据目录，数据不会自动存放在程序旁边。
- `SHA256SUMS.txt`：安装包和 ZIP 的 SHA256 校验值。Windows 可用 `Get-FileHash <文件路径> -Algorithm SHA256` 核对。

程序内置中间件目录和部署脚本模板；crane、Docker 安装材料和业务镜像需在应用中另行准备。安装包内置的 WebView2 用于桌面程序界面，与应用生成的 Linux 离线部署包无关。

当前安装包未做 Windows 代码签名。版本升级前建议通过设置中的备份功能保存应用数据。
