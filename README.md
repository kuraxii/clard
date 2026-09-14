# Clard

Clard 是 Linux 上的完整代理管理工具：系统级常驻服务 `clard-helper`（root，拥有 mihomo 核心、TUN 与全部数据）+ TUI 客户端 `clard`（ratatui）。数据面 **TUN-only**，系统级服务、不分用户。

设计文档（唯一实现依据）：`doc/01-方案设计.md`（架构/边界/安全）、`doc/03-ui设计.md`（UI/UX）、`doc/04-mihomo调研.md`（运行时接口）、`doc/05-需求文档.md`（需求清单）。

## 需求 TODO（依据 doc/05-需求文档.md）

### 已完成（基础设施，非用户操作层）

- [x] helper daemon：flock 单实例、0666 unix socket、`SO_PEERCRED` 记录 actor、审计双写
- [x] profiles 存储迁移 helper（`/var/lib/clard`，同 URL 覆盖更新）
- [x] config_gen：base64 归一化、7 种节点协议转换（vless/vmess/ss/trojan/http/socks/hysteria2）、深合并、托管字段注入
- [x] 订阅下载（HttpFetcher：30s 超时、8MiB 上限）
- [x] 订阅管理 CLI（`clard profiles import/list/update/remove/current/set-current`，经 IPC 调 helper）

### 页面与导航

- [x] 七页导航（主页/配置/代理/连接/日志/设置/规则，`1`–`7` 直达，状态保持；规则页为 `7`）
- [x] 全局键位与帮助浮层（`?`）
- [ ] 语言（默认英语，rust-i18n 中文）

### 订阅配置

- [x] URL 导入与覆盖更新（CLI ✓，TUI ✓）
- [ ] 切换当前配置（事务：config_gen → ApplyConfig 热重载 → 恢复记忆节点）
- [x] 手动更新订阅（CLI ✓，TUI ✓）
- [x] 删除配置（CLI ✓，TUI ✓）
- [x] 改名 / 排序（上移下移）
- [ ] 订阅信息展示（流量/到期，`subscription-userinfo`）
- [ ] 自动更新（helper 全局定时，默认 6 小时，可编辑）
- [ ] 版本回滚（保留 3 份）

### 代理

- [x] 分组树与节点选择 / 清除固定选择（`PUT/DELETE /proxies/:name`）
- [x] 测延迟（单个 / 全组，`/proxies/:name/delay`、`/group/:name/delay`）
- [ ] 测速 URL 配置 / 节点过滤排序

### 连接

- [x] 连接列表与实时流量（`/connections`、`/traffic` WS）
- [x] 关闭单个 / 全部连接
- [x] 排序 / 搜索 / 内存显示
- [ ] 单位切换（KB/s ⇄ 总量）

### 规则

- [x] 规则列表查看（`GET /rules`）
- [x] 规则启用/禁用（`PATCH /rules/disable`）
- [x] 搜索过滤 / 规则集视图（`/providers/rules`）

### 日志与审计

- [ ] TUI 日志页三栏（应用/核心/审计）与过滤导出
- [ ] 审计完整规格（intent+result、net 快照、cfg_sha256、按 op 过滤）——helper 基础记录已实现

### 设置

- [ ] 通用（混合端口 7890 仅回环、自动更新间隔、语言、主题）
- [ ] TUN 与旁路（热重载 + 读回校验）
- [ ] 核心（版本 / 检查更新 / 升级）
- [ ] 后台服务（安装 / 卸载 / 偏执模式）
- [ ] 关于（版本 / 路径一览 / 打开目录）

### 备份与恢复

- [ ] 本地备份 / 恢复 / 备份管理（tar.gz 打包 `/var/lib/clard`，`/var/backups/clard/`）
