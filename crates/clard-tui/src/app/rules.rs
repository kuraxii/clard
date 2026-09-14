//! 规则页状态（doc/03 §5.5）：生效规则查看、启用/禁用、搜索过滤、规则集。
//!
//! 数据源：`GET /rules`、`PATCH /rules/disable`、`GET/PUT /providers/rules`（doc/04 §5/§6）。

use std::collections::HashMap;

use clard_core::mihomo::models::{Rule, RuleProvider};
use ratatui::widgets::TableState;

/// 规则页页签：生效规则 / 规则集（rule-provider）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RulesTab {
    #[default]
    Rules,
    Providers,
}

/// 规则页状态。
#[derive(Debug)]
pub struct RulesState {
    pub tab: RulesTab,
    all_rules: Vec<Rule>,
    /// 过滤后的渲染视图。
    pub rules: Vec<Rule>,
    pub filter: String,
    pub rules_state: TableState,
    /// 规则集（按名排序，显示顺序稳定）。
    pub providers: Vec<(String, RuleProvider)>,
    pub providers_state: TableState,
}

impl RulesState {
    pub fn new() -> Self {
        Self {
            tab: RulesTab::Rules,
            all_rules: Vec::new(),
            rules: Vec::new(),
            filter: String::new(),
            rules_state: TableState::default(),
            providers: Vec::new(),
            providers_state: TableState::default(),
        }
    }

    pub fn update_rules(&mut self, rules: Vec<Rule>) {
        self.all_rules = rules;
        self.refresh_rules();
    }

    pub fn update_providers(&mut self, providers: HashMap<String, RuleProvider>) {
        let mut v: Vec<_> = providers.into_iter().collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        self.providers = v;

        if self.providers.is_empty() {
            self.providers_state.select(None);
        } else if self.providers_state.selected().is_none() {
            self.providers_state.select(Some(0));
        } else if let Some(i) = self.providers_state.selected()
            && i >= self.providers.len()
        {
            self.providers_state.select(Some(self.providers.len() - 1));
        }
    }

    /// 设置关键字过滤（类型/内容/策略），空串 = 不过滤。
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.refresh_rules();
    }

    pub fn toggle_tab(&mut self) {
        self.tab = match self.tab {
            RulesTab::Rules => RulesTab::Providers,
            RulesTab::Providers => RulesTab::Rules,
        };
    }

    pub fn selected_rule(&self) -> Option<&Rule> {
        self.rules_state.selected().and_then(|i| self.rules.get(i))
    }

    pub fn selected_provider_name(&self) -> Option<String> {
        self.providers_state
            .selected()
            .and_then(|i| self.providers.get(i))
            .map(|(name, _)| name.clone())
    }

    pub fn on_down_key(&mut self) {
        match self.tab {
            RulesTab::Rules => {
                if self.rules.is_empty() {
                    return;
                }
                let i = match self.rules_state.selected() {
                    Some(i) if i + 1 < self.rules.len() => i + 1,
                    _ => 0,
                };
                self.rules_state.select(Some(i));
            }
            RulesTab::Providers => {
                if self.providers.is_empty() {
                    return;
                }
                let i = match self.providers_state.selected() {
                    Some(i) if i + 1 < self.providers.len() => i + 1,
                    _ => 0,
                };
                self.providers_state.select(Some(i));
            }
        }
    }

    pub fn on_up_key(&mut self) {
        match self.tab {
            RulesTab::Rules => {
                if self.rules.is_empty() {
                    return;
                }
                let i = match self.rules_state.selected() {
                    Some(0) | None => self.rules.len() - 1,
                    Some(i) => i - 1,
                };
                self.rules_state.select(Some(i));
            }
            RulesTab::Providers => {
                if self.providers.is_empty() {
                    return;
                }
                let i = match self.providers_state.selected() {
                    Some(0) | None => self.providers.len() - 1,
                    Some(i) => i - 1,
                };
                self.providers_state.select(Some(i));
            }
        }
    }

    fn refresh_rules(&mut self) {
        self.rules = self
            .all_rules
            .iter()
            .filter(|r| rule_matches(r, &self.filter))
            .cloned()
            .collect();

        if self.rules.is_empty() {
            self.rules_state.select(None);
        } else if let Some(i) = self.rules_state.selected() {
            if i >= self.rules.len() {
                self.rules_state.select(Some(self.rules.len() - 1));
            }
        } else {
            self.rules_state.select(Some(0));
        }
    }
}

impl Default for RulesState {
    fn default() -> Self {
        Self::new()
    }
}

/// 关键字过滤：命中类型/内容/策略（忽略大小写，doc/05 §5 R5.3）。
fn rule_matches(rule: &Rule, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let needle = needle.to_lowercase();
    rule.rule_type.to_lowercase().contains(&needle)
        || rule.payload.to_lowercase().contains(&needle)
        || rule.proxy.to_lowercase().contains(&needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(index: usize, rule_type: &str, payload: &str, proxy: &str) -> Rule {
        Rule {
            index,
            rule_type: rule_type.to_string(),
            payload: payload.to_string(),
            proxy: proxy.to_string(),
            size: -1,
            extra: None,
        }
    }

    fn provider(name: &str, count: u32) -> RuleProvider {
        RuleProvider {
            behavior: clard_core::mihomo::models::RuleBehavior::Domain,
            format: clard_core::mihomo::models::RuleFormat::Text,
            name: name.to_string(),
            rule_count: count,
            provider_type: clard_core::mihomo::models::ProviderType::Rule,
            updated_at: String::new(),
            vehicle_type: clard_core::mihomo::models::VehicleType::HTTP,
        }
    }

    #[test]
    fn filter_matches_type_payload_proxy() {
        let mut state = RulesState::new();
        state.update_rules(vec![
            rule(0, "DOMAIN", "google.com", "Proxy"),
            rule(1, "GEOIP", "CN", "DIRECT"),
            rule(2, "MATCH", "*", "Proxy"),
        ]);

        state.set_filter("google".to_string());
        assert_eq!(state.rules.len(), 1);
        assert_eq!(state.rules[0].index, 0);

        state.set_filter("geoip".to_string());
        assert_eq!(state.rules.len(), 1);
        assert_eq!(state.rules[0].index, 1);

        state.set_filter("direct".to_string());
        assert_eq!(state.rules.len(), 1);
        assert_eq!(state.rules[0].index, 1);

        state.set_filter(String::new());
        assert_eq!(state.rules.len(), 3);
    }

    #[test]
    fn empty_filter_and_no_match_clear_selection() {
        let mut state = RulesState::new();
        state.update_rules(vec![rule(0, "DOMAIN", "a", "Proxy")]);
        assert!(state.selected_rule().is_some());

        state.set_filter("nomatch".to_string());
        assert!(state.rules.is_empty());
        assert!(state.selected_rule().is_none());
    }

    #[test]
    fn providers_are_sorted_by_name() {
        let mut state = RulesState::new();
        let mut map = HashMap::new();
        map.insert("b".to_string(), provider("b", 2));
        map.insert("a".to_string(), provider("a", 1));
        state.update_providers(map);

        assert_eq!(state.providers[0].0, "a");
        assert_eq!(state.providers[1].0, "b");
        assert_eq!(state.selected_provider_name(), Some("a".to_string()));
    }

    #[test]
    fn tab_toggle_switches_views() {
        let mut state = RulesState::new();
        assert_eq!(state.tab, RulesTab::Rules);
        state.toggle_tab();
        assert_eq!(state.tab, RulesTab::Providers);
        state.toggle_tab();
        assert_eq!(state.tab, RulesTab::Rules);
    }
}
