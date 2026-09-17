use ratatui::widgets::ListState;
use std::collections::HashMap;

use clard_core::mihomo::models::{Extra, Groups, Proxy as ProxyModel};

#[derive(PartialEq, Eq, Debug)]
pub enum ProxyFocus {
    Groups,
    Proxies,
}

/// 节点排序（R3.5）：None=名称序；延迟缺失排最后/最前。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxySort {
    #[default]
    None,
    NameAsc,
    NameDesc,
    DelayAsc,
    DelayDesc,
}

impl ProxySort {
    fn next(self) -> Self {
        match self {
            Self::None => Self::NameAsc,
            Self::NameAsc => Self::NameDesc,
            Self::NameDesc => Self::DelayAsc,
            Self::DelayAsc => Self::DelayDesc,
            Self::DelayDesc => Self::None,
        }
    }
}

#[derive(Debug)]
pub struct ProxyState {
    pub focus: ProxyFocus,
    /// 全量分组（过滤/排序前的节点列表）。
    raw_groups: Vec<ProxyModel>,
    /// 过滤+排序后的渲染视图。
    pub groups: Vec<ProxyModel>,
    /// 节点索引：节点名 → {alive, history}（取自 `GET /proxies` 各代理自身字段，
    /// 延迟以 `history` 最新一条为准；`/group` 不含节点延迟数据）。
    pub node_extra: HashMap<String, Extra>,
    pub filter: String,
    pub sort: ProxySort,
    pub group_list_state: ListState,
    pub proxy_list_state: ListState,
}

impl ProxyState {
    pub fn new() -> Self {
        Self {
            focus: ProxyFocus::Groups,
            raw_groups: Vec::new(),
            groups: Vec::new(),
            node_extra: HashMap::new(),
            filter: String::new(),
            sort: ProxySort::None,
            group_list_state: ListState::default(),
            proxy_list_state: ListState::default(),
        }
    }

    pub fn update_groups(&mut self, groups_data: Groups) {
        let selected_group_name = self
            .group_list_state
            .selected()
            .and_then(|idx| self.raw_groups.get(idx))
            .map(|g| g.name.clone());
        let selected_proxy_name = self.selected_node().map(|(_, n)| n.to_string());

        // 节点索引：/proxies 返回全部代理（节点+组），节点延迟/存活取自各自 history/alive
        let mut node_extra = HashMap::with_capacity(groups_data.proxies.len());
        for p in &groups_data.proxies {
            node_extra.insert(
                p.name.clone(),
                Extra {
                    alive: p.alive,
                    history: p.history.clone(),
                },
            );
        }
        self.node_extra = node_extra;

        // 过滤掉无节点（GLOBAL/REJECT/DIRECT 等）；分组与节点均按名排序
        let mut raw: Vec<ProxyModel> = groups_data
            .proxies
            .into_iter()
            .filter(|p| p.all.as_ref().is_some_and(|all| !all.is_empty()))
            .collect();
        raw.sort_by_key(|p| p.name.to_ascii_lowercase());
        for group in &mut raw {
            if let Some(all) = &mut group.all {
                all.sort_by_key(|name| name.to_ascii_lowercase());
            }
        }
        self.raw_groups = raw;
        self.refresh_view();

        if self.groups.is_empty() {
            self.group_list_state.select(None);
            self.proxy_list_state.select(None);
            return;
        }

        let group_idx = selected_group_name
            .as_deref()
            .and_then(|name| self.groups.iter().position(|g| g.name == name))
            .unwrap_or(0);
        self.group_list_state.select(Some(group_idx));

        let proxy_idx = self
            .groups
            .get(group_idx)
            .and_then(|group| {
                group.all.as_ref().and_then(|all| {
                    selected_proxy_name
                        .as_deref()
                        .and_then(|name| all.iter().position(|n| n == name))
                        .or_else(|| group_now_idx(group.now.as_deref(), all))
                })
            })
            .or(Some(0));
        self.proxy_list_state.select(proxy_idx);
    }

