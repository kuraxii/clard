//! 轻量 i18n（doc/05 §1 R1.3）：默认英语，可切中文。
//!
//! 说明：doc 指定 rust-i18n 宏方案；为控制依赖面与改动面，本实现用「键 → (en, zh)」字典，
//! 语言键存 helper 侧 `clard.toml`（`Settings.language`），TUI 启动/切换时经 IPC 读取后
//! 全局生效。行为等价（默认 en、可切 zh），文案覆盖后续增量补全。

/// 语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Zh,
}

impl Lang {
    pub fn parse(s: &str) -> Self {
        match s {
            "zh" => Self::Zh,
            _ => Self::En,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::En => Self::Zh,
            Self::Zh => Self::En,
        }
    }
}

/// 翻译：`key` 不存在时回退到 key 本身。
pub fn t(lang: Lang, key: &str) -> &str {
    let (en, zh) = match key {
        "page.home" => ("Home", "主页"),
        "page.profiles" => ("Profiles", "配置"),
        "page.proxies" => ("Proxies", "代理"),
        "page.connections" => ("Connections", "连接"),
        "page.logs" => ("Logs", "日志"),
        "page.settings" => ("Settings", "设置"),
        "page.rules" => ("Rules", "规则"),
        "status.ready" => ("Ready", "就绪"),
        "footer.help" => ("help", "帮助"),
        "footer.quit" => ("quit", "退出"),
        _ => (key, key),
    };
    match lang {
        Lang::En => en,
        Lang::Zh => zh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_parse_and_toggle() {
        assert_eq!(Lang::parse("zh"), Lang::Zh);
        assert_eq!(Lang::parse("en"), Lang::En);
        assert_eq!(Lang::parse("fr"), Lang::En, "未知回退 en");
        assert_eq!(Lang::En.toggle(), Lang::Zh);
        assert_eq!(Lang::Zh.toggle(), Lang::En);
    }

    #[test]
    fn translate_pages_and_unknown_fallback() {
        assert_eq!(t(Lang::En, "page.home"), "Home");
        assert_eq!(t(Lang::Zh, "page.home"), "主页");
        assert_eq!(t(Lang::Zh, "page.proxies"), "代理");
        assert_eq!(t(Lang::En, "page.unknown"), "page.unknown");
    }
}
