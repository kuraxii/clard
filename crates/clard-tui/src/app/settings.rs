//! 设置页状态（doc/03 §5.7）：通用 / 核心 / 后台服务 / 备份 / 关于。
//!
//! TUN 页签（doc/05 R7.2）按计划最后实现（开始前停下确认）。设置值经 IPC
//! `SettingsGet/Set` 读写 helper 侧 `clard.toml`；核心状态经 `Status`；备份经 `Backup*`。

use clard_proto::{BackupItem, Settings};
use ratatui::widgets::{ListState, TableState};

/// 设置页页签（doc/03 §5.7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    /// TUN 与旁路（doc/05 §7 R7.2）
    Tun,
    Core,
    Service,
    Backup,
    /// 日志与审计配置（R7.5：/etc/clard/helper.toml，提示 sudo 编辑）
    Logs,
    About,
}

/// 通用页签的配置行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneralRow {
    MixedPort,
    /// 代理模式：rule（分流）/ global（全代理）/ direct（全直连），循环切换
    Mode,
    AutoUpdateHours,
    Language,
    Theme,
    TestUrl,
}

impl GeneralRow {
    pub const ALL: [Self; 6] = [
        Self::MixedPort,
        Self::Mode,
        Self::AutoUpdateHours,
        Self::Language,
        Self::Theme,
        Self::TestUrl,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::MixedPort => "Mixed port",
            Self::Mode => "Mode",
            Self::AutoUpdateHours => "Auto update (h)",
            Self::Language => "Language",
            Self::Theme => "Theme",
            Self::TestUrl => "Test URL",
        }
    }
}

/// TUN 页签的配置行（doc/05 §7 R7.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunRow {
    TunEnabled,
    TunStack,
    /// TUN 下 DNS 模式：fake-ip / redir-host（循环切换）
    TunDnsMode,
    DnsHijack,
    RouteExclude,
    ExcludeUid,
    ExcludeInterface,
    ExcludeDstPort,
    StrictRoute,
    AutoRedirect,
    /// 紧急恢复直连（CleanupTun，§6.4 手动挂载点）
    RecoverDirect,
}

impl TunRow {
    pub const ALL: [Self; 11] = [
        Self::TunEnabled,
        Self::TunStack,
        Self::TunDnsMode,
        Self::DnsHijack,
        Self::RouteExclude,
        Self::ExcludeUid,
        Self::ExcludeInterface,
        Self::ExcludeDstPort,
        Self::StrictRoute,
        Self::AutoRedirect,
        Self::RecoverDirect,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::TunEnabled => "TUN",
            Self::TunStack => "TUN stack",
            Self::TunDnsMode => "DNS mode",
            Self::DnsHijack => "dns-hijack",
            Self::RouteExclude => "Route exclude",
            Self::ExcludeUid => "exclude-uid",
            Self::ExcludeInterface => "exclude-interface",
            Self::ExcludeDstPort => "exclude-dst-port",
            Self::StrictRoute => "strict-route",
            Self::AutoRedirect => "auto-redirect",
            Self::RecoverDirect => "Recover direct",
        }
    }
}

/// Logs 页签配置行（R7.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogsRow {
    CoreLogLevel,
    AppLogMaxBytes,
    AppLogKeep,
    AuditKeep,
    AuditDualWrite,
}

impl LogsRow {
    pub const ALL: [Self; 5] = [
        Self::CoreLogLevel,
        Self::AppLogMaxBytes,
        Self::AppLogKeep,
        Self::AuditKeep,
        Self::AuditDualWrite,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::CoreLogLevel => "Core log level",
            Self::AppLogMaxBytes => "App log max (MiB)",
            Self::AppLogKeep => "App log keep",
            Self::AuditKeep => "Audit keep",
            Self::AuditDualWrite => "Audit dual-write",
        }
    }

    /// 修改该行对应的 sudo 命令（TUI 不直写 helper.toml，R7.5）。
    pub fn edit_hint(self, _current: &clard_proto::HelperConfig) -> String {
        match self {
            Self::CoreLogLevel => "sudo sed -i 's/log_level = \"info\"/log_level = \"debug\"/' /etc/clard/helper.toml".into(),
            Self::AppLogMaxBytes => "sudo sed -i 's/app_log_max_bytes = [0-9]*/app_log_max_bytes = 2097152/' /etc/clard/helper.toml".into(),
            Self::AppLogKeep => "sudo sed -i 's/app_log_keep = [0-9]*/app_log_keep = 5/' /etc/clard/helper.toml".into(),
            Self::AuditKeep => "sudo sed -i 's/audit_keep = [0-9]*/audit_keep = 5/' /etc/clard/helper.toml".into(),
            Self::AuditDualWrite => "sudo sed -i 's/audit_dual_write = true/audit_dual_write = false/' /etc/clard/helper.toml".into(),
        }
    }
}

