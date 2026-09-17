# mihomo 运行时接口调研（External Controller REST/WS）

> 调研基线：mihomo **Meta 分支** commit `ac017cdd`（2026-08-16），出处均为上游 `hub/route/*.go`。
> 范围：**仅运行时接口**——`external-controller`（TCP）/ `external-controller-unix`（UDS）暴露的
> REST + WebSocket 端点；**不含**配置 yaml schema（配置项调研另章补充）。
> 用途：clard TUI 直连 `core.sock`（REST/WS）；helper 热重载与回读校验（doc/01 §5.5/§5.6）。

## 0. 访问、鉴权与通用约定

- **监听**：`external-controller`（TCP）、`external-controller-unix`（Unix socket）。clard 用后者
  （`/run/clard/core.sock`，0666 系统级），不经 TCP、不开网络监听。
- **鉴权**：配置了 `secret` 时，
  - REST：请求头 `Authorization: Bearer <secret>`；
  - WebSocket：`?token=<secret>`（浏览器 WS 无法自定义头），或同样用 Bearer 头；
  - 无 `secret` 时不鉴权。校验用常量时间比较（`safeEqual`，server.go）。
  - 401 响应：`{"message":"Unauthorized"}`。
- **错误格式**：统一 `HTTPError`，`{"message": "..."}`（errors.go）：
  `400 Body invalid` / `401 Unauthorized` / `404 Resource not found` / `408 Timeout` / `503`。
- **路径参数**：URL 编码（`PathUnescape`，common.go `getEscapeParam`），节点/组名含特殊字符时按
  `%XX` 编码后放在路径里。
- **CORS**：默认放开（`cors.Apply`）。
- **embed 模式**（Android cmfa 构建）：禁用 `PUT/PATCH /configs`、`PATCH /rules/disable`、
  `/restart`、`/upgrade`（`server.go`/`patch_android.go`）。
- 附注：`external-controller` 还有一个插件扩展点 `addExternalRouters`（route.Register，第三方可挂
  自定义路由），clard 不依赖。

## 1. 路由总览（挂载点全表，server.go:115-156）

| 路径 | 方法 | 说明 | 出处 |
|---|---|---|---|
| `/` | GET | `{"hello":"mihomo"}` 存活探测 | server.go:123 |
| `/version` | GET | 版本 | server.go:127 |
| `/logs` | GET (WS) | 日志流 | server.go:124 |
| `/traffic` | GET (WS) | 实时流量 | server.go:125 |
| `/memory` | GET (WS) | 内存占用 | server.go:126 |
| `/configs` | GET / PUT / PATCH / POST `/geo` | 配置读写与热更 | configs.go |
| `/proxies` | GET / GET·PUT·DELETE·GET`/delay` `/{name}` | 代理与组 | proxies.go |
| `/group` | GET / GET·GET`/delay` `/{name}` | 组视图（Meta 扩展） | groups.go |
| `/rules` | GET / PATCH `/disable` | 规则查询/启停 | rules.go |
| `/connections` | GET(WS) / DELETE / DELETE `/{id}` | 连接管理 | connections.go |
| `/providers/proxies` | GET 及 `/{name}` 系列 | 代理提供商 | provider.go |
| `/providers/rules` | GET / PUT `/{name}` | 规则集 | provider.go |
| `/cache` | POST `/fakeip/flush`、`/dns/flush` | 缓存清理 | cache.go |
| `/dns` | GET `/query` | DNS 查询（Meta 扩展） | dns.go |
| `/storage` | GET / PUT / DELETE `/{key}` | 通用 KV 存储 | storage.go |
| `/restart` | POST | 重启自身 | restart.go |
| `/upgrade` | POST `/`、`/ui`、`/geo` | 核心/UI/geo 升级 | upgrade.go |
| `/debug` | PUT `/gc`、`/debug/*` | 仅 debug 构建：GC + pprof | server.go:109-114 |
| `/ui` | GET | 静态 Web UI（配了 ui 目录时） | server.go:149-150 |
| DoH 路径 | GET / POST | 配置 `doh-server` 为路径时挂载 | doh.go |

> 除 `/traffic` `/memory` `/logs` `/connections` 外，其余均为普通 REST；上述四个支持 WebSocket
> Upgrade（握手校验见 common.go），WS 鉴权走 `?token=`。

## 2. 基础端点

### GET `/` → `{"hello":"mihomo"}`
存活/就绪探测。clard helper 的就绪探测（doc/01 §5.2）应改用 `GET /version` 更可靠。

