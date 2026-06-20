pub mod checker;
pub mod connections;
pub mod nettest;
pub mod proxy;
pub mod state;

use std::sync::Arc;

use connections::ConnectionsState;
use nettest::NetTestState;
use proxy::ProxyState;
use state::{MenuItem, MenuState};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    event::ClardEvent,
    ipc::{backend::Backend, models::Traffic, websocket::get_websocket_url},
};

/// WindowState
/// 用于记录窗口的状态，MENU、PROXY、CONNECTIONS、TEST
#[derive(Debug)]
pub enum WindowState {
    /// 菜单页面
    Memu,
    /// 代理查看选择页面
    Proxy(ProxyState),
    /// 连接流量统计页面
    Connects(ConnectionsState),
    /// ip 测试页面
    NetTest(NetTestState),
}

pub struct APP {
    pub current_page: WindowState,
    pub menusate: MenuState,
    pub event_sender: UnboundedSender<ClardEvent>,
    pub backend: Arc<Backend>,
    pub message: Option<String>,
    pub show_help: bool,
    traffic_subscription_started: bool,
}

impl APP {
    pub fn init(sender: UnboundedSender<ClardEvent>, backend: Arc<Backend>) -> Self {
        APP {
            current_page: WindowState::Memu,
            menusate: MenuState::init(),
            event_sender: sender,
            backend,
            message: None,
            show_help: false,
            traffic_subscription_started: false,
        }
    }

