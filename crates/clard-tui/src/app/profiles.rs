//! 配置页状态（doc/03 §5.2）：订阅配置全生命周期管理。
//!
//! helper 侧已实现 `ProfileList/Import/Get/Remove/SetCurrent`（README「已完成」）；
//! 本模块维护 TUI 侧列表/选中/行内忙碌状态，切换页面后保留（doc/03 §1 原则 3）。

use clard_proto::ProfileItem;
use ratatui::widgets::ListState;

/// 行内操作类型（spinner 展示用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileBusy {
    #[default]
    Idle,
    Importing,
    Updating,
}

/// 配置页状态。
#[derive(Debug)]
pub struct ProfilesState {
    pub items: Vec<ProfileItem>,
    pub current: Option<String>,
    pub list_state: ListState,
    /// 正在操作的 uid（更新中）；导入时因 uid 未知而为 `None`。
    pub busy_uid: Option<String>,
    pub busy: ProfileBusy,
}

impl ProfilesState {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            current: None,
            list_state: ListState::default(),
            busy_uid: None,
            busy: ProfileBusy::Idle,
        }
    }

    /// 当前选中项。
    pub fn selected(&self) -> Option<&ProfileItem> {
        self.list_state.selected().and_then(|i| self.items.get(i))
    }

    /// 刷新列表并尽量保持原选中 uid。
    pub fn update_list(&mut self, current: Option<String>, items: Vec<ProfileItem>) {
        let selected_uid = self.selected().map(|p| p.uid.clone());
        self.items = items;
        self.current = current;

        if self.items.is_empty() {
            self.list_state.select(None);
            return;
        }

        let idx = selected_uid
            .and_then(|uid| self.items.iter().position(|p| p.uid == uid))
            .unwrap_or(0);
        self.list_state.select(Some(idx));
    }

    pub fn on_down_key(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) if i + 1 < self.items.len() => i + 1,
            _ => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn on_up_key(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(0) | None => self.items.len() - 1,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(i));
    }
}

impl Default for ProfilesState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(uid: &str, name: &str) -> ProfileItem {
        ProfileItem {
            uid: uid.to_string(),
            name: name.to_string(),
            url: format!("https://example.com/{uid}"),
            updated_at: None,
            interval: 0,
        }
    }

    #[test]
    fn update_list_keeps_selection_by_uid() {
        let mut state = ProfilesState::new();
        state.update_list(None, vec![item("a", "A"), item("b", "B")]);
        state.on_down_key(); // 选中 b
        assert_eq!(state.selected().unwrap().uid, "b");

        // 刷新后仍选中 b（即使顺序变化）
        state.update_list(None, vec![item("b", "B"), item("a", "A")]);
        assert_eq!(state.selected().unwrap().uid, "b");
    }

    #[test]
    fn update_list_selects_first_when_previous_uid_gone() {
        let mut state = ProfilesState::new();
        state.update_list(None, vec![item("a", "A"), item("b", "B")]);
        state.on_down_key();
        assert_eq!(state.selected().unwrap().uid, "b");

        state.update_list(None, vec![item("a", "A")]);
        assert_eq!(state.selected().unwrap().uid, "a");
    }

    #[test]
    fn empty_list_clears_selection() {
        let mut state = ProfilesState::new();
        state.update_list(None, vec![item("a", "A")]);
        assert!(state.selected().is_some());
        state.update_list(None, vec![]);
        assert!(state.selected().is_none());
    }

    #[test]
    fn up_down_wrap_around() {
        let mut state = ProfilesState::new();
        state.update_list(None, vec![item("a", "A"), item("b", "B"), item("c", "C")]);
        state.on_up_key(); // 从 0 绕到末尾
        assert_eq!(state.selected().unwrap().uid, "c");
        state.on_down_key(); // 从末尾绕到 0
        assert_eq!(state.selected().unwrap().uid, "a");
    }
}