### GET `/version` → `{"meta":"<meta版本>","version":"<版本>"}`
clard「核心版本 · 待更新」展示与 helper 就绪探测的取数点。

## 3. 配置（configs.go）

### GET `/configs`
返回 `executor.GetGeneral()`：当前生效的 general 配置全量（端口、`mixed-port`、`mode`、
`log-level`、`ipv6`、`allow-lan`、`tun`、`find-process-mode` 等）。
→ clard 用它做「回读校验」（doc/01 §5.5 第 5 步：投递后回读比对 `cfg_sha256`）。

### PATCH `/configs`（字段级部分热更新，响应 204）
`patchConfigs`（configs.go:320）把请求体解码进 `configSchema`（全指针字段），**只处理出现的字段**；
`tun` 经 `pointerOrDefaultTun` 合并：`Enable` 强制覆盖、其余字段保留 `LastTunConf`——**支持 tun 字段级热更**
（实测 `{"tun":{"enable":false}}` → 204，规则即刻清理、设备约 2s 消失）。
请求体为 `configSchema` 子集，**缺省字段不动，出现即生效**：

| 字段 | 说明 |
|---|---|
| `port` / `socks-port` / `redir-port` / `tproxy-port` / `mixed-port` | 重建对应监听（0 关闭） |
| `tun` | **整个 tunSchema 均可热更**：`enable/device/stack/dns-hijack/auto-route/auto-detect-interface/mtu/gso/iproute2-table-index/iproute2-rule-index/auto-redirect(+input/output-mark/fallback-rule-index)/strict-route/route-address(-set)/route-exclude-address(-set)/include·exclude-interface/include·exclude-uid(-range)/endpoint-independent-nat/udp-timeout/icmp-timeout/loopback-address` 等（configs.go `tunSchema`） |
| `allow-lan` / `skip-auth-prefixes` / `lan-allowed-ips` / `lan-disallowed-ips` / `bind-address` | 局域网与来源限制 |
| `mode` | **`rule` / `global` / `direct` 运行时切换**（`tunnel.SetMode`） |
| `log-level` / `ipv6` / `sniffing` / `tcp-concurrent` / `find-process-mode` / `interface-name` | 杂项开关 |
| `ss-config` / `vmess-config` / `tuic-server` | 入站重建（Meta 扩展） |

→ **对 clard 的意义**：doc/01 §6.3「托管字段」几乎全部落在 `tun` 子对象里，且 mihomo 原生支持
`PATCH /configs` 热更 TUN（`patchConfigs` → `ReCreateTun`）。但 clard 的 `SetTun` 除 `tun` 外还需
一并热更 `dns` 块（TUN 开启注入 nameserver），而 `configSchema` **无 dns 字段**（PATCH 无法热更 dns，
encoding/json 忽略未知字段）——故 SetTun 走 **PUT `/configs` 内联完整 yaml**（tun+dns 同时生效）。
注意：PATCH 关闭 TUN 正常（204 + 设备消失），曾出现的「回读校验未通过」是核心 New TUN 失败
（device busy）遗留的孤儿设备导致，非 PATCH 不支持（见 doc/01 §5.3 启动自检清理）。

### PUT `/configs`（全量重载，响应 204）
请求体 `{"path": "绝对路径"}` 或 `{"payload": "<完整 yaml 文本>"}`（payload 优先）；查询参数
`?force=true` 强制应用。路径必须为绝对路径且经 `C.Path.IsSafePath` 校验，否则 400。
→ **内联 payload 全量重载（热加载新配置/新规则）走的是这个端点**。

> ⚠️ **与 doc/01 §5.5 的出入（需修正设计文档）**：
> doc/01 §5.5 写「PATCH /configs?force=true（**内联 payload**…）」。实测：内联 payload 全量重载是
> **`PUT /configs`**（body 带 `payload` 字段，可加 `?force=true`）；`PATCH /configs` 是字段级部分更新、
> 不接受 payload。clard 的 ApplyConfig 链路应使用 **`PUT /configs` + `{"payload": yaml}`**。

### POST `/configs/geo`（响应 204）
更新 geo 数据库（geoip/geosite）。与 `POST /upgrade/geo` 等价。失败 500。

## 4. 代理与组（proxies.go / groups.go）

### GET `/proxies` → `{"proxies": {"<name>": <proxy对象>}}`
全量代理+组。proxy 对象含 `name/type/now/all/history/udp/xudp/tfo/…`；`type` 即 Selector、
URLTest、Fallback、Direct、Reject、Shadowsocks… 内置 `DIRECT`/`REJECT` 也在其中。
→ clard 代理页树的数据源（doc/02 §3.3）。