    /// 设置关键字过滤（R3.5，节点名，忽略大小写）；空串 = 不过滤。
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.refresh_view();
        self.clamp_selection();
    }

    /// 循环切换排序方式（R3.5）。
    pub fn cycle_sort(&mut self) {
        self.sort = self.sort.next();
        self.refresh_view();
        self.clamp_selection();
    }

    pub fn selected_node(&self) -> Option<(&str, &str)> {
        let group_idx = self.group_list_state.selected()?;
        let proxy_idx = self.proxy_list_state.selected()?;
        let group = self.groups.get(group_idx)?;
        let node = group.all.as_ref()?.get(proxy_idx)?;
        Some((&group.name, node))
    }

    /// 当前选中的分组名（清除固定选择 / 全组测速用）。
    pub fn selected_group_name(&self) -> Option<&str> {
        self.group_list_state
            .selected()
            .and_then(|idx| self.groups.get(idx))
            .map(|group| group.name.as_str())
    }

    fn refresh_view(&mut self) {
        self.groups = self.raw_groups.clone();
        let filter = self.filter.to_lowercase();
        // 节点名 → 最新延迟（全表一致，跨组复用；可变借用 all 前预取）
        let delays: Vec<(String, u16)> = self
            .node_extra
            .iter()
            .filter_map(|(n, e)| e.history.last().map(|h| (n.clone(), h.delay)))
            .collect();

        for group in &mut self.groups {
            if let Some(all) = &mut group.all {
                if !filter.is_empty() {
                    all.retain(|n| n.to_lowercase().contains(&filter));
                }
                match self.sort {
                    ProxySort::None => {}
                    ProxySort::NameAsc => all.sort_by_key(|a| a.to_ascii_lowercase()),
                    ProxySort::NameDesc => all.sort_by_key(|a| std::cmp::Reverse(a.to_ascii_lowercase())),
                    ProxySort::DelayAsc => {
                        all.sort_by_key(|n| delay_of(&delays, n).unwrap_or(u16::MAX));
                    }
                    ProxySort::DelayDesc => {
                        all.sort_by_key(|n| std::cmp::Reverse(delay_of(&delays, n).unwrap_or(0)));
                    }
                }
            }
        }
    }

    fn clamp_selection(&mut self) {
        if let Some(gi) = self.group_list_state.selected()
            && let Some(group) = self.groups.get(gi)
            && let Some(all) = &group.all
        {
            if all.is_empty() {
                self.proxy_list_state.select(None);
            } else if let Some(pi) = self.proxy_list_state.selected()
                && pi >= all.len()
            {
                self.proxy_list_state.select(Some(all.len() - 1));
            } else if self.proxy_list_state.selected().is_none() {
                self.proxy_list_state.select(Some(0));
            }
        }
    }

    fn select_default_proxy_for_group(&mut self, group_idx: usize) {
        let selected_idx = self.groups.get(group_idx).and_then(|group| {
            group
                .all
                .as_ref()
                .and_then(|all| group_now_idx(group.now.as_deref(), all).or(Some(0)))
        });
        self.proxy_list_state.select(selected_idx);
    }

    pub fn on_down_key(&mut self, viewport: usize) {
        match self.focus {
            ProxyFocus::Groups => {
                if !self.groups.is_empty() {
                    let next = self.group_list_state.selected().unwrap_or(0);
                    crate::nav::move_list_cursor(&mut self.group_list_state, self.groups.len(), viewport, 1);
                    let i = self.group_list_state.selected().unwrap_or(next);
                    self.select_default_proxy_for_group(i);
                }
            }
            ProxyFocus::Proxies => {
                if let Some(group_idx) = self.group_list_state.selected()
                    && let Some(group) = self.groups.get(group_idx)
                    && let Some(all) = &group.all
                {
                    crate::nav::move_list_cursor(&mut self.proxy_list_state, all.len(), viewport, 1);
                }
            }
        }
    }

    pub fn on_up_key(&mut self, viewport: usize) {
        match self.focus {
            ProxyFocus::Groups => {
                if !self.groups.is_empty() {
                    crate::nav::move_list_cursor(&mut self.group_list_state, self.groups.len(), viewport, -1);
                    let i = self.group_list_state.selected().unwrap_or(0);
                    self.select_default_proxy_for_group(i);
                }
            }
            ProxyFocus::Proxies => {
                if let Some(group_idx) = self.group_list_state.selected()
                    && let Some(group) = self.groups.get(group_idx)
                    && let Some(all) = &group.all
                {
                    crate::nav::move_list_cursor(&mut self.proxy_list_state, all.len(), viewport, -1);
                }
            }
        }
    }

    pub fn on_left_key(&mut self) {
        self.focus = ProxyFocus::Groups;
    }

    pub fn on_right_key(&mut self) {
        self.focus = ProxyFocus::Proxies;
        if self.proxy_list_state.selected().is_none() {
            self.proxy_list_state.select(Some(0));
        }
    }
}

impl Default for ProxyState {
    fn default() -> Self {
        Self::new()
    }
}

fn group_now_idx(now: Option<&str>, all: &[String]) -> Option<usize> {
    now.and_then(|now_name| all.iter().position(|name| name == now_name))
}

