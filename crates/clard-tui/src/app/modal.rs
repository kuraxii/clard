//! 输入 / 确认弹窗状态（doc/03 §4.3）。
//!
//! - 输入弹窗：单行输入，`Ctrl+u` 清行、`Backspace` 删字符、`Enter` 提交、`Esc` 取消。
//! - 确认弹窗：`Enter`=确认、`Esc`=取消。

/// 输入弹窗状态；`cursor` 为字节偏移（始终保持在 UTF-8 字符边界）。
#[derive(Debug)]
pub struct InputState {
    pub title: String,
    pub buffer: String,
    pub cursor: usize,
    pub purpose: InputPurpose,
}

impl InputState {
    pub fn new(title: impl Into<String>, purpose: InputPurpose) -> Self {
        Self {
            title: title.into(),
            buffer: String::new(),
            cursor: 0,
            purpose,
        }
    }

    /// 在光标处插入一个字符。
    pub fn push_char(&mut self, c: char) {
        let len = c.len_utf8();
        self.buffer.insert(self.cursor, c);
        self.cursor += len;
    }

    /// 删除光标前一个字符。
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self
            .buffer
            .get(..self.cursor)
            .and_then(|s| s.char_indices().next_back())
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.buffer.remove(prev);
        self.cursor = prev;
    }

    /// 光标左移一个字符。
    pub fn move_left(&mut self) {
        self.cursor = self
            .buffer
            .get(..self.cursor)
            .and_then(|s| s.char_indices().next_back())
            .map(|(idx, _)| idx)
            .unwrap_or(0);
    }

    /// 光标右移一个字符。
    pub fn move_right(&mut self) {
        self.cursor = self
            .buffer
            .get(self.cursor..)
            .and_then(|s| s.chars().next())
            .map(|c| self.cursor + c.len_utf8())
            .unwrap_or(self.cursor);
    }

    /// 清空整行。
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }

    /// 批量插入（粘贴）：过滤换行/制表符（URL 粘贴常带 `\n`），在光标处逐字符插入。
    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            if matches!(c, '\r' | '\n' | '\t') {
                continue;
            }
            self.push_char(c);
        }
    }

    /// 提交的文本（与光标解耦）。
    pub fn text(&self) -> &str {
        &self.buffer
    }
}

/// 输入弹窗的提交目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPurpose {
    /// 从 URL 导入订阅（doc/05 §2 R2.1）
    ImportProfileUrl,
    /// 连接页关键字过滤（doc/05 §4 R4.4）
    FilterConnections,
    /// 规则页关键字过滤（doc/05 §5 R5.3）
    FilterRules,
    /// 配置改名（doc/05 §2 R2.5）
    RenameProfile { uid: String },
    /// 代理页节点过滤（doc/05 §3 R3.5）
    FilterProxies,
    /// 日志页关键字过滤（doc/05 §6 R6.1）
    FilterLogs,
    /// 设置：混合端口（doc/05 §7 R7.1）
    EditMixedPort,
    /// 设置：自动更新间隔小时（doc/05 §7 R7.1）
    EditAutoUpdateHours,
    /// 设置：自定义测速 URL（doc/05 §3 R3.4）
    EditTestUrl,
    /// 设置：dns-hijack 列表（逗号分隔，doc/05 §7 R7.2）
    EditDnsHijack,
    /// 设置：route-exclude-address（逗号分隔 CIDR，空 = 默认私网段）
    EditRouteExclude,
    /// 设置：fake-ip-filter 域名列表（逗号分隔，`*.` 通配）
    EditDnsFakeIpFilter,
    /// 设置：hosts 静态映射（逗号分隔 `domain=ip`）
    EditDnsHosts,
    /// 设置：nameserver-policy（逗号分隔 `domain=dns1,dns2`）
    EditDnsNameserverPolicy,
    /// 设置：全局 nameserver（逗号分隔；空 = clard 默认）
    EditDnsNameserver,
    /// 设置：default-nameserver（逗号分隔；空 = clard 默认）
    EditDnsDefaultNameserver,
    /// 审计按 op 过滤（R6.3 `o`）
    FilterAuditOp,
    /// 审计导出路径（R6.3 `x`）
    ExportAudit,
}

/// 确认弹窗状态。
#[derive(Debug)]
pub struct ConfirmState {
    pub title: String,
    pub message: String,
    pub purpose: ConfirmPurpose,
}

impl ConfirmState {
    pub fn new(title: impl Into<String>, message: impl Into<String>, purpose: ConfirmPurpose) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            purpose,
        }
    }
}

/// 确认弹窗的确认目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmPurpose {
    /// 删除配置（doc/05 §2 R2.4）
    DeleteProfile { uid: String },
    /// 关闭全部连接（doc/05 §4 R4.2）
    CloseAllConnections,
    /// 停止核心（doc/05 §7 R7.3）
    StopCore,
    /// 删除备份（doc/05 §8 R8.3）
    DeleteBackup { name: String },
    /// 恢复备份（doc/05 §8 R8.2）
    RestoreBackup { name: String },
    /// TUN 开关（doc/05 §7 R7.2，确认后热重载 + 读回校验）
    SetTun { enable: bool },
    /// 检测到其他 TUN 共存时强开（用户已确认风险，doc/01 §6.2）
    SetTunForce,
    /// 开启 strict-route（二次确认 + 风险提示，doc/01 §6.3）
    EnableStrictRoute,
    /// 开启 auto-redirect（二次确认 + 风险提示，doc/01 §6.3）
    EnableAutoRedirect,
    /// 紧急恢复直连：cleanup-tun（doc/01 §6.4）
    CleanupTun,
    /// 升级核心：从 GitHub 最新 release 下载并重启（R7.3，会短暂中断）
    UpgradeCore,
    /// 更新 geo 数据（geoip/geosite，doc/01 §6.3，会短暂中断）
    UpdateGeoData,
}