### GET `/proxies/:name` → 单个 proxy/组对象（404 不存在）

### PUT `/proxies/:name`（body `{"name":"节点"}`，响应 204）
**为组选择节点**。仅 `Selector` 类型（`SelectAble`）可用，否则 400 `Must be a Selector`；节点无效
400。选择结果写入 mihomo cachefile（`SetSelected`，重启保留）。
→ clard 节点切换的核心调用（doc/02 §3.3 `Enter`）。

### DELETE `/proxies/:name`（响应 204）
**清除固定选择**（`ForceSet("")`）。仅非 Selector 的 `SelectAble`（URLTest/Fallback 等自动组）可用，
Selector 或不可选择 → 400。
→ clard 代理页 `d`（回退 URLTest 自动）对应此端点。

### GET `/proxies/:name/delay?url=&timeout=&expected=` → `{"delay": <ms>}`
延迟测试。`url` 测速目标（缺省用内置）；`timeout` **必填**（ms）；`expected` 可选状态码区间
（如 `204,300-399`）。超时 408，失败 503。
→ clard 全组测速（`t`/`T` 同一动作）。

### GET `/group` / `/group/:name` / `/group/:name/delay`（Meta 扩展）
- `GET /group` → `{"proxies":[仅组]}`；`GET /group/:name` → 组对象（非组 404）。
- `GET /group/:name/delay` → 组内**所有节点**的延迟 map（自动组测前先 `ForceSet("")` 清固定选择）。
→ clard 的 `T`（全组测延迟）可优先用此端点，比逐个 `GET /proxies/:name/delay` 高效。

## 5. 规则（rules.go）

### GET `/rules` → `{"rules":[{index,type,payload,proxy,size,extra?},…]}`
当前生效规则（按序）。`type`=DOMAIN/GEOIP/GEOSITE/MATCH/…；`payload`=匹配内容；`proxy`=策略
（组名或 DIRECT）；`size`=GEOIP/GEOSITE 规则集条目数（其他 -1）；`extra` 含 `disabled/hitCount/
hitAt/missCount/missAt` 命中统计。

### PATCH `/rules/disable`（body `{"<index>": true|false}`，响应 204，Meta 扩展）
按索引**启用/禁用**规则，无需重载配置。embed 模式禁用。

### 规则增删改：**无 API**，只能改配置重载
规则的增/删/改只能通过修改配置 yaml 的 `rules:`/`rule-providers:` 再 `PUT /configs` 全量重载。
→ **与 clard 架构吻合**：规则变更不由 TUI 直接改 mihomo，而是 TUI `config_gen` 生成
→ helper `ApplyConfig`（`PUT /configs` 内联）→ 热重载（doc/01 §5.5）。

## 6. 提供商（provider.go）

### GET `/providers/proxies` → `{"providers":{...}}`
代理提供商列表（含 `name/type/vehicleType/updatedAt/proxies/…`）。

### GET `/providers/proxies/:name` → 单提供商

### PUT `/providers/proxies/:name`（响应 204 / 503）
**更新提供商**（按订阅 URL 重拉）。失败 503。

### GET `/providers/proxies/:name/healthcheck`（响应 204）
触发该提供商的健康检查（URLTest 组自动更新延迟）。

### GET `/providers/proxies/:name/:proxy` / `GET …/:proxy/healthcheck`
提供商内单个节点的信息 / 延迟测试（参数同 `/proxies/:name/delay`）。

### GET `/providers/rules` → `{"providers":{...}}`
规则集列表（含 `behavior/format/ruleCount/updatedAt/vehicleType` 等）。

### PUT `/providers/rules/:name`（响应 204 / 503）
更新指定规则集。

→ clard 订阅更新的「TUI 下载 → 校验 → 投递 → 重载」链路**不依赖**此处；本组端点用于 TUI
「立即更新选中订阅」时可选的直连加速（仅当 TUI 想绕过自身下载流程时，不建议）。

## 7. 连接（connections.go）

### GET `/connections`
- 普通请求 → 快照 `{"downloadTotal","uploadTotal","connections":[<连接对象>]}`；
  连接对象含 `id/metadata{…}/upload/download/start/chains/rule/rulePayload/duration`。
- **WS Upgrade** → 按 `?interval=`（默认 1000ms）周期推送快照。
→ clard 连接页（doc/02 §3.4）。

### DELETE `/connections`（响应 204）关闭全部；`DELETE /connections/:id`（204）关闭单个
→ clard 连接页 `x` / `X`。

## 8. 缓存与 DNS（cache.go / dns.go）

