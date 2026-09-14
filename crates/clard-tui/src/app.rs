pub mod connections;
pub mod home;
pub mod i18n;
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
use i18n::Lang;
use logs::{LogsState, LogsTab};
use modal::{ConfirmPurpose, ConfirmState, InputPurpose, InputState};
use page::Page;
use profiles::{HistoryView, ProfileBusy, ProfilesState};
use proxy::{ProxyFocus, ProxyState};
use rules::{RulesState, RulesTab};
use settings::{GeneralRow, SettingsState, SettingsTab, TunRow};
use tokio::sync::mpsc::UnboundedSender;

use clard_core::{
    config_gen::{self, subscription_to_yaml, ConfigGenOptions, TunOptions},
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
    pub lang: Lang,
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
            lang: Lang::En,
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
            Page::Logs => {
                self.fetch_logs();
                self.fetch_audit();
            }
            Page::Settings => {
                self.fetch_settings();
                self.fetch_core_status();
                self.fetch_backups();
                self.fetch_helper_config();
            }
            Page::Home => {
                self.fetch_core_status();
                self.fetch_helper_version();
            }
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
        let settings = self.settings.settings.clone().unwrap_or_default();
        let log_level = self
            .settings
            .helper_config
            .as_ref()
            .map(|c| c.log_level.clone())
            .unwrap_or_else(|| "info".to_string());
        let backend = self.backend.clone();
        self.message = Some(format!("switching to {uid}…"));
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match switch_profile_flow(&uid, previous, &settings, &log_level).await {
                Ok(msg) => {
                    // R2.2 记忆节点恢复：新配置的 selected → 逐个 PUT /proxies/:name
                    if let Err(e) = restore_memorized_nodes(&backend, &uid).await {
                        let _ = sender.send(ClardEvent::Error(format!("restore nodes: {e}")));
                    }
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

    /// 粘贴（bracketed paste）：插入当前输入弹窗（过滤换行/制表符）。
    pub fn on_paste(&mut self, text: &str) {
        if let Some(input) = &mut self.input {
            input.insert_str(text);
        }
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
            InputPurpose::FilterLogs => {
                self.logs.set_filter(text.trim().to_string());
            }
            InputPurpose::EditMixedPort => {
                if let Ok(port) = text.trim().parse::<u16>() {
                    let patch = clard_proto::SettingsPatch {
                        mixed_port: Some(port),
                        ..Default::default()
                    };
                    self.set_setting(patch);
                } else {
                    self.message = Some("invalid port".to_string());
                }
            }
            InputPurpose::EditAutoUpdateHours => {
                if let Ok(h) = text.trim().parse::<u64>() {
                    let patch = clard_proto::SettingsPatch {
                        auto_update_interval_hours: Some(h),
                        ..Default::default()
                    };
                    self.set_setting(patch);
                } else {
                    self.message = Some("invalid hours".to_string());
                }
            }
            InputPurpose::EditTestUrl => {
                let patch = clard_proto::SettingsPatch {
                    test_url: Some(text.trim().to_string()),
                    ..Default::default()
                };
                self.set_setting(patch);
            }
            InputPurpose::EditDnsHijack => {
                let list = split_csv(&text);
                let patch = clard_proto::SettingsPatch {
                    dns_hijack: Some(list),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            InputPurpose::EditRouteExclude => {
                let list = split_csv(&text);
                let patch = clard_proto::SettingsPatch {
                    route_exclude_address: Some(list),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            InputPurpose::EditExcludeUid => {
                let list = split_csv_u32(&text);
                let patch = clard_proto::SettingsPatch {
                    exclude_uid: Some(list),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            InputPurpose::EditExcludeInterface => {
                let list = split_csv(&text);
                let patch = clard_proto::SettingsPatch {
                    exclude_interface: Some(list),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            InputPurpose::EditExcludeDstPort => {
                let list = split_csv_u16(&text);
                let patch = clard_proto::SettingsPatch {
                    exclude_dst_port: Some(list),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            InputPurpose::FilterAuditOp => {
                self.logs.set_op_filter(text.trim().to_string());
            }
            InputPurpose::ExportAudit => {
                let rows = self.logs.visible_audit();
                let path = expand_home(text.trim());
                match export_audit(&path, &rows) {
                    Ok(n) => {
                        self.message = Some(format!("exported {n} audit rows to {path}"));
                    }
                    Err(e) => {
                        self.message = Some(format!("export failed: {e}"));
                    }
                }
            }
        }
    }

    fn submit_confirm(&mut self, purpose: ConfirmPurpose) {
        match purpose {
            ConfirmPurpose::DeleteProfile { uid } => self.remove_profile(uid),
            ConfirmPurpose::CloseAllConnections => self.close_all_connections(),
            ConfirmPurpose::StopCore => self.stop_core(),
            ConfirmPurpose::DeleteBackup { name } => self.delete_backup(name),
            ConfirmPurpose::RestoreBackup { name } => self.restore_backup(name),
            ConfirmPurpose::SetTun { enable } => self.toggle_tun(enable),
            ConfirmPurpose::EnableStrictRoute => {
                let patch = clard_proto::SettingsPatch {
                    strict_route: Some(true),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            ConfirmPurpose::EnableAutoRedirect => {
                let patch = clard_proto::SettingsPatch {
                    auto_redirect: Some(true),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            ConfirmPurpose::CleanupTun => self.recover_direct(),
            ConfirmPurpose::UpgradeCore => self.upgrade_core(),
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

    // ---- 设置（Settings）----

    pub fn fetch_settings(&self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(Response::Settings { settings }) = rpc::call(&Request::SettingsGet).await {
                let _ = sender.send(ClardEvent::SettingsReady(settings));
            }
        });
    }

    /// 拉取 helper 系统配置（R7.5：/etc/clard/helper.toml）。
    pub fn fetch_helper_config(&self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(Response::HelperConfig { config }) =
                rpc::call(&Request::HelperConfigGet).await
            {
                let _ = sender.send(ClardEvent::HelperConfigReady(config));
            }
        });
    }

    /// 拉取 helper 版本（`Hello` 握手）。
    pub fn fetch_helper_version(&self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(Response::Hello { helper_version, .. }) = rpc::call(&Request::Hello).await {
                let _ = sender.send(ClardEvent::HelperVersion(helper_version));
            }
        });
    }

    pub fn fetch_core_status(&self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(Response::Status {
                core_state,
                core_pid,
                core_version,
                tun_active,
                core_sha256,
            }) = rpc::call(&Request::Status).await
            {
                let _ = sender.send(ClardEvent::CoreStatusReady {
                    state: core_state,
                    pid: core_pid,
                    version: core_version,
                    tun_active,
                    core_sha256,
                });
            }
        });
    }

    pub fn fetch_backups(&self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(Response::BackupList { backups }) = rpc::call(&Request::BackupList).await {
                let _ = sender.send(ClardEvent::BackupsReady(backups));
            }
        });
    }

    /// 应用 helper 设置（含语言键 → 全局 lang）。
    pub fn apply_settings(&mut self, settings: clard_proto::Settings) {
        self.lang = Lang::parse(&settings.language);
        self.settings.apply_settings(settings);
    }

    /// 提交设置补丁并刷新。
    pub fn set_setting(&mut self, patch: clard_proto::SettingsPatch) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::SettingsSet(patch)).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify("settings saved".to_string()));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_settings(&sender).await;
        });
    }

    /// TUN 开关（R7.2）：经 helper `SetTun`（托管注入 + 热重载 + 读回校验，失败已回退）。
    pub fn toggle_tun(&mut self, enable: bool) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::SetTun { enable }).await {
                Ok(Response::TunSet { hot_reloaded, .. }) => {
                    let msg = if hot_reloaded {
                        if enable {
                            "TUN on (verified)".to_string()
                        } else {
                            "TUN off (verified)".to_string()
                        }
                    } else {
                        "TUN setting saved; core not running, takes effect on start".to_string()
                    };
                    let _ = sender.send(ClardEvent::Notify(msg));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_settings(&sender).await;
            send_core_status(&sender).await;
        });
    }

    /// 紧急恢复直连（§6.4）：幂等清理 TUN 残留（ip rule / route / 网卡）。
    pub fn recover_direct(&mut self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::CleanupTun).await {
                Ok(Response::CleanupResult { clean, residuals }) => {
                    if clean {
                        let _ = sender.send(ClardEvent::Notify(
                            "TUN cleaned: direct connection restored".to_string(),
                        ));
                    } else {
                        let _ = sender.send(ClardEvent::Error(format!(
                            "cleanup partial, residuals: {}",
                            residuals.join(", ")
                        )));
                    }
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_core_status(&sender).await;
        });
    }

    /// TUN 可配字段改动（R7.2）：存 helper → 用最新设置重新应用当前配置（热重载生效）。
    /// 无 current 配置时仅保存。
    pub fn set_tun_setting(&mut self, patch: clard_proto::SettingsPatch) {
        let sender = self.event_sender.clone();
        let uid = self.profiles.current.clone();
        let log_level = self
            .settings
            .helper_config
            .as_ref()
            .map(|c| c.log_level.clone())
            .unwrap_or_else(|| "info".to_string());
        tokio::spawn(async move {
            match rpc::call(&Request::SettingsSet(patch)).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify("settings saved".to_string()));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_settings(&sender).await;
            // 用最新设置重新应用当前配置（使 TUN 字段生效）
            if let Some(uid) = uid {
                if let Ok(Response::Settings { settings }) = rpc::call(&Request::SettingsGet).await {
                    match switch_profile_flow(&uid, None, &settings, &log_level).await {
                        Ok(msg) => {
                            let _ = sender.send(ClardEvent::Notify(msg));
                        }
                        Err(e) => {
                            let _ = sender.send(ClardEvent::Error(e));
                        }
                    }
                }
            }
        });
    }

    pub fn start_core(&mut self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::StartCore).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify("core started".to_string()));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_core_status(&sender).await;
        });
    }

    pub fn stop_core(&mut self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::StopCore).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify("core stopped".to_string()));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_core_status(&sender).await;
        });
    }

    pub fn restart_core(&mut self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::RestartCore).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify("core restarted".to_string()));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_core_status(&sender).await;
        });
    }

    /// 检查更新（R7.3）：对比 GitHub 最新 release 与当前版本。
    pub fn check_update(&mut self) {
        self.message = Some("checking for updates…".to_string());
        let sender = self.event_sender.clone();
        let current = self.settings.core_version.clone();
        tokio::spawn(async move {
            match check_update_flow(current).await {
                Ok(msg) => {
                    let _ = sender.send(ClardEvent::Notify(msg));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e));
                }
            }
        });
    }

    /// 升级核心（R7.3）：自动下载 GitHub 最新 release → inbox → helper 复核+原子替换+重启。
    pub fn upgrade_core(&mut self) {
        self.message = Some("downloading latest core…".to_string());
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match upgrade_core_flow().await {
                Ok(msg) => {
                    let _ = sender.send(ClardEvent::Notify(msg));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e));
                }
            }
            send_core_status(&sender).await;
        });
    }

    pub fn create_backup(&mut self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::BackupCreate { name: None }).await {
                Ok(Response::BackupCreated { item }) => {
                    let _ = sender.send(ClardEvent::Notify(format!("backup created: {}", item.name)));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_backups(&sender).await;
        });
    }

    pub fn delete_backup(&mut self, name: String) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::BackupDelete { name: name.clone() }).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify(format!("backup deleted: {name}")));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_backups(&sender).await;
        });
    }

    pub fn restore_backup(&mut self, name: String) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            match rpc::call(&Request::BackupRestore { name: name.clone() }).await {
                Ok(Response::Ok) => {
                    let _ = sender.send(ClardEvent::Notify(format!("backup restored: {name}")));
                }
                Ok(other) => {
                    let _ = sender.send(ClardEvent::Error(rpc::unexpected(other).to_string()));
                }
                Err(e) => {
                    let _ = sender.send(ClardEvent::Error(e.to_string()));
                }
            }
            send_profiles(&sender).await;
            send_backups(&sender).await;
        });
    }

    /// 测速 URL：设置里配置的自定义 URL，否则用默认。
    fn test_url(&self) -> String {
        self.settings
            .settings
            .as_ref()
            .and_then(|s| (!s.test_url.is_empty()).then_some(s.test_url.clone()))
            .unwrap_or_else(|| "http://www.gstatic.com/generate_204".to_string())
    }

    fn on_settings_char(&mut self, c: char) {
        match self.settings.tab {
            SettingsTab::General => self.on_settings_general_char(c),
            SettingsTab::Tun => self.on_settings_tun_char(c),
            SettingsTab::Core => match c {
                's' => self.start_core(),
                'S' => {
                    self.confirm = Some(ConfirmState::new(
                        "Stop core",
                        "Stop the proxy core?",
                        ConfirmPurpose::StopCore,
                    ));
                }
                'r' => self.restart_core(),
                'c' => self.check_update(),
                'i' => {
                    // 升级 = 从 GitHub 最新 release 下载并重启（会短暂中断，二次确认）
                    self.confirm = Some(ConfirmState::new(
                        "Upgrade core",
                        "Download the latest mihomo from GitHub and restart the core? (brief interruption)",
                        ConfirmPurpose::UpgradeCore,
                    ));
                }
                _ => {}
            },
            SettingsTab::Logs => self.on_settings_logs_char(c),
            SettingsTab::Backup => match c {
                'b' => self.create_backup(),
                'd' => {
                    if let Some(b) = self.settings.selected_backup() {
                        let name = b.name.clone();
                        self.confirm = Some(ConfirmState::new(
                            "Delete backup",
                            format!("Delete backup '{name}'?"),
                            ConfirmPurpose::DeleteBackup { name },
                        ));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// TUN 页签编辑（R7.2）：`e` 编辑当前行。布尔项二次确认，枚举循环，列表项进输入弹窗。
    fn on_settings_tun_char(&mut self, c: char) {
        if c != 'e' {
            return;
        }
        let Some(row) = self.settings.selected_tun_row() else {
            return;
        };
        match row {
            TunRow::TunEnabled => {
                let enable = !self
                    .settings
                    .settings
                    .as_ref()
                    .map(|s| s.tun_enabled)
                    .unwrap_or(false);
                let (title, msg) = if enable {
                    (
                        "Enable TUN",
                        "Enable TUN? Global transparent proxy + DNS hijack (fake-ip).",
                    )
                } else {
                    ("Disable TUN", "Disable TUN? Restores direct connection.")
                };
                self.confirm = Some(ConfirmState::new(title, msg, ConfirmPurpose::SetTun { enable }));
            }
            TunRow::TunStack => {
                let cur = self
                    .settings
                    .settings
                    .as_ref()
                    .map(|s| s.tun_stack.clone())
                    .unwrap_or_else(|| "gvisor".into());
                let next = match cur.as_str() {
                    "system" => "gvisor",
                    "gvisor" => "mixed",
                    _ => "system",
                };
                let patch = clard_proto::SettingsPatch {
                    tun_stack: Some(next.to_string()),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            TunRow::TunDnsMode => {
                let cur = self
                    .settings
                    .settings
                    .as_ref()
                    .map(|s| s.tun_dns_mode.clone())
                    .unwrap_or_else(|| "fake-ip".into());
                let next = if cur.as_str() == "redir-host" {
                    "fake-ip"
                } else {
                    "redir-host"
                };
                let patch = clard_proto::SettingsPatch {
                    tun_dns_mode: Some(next.to_string()),
                    ..Default::default()
                };
                self.set_tun_setting(patch);
            }
            TunRow::DnsHijack => {
                let mut input = InputState::new(
                    "dns-hijack (comma separated; empty = default any:53,tcp://any:53)",
                    InputPurpose::EditDnsHijack,
                );
                if let Some(s) = self.settings.settings.as_ref() {
                    input.buffer = s.dns_hijack.join(",");
                    input.cursor = input.buffer.len();
                }
                self.input = Some(input);
            }
            TunRow::RouteExclude => {
                let mut input = InputState::new(
                    "route-exclude CIDRs (comma separated; empty = private nets)",
                    InputPurpose::EditRouteExclude,
                );
                if let Some(s) = self.settings.settings.as_ref() {
                    input.buffer = s.route_exclude_address.join(",");
                    input.cursor = input.buffer.len();
                }
                self.input = Some(input);
            }
            TunRow::ExcludeUid => {
                let mut input = InputState::new(
                    "exclude-uid (comma separated uids)",
                    InputPurpose::EditExcludeUid,
                );
                if let Some(s) = self.settings.settings.as_ref() {
                    input.buffer = s.exclude_uid.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                    input.cursor = input.buffer.len();
                }
                self.input = Some(input);
            }
            TunRow::ExcludeInterface => {
                let mut input = InputState::new(
                    "exclude-interface (comma separated ifaces)",
                    InputPurpose::EditExcludeInterface,
                );
                if let Some(s) = self.settings.settings.as_ref() {
                    input.buffer = s.exclude_interface.join(",");
                    input.cursor = input.buffer.len();
                }
                self.input = Some(input);
            }
            TunRow::ExcludeDstPort => {
                let mut input = InputState::new(
                    "exclude-dst-port (comma separated ports)",
                    InputPurpose::EditExcludeDstPort,
                );
                if let Some(s) = self.settings.settings.as_ref() {
                    input.buffer =
                        s.exclude_dst_port.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                    input.cursor = input.buffer.len();
                }
                self.input = Some(input);
            }
            TunRow::StrictRoute => {
                let enable = !self
                    .settings
                    .settings
                    .as_ref()
                    .map(|s| s.strict_route)
                    .unwrap_or(false);
                if enable {
                    // 二次确认 + 风险提示（§6.3：残留即全机断网）
                    self.confirm = Some(ConfirmState::new(
                        "Enable strict-route",
                        "strict-route installs unreachable rules; crash residuals = full network outage. Enable?",
                        ConfirmPurpose::EnableStrictRoute,
                    ));
                } else {
                    let patch = clard_proto::SettingsPatch {
                        strict_route: Some(false),
                        ..Default::default()
                    };
                    self.set_tun_setting(patch);
                }
            }
            TunRow::AutoRedirect => {
                let enable = !self
                    .settings
                    .settings
                    .as_ref()
                    .map(|s| s.auto_redirect)
                    .unwrap_or(false);
                if enable {
                    self.confirm = Some(ConfirmState::new(
                        "Enable auto-redirect",
                        "auto-redirect installs nftables/iptables rules; residuals persist after crash. Enable?",
                        ConfirmPurpose::EnableAutoRedirect,
                    ));
                } else {
                    let patch = clard_proto::SettingsPatch {
                        auto_redirect: Some(false),
                        ..Default::default()
                    };
                    self.set_tun_setting(patch);
                }
            }
            TunRow::RecoverDirect => {
                self.confirm = Some(ConfirmState::new(
                    "Recover direct",
                    "Run cleanup-tun? Removes ip rules / routes / clard0 residuals (idempotent).",
                    ConfirmPurpose::CleanupTun,
                ));
            }
        }
    }

    /// Logs 页签：`e`/Enter 显示该行的 sudo 编辑命令（R7.5，TUI 不直写 helper.toml）。
    fn on_settings_logs_char(&mut self, c: char) {
        if c != 'e' {
            return;
        }
        let Some(row) = self.settings.selected_logs_row() else {
            return;
        };
        let Some(cfg) = self.settings.helper_config.clone() else {
            return;
        };
        self.message = Some(row.edit_hint(&cfg));
    }

    fn on_settings_general_char(&mut self, c: char) {
        if c != 'e' {
            return;
        }
        let Some(row) = self.settings.selected_general_row() else {
            return;
        };
        match row {
            GeneralRow::MixedPort => {
                self.input = Some(InputState::new("Mixed port (1-65535)", InputPurpose::EditMixedPort));
            }
            GeneralRow::AutoUpdateHours => {
                self.input = Some(InputState::new(
                    "Auto update interval (hours, 0=off)",
                    InputPurpose::EditAutoUpdateHours,
                ));
            }
            GeneralRow::Language => {
                let next = self.lang.toggle();
                let patch = clard_proto::SettingsPatch {
                    language: Some(next.as_str().to_string()),
                    ..Default::default()
                };
                self.set_setting(patch);
            }
            GeneralRow::Theme => {
                let cur = self
                    .settings
                    .settings
                    .as_ref()
                    .map(|s| s.theme.clone())
                    .unwrap_or_else(|| "dark".into());
                let next = if cur == "dark" { "light" } else { "dark" };
                let patch = clard_proto::SettingsPatch {
                    theme: Some(next.to_string()),
                    ..Default::default()
                };
                self.set_setting(patch);
            }
            GeneralRow::TestUrl => {
                let mut input = InputState::new("Test URL (empty = default)", InputPurpose::EditTestUrl);
                if let Some(s) = self.settings.settings.as_ref() {
                    input.buffer = s.test_url.clone();
                    input.cursor = input.buffer.len();
                }
                self.input = Some(input);
            }
        }
    }

    fn on_settings_enter(&mut self) {
        match self.settings.tab {
            SettingsTab::Tun => self.on_settings_tun_char('e'),
            SettingsTab::Backup => {
                if let Some(b) = self.settings.selected_backup() {
                    let name = b.name.clone();
                    self.confirm = Some(ConfirmState::new(
                        "Restore backup",
                        format!("Restore '{name}'? This overwrites current profiles and settings."),
                        ConfirmPurpose::RestoreBackup { name },
                    ));
                }
            }
            SettingsTab::Logs => self.on_settings_logs_char('e'),
            _ => {}
        }
    }

    // ---- 日志（Logs）----

    /// 拉取应用/核心日志（每次从 0 重读，语义为刷新）。
    pub fn fetch_logs(&self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            for source in ["tui", "core"] {
                if let Ok(Response::LogTail { cursor, lines }) = rpc::call(&Request::LogTail {
                    source: source.to_string(),
                    cursor: 0,
                })
                .await
                {
                    let _ = sender.send(ClardEvent::LogLinesReady {
                        source: source.to_string(),
                        cursor,
                        lines,
                    });
                }
            }
        });
    }

    /// 拉取审计日志（每次从 0 重读）。
    pub fn fetch_audit(&self) {
        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            if let Ok(Response::AuditQuery { cursor, records }) =
                rpc::call(&Request::AuditQuery { cursor: 0 }).await
            {
                let _ = sender.send(ClardEvent::AuditRecordsReady { cursor, records });
            }
        });
    }

    /// TUI 应用日志交 helper 落盘（R6.2，/var/clard/log/tui.log）。
    pub fn submit_app_log(&self, msg: String) {
        tokio::spawn(async move {
            let _ = rpc::call(&Request::LogSubmit { line: msg }).await;
        });
    }

    fn open_logs_filter(&mut self) {
        let mut input = InputState::new("Filter logs", InputPurpose::FilterLogs);
        input.buffer = self.logs.filter.clone();
        input.cursor = input.buffer.len();
        self.input = Some(input);
    }

    fn on_logs_char(&mut self, c: char) {
        match c {
            'f' => self.open_logs_filter(),
            // R6.1：核心日志级别过滤（全部 → info → warn → error → debug → 全部）
            'e' => self.logs.cycle_level_filter(),
            // R6.3：审计按 op 过滤
            'o' => {
                let mut input = InputState::new(
                    "Audit op filter (prefix, e.g. tun./core./backup.*)",
                    InputPurpose::FilterAuditOp,
                );
                input.buffer = self.logs.op_filter.clone();
                input.cursor = input.buffer.len();
                self.input = Some(input);
            }
            // R6.3：intent/result 配对切换
            'I' => self.logs.cycle_pair_mode(),
            // R6.3：导出当前过滤后的审计
            'x' => {
                let mut input = InputState::new(
                    "Export audit to file (default ~/clard-audit.json)",
                    InputPurpose::ExportAudit,
                );
                input.buffer = "~/clard-audit.json".to_string();
                input.cursor = input.buffer.len();
                self.input = Some(input);
            }
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

            // 记忆当前配置的组选择（R2.2：切换配置后恢复）
            let _ = rpc::call(&Request::ProfileMemorize {
                group: group_name.clone(),
                node: node.clone(),
            })
            .await;

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
        let url = self.test_url();
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
        let url = self.test_url();
        tokio::spawn(async move {
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
            Page::Logs => self.logs.on_up_key(),
            Page::Settings => self.settings.on_up_key(),
            _ => {}
        }
    }

    pub fn on_down_key(&mut self) {
        match self.current_page {
            Page::Profiles => self.profiles.on_down_key(),
            Page::Proxies => self.proxies.on_down_key(),
            Page::Connections => self.connections.on_down_key(),
            Page::Rules => self.rules.on_down_key(),
            Page::Logs => self.logs.on_down_key(),
            Page::Settings => self.settings.on_down_key(),
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
            Page::Logs => self.logs.next_tab(),
            Page::Settings => self.settings.next_tab(),
            _ => {}
        }
    }

    pub fn on_esc_key(&mut self) {
        if self.show_help {
            self.show_help = false;
            return;
        }
        // 审计展开详情：Esc 收起（R6.3）
        if self.logs.audit_detail.is_some() {
            self.logs.audit_detail = None;
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
            Page::Settings => self.on_settings_enter(),
            Page::Rules => {
                if self.rules.tab == RulesTab::Rules {
                    self.toggle_rule();
                } else {
                    self.update_rule_provider();
                }
            }
            // R6.3：审计行展开详情（Enter 切换展开/收起）
            Page::Logs if self.logs.tab == LogsTab::Audit => {
                self.logs.toggle_audit_detail();
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
            Page::Logs => self.on_logs_char(char),
            Page::Settings => self.on_settings_char(char),
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

async fn send_core_status(sender: &UnboundedSender<ClardEvent>) {
    if let Ok(Response::Status {
        core_state,
        core_pid,
        core_version,
        tun_active,
        core_sha256,
    }) = rpc::call(&Request::Status).await
    {
        let _ = sender.send(ClardEvent::CoreStatusReady {
            state: core_state,
            pid: core_pid,
            version: core_version,
            tun_active,
            core_sha256,
        });
    }
}

async fn send_settings(sender: &UnboundedSender<ClardEvent>) {
    if let Ok(Response::Settings { settings }) = rpc::call(&Request::SettingsGet).await {
        let _ = sender.send(ClardEvent::SettingsReady(settings));
    }
}

async fn send_backups(sender: &UnboundedSender<ClardEvent>) {
    if let Ok(Response::BackupList { backups }) = rpc::call(&Request::BackupList).await {
        let _ = sender.send(ClardEvent::BackupsReady(backups));
    }
}

/// 切换配置事务（R2.2）：标记 current → 取回原始 yaml → config_gen（注入 TUN 设置）→
/// ApplyConfig → 启动核心。任一步失败回滚 current 到 previous。
async fn switch_profile_flow(
    uid: &str,
    previous: Option<String>,
    settings: &clard_proto::Settings,
    log_level: &str,
) -> Result<String, String> {
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
        let runtime = config_gen::generate(&yaml, None, &config_options(settings, log_level))
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

/// 从系统设置构造 config_gen 托管选项（doc/01 §6.3）：TUN 开关 + 可配字段 + 混合端口。
/// 空列表语义 = 用默认值（与 helper tun::build_tun_block 的约定一致）。
fn config_options(settings: &clard_proto::Settings, log_level: &str) -> ConfigGenOptions {
    let mut base = ConfigGenOptions::default();
    base.mixed_port = settings.mixed_port;
    base.log_level = if log_level.trim().is_empty() {
        "info".to_string()
    } else {
        log_level.trim().to_string()
    };
    base.tun = if settings.tun_enabled {
        let default_tun = TunOptions::default();
        Some(TunOptions {
            stack: if settings.tun_stack.is_empty() {
                default_tun.stack
            } else {
                settings.tun_stack.clone()
            },
            dns_hijack: if settings.dns_hijack.is_empty() {
                default_tun.dns_hijack
            } else {
                settings.dns_hijack.clone()
            },
            dns_mode: if settings.tun_dns_mode.is_empty() {
                default_tun.dns_mode
            } else {
                settings.tun_dns_mode.clone()
            },
            route_exclude_address: if settings.route_exclude_address.is_empty() {
                default_tun.route_exclude_address
            } else {
                settings.route_exclude_address.clone()
            },
            exclude_uid: settings.exclude_uid.clone(),
            exclude_interface: settings.exclude_interface.clone(),
            exclude_dst_port: settings.exclude_dst_port.clone(),
            strict_route: settings.strict_route,
            auto_redirect: settings.auto_redirect,
            ..default_tun
        })
    } else {
        None
    };
    base
}

/// 逗号分隔字符串 → 非空条目列表（trim + 去空，R7.2 列表项输入）。
fn split_csv(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// 审计导出（R6.3 `x`）：当前过滤/配对后的行 → JSON lines 文件。
fn export_audit(path: &str, rows: &[logs::AuditRow]) -> std::io::Result<usize> {
    use std::io::Write;
    let path = expand_home(path);
    let mut f = std::fs::File::create(&path)?;
    for r in rows {
        let line = serde_json::json!({
            "op": r.op, "op_id": r.op_id, "ts": r.ts,
            "actor": {"uid": r.actor.uid, "pid": r.actor.pid},
            "result": r.result, "intent": r.intent,
            "net_before": r.net_before, "net_after": r.net_after,
            "cfg_sha256": r.cfg_sha256, "err": r.err,
        });
        writeln!(f, "{line}")?;
    }
    Ok(rows.len())
}

/// `~/` 展开为 HOME（避免为此引入 shellexpand 运行依赖）。
fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{}", home.to_string_lossy(), rest);
        }
    }
    path.to_string()
}

/// 逗号分隔数字 → u32 列表（忽略非法项）。
fn split_csv_u32(text: &str) -> Vec<u32> {
    split_csv(text).iter().filter_map(|s| s.parse().ok()).collect()
}

/// 逗号分隔数字 → u16 列表（忽略非法项）。
fn split_csv_u16(text: &str) -> Vec<u16> {
    split_csv(text).iter().filter_map(|s| s.parse().ok()).collect()
}

/// 检查更新（R7.3）：拉取 GitHub 最新 release tag，对比当前版本（忽略前导 v）。
async fn check_update_flow(current: Option<String>) -> Result<String, String> {
    let client = reqwest::Client::new();
    let info = clard_core::upgrade::fetch_latest_release(&client, clard_core::upgrade::GITHUB_API_URL)
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?;
    let cur = current.unwrap_or_default().trim_start_matches('v').to_string();
    let latest = info.tag.trim_start_matches('v');
    if !cur.is_empty() && cur == latest {
        Ok(format!("core is up to date (v{latest})"))
    } else if cur.is_empty() {
        Ok(format!("latest core: v{latest}  (press i to install)"))
    } else {
        Ok(format!("update available: v{cur} → v{latest}  (press i to upgrade)"))
    }
}

/// 升级核心（R7.3）：自动从 GitHub 最新 release 下载（自算 sha256 供 helper 复核）→
/// 写 inbox → IPC InstallCore（helper 复核 + 原子替换 + 重启）。失败无副作用（inbox 由 helper 清理）。
async fn upgrade_core_flow() -> Result<String, String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let client = reqwest::Client::new();
    let (bytes, info, sha) = clard_core::upgrade::download_latest(&client, clard_core::upgrade::GITHUB_API_URL)
        .await
        .map_err(|e| format!("下载/校验失败: {e}"))?;

    // 写 inbox（/run/clard/inbox 0733+sticky，任意本地用户可写；doc/01 §4）
    let dir = "/run/clard/inbox";
    std::fs::create_dir_all(dir).map_err(|e| format!("创建 inbox 失败: {e}"))?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let inbox_path = format!("{dir}/clard-core-{}-{nanos}.bin", std::process::id());
    std::fs::write(&inbox_path, &bytes).map_err(|e| format!("写入 inbox 失败: {e}"))?;

    match rpc::call(&Request::InstallCore {
        inbox_path,
        sha256: sha,
        version: info.tag.clone(),
    })
    .await
    {
        Ok(Response::Ok) => Ok(format!("core upgraded to {} & restarted", info.tag)),
        Ok(other) => Err(rpc::unexpected(other).to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// 恢复指定配置的记忆节点（R2.2）：读 ProfileGet.selected → 逐个 `PUT /proxies/:name`。
/// 核心未运行/节点不存在时跳过（best-effort，失败不阻断切换）。
async fn restore_memorized_nodes(
    backend: &Backend,
    uid: &str,
) -> Result<(), String> {
    let Response::ProfileContent { item, .. } = rpc::call(&Request::ProfileGet {
        uid: uid.to_string(),
    })
    .await
    .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    for sel in &item.selected {
        let _ = backend
            .select_node_for_group(&sel.group, &sel.node)
            .await;
    }
    Ok(())
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
