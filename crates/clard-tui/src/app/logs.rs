//! 日志页状态（doc/03 §5.6）：应用 / 核心 / 审计 三类日志查看。
//!
//! 数据经 IPC：`LogTail`（tui/core，字节游标分页）、`AuditQuery`（结构化审计记录）。

use clard_proto::{AuditActor, AuditRecord};
use ratatui::widgets::{ListState, TableState};

/// 审计展示模式（R6.3 `I` 循环切换）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PairMode {
    /// 配对视图：intent+result 合成一行（默认）
    #[default]
    Paired,
    /// 只看 intent 记录
    Intent,
    /// 只看 result 记录
    Result,
}

/// 配对后的审计行（intent+result 合成；缺 result = pending）。
#[derive(Debug, Clone)]
pub struct AuditRow {
    pub op: String,
    pub op_id: String,
    pub ts: i64,
    pub actor: AuditActor,
    pub result: String,
    pub intent: String,
    pub net_before: Option<String>,
    pub net_after: Option<String>,
    pub cfg_sha256: Option<String>,
    pub err: Option<String>,
}

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
    /// 审计按 op 过滤（`o` 输入；空 = 全部）。
    pub op_filter: String,
    /// 审计展示模式（`I` 循环）。
    pub pair_mode: PairMode,
    /// 审计行展开详情（Enter；None = 收起）。
    pub audit_detail: Option<AuditRow>,
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
            op_filter: String::new(),
            pair_mode: PairMode::Paired,
            audit_detail: None,
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

    /// `o` 审计按 op 过滤（支持前缀，如 `tun.`/`core.`）。
    pub fn set_op_filter(&mut self, op: String) {
        self.op_filter = op;
    }

    /// `I` 循环：配对 → intent → result → 配对。
    pub fn cycle_pair_mode(&mut self) {
        self.pair_mode = match self.pair_mode {
            PairMode::Paired => PairMode::Intent,
            PairMode::Intent => PairMode::Result,
            PairMode::Result => PairMode::Paired,
        };
    }

    /// 当前模式文案。
    pub fn pair_mode_label(&self) -> &'static str {
        match self.pair_mode {
            PairMode::Paired => "paired",
            PairMode::Intent => "intent",
            PairMode::Result => "result",
        }
    }

    /// 选中审计行的展开详情（Enter）。
    pub fn toggle_audit_detail(&mut self) {
        if self.audit_detail.is_some() {
            self.audit_detail = None;
            return;
        }
        if let Some(idx) = self.table_state.selected()
            && let Some(row) = self.visible_audit().get(idx)
        {
            self.audit_detail = Some(row.clone());
        }
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

    /// 当前栏渲染用的审计行（已按 op/关键字/配对模式过滤）。
    pub fn visible_audit(&self) -> Vec<AuditRow> {
        let op_matches = |op: &str| {
            self.op_filter.is_empty() || op.starts_with(self.op_filter.trim())
        };
        let needle = self.filter.to_lowercase();
        let kw_matches = |rec: &AuditRecord| {
            needle.is_empty()
                || rec.op.to_lowercase().contains(&needle)
                || rec.intent.to_lowercase().contains(&needle)
                || rec.result.to_lowercase().contains(&needle)
        };
        let recs: Vec<&AuditRecord> = self
            .audit_records
            .iter()
            .filter(|r| op_matches(&r.op) && kw_matches(r))
            .collect();
        match self.pair_mode {
            PairMode::Paired => pair_records(&recs),
            PairMode::Intent => recs
                .iter()
                .filter(|r| r.phase == "intent")
                .map(|r| row_from_rec(r, None))
                .collect(),
            PairMode::Result => recs
                .iter()
                .filter(|r| r.phase == "result")
                .map(|r| row_from_rec(r, None))
                .collect(),
        }
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

/// intent/result 双记录按 op_id 配对成一行（缺 result = pending，缺 intent 用记录自身）。
fn pair_records(recs: &[&AuditRecord]) -> Vec<AuditRow> {
    use std::collections::BTreeMap;
    let mut by_id: BTreeMap<&str, (Option<&AuditRecord>, Option<&AuditRecord>)> = BTreeMap::new();
    for r in recs {
        let e = by_id.entry(r.op_id.as_str()).or_default();
        if r.phase == "intent" {
            e.0 = Some(r);
        } else {
            e.1 = Some(r);
        }
    }
    let mut out: Vec<AuditRow> = by_id
        .values()
        .map(|(intent, result)| {
            let intent = *intent;
            let result = *result;
            row_from_rec(intent.unwrap_or_else(|| result.unwrap()), result)
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.ts));
    out
}

/// 记录 → 展示行。
fn row_from_rec(rec: &AuditRecord, result: Option<&AuditRecord>) -> AuditRow {
    let result = result.unwrap_or(rec);
    AuditRow {
        op: rec.op.clone(),
        op_id: rec.op_id.clone(),
        ts: rec.ts,
        actor: rec.actor.clone(),
        result: result.result.clone(),
        intent: rec.intent.clone(),
        net_before: rec.net.clone(),
        net_after: result.net.clone(),
        cfg_sha256: result.cfg_sha256.clone(),
        err: result.err.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(phase: &str, op_id: &str, op: &str, result: &str) -> AuditRecord {
        AuditRecord {
            ts: 1,
            op: op.into(),
            op_id: op_id.into(),
            phase: phase.into(),
            actor: clard_proto::AuditActor { uid: 0, pid: 1 },
            result: result.into(),
            intent: String::new(),
            net: None,
            cfg_sha256: None,
            err: None,
        }
    }

    #[test]
    fn pair_records_joins_intent_and_result_by_op_id() {
        let all = vec![
            rec("intent", "A", "tun.enable", "pending"),
            rec("result", "A", "tun.enable", "ok"),
            rec("intent", "B", "core.start", "pending"),
        ];
        let recs: Vec<&AuditRecord> = all.iter().collect();
        let rows = pair_records(&recs);
        assert_eq!(rows.len(), 2);
        let a = rows.iter().find(|r| r.op_id == "A").unwrap();
        assert_eq!(a.result, "ok");
        let b = rows.iter().find(|r| r.op_id == "B").unwrap();
        assert_eq!(b.result, "pending", "缺 result 保持 pending");
    }

    #[test]
    fn op_filter_and_pair_modes() {
        let mut s = LogsState::new();
        s.audit_records = vec![
            rec("intent", "A", "tun.enable", "pending"),
            rec("result", "A", "tun.enable", "ok"),
            rec("intent", "B", "core.start", "pending"),
            rec("result", "B", "core.start", "error"),
        ];
        // 默认配对：2 行
        assert_eq!(s.visible_audit().len(), 2);
        // o 按 op 前缀过滤
        s.set_op_filter("tun.".into());
        let rows = s.visible_audit();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].op, "tun.enable");
        // I 循环模式
        s.set_op_filter(String::new());
        s.cycle_pair_mode();
        assert_eq!(s.pair_mode, PairMode::Intent);
        assert_eq!(s.visible_audit().len(), 2, "只看 intent");
        s.cycle_pair_mode();
        assert_eq!(s.visible_audit().len(), 2, "只看 result");
        s.cycle_pair_mode();
        assert_eq!(s.pair_mode, PairMode::Paired);
    }

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
