use ratatui::widgets::ListState;

use crate::ipc::models::{Groups, Proxy as ProxyModel};

#[derive(PartialEq, Eq, Debug)]
pub enum ProxyFocus {
    Groups,
    Proxies,
}

#[derive(Debug)]
pub struct ProxyState {
    pub focus: ProxyFocus,
    pub groups: Vec<ProxyModel>,
    pub group_list_state: ListState,
    pub proxy_list_state: ListState,
}

impl ProxyState {
    pub fn new() -> Self {
        Self {
            focus: ProxyFocus::Groups,
            groups: Vec::new(),
            group_list_state: ListState::default(),
            proxy_list_state: ListState::default(),
        }
    }

    pub fn update_groups(&mut self, groups_data: Groups) {
        let selected_group_name = self
            .group_list_state
            .selected()
            .and_then(|idx| self.groups.get(idx))
            .map(|group| group.name.clone());

        let selected_proxy_name = self
            .group_list_state
            .selected()
            .and_then(|g_idx| self.groups.get(g_idx))
            .and_then(|group| {
                self.proxy_list_state
                    .selected()
                    .and_then(|p_idx| group.all.as_ref().and_then(|all| all.get(p_idx).cloned()))
            });

        // filter out GLOBAL or REJECT if needed, but for now just keep all
        // Clash usually returns "GLOBAL" "REJECT" "DIRECT" and actual groups.
        // Usually actual groups have `all` field.
        let mut groups: Vec<ProxyModel> = groups_data
            .proxies
            .into_iter()
            .filter(|p| p.all.as_ref().is_some_and(|all| !all.is_empty()))
            .collect();

        groups.sort_by_key(|p| p.name.to_ascii_lowercase());
        for group in &mut groups {
            if let Some(all) = &mut group.all {
                all.sort_by_key(|name| name.to_ascii_lowercase());
            }
        }

        self.groups = groups;

        if self.groups.is_empty() {
            self.group_list_state.select(None);
            self.proxy_list_state.select(None);
            return;
        }

        let selected_group_idx = selected_group_name
            .and_then(|group_name| self.groups.iter().position(|group| group.name == group_name))
            .unwrap_or(0);
        self.group_list_state.select(Some(selected_group_idx));

        let selected_proxy_idx = self
            .groups
            .get(selected_group_idx)
            .and_then(|group| {
                group.all.as_ref().and_then(|all| {
                    selected_proxy_name
                        .as_ref()
                        .and_then(|proxy_name| all.iter().position(|name| name == proxy_name))
                        .or_else(|| group_now_idx(group.now.as_deref(), all))
                })
            })
            .or(Some(0));

        self.proxy_list_state.select(selected_proxy_idx);

        if self.group_list_state.selected().is_none() && !self.groups.is_empty() {
            self.group_list_state.select(Some(0));
        }
    }

    pub fn selected_node(&self) -> Option<(&str, &str)> {
        let group_idx = self.group_list_state.selected()?;
        let proxy_idx = self.proxy_list_state.selected()?;
        let group = self.groups.get(group_idx)?;
        let node = group.all.as_ref()?.get(proxy_idx)?;

        Some((&group.name, node))
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

    pub fn on_down_key(&mut self) {
        match self.focus {
            ProxyFocus::Groups => {
                if !self.groups.is_empty() {
                    let i = match self.group_list_state.selected() {
                        Some(i) => {
                            if i >= self.groups.len() - 1 {
                                0
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    self.group_list_state.select(Some(i));
                    self.select_default_proxy_for_group(i);
                }
            }
            ProxyFocus::Proxies => {
                if let Some(group_idx) = self.group_list_state.selected() {
                    if let Some(group) = self.groups.get(group_idx) {
                        if let Some(all) = &group.all {
                            if !all.is_empty() {
                                let i = match self.proxy_list_state.selected() {
                                    Some(i) => {
                                        if i >= all.len() - 1 {
                                            0
                                        } else {
                                            i + 1
                                        }
                                    }
                                    None => 0,
                                };
                                self.proxy_list_state.select(Some(i));
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn on_up_key(&mut self) {
        match self.focus {
            ProxyFocus::Groups => {
                if !self.groups.is_empty() {
                    let i = match self.group_list_state.selected() {
                        Some(i) => {
                            if i == 0 {
                                self.groups.len() - 1
                            } else {
                                i - 1
                            }
                        }
                        None => 0,
                    };
                    self.group_list_state.select(Some(i));
                    self.select_default_proxy_for_group(i);
                }
            }
            ProxyFocus::Proxies => {
                if let Some(group_idx) = self.group_list_state.selected() {
                    if let Some(group) = self.groups.get(group_idx) {
                        if let Some(all) = &group.all {
                            if !all.is_empty() {
                                let i = match self.proxy_list_state.selected() {
                                    Some(i) => {
                                        if i == 0 {
                                            all.len() - 1
                                        } else {
                                            i - 1
                                        }
                                    }
                                    None => 0,
                                };
                                self.proxy_list_state.select(Some(i));
                            }
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::models::{DelayHistory, Extra, ProxyType};
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
    fn selected_node_tracks_proxy_focus_selection() {
        let mut state = ProxyState::new();
        state.update_groups(Groups {
            proxies: vec![proxy_group("group", None, vec!["node-a", "node-b"])],
        });

        state.on_right_key();
        state.on_down_key();

        assert_eq!(state.selected_node(), Some(("group", "node-b")));
    }
}
