pub mod connections;
pub mod home;
pub mod logs;
pub mod page;
pub mod profiles;
pub mod proxy;
pub mod rules;
pub mod settings;

use std::sync::Arc;

use connections::ConnectionsState;
use home::HomeState;
use logs::LogsState;
use page::Page;
use profiles::ProfilesState;
use proxy::{ProxyFocus, ProxyState};
use rules::RulesState;
use settings::SettingsState;
use tokio::sync::mpsc::UnboundedSender;

use clard_core::mihomo::{backend::Backend, models::Traffic, websocket::get_websocket_url};

use crate::event::ClardEvent;

/// 应用状态：当前页 + 各页独立状态（切页保留内部状态，doc/03 §1 原则 3）。
pub struct APP {
    pub current_page: Page,
    pub home: HomeState,
    pub profiles: ProfilesState,
    pub proxies: ProxyState,
    pub connections: ConnectionsState,
    pub logs: LogsState,
    pub settings: SettingsState,
    pub rules: RulesState,
    pub event_sender: UnboundedSender<ClardEvent>,
    pub backend: Arc<Backend>,
    pub message: Option<String>,
    pub show_help: bool,
    traffic_subscription_started: bool,
}

impl APP {
    pub fn init(sender: UnboundedSender<ClardEvent>, backend: Arc<Backend>) -> Self {
        APP {
            current_page: Page::Home,
            home: HomeState::default(),
            profiles: ProfilesState::default(),
            proxies: ProxyState::new(),
            connections: ConnectionsState::new(),
            logs: LogsState::default(),
            settings: SettingsState::default(),
            rules: RulesState::default(),
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

    /// 切换到指定页面；进入页面时按需拉取/订阅数据。
    pub fn switch_page(&mut self, page: Page) {
        self.current_page = page;
        match page {
            Page::Proxies => self.fetch_groups(),
            Page::Connections => {
                self.fetch_connections();
                self.subscribe_traffic();
            }
            _ => {}
        }
    }

    pub fn on_up_key(&mut self) {
        match self.current_page {
            Page::Proxies => self.proxies.on_up_key(),
            Page::Connections => self.connections.on_up_key(),
            _ => {}
        }
    }

    pub fn on_down_key(&mut self) {
        match self.current_page {
            Page::Proxies => self.proxies.on_down_key(),
            Page::Connections => self.connections.on_down_key(),
            _ => {}
        }
    }

    pub fn on_left_key(&mut self) {
        if self.current_page == Page::Proxies {
            self.proxies.on_left_key();
        }
    }

    pub fn on_right_key(&mut self) {
        if self.current_page == Page::Proxies {
            self.proxies.on_right_key();
        }
    }

    pub fn on_tab_key(&mut self) {
        if self.current_page == Page::Proxies {
            let state = &mut self.proxies;
            if state.focus == ProxyFocus::Groups {
                state.on_right_key();
            } else {
                state.on_left_key();
            }
        }
    }

    pub fn on_home_key(&mut self) {}

    pub fn on_end_key(&mut self) {}

    pub fn on_pagedown_key(&mut self) {}

    pub fn on_pageup_key(&mut self) {}

    pub fn on_backspace_key(&mut self) {}

    pub fn on_delete_key(&mut self) {}

    pub fn on_esc_key(&mut self) {
        if self.show_help {
            self.show_help = false;
            return;
        }

        if self.current_page == Page::Home {
            // 主页时 Esc = 退出（doc/03 §4.2）
            let _ = self.event_sender.send(ClardEvent::Terminal);
        } else {
            // 其它页返回主页（「返回上一层」）
            self.current_page = Page::Home;
        }
    }

    pub fn on_enter_key(&mut self) {
        if self.current_page == Page::Proxies {
            self.select_proxy_node();
        }
    }

    fn select_proxy_node(&mut self) {
        let state = &mut self.proxies;
        if state.focus != ProxyFocus::Proxies {
            return;
        }

        let Some((group_name, node_name)) = state.selected_node() else {
            return;
        };
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

    fn test_proxy_delay(&mut self) {
        let state = &self.proxies;
        let Some((_group_name, node_name)) = state.selected_node() else {
            return;
        };
        let backend = self.backend.clone();
        let node = node_name.to_string();
        let sender = self.event_sender.clone();
        let timeout = 5000;
        let url = "http://www.gstatic.com/generate_204".to_string();
        tokio::spawn(async move {
            match backend.delay_proxy_for_name(&node, &url, timeout).await {
                Ok(delay) => {
                    let _ = sender.send(ClardEvent::Notify(format!("{node} delay: {delay}ms")));
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

    fn close_selected_connection(&mut self) {
        let Some(idx) = self.connections.list_state.selected() else {
            return;
        };
        let Some(conn) = self.connections.connections.get(idx) else {
            return;
        };
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
            '1'..='7' => {
                if let Some(page) = Page::from_key(char) {
                    self.switch_page(page);
                }
                return;
            }
            _ => {}
        }

        if self.show_help {
            return;
        }

        match self.current_page {
            Page::Proxies if (char == 'd' || char == 't') && self.proxies.focus == ProxyFocus::Proxies => {
                self.test_proxy_delay();
            }
            Page::Connections if char == 'u' => self.connections.sort_by_upload(),
            Page::Connections if char == 'd' => self.connections.sort_by_download(),
            Page::Connections if char == 'x' => self.close_selected_connection(),
            _ => {}
        }
    }
}
