//! 系统级设置存储（`<state>/clard.toml`，doc/01 §7 / doc/05 §7 R7.1）。
//!
//! 默认：自动更新间隔 6 小时（0=关）、语言 en、主题 dark、混合端口 7890（仅回环）。
//! 写操作原子替换索引式落盘。

use std::{
    fs,
    io,
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

impl SettingsStore {
    /// 打开设置；文件不存在则用默认值（首次写时落盘）。
    pub fn open(root: &Path) -> Result<Self, SettingsError> {
        let path = root.join(SETTINGS_FILE);
        let settings = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path, settings })
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// 从磁盘重新载入（备份恢复后调用）。
    pub fn reload(&mut self) -> Result<(), SettingsError> {
        let root = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        *self = Self::open(&root)?;
        Ok(())
    }

    /// 应用补丁并原子落盘。
    pub fn patch(&mut self, patch: &SettingsPatch) -> Result<(), SettingsError> {
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
        if let Some(v) = &patch.dns_hijack {
            self.settings.dns_hijack = v.clone();
        }
        if let Some(v) = &patch.route_exclude_address {
            self.settings.route_exclude_address = v.clone();
        }
        if let Some(v) = &patch.exclude_uid {
            self.settings.exclude_uid = v.clone();
        }
        if let Some(v) = &patch.exclude_interface {
            self.settings.exclude_interface = v.clone();
        }
        if let Some(v) = &patch.exclude_dst_port {
            self.settings.exclude_dst_port = v.clone();
        }
        if let Some(v) = patch.strict_route {
            self.settings.strict_route = v;
        }
        if let Some(v) = patch.auto_redirect {
            self.settings.auto_redirect = v;
        }
        self.save()
    }

    fn save(&self) -> Result<(), SettingsError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(&self.settings)?;
        let tmp = self.path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
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
        assert_eq!(s.tun_stack, "system");
        assert!(s.route_exclude_address.is_empty(), "空 = 用默认私网段");
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
                    test_url: Some("http://x".into()),
                    tun_enabled: Some(true),
                    tun_stack: Some("gvisor".into()),
                    dns_hijack: Some(vec!["any:53".into()]),
                    route_exclude_address: Some(vec!["10.0.0.0/8".into()]),
                    exclude_uid: Some(vec![1000]),
                    exclude_interface: Some(vec!["eth1".into()]),
                    exclude_dst_port: Some(vec![5353]),
                    strict_route: Some(false),
                    auto_redirect: Some(true),
                })
                .unwrap();
        }
        let store = SettingsStore::open(dir.path()).unwrap();
        let s = store.get();
        assert_eq!(s.auto_update_interval_hours, 12);
        assert_eq!(s.language, "zh");
        assert_eq!(s.theme, "dark", "未补丁字段保持默认");
        assert_eq!(s.mixed_port, 7891);
        assert!(s.tun_enabled);
        assert_eq!(s.tun_stack, "gvisor");
        assert_eq!(s.dns_hijack, vec!["any:53"]);
        assert_eq!(s.route_exclude_address, vec!["10.0.0.0/8"]);
        assert_eq!(s.exclude_uid, vec![1000]);
        assert_eq!(s.exclude_interface, vec!["eth1"]);
        assert_eq!(s.exclude_dst_port, vec![5353]);
        assert!(!s.strict_route);
        assert!(s.auto_redirect);
    }

    #[test]
    fn zero_port_rejected() {
        let dir = tempdir().unwrap();
        let mut store = SettingsStore::open(dir.path()).unwrap();
        assert!(matches!(
            store.patch(&SettingsPatch { mixed_port: Some(0), ..Default::default() }),
            Err(SettingsError::InvalidPort(0))
        ));
    }
}
