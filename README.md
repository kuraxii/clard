# Clard

Clard is an application.

## Getting Started

This application is authored using [Abscissa], a Rust application framework.

For more information, see:

[Documentation]

[Abscissa]: https://github.com/iqlusioninc/abscissa
[Documentation]: https://docs.rs/abscissa_core/


## TODO 列表

下面为常见的 GitHub 开发者使用的扁平任务清单格式，方便在 PR/Issue 中直接勾选。已完成项已标记。

### 实时数据（WebSocket）

- [x] 流量数据
- [ ] 内存使用情况
- [ ] 连接信息数据
- [ ] 日志

### 简单请求（HTTP）

- [x] 版本信息
- [x] 清理 fakeip 缓存
- [x] 清理 DNS 缓存
- [x] 获取全部连接信息
- [x] 关闭全部连接
- [x] 关闭指定 ID 的连接

### 代理相关

- [x] 获取所有的代理组
- [x] 获取指定名称的代理组
- [ ] 对指定代理组进行延迟测试（同时清理代理组已固定的节点）
- [ ] 获取代理组提供者的信息
- [ ] 获取指定代理提供者的信息
- [ ] 更新指定代理提供者的信息
- [ ] 对指定代理提供者进行健康检查
- [ ] 对指定代理提供者下的指定节点（非代理组）进行健康检查，并返回新的延迟信息
- [ ] 获取所有代理信息
- [ ] 获取指定代理信息
- [x] 为指定代理选择节点（一般为指定代理组下使用指定的代理节点）
- [x] 指定代理组下不再使用固定的代理节点
- [ ] 对指定代理进行延迟测试（可用于代理节点或代理组）

### 规则 & 提供者

- [x] 获取所有规则信息
- [ ] 获取所有规则提供者信息
- [ ] 更新规则提供者信息

### 配置 & 维护

- [ ] 获取基础配置
- [ ] 重新加载配置
- [ ] 更新基础配置
- [ ] 更新 Geo 地理位置信息
- [ ] 重启核心
- [ ] 升级核心
- [ ] 更新 UI
- [ ] 更新 Geo

### tui 架构 M(app) -- V(canvas) -- C(controller)

app 存储tui 组件之间的状态机状态、获取的数据
canvas 负责绘制，以及组件内部状态管理
controller 负责整理存储的数据 排序
