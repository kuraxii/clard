use ratatui::widgets::TableState;

use crate::ipc::models::Rule;

#[derive(Debug)]
pub struct RulesState {
    pub rules: Vec<Rule>,
    pub list_state: TableState,
}

impl RulesState {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            list_state: TableState::default(),
        }
    }

    pub fn update_rules(&mut self, rules: Vec<Rule>) {
        self.rules = rules;
        if self.rules.is_empty() {
            self.list_state.select(None);
        } else if self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
        }
    }

    pub fn on_down_key(&mut self) {
        if !self.rules.is_empty() {
            let i = match self.list_state.selected() {
                Some(i) => {
                    if i >= self.rules.len() - 1 {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.list_state.select(Some(i));
        }
    }

    pub fn on_up_key(&mut self) {
        if !self.rules.is_empty() {
            let i = match self.list_state.selected() {
                Some(i) => {
                    if i == 0 {
                        self.rules.len() - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.list_state.select(Some(i));
        }
    }
}

impl Default for RulesState {
    fn default() -> Self {
        Self::new()
    }
}
