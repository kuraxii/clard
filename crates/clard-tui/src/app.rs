pub mod connections;
pub mod home;
pub mod logs;
pub mod modal;
pub mod page;
pub mod profiles;
pub mod proxy;
pub mod rules;
pub mod settings;

use std::sync::Arc;

use connections::ConnectionsState;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use home::HomeState;
use logs::LogsState;
use modal::{ConfirmPurpose, ConfirmState, InputPurpose, InputState};
use page::Page;
use profiles::{HistoryView, ProfileBusy, ProfilesState};
use proxy::{ProxyFocus, ProxyState};
use rules::{RulesState, RulesTab};
use settings::SettingsState;
use tokio::sync::mpsc::UnboundedSender;

use clard_core::{
    config_gen::{self, subscription_to_yaml, ConfigGenOptions},
    mihomo::{backend::Backend, models::Traffic, websocket::get_websocket_url},
    profiles::HttpFetcher,
};
use clard_proto::{ProfileImport, ProfileItem, Request, Response};

use crate::{event::ClardEvent, rpc};

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
    /// 输入弹窗（订阅 URL、搜索、数字等）
    pub input: Option<InputState>,
    /// 确认弹窗（危险操作二次确认）
    pub confirm: Option<ConfirmState>,
    /// 配置历史版本视图（`h` 打开）
    pub history: Option<HistoryView>,
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
            profiles: ProfilesState::new(),
            proxies: ProxyState::new(),
            connections: ConnectionsState::new(),
            logs: LogsState::default(),
            settings: SettingsState::default(),
            rules: RulesState::default(),
            input: None,
            confirm: None,
            history: None,
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
            Page::Profiles => self.fetch_profiles(),
            Page::Proxies => self.fetch_groups(),
            Page::Connections => {
                self.fetch_connections();
                self.subscribe_traffic();
            }
            Page::Rules => {
                self.fetch_rules();
                self.fetch_rule_providers();
            }
            _ => {}
        }
    }

    // ---- 订阅配置（Profiles）----

    pub fn fetch_profiles(&self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            send_profiles(&sender).await;
        });
    }

    /// 应用 `ProfileList` 结果并清空行内忙碌状态。
    pub fn apply_profiles(&mut self, current: Option<String>, items: Vec<ProfileItem>) {
        self.profiles.update_list(current, items);
        self.profiles.busy = ProfileBusy::Idle;
        self.profiles.busy_uid = None;
    }

    /// 从 URL 导入订阅（R2.1）：下载 → 归一化 → helper 落盘 → 刷新列表。
    pub fn import_profile(&mut self, url: String) {
        self.message = Some(format!("importing {url}…"));
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match import_profile_flow(url).await {
                Ok(msg) => {
                    let _ = sender.send(ClardEvent::Notify(msg));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e));
                }
            }
            send_profiles(&sender).await;
        });
    }

    /// 手动更新订阅（R2.3）：取回 URL → 重新下载 → 同 URL 覆盖。
    pub fn update_profile(&mut self, uid: String) {
        self.profiles.busy = ProfileBusy::Updating;
        self.profiles.busy_uid = Some(uid.clone());
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match update_profile_flow(uid).await {
                Ok(msg) => {
                    let _ = sender.send(ClardEvent::Notify(msg));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e));
                }
            }
            send_profiles(&sender).await;
        });
    }

    /// 删除配置（R2.4）；若删的是当前配置，helper 清空 current 后 TUI 自动选中下一个。
    pub fn remove_profile(&mut self, uid: String) {
        let was_current = self.profiles.current.as_deref() == Some(uid.as_str());
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            let result = match rpc::call(&Request::ProfileRemove { uid: uid.clone() }).await {
                Ok(Response::Ok) => Ok(()),
                Ok(other) => Err(rpc::unexpected(other).to_string()),
                Err(e) => Err(e.to_string()),
            };

            if result.is_ok() && was_current
                && let Ok(Response::ProfileList { items, .. }) = rpc::call(&Request::ProfileList).await
                && let Some(next) = items.first()
            {
                // helper 已清空 current，按列表顺序自动切换到下一个（doc/05 §2 R2.4）
                let _ = rpc::call(&Request::ProfileSetCurrent { uid: next.uid.clone() }).await;
            }

            match result {
                Ok(()) => {
                    let _ = sender.send(ClardEvent::Notify(format!("profile deleted: {uid}")));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e));
                }
            }
            send_profiles(&sender).await;
        });
    }

    /// 改名（R2.5）。
    pub fn rename_profile(&mut self, uid: String, name: String) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::ProfileRename { uid: uid.clone(), name }).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify(format!("profile renamed: {uid}")));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_profiles(&sender).await;
        });
    }

    /// 上移/下移（R2.6）。
    pub fn move_profile(&mut self, uid: String, up: bool) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Err(e) = rpc::call(&Request::ProfileMove { uid, up }).await {
                let _ = sender.send(ClardEvent::Error(e.to_string()));
            }
            send_profiles(&sender).await;
        });
    }

    /// 打开历史版本视图（R2.9）。
    pub fn open_history(&mut self) {
        let Some(uid) = self.profiles.selected().map(|p| p.uid.clone()) else {
            return;
        };
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::ProfileHistory { uid: uid.clone() }).await {
                Ok(Response::ProfileHistory { versions }) => {
                    let _ = sender.send(ClardEvent::ProfileHistoryReady { uid, versions });
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
        });
    }

    /// 恢复指定历史版本（R2.9）。
    pub fn restore_profile(&mut self, uid: String, version: u32) {
        self.history = None;
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::ProfileRestore { uid, version }).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify("profile restored".to_string()));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_profiles(&sender).await;
        });
    }

    /// 历史视图按键（Enter 恢复 / Esc 关闭 / ↑↓ 移动）。
    pub fn on_history_key(&mut self, event: KeyEvent) {
        if !event.modifiers.is_empty() {
            return;
        }
        match event.code {
            KeyCode::Esc => self.history = None,
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(h) = &mut self.history {
                    h.on_up_key();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(h) = &mut self.history {
                    h.on_down_key();
                }
            }
            KeyCode::Enter => {
                if let Some(h) = &self.history
                    && let Some(v) = h.selected()
                {
                    let (uid, version) = (h.uid.clone(), v.version);
                    self.restore_profile(uid, version);
                }
            }
            _ => {}
        }
    }

    /// 切换当前配置（R2.2，事务）：标记 current → 取回原始 yaml → config_gen 生成
    /// 运行态配置 → ApplyConfig 落盘+热重载 → 核心未运行则启动；任一步失败回滚 current。
    pub fn set_current_profile(&mut self, uid: String) {
        let previous = self.profiles.current.clone();
        self.message = Some(format!("switching to {uid}…"));
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match switch_profile_flow(&uid, previous).await {
                Ok(msg) => {
                    let _ = sender.send(ClardEvent::Notify(msg));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e));
                }
            }
            send_profiles(&sender).await;
        });
    }

    fn open_import_input(&mut self) {
        self.input = Some(InputState::new("Import profile URL", InputPurpose::ImportProfileUrl));
    }

    fn open_delete_confirm(&mut self, uid: String, name: String) {
        self.confirm = Some(ConfirmState::new(
            "Delete profile",
            format!("Delete '{name}'? This cannot be undone."),
            ConfirmPurpose::DeleteProfile { uid },
        ));
    }

    /// 输入弹窗按键处理（doc/03 §4.3：`Ctrl+u` 清行、`Backspace` 删字符、`Enter` 提交、`Esc` 取消）。
    pub fn on_input_key(&mut self, event: KeyEvent) {
        if event.modifiers == KeyModifiers::CONTROL && event.code == KeyCode::Char('u') {
            if let Some(input) = &mut self.input {
                input.clear();
            }
            return;
        }
        if !event.modifiers.is_empty() {
            return;
        }

        let Some(input) = &mut self.input else { return };
        match event.code {
            KeyCode::Esc => self.input = None,
            KeyCode::Enter => {
                let purpose = input.purpose.clone();
                let text = input.text().to_string();
                self.input = None;
                self.submit_input(purpose, text);
            }
            KeyCode::Backspace => input.backspace(),
            KeyCode::Left => input.move_left(),
            KeyCode::Right => input.move_right(),
            KeyCode::Char(c) => input.push_char(c),
            _ => {}
        }
    }

    /// 确认弹窗按键处理（`Enter`=确认、`Esc`=取消）。
    pub fn on_confirm_key(&mut self, event: KeyEvent) {
        if !event.modifiers.is_empty() {
            return;
        }
        let Some(confirm) = &self.confirm else { return };
        match event.code {
            KeyCode::Esc => self.confirm = None,
            KeyCode::Enter => {
                let purpose = confirm.purpose.clone();
                self.confirm = None;
                self.submit_confirm(purpose);
            }
            _ => {}
        }
    }

    fn submit_input(&mut self, purpose: InputPurpose, text: String) {
        match purpose {
            InputPurpose::ImportProfileUrl => {
                let url = text.trim().to_string();
                if !url.is_empty() {
                    self.import_profile(url);
                }
            }
            InputPurpose::FilterConnections => {
                self.connections.set_filter(text.trim().to_string());
            }
            InputPurpose::FilterRules => {
                self.rules.set_filter(text.trim().to_string());
            }
            InputPurpose::RenameProfile { uid } => {
                let name = text.trim().to_string();
                if !name.is_empty() {
                    self.rename_profile(uid, name);
                }
            }
            InputPurpose::FilterProxies => {
                self.proxies.set_filter(text.trim().to_string());
            }
        }
    }

    fn submit_confirm(&mut self, purpose: ConfirmPurpose) {
        match purpose {
            ConfirmPurpose::DeleteProfile { uid } => self.remove_profile(uid),
            ConfirmPurpose::CloseAllConnections => self.close_all_connections(),
        }
    }

    // ---- 规则（Rules）----

    pub fn fetch_rules(&self) {
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match backend.get_rules().await {
                Ok(rules) => {
                    let _ = sender.send(ClardEvent::RulesUpdated(rules.rules));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(format!("Fetch rules error: {e}")));
                }
            }
        });
    }

    pub fn fetch_rule_providers(&self) {
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match backend.get_rule_providers().await {
                Ok(providers) => {
                    let _ = sender.send(ClardEvent::RuleProvidersUpdated(providers.providers));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(format!("Fetch rule providers error: {e}")));
                }
            }
        });
    }

    /// 启用/禁用当前规则（R5.2，`PATCH /rules/disable`，热生效）。
    fn toggle_rule(&mut self) {
        let Some(rule) = self.rules.selected_rule() else {
            return;
        };
        let index = rule.index;
        let new_disabled = !rule.extra.as_ref().is_some_and(|e| e.disabled);
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match backend.disable_rule(index, new_disabled).await {
                Ok(()) => {
                    let state = if new_disabled { "disabled" } else { "enabled" };
                    let _ = sender.send(ClardEvent::Notify(format!("rule {index}: {state}")));
                    if let Ok(rules) = backend.get_rules().await {
                        let _ = sender.send(ClardEvent::RulesUpdated(rules.rules));
                    }
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(format!("Toggle rule error: {e}")));
                }
            }
        });
    }

    /// 更新选中的规则集（R5.4，`PUT /providers/rules/:name`）。
    fn update_rule_provider(&mut self) {
        let Some(name) = self.rules.selected_provider_name() else {
            return;
        };
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match backend.update_rule_provider(&name).await {
                Ok(()) => {
                    let _ = sender.send(ClardEvent::Notify(format!("rule provider updated: {name}")));
                    if let Ok(providers) = backend.get_rule_providers().await {
                        let _ = sender.send(ClardEvent::RuleProvidersUpdated(providers.providers));
                    }
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(format!("Update rule provider error: {e}")));
                }
            }
        });
    }

    fn open_rules_filter(&mut self) {
        let mut input = InputState::new("Filter rules", InputPurpose::FilterRules);
        input.buffer = self.rules.filter.clone();
        input.cursor = input.buffer.len();
        self.input = Some(input);
    }

    fn on_rules_char(&mut self, c: char) {
        match c {
            'f' => self.open_rules_filter(),
            'u' if self.rules.tab == RulesTab::Providers => self.update_rule_provider(),
            _ => {}
        }
    }

    // ---- 代理（Proxies）----

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

    /// 清除当前分组的固定选择，回退 URLTest 自动（R3.2，`DELETE /proxies/:name`）。
    fn clear_group_selection(&mut self) {
        let Some(group_name) = self.proxies.selected_group_name().map(str::to_string) else {
            return;
        };
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        let name = group_name.clone();
        tokio::spawn(async move {
            match backend.unfixed_proxy(&group_name).await {
                Ok(()) => {
                    let _ = sender.send(ClardEvent::Notify(format!("cleared selection: {name}")));
                    if let Ok(groups) = backend.get_groups().await {
                        let _ = sender.send(ClardEvent::UpdateGroups(groups));
                    }
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(format!("Clear selection error: {e}")));
                }
            }
        });
    }

    /// 全部分组测速（R3.3 `T`，`GET /group/:name/delay`）。
    fn test_all_groups(&mut self) {
        let groups: Vec<String> = self.proxies.groups.iter().map(|g| g.name.clone()).collect();
        if groups.is_empty() {
            return;
        }
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            let url = "http://www.gstatic.com/generate_204".to_string();
            let timeout = 5000;
            for group in groups {
                match backend.delay_group_for_name(&group, &url, timeout).await {
                    Ok(map) => {
                        let _ = sender.send(ClardEvent::Notify(format!("{group}: {} nodes tested", map.len())));
                    }
                    Err(e) => {
                        let _ = sender.send(ClardEvent::Error(format!("{group} test failed: {e}")));
                    }
                }
            }
            if let Ok(groups_data) = backend.get_groups().await {
                let _ = sender.send(ClardEvent::UpdateGroups(groups_data));
            }
        });
    }

    fn open_proxies_filter(&mut self) {
        let mut input = InputState::new("Filter nodes", InputPurpose::FilterProxies);
        input.buffer = self.proxies.filter.clone();
        input.cursor = input.buffer.len();
        self.input = Some(input);
    }

    fn on_proxies_char(&mut self, c: char) {
        match c {
            't' if self.proxies.focus == ProxyFocus::Proxies => self.test_proxy_delay(),
            'T' => self.test_all_groups(),
            'd' => self.clear_group_selection(),
            'f' => self.open_proxies_filter(),
            's' => self.proxies.cycle_sort(),
            _ => {}
        }
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

    /// 关闭全部连接（R4.2，二次确认后触发）。
    pub fn close_all_connections(&mut self) {
        let backend = self.backend.clone();
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match backend.close_all_connections().await {
                Ok(()) => {
                    let _ = sender.send(ClardEvent::Notify("all connections closed".to_string()));
                    if let Ok(conns) = backend.get_connections().await {
                        let _ = sender.send(ClardEvent::UpdateConnections(conns));
                    }
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(format!("Close all failed: {e}")));
                }
            }
        });
    }

    fn open_connections_filter(&mut self) {
        let mut input = InputState::new("Filter connections", InputPurpose::FilterConnections);
        input.buffer = self.connections.filter.clone();
        input.cursor = input.buffer.len();
        self.input = Some(input);
    }

    // ---- 键位分派 ----

    pub fn on_up_key(&mut self) {
        match self.current_page {
            Page::Profiles => self.profiles.on_up_key(),
            Page::Proxies => self.proxies.on_up_key(),
            Page::Connections => self.connections.on_up_key(),
            Page::Rules => self.rules.on_up_key(),
            _ => {}
        }
    }

    pub fn on_down_key(&mut self) {
        match self.current_page {
            Page::Profiles => self.profiles.on_down_key(),
            Page::Proxies => self.proxies.on_down_key(),
            Page::Connections => self.connections.on_down_key(),
            Page::Rules => self.rules.on_down_key(),
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

    pub fn on_home_key(&mut self) {}

    pub fn on_end_key(&mut self) {}

    pub fn on_pagedown_key(&mut self) {}

    pub fn on_pageup_key(&mut self) {}

    pub fn on_backspace_key(&mut self) {}

    pub fn on_delete_key(&mut self) {}

    pub fn on_tab_key(&mut self) {
        match self.current_page {
            Page::Proxies => {
                let state = &mut self.proxies;
                if state.focus == ProxyFocus::Groups {
                    state.on_right_key();
                } else {
                    state.on_left_key();
                }
            }
            Page::Rules => self.rules.toggle_tab(),
            _ => {}
        }
    }

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
        match self.current_page {
            Page::Profiles => {
                if let Some(p) = self.profiles.selected() {
                    let uid = p.uid.clone();
                    self.set_current_profile(uid);
                }
            }
            Page::Proxies => self.select_proxy_node(),
            Page::Rules => {
                if self.rules.tab == RulesTab::Rules {
                    self.toggle_rule();
                } else {
                    self.update_rule_provider();
                }
            }
            _ => {}
        }
    }

    fn open_rename_input(&mut self) {
        let Some(p) = self.profiles.selected() else {
            return;
        };
        let mut input = InputState::new(
            "Rename profile",
            InputPurpose::RenameProfile { uid: p.uid.clone() },
        );
        input.buffer = p.name.clone();
        input.cursor = input.buffer.len();
        self.input = Some(input);
    }

    fn on_profiles_char(&mut self, c: char) {
        match c {
            'i' => self.open_import_input(),
            'u' => {
                if let Some(p) = self.profiles.selected() {
                    let uid = p.uid.clone();
                    self.update_profile(uid);
                }
            }
            'd' => {
                if let Some(p) = self.profiles.selected() {
                    let (uid, name) = (p.uid.clone(), p.name.clone());
                    self.open_delete_confirm(uid, name);
                }
            }
            'r' => self.open_rename_input(),
            'h' => self.open_history(),
            '[' => {
                if let Some(p) = self.profiles.selected() {
                    let uid = p.uid.clone();
                    self.move_profile(uid, true);
                }
            }
            ']' => {
                if let Some(p) = self.profiles.selected() {
                    let uid = p.uid.clone();
                    self.move_profile(uid, false);
                }
            }
            _ => {}
        }
    }

    fn on_connections_char(&mut self, c: char) {
        match c {
            'u' => self.connections.sort_by_upload(),
            'd' => self.connections.sort_by_download(),
            'c' => self.connections.toggle_unit(),
            'x' => self.close_selected_connection(),
            'X' => {
                self.confirm = Some(ConfirmState::new(
                    "Close all connections",
                    "Close every active connection?",
                    ConfirmPurpose::CloseAllConnections,
                ));
            }
            'f' => self.open_connections_filter(),
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
            Page::Profiles => self.on_profiles_char(char),
            Page::Proxies => self.on_proxies_char(char),
            Page::Connections => self.on_connections_char(char),
            Page::Rules => self.on_rules_char(char),
            _ => {}
        }
    }
}