### POST `/cache/fakeip/flush`（204 / 400）
清 fake-ip 池。

### POST `/cache/dns/flush`（204）
清 DNS 缓存。

### GET `/dns/query?name=&type=`（Meta 扩展）
手动 DNS 查询。`type` 缺省 `A`（A/AAAA/CNAME/…）。DNS 未启用 → 500。响应
`{Status,Question,TC,RD,RA,AD,CD,Answer[],Authority[],Additional[]}`（RR 字段 name/type/TTL/data）。
→ 排障工具，TUI 日志页/调试可用。

### DoH（`doh-server` 配置为路径时挂到 controller）
`GET ?dns=<base64>` 或 `POST Content-Type: application/dns-message`（RFC 8484），返回二进制 DNS
响应。与 clard 无关，记录备查。

## 9. 存储（storage.go，Meta 扩展）

通用 KV：`GET /storage/:key`（返回 JSON，无则 `null`）、`PUT /storage/:key`（body 必须是合法 JSON，
**≤1MB**，超限 413、非法 400）、`DELETE /storage/:key`（204）。落盘在 mihomo 的 cachefile
（root 侧 `/var/clard/lib/runtime/` 内，随 `-d` 目录持久化）。
→ clard **不使用**（节点选择记忆由 mihomo `PUT /proxies` 内部持久化；其余状态归 TUI/helper），记录备查。

## 10. 流量 / 内存 / 日志（server.go，支持 WS 或逐行 JSON 流）

### GET `/traffic`
每秒推 `{"up","down","upTotal","downTotal"}`（字节）。WS 或普通请求（newline-delimited JSON 流）。
→ clard 流量摘要/连接页仪表（doc/03 §3.1）。

### GET `/memory`
每秒推 `{"inuse","osLimit"}`（首帧 `inuse=0`）。→ 可选。

### GET `/logs?level=&format=`
日志流。`level` 缺省 `info`（debug/info/warning/error）；`format=structured` 时推
`{"time","level","message","fields"}`，否则 `{"type","payload"}`（type=debug/info/warning/error）。
→ clard 日志页「核心」栏（doc/02 §3.5）；注意默认是 `warning`→`warn` 的 `type` 字段。

## 11. 升级与重启（upgrade.go / restart.go）

| 端点 | 说明 |
|---|---|
| `POST /upgrade?channel=&force=` | 核心自升级（updater 拉 MetaCubeX release，原子替换+重启），成功回 `{"status":"ok"}` 后 `exec` 重启 |
| `POST /upgrade/ui` | 下载最新 Web UI → `{"status":"ok"}` |
| `POST /upgrade/geo` | 更新 geo 数据库（≡ `POST /configs/geo`） |
| `POST /restart` | 回 `{"status":"ok"}` 后 `syscall.Exec` 重启自身（保留参数） |

→ clard 核心升级走 **helper `InstallCore`**（TUI 下载→校验→helper 复核→原子替换，doc/01 §5.1），
**不使用** mihomo 自带 `/upgrade`（避免 root 侧按 MetaCubeX 发布源自行下载、与 clard 校验链路冲突）。

## 12. 对 clard 的接口选型结论

| 需求（doc/01） | 选用接口 |
|---|---|
| 就绪探测（§5.2） | `GET /version` |
| 模式切换 rule/global/direct | `PATCH /configs {"mode": …}` |
| 托管字段热更 + TUN 开关（§5.6 SetTun） | `PATCH /configs {"tun": …}` + 回读 `GET /configs` |
| 配置投递热重载（§5.5 ApplyConfig） | **`PUT /configs {"payload": yaml}`（?force=true）** —— ⚠️ 修正 doc/01 §5.5 的 PATCH 表述 |
| 节点切换 / 回退自动 | `PUT /proxies/:name` / `DELETE /proxies/:name` |
| 延迟测试 | `GET /proxies/:name/delay`；全组用 `GET /group/:name/delay` |
| 连接管理 | `GET /connections`(WS) / `DELETE /connections[:id]` |
| 实时流量 / 核心日志 | `GET /traffic`(WS) / `GET /logs`(WS) |
| 版本展示 / 升级状态 | `GET /version`；升级走 helper `InstallCore`，不用 `/upgrade` |

**安全边界**：clard 经 unix socket `core.sock`（0666，系统级服务不分用户）直连，**不设 secret、不开 TCP
controller**（doc/01 §6.3 托管字段强制 `external-controller-unix`）。`PUT /configs` 的 `path` 走
`IsSafePath` 校验，clard 用 `payload` 内联即天然避开路径安全问题。
