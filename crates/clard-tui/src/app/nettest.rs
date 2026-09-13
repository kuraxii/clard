use ratatui::widgets::TableState;

use crate::app::checker::AnalysisResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestStatus {
    Pending,
    Testing,
    Done(u16), // latency in ms
    Timeout,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetTestTab {
    Latency,
    Analysis,
}

#[derive(Debug)]
pub struct NodeTestInfo {
    pub name: String,
    pub status: TestStatus,
    pub history: Vec<u16>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SortOrder {
    None,
    LatencyAsc,
    LatencyDesc,
    NameAsc,
    NameDesc,
}

#[derive(Debug)]
pub struct NetTestState {
    pub nodes: Vec<NodeTestInfo>,
    pub list_state: TableState,
    pub sort_order: SortOrder,
    pub is_testing: bool,
    pub tab: NetTestTab,
    pub analysis_result: AnalysisResult,
    pub analysis_testing: bool,
}

impl NetTestState {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            list_state: TableState::default(),
            sort_order: SortOrder::None,
            is_testing: false,
            tab: NetTestTab::Analysis,
            analysis_result: AnalysisResult::default(),
            analysis_testing: false,
        }
    }

    pub fn set_nodes(&mut self, node_names: Vec<String>) {
        self.nodes = node_names
            .into_iter()
            .map(|name| NodeTestInfo {
                name,
                status: TestStatus::Pending,
                history: Vec::new(),
            })
            .collect();

        if !self.nodes.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    pub fn update_node_latency(&mut self, name: &str, latency: u16) {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.name == name) {
            node.status = TestStatus::Done(latency);
            node.history.push(latency);
            if node.history.len() > 20 {
                node.history.remove(0);
            }
        }
    }

    pub fn update_node_error(&mut self, name: &str, err: String) {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.name == name) {
            node.status = TestStatus::Error(err);
        }
    }

    pub fn start_testing_all(&mut self) -> bool {
        if self.nodes.is_empty() || self.is_testing {
            return false;
        }

        self.is_testing = true;
        for node in &mut self.nodes {
            node.status = TestStatus::Testing;
        }
        true
    }

    pub fn finish_testing_if_complete(&mut self) -> bool {
        if !self.is_testing {
            return false;
        }

        let complete = self.nodes.iter().all(|node| {
            matches!(
                node.status,
                TestStatus::Done(_) | TestStatus::Timeout | TestStatus::Error(_)
            )
        });
        if complete {
            self.is_testing = false;
            self.apply_sort_order();
        }
        complete
    }

    pub fn sort(&mut self) {
        match self.sort_order {
            SortOrder::None => self.sort_order = SortOrder::LatencyAsc,
            SortOrder::LatencyAsc => self.sort_order = SortOrder::LatencyDesc,
            SortOrder::LatencyDesc => self.sort_order = SortOrder::NameAsc,
            SortOrder::NameAsc => self.sort_order = SortOrder::NameDesc,
            SortOrder::NameDesc => self.sort_order = SortOrder::LatencyAsc,
        }

        self.apply_sort_order();
    }

    pub fn apply_sort_order(&mut self) {
        self.nodes.sort_by(|a, b| match self.sort_order {
            SortOrder::LatencyAsc | SortOrder::LatencyDesc => {
                let lat_a = match a.status {
                    TestStatus::Done(l) => l,
                    _ => u16::MAX,
                };
                let lat_b = match b.status {
                    TestStatus::Done(l) => l,
                    _ => u16::MAX,
                };
                if self.sort_order == SortOrder::LatencyAsc {
                    lat_a.cmp(&lat_b)
                } else {
                    lat_b.cmp(&lat_a)
                }
            }
            SortOrder::NameAsc => a.name.cmp(&b.name),
            SortOrder::NameDesc => b.name.cmp(&a.name),
            SortOrder::None => std::cmp::Ordering::Equal,
        });
    }

    pub fn on_down_key(&mut self) {
        if !self.nodes.is_empty() {
            let i = match self.list_state.selected() {
                Some(i) => {
                    if i >= self.nodes.len() - 1 {
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
        if !self.nodes.is_empty() {
            let i = match self.list_state.selected() {
                Some(i) => {
                    if i == 0 {
                        self.nodes.len() - 1
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

impl Default for NetTestState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_testing_only_after_every_node_has_result() {
        let mut state = NetTestState::new();
        state.set_nodes(vec!["a".to_string(), "b".to_string()]);

        assert!(state.start_testing_all());
        assert!(state.is_testing);
        assert_eq!(state.nodes[0].status, TestStatus::Testing);
        assert_eq!(state.nodes[1].status, TestStatus::Testing);

        state.update_node_latency("a", 120);
        assert!(!state.finish_testing_if_complete());
        assert!(state.is_testing);

        state.update_node_error("b", "failed".to_string());
        assert!(state.finish_testing_if_complete());
        assert!(!state.is_testing);
    }
}
