//! 订阅配置（profiles）存储：helper 侧管理（doc/01 §7）。
//!
//! 布局：
//! - `<state>/profiles.yaml` —— 索引（`current` + `items`）
//! - `<state>/profiles/<uid>.yaml` —— 订阅内容文件
//!
//! `state` 默认 `/var/lib/clard`（测试/开发可经 `CLARD_STATE_DIR` 覆盖）。
//! 规则：同 URL 再次导入 → **覆盖更新**（保留 uid/内容路径）；写操作原子替换索引。

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 索引文件名
pub const PROFILES_FILE: &str = "profiles.yaml";
/// 订阅内容子目录
pub const PROFILES_SUBDIR: &str = "profiles";

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

/// 配置索引存储；`root` 为状态目录（默认 `/var/lib/clard`）。
pub struct ProfilesStore {
    root: PathBuf,
    index: ProfilesIndex,
}

/// 状态目录：`CLARD_STATE_DIR` 覆盖，默认 `/var/lib/clard`。
pub fn state_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLARD_STATE_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from("/var/lib/clard")
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

    /// 导入订阅（yaml 由 TUI 下载并归一化后提交，§7.1）：
    /// 同 URL → 覆盖更新（保留 uid 与内容路径），否则新建。
    pub fn import(
        &mut self,
        url: &str,
        name: Option<&str>,
        interval: u64,
        yaml: &str,
    ) -> Result<ImportOutcome, ProfilesError> {
        let now = now_unix();
        if let Some(idx) = self.index.items.iter().position(|p| p.url == url) {
            let (uid, file) = {
                let item = &self.index.items[idx];
                (item.uid.clone(), item.file.clone())
            };
            write_file(&self.root.join(file), yaml)?;
            let item = &mut self.index.items[idx];
            if let Some(name) = name {
                item.name = name.to_string();
            }
            item.interval = interval;
            item.updated_at = Some(now);
            self.save_index()?;
            return Ok(ImportOutcome::Updated { uid });
        }
        let uid = gen_uid();
        let file = PathBuf::from(PROFILES_SUBDIR).join(format!("{uid}.yaml"));
        write_file(&self.root.join(&file), yaml)?;
        let name = name.map(str::to_string).unwrap_or_else(|| default_name(url));
        self.index.items.push(Profile {
            uid: uid.clone(),
            name,
            kind: ProfileKind::Remote,
            url: url.to_string(),
            file,
            updated_at: Some(now),
            interval,
            selected: Vec::new(),
        });
        self.save_index()?;
        Ok(ImportOutcome::Created { uid })
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

    fn save_index(&self) -> Result<(), ProfilesError> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join(PROFILES_FILE);
        let text = serde_yaml_ng::to_string(&self.index)?;
        atomic_write(&path, text.as_bytes())
    }
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
        let outcome = store.import(URL_A, None, 0, "proxies: []\n").unwrap();
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
        let first_uid = match store.import(URL_A, Some("订阅A"), 0, "v1").unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        let second = store.import(URL_A, Some("改名"), 3600, "v2").unwrap();
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
        store.import(URL_A, None, 0, "a").unwrap();
        store.import(URL_B, None, 0, "b").unwrap();
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn remove_deletes_entry_and_file() {
        let (_dir, mut store) = store_in_tempdir();
        let uid = match store.import(URL_A, None, 0, "a").unwrap() {
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
        let uid = match store.import(URL_A, None, 0, "a").unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        store.set_current(&uid).unwrap();
        assert_eq!(store.current().unwrap().uid, uid);
        store.remove(&uid).unwrap();
        assert!(store.current().is_none());
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
        let uid = match store.import(URL_A, Some("旧名"), 0, "a").unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        store.rename(&uid, "新名").unwrap();
        assert_eq!(store.get(&uid).unwrap().name, "新名");
        assert!(matches!(store.rename(&uid, "  "), Err(ProfilesError::EmptyName)));
    }

    #[test]
    fn move_item_swaps_order_and_bounds_are_noop() {
        let (_dir, mut store) = store_in_tempdir();
        let a = match store.import(URL_A, None, 0, "a").unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        let b = match store.import(URL_B, None, 0, "b").unwrap() {
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
            store.import(URL_A, Some("订阅A"), 3600, "a").unwrap();
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