    pub fn fetch_groups(&self) {
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match backend.get_groups().await {
                Ok(groups) => {
                    let _ = sender.send(ClardEvent::UpdateGroups(groups));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(format!("Fetch groups error: {}", e)));
                }
            }
        });
    }

    pub fn fetch_connections(&self) {
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(conns) = backend.get_connections().await {
                let _ = sender.send(ClardEvent::UpdateConnections(conns));
            }
        });
    }

    pub fn subscribe_traffic(&mut self) {
        if self.traffic_subscription_started {
            return;
        }
        self.traffic_subscription_started = true;

        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            let Ok(url) = reqwest::Url::parse(&get_websocket_url("traffic")) else {
                let _ = sender.send(ClardEvent::Error("Invalid traffic websocket URL".to_string()));
                return;
            };

            match backend.subscribe::<Traffic>(url).await {
                Ok((mut traffic_rx, _ctrl_tx)) => {
                    while let Some(traffic) = traffic_rx.recv().await {
                        if sender.send(ClardEvent::UpdateTraffic(traffic)).is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(format!("Subscribe traffic error: {}", e)));
                }
            }
        });
    }

    pub fn fetch_nettest_nodes(&self) {
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(groups) = backend.get_groups().await {
                // Collect all unique node names across all groups
                let mut node_names = std::collections::HashSet::new();
                for group in groups.proxies {
                    if let Some(all) = group.all {
                        for node in all {
                            node_names.insert(node);
                        }
                    }
                }

                let mut node_list: Vec<String> = node_names.into_iter().collect();
                node_list.sort();

                let _ = sender.send(ClardEvent::NetTestNodesReady(node_list));
            }
        });
    }

    pub fn trigger_analysis(&mut self) {
        if let WindowState::NetTest(state) = &mut self.current_page {
            if state.analysis_testing {
                return;
            }
            state.analysis_testing = true;
        }

        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            let mut result = checker::AnalysisResult::default();

            // Wait for tests
            // In a real implementation we would fetch active proxy and pass its proxy_url
            // Here we just use a default or mocked proxy_url
            let proxy_url = "http://127.0.0.1:7890";

            let direct_ip_future = checker::check_ip_direct();
            let proxy_ip_future = checker::check_ip_proxy(proxy_url);

            if let Ok(proxy_client) = checker::create_proxy_client(proxy_url) {
                let netflix_future = checker::check_netflix(&proxy_client);
                let youtube_future = checker::check_youtube(&proxy_client);
                let spotify_future = checker::check_spotify(&proxy_client);
                let bilibili_future = checker::check_bilibili(&proxy_client);

                let (direct_ip, proxy_ip, netflix, youtube, spotify, bilibili) = tokio::join!(
                    direct_ip_future,
                    proxy_ip_future,
                    netflix_future,
                    youtube_future,
                    spotify_future,
                    bilibili_future
                );

                result.direct_ip = direct_ip.ok();
                result.proxy_ip = proxy_ip.ok();
                result.netflix_status = netflix;
                result.youtube_status = youtube;
                result.spotify_status = spotify;
                result.bilibili_status = bilibili;
            } else {
                // Just do direct IP
                let direct_ip = direct_ip_future.await;
                result.direct_ip = direct_ip.ok();
            }

            let _ = sender.send(ClardEvent::AnalysisResultUpdated(Box::new(result)));
        });
    }

    pub fn switch_page(&mut self, item: MenuItem) {
        match item {
            MenuItem::Proxy => {
                self.current_page = WindowState::Proxy(ProxyState::new());
                self.fetch_groups();
            }
            MenuItem::Connections => {
                self.current_page = WindowState::Connects(ConnectionsState::new());
                self.fetch_connections();
                self.subscribe_traffic();
            }
            MenuItem::NetTest => {
                self.current_page = WindowState::NetTest(NetTestState::new());
                self.fetch_nettest_nodes();
                self.trigger_analysis();
            }
        }
    }

    pub fn on_up_key(&mut self) {
        match &mut self.current_page {
            WindowState::Memu => self.menusate.prev(),
            WindowState::Proxy(state) => state.on_up_key(),
            WindowState::Connects(state) => state.on_up_key(),
            WindowState::NetTest(state) => state.on_up_key(),
        }
    }

    pub fn on_down_key(&mut self) {
        match &mut self.current_page {
            WindowState::Memu => self.menusate.next(),
            WindowState::Proxy(state) => state.on_down_key(),
            WindowState::Connects(state) => state.on_down_key(),
            WindowState::NetTest(state) => state.on_down_key(),
        }
    }

    pub fn on_left_key(&mut self) {
        if let WindowState::Proxy(state) = &mut self.current_page {
            state.on_left_key();
        }
    }

    pub fn on_right_key(&mut self) {
        if let WindowState::Proxy(state) = &mut self.current_page {
            state.on_right_key();
        }
    }

    pub fn on_home_key(&mut self) {}

    pub fn on_end_key(&mut self) {}

    pub fn on_pagedown_key(&mut self) {}

    pub fn on_pageup_key(&mut self) {}

    pub fn on_backspace_key(&mut self) {}

    pub fn on_delete_key(&mut self) {}

    pub fn on_tab_key(&mut self) {
        match &mut self.current_page {
            WindowState::Memu => {
                let current_item = self.menusate.current();
                self.switch_page(current_item);
            }
            WindowState::NetTest(state) => {
                let mut should_trigger = false;
                state.tab = match state.tab {
                    nettest::NetTestTab::Latency => {
                        should_trigger = true;
                        nettest::NetTestTab::Analysis
                    }
                    nettest::NetTestTab::Analysis => nettest::NetTestTab::Latency,
                };
                if should_trigger {
                    self.trigger_analysis();
                }
            }
            WindowState::Proxy(state) => {
                if state.focus == proxy::ProxyFocus::Groups {
                    state.on_right_key();
                } else {
                    state.on_left_key();
                }
            }
            _ => {
                self.current_page = WindowState::Memu;
            }
        }
    }

    pub fn on_esc_key(&mut self) {
        if self.show_help {
            self.show_help = false;
            return;
        }

        if matches!(self.current_page, WindowState::Memu) {
            let _ = self.event_sender.send(ClardEvent::Terminal);
        } else {
            self.current_page = WindowState::Memu;
        }
    }

    pub fn on_enter_key(&mut self) {
        match &mut self.current_page {
            WindowState::Memu => {
                let current_item = self.menusate.current();
                self.switch_page(current_item);
            }
            WindowState::Proxy(state) => {
                if state.focus != proxy::ProxyFocus::Proxies {
                    return;
                }

                if let Some((group_name, node_name)) = state.selected_node() {
                    let backend = self.backend.clone();
                    let group_name = group_name.to_string();
                    let node = node_name.to_string();
                    let sender = self.event_sender.clone();

                    tokio::spawn(async move {
                        if let Err(e) = backend.select_node_for_group(&group_name, &node).await {
                            let _ = sender.send(ClardEvent::Error(format!("Select node error: {}", e)));
                            return;
                        }

                        if let Ok(groups) = backend.get_groups().await {
                            let _ = sender.send(ClardEvent::UpdateGroups(groups));
                        }
                    });
                }
            }
            _ => {}
        }
    }

    pub fn on_char(&mut self, char: char) {
        match char {
            '?' => {
                self.show_help = !self.show_help;
                return;
            }
            'q' => {
                let _ = self.event_sender.send(ClardEvent::Terminal);
                return;
            }
            'j' => {
                self.on_down_key();
                return;
            }
            'k' => {
                self.on_up_key();
                return;
            }
            'h' => {
                self.on_left_key();
                return;
            }
            'l' => {
                self.on_right_key();
                return;
            }
            '1' => {
                self.switch_page(MenuItem::Proxy);
                return;
            }
            '2' => {
                self.switch_page(MenuItem::Connections);
                return;
            }
            '3' => {
                self.switch_page(MenuItem::NetTest);
                return;
            }
            _ => {}
        }

        if self.show_help {
            return;
        }

        match &mut self.current_page {
            WindowState::Memu => {
                self.menusate.on_char(char, self.event_sender.clone());
            }
            WindowState::Proxy(state) if (char == 'd' || char == 't') && state.focus == proxy::ProxyFocus::Proxies => {
                // Test latency for currently selected node
                if let Some((_group_name, node_name)) = state.selected_node() {
                    let backend = self.backend.clone();
                    let node = node_name.to_string();
                    let sender = self.event_sender.clone();
                    let timeout = 5000;
                    let url = "http://www.gstatic.com/generate_204".to_string();
                    tokio::spawn(async move {
                        match backend.delay_proxy_for_name(&node, &url, timeout).await {
                            Ok(delay) => {
                                let _ = sender.send(ClardEvent::NodeTested(node, delay));
                                if let Ok(groups) = backend.get_groups().await {
                                    let _ = sender.send(ClardEvent::UpdateGroups(groups));
                                }
                            }
                            Err(e) => {
                                let _ = sender.send(ClardEvent::Error(format!("Delay test error: {}", e)));
                            }
                        }
                    });
                }
            }
            WindowState::Connects(state) if char == 'u' => {
                state.sort_by_upload();
            }
            WindowState::Connects(state) if char == 'd' => {
                state.sort_by_download();
            }
            WindowState::Connects(state) if char == 'x' => {
                if let Some(idx) = state.list_state.selected() {
                    if let Some(conn) = state.connections.get(idx) {
                        let backend = self.backend.clone();
                        let conn_id = conn.id.clone();
                        let sender = self.event_sender.clone();
                        tokio::spawn(async move {
                            if backend.close_connection(&conn_id).await.is_ok() {
                                if let Ok(conns) = backend.get_connections().await {
                                    let _ = sender.send(ClardEvent::UpdateConnections(conns));
                                }
                            }
                        });
                    }
                }
            }
            WindowState::NetTest(state) => {
                if char == 's' {
                    state.sort();
                } else if (char == 't' || char == 'r') && state.start_testing_all() {
                    let backend = self.backend.clone();
                    let sender = self.event_sender.clone();
                    let nodes: Vec<String> = state.nodes.iter().map(|n| n.name.clone()).collect();

                    tokio::spawn(async move {
                        for node in nodes {
                            let url = "http://www.gstatic.com/generate_204".to_string();
                            let timeout = 5000;
                            let node_clone = node.clone();
                            let b_clone = backend.clone();
                            let s_clone = sender.clone();

                            // Test concurrently without waiting for previous
                            tokio::spawn(async move {
                                match b_clone.delay_proxy_for_name(&node_clone, &url, timeout).await {
                                    Ok(delay) => {
                                        let _ = s_clone.send(ClardEvent::NodeTested(node_clone, delay));
                                    }
                                    Err(e) => {
                                        let _ = s_clone.send(ClardEvent::NetTestError(node_clone, format!("{}", e)));
                                    }
                                }
                            });
                        }
                    });
                }
            }
            _ => {}
        }
    }
}
