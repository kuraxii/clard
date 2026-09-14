use ratatui::widgets::TableState;

use clard_core::mihomo::models::{Connection, Connections, Traffic};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionsSort {
    Upload,
    Download,
}

#[derive(Debug)]
pub struct ConnectionsState {
    pub connections_data: Option<Connections>,
    pub list_state: TableState,
    pub connections: Vec<Connection>,
    pub sort: ConnectionsSort,
    pub traffic: Option<Traffic>,
    pub upload_history: Vec<u64>,
    pub download_history: Vec<u64>,
}

impl ConnectionsState {
    pub fn new() -> Self {
        Self {
            connections_data: None,
            list_state: TableState::default(),
            connections: Vec::new(),
            sort: ConnectionsSort::Download,
            traffic: None,
            upload_history: Vec::new(),
            download_history: Vec::new(),
        }
    }

    pub fn update_connections(&mut self, data: Connections) {
        self.connections = data.connections.clone().unwrap_or_default();
        self.apply_sort();
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

    pub fn update_traffic(&mut self, traffic: Traffic) {
        self.upload_history.push(traffic.up);
        self.download_history.push(traffic.down);
        if self.upload_history.len() > 60 {
            self.upload_history.remove(0);
        }
        if self.download_history.len() > 60 {
            self.download_history.remove(0);
        }
        self.traffic = Some(traffic);
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

    pub fn sort_by_upload(&mut self) {
        self.sort = ConnectionsSort::Upload;
        self.apply_sort();
        self.select_first_if_needed();
    }

    pub fn sort_by_download(&mut self) {
        self.sort = ConnectionsSort::Download;
        self.apply_sort();
        self.select_first_if_needed();
    }

    fn apply_sort(&mut self) {
        match self.sort {
            ConnectionsSort::Upload => {
                self.connections.sort_by_key(|c| std::cmp::Reverse(c.upload));
            }
            ConnectionsSort::Download => {
                self.connections.sort_by_key(|c| std::cmp::Reverse(c.download));
            }
        }
    }

    fn select_first_if_needed(&mut self) {
        if self.connections.is_empty() {
            self.list_state.select(None);
        } else if self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
        }
    }
}

impl Default for ConnectionsState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ConnectionsState;
    use clard_core::mihomo::models::Connections;

    #[test]
    fn sorts_connections_by_download_by_default_and_switches_with_keys() {
        let data: Connections = serde_json::from_str(
            r#"{
                "downloadTotal":0,
                "uploadTotal":0,
                "memory":0,
                "connections":[
                    {"id":"a","upload":300,"download":100,"start":"","chains":[],"rule":"","rulePayload":"","metadata":{"network":"tcp","type":"HTTP","sourceIP":"","destinationIP":"","sourceGeoIP":null,"destinationGeoIP":null,"sourceIPASN":"","destinationIPASN":"","sourcePort":"","destinationPort":"","inboundIP":"","inboundPort":"","inboundName":"","inboundUser":"","host":"","dnsMode":"normal","uid":0,"process":"","processPath":"","specialProxy":"","specialRules":"","remoteDestination":"","dscp":0,"sniffHost":""}},
                    {"id":"b","upload":100,"download":300,"start":"","chains":[],"rule":"","rulePayload":"","metadata":{"network":"tcp","type":"HTTP","sourceIP":"","destinationIP":"","sourceGeoIP":null,"destinationGeoIP":null,"sourceIPASN":"","destinationIPASN":"","sourcePort":"","destinationPort":"","inboundIP":"","inboundPort":"","inboundName":"","inboundUser":"","host":"","dnsMode":"normal","uid":0,"process":"","processPath":"","specialProxy":"","specialRules":"","remoteDestination":"","dscp":0,"sniffHost":""}}
                ]
            }"#,
        )
        .unwrap();

        let mut state = ConnectionsState::new();
        state.update_connections(data);
        assert_eq!(state.connections[0].id, "b");

        state.sort_by_upload();
        assert_eq!(state.connections[0].id, "a");

        state.sort_by_download();
        assert_eq!(state.connections[0].id, "b");
    }
}