/// 设置页状态。
#[derive(Debug)]
pub struct SettingsState {
    pub tab: SettingsTab,
    /// helper 侧设置（clard.toml）。
    pub settings: Option<Settings>,
    pub core_state: Option<String>,
    pub core_pid: Option<u32>,
    pub core_version: Option<String>,
    /// 已安装核心的 sha256（R7.3）
    pub core_sha256: Option<String>,
    pub helper_version: Option<String>,
    /// helper 系统配置（R7.5，/etc/clard/helper.toml）
    pub helper_config: Option<clard_proto::HelperConfig>,
    pub backups: Vec<BackupItem>,
    pub list_state: ListState,
    pub backups_state: TableState,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            tab: SettingsTab::General,
            settings: None,
            core_state: None,
            core_pid: None,
            core_version: None,
            core_sha256: None,
            helper_version: None,
            helper_config: None,
            backups: Vec::new(),
            list_state: ListState::default(),
            backups_state: TableState::default(),
        }
    }

    pub fn apply_settings(&mut self, settings: Settings) {
        self.settings = Some(settings);
    }

    pub fn apply_core_status(
        &mut self,
        state: String,
        pid: Option<u32>,
        version: Option<String>,
        core_sha256: Option<String>,
    ) {
        self.core_state = Some(state);
        self.core_pid = pid;
        self.core_version = version;
        self.core_sha256 = core_sha256;
    }

    pub fn apply_backups(&mut self, backups: Vec<BackupItem>) {
        self.backups = backups;
        if self.backups.is_empty() {
            self.backups_state.select(None);
        } else if self.backups_state.selected().is_none() {
            self.backups_state.select(Some(0));
        } else if let Some(i) = self.backups_state.selected()
            && i >= self.backups.len()
        {
            self.backups_state.select(Some(self.backups.len() - 1));
        }
    }

    pub fn next_tab(&mut self) {
        self.set_tab(match self.tab {
            SettingsTab::General => SettingsTab::Tun,
            SettingsTab::Tun => SettingsTab::Core,
            SettingsTab::Core => SettingsTab::Service,
            SettingsTab::Service => SettingsTab::Backup,
            SettingsTab::Backup => SettingsTab::Logs,
            SettingsTab::Logs => SettingsTab::About,
            SettingsTab::About => SettingsTab::General,
        });
    }

    pub fn prev_tab(&mut self) {
        self.set_tab(match self.tab {
            SettingsTab::General => SettingsTab::About,
            SettingsTab::Tun => SettingsTab::General,
            SettingsTab::Core => SettingsTab::Tun,
            SettingsTab::Service => SettingsTab::Core,
            SettingsTab::Backup => SettingsTab::Service,
            SettingsTab::Logs => SettingsTab::Backup,
            SettingsTab::About => SettingsTab::Logs,
        });
    }

    pub fn set_tab(&mut self, tab: SettingsTab) {
        self.tab = tab;
        self.list_state.select(if matches!(
            tab,
            SettingsTab::General | SettingsTab::Tun | SettingsTab::Logs
        ) {
            Some(0)
        } else {
            None
        });
        self.backups_state.select(None);
        if tab == SettingsTab::Backup && !self.backups.is_empty() {
            self.backups_state.select(Some(0));
        }
    }

    /// 当前 General 配置行。
    pub fn selected_general_row(&self) -> Option<GeneralRow> {
        self.list_state.selected().and_then(|i| GeneralRow::ALL.get(i).copied())
    }

    /// 当前 TUN 配置行。
    pub fn selected_tun_row(&self) -> Option<TunRow> {
        self.list_state.selected().and_then(|i| TunRow::ALL.get(i).copied())
    }

    /// 应用 helper 系统配置（R7.5）。
    pub fn apply_helper_config(&mut self, cfg: clard_proto::HelperConfig) {
        self.helper_config = Some(cfg);
    }

    /// 当前 Logs 配置行。
    pub fn selected_logs_row(&self) -> Option<LogsRow> {
        self.list_state.selected().and_then(|i| LogsRow::ALL.get(i).copied())
    }

    pub fn selected_backup(&self) -> Option<&BackupItem> {
        self.backups_state.selected().and_then(|i| self.backups.get(i))
    }

    pub fn on_down_key(&mut self, viewport: usize) {
        let row_count = match self.tab {
            SettingsTab::General => GeneralRow::ALL.len(),
            SettingsTab::Tun => TunRow::ALL.len(),
            SettingsTab::Logs => LogsRow::ALL.len(),
            _ => 0,
        };
        if row_count > 0 {
            crate::nav::move_list_cursor(&mut self.list_state, row_count, viewport, 1);
        } else if self.tab == SettingsTab::Backup && !self.backups.is_empty() {
            crate::nav::move_list_cursor(&mut self.backups_state, self.backups.len(), viewport, 1);
        }
    }

    pub fn on_up_key(&mut self, viewport: usize) {
        let row_count = match self.tab {
            SettingsTab::General => GeneralRow::ALL.len(),
            SettingsTab::Tun => TunRow::ALL.len(),
            SettingsTab::Logs => LogsRow::ALL.len(),
            _ => 0,
        };
        if row_count > 0 {
            crate::nav::move_list_cursor(&mut self.list_state, row_count, viewport, -1);
        } else if self.tab == SettingsTab::Backup && !self.backups.is_empty() {
            crate::nav::move_list_cursor(&mut self.backups_state, self.backups.len(), viewport, -1);
        }
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_cycle_wraps() {
        let mut s = SettingsState::new();
        assert_eq!(s.tab, SettingsTab::General);
        s.next_tab();
        assert_eq!(s.tab, SettingsTab::Tun);
        for _ in 0..6 {
            s.next_tab();
        }
        assert_eq!(s.tab, SettingsTab::General, "7 次 next 循环回到 General");
        s.prev_tab();
        assert_eq!(s.tab, SettingsTab::About);
    }

    #[test]
    fn tun_row_selection_cycles() {
        let mut s = SettingsState::new();
        s.set_tab(SettingsTab::Tun);
        assert_eq!(s.selected_tun_row(), Some(TunRow::TunEnabled));
        s.on_down_key(10);
        s.on_down_key(10);
        assert_eq!(s.selected_tun_row(), Some(TunRow::TunDnsMode), "TunEnabled→TunStack→TunDnsMode");
        // 11 行循环：TunDnsMode(2) + 9 = 11 ≡ 0（回到 TunEnabled）
        for _ in 0..9 {
            s.on_down_key(10);
        }
        assert_eq!(s.selected_tun_row(), Some(TunRow::TunEnabled), "11 行循环回到 TUN");
    }

    #[test]
    fn general_row_selection_cycles() {
        let mut s = SettingsState::new();
        s.set_tab(SettingsTab::General);
        assert_eq!(s.selected_general_row(), Some(GeneralRow::MixedPort));
        s.on_down_key(10);
        assert_eq!(s.selected_general_row(), Some(GeneralRow::Mode));
        s.on_down_key(10);
        assert_eq!(s.selected_general_row(), Some(GeneralRow::AutoUpdateHours));
        for _ in 0..4 {
            s.on_down_key(10);
        }
        assert_eq!(s.selected_general_row(), Some(GeneralRow::MixedPort), "6 行循环回到 MixedPort");
    }

    #[test]
    fn backups_selection_clamps_on_update() {
        let mut s = SettingsState::new();
        s.set_tab(SettingsTab::Backup);
        let item = |name: &str| BackupItem {
            name: name.into(),
            created_at: 1,
            size: 2,
        };
        s.apply_backups(vec![item("a"), item("b")]);
        assert_eq!(s.selected_backup().map(|b| b.name.as_str()), Some("a"));
        s.on_down_key(10);
        assert_eq!(s.selected_backup().map(|b| b.name.as_str()), Some("b"));
        s.apply_backups(vec![item("a")]);
        assert_eq!(s.selected_backup().map(|b| b.name.as_str()), Some("a"), "下标收拢");
    }
}
