# OfflinePreOpsTool（离线部署运维工具）

面向政务等完全离线（内网）环境的一站式离线部署方案生成工具：

- 在**有外网的办公机**上定义服务器清单（区分 CPU 架构/OS）、挑选中间件与版本、定义服务器间端口访问关系；
- 在线（或本地导入）拉取对应架构的 Docker 镜像，为**每台服务器生成自包含离线包**：
  - Docker 引擎离线安装包（按架构/OS 匹配）
  - 全部镜像 tar（docker save 格式）
  - `deploy.sh` 一键部署脚本（预检→装 Docker→导镜像→起服务→健康检查）
  - `ops.sh` 日常运维脚本（重启/日志/备份/升级/回滚/诊断）
  - 自动生成的运维操作文档与端口矩阵

## 文档

- [方案设计（v1.1）](docs/方案设计.md) —— 架构、数据模型、技术选型、脚本设计、里程碑与风险

## 开发

```bash
pnpm install        # 安装前端依赖
pnpm tauri dev      # 开发模式运行（需要 Rust 工具链）
pnpm build          # 前端类型检查 + 构建
cargo test          # 后端单元测试（在 src-tauri/ 下执行）
```

### 当前进度（M0 竖切已完成）

- [x] Tauri 2 + Vue 3 + Vite + Tailwind 4 + Element Plus + Pinia 骨架
- [x] 设计系统：五套主题（暗色/亮色/科技绿/商务蓝/极光紫）、玻璃标题栏、侧边导航
- [x] 方案管理：新建/列表/删除/编辑（服务器清单、中间件编排、端口矩阵、校验、构建五个页签）
- [x] 服务器管理：架构（含信创 arm64/loongarch64）/OS/资源配置，唯一性与级联删除
- [x] 中间件目录：预置 10 个常用中间件 + 手动输入镜像，端口/参数编辑，同机端口冲突校验
- [x] 端口矩阵：可点击矩阵 + 访问规则明细，防火墙脚本生成
- [x] 构建中心：minijinja 渲染 deploy/ops/precheck/apply-firewall 脚本、compose、运维文档；
      本地镜像 tar 装入包内，tar.gz 打包（镜像 store 模式不二次压缩），SHA256SUMS，进度事件
- [x] 设置：存储根路径全部可自定义（防 C 盘堆积）、镜像源管理
- [ ] M1：skopeo 在线拉取（跨架构）、Registry tag/架构查询、镜像缓存去重
- [ ] M2：Docker 离线安装包库管理、端口开通申请表 xlsx 导出、拓扑图
- [ ] M3：升级/回滚脚本完善、GPG 签名、信创 OS 真机验证

### 产物结构（每台服务器一个自包含包）

```text
<server>_<arch>/
├── manifest.json / SHA256SUMS / README.md
├── docker-offline/   # Docker 引擎离线安装（install-docker.sh + packages/）
├── images/           # 镜像 tar（本地导入或 M1 在线拉取）
├── stack/            # docker-compose.yml + data/
├── scripts/          # deploy.sh / ops.sh / precheck.sh / apply-firewall.sh
├── firewall/ docs/   # 端口说明、OPS-GUIDE.md、PORT-MATRIX.md
└── logs/
```
