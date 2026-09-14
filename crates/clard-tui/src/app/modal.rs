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
    /// 设置：exclude-uid（逗号分隔数字）
    EditExcludeUid,
    /// 设置：exclude-interface（逗号分隔网卡名）
    EditExcludeInterface,
    /// 设置：exclude-dst-port（逗号分隔端口）
    EditExcludeDstPort,
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
    /// 开启 strict-route（二次确认 + 风险提示，doc/01 §6.3）
    EnableStrictRoute,
    /// 开启 auto-redirect（二次确认 + 风险提示，doc/01 §6.3）
    EnableAutoRedirect,
    /// 紧急恢复直连：cleanup-tun（doc/01 §6.4）
    CleanupTun,
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
}
