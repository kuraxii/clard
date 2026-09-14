use ratatui::widgets::TableState;

use clard_core::mihomo::models::{Connection, Connections, Traffic};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionsSort {
    Upload,
    Download,
}

/// 流量单位（R4.3 `c` 切换）：自动二进制单位 ⇄ 固定 KB。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnUnit {
    #[default]
    Auto,
    Kb,
}

/// 连接页状态：`all` 为全量数据，`connections` 为过滤+排序后的渲染视图。
#[derive(Debug)]
pub struct ConnectionsState {
    pub connections_data: Option<Connections>,
    pub list_state: TableState,
    all: Vec<Connection>,
    pub connections: Vec<Connection>,
    pub filter: String,
    pub sort: ConnectionsSort,
    pub unit: ConnUnit,
    pub traffic: Option<Traffic>,
    pub upload_history: Vec<u64>,
    pub download_history: Vec<u64>,
}

impl ConnectionsState {
    pub fn new() -> Self {
        Self {
            connections_data: None,
            list_state: TableState::default(),
            all: Vec::new(),
            connections: Vec::new(),
            filter: String::new(),
            sort: ConnectionsSort::Download,
            unit: ConnUnit::Auto,
            traffic: None,
            upload_history: Vec::new(),
            download_history: Vec::new(),
        }
    }

    pub fn update_connections(&mut self, data: Connections) {
        self.all = data.connections.clone().unwrap_or_default();
        self.connections_data = Some(data);
        self.refresh_view();
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

    /// 设置关键字过滤（来源/目标/规则/进程），空串 = 不过滤。
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.refresh_view();
    }

    /// 切换流量单位（R4.3）。
    pub fn toggle_unit(&mut self) {
        self.unit = match self.unit {
            ConnUnit::Auto => ConnUnit::Kb,
            ConnUnit::Kb => ConnUnit::Auto,
        };
    }

    pub fn on_down_key(&mut self) {
        if self.connections.is_empty() {
            return;
        }
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

    pub fn on_up_key(&mut self) {
        if self.connections.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(0) | None => self.connections.len() - 1,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(i));
    }

    pub fn sort_by_upload(&mut self) {
        self.sort = ConnectionsSort::Upload;
        self.refresh_view();
    }

    pub fn sort_by_download(&mut self) {
        self.sort = ConnectionsSort::Download;
        self.refresh_view();
    }

    /// 过滤 + 排序 + 修正选中下标。
    fn refresh_view(&mut self) {
        self.connections = self
            .all
            .iter()
            .filter(|c| connection_matches(c, &self.filter))
            .cloned()
            .collect();
        self.apply_sort();

        if self.connections.is_empty() {
            self.list_state.select(None);
        } else if let Some(i) = self.list_state.selected() {
            if i >= self.connections.len() {
                self.list_state.select(Some(self.connections.len() - 1));
            }
        } else {
            self.list_state.select(Some(0));
        }
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
}

impl Default for ConnectionsState {
    fn default() -> Self {
        Self::new()
    }
}

/// 关键字过滤：命中 host/来源 IP/目标 IP/规则/规则负载/进程（忽略大小写）。
fn connection_matches(conn: &Connection, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let needle = needle.to_lowercase();
    let host = if !conn.metadata.host.is_empty() {
        &conn.metadata.host
    } else if !conn.metadata.sniff_host.is_empty() {
        &conn.metadata.sniff_host
    } else {
        &conn.metadata.destination_ip
    };

    host.to_lowercase().contains(&needle)
        || conn.metadata.source_ip.to_lowercase().contains(&needle)
        || conn.metadata.destination_ip.to_lowercase().contains(&needle)
        || conn.rule.to_lowercase().contains(&needle)
        || conn.rule_payload.to_lowercase().contains(&needle)
        || conn.metadata.process.to_lowercase().contains(&needle)
}

#[cfg(test)]
mod tests {
    use super::{ConnUnit, ConnectionsState};
    use clard_core::mihomo::models::Connections;

    fn sample_connections() -> Connections {
        serde_json::from_str(
            r#"{
                "downloadTotal":0,
                "uploadTotal":0,
                "memory":0,
                "connections":[
                    {"id":"a","upload":300,"download":100,"start":"","chains":[],"rule":"Proxy","rulePayload":"google.com","metadata":{"network":"tcp","type":"HTTP","sourceIP":"192.168.1.5","destinationIP":"104.16.1.1","sourceGeoIP":null,"destinationGeoIP":null,"sourceIPASN":"","destinationIPASN":"","sourcePort":"52000","destinationPort":"443","inboundIP":"","inboundPort":"","inboundName":"","inboundUser":"","host":"google.com","dnsMode":"normal","uid":0,"process":"chrome","processPath":"","specialProxy":"","specialRules":"","remoteDestination":"","dscp":0,"sniffHost":""}},
                    {"id":"b","upload":100,"download":300,"start":"","chains":[],"rule":"DIRECT","rulePayload":"","metadata":{"network":"udp","type":"HTTP","sourceIP":"192.168.1.5","destinationIP":"8.8.8.8","sourceGeoIP":null,"destinationGeoIP":null,"sourceIPASN":"","destinationIPASN":"","sourcePort":"52001","destinationPort":"53","inboundIP":"","inboundPort":"","inboundName":"","inboundUser":"","host":"","dnsMode":"normal","uid":0,"process":"dns","processPath":"","specialProxy":"","specialRules":"","remoteDestination":"","dscp":0,"sniffHost":"dns.google"}}
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn sorts_connections_by_download_by_default_and_switches_with_keys() {
        let mut state = ConnectionsState::new();
        state.update_connections(sample_connections());
        assert_eq!(state.connections[0].id, "b");

        state.sort_by_upload();
        assert_eq!(state.connections[0].id, "a");

        state.sort_by_download();
        assert_eq!(state.connections[0].id, "b");
    }

    #[test]
    fn filter_matches_host_rule_and_process() {
        let mut state = ConnectionsState::new();
        state.update_connections(sample_connections());

        state.set_filter("google".to_string());
        assert_eq!(state.connections.len(), 2, "host/rule 命中两条");

        state.set_filter("dns.google".to_string());
        assert_eq!(state.connections.len(), 1);
        assert_eq!(state.connections[0].id, "b");

        state.set_filter("chrome".to_string());
        assert_eq!(state.connections.len(), 1);
        assert_eq!(state.connections[0].id, "a");

        state.set_filter("8.8.8.8".to_string());
        assert_eq!(state.connections.len(), 1);
        assert_eq!(state.connections[0].id, "b");
    }

    #[test]
    fn unit_toggle_cycles_auto_and_kb() {
        let mut state = ConnectionsState::new();
        assert_eq!(state.unit, ConnUnit::Auto);
        state.toggle_unit();
        assert_eq!(state.unit, ConnUnit::Kb);
        state.toggle_unit();
        assert_eq!(state.unit, ConnUnit::Auto);
    }

    #[test]
    fn empty_filter_shows_all_and_filter_update_keeps_selection_sane() {
        let mut state = ConnectionsState::new();
        state.update_connections(sample_connections());
        assert_eq!(state.connections.len(), 2);

        state.set_filter("no-match".to_string());
        assert!(state.connections.is_empty());
        assert!(state.list_state.selected().is_none());

        state.set_filter(String::new());
        assert_eq!(state.connections.len(), 2);
        assert!(state.list_state.selected().is_some());
    }
}