/// 选择列表弹窗状态（doc/03 §4.3）：设置页枚举项的可选项列表。
/// `↑`/`↓`（或 `k`/`j`）移动、`Enter` 选择、`Esc` 取消，打开时高亮当前值。
#[derive(Debug)]
pub struct ChoiceListState {
    pub title: String,
    /// 可选项（显示文本即值）。
    pub options: Vec<String>,
    /// 当前选中下标（打开时定位到当前值）。
    pub selected: usize,
    /// 提交目标。
    pub purpose: ChoicePurpose,
}

impl ChoiceListState {
    pub fn new(title: impl Into<String>, options: Vec<String>, current: usize, purpose: ChoicePurpose) -> Self {
        let max = options.len().saturating_sub(1);
        Self {
            title: title.into(),
            options,
            selected: current.min(max),
            purpose,
        }
    }

    pub fn move_up(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.options.len() - 1
        } else {
            self.selected - 1
        };
    }

    pub fn move_down(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.options.len();
    }

    /// 当前选中项（显示文本即提交值）。
    pub fn selected_value(&self) -> Option<&str> {
        self.options.get(self.selected).map(String::as_str)
    }
}

/// 选择列表弹窗的提交目标（设置页枚举项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoicePurpose {
    /// 代理模式：rule / global / direct（doc/05 §7 R7.1）
    SetMode,
    /// 语言：en / zh
    SetLanguage,
    /// 主题：dark / light
    SetTheme,
    /// TUN stack：gvisor / system / mixed（doc/05 §7 R7.2）
    SetTunStack,
    /// TUN 下 DNS 模式：fake-ip / redir-host
    SetTunDnsMode,
    /// fake-ip-filter-mode：blacklist / whitelist / rule（doc/05 R7.2.1）
    SetDnsFilterMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_backspace_edit_buffer() {
        let mut input = InputState::new("t", InputPurpose::ImportProfileUrl);
        input.push_char('h');
        input.push_char('i');
        assert_eq!(input.text(), "hi");
        assert_eq!(input.cursor, 2);

        input.backspace();
        assert_eq!(input.text(), "h");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut input = InputState::new("t", InputPurpose::ImportProfileUrl);
        input.backspace();
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn clear_resets_buffer_and_cursor() {
        let mut input = InputState::new("t", InputPurpose::ImportProfileUrl);
        input.push_char('a');
        input.push_char('b');
        input.clear();
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn insert_str_filters_newlines_and_tabs() {
        let mut input = InputState::new("t", InputPurpose::ImportProfileUrl);
        input.insert_str("https://example.com/sub\n\t\r");
        assert_eq!(input.text(), "https://example.com/sub");
        assert_eq!(input.cursor, "https://example.com/sub".len());
        // 光标不在末尾时插入到光标处
        input.cursor = 8;
        input.insert_str("X");
        assert_eq!(input.text(), "https://Xexample.com/sub");
    }

    #[test]
    fn cursor_moves_across_multibyte_chars_on_char_boundaries() {
        let mut input = InputState::new("t", InputPurpose::ImportProfileUrl);
        for c in "中a文".chars() {
            input.push_char(c);
        }
        // 光标在末尾
        assert_eq!(input.text(), "中a文");

        input.move_left(); // 越过 '文'
        input.move_left(); // 越过 'a'
        assert_eq!(input.cursor, "中".len());
        input.move_left(); // 越过 '中'
        assert_eq!(input.cursor, 0);

        input.move_right();
        assert_eq!(input.cursor, "中".len());
        input.move_right();
        assert_eq!(input.cursor, "中a".len());
    }

    #[test]
    fn backspace_removes_multibyte_char_wholly() {
        let mut input = InputState::new("t", InputPurpose::ImportProfileUrl);
        for c in "中文".chars() {
            input.push_char(c);
        }
        input.backspace(); // 删 '文'
        assert_eq!(input.text(), "中");
        assert_eq!(input.cursor, "中".len());
    }

    #[test]
    fn choice_list_selects_and_wraps() {
        let mut c = ChoiceListState::new(
            "mode",
            vec!["rule".into(), "global".into(), "direct".into()],
            0,
            ChoicePurpose::SetMode,
        );
        assert_eq!(c.selected, 0);
        assert_eq!(c.selected_value(), Some("rule"));
        c.move_down();
        assert_eq!(c.selected_value(), Some("global"));
        c.move_down();
        assert_eq!(c.selected_value(), Some("direct"));
        c.move_down();
        assert_eq!(c.selected_value(), Some("rule"), "down 循环回卷");
        c.move_up();
        assert_eq!(c.selected_value(), Some("direct"), "up 回卷到末尾");
    }

    #[test]
    fn choice_list_clamps_current_and_tolerates_empty() {
        let c = ChoiceListState::new("t", vec!["a".into(), "b".into()], 99, ChoicePurpose::SetTheme);
        assert_eq!(c.selected, 1, "越界 current 收拢到末尾");

        let mut empty = ChoiceListState::new("t", Vec::<String>::new(), 0, ChoicePurpose::SetTheme);
        assert_eq!(empty.selected, 0);
        assert_eq!(empty.selected_value(), None);
        empty.move_down();
        empty.move_up();
        assert_eq!(empty.selected, 0, "空列表移动为 no-op");
    }
}
