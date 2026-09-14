//! 订阅配置（profiles）存储：helper 侧管理（doc/01 §7）。
//!
//! 布局：
//! - `<state>/profiles.yaml` —— 索引（`current` + `items`）
//! - `<state>/profiles/<uid>.yaml` —— 订阅内容文件
//!
//! `state` 默认 `/var/clard/lib`（测试/开发可经 `CLARD_STATE_DIR` 覆盖）。
//! 规则：同 URL 再次导入 → **覆盖更新**（保留 uid/内容路径）；写操作原子替换索引。

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use clard_proto::{ProfileVersion, SubscriptionInfo};
use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 索引文件名
pub const PROFILES_FILE: &str = "profiles.yaml";
/// 订阅内容子目录
pub const PROFILES_SUBDIR: &str = "profiles";
/// 每配置保留的历史版本数（doc/05 §2 R2.9）
pub const MAX_BACKUPS: u32 = 3;

#[derive(Debug, Error)]
pub enum ProfilesError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("profiles.yaml 解析失败: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("配置不存在: {uid}")]
    NotFound { uid: String },
    #[error("配置名为空")]
    EmptyName,
    #[error("版本不存在: {version}")]
    BackupNotFound { version: u32 },
}

/// 索引文件结构（doc/01 §7.1）
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfilesIndex {
    /// 当前配置 uid
    pub current: Option<String>,
    pub items: Vec<Profile>,
}

/// 单条配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub uid: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ProfileKind,
    /// 订阅 URL（remote 必填；一期仅 remote）
    pub url: String,
    /// 内容文件相对路径（`profiles/<uid>.yaml`）
    pub file: PathBuf,
    /// 最近更新时间（unix 秒）
    pub updated_at: Option<i64>,
    /// 定时更新间隔（秒），0=关闭
    pub interval: u64,
    /// 订阅流量/到期（`subscription-userinfo`，doc/05 §2 R2.7）
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub expire: Option<i64>,
    /// 节点选择记忆（doc/01 §7.1）；本期恒为空，切换事务里程碑填充
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<NodeSelection>,
}

/// 节点选择记忆条目
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeSelection {
    pub group: String,
    pub node: String,
}

/// 配置来源类型；一期仅 `remote`
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    Remote,
}

/// `import` 的结果，用于区分「新建」与「同 URL 覆盖更新」
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    Created { uid: String },
    Updated { uid: String },
}

/// 配置索引存储；`root` 为状态目录（默认 `/var/clard/lib`）。
pub struct ProfilesStore {
    root: PathBuf,
    index: ProfilesIndex,
}

/// 状态目录：`CLARD_STATE_DIR` 覆盖，默认 `/var/clard/lib`（doc/01 §4，统一 /var/clard 子树）。
pub fn state_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLARD_STATE_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from("/var/clard/lib")
}

