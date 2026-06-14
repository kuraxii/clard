use ratatui::widgets::TableState;

use crate::ipc::models::{Connection, Connections};

#[derive(Debug)]
pub struct ConnectionsState {
    pub connections_data: Option<Connections>,
    pub list_state: TableState,
    pub connections: Vec<Connection>,
}

impl ConnectionsState {
    pub fn new() -> Self {
        Self {
            connections_data: None,
            list_state: TableState::default(),
            connections: Vec::new(),
        }
    }

    pub fn update_connections(&mut self, data: Connections) {
        self.connections = data.connections.clone().unwrap_or_default();
        self.connections_data = Some(data);

        if self.list_state.selected().is_none() && !self.connections.is_empty() {
            self.list_state.select(Some(0));
        } else if let Some(i) = self.list_state.selected() {
            if i >= self.connections.len() {
                if self.connections.is_empty() {
                    self.list_state.select(None);
                } else {
                    self.list_state.select(Some(self.connections.len() - 1));
                }
            }
        }
    }

    pub fn on_down_key(&mut self) {
        if !self.connections.is_empty() {
            let i = match self.list_state.selected() {
                Some(i) => {
                    if i >= self.connections.len() - 1 {
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
        if !self.connections.is_empty() {
            let i = match self.list_state.selected() {
                Some(i) => {
                    if i == 0 {
                        self.connections.len() - 1
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

impl Default for ConnectionsState {
    fn default() -> Self {
        Self::new()
    }
}
