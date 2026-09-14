# Clard

Clard 是 Linux 上的完整代理管理工具：系统级常驻服务 `clard-helper`（root，拥有 mihomo 核心、TUN 与全部数据）+ TUI 客户端 `clard`（ratatui）。数据面 **TUN-only**，系统级服务、不分用户。

设计文档（唯一实现依据）：`doc/01-方案设计.md`（架构/边界/安全）、`doc/03-ui设计.md`（UI/UX）、`doc/04-mihomo调研.md`（运行时接口）、`doc/05-需求文档.md`（需求清单）。

## 需求 TODO（依据 doc/05-需求文档.md）

### 已完成（基础设施，非用户操作层）

- [x] helper daemon：flock 单实例、0666 unix socket、`SO_PEERCRED` 记录 actor、审计双写
- [x] profiles 存储迁移 helper（`/var/clard/lib`，同 URL 覆盖更新）
- [x] config_gen：base64 归一化、7 种节点协议转换（vless/vmess/ss/trojan/http/socks/hysteria2）、深合并、托管字段注入
- [x] 订阅下载（HttpFetcher：30s 超时、8MiB 上限）
- [x] 订阅管理 CLI（`clard profiles import/list/update/remove/current/set-current`，经 IPC 调 helper）

### 页面与导航

- [x] 七页导航（主页/配置/代理/连接/日志/设置/规则，`1`–`7` 直达，状态保持；规则页为 `7`）
- [x] 全局键位与帮助浮层（`?`）
- [x] 语言（默认英语，轻量 i18n 字典，设置页切换即全局生效；rust-i18n 宏方案留作后续替换）

### 订阅配置

- [x] URL 导入与覆盖更新（CLI ✓，TUI ✓）
- [x] 切换当前配置（事务：config_gen → ApplyConfig 热重载 → 启动核心；记忆节点恢复随核心生命周期后续）
- [x] 手动更新订阅（CLI ✓，TUI ✓）
- [x] 删除配置（CLI ✓，TUI ✓）
- [x] 改名 / 排序（上移下移）
- [x] 订阅信息展示（流量/到期，`subscription-userinfo`）
- [x] 自动更新（helper 全局定时，默认 6 小时，可编辑；设置存储已就绪，TUI 编辑入口随设置页）
- [x] 版本回滚（保留 3 份）

### 代理

- [x] 分组树与节点选择 / 清除固定选择（`PUT/DELETE /proxies/:name`）
- [x] 测延迟（单个 / 全组，`/proxies/:name/delay`、`/group/:name/delay`）
- [x] 测速 URL 配置（设置页 General「Test URL」，空=默认）
- [x] 节点过滤排序（`f` 过滤、`s` 按名称/延迟排序）

### 连接

- [x] 连接列表与实时流量（`/connections`、`/traffic` WS）
- [x] 关闭单个 / 全部连接
- [x] 排序 / 搜索 / 内存显示
- [x] 单位切换（自动单位 ⇄ KB）

### 规则

- [x] 规则列表查看（`GET /rules`）
- [x] 规则启用/禁用（`PATCH /rules/disable`）
- [x] 搜索过滤 / 规则集视图（`/providers/rules`）

### 日志与审计

- [x] TUI 日志页三栏（应用/核心/审计）与过滤
- [x] 审计完整规格（intent+result 双记录、net 前后快照、cfg_sha256；`o` 按 op 过滤、`I` 配对、`Enter` 展开、`x` 导出）
- [x] 核心日志接入（stdout 管道写 core.log；`e` 级别过滤）
- [x] 日志轮转（audit/core 10MB×5、tui 1MB×5）

### 设置

- [x] 通用（混合端口 7890 仅回环、自动更新间隔、语言、主题）
- [x] TUN 与旁路（热重载 + 读回校验）：开关（helper 托管注入 + `PATCH /configs` 热更 + 回读校验 + 失败回退）、stack、dns-hijack、route-exclude、exclude-uid/interface/dst-port、strict-route/auto-redirect（二次确认 + 风险提示）、紧急恢复直连（cleanup-tun，幂等）；能力探测/冲突检测；启动自检残留清理；配置生成注入设置
- [x] 核心（版本/checksum 展示、启停/重启、`c` 检查更新、`i` 升级——自动获取 GitHub 最新 release，helper 复核 + 原子替换 + 重启）；RPM 随包携带最新 mihomo 安装即用，升级为可选
- [x] 后台服务（RPM 安装/卸载命令展示；安装即 systemd enable --now，packaging/clard.spec）
- [x] 关于（版本 / 路径一览）
- [ ] 日志与审计配置（R7.5：核心日志级别、应用/审计日志大小份数、双写；设置页 Logs 页签）

### 备份与恢复

- [x] 本地备份 / 恢复 / 备份管理（设置页 Backup 页签：b 创建 / Enter 恢复 / d 删除）

## 未实现 TODO（按优先级）

### P1 短期独立（让现有功能真正可用）

- [x] 核心日志接入（R6.1）：核心 stdout/stderr 管道 → core.log；日志页核心栏 `e` 级别过滤
- [x] 日志轮转（doc/01 §10）：audit.log 10MB×5、core.log 10MB×5、tui.log 1MB×5
- [x] 审计完整规格（R6.3）：intent+result 双记录、net 前后快照、cfg_sha256；日志页审计栏 `o` 按 op 过滤、`Enter` 展开详情、`x` 导出、`I` 配对切换

### P2 崩溃安全（doc/01 §5.4/§6.5，核心承诺）

- [x] watchdog 退避重启：核心崩溃 `max_restarts=10`/`window=600s`/`backoff≤30s`，超限 cleanup-tun + fail-open + 审计 `watchdog.failopen` + `Degraded` 事件
- [x] TUN 健康 watchdog：每 3s 检查（`ip link clard0 UP` + table 2023 路由 + rule 9100），连续 3 次不满足 → cleanup-tun + 审计 `watchdog.failopen` + `Degraded` 事件
- [x] Subscribe 事件流（§5.6）：helper 广播 `CoreStatusChanged`/`TunChanged`/`Degraded`；TUI 订阅长连接 + 断线重连全量同步；`Degraded` → 消息条红色提示

### P3 可选加固 / 体验

- [ ] 偏执模式（R7.4，可选加固默认关）：开关 TUN 前 `pkexec`/polkit `auth_admin_keep` 授权
- [ ] 记忆节点恢复：切换配置后记住当前选中节点（随核心生命周期）
- [ ] 日志与审计配置（R7.5）：设置页 Logs 页签（核心日志级别、应用/审计日志大小份数、双写）
