use std::{net::SocketAddr, path::PathBuf};

use http::Method;
use reqwest::{Client, RequestBuilder, Url};
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::sync::mpsc;

use super::{
    error::{IpcError, Result},
    models::{
        BackendVersion, BaseConfig, Connections, CoreUpdaterChannel, Groups, Proxy, ResponseError, RuleProviders, Rules,
    },
    websocket::{WebSocketMessage, WsControl, connect_stream},
};

/// websocket id 通过id索引
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebSocketId(u32);

/// 后端类型
/// 支持 uds TCP
#[derive(Debug, Clone)]
pub enum Protocol {
    /// unix domain socket
    UDS(PathBuf),
    /// tcp/ip socket
    TCP(SocketAddr),
}

/// `BackendBuilder` 用于配置并构建 [`Backend`] 实例。
#[derive(Default, Debug)]
pub struct BackendBuilder {
    unix_path: Option<String>,
    tcp_addr: Option<String>,
}

impl BackendBuilder {
    /// 设置 Unix Domain Socket 路径（如 `/tmp/mihomo.sock`）。
    pub fn set_unix_socket(mut self, socket_path: &str) -> Self {
        self.unix_path = Some(socket_path.to_string());
        self
    }

    /// 设置 TCP 连接地址（如 `127.0.0.1:8080`）。
    pub fn set_tcp_addr(mut self, addr: &str) -> Self {
        self.tcp_addr = Some(addr.to_string());
        self
    }

    /// 构建 Backend 实例。若 UDS 和 TCP 同时存在，优先使用 UDS。
    pub fn build(self) -> Result<Backend> {
        let protocol = match (self.unix_path, self.tcp_addr) {
            (Some(path), _) => Protocol::UDS(PathBuf::from(path)),
            (None, Some(addr_str)) => {
                let socket_addr = addr_str
                    .parse::<SocketAddr>()
                    .map_err(|e| IpcError::FailedBackend(format!("无效的 TCP 地址: {}", e)))?;
                Protocol::TCP(socket_addr)
            }
            (None, None) => {
                return Err(IpcError::FailedBackend("未设置任何后端类型".to_owned()));
            }
        };

        Ok(Backend {
            protocol: protocol.clone(),
            client: match protocol {
                Protocol::UDS(unix_path) => Client::builder().unix_socket(unix_path).build()?,
                Protocol::TCP(_) => Client::new(),
            },
        })
    }
}

/// mihomo 后端管理
#[derive(Debug)]
pub struct Backend {
    protocol: Protocol,
    client: Client,
}

impl Backend {
    /// 建造者 backend 的唯一初始化方法
    pub fn builder() -> BackendBuilder {
        BackendBuilder::default()
    }

    /// 切换后端类型
    pub fn switch_backend() -> Result<()> {
        Ok(())
    }

    /// 获取 http url
    fn get_http_url(&self, suffix: &str) -> String {
        let suffix = suffix.trim_start_matches('/');
        match self.protocol {
            Protocol::TCP(addr) => format!("http://{}:{}/{}", addr.ip(), addr.port(), suffix),
            Protocol::UDS(_) => format!("http://localhost/{suffix}"),
        }
    }

