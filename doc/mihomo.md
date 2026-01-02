### 协议

#### 请求示例¶
curl 示例
```shell
curl -H 'Authorization: Bearer ${secret}' http://${controller-api}/configs?force=true -d '{"path": "", "payload": ""}' -X PUT
# 此请求附带 'Authorization: Bearer ${secret}' 请求头，其中：

# ${secret} 为配置文件设置的api密钥
# ${controller-api} 为配置文件中设置的api监听地址
# ?force=true 为携带参数，部分请求需携带
# '{"path": "", "payload": ""}' 为要更新的资源的数据
# 大多数情况传入的数据都为'{"path": "", "payload": ""}'，可以附带新的配置文件路径
```
使用unix域套接字
```shell
curl --unix-socket /tmp/verge/verge-mihomo.sock http://localhost/
```

-----

## ⚡️ Clash API 文档

### 🛠️ API 请求示例

```bash
curl -H 'Authorization: Bearer ${secret}' http://${controller-api}/configs?force=true -d '{"path": "", "payload": ""}' -X PUT
```

| 变量 | 描述 |
| :--- | :--- |
| **\`${secret}\`** | 配置文件设置的 API 密钥 |
| **\`${controller-api}\`** | 配置文件中设置的 API 监听地址 |
| **\`?force=true\`** | 携带参数，部分请求需携带，用于强制执行 |
| **\`'{"path": "", "payload": ""}'\`** | 要更新的资源数据，可附带新的配置文件路径 |

> **Note:**
> 如果路径不在 Clash 工作目录，请手动设置 `SAFE_PATHS` 环境变量将其加入安全路径。该环境变量的语法同本操作系统的 `PATH` 环境变量解析规则（即 Windows 下以分号分割，其他系统下以冒号分割）。

### 📄 日志 (Logs)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/logs`** | `GET` | 获取实时日志 |
| **`/logs?level=log_level`** | `GET` | 获取指定等级日志。可选值：`info`、`debug`、`warning`、`error` |

### 📈 流量信息 (Traffic)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/traffic`** | `GET` | 获取实时流量，单位 `kbps` |

### 🧠 内存信息 (Memory)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/memory`** | `GET` | 获取实时内存占用，单位 `kb` |

### ⚙️ 版本信息 (Version)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/version`** | `GET` | 获取 Clash 版本 |

### 🗑️ 缓存 (Cache) 

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/cache/fakeip/flush`** | `POST` | 清除 fakeip 缓存 |

### 🔧 运行配置 (Configs)

| 路径 | 方法 | 描述 | 请求数据/参数示例 |
| :--- | :--- | :--- | :--- |
| **`/configs`** | `GET` | 获取基本配置 | 无 |
| **`/configs?force=true`** | `PUT` | 重新加载基本配置，**必须发送数据** | `'{"path": "", "payload": ""}'` |
| **`/configs`** | `PATCH` | 更新基本配置，**必须发送数据** | `'{"mixed-port": 7890}'` |
| **`/configs/geo`** | `POST` | 更新 GEO 数据库，**必须发送数据** | 无特定格式，只需发送数据 |
| **`/restart`** | `POST` | 重启内核，**必须发送数据** | 无特定格式，只需发送数据 |

### ⬆️ 更新 (Upgrade)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/upgrade`** | `POST` | 更新内核，**必须发送数据** |
| **`/upgrade/ui`** | `POST` | 更新面板，须设置 `external-ui` |
| **`/upgrade/geo`** | `POST` | 更新 GEO 数据库，**必须发送数据** |

### 🎯 策略组 (Groups)

| 路径 | 方法 | 描述 | 参数示例 |
| :--- | :--- | :--- | :--- |
| **`/group`** | `GET` | 获取策略组信息 | 无 |
| **`/group/group_name`** | `GET` | 获取具体的策略组信息 | 无 |
| **`/group/group_name`** | `DELETE` | 清除自动策略组 `fixed` 选择 | 无 |
| **`/group/group_name/delay`** | `GET` | 对指定策略组内的节点/策略组进行测试，返回新的延迟信息，并清除自动策略组的 `fixed` 选择 | `?url=xxx&timeout=5000` |

### 🌐 代理 (Proxies)

| 路径 | 方法 | 描述 | 请求数据/参数示例 |
| :--- | :--- | :--- | :--- |
| **`/proxies`** | `GET` | 获取代理信息 | 无 |
| **`/proxies/proxies_name`** | `GET` | 获取具体的代理信息 | 无 |
| **`/proxies/proxies_name`** | `PUT` | 选择特定的代理，**需携带数据** | `'{"name":"日本"}'` |
| **`/proxies/proxies_name/delay`** | `GET` | 对指定代理进行测试，并返回新的延迟信息 | `?url=xxx&timeout=5000` |

### 📦 代理集合 (Proxy Providers)

| 路径 | 方法 | 描述 | 参数示例 |
| :--- | :--- | :--- | :--- |
| **`/providers/proxies`** | `GET` | 获取所有代理集合的所有信息 | 无 |
| **`/providers/proxies/providers_name`** | `GET` | 获取特定代理集合的信息 | 无 |
| **`/providers/proxies/providers_name`** | `PUT` | 更新代理集合 | 无特定格式，只需发送数据 |
| **`/providers/proxies/providers_name/healthcheck`** | `GET` | 触发特定代理集合的健康检查 | 无 |
| **`/providers/proxies/providers_name/proxies_name/healthcheck`** | `GET` | 对代理集合内的指定代理进行测试，并返回新的延迟信息 | `?url=xxx&timeout=5000` |

### 📜 规则 (Rules)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/rules`** | `GET` | 获取规则信息 |

### 📦 规则集合 (Rule Providers)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/providers/rules`** | `GET` | 获取所有规则集合的所有信息 |
| **`/providers/rules/providers_name`** | `PUT` | 更新规则集合 |

### 🔗 连接 (Connections)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/connections`** | `GET` | 获取连接信息 |
| **`/connections`** | `DELETE` | 关闭所有连接 |
| **`/connections/:id`** | `DELETE` | 关闭特定连接 |

### 🔍 域名查询 (DNS Query)

| 路径 | 方法 | 描述 | 参数示例 |
| :--- | :--- | :--- | :--- |
| **`/dns/query`** | `GET` | 获取指定名称和类型的 DNS 查询数据 | `?name=example.com&type=A` |

### 🐞 DEBUG (需内核启动时日志级别为 `debug`)

| 路径 | 方法 | 描述 |
| :--- | :--- | :--- |
| **`/debug/gc`** | `PUT` | 进行主动 GC (Garbage Collection) |
| **`/debug/pprof`** | `GET` | 浏览器打开 `http://${controller-api}/debug/pprof` 可查看原始 DEBUG 信息，包括 `allocs` 和 `heap` 报告。 |

-----


命令设计
基本命令
```bash
clard start   # 开启代理
clard mode # 输出当前模式 显示当前模式的代理
clard proxy list # 输出当前模式下的代理，并进行标号  使用标号进行代理选择 
clard proxy set # 使用标号进行代理设置

clard group list # 输出代理组，并进行标号  使用标号进行代理选择 
clard group list # 设置代理组代理



```

全局设置
```bash
-l,--log=<info,debug...> # 日志等级


```


#### tui

##### 查看连接

clard dashboard    查看 所有活动的连接  以及总流量情况



