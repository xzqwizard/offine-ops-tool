# OfflinePreOpsTool

Windows 桌面工具，用于整理服务器/中间件方案，准备 Docker 安装材料和镜像，在有外网的办公机生成每台服务器的离线全量包或增量升级包。目标 Linux 主机使用包内脚本部署、巡检、冷备份和恢复。

## 当前能力

| 环节 | 当前实现 | 验证边界 |
|---|---|---|
| 方案和中间件目录 | 导入、复制、模板冻结、后端校验；本机凭据用 Windows DPAPI 加密 | 便携导出、ZIP 备份、构建快照不含仓库/代理/中间件密码；导入后需重新填写 |
| 镜像 | 按来源/平台/digest 隔离缓存，验证 tar 标签、平台、层和校验和；缺镜像阻断构建 | 在线拉取需要 crane；本地 docker-save tar 不能证明远端仓库 digest |
| 安装材料 | amd64/arm64 官方静态包与 Compose 在线下载；RPM/DEB 需同 OS、版本、架构和组件声明；其它架构手工导入 | 导入静态包校验 ELF；信创发行版、内核和包依赖需在目标机验收 |
| 构建和现场脚本 | 完整包门禁、稳定身份、SHA256、升级前冷备、失败自动回退、按来源限制 Docker 端口 | Windows 模拟流程已覆盖；真实容器和防火墙由 Linux CI / 目标机验收 |
| 签名 | 尚未实现 | 开启签名的方案会被后端拒绝，不能误标为已签名交付 |

`docs/代码审查与业务闭环核查-2026-10-02.md` 记录原始审查及本轮修复状态。方案设计见 `docs/方案设计.md`。

## 开发和验证

Windows 开发机需要 Node 22、pnpm 10、Rust 1.97.1、Tauri 2 的系统依赖。当前仓库的 Windows GNU 构建使用 MinGW windres、GCC specs、rust-lld 和 WebView2；`src-tauri/.cargo/config.toml` 说明了本机链接设置。MSVC 与 Linux 走标准 Tauri 构建路径。Linux CI 另需 WebKitGTK 4.1、GTK、librsvg、appindicator 等系统包。

```bash
pnpm install --frozen-lockfile
pnpm test
pnpm build
cd src-tauri
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

本地桌面运行：`pnpm tauri dev`。前端单独用 `pnpm dev` 可预览界面，但 Tauri 命令不会在普通浏览器中执行。

GitHub Actions 的 `.github/workflows/verify.yml` 在 Ubuntu 上增加了真实 Docker 部署、重复部署、来源允许/拒绝、PostgreSQL 数据冷备/恢复、升级和回滚冒烟测试。此工作流的实际结果以首次 CI 运行记录为准。其它数据库、中间件、国产 OS/架构与生产防火墙后端仍需使用真实镜像/硬件和业务数据逐项验收。

## RPM/DEB 材料声明

导入每个本地 RPM/DEB 时，在同目录提供 `<文件名>.pkg.json`。每个文件独立声明；同一 Docker 版本及 OS 的材料需包括 `engine`、`cli`、`containerd` 三类角色，其他依赖填 `dependency`。`engineVersion` 是精确匹配的 Docker 版本；附件组的 `dockerVersion` 填 `deps`。SHA256 填实际文件的 64 位十六进制值。

```json
{
  "arch": "amd64",
  "kind": "deb",
  "dockerVersion": "27.5.1",
  "engineVersion": "27.5.1",
  "osFamily": "ubuntu",
  "osVersion": "24.04",
  "sha256": { "docker-ce_27.5.1_amd64.deb": "<64位十六进制 SHA256>" },
  "components": { "docker-ce_27.5.1_amd64.deb": "engine" }
}
```

文件名不符合官方惯例时由声明确定类型、版本和角色；不匹配的 OS、组件不齐或校验失败会阻断全量包构建。官方静态包与 Compose 插件仍可通过应用下载；在线源暂仅提供 amd64/arm64。其它架构必须准备并验证对应的 64 位 Linux 静态包、Compose 插件或同发行版原生包。

## 交付约束

全量包每台服务器包含 `manifest.json`、`SHA256SUMS`、`deployment.env`、`docker-offline/`、`images/`、`stack/`、`scripts/` 和 `docs/`。增量包包含新编排/脚本/文档及变化镜像，必须绑定当前已部署构建号。失败构建会出现在历史中，但不能作为升级基线。打包模式默认快速 gzip；目录模式保留各服务器自身的 `SHA256SUMS`，不生成外层 `SHA256SUMS.all`。

Linux 现场按包内 `README.md` 和 `docs/OPS-GUIDE.md` 操作。冷备份在停止该方案服务后制作并校验；恢复会保留恢复前目录，管理员确认后清理。防火墙脚本仅支持 Docker IPv4 iptables `DOCKER-USER`，并需单独执行 `--apply`；无来源规则的发布端口将拒绝外部新连接。
