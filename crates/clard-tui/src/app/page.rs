//! 顶层页面枚举与键位映射（doc/03 §4.1）。
//!
//! 七页：主页 / 配置 / 代理 / 连接 / 日志 / 设置 / 规则；数字键 `1`–`7` 直达。
//! 注意：规则页为 `7`（doc/03 §2 导航条、§4.1 状态机、§5.5），日志为 `5`、设置为 `6`。

/// 顶层页面（`1`–`7` 直达；顺序即数字键顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Profiles,
    Proxies,
    Connections,
    Logs,
    Settings,
    Rules,
}

impl Page {
    /// 全部页面，顺序即数字键顺序。
    pub const ALL: [Self; 7] = [
        Self::Home,
        Self::Profiles,
        Self::Proxies,
        Self::Connections,
        Self::Logs,
        Self::Settings,
        Self::Rules,
    ];

    /// 数字键 → 页面；非 `1`–`7` 返回 `None`。
    pub fn from_key(key: char) -> Option<Self> {
        let digit = key.to_digit(10)?;
        if digit == 0 {
            return None;
        }
        Self::ALL.get(digit as usize - 1).copied()
    }

    /// 页面 → 序号（0-based，用于导航高亮）。
    pub fn index(self) -> usize {
        match self {
            Self::Home => 0,
            Self::Profiles => 1,
            Self::Proxies => 2,
            Self::Connections => 3,
            Self::Logs => 4,
            Self::Settings => 5,
            Self::Rules => 6,
        }
    }

    /// 页面英文标题（UI 文案默认英语，doc/05 §1 R1.3）。
    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Profiles => "Profiles",
            Self::Proxies => "Proxies",
            Self::Connections => "Connections",
            Self::Logs => "Logs",
            Self::Settings => "Settings",
            Self::Rules => "Rules",
        }
    }

    /// 页面直达数字键（页脚/帮助展示用）。
    pub fn key(self) -> &'static str {
        match self {
            Self::Home => "1",
            Self::Profiles => "2",
            Self::Proxies => "3",
            Self::Connections => "4",
            Self::Logs => "5",
            Self::Settings => "6",
            Self::Rules => "7",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Page;

    #[test]
    fn key_mapping_matches_doc_order() {
        assert_eq!(Page::from_key('1'), Some(Page::Home));
        assert_eq!(Page::from_key('2'), Some(Page::Profiles));
        assert_eq!(Page::from_key('3'), Some(Page::Proxies));
        assert_eq!(Page::from_key('4'), Some(Page::Connections));
        assert_eq!(Page::from_key('5'), Some(Page::Logs));
        assert_eq!(Page::from_key('6'), Some(Page::Settings));
        assert_eq!(Page::from_key('7'), Some(Page::Rules));
    }

    #[test]
    fn key_mapping_rejects_unknown_keys() {
        assert_eq!(Page::from_key('0'), None);
        assert_eq!(Page::from_key('8'), None);
        assert_eq!(Page::from_key('9'), None);
        assert_eq!(Page::from_key('a'), None);
    }

    #[test]
    fn index_round_trips() {
        for (idx, page) in Page::ALL.iter().enumerate() {
            assert_eq!(page.index(), idx);
        }
    }
}
