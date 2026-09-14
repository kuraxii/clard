//! 列表导航辅助：移动选中项（循环）并维护滚动 offset。
//!
//! canvas 渲染是**只读** clone（`render_stateful_widget` 调整的 offset 不写回 state），
//! 若 offset 不在状态层维护，长列表每次渲染都会从 offset=0 重新滚到选中项，
//! 表现为「光标钉在视口底部、直到第一行出现才上移」。故 offset 在状态层维护。

use ratatui::widgets::{ListState, TableState};

/// 光标状态抽象：ListState 与 TableState 共享 offset/selected 语义
/// （ratatui 0.29 的 offset 字段私有，用 `offset_mut`/`select` 公开 API）。
pub trait CursorState {
    fn cur_selected(&self) -> Option<usize>;
    fn cur_offset(&mut self) -> usize;
    fn set_cursor(&mut self, offset: usize, selected: usize);
}

impl CursorState for ListState {
    fn cur_selected(&self) -> Option<usize> { self.selected() }
    fn cur_offset(&mut self) -> usize { *self.offset_mut() }
    fn set_cursor(&mut self, offset: usize, selected: usize) {
        *self.offset_mut() = offset;
        self.select(Some(selected));
    }
}

impl CursorState for TableState {
    fn cur_selected(&self) -> Option<usize> { self.selected() }
    fn cur_offset(&mut self) -> usize { *self.offset_mut() }
    fn set_cursor(&mut self, offset: usize, selected: usize) {
        *self.offset_mut() = offset;
        self.select(Some(selected));
    }
}

/// 移动列表选中项（step = ±1，循环）并同步滚动 offset。
///
/// - 上移越过视口顶 → `offset = selected`（光标停在视口顶）；
/// - 下移越过视口底 → `offset = selected + 1 - viewport`（光标停在视口底）；
/// - 否则 offset 不变（光标在视口内自由移动）。
///
/// `viewport` 为列表可视行数（App 从终端尺寸估算）。**宁小勿大**：估大时
/// offset 滞后、ratatui 渲染会把光标钉回视口底（即原 bug 现象复发）。
pub fn move_list_cursor<S: CursorState>(state: &mut S, len: usize, viewport: usize, step: i32) {
    if len == 0 {
        return;
    }
    let sel = state.cur_selected().unwrap_or(0) as i32;
    let next = ((sel + step).rem_euclid(len as i32)) as usize;
    let offset = state.cur_offset();
    let new_off = if next < offset {
        next
    } else if offset + viewport <= next {
        next + 1 - viewport
    } else {
        offset
    };
    state.set_cursor(new_off, next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_within_viewport_without_scrolling() {
        let mut st = ListState::default();
        st.select(Some(3));
        move_list_cursor(&mut st, 100, 10, 1);
        assert_eq!(st.selected(), Some(4));
        assert_eq!(*st.offset_mut(), 0, "视口内移动不滚动");
        move_list_cursor(&mut st, 100, 10, -1);
        assert_eq!(st.selected(), Some(3));
        assert_eq!(*st.offset_mut(), 0);
    }

    #[test]
    fn scrolls_when_crossing_viewport_bottom() {
        let mut st = ListState::default();
        st.select(Some(8));
        *st.offset_mut() = 0;
        // 8 在视口 [0,10) 内
        move_list_cursor(&mut st, 100, 10, 1); // 9
        assert_eq!(st.selected(), Some(9));
        assert_eq!(*st.offset_mut(), 0);
        move_list_cursor(&mut st, 100, 10, 1); // 10 越过底
        assert_eq!(st.selected(), Some(10));
        assert_eq!(*st.offset_mut(), 1, "下移越过视口底 → offset 跟随");
    }

    #[test]
    fn scrolls_when_crossing_viewport_top() {
        let mut st = ListState::default();
        st.select(Some(3));
        *st.offset_mut() = 5;
        move_list_cursor(&mut st, 100, 10, -1); // 2 < offset 5
        assert_eq!(st.selected(), Some(2));
        assert_eq!(*st.offset_mut(), 2, "上移越过视口顶 → offset 跟随");
    }

    #[test]
    fn wraps_around_ends() {
        let mut st = ListState::default();
        st.select(Some(99));
        *st.offset_mut() = 91;
        move_list_cursor(&mut st, 100, 10, 1); // 100 → 0
        assert_eq!(st.selected(), Some(0));
        assert_eq!(*st.offset_mut(), 0, "回绕到顶部时 offset 归零");
        move_list_cursor(&mut st, 100, 10, -1); // 0 → 99
        assert_eq!(st.selected(), Some(99));
        assert_eq!(*st.offset_mut(), 90, "回绕到底部时 offset 跟随");
    }

    #[test]
    fn empty_list_is_noop() {
        let mut st = ListState::default();
        move_list_cursor(&mut st, 0, 10, 1);
        assert_eq!(st.selected(), None);
    }
}
