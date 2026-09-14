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

    /// 级别过滤匹配（None = 全部；level 已归一为 warn/debug/info/error）。
    pub fn matches_level(&self, level: Option<&str>) -> bool {
        match level {
            None => true,
            Some(l) => self.level.eq_ignore_ascii_case(l),
        }
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
    /// 核心日志级别过滤（None = 全部；R6.1 `e` 循环切换）。
    pub level_filter: Option<String>,
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
            level_filter: None,
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

    /// `e` 级别过滤循环：全部 → info → warn → error → debug → 全部。
    pub fn cycle_level_filter(&mut self) {
        self.level_filter = match self.level_filter.as_deref() {
            None => Some("info".to_string()),
            Some("info") => Some("warn".to_string()),
            Some("warn") => Some("error".to_string()),
            Some("error") => Some("debug".to_string()),
            _ => None,
        };
    }

    /// 当前级别过滤文案（None = all）。
    pub fn level_filter_label(&self) -> String {
        self.level_filter.clone().unwrap_or_else(|| "all".to_string())
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
            .filter(|l| l.matches(&self.filter) && l.matches_level(self.level_filter.as_deref()))
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

/// 文本行解析：`HH:MM:SS [level] [source] msg` 或 mihomo structured 格式。
/// 简单启发式：前 8 字符为时间戳；级别取 `level=` 或 `[INFO]` 等。
fn parse_text_line(line: String, source: &str) -> LogLine {
    let ts = line
        .get(..8)
        .filter(|s| s.len() == 8 && s.as_bytes()[2] == b':' && s.as_bytes()[5] == b':')
        .map(|_| 0);
    LogLine {
        ts,
        level: extract_level(&line),
        source: source.to_string(),
        message: line,
    }
}

/// 从日志行提取级别（归一化：warning→warn）。
fn extract_level(line: &str) -> String {
    // mihomo structured：`level=info` / `level=warning`
    if let Some(rest) = line.split("level=").nth(1) {
        let v = rest
            .split(|c: char| c.is_whitespace() || c == '"' || c == ',')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if !v.is_empty() {
            return normalize_level(&v);
        }
    }
    // 传统：`[INFO]` / `[WARN]` / `[ERROR]` / `[DEBUG]`
    for lvl in ["DEBUG", "INFO", "WARN", "ERROR"] {
        if line.contains(&format!("[{lvl}]")) {
            return normalize_level(lvl);
        }
    }
    "info".to_string()
}

fn normalize_level(level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "warning" => "warn".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_level_from_structured_and_bracket() {
        assert_eq!(extract_level(r#"time="x" level=warning msg="y""#), "warn");
        assert_eq!(extract_level("level=debug something"), "debug");
        assert_eq!(extract_level("2026/09/14 05:12:47 [ERROR] boom"), "error");
        assert_eq!(extract_level("no level here"), "info");
    }

    #[test]
    fn level_filter_cycles_and_matches() {
        let mut s = LogsState::new();
        assert_eq!(s.level_filter, None);
        s.cycle_level_filter();
        assert_eq!(s.level_filter.as_deref(), Some("info"));
        s.cycle_level_filter();
        assert_eq!(s.level_filter.as_deref(), Some("warn"));
        for _ in 0..2 {
            s.cycle_level_filter();
        }
        assert_eq!(s.level_filter.as_deref(), Some("debug"));
        s.cycle_level_filter();
        assert_eq!(s.level_filter, None, "debug → all");

        let warn = LogLine { ts: None, level: "warn".into(), source: "Core".into(), message: "x".into() };
        let info = LogLine { ts: None, level: "info".into(), source: "Core".into(), message: "y".into() };
        assert!(warn.matches_level(Some("warn")));
        assert!(!info.matches_level(Some("warn")));
        assert!(info.matches_level(None));
    }
}