    /// http 请求构建器
    fn build_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let url = self.get_http_url(path);
        Ok(match method {
            Method::GET => self.client.get(url),
            Method::PUT => self.client.put(url),
            Method::DELETE => self.client.delete(url),
            Method::PATCH => self.client.patch(url),
            Method::POST => self.client.post(url),
            _ => return Err(IpcError::MethodNotSupported(method.to_string())),
        })
    }

    /// 订阅实时数据
    /// 返回数据接收器 控制发送器
    pub async fn subscribe<T>(&self, url: Url) -> Result<(mpsc::UnboundedReceiver<T>, mpsc::UnboundedSender<WsControl>)>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let (mut msg_rx, ctrl_tx) = connect_stream::<T>(self.protocol.clone(), url).await?;

        let (ui_tx, ui_rx) = mpsc::unbounded_channel::<T>();
        tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                if let WebSocketMessage::Text(v) = msg {
                    if ui_tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });

        Ok((ui_rx, ctrl_tx))
    }

    /// clash 业务实现

    /// 获取后端版本
    pub async fn get_version(&self) -> Result<BackendVersion> {
        let req = self.build_request(Method::GET, "version")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("get version failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<BackendVersion>().await?)
    }

    /// 清理 FakeIP 缓存
    pub async fn flush_fakeip(&self) -> Result<()> {
        let req = self.build_request(Method::POST, "/cache/fakeip/flush")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("flush fakeip failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 清理 DNS 缓存
    pub async fn flush_dns(&self) -> Result<()> {
        let req = self.build_request(Method::POST, "/cache/dns/flush")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res
                .json::<ResponseError>()
                .await
                .map_or_else(|msg| format!("flush dns failed: {msg}"), |err| err.message.to_string());
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 获取全部连接信息
    pub async fn get_connections(&self) -> Result<Connections> {
        let req = self.build_request(Method::GET, "/connections")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("get all connections failed, {}", msg),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<Connections>().await?)
    }

    /// 关闭全部连接
    pub async fn close_all_connections(&self) -> Result<()> {
        let req = self.build_request(Method::DELETE, "/connections")?;
        let res = req.send().await?;

        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("Close all connections failed!, {}", msg),
                |err_res| err_res.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 关闭指定 ID 的连接
    pub async fn close_connection(&self, connection_id: &str) -> Result<()> {
        let req = self.build_request(Method::DELETE, &format!("/connections/{connection_id}"))?;
        let res = req.send().await?;

        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("Close connection failed!, {}", msg),
                |err_res| err_res.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 获取所有的代理组
    pub async fn get_groups(&self) -> Result<Groups> {
        let req = self.build_request(Method::GET, "/group")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res
                .json::<ResponseError>()
                .await
                .map_or_else(|msg| format!("Get groups failed: {msg}"), |err| err.message.to_string());
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<Groups>().await?)
    }

    /// 获取指定名称的代理组
    pub async fn get_group_by_name(&self, group_name: &str) -> Result<Proxy> {
        let group_name = urlencoding::encode(group_name);
        let req = self.build_request(Method::GET, &format!("/group/{group_name}"))?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res
                .json::<ResponseError>()
                .await
                .map_or_else(|msg| format!("Get groups failed: {msg}"), |err| err.message.to_string());
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<Proxy>().await?)
    }

    /// 为指定代理组下使用指定的代理节点 【代理组/节点】
    pub async fn select_node_for_group(&self, group_name: &str, node: &str) -> Result<()> {
        let group_encode = urlencoding::encode(group_name);
        let body = json!({"name" : node});
        let req = self
            .build_request(Method::PUT, &format!("/proxies/{group_encode}"))?
            .json(&body);

        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("select node for {group_name} failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }

        Ok(())
    }

    /// 指定代理组下不再使用固定的代理节点
    ///
    /// 一般用于自动选择的代理组（例如：URLTest 类型的代理组）下的节点
    pub async fn unfixed_proxy(&self, group_name: &str) -> Result<()> {
        let group_name_encode = urlencoding::encode(group_name);
        let req = self.build_request(Method::GET, &format!("/proxies/{group_name_encode}"))?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("unfixed group [{group_name}] failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 对指定代理进行延迟测试
    ///
    /// 一般用于代理节点的延迟测试，也可传代理组名称（只会测试代理组下选中的代理节点）
    pub async fn delay_proxy_for_name(&self, _proxy_name: &str, _test_url: &str, _timeout: u32) -> Result<()> {
        todo!();
    }

    /// 获取所有的规则信息
    pub async fn get_rules(&self) -> Result<Rules> {
        let req = self.build_request(Method::GET, &format!("/rules"))?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res
                .json::<ResponseError>()
                .await
                .map_or_else(|msg| format!("Get groups failed: {msg}"), |err| err.message.to_string());
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<Rules>().await?)
    }

    /// 获取所有规则提供者信息
    pub async fn get_rule_providers(&self) -> Result<RuleProviders> {
        todo!();
        // let client = self.build_request(Method::GET, "/providers/rules")?;
        // let response = self.send_by_protocol(client).await?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("get all rule providers failed, {}", e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(response.json::<RuleProviders>().await?)
    }

    /// 更新规则提供者信息
    pub async fn update_rule_provider(&self, provider_name: &str) -> Result<()> {
        todo!();
        // let provider_name_encode = urlencoding::encode(provider_name);
        // let client = self.build_request(Method::PUT, &format!("/providers/rules/{provider_name_encode}"))?;
        // let response = self.send_by_protocol(client).await?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("update rule provider[{}] failed, {}", provider_name, e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(())
    }

    /// 获取基础配置
    pub async fn get_base_config(&self) -> Result<BaseConfig> {
        todo!();
        // let client = self.build_request(Method::GET, "/configs")?;
        // let response = self.send_by_protocol(client).await?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("get base config failed, {}", e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(response.json::<BaseConfig>().await?)
    }

    /// 重新加载配置
    pub async fn reload_config(&self, force: bool, config_path: &str) -> Result<()> {
        todo!();
        // let body = json!({ "path": config_path });
        // let client = self
        //     .build_request(Method::PUT, "/configs")?
        //     .timeout(Duration::from_secs(60))
        //     .query(&[("force", force)])
        //     .json(&body);
        // let response_result = self.send_by_protocol(client).await;
        // if matches!(self.protocol, Protocol::LocalSocket)
        //     && let Ok(pool) = IpcConnectionPool::global()
        // {
        //     pool.clear_pool().await;
        // }
        // let response = response_result?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("reload base config failed, {}", e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(())
    }

    /// 更新基础配置
    pub async fn patch_base_config<D: serde::Serialize + Clone + Sync>(&self, data: &D) -> Result<()> {
        todo!();
        // let client = { self.build_request(Method::PATCH, "/configs")?.json(&data) };
        // let response = { self.send_by_protocol(client).await? };
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("patch base config failed, {}", e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(())
    }

    /// 更新 Geo, 同 [`upgrade_geo`](crate::mihomo::Mihomo::upgrade_geo)
    pub async fn update_geo(&self) -> Result<()> {
        todo!();
        // let client = self
        //     .build_request(Method::POST, "/configs/geo")?
        //     .timeout(Duration::from_secs(60));
        // let response = self.send_by_protocol(client).await?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("update geo database failed, {}", e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(())
    }

    /// 重启核心
    pub async fn restart(&self) -> Result<()> {
        todo!();
        // let client = self.build_request(Method::POST, "/restart")?;
        // let response = self.send_by_protocol(client).await?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("restart core failed, {}", e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(())
    }

    /// 升级核心
    pub async fn upgrade_core(&self, channel: CoreUpdaterChannel, force: bool) -> Result<()> {
        todo!();
        // let client = self
        //     .build_request(Method::POST, "/upgrade")?
        //     .timeout(Duration::from_secs(60))
        //     .query(&[("channel", &channel.to_string()), ("force", &force.to_string())]);
        // let response = self.send_by_protocol(client).await?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("upgrade core failed, {}", e),
        //         |err_res| {
        //             let msg = err_res.message;
        //             if msg.to_lowercase().contains("already using latest version") {
        //                 "already using latest version".to_string()
        //             } else {
        //                 msg.to_string()
        //             }
        //         },
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(())
    }

    /// 更新 UI
    pub async fn upgrade_ui(&self) -> Result<()> {
        todo!();
        // let client = self
        //     .build_request(Method::POST, "/upgrade/ui")?
        //     .timeout(Duration::from_secs(60));
        // let response = self.send_by_protocol(client).await?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("upgrade ui failed, {}", e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(())
    }

    /// 更新 Geo
    pub async fn upgrade_geo(&self) -> Result<()> {
        todo!();
        // let client = self
        //     .build_request(Method::POST, "/upgrade/geo")?
        //     .timeout(Duration::from_secs(60));
        // let response = self.send_by_protocol(client).await?;
        // if !response.status().is_success() {
        //     let err_msg = response.json::<ErrorResponse>().await.map_or_else(
        //         |e| format!("upgrade geo database failed, {}", e),
        //         |err_res| err_res.message.to_string(),
        //     );
        //     ret_failed_resp!("{}", err_msg);
        // }
        // Ok(())
    }
}

pub async fn get_uds<T>(usd_path: &str, url: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let client = Client::builder().unix_socket(usd_path).build()?;
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.json::<T>().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn backend() -> Result<Backend> {
        Ok(Backend::builder()
            .set_unix_socket("/tmp/verge/verge-mihomo.sock")
            .build()?)
    }

    #[test]
    fn build_backend() -> Result<()> {
        let _ = backend();
        Ok(())
    }

    #[tokio::test]
    async fn test_get_version() -> Result<()> {
        let backend = backend()?;
        let result = backend.get_version().await;

        assert!(result.is_ok());
        let version = result.unwrap();
        println!("version: {:?}", version);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_traffic() -> Result<()> {
        // let backend = backend()?;
        // let url = Url::parse(&websocket::get_websocket_url("traffic")).unwrap();
        // let (mut traffic_rx, ctrl_tx) = backend.subscribe::<Traffic>(url).await?;

        // while let Some(traffic) = traffic_rx.recv().await {
        //     println!("traffic: {:?}", traffic);
        // }

        Ok(())
    }

    #[tokio::test]
    async fn flush_fakeip() -> Result<()> {
        let backend = backend()?;
        let result = backend.flush_fakeip().await;
        assert!(result.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn flush_dns() -> Result<()> {
        let backend = backend()?;
        let result = backend.flush_dns().await;

        assert!(result.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn get_connections() -> Result<()> {
        let backend = backend()?;
        let result = backend.get_connections().await;

        assert!(result.is_ok());
        println!("connects: {:?}", result.unwrap());

        Ok(())
    }

    #[tokio::test]
    async fn close_all_connections() -> Result<()> {
        let backend = backend()?;
        let result = backend.close_all_connections().await;

        assert!(result.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn get_groups() -> Result<()> {
        let backend = backend()?;
        let result = backend.get_groups().await;

        assert!(result.is_ok());
        println!("group: {:?}", result.unwrap());

        Ok(())
    }

    #[tokio::test]
    async fn get_group_by_name() -> Result<()> {
        let backend = backend()?;
        let result = backend.get_group_by_name("🔰 选择节点").await;

        assert!(result.is_ok());
        println!("group: {:?}", result.unwrap());

        Ok(())
    }

    #[tokio::test]
    async fn request_select_node_for_group() -> Result<()> {
        let backend = backend()?;
        let group = backend.get_group_by_name("📺 动画疯").await.unwrap();
        let now = group.now.unwrap();
        let new = group
            .all
            .iter()
            .flat_map(|v| v.iter())
            .find(|value| **value != now)
            .unwrap()
            .clone();
        println!("now: {}, new:{}", now, new);
        let result = backend.select_node_for_group("📺 动画疯", &new).await;
        assert!(result.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn get_rules() -> Result<()> {
        let backend = backend()?;
        let result = backend.get_rules().await;

        assert!(result.is_ok());

        println!("reules: {}", serde_json::to_string(&result.unwrap()).unwrap());

        Ok(())
    }
}