/// 拉取配置列表并发回 `ProfilesUpdated`（失败也发回错误消息，保证 UI 反馈）。
async fn send_profiles(sender: &UnboundedSender<ClardEvent>) {
    match rpc::call(&Request::ProfileList).await {
        Ok(Response::ProfileList { current, items }) => {
            let _ = sender.send(ClardEvent::ProfilesUpdated { current, items });
        }
        Ok(other) => {
            let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
        }
        Err(e) => {
            let _ = sender.send(ClardEvent::Error(format!("profile list failed: {e}")));
        }
    }
}

/// 切换配置事务（R2.2）：标记 current → 取回原始 yaml → config_gen → ApplyConfig → 启动核心。
/// 任一步失败回滚 current 到 previous。
async fn switch_profile_flow(uid: &str, previous: Option<String>) -> Result<String, String> {
    rpc::call(&Request::ProfileSetCurrent {
        uid: uid.to_string(),
    })
    .await
    .map_err(|e| format!("标记 current 失败: {e}"))?;

    let result: Result<String, String> = async {
        let yaml = match rpc::call(&Request::ProfileGet {
            uid: uid.to_string(),
        })
        .await
        {
            Ok(Response::ProfileContent { yaml, .. }) => yaml,
            Ok(other) => return Err(rpc::unexpected(other).to_string()),
            Err(e) => return Err(e.to_string()),
        };
        let runtime = config_gen::generate(&yaml, None, &ConfigGenOptions::default())
            .map_err(|e| format!("生成运行态配置失败: {e}"))?;
        rpc::call(&Request::ApplyConfig { yaml: runtime })
            .await
            .map_err(|e| format!("应用配置失败: {e}"))?;
        // 核心未运行则启动
        if let Ok(Response::Status { core_state, .. }) = rpc::call(&Request::Status).await {
            if core_state != "running" {
                rpc::call(&Request::StartCore)
                    .await
                    .map_err(|e| format!("启动核心失败: {e}"))?;
            }
        }
        Ok(format!("switched to {uid}"))
    }
    .await;

    if result.is_err()
        && let Some(prev) = previous
    {
        let _ = rpc::call(&Request::ProfileSetCurrent { uid: prev }).await;
    }
    result
}

