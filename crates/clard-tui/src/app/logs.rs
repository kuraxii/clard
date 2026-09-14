//! 日志页状态（doc/03 §5.6）：应用 / 核心 / 审计 三类日志查看。
//!
//! 数据经 IPC：`LogTail`（tui/core，字节游标分页）、`AuditQuery`（结构化审计记录）。

use clard_proto::AuditRecord;
use ratatui::widgets::{ListState, TableState};

/// 日志页签。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogsTab {
    #[default]
    App,
    Core,
    Audit,
}

/// 日志行（统一展示格式：时间戳 [级别] [来源] 消息）。
#[derive(Debug, Clone)]
pub struct LogLine {
    pub ts: Option<i64>,
    pub level: String,
    pub source: String,
    pub message: String,
}

impl LogLine {
    pub fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_lowercase();
        self.message.to_lowercase().contains(&needle)
            || self.source.to_lowercase().contains(&needle)
            || self.level.to_lowercase().contains(&needle)
    }
}

/// 日志页状态。
#[derive(Debug)]
pub struct LogsState {
    pub tab: LogsTab,
    /// 各栏原始行（app/core 为文本行，audit 为记录）。
    pub app_lines: Vec<LogLine>,
    pub core_lines: Vec<LogLine>,
    pub audit_records: Vec<AuditRecord>,
    /// 各栏读取游标。
    app_cursor: u64,
    core_cursor: u64,
    audit_cursor: u64,
    /// 关键字过滤（本地）。
    pub filter: String,
    /// 行视图（App/Core）选中。
    pub lines_state: ListState,
    /// 审计表选中。
    pub table_state: TableState,
}

impl LogsState {
    pub fn new() -> Self {
        Self {
            tab: LogsTab::App,
            app_lines: Vec::new(),
            core_lines: Vec::new(),
            audit_records: Vec::new(),
            app_cursor: 0,
            core_cursor: 0,
            audit_cursor: 0,
            filter: String::new(),
            lines_state: ListState::default(),
            table_state: TableState::default(),
        }
    }

    pub fn set_tab(&mut self, tab: LogsTab) {
        self.tab = tab;
        self.lines_state.select(None);
        self.table_state.select(None);
    }

    pub fn next_tab(&mut self) {
        self.set_tab(match self.tab {
            LogsTab::App => LogsTab::Core,
            LogsTab::Core => LogsTab::Audit,
            LogsTab::Audit => LogsTab::App,
        });
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
    }

    /// 全量替换应用日志（每次从游标 0 重读，语义为「刷新」）。
    pub fn set_app(&mut self, cursor: u64, lines: Vec<String>) {
        self.app_cursor = cursor;
        self.app_lines = lines.into_iter().map(|l| parse_text_line(l, "App")).collect();
    }

    pub fn set_core(&mut self, cursor: u64, lines: Vec<String>) {
        self.core_cursor = cursor;
        self.core_lines = lines.into_iter().map(|l| parse_text_line(l, "Core")).collect();
    }

    pub fn set_audit(&mut self, cursor: u64, records: Vec<AuditRecord>) {
        self.audit_cursor = cursor;
        self.audit_records = records;
    }

    /// 当前栏渲染用的行（已过滤）。
    pub fn visible_lines(&self) -> Vec<&LogLine> {
        let lines: Vec<&LogLine> = match self.tab {
            LogsTab::App => self.app_lines.iter().collect(),
            LogsTab::Core => self.core_lines.iter().collect(),
            LogsTab::Audit => Vec::new(),
        };
        lines
            .into_iter()
            .filter(|l| l.matches(&self.filter))
            .collect()
    }

    /// 当前栏渲染用的审计记录（已按 op/关键字过滤）。
    pub fn visible_audit(&self) -> Vec<&AuditRecord> {
        let needle = self.filter.to_lowercase();
        self.audit_records
            .iter()
            .filter(|r| needle.is_empty() || r.op.to_lowercase().contains(&needle))
            .collect()
    }

    pub fn on_down_key(&mut self) {
        let len = match self.tab {
            LogsTab::Audit => self.visible_audit().len(),
            _ => self.visible_lines().len(),
        };
        if len == 0 {
            return;
        }
        if self.tab == LogsTab::Audit {
            let i = match self.table_state.selected() {
                Some(i) if i + 1 < len => i + 1,
                _ => 0,
            };
            self.table_state.select(Some(i));
        } else {
            let i = match self.lines_state.selected() {
                Some(i) if i + 1 < len => i + 1,
                _ => 0,
            };
            self.lines_state.select(Some(i));
        }
    }

    pub fn on_up_key(&mut self) {
        let len = match self.tab {
            LogsTab::Audit => self.visible_audit().len(),
            _ => self.visible_lines().len(),
        };
        if len == 0 {
            return;
        }
        if self.tab == LogsTab::Audit {
            let i = match self.table_state.selected() {
                Some(0) | None => len - 1,
                Some(i) => i - 1,
            };
            self.table_state.select(Some(i));
        } else {
            let i = match self.lines_state.selected() {
                Some(0) | None => len - 1,
                Some(i) => i - 1,
            };
            self.lines_state.select(Some(i));
        }
    }
}

impl Default for LogsState {
    fn default() -> Self {
        Self::new()
    }
}

/// 文本行解析：`HH:MM:SS [level] [source] msg` 或原样。简单启发式：前 8 字符为时间戳。
fn parse_text_line(line: String, source: &str) -> LogLine {
    let ts = line
        .get(..8)
        .filter(|s| s.len() == 8 && s.as_bytes()[2] == b':' && s.as_bytes()[5] == b':')
        .map(|_| 0);
    LogLine {
        ts,
        level: "info".to_string(),
        source: source.to_string(),
        message: line,
    }
}
