//! helper 系统配置（`/etc/clard/helper.toml`，root 0644；doc/01 §4 / doc/05 R7.5）。
//!
//! 与用户设置（clard.toml）分离：日志轮转/双写/核心日志级别是系统级运维参数。
//! 启动时读取一次（`init`），改动需重启 helper 生效（TUI 提示 sudo 编辑命令，不直写）。
//! 文件不存在 → 用默认值。`CLARD_HELPER_TOML` 可覆盖（开发/测试）。

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::Deserialize;

/// helper 配置（全字段可选，toml 缺省用默认）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct HelperConfig {
    /// 核心日志级别（注入 mihomo `log-level`）：debug / info / warn / error
    pub log_level: String,
    /// 应用日志（tui.log）轮转上限（字节）
    pub app_log_max_bytes: u64,
    /// 应用日志保留份数
    pub app_log_keep: usize,
    /// 审计日志保留份数（audit.log，上限 10MB 固定）
    pub audit_keep: usize,
    /// 审计是否双写 journald（stdout KEY=VALUE）
    pub audit_dual_write: bool,
}

impl Default for HelperConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            app_log_max_bytes: 1024 * 1024,
            app_log_keep: 5,
            audit_keep: 5,
            audit_dual_write: true,
        }
    }
}

impl HelperConfig {
    /// 配置文件路径：`CLARD_HELPER_TOML` 覆盖，默认 `/etc/clard/helper.toml`。
    pub fn path() -> PathBuf {
        if let Ok(p) = std::env::var("CLARD_HELPER_TOML") {
            if !p.is_empty() {
                return PathBuf::from(p);
            }
        }
        PathBuf::from("/etc/clard/helper.toml")
    }

    /// 读取；文件缺失/解析失败 → 默认值（不阻塞启动）。
    pub fn load() -> Self {
        Self::load_at(&Self::path())
    }

    /// 从指定路径读取（测试注入；逻辑同 [`Self::load`]）。
    pub fn load_at(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

static CFG: OnceLock<HelperConfig> = OnceLock::new();

/// 启动时初始化（daemon::run 早期调用；单进程单次）。
pub fn init() {
    let _ = CFG.set(HelperConfig::load());
}

/// 全局配置访问（logs/audit/rpc 用）。
pub fn global() -> &'static HelperConfig {
    CFG.get().expect("helper config 未初始化（daemon::run 应先调用 init）")
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn default_values_match_docs() {
        let c = HelperConfig::default();
        assert_eq!(c.log_level, "info");
        assert_eq!(c.app_log_max_bytes, 1024 * 1024);
        assert_eq!(c.app_log_keep, 5);
        assert_eq!(c.audit_keep, 5);
        assert!(c.audit_dual_write);
    }

    #[test]
    fn load_parses_toml() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("helper.toml");
        std::fs::write(
            &path,
            "log_level = \"debug\"\napp_log_max_bytes = 2048\napp_log_keep = 3\naudit_keep = 2\naudit_dual_write = false\n",
        )
        .unwrap();
        let c = HelperConfig::load_at(&path);
        assert_eq!(c.log_level, "debug");
        assert_eq!(c.app_log_max_bytes, 2048);
        assert_eq!(c.app_log_keep, 3);
        assert_eq!(c.audit_keep, 2);
        assert!(!c.audit_dual_write);
    }

    #[test]
    fn load_partial_toml_fills_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("helper.toml");
        std::fs::write(&path, "log_level = \"warn\"\n").unwrap();
        let c = HelperConfig::load_at(&path);
        assert_eq!(c.log_level, "warn");
        assert_eq!(c.app_log_keep, 5, "未配置字段用默认");
    }

    #[test]
    fn load_missing_file_uses_defaults() {
        let dir = tempdir().unwrap();
        let c = HelperConfig::load_at(&dir.path().join("nope.toml"));
        assert_eq!(c, HelperConfig::default());
    }
}
