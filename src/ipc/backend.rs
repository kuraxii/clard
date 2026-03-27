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
#[derive(Debug, Clone)]
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
    pub async fn delay_proxy_for_name(&self, proxy_name: &str, test_url: &str, timeout: u32) -> Result<u16> {
        let proxy_name_encode = urlencoding::encode(proxy_name);
        let req = self
            .build_request(Method::GET, &format!("/proxies/{}/delay", proxy_name_encode))?
            .query(&[("url", test_url), ("timeout", &timeout.to_string())]);

        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("delay test for [{}] failed: {}", proxy_name, msg),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }

        #[derive(serde::Deserialize)]
        struct DelayResp {
            delay: u16,
        }

        let delay_res = res.json::<DelayResp>().await?;
        Ok(delay_res.delay)
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
        let req = self.build_request(Method::GET, "/providers/rules")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("get all rule providers failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<RuleProviders>().await?)
    }

    /// 更新规则提供者信息
    pub async fn update_rule_provider(&self, provider_name: &str) -> Result<()> {
        let provider_name_encode = urlencoding::encode(provider_name);
        let req = self.build_request(Method::PUT, &format!("/providers/rules/{provider_name_encode}"))?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("update rule provider [{provider_name}] failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 获取基础配置
    pub async fn get_base_config(&self) -> Result<BaseConfig> {
        let req = self.build_request(Method::GET, "/configs")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("get base config failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<BaseConfig>().await?)
    }

    /// 重新加载配置
    pub async fn reload_config(&self, force: bool, config_path: &str) -> Result<()> {
        let body = json!({ "path": config_path });
        let req = self
            .build_request(Method::PUT, "/configs")?
            .query(&[("force", force)])
            .json(&body);
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("reload base config failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 更新基础配置
    pub async fn patch_base_config<D: serde::Serialize + Clone + Sync>(&self, data: &D) -> Result<()> {
        let req = self.build_request(Method::PATCH, "/configs")?.json(data);
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("patch base config failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 更新 Geo, 同 [`upgrade_geo`](crate::mihomo::Mihomo::upgrade_geo)
    pub async fn update_geo(&self) -> Result<()> {
        let req = self.build_request(Method::POST, "/configs/geo")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("update geo database failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 重启核心
    pub async fn restart(&self) -> Result<()> {
        let req = self.build_request(Method::POST, "/restart")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("restart core failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 升级核心
    pub async fn upgrade_core(&self, channel: CoreUpdaterChannel, force: bool) -> Result<()> {
        let req = self
            .build_request(Method::POST, "/upgrade")?
            .query(&[("channel", channel.to_string()), ("force", force.to_string())]);
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("upgrade core failed: {msg}"),
                |err| {
                    let msg = err.message;
                    if msg.to_lowercase().contains("already using latest version") {
                        "already using latest version".to_string()
                    } else {
                        msg
                    }
                },
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 更新 UI
    pub async fn upgrade_ui(&self) -> Result<()> {
        let req = self.build_request(Method::POST, "/upgrade/ui")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("upgrade ui failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 更新 Geo
    pub async fn upgrade_geo(&self) -> Result<()> {
        let req = self.build_request(Method::POST, "/upgrade/geo")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("upgrade geo database failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
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
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        query: Option<String>,
        body: String,
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    async fn spawn_mock_server(status: &str, body: &str) -> Result<(SocketAddr, tokio::task::JoinHandle<CapturedRequest>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept failed");

            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            let mut header_end = None;
            let mut content_len = 0usize;

            loop {
                let n = stream.read(&mut tmp).await.expect("read failed");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);

                if header_end.is_none()
                    && let Some(pos) = find_bytes(&buf, b"\r\n\r\n")
                {
                    header_end = Some(pos + 4);
                    let head = String::from_utf8_lossy(&buf[..pos]);
                    for line in head.lines() {
                        if let Some((name, value)) = line.split_once(':')
                            && name.trim().eq_ignore_ascii_case("content-length")
                        {
                            content_len = value.trim().parse::<usize>().unwrap_or(0);
                        }
                    }
                }

                if let Some(end) = header_end
                    && buf.len() >= end + content_len
                {
                    break;
                }
            }

            let end = header_end.expect("header end not found");
            let head = String::from_utf8_lossy(&buf[..end - 4]);
            let mut lines = head.lines();
            let req_line = lines.next().expect("missing request line");
            let mut parts = req_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_string();
            let uri = parts.next().unwrap_or_default();
            let (path, query) = match uri.split_once('?') {
                Some((p, q)) => (p.to_string(), Some(q.to_string())),
                None => (uri.to_string(), None),
            };
            let body_text = String::from_utf8_lossy(&buf[end..]).to_string();

            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response failed");

            CapturedRequest {
                method,
                path,
                query,
                body: body_text,
            }
        });

        Ok((addr, handle))
    }

    fn backend_tcp(addr: SocketAddr) -> Result<Backend> {
        Backend::builder().set_tcp_addr(&addr.to_string()).build()
    }

    async fn mock_backend_ok() -> Result<(Backend, tokio::task::JoinHandle<CapturedRequest>)> {
        let (addr, handle) = spawn_mock_server("200 OK", "{}").await?;
        Ok((backend_tcp(addr)?, handle))
    }

    async fn wait_request(handle: tokio::task::JoinHandle<CapturedRequest>) -> CapturedRequest {
        handle.await.expect("mock server task failed")
    }

    fn assert_method_path(req: &CapturedRequest, method: &str, path: &str) {
        assert_eq!(req.method, method);
        assert_eq!(req.path, path);
    }

    fn assert_response_error<T>(result: Result<T>, expected: &str) {
        assert!(matches!(result, Err(IpcError::ResponseError(msg)) if msg == expected));
    }

    #[test]
    fn build_backend() -> Result<()> {
        let _ = Backend::builder().set_tcp_addr("127.0.0.1:9090").build()?;
        Ok(())
    }

    #[tokio::test]
    async fn test_get_version() -> Result<()> {
        let (addr, handle) = spawn_mock_server("200 OK", r#"{"meta":false,"version":"1.0.0"}"#).await?;
        let backend = backend_tcp(addr)?;
        let result = backend.get_version().await;

        assert!(result.is_ok());
        let version = result?;
        assert_eq!(version.version, "1.0.0");
        assert!(!version.meta);

        let req = wait_request(handle).await;
        assert_method_path(&req, "GET", "/version");
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
        let (backend, handle) = mock_backend_ok().await?;
        let result = backend.flush_fakeip().await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "POST", "/cache/fakeip/flush");

        Ok(())
    }

    #[tokio::test]
    async fn flush_dns() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;
        let result = backend.flush_dns().await;

        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "POST", "/cache/dns/flush");

        Ok(())
    }

    #[tokio::test]
    async fn get_connections() -> Result<()> {
        let (addr, handle) =
            spawn_mock_server("200 OK", r#"{"downloadTotal":1,"uploadTotal":2,"connections":[],"memory":3}"#).await?;
        let backend = backend_tcp(addr)?;
        let result = backend.get_connections().await;

        assert!(result.is_ok());
        let conns = result?;
        assert_eq!(conns.download_total, 1);
        assert_eq!(conns.upload_total, 2);
        assert_eq!(conns.memory, 3);

        let req = wait_request(handle).await;
        assert_method_path(&req, "GET", "/connections");

        Ok(())
    }

    #[tokio::test]
    async fn close_all_connections() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;
        let result = backend.close_all_connections().await;

        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "DELETE", "/connections");

        Ok(())
    }

    #[tokio::test]
    async fn get_groups() -> Result<()> {
        let (addr, handle) = spawn_mock_server("200 OK", r#"{"proxies":[]}"#).await?;
        let backend = backend_tcp(addr)?;
        let result = backend.get_groups().await;

        assert!(result.is_ok());
        let groups = result?;
        assert!(groups.proxies.is_empty());

        let req = wait_request(handle).await;
        assert_method_path(&req, "GET", "/group");

        Ok(())
    }

    #[tokio::test]
    async fn get_group_by_name() -> Result<()> {
        let body = r#"{"alive":true,"history":[],"extra":{},"name":"test-group","udp":true,"uot":false,"type":"Selector","xudp":false,"tfo":false,"mptcp":false,"smux":false,"interface":"","dialer-proxy":"","routing-mark":0}"#;
        let (addr, handle) = spawn_mock_server("200 OK", body).await?;
        let backend = backend_tcp(addr)?;
        let result = backend.get_group_by_name("test-group").await;

        assert!(result.is_ok());
        let group = result?;
        assert_eq!(group.name, "test-group");

        let req = wait_request(handle).await;
        assert_method_path(&req, "GET", "/group/test-group");

        Ok(())
    }

    #[tokio::test]
    async fn select_node_for_group() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;
        let result = backend.select_node_for_group("group a/b", "node-1").await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "PUT", "/proxies/group%20a%2Fb");
        let body: Value = serde_json::from_str(&req.body)?;
        assert_eq!(body["name"], "node-1");

        Ok(())
    }

    #[tokio::test]
    async fn get_rules() -> Result<()> {
        let (addr, handle) = spawn_mock_server("200 OK", r#"{"rules":[]}"#).await?;
        let backend = backend_tcp(addr)?;
        let result = backend.get_rules().await;

        assert!(result.is_ok());
        let rules = result?;
        assert!(rules.rules.is_empty());

        let req = wait_request(handle).await;
        assert_method_path(&req, "GET", "/rules");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_rule_providers() -> Result<()> {
        let (addr, handle) = spawn_mock_server("200 OK", r#"{"providers":{}}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.get_rule_providers().await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "GET", "/providers/rules");
        assert!(req.query.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_get_rule_providers_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"providers failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.get_rule_providers().await;
        assert_response_error(result, "providers failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_update_rule_provider() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;

        let result = backend.update_rule_provider("my-provider").await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "PUT", "/providers/rules/my-provider");

        Ok(())
    }

    #[tokio::test]
    async fn test_update_rule_provider_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"update provider failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.update_rule_provider("my-provider").await;
        assert_response_error(result, "update provider failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_base_config_error_path() -> Result<()> {
        let (addr, _handle) = spawn_mock_server("500 Internal Server Error", r#"{"message":"cfg failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.get_base_config().await;
        assert_response_error(result, "cfg failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_reload_config() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;

        let result = backend.reload_config(true, "/etc/mihomo/config.yaml").await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "PUT", "/configs");
        assert!(req.query.as_deref().unwrap_or_default().contains("force=true"));
        let body: Value = serde_json::from_str(&req.body)?;
        assert_eq!(body["path"], "/etc/mihomo/config.yaml");

        Ok(())
    }

    #[tokio::test]
    async fn test_reload_config_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"reload failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.reload_config(true, "/etc/mihomo/config.yaml").await;
        assert_response_error(result, "reload failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_patch_base_config() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;
        let payload = serde_json::json!({"mode": "rule", "allow-lan": true});

        let result = backend.patch_base_config(&payload).await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "PATCH", "/configs");
        let body: Value = serde_json::from_str(&req.body)?;
        assert_eq!(body["mode"], "rule");
        assert_eq!(body["allow-lan"], true);

        Ok(())
    }

    #[tokio::test]
    async fn test_patch_base_config_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"patch failed"}"#).await?;
        let backend = backend_tcp(addr)?;
        let payload = serde_json::json!({"mode": "rule"});

        let result = backend.patch_base_config(&payload).await;
        assert_response_error(result, "patch failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_update_geo() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;

        let result = backend.update_geo().await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "POST", "/configs/geo");

        Ok(())
    }

    #[tokio::test]
    async fn test_update_geo_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"update geo failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.update_geo().await;
        assert_response_error(result, "update geo failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_restart() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;

        let result = backend.restart().await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "POST", "/restart");

        Ok(())
    }

    #[tokio::test]
    async fn test_restart_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"restart failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.restart().await;
        assert_response_error(result, "restart failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_upgrade_core() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;

        let result = backend
            .upgrade_core(CoreUpdaterChannel::ReleaseChannel, true)
            .await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "POST", "/upgrade");
        let query = req.query.unwrap_or_default();
        assert!(query.contains("channel=release"));
        assert!(query.contains("force=true"));

        Ok(())
    }

    #[tokio::test]
    async fn test_upgrade_core_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"upgrade core failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend
            .upgrade_core(CoreUpdaterChannel::ReleaseChannel, true)
            .await;
        assert_response_error(result, "upgrade core failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_upgrade_core_latest_version_message_normalized() -> Result<()> {
        let (addr, _handle) = spawn_mock_server(
            "500 Internal Server Error",
            r#"{"message":"Already using latest version v1.2.3"}"#,
        )
        .await?;
        let backend = backend_tcp(addr)?;

        let result = backend
            .upgrade_core(CoreUpdaterChannel::ReleaseChannel, true)
            .await;
        assert_response_error(result, "already using latest version");

        Ok(())
    }

    #[tokio::test]
    async fn test_upgrade_ui() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;

        let result = backend.upgrade_ui().await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "POST", "/upgrade/ui");

        Ok(())
    }

    #[tokio::test]
    async fn test_upgrade_ui_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"upgrade ui failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.upgrade_ui().await;
        assert_response_error(result, "upgrade ui failed");

        Ok(())
    }

    #[tokio::test]
    async fn test_upgrade_geo() -> Result<()> {
        let (backend, handle) = mock_backend_ok().await?;

        let result = backend.upgrade_geo().await;
        assert!(result.is_ok());

        let req = wait_request(handle).await;
        assert_method_path(&req, "POST", "/upgrade/geo");

        Ok(())
    }

    #[tokio::test]
    async fn test_upgrade_geo_error_path() -> Result<()> {
        let (addr, _handle) =
            spawn_mock_server("500 Internal Server Error", r#"{"message":"upgrade geo failed"}"#).await?;
        let backend = backend_tcp(addr)?;

        let result = backend.upgrade_geo().await;
        assert_response_error(result, "upgrade geo failed");

        Ok(())
    }
}