impl ProfilesStore {
    /// 打开索引；`root/profiles.yaml` 不存在则初始化为空索引（首次变更时落盘）。
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ProfilesError> {
        let root = root.into();
        let index = match std::fs::read_to_string(root.join(PROFILES_FILE)) {
            Ok(text) => serde_yaml_ng::from_str(&text)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProfilesIndex::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { root, index })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 从磁盘重新载入索引（备份恢复后调用）。
    pub fn reload(&mut self) -> Result<(), ProfilesError> {
        *self = Self::open(&self.root)?;
        Ok(())
    }

    pub fn list(&self) -> &[Profile] {
        &self.index.items
    }

    pub fn current(&self) -> Option<&Profile> {
        self.index.current.as_deref().and_then(|uid| self.get(uid))
    }

    pub fn get(&self, uid: &str) -> Option<&Profile> {
        self.index.items.iter().find(|p| p.uid == uid)
    }

    /// 内容文件绝对路径
    pub fn content_path(&self, uid: &str) -> Option<PathBuf> {
        self.get(uid).map(|p| self.root.join(&p.file))
    }

    /// 读取订阅内容（yaml 文本）
    pub fn content(&self, uid: &str) -> Result<String, ProfilesError> {
        let path = self
            .content_path(uid)
            .ok_or_else(|| ProfilesError::NotFound { uid: uid.into() })?;
        Ok(std::fs::read_to_string(path)?)
    }

    /// 切换当前配置；uid 必须存在。
    pub fn set_current(&mut self, uid: &str) -> Result<(), ProfilesError> {
        if self.get(uid).is_none() {
            return Err(ProfilesError::NotFound { uid: uid.into() });
        }
        self.index.current = Some(uid.to_string());
        self.save_index()
    }

    /// 记忆当前配置的组节点选择（doc/05 §2 R2.2，切换后恢复）。
    pub fn memorize(&mut self, group: &str, node: &str) -> Result<(), ProfilesError> {
        let uid = self
            .current()
            .ok_or_else(|| ProfilesError::NotFound { uid: "(无当前配置)".into() })?
            .uid
            .clone();
        let p = self
            .index
            .items
            .iter_mut()
            .find(|p| p.uid == uid)
            .ok_or_else(|| ProfilesError::NotFound { uid: uid.clone() })?;
        if let Some(s) = p.selected.iter_mut().find(|s| s.group == group) {
            s.node = node.to_string();
        } else {
            p.selected.push(NodeSelection {
                group: group.to_string(),
                node: node.to_string(),
            });
        }
        self.save_index()
    }

    /// 导入订阅（yaml 由 TUI 下载并归一化后提交，§7.1）：
    /// 同 URL → 覆盖更新（保留 uid 与内容路径），否则新建。
    pub fn import(
        &mut self,
        url: &str,
        name: Option<&str>,
        interval: u64,
        yaml: &str,
        info: Option<SubscriptionInfo>,
    ) -> Result<ImportOutcome, ProfilesError> {
        let now = now_unix();
        if let Some(idx) = self.index.items.iter().position(|p| p.url == url) {
            let (uid, file) = {
                let item = &self.index.items[idx];
                (item.uid.clone(), item.file.clone())
            };
            // 覆盖前先把旧内容轮转为备份（R2.9）
            self.rotate_backups(&self.root.join(&file))?;
            write_file(&self.root.join(file), yaml)?;
            let item = &mut self.index.items[idx];
            if let Some(name) = name {
                item.name = name.to_string();
            }
            item.interval = interval;
            item.updated_at = Some(now);
            if let Some(info) = info {
                item.upload = info.upload;
                item.download = info.download;
                item.total = info.total;
                item.expire = info.expire;
            }
            self.save_index()?;
            return Ok(ImportOutcome::Updated { uid });
        }
        let uid = gen_uid();
        let file = PathBuf::from(PROFILES_SUBDIR).join(format!("{uid}.yaml"));
        write_file(&self.root.join(&file), yaml)?;
        let name = name.map(str::to_string).unwrap_or_else(|| default_name(url));
        let (upload, download, total, expire) = info.map_or((0, 0, 0, None), |i| {
            (i.upload, i.download, i.total, i.expire)
        });
        self.index.items.push(Profile {
            uid: uid.clone(),
            name,
            kind: ProfileKind::Remote,
            url: url.to_string(),
            file,
            updated_at: Some(now),
            interval,
            upload,
            download,
            total,
            expire,
            selected: Vec::new(),
        });
        self.save_index()?;
        Ok(ImportOutcome::Created { uid })
    }

    /// 自动更新单条 remote 订阅（R2.8）：内容原样覆盖落盘、刷新流量/到期与 `updated_at`，
    /// 保留 uid/名称/间隔。返回是否命中（按 URL 定位）。
    pub fn auto_update(
        &mut self,
        url: &str,
        yaml: &str,
        info: Option<SubscriptionInfo>,
    ) -> Result<bool, ProfilesError> {
        let Some(idx) = self.index.items.iter().position(|p| p.url == url) else {
            return Ok(false);
        };
        let file = self.index.items[idx].file.clone();
        write_file(&self.root.join(file), yaml)?;
        let item = &mut self.index.items[idx];
        item.updated_at = Some(now_unix());
        if let Some(info) = info {
            item.upload = info.upload;
            item.download = info.download;
            item.total = info.total;
            item.expire = info.expire;
        }
        self.save_index()?;
        Ok(true)
    }

    /// 改名（doc/05 §2 R2.5）。
    pub fn rename(&mut self, uid: &str, name: &str) -> Result<(), ProfilesError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProfilesError::EmptyName);
        }
        let Some(item) = self.index.items.iter_mut().find(|p| p.uid == uid) else {
            return Err(ProfilesError::NotFound { uid: uid.into() });
        };
        item.name = name.to_string();
        self.save_index()
    }

    /// 上移/下移（doc/05 §2 R2.6）；到边界时无副作用。
    pub fn move_item(&mut self, uid: &str, up: bool) -> Result<(), ProfilesError> {
        let Some(idx) = self.index.items.iter().position(|p| p.uid == uid) else {
            return Err(ProfilesError::NotFound { uid: uid.into() });
        };
        let target = if up {
            idx.checked_sub(1)
        } else {
            (idx + 1 < self.index.items.len()).then_some(idx + 1)
        };
        let Some(target) = target else {
            return Ok(());
        };
        self.index.items.swap(idx, target);
        self.save_index()
    }

    /// 删除配置：移除索引条目并删除内容文件；若删的是当前配置则清空 `current`。
    pub fn remove(&mut self, uid: &str) -> Result<(), ProfilesError> {
        let Some(idx) = self.index.items.iter().position(|p| p.uid == uid) else {
            return Err(ProfilesError::NotFound { uid: uid.into() });
        };
        let removed = self.index.items.remove(idx);
        let _ = std::fs::remove_file(self.root.join(&removed.file));
        if self.index.current.as_deref() == Some(uid) {
            self.index.current = None;
        }
        self.save_index()
    }

    /// 列出配置的历史版本（1=最近，最多 `MAX_BACKUPS` 份）。
    pub fn history(&self, uid: &str) -> Result<Vec<ProfileVersion>, ProfilesError> {
        let path = self
            .content_path(uid)
            .ok_or_else(|| ProfilesError::NotFound { uid: uid.into() })?;
        let mut versions = Vec::new();
        for n in 1..=MAX_BACKUPS {
            let backup = backup_path(&path, n);
            if backup.exists() {
                let updated_at = backup
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(modified_to_unix);
                versions.push(ProfileVersion { version: n, updated_at });
            }
        }
        Ok(versions)
    }

    /// 恢复指定历史版本：内容写回主文件并更新索引 `updated_at`（R2.9）。
    pub fn restore(&mut self, uid: &str, version: u32) -> Result<(), ProfilesError> {
        if version == 0 || version > MAX_BACKUPS {
            return Err(ProfilesError::BackupNotFound { version });
        }
        let Some(item) = self.index.items.iter().find(|p| p.uid == uid) else {
            return Err(ProfilesError::NotFound { uid: uid.into() });
        };
        let path = self.root.join(&item.file);
        let backup = backup_path(&path, version);
        if !backup.exists() {
            return Err(ProfilesError::BackupNotFound { version });
        }
        let content = std::fs::read_to_string(&backup)?;
        // 恢复前同样轮转，保证恢复动作本身可逆
        self.rotate_backups(&path)?;
        write_file(&path, &content)?;
        let item = self
            .index
            .items
            .iter_mut()
            .find(|p| p.uid == uid)
            .expect("uid 已校验存在");
        item.updated_at = Some(now_unix());
        self.save_index()
    }

    /// 轮转备份：`bak.2→bak.3`、`bak.1→bak.2`、当前内容 → `bak.1`（最多 3 份）。
    fn rotate_backups(&self, path: &Path) -> Result<(), ProfilesError> {
        let newest = backup_path(path, MAX_BACKUPS);
        let _ = std::fs::remove_file(&newest);
        for n in (1..MAX_BACKUPS).rev() {
            let src = backup_path(path, n);
            let dst = backup_path(path, n + 1);
            if src.exists() {
                std::fs::rename(&src, &dst)?;
            }
        }
        if path.exists() {
            std::fs::copy(path, backup_path(path, 1))?;
        }
        Ok(())
    }

    fn save_index(&self) -> Result<(), ProfilesError> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join(PROFILES_FILE);
        let text = serde_yaml_ng::to_string(&self.index)?;
        atomic_write(&path, text.as_bytes())
    }
}

