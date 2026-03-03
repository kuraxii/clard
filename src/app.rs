pub mod state;
pub mod proxy;
pub mod preview;
pub mod connections;

use state::{MenuItem, MenuState};
use tokio::sync::mpsc::UnboundedSender;
use std::sync::Arc;

use crate::{event::ClardEvent, ipc::backend::Backend};
use proxy::ProxyState;
use preview::PreviewState;
use connections::ConnectionsState;

/// WindowState
/// 用于记录窗口的状态，MENU、Preview、PROXY、CONNECTIONS、RULE、TEST
#[derive(Debug)]
pub enum WindowState {
    /// 菜单页面
    Memu,
    /// 预览页面
    Preview(PreviewState),
    /// 代理查看选择页面
    Proxy(ProxyState),
    /// 连接流量统计页面
    Connects(ConnectionsState),
    /// 规则页面
    Rules,
    /// ip 测试页面
    NetTest,
}

pub struct APP {
    pub current_page: WindowState,
    pub menusate: MenuState,
    pub event_sender: UnboundedSender<ClardEvent>,
    pub backend: Arc<Backend>,
    pub message: Option<String>,
}

impl APP {
    pub fn init(sender: UnboundedSender<ClardEvent>, backend: Arc<Backend>) -> Self {
        APP {
            current_page: WindowState::Memu,
            menusate: MenuState::init(),
            event_sender: sender,
            backend,
            message: None,
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

    pub fn fetch_preview(&self) {
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(version) = backend.get_version().await {
                let _ = sender.send(ClardEvent::UpdateVersion(version));
            }
            if let Ok(conns) = backend.get_connections().await {
                let _ = sender.send(ClardEvent::UpdateConnections(conns));
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

    pub fn switch_page(&mut self, item: MenuItem) {
        match item {
            MenuItem::Proxy => {
                self.current_page = WindowState::Proxy(ProxyState::new());
                self.fetch_groups();
            }
            MenuItem::Preview => {
                self.current_page = WindowState::Preview(PreviewState::new());
                self.fetch_preview();
            }
            MenuItem::Connections => {
                self.current_page = WindowState::Connects(ConnectionsState::new());
                self.fetch_connections();
            }
            MenuItem::Rules => {
                self.current_page = WindowState::Rules;
            }
            MenuItem::NetTest => {
                self.current_page = WindowState::NetTest;
            }
        }
    }

    pub fn on_up_key(&mut self) {
        match &mut self.current_page {
            WindowState::Memu => self.menusate.prev(),
            WindowState::Preview(_) => {}
            WindowState::Proxy(state) => state.on_up_key(),
            WindowState::Connects(state) => state.on_up_key(),
            WindowState::Rules => {}
            WindowState::NetTest => {}
        }
    }

    pub fn on_down_key(&mut self) {
        match &mut self.current_page {
            WindowState::Memu => self.menusate.next(),
            WindowState::Preview(_) => {}
            WindowState::Proxy(state) => state.on_down_key(),
            WindowState::Connects(state) => state.on_down_key(),
            WindowState::Rules => {}
            WindowState::NetTest => {}
        }
    }

    pub fn on_left_key(&mut self) {
        match &mut self.current_page {
            WindowState::Proxy(state) => state.on_left_key(),
            _ => {}
        }
    }

    pub fn on_right_key(&mut self) {
        match &mut self.current_page {
            WindowState::Proxy(state) => state.on_right_key(),
            _ => {}
        }
    }

    pub fn on_home_key(&mut self) {}

    pub fn on_end_key(&mut self) {}

    pub fn on_pagedown_key(&mut self) {}

    pub fn on_pageup_key(&mut self) {}

    pub fn on_backspace_key(&mut self) {}

    pub fn on_delete_key(&mut self) {}

    pub fn on_tab_key(&mut self) {
        match &self.current_page {
            WindowState::Memu => {
                let current_item = self.menusate.current();
                self.switch_page(current_item);
            }
            _ => {
                self.current_page = WindowState::Memu;
            }
        }
    }

    pub fn on_esc_key(&mut self) {
        self.current_page = WindowState::Memu;
    }

    pub fn on_enter_key(&mut self) {
        match &mut self.current_page {
            WindowState::Memu => {
                let current_item = self.menusate.current();
                self.switch_page(current_item);
            }
            WindowState::Proxy(state) => {
                if state.focus == proxy::ProxyFocus::Proxies {
                    if let Some(g_idx) = state.group_list_state.selected() {
                        if let Some(p_idx) = state.proxy_list_state.selected() {
                            if let Some(group) = state.groups.get(g_idx) {
                                if let Some(all) = &group.all {
                                    if let Some(node_name) = all.get(p_idx) {
                                        let backend = self.backend.clone();
                                        let group_name = group.name.clone();
                                        let node = node_name.clone();
                                        let sender = self.event_sender.clone();
                                        
                                        tokio::spawn(async move {
                                            match backend.select_node_for_group(&group_name, &node).await {
                                                Ok(_) => {
                                                    // Refresh groups to see updated 'now'
                                                    match backend.get_groups().await {
                                                        Ok(groups) => {
                                                            let _ = sender.send(ClardEvent::UpdateGroups(groups));
                                                        }
                                                        Err(_) => {}
                                                    }
                                                }
                                                Err(e) => {
                                                    let _ = sender.send(ClardEvent::Error(format!("Select node error: {}", e)));
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn on_char(&mut self, char: char) {
        match &mut self.current_page {
            WindowState::Memu => {
                let _ = self.menusate.on_char(char, self.event_sender.clone());
            }
            WindowState::Proxy(state) => {
                if char == 'd' || char == 't' {
                    // Test latency for currently selected node
                    if state.focus == proxy::ProxyFocus::Proxies {
                        if let Some(g_idx) = state.group_list_state.selected() {
                            if let Some(p_idx) = state.proxy_list_state.selected() {
                                if let Some(group) = state.groups.get(g_idx) {
                                    if let Some(all) = &group.all {
                                        if let Some(node_name) = all.get(p_idx) {
                                            let backend = self.backend.clone();
                                            let node = node_name.clone();
                                            let sender = self.event_sender.clone();
                                            // The default url to test
                                            let url = "http://www.gstatic.com/generate_204".to_string();
                                            let timeout = 5000;
                                            
                                            tokio::spawn(async move {
                                                match backend.delay_proxy_for_name(&node, &url, timeout).await {
                                                    Ok(delay) => {
                                                        let _ = sender.send(ClardEvent::NodeTested(node, delay));
                                                        // we can also re-fetch groups to refresh the history in the state
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
                                }
                            }
                        }
                    }
                }
            }
            WindowState::Connects(state) => {
                if char == 'x' || char == 'd' {
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
            }
            _ => {}
        }
    }
}

