Clard：Linux 代理管理工具（Rust）。架构 = root 常驻服务 `clard-helper`（持有 mihomo 核心、TUN 与全部数据）+ TUI 客户端 `clard`（ratatui）。

## 文档即规范

**文档是唯一实现依据**：实现/改动前先读对应文档；架构、边界、安全、数据归属、存储、并发、审计、退出等约束一律以文档为准，本文不重复。

- `doc/01-方案设计.md` — 总体架构、组件关系（§3.4）、系统级服务与访问模型（§4）、核心生命周期、TUN、配置管理、审计。改架构前必读。
- `doc/03-ui设计.md` — UI/UX（布局/配色/键位/页面）。改 TUI 前必读。
- `doc/04-mihomo调研.md` — mihomo 运行时接口（REST/WS）。
- `doc/05-需求文档.md` — 用户操作层需求清单（含每项实现方法）。

文档与代码同步：文档过时/缺失时先改文档，再实现。

## 参考实现

功能实现方式不确定时看 clash-verge-rev；mihomo 接口/行为不确定时看 mihomo（Meta 分支）源码与 `doc/04`。路径优先本机，其次上游；改代码前先读对应实现。

- **clash-verge-rev** — 本机 `/root/workspace/repo/clash-verge-rev`（main），dev 分支备选 `~/workspace/project/clash-verge-rev`，上游 https://github.com/clash-verge-rev/clash-verge-rev 。核心进程与退出清理看 `src-tauri/src/core/manager/*.rs`、`core/service.rs`、`feat/window.rs`；TUN 降级看 `feat/tun.rs`；配置生成看 `src-tauri/src/config/clash.rs`。`core/sysopt.rs` 已弃用，仅参考不采用。
- **clash-verge-service-ipc**（特权服务契约 v2.6）— 本机 `/root/workspace/repo/clash-verge-service-ipc`，上游 https://github.com/clash-verge-rev/clash-verge-service-ipc 。启动自检/孤儿清理看 `src/core/reconcile.rs`、`runtime.rs`、`process.rs`；崩溃自愈 watchdog 看 `src/core/manager.rs`；配置投递的 `RuntimeBundle` 看 `src/core/structure.rs`。
- **mihomo（Meta 分支）** — 本机 `/root/workspace/repo/mihomo`（Meta），上游 https://github.com/MetaCubeX/mihomo/tree/Meta 。TUN 落地看 `listener/sing_tun/server.go`（默认设备名 `Meta`、`tun.Options` 组装）；`config/config.go` 解析 tun/listeners；`PUT /configs` 热重载入口在 `hub/route/configs.go`。

## 开发约定

- TUI 改动遵循 `.pi/skills/tui-design/SKILL.md` 与 `doc/03-ui设计.md` 的布局/配色/键位约定。
- 先写功能边界的单元测试再实现；单元测试通过后再组装/集成（跨组件）。每个逻辑单元与测试一并提交，回归必须全绿。
- 按 `README.md` 的「需求 TODO」逐项推进并勾选；新增/调整需求先改 `doc/05-需求文档.md` 与 README TODO，再实现。
- 改本文件遵循 `.pi/skills/agents-md/SKILL.md`。

## 提交

- 分阶段提交：每个可独立运行/回滚的逻辑单元立即 `git commit`。
- Conventional Commits，`<type>: <中文简述>`；type 取 feat/fix/refactor/docs/style/chore/build。
