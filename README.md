# OfflinePreOpsTool

Windows 桌面工具，用于在联网电脑上整理服务器与中间件方案，准备镜像和 Docker 安装材料，为每台 Linux 服务器生成离线部署包。目标机通过包内脚本完成部署、状态检查、冷备份、恢复及增量升级。

## 下载与运行

从 [GitHub Releases](https://github.com/xzqwizard/offine-ops-tool/releases) 下载已发布版本；若列表为空，表示尚未发布可下载版本。

| 文件 | 使用方式 |
|---|---|
| `*-setup.exe` | Windows 10/11 x64 安装包，内置 WebView2 离线安装程序 |
| `*-portable.zip` | 完整解压到可写目录，运行 `OfflinePreOpsTool.exe`；需已安装 WebView2 Runtime |
| `SHA256SUMS.txt` | 用 `Get-FileHash <文件路径> -Algorithm SHA256` 核对下载文件 |

程序内置中间件目录和部署模板；crane、Docker/Compose 安装材料、业务镜像需另行准备。当前 Windows 包未配置代码签名，应用内自动更新尚未实现。

## 基本流程

1. 新建方案，填写服务器的 IPv4、OS/版本、架构、资源、Docker 版本及目录。
2. 从内置目录或自定义配置添加中间件，配置持久化目录、参数、发布端口和访问来源。
3. 在镜像库安装 crane 并拉取镜像，或导入可信的 `docker save` 归档；为每种目标环境准备 Docker 与 Compose 材料。
4. 通过校验后构建全量包，将对应服务器的包和校验文件传入目标环境。
5. 在目标机按包内 `README.md` 核验 SHA256 并部署；日常操作见包内 `docs/OPS-GUIDE.md`。

增量升级需选择已成功构建、与现场部署一致的基线。旧部署的数据目录和材料兼容处理见 [迁移与材料准备指南](docs/迁移与材料准备指南-2026-10-09.md)。

## 能力与边界

| 环节 | 已实现能力 | 使用边界 |
|---|---|---|
| 方案与目录 | 导入、复制、模板冻结、自动保存、后端校验 | 导出/恢复后需重新填写密码 |
| 镜像 | 按来源、平台、digest 隔离缓存，校验标签、config、层和归档 | 在线拉取使用 crane；本地 tar 不能证明远端 digest |
| 安装材料 | amd64/arm64 官方静态 Docker 与 Compose 下载；声明式 RPM/DEB 导入 | 其它架构需手工准备；发行版依赖需在目标环境验收 |
| 构建与运维 | 完整性门禁、稳定实例身份、SHA256、冷备恢复、升级失败自动回退 | 无探针仅检查稳定运行；备份不含容器可写层及匿名卷 |
| 网络规则 | Docker IPv4 iptables 下按来源限制发布端口 | 需显式执行 `apply-firewall.sh --apply`；原生 Docker nftables 不支持 |
| 签名 | 尚未实现，启用签名会拒绝构建 | SHA256 校验不能替代签名和来源认证 |

配置支持 amd64、arm64、loongarch64、mips64el、sw64；实际可用性取决于目标镜像、安装材料、OS 与内核。Linux CI 覆盖 TCP 来源限制及 PostgreSQL 备份、恢复、升级、回滚、镜像身份场景，其它中间件和平台需专项验证。

## 数据与隐私

默认配置和数据位于程序旁的 `data/`，五类存储根可分别调整。本机仓库/代理密码及被识别为秘密的中间件参数使用 Windows DPAPI，与 Windows 用户绑定；换机器不能依赖直接复制密文恢复凭据。

方案导出、ZIP 数据备份和构建快照会排除上述密码，但仍可能包含项目/客户名称、服务器地址、仓库地址、用户名和本地路径。**部署包的 Compose 含运行所需密码；部署包及业务备份应按敏感材料管理。** 提交 Issue、日志或示例前请按 [数据与隐私说明](docs/数据与隐私.md) 手动检查。

## 文档

- [使用说明](docs/项目说明.md)：首次使用、构建、现场操作及数据管理。
- [功能清单](docs/功能清单.md)：各模块的具体行为和限制。
- [迁移与材料准备](docs/迁移与材料准备指南-2026-10-09.md)：旧部署迁移、材料声明与故障恢复。
- [项目结构](docs/项目结构.md) / [架构设计](docs/方案设计.md)：源码职责、数据流与设计约定。
- [版本发布](docs/版本发布.md)：维护者的标签发版与试构建流程。
- 历史审查：[2026-10-02](docs/代码审查与业务闭环核查-2026-10-02.md)、[2026-10-09](docs/代码审查与业务闭环核查-2026-10-09.md)。报告前部是当时快照，后续修复见各报告记录。

## 开发与验证

需要 Node.js 22、pnpm 10、Rust 1.97.1 和 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。Windows 发布使用 MSVC；开发环境及 GNU 工具链说明见 [项目结构](docs/项目结构.md)。

```bash
pnpm install --frozen-lockfile
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
pnpm tauri dev
```

`pnpm dev` 可在浏览器预览前端，桌面命令需 Tauri 环境。[Verify 工作流](https://github.com/xzqwizard/offine-ops-tool/actions/workflows/verify.yml) 执行代码检查、依赖审计及独立 Linux Docker 场景；Windows 包由 [Release 工作流](https://github.com/xzqwizard/offine-ops-tool/actions/workflows/release.yml) 构建。验证结果以对应提交的运行记录为准。

## 版本发布与许可

维护者用 `pnpm release:version <版本>` 同步应用版本，提交代码并确认 Verify 成功后，推送对应 `vX.Y.Z` 标签。工作流自动构建 Windows x64 安装包、便携 ZIP 和 SHA256 文件并发布 Release；带预发布后缀的标签标记为 Pre-release。详细步骤见 [版本发布指南](docs/版本发布.md)。

本项目采用 [Apache License 2.0](LICENSE)。
