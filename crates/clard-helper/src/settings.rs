//! 系统级设置存储（`<state>/clard.toml`，doc/01 §7 / doc/05 §7 R7.1）。
//!
//! 默认：自动更新间隔 6 小时（0=关）、语言 en、主题 dark、混合端口 7890（仅回环）。
//! 写操作原子替换索引式落盘。

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use clard_proto::{Settings, SettingsPatch};
use thiserror::Error;

/// 设置文件名
pub const SETTINGS_FILE: &str = "clard.toml";

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("IO 错误: {0}")]
    Io(#[from] io::Error),
    #[error("clard.toml 解析失败: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("clard.toml 序列化失败: {0}")]
    Encode(#[from] toml::ser::Error),
    #[error("端口必须在 1-65535: {0}")]
    InvalidPort(u16),
}

/// 设置存储（helper 单写者经锁串行访问）。
pub struct SettingsStore {
    path: PathBuf,
    settings: Settings,
}

/// 设置归一化扩展：tun_dns_mode 空/非法 → fake-ip；mode 空/非法 → rule。
trait SettingsNormalize {
    fn normalize_dns_mode(&mut self);
    fn normalize_mode(&mut self);
    fn normalize_dns_filter_mode(&mut self);
}

impl SettingsNormalize for Settings {
    fn normalize_dns_mode(&mut self) {
        match self.tun_dns_mode.as_str() {
            "fake-ip" | "redir-host" => {}
            _ => self.tun_dns_mode = "fake-ip".into(),
        }
    }

    fn normalize_mode(&mut self) {
        match self.mode.as_str() {
            "rule" | "global" | "direct" => {}
            // 空或非法值归一化为 rule（向后兼容旧 clard.toml / 手改坏值）
            _ => self.mode = "rule".into(),
        }
    }

    fn normalize_dns_filter_mode(&mut self) {
        match self.dns_fake_ip_filter_mode.as_str() {
            "blacklist" | "whitelist" | "rule" => {}
            _ => self.dns_fake_ip_filter_mode = "blacklist".into(),
        }
    }
}

impl SettingsStore {
    /// 打开设置；文件不存在则用默认值（首次写时落盘）。
    pub fn open(root: &Path) -> Result<Self, SettingsError> {
        let path = root.join(SETTINGS_FILE);
        let mut settings = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(e) => return Err(e.into()),
        };
        // 归一化：旧 clard.toml 无 tun_dns_mode（空串）→ fake-ip；mode 空/非法 → rule
        settings.normalize_dns_mode();
        settings.normalize_mode();
        settings.normalize_dns_filter_mode();
        Ok(Self { path, settings })
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// 从磁盘重新载入（备份恢复后调用）。
    pub fn reload(&mut self) -> Result<(), SettingsError> {
        let root = self.path.parent().map(Path::to_path_buf).unwrap_or_default();
        *self = Self::open(&root)?;
        Ok(())
    }

    /// 应用补丁并原子落盘（非 yaml 白名单字段 / 无需 regenerate 的路径）。
    pub fn patch(&mut self, patch: &SettingsPatch) -> Result<(), SettingsError> {
        self.apply_in_memory(patch)?;
        self.save()
    }

    /// 仅应用到内存（不落盘）。yaml 白名单字段（§8.4）由 rpc 先 apply → regenerate
    /// 成功 → [`Self::save`] 持久化；失败 → [`Self::restore`] 恢复内存（§7.2 设置不静默变）。
    pub(crate) fn apply_in_memory(&mut self, patch: &SettingsPatch) -> Result<(), SettingsError> {
        if let Some(v) = patch.auto_update_interval_hours {
            self.settings.auto_update_interval_hours = v;
        }
        if let Some(v) = &patch.language {
            self.settings.language = v.clone();
        }
        if let Some(v) = &patch.theme {
            self.settings.theme = v.clone();
        }
        if let Some(v) = patch.mixed_port {
            if v == 0 {
                return Err(SettingsError::InvalidPort(v));
            }
            self.settings.mixed_port = v;
        }
        if let Some(v) = &patch.mode {
            // 仅接受 rule / global / direct；空或非法值归一化为 rule（向后兼容）
            self.settings.mode = match v.as_str() {
                "global" => "global".into(),
                "direct" => "direct".into(),
                _ => "rule".into(),
            };
        }
        if let Some(v) = &patch.test_url {
            self.settings.test_url = v.clone();
        }
        // TUN（R7.2）
        if let Some(v) = patch.tun_enabled {
            self.settings.tun_enabled = v;
        }
        if let Some(v) = &patch.tun_stack {
            self.settings.tun_stack = v.clone();
        }
        if let Some(v) = &patch.tun_dns_mode {
            // 仅接受 fake-ip / redir-host；空或非法值归一化为 fake-ip（向后兼容旧 clard.toml）
            self.settings.tun_dns_mode = match v.as_str() {
                "redir-host" => "redir-host".into(),
                _ => "fake-ip".into(),
            };
        }
        if let Some(v) = &patch.dns_hijack {
            self.settings.dns_hijack = v.clone();
        }
        if let Some(v) = &patch.route_exclude_address {
            self.settings.route_exclude_address = v.clone();
        }
        if let Some(v) = patch.strict_route {
            self.settings.strict_route = v;
        }
        if let Some(v) = patch.auto_redirect {
            self.settings.auto_redirect = v;
        }
        // DNS 页签（R7.2.1）
        if let Some(v) = patch.dns_enable {
            self.settings.dns_enable = v;
        }
        if let Some(v) = &patch.dns_fake_ip_filter_mode {
            // 仅接受 blacklist / whitelist / rule；空或非法值归一化为 blacklist
            self.settings.dns_fake_ip_filter_mode = match v.as_str() {
                "whitelist" => "whitelist".into(),
                "rule" => "rule".into(),
                _ => "blacklist".into(),
            };
        }
        if let Some(v) = &patch.dns_fake_ip_filter {
            self.settings.dns_fake_ip_filter = v.clone();
        }
        if let Some(v) = patch.dns_use_hosts {
            self.settings.dns_use_hosts = v;
        }
        if let Some(v) = patch.dns_use_system_hosts {
            self.settings.dns_use_system_hosts = v;
        }
        if let Some(v) = &patch.dns_nameserver_policy {
            self.settings.dns_nameserver_policy = v.clone();
        }
        if let Some(v) = &patch.dns_hosts {
            self.settings.dns_hosts = v.clone();
        }
        if let Some(v) = &patch.dns_nameserver {
            self.settings.dns_nameserver = v.clone();
        }
        if let Some(v) = &patch.dns_default_nameserver {
            self.settings.dns_default_nameserver = v.clone();
        }
        Ok(())
    }

    /// 原子落盘当前内存设置（tmp + rename）。
    pub(crate) fn save(&self) -> Result<(), SettingsError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(&self.settings)?;
        let tmp = self.path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// 恢复内存设置（不落盘；用于 regenerate 失败后的回滚，磁盘从未写入新值）。
    pub(crate) fn restore(&mut self, s: Settings) {
        self.settings = s;
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn open_missing_yields_defaults() {
        let dir = tempdir().unwrap();
        let store = SettingsStore::open(dir.path()).unwrap();
        let s = store.get();
        assert_eq!(s.auto_update_interval_hours, 6);
        assert_eq!(s.language, "en");
        assert_eq!(s.theme, "dark");
        assert_eq!(s.mixed_port, 7890);
        assert!(!s.tun_enabled, "TUN 默认关");
        assert_eq!(s.tun_stack, "gvisor");
        assert!(s.route_exclude_address.is_empty(), "空 = 用默认私网段");
        assert!(!s.dns_enable, "DNS 开关默认关");
        assert_eq!(s.dns_fake_ip_filter_mode, "blacklist");
        assert!(s.dns_use_hosts);
        assert!(s.dns_use_system_hosts);
        assert!(s.dns_nameserver_policy.is_empty());
        assert!(s.dns_hosts.is_empty());
        assert!(s.dns_nameserver.is_empty());
        assert!(s.dns_default_nameserver.is_empty());
    }

    #[test]
    fn patch_updates_and_persists() {
        let dir = tempdir().unwrap();
        {
            let mut store = SettingsStore::open(dir.path()).unwrap();
            store
                .patch(&SettingsPatch {
                    auto_update_interval_hours: Some(12),
                    language: Some("zh".into()),
                    theme: None,
                    mixed_port: Some(7891),
                    mode: Some("global".into()),
                    test_url: Some("http://x".into()),
                    tun_enabled: Some(true),
                    force_tun: None,
                    tun_stack: Some("gvisor".into()),
                    tun_dns_mode: Some("redir-host".into()),
                    dns_hijack: Some(vec!["any:53".into()]),
                    route_exclude_address: Some(vec!["10.0.0.0/8".into()]),
                    strict_route: Some(false),
                    auto_redirect: Some(true),
                    dns_enable: Some(true),
                    dns_fake_ip_filter_mode: Some("whitelist".into()),
                    dns_fake_ip_filter: Some(vec!["*.lan".into(), "oa.x".into()]),
                    dns_use_hosts: Some(false),
                    dns_use_system_hosts: Some(true),
                    dns_nameserver_policy: Some(vec!["+.corp=10.10.0.2".into()]),
                    dns_hosts: Some(vec!["oa.x=10.20.30.40".into()]),
                    dns_nameserver: Some(vec!["tls://1.1.1.1".into()]),
                    dns_default_nameserver: Some(vec!["223.5.5.5".into()]),
                })
                .unwrap();
        }
        let store = SettingsStore::open(dir.path()).unwrap();
        let s = store.get();
        assert_eq!(s.auto_update_interval_hours, 12);
        assert_eq!(s.language, "zh");
        assert_eq!(s.theme, "dark", "未补丁字段保持默认");
        assert_eq!(s.mixed_port, 7891);
        assert_eq!(s.mode, "global");
        assert!(s.tun_enabled);
        assert_eq!(s.tun_stack, "gvisor");
        assert_eq!(s.tun_dns_mode, "redir-host");
        assert_eq!(s.dns_hijack, vec!["any:53"]);
        assert_eq!(s.route_exclude_address, vec!["10.0.0.0/8"]);
        assert!(!s.strict_route);
        assert!(s.auto_redirect);
        assert!(s.dns_enable);
        assert_eq!(s.dns_fake_ip_filter_mode, "whitelist");
        assert_eq!(s.dns_fake_ip_filter, vec!["*.lan", "oa.x"]);
        assert!(!s.dns_use_hosts);
        assert!(s.dns_use_system_hosts);
        assert_eq!(s.dns_nameserver_policy, vec!["+.corp=10.10.0.2"]);
        assert_eq!(s.dns_hosts, vec!["oa.x=10.20.30.40"]);
        assert_eq!(s.dns_nameserver, vec!["tls://1.1.1.1"]);
        assert_eq!(s.dns_default_nameserver, vec!["223.5.5.5"]);
    }

    #[test]
    fn mode_normalized_on_patch() {
        let dir = tempdir().unwrap();
        let mut store = SettingsStore::open(dir.path()).unwrap();
        // 合法值直通
        store
            .patch(&SettingsPatch {
                mode: Some("global".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(store.get().mode, "global");
        // 空/非法归一化为 rule（向后兼容）
        store
            .patch(&SettingsPatch {
                mode: Some("bogus".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(store.get().mode, "rule");
    }

    #[test]
    fn mode_normalized_on_open() {
        let dir = tempdir().unwrap();
        // 手改 clard.toml 为非法值 → open 归一化为 rule
        std::fs::write(dir.path().join("clard.toml"), "mode = \"bogus\"\n").unwrap();
        let store = SettingsStore::open(dir.path()).unwrap();
        assert_eq!(store.get().mode, "rule");
    }

    #[test]
    fn dns_filter_mode_normalized_on_patch_and_open() {
        let dir = tempdir().unwrap();
        let mut store = SettingsStore::open(dir.path()).unwrap();
        // 非法值归一化为 blacklist
        store.patch(&SettingsPatch { dns_fake_ip_filter_mode: Some("bogus".into()), ..Default::default() }).unwrap();
        assert_eq!(store.get().dns_fake_ip_filter_mode, "blacklist");
        store.patch(&SettingsPatch { dns_fake_ip_filter_mode: Some("rule".into()), ..Default::default() }).unwrap();
        assert_eq!(store.get().dns_fake_ip_filter_mode, "rule");
        // 手改 clard.toml 非法值 → open 归一化
        std::fs::write(dir.path().join("clard.toml"), "dns_fake_ip_filter_mode = \"bogus\"\n").unwrap();
        let store = SettingsStore::open(dir.path()).unwrap();
        assert_eq!(store.get().dns_fake_ip_filter_mode, "blacklist");
    }

    #[test]
    fn zero_port_rejected() {
        let dir = tempdir().unwrap();
        let mut store = SettingsStore::open(dir.path()).unwrap();
        assert!(matches!(
            store.patch(&SettingsPatch {
                mixed_port: Some(0),
                ..Default::default()
            }),
            Err(SettingsError::InvalidPort(0))
        ));
    }
}