/// 历史版本文件路径：`<主文件>.yaml.bak.<n>`。
fn backup_path(path: &Path, n: u32) -> PathBuf {
    path.with_extension(format!("yaml.bak.{n}"))
}

fn modified_to_unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn write_file(path: &Path, content: &str) -> Result<(), ProfilesError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

/// 原子写：先写临时文件再 rename，避免索引文件读到半截。
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), ProfilesError> {
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 生成 `R` + 12 位小写 hex（时间戳低 32 位 + 随机 16 位）。
fn gen_uid() -> String {
    let ts = now_unix() as u32;
    let rand: u16 = rand::rng().random();
    format!("R{:08x}{:04x}", ts, rand)
}

/// 缺省配置名：URL host。
fn default_name(url: &str) -> String {
    reqwest_url_host(url).unwrap_or_else(|| "订阅".to_string())
}

fn reqwest_url_host(url: &str) -> Option<String> {
    // 避免 helper 引入 reqwest 依赖：轻量提取 host
    let after = url.split("://").nth(1)?;
    let host = after.split(['/', '?', '#']).next()?;
    Some(host.to_string())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    const URL_A: &str = "https://example.com/sub-a";
    const URL_B: &str = "https://example.com/sub-b";

    fn store_in_tempdir() -> (tempfile::TempDir, ProfilesStore) {
        let dir = tempdir().unwrap();
        let store = ProfilesStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn open_missing_index_yields_empty() {
        let dir = tempdir().unwrap();
        let store = ProfilesStore::open(dir.path()).unwrap();
        assert!(store.list().is_empty());
        assert!(store.current().is_none());
        assert!(!dir.path().join(PROFILES_FILE).exists());
    }

    #[test]
    fn uid_has_expected_shape() {
        let uid = gen_uid();
        assert!(uid.len() == 13 && uid.starts_with('R'), "got {uid}");
        assert!(uid[1..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn import_creates_profile_and_file() {
        let (_dir, mut store) = store_in_tempdir();
        let outcome = store.import(URL_A, None, 0, "proxies: []\n", None).unwrap();
        let uid = match outcome {
            ImportOutcome::Created { uid } => uid,
            other => panic!("expected Created, got {other:?}"),
        };
        assert_eq!(store.list().len(), 1);
        let p = store.get(&uid).unwrap();
        assert_eq!(p.kind, ProfileKind::Remote);
        assert_eq!(p.name, "example.com", "缺省名取 URL host");
        assert_eq!(store.content(&uid).unwrap(), "proxies: []\n");
    }

    #[test]
    fn import_same_url_overwrites_instead_of_duplicate() {
        let (_dir, mut store) = store_in_tempdir();
        let first_uid = match store.import(URL_A, Some("订阅A"), 0, "v1", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        let second = store.import(URL_A, Some("改名"), 3600, "v2", None).unwrap();
        assert_eq!(second, ImportOutcome::Updated { uid: first_uid.clone() });
        assert_eq!(store.list().len(), 1, "同 URL 不得重复");
        let p = store.get(&first_uid).unwrap();
        assert_eq!(p.name, "改名");
        assert_eq!(p.interval, 3600);
        assert_eq!(store.content(&first_uid).unwrap(), "v2");
    }

    #[test]
    fn import_different_urls_create_multiple() {
        let (_dir, mut store) = store_in_tempdir();
        store.import(URL_A, None, 0, "a", None).unwrap();
        store.import(URL_B, None, 0, "b", None).unwrap();
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn remove_deletes_entry_and_file() {
        let (_dir, mut store) = store_in_tempdir();
        let uid = match store.import(URL_A, None, 0, "a", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        let file = store.content_path(&uid).unwrap();
        assert!(file.exists());
        store.remove(&uid).unwrap();
        assert!(store.list().is_empty());
        assert!(!file.exists());
    }

    #[test]
    fn set_current_and_remove_current_clears() {
        let (_dir, mut store) = store_in_tempdir();
        let uid = match store.import(URL_A, None, 0, "a", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        store.set_current(&uid).unwrap();
        assert_eq!(store.current().unwrap().uid, uid);
        store.remove(&uid).unwrap();
        assert!(store.current().is_none());
    }

    #[test]
    fn memorize_persists_selection_to_current_profile() {
        let (dir, mut store) = store_in_tempdir();
        let uid = match store.import(URL_A, None, 0, "a", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        // 无当前配置 → 拒绝
        assert!(store.memorize("G1", "N1").is_err());

        store.set_current(&uid).unwrap();
        store.memorize("G1", "N1").unwrap();
        store.memorize("G2", "N2").unwrap();
        // 同组覆盖
        store.memorize("G1", "N1b").unwrap();
        let p = store.get(&uid).unwrap();
        let sel: Vec<_> = p.selected.iter().map(|s| (s.group.as_str(), s.node.as_str())).collect();
        assert_eq!(sel, vec![("G1", "N1b"), ("G2", "N2")]);

        // 重开同一目录（持久化验证）
        let reopened = ProfilesStore::open(dir.path()).unwrap();
        let p = reopened.get(&uid).unwrap();
        let sel: Vec<_> = p.selected.iter().map(|s| (s.group.as_str(), s.node.as_str())).collect();
        assert_eq!(sel, vec![("G1", "N1b"), ("G2", "N2")]);
    }

    #[test]
    fn unknown_uid_errors() {
        let (_dir, mut store) = store_in_tempdir();
        assert!(matches!(store.set_current("nope"), Err(ProfilesError::NotFound { .. })));
        assert!(matches!(store.remove("nope"), Err(ProfilesError::NotFound { .. })));
        assert!(matches!(store.content("nope"), Err(ProfilesError::NotFound { .. })));
    }

    #[test]
    fn rename_updates_name_and_rejects_empty() {
        let (_dir, mut store) = store_in_tempdir();
        let uid = match store.import(URL_A, Some("旧名"), 0, "a", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        store.rename(&uid, "新名").unwrap();
        assert_eq!(store.get(&uid).unwrap().name, "新名");
        assert!(matches!(store.rename(&uid, "  "), Err(ProfilesError::EmptyName)));
    }

    #[test]
    fn history_and_restore_keep_three_versions() {
        let (_dir, mut store) = store_in_tempdir();
        let uid = match store.import(URL_A, Some("订阅"), 0, "v1", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        assert!(store.history(&uid).unwrap().is_empty());

        store.import(URL_A, None, 0, "v2", None).unwrap(); // v1 → bak.1
        assert_eq!(store.history(&uid).unwrap().len(), 1);

        store.import(URL_A, None, 0, "v3", None).unwrap(); // bak.1=v2, bak.2=v1
        assert_eq!(store.history(&uid).unwrap().len(), 2);
        assert_eq!(store.content(&uid).unwrap(), "v3");

        store.restore(&uid, 1).unwrap(); // bak.1 = v2
        assert_eq!(store.content(&uid).unwrap(), "v2");

        for v in ["v4", "v5", "v6"] {
            store.import(URL_A, None, 0, v, None).unwrap();
        }
        assert_eq!(store.history(&uid).unwrap().len(), 3, "最多保留 3 份");
    }

    #[test]
    fn restore_unknown_version_errors() {
        let (_dir, mut store) = store_in_tempdir();
        let uid = match store.import(URL_A, None, 0, "v1", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            store.restore(&uid, 1),
            Err(ProfilesError::BackupNotFound { version: 1 })
        ));
        assert!(matches!(
            store.restore(&uid, 99),
            Err(ProfilesError::BackupNotFound { version: 99 })
        ));
    }

    #[test]
    fn auto_update_overwrites_and_preserves_identity() {
        let (_dir, mut store) = store_in_tempdir();
        let uid = match store.import(URL_A, Some("订阅A"), 3600, "old", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        let info = SubscriptionInfo {
            upload: 1,
            download: 2,
            total: 3,
            expire: Some(9),
        };
        assert!(store.auto_update(URL_A, "new", Some(info)).unwrap());
        let p = store.get(&uid).unwrap();
        assert_eq!(p.name, "订阅A", "保留名称");
        assert_eq!(p.interval, 3600, "保留间隔");
        assert_eq!((p.upload, p.download, p.total), (1, 2, 3));
        assert_eq!(store.content(&uid).unwrap(), "new");

        assert!(!store.auto_update("https://example.com/unknown", "x", None).unwrap());
    }

    #[test]
    fn move_item_swaps_order_and_bounds_are_noop() {
        let (_dir, mut store) = store_in_tempdir();
        let a = match store.import(URL_A, None, 0, "a", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        let b = match store.import(URL_B, None, 0, "b", None).unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        assert_eq!(store.list()[0].uid, a);

        store.move_item(&a, false).unwrap(); // a 下移 → [b, a]
        assert_eq!(store.list()[0].uid, b);
        assert_eq!(store.list()[1].uid, a);

        store.move_item(&b, true).unwrap(); // b 已在顶，上移无副作用
        assert_eq!(store.list()[0].uid, b);
    }

    #[test]
    fn roundtrip_persists_index() {
        let dir = tempdir().unwrap();
        {
            let mut store = ProfilesStore::open(dir.path()).unwrap();
            store.import(URL_A, Some("订阅A"), 3600, "a", None).unwrap();
            let uid = store.list()[0].uid.clone();
            store.set_current(&uid).unwrap();
        }
        let store = ProfilesStore::open(dir.path()).unwrap();
        assert_eq!(store.list().len(), 1);
        let p = &store.list()[0];
        assert_eq!((p.url.as_str(), p.name.as_str(), p.interval), (URL_A, "订阅A", 3600));
        assert_eq!(store.current().unwrap().uid, p.uid);
    }

    #[test]
    fn default_name_from_url_without_reqwest() {
        assert_eq!(default_name("https://link.xingvoy.com/api/sub?x=1"), "link.xingvoy.com");
        assert_eq!(default_name("not a url"), "订阅");
    }
}