fn delay_of(delays: &[(String, u16)], node: &str) -> Option<u16> {
    delays.iter().find(|(n, _)| n == node).map(|(_, d)| *d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clard_core::mihomo::models::{DelayHistory, Extra, ProxyType};
    use std::collections::HashMap;

    fn proxy_group(name: &str, now: Option<&str>, all: Vec<&str>) -> ProxyModel {
        ProxyModel {
            all: Some(all.into_iter().map(ToString::to_string).collect()),
            expected_status: None,
            fixed: None,
            hidden: None,
            icon: None,
            now: now.map(ToString::to_string),
            test_url: None,
            id: None,
            provider_name: None,
            alive: true,
            history: Vec::<DelayHistory>::new(),
            extra: HashMap::<String, Extra>::new(),
            name: name.to_string(),
            udp: true,
            uot: false,
            proxy_type: ProxyType::Selector,
            xudp: false,
            tfo: false,
            mptcp: false,
            smux: false,
            interface: String::new(),
            dialer_proxy: String::new(),
            routing_mark: 0,
        }
    }

    #[test]
    fn update_groups_selects_current_node_by_default() {
        let mut state = ProxyState::new();
        state.update_groups(Groups {
            proxies: vec![proxy_group("group", Some("node-b"), vec!["node-a", "node-b"])],
        });

        assert_eq!(state.selected_node(), Some(("group", "node-b")));
    }

    #[test]
    fn node_extra_indexes_delay_from_node_proxies() {
        let mut state = ProxyState::new();
        let mut node_a = proxy_group("node-a", None, vec![]);
        node_a.history = vec![DelayHistory { time: "t".into(), delay: 120 }];
        let mut node_b = proxy_group("node-b", None, vec![]);
        node_b.history = vec![DelayHistory { time: "t".into(), delay: 0 }];
        state.update_groups(Groups {
            proxies: vec![proxy_group("group", None, vec!["node-a", "node-b"]), node_a, node_b],
        });

        // 节点延迟/存活索引取自各代理自身 history/alive
        assert_eq!(
            state.node_extra["node-a"].history.last().map(|h| h.delay),
            Some(120)
        );
        assert_eq!(
            state.node_extra["node-b"].history.last().map(|h| h.delay),
            Some(0),
            "超时节点 delay==0"
        );
        // 无 all 的节点代理不进入分组列表
        assert_eq!(state.groups.len(), 1);
    }

    #[test]
    fn delay_sort_uses_node_extra() {
        let mut state = ProxyState::new();
        let mut node_a = proxy_group("node-a", None, vec![]);
        node_a.history = vec![DelayHistory { time: "t".into(), delay: 300 }];
        let mut node_b = proxy_group("node-b", None, vec![]);
        node_b.history = vec![DelayHistory { time: "t".into(), delay: 50 }];
        state.update_groups(Groups {
            proxies: vec![
                proxy_group("group", None, vec!["node-a", "node-b"]),
                node_a,
                node_b,
            ],
        });

        state.cycle_sort(); // NameAsc
        state.cycle_sort(); // NameDesc
        state.cycle_sort(); // DelayAsc
        let order: Vec<&str> = state.groups[0]
            .all
            .as_ref()
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(order, vec!["node-b", "node-a"], "按延迟升序：b(50ms) 在 a(300ms) 前");
    }

    #[test]
    fn selected_node_tracks_proxy_focus_selection() {
        let mut state = ProxyState::new();
        state.update_groups(Groups {
            proxies: vec![proxy_group("group", None, vec!["node-a", "node-b"])],
        });

        state.on_right_key();
        state.on_down_key(10);

        assert_eq!(state.selected_node(), Some(("group", "node-b")));
    }

    #[test]
    fn filter_narrows_nodes_and_restores_all() {
        let mut state = ProxyState::new();
        state.update_groups(Groups {
            proxies: vec![proxy_group("group", None, vec!["apple", "banana", "cherry"])],
        });

        state.set_filter("an".to_string());
        let filtered: Vec<&str> = state.groups[0]
            .all
            .as_ref()
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(filtered, vec!["banana"], "只有 banana 含 'an'");
        assert!(state.selected_node().is_some());

        state.set_filter("a".to_string());
        assert_eq!(
            state.groups[0].all.as_ref().map(Vec::len),
            Some(2),
            "apple/banana 含 'a'，cherry 不含"
        );

        state.set_filter(String::new());
        assert_eq!(state.groups[0].all.as_ref().map(Vec::len), Some(3));
    }

    #[test]
    fn cycle_sort_tracks_order() {
        let mut state = ProxyState::new();
        assert_eq!(state.sort, ProxySort::None);
        state.cycle_sort();
        assert_eq!(state.sort, ProxySort::NameAsc);
        state.cycle_sort();
        state.cycle_sort();
        state.cycle_sort();
        assert_eq!(state.sort, ProxySort::DelayDesc);
        state.cycle_sort();
        assert_eq!(state.sort, ProxySort::None, "循环回到 None");
    }
}