/// 下载 → 归一化 → 提交 helper（R2.1/R2.3 共用）。
async fn import_profile_flow(url: String) -> Result<String, String> {
    let (raw, info) = HttpFetcher::new(reqwest::Client::new())
        .fetch_with_info(&url)
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    let yaml = subscription_to_yaml(&raw).map_err(|e| format!("parse failed: {e}"))?;
    let info = clard_proto::SubscriptionInfo {
        upload: info.upload,
        download: info.download,
        total: info.total,
        expire: info.expire,
    };
    let resp = rpc::call(&Request::ProfileImport(ProfileImport {
        name: None,
        url,
        interval: 0,
        yaml,
        info: Some(info),
    }))
    .await
    .map_err(|e| e.to_string())?;
    match resp {
        Response::ProfileImported { uid, updated } => {
            Ok(if updated {
                format!("profile updated: {uid}")
            } else {
                format!("profile imported: {uid}")
            })
        }
        other => Err(rpc::unexpected(other).to_string()),
    }
}

/// 取回 URL/间隔 → 重新下载 → 同 URL 覆盖更新（R2.3）。
async fn update_profile_flow(uid: String) -> Result<String, String> {
    let (url, interval) = match rpc::call(&Request::ProfileGet { uid: uid.clone() }).await {
        Ok(Response::ProfileContent { item, .. }) => (item.url, item.interval),
        Ok(other) => return Err(rpc::unexpected(other).to_string()),
        Err(e) => return Err(e.to_string()),
    };
    let (raw, info) = HttpFetcher::new(reqwest::Client::new())
        .fetch_with_info(&url)
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    let yaml = subscription_to_yaml(&raw).map_err(|e| format!("parse failed: {e}"))?;
    let info = clard_proto::SubscriptionInfo {
        upload: info.upload,
        download: info.download,
        total: info.total,
        expire: info.expire,
    };
    let resp = rpc::call(&Request::ProfileImport(ProfileImport {
        name: None,
        url,
        interval,
        yaml,
        info: Some(info),
    }))
    .await
    .map_err(|e| e.to_string())?;
    match resp {
        Response::ProfileImported { uid, updated } => Ok(if updated {
            format!("profile updated: {uid}")
        } else {
            format!("profile imported: {uid}")
        }),
        other => Err(rpc::unexpected(other).to_string()),
    }
}
