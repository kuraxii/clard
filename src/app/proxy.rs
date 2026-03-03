use ratatui::widgets::ListState;
use crate::ipc::models::{Proxy as ProxyModel, Groups};

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
        // filter out GLOBAL or REJECT if needed, but for now just keep all
        // Clash usually returns "GLOBAL" "REJECT" "DIRECT" and actual groups.
        // Usually actual groups have `all` field.
        self.groups = groups_data.proxies.into_iter()
            .filter(|p| p.all.is_some() && !p.all.as_ref().unwrap().is_empty())
            .collect();
            
        if self.group_list_state.selected().is_none() && !self.groups.is_empty() {
            self.group_list_state.select(Some(0));
        }
    }

    pub fn on_down_key(&mut self) {
        match self.focus {
            ProxyFocus::Groups => {
                if !self.groups.is_empty() {
                    let i = match self.group_list_state.selected() {
                        Some(i) => {
                            if i >= self.groups.len() - 1 { 0 } else { i + 1 }
                        }
                        None => 0,
                    };
                    self.group_list_state.select(Some(i));
                    self.proxy_list_state.select(Some(0)); // reset proxy selection
                }
            }
            ProxyFocus::Proxies => {
                if let Some(group_idx) = self.group_list_state.selected() {
                    if let Some(group) = self.groups.get(group_idx) {
                        if let Some(all) = &group.all {
                            if !all.is_empty() {
                                let i = match self.proxy_list_state.selected() {
                                    Some(i) => {
                                        if i >= all.len() - 1 { 0 } else { i + 1 }
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
                            if i == 0 { self.groups.len() - 1 } else { i - 1 }
                        }
                        None => 0,
                    };
                    self.group_list_state.select(Some(i));
                    self.proxy_list_state.select(Some(0)); // reset proxy selection
                }
            }
            ProxyFocus::Proxies => {
                if let Some(group_idx) = self.group_list_state.selected() {
                    if let Some(group) = self.groups.get(group_idx) {
                        if let Some(all) = &group.all {
                            if !all.is_empty() {
                                let i = match self.proxy_list_state.selected() {
                                    Some(i) => {
                                        if i == 0 { all.len() - 1 } else { i - 1 }
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
