
use base64::{Engine, engine::general_purpose};
use futures_util::{SinkExt, Stream, StreamExt, stream::SplitSink};
use http::{
    Request,
    header::{CONNECTION, HOST, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION, UPGRADE},
};
use rand::Rng;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpStream, UnixStream},
    sync::mpsc::{UnboundedSender, unbounded_channel},
};
use tokio_tungstenite::{WebSocketStream, client_async, tungstenite::Message};
use tracing::{error, info, warn};

use super::{
    backend::Protocol,
    error::{IpcError, Result},
};

/// websocket 控制消息
#[derive(Debug)]
pub enum WsControl {
    /// 发送消息
    Send(Message),
    /// 关闭连接
    Close,
}

/// Stream 枚举 以支持TCP 与 unix套接字
#[derive(Debug)]
pub enum MaybeStream {
    /// TCP流
    TCP(TcpStream),
    /// unix socket 流
    UDS(UnixStream),
}

/// 连接websocket callback 模式
/// 返回sender 可以由上层主动关闭连接
pub async fn connect_with<T, F>(
    backend_type: Protocol,
    url: Url,
    handle_message: F,
) -> Result<UnboundedSender<WsControl>>
where
    T: serde::de::DeserializeOwned + Send + 'static,
    F: Fn(WebSocketMessage<T>) + Send + 'static,
{
    // 1. 建立基础流
    let url_clone = url.clone();
    let (mut writer, mut stream) = connect::<T>(backend_type, url).await?;
    let (tx, mut rx) = unbounded_channel::<WsControl>();

    // 2. 启动异步任务
    tokio::spawn(async move {
        info!("WebSocket worker started for URL: {}", url_clone);

        loop {
            tokio::select! {
                // 使用 biased 确保优先处理关闭和发送指令
                biased;

                // 处理外部发送指令
                msg_in = rx.recv() => {
                    match msg_in {
                        Some(WsControl::Send(msg)) => {
                            if let Err(e) = writer.send(msg).await {
                                error!("Failed to send WS message: {:?}", e);
                                break;
                            }
                        }
                        Some(WsControl::Close) => {
                            let _ = writer.close().await;
                            info!("Connection closed by local WsControl");
                            break;
                        }
                        None => {
                            warn!("WsControl sender dropped, closing worker");
                            let _ = writer.close().await;
                            break;
                        }
                    }
                }

                // 处理收到的网络消息
                ws_in = stream.next() => {
                    match ws_in {
                        Some(Ok(message)) => {
                            let is_close = matches!(message, WebSocketMessage::Close(_));
                            // 使用回调/闭包处理数据
                            handle_message(message);

                            // 如果是close消息则退出
                            if is_close {
                                let _ = writer.close().await;
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            error!("WebSocket Stream Error: {:?}", e);
                            break;
                        }
                        None => {
                            info!("WebSocket connection closed by remote");
                            break;
                        }
                    }
                }
            }
        }
        info!("WebSocket worker task exited.");
    });

    Ok(tx)
}

/// trait 绑定  只要实现了AsyncRead + AsyncWrite + Unpin的类型就可以被视为 AsyncStream
pub trait AsyncStream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<S: AsyncRead + AsyncWrite + Send + Unpin> AsyncStream for S {}

/// websocket 写
pub type WsWriter = SplitSink<WebSocketStream<Box<dyn AsyncStream + Send>>, Message>;

/// 连接websocket stream模式
pub async fn connect<T>(
    protocol: Protocol,
    url: Url,
) -> Result<(WsWriter, impl Stream<Item = Result<WebSocketMessage<T>>>)>
where
    T: serde::de::DeserializeOwned,
{
    let stream: Box<dyn AsyncStream + Send + 'static> = match protocol {
        Protocol::UDS(path) => Box::new(UnixStream::connect(path).await?),
        Protocol::TCP(addr) => Box::new(TcpStream::connect(addr).await?),
    };

    let request = Request::builder()
        .uri(url.as_str())
        .header(HOST, url.host_str().unwrap_or("localhost"))
        .header(SEC_WEBSOCKET_KEY, generate_websocket_key())
        .header(CONNECTION, "Upgrade")
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_VERSION, "13")
        .body(())?;
    let (ws_stream, _) = client_async(request, stream).await?;
    let (writer, reader) = ws_stream.split();

    let stream = reader.map(|message| match message {
        Ok(Message::Text(msg)) => serde_json::from_str::<T>(msg.as_str())
            .map(WebSocketMessage::Text)
            .map_err(IpcError::Json),
        Ok(Message::Binary(msg)) => Ok(WebSocketMessage::Binary(msg.to_vec())),
        Ok(Message::Ping(msg)) => Ok(WebSocketMessage::Ping(msg.to_vec())),
        Ok(Message::Pong(msg)) => Ok(WebSocketMessage::Pong(msg.to_vec())),
        Ok(Message::Frame(_)) => Ok(WebSocketMessage::None),
        Ok(Message::Close(msg)) => Ok(WebSocketMessage::Close(msg.map(|v| CloseFrame {
            code: v.code.into(),
            reason: v.reason.to_string(),
        }))),
        Err(e) => Err(IpcError::WebSocket(e)),
    });

    Ok((writer, stream))
}

/// 获取websocket url, localhost only
#[inline]
pub fn get_websocket_url(suffix: &str) -> String {
    let clean_suffix = suffix.trim_start_matches('/');
    format!("ws://localhost/{}", clean_suffix)
}

/// 生成webwocket密钥
#[inline]
pub fn generate_websocket_key() -> String {
    let mut rng = rand::rng();
    let mut key = [0u8; 16];
    rng.fill(&mut key);
    general_purpose::STANDARD.encode(key)
}

///
#[derive(Debug, Deserialize, Serialize)]
pub struct CloseFrame {
    ///
    pub code: u16,
    ///
    pub reason: String,
}

/// websocket wapper
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum WebSocketMessage<T> {
    /// 文本消息体
    Text(T),
    /// 二进制消息
    Binary(Vec<u8>),
    /// 心跳消息
    Ping(Vec<u8>),
    /// 心跳消息
    Pong(Vec<u8>),
    /// 连接关闭消息
    Close(Option<CloseFrame>),
    /// 无效消息
    None,
}
