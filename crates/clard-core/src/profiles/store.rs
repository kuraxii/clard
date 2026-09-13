//! 订阅配置（profiles）索引存储：加载/保存、增删改查、URL 导入与覆盖更新。
//!
//! 布局（doc/01 §3.3 / §7）：
//! - `$XDG_CONFIG_HOME/clard/profiles.yaml` —— 索引（`current` + `items`）
//! - `$XDG_CONFIG_HOME/clard/profiles/<uid>.yaml` —— 订阅内容文件
//!
//! 规则：
//! - 同 URL 再次导入 → **覆盖更新**（重下载、更新元数据、保留 uid/内容路径），不产生重复项；
//! - 写操作先落内容文件、再原子替换 `profiles.yaml`（tmp + rename）；
//! - 定时更新间隔字段存在，但定时器一期不实现（doc/01 §14 Q1：TUI 不在场不更新）。

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::download::{DownloadError, SubscriptionFetcher};

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
    #[error("下载失败: {0}")]
    Download(#[from] DownloadError),
    #[error("配置不存在: {uid}")]
    NotFound { uid: String },
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
    /// 订阅 URL（remote 必填；本期仅支持 remote）
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

/// 配置来源类型；本期仅 `remote`（URL 导入，doc/01 §7 非目标含 local 类型）
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

/// 配置索引存储。所有内容文件与索引都落在 `root`（XDG 配置根）下。
pub struct ProfilesStore {
    root: PathBuf,
    index: ProfilesIndex,
}

/// 默认配置根目录：`$XDG_CONFIG_HOME/clard`（缺省 `~/.config/clard`）
pub fn default_config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("clard");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config").join("clard");
    }
    PathBuf::from(".clard")
}

impl ProfilesStore {
    /// 打开索引；`root/profiles.yaml` 不存在则初始化为空索引（不落盘，首次变更时落盘）。
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

    pub fn get(&self, uid: &str) -> Option<&Profile> {
        self.index.items.iter().find(|p| p.uid == uid)
    }

    pub fn current(&self) -> Option<&Profile> {
        self.index.current.as_deref().and_then(|uid| self.get(uid))
    }

    /// 内容文件绝对路径（供后续 config_gen 读取）
    pub fn content_path(&self, uid: &str) -> Option<PathBuf> {
        self.get(uid).map(|p| self.root.join(&p.file))
    }

    /// 切换当前配置；uid 必须存在。
    pub fn set_current(&mut self, uid: &str) -> Result<(), ProfilesError> {
        if self.get(uid).is_none() {
            return Err(ProfilesError::NotFound { uid: uid.into() });
        }
        self.index.current = Some(uid.to_string());
        self.save_index()
    }

    /// 从 URL 导入订阅：
    /// - URL 已存在 → **覆盖更新**（重下载内容、更新名称/间隔/时间，保留 uid 与内容路径）；
    /// - 否则新建条目并写入内容文件。
    ///
    /// 下载失败时不做任何改动（无副作用）。
    pub async fn import<F: SubscriptionFetcher + ?Sized>(
        &mut self,
        url: &str,
        name: Option<&str>,
        interval: u64,
        fetcher: &F,
    ) -> Result<ImportOutcome, ProfilesError> {
        let content = fetcher.fetch(url).await?;
        let now = now_unix();

        if let Some(idx) = self.index.items.iter().position(|p| p.url == url) {
            // 覆盖更新：重写内容文件，保留 uid / file / selected
            let (uid, file) = {
                let item = &self.index.items[idx];
                (item.uid.clone(), item.file.clone())
            };
            write_file(&self.root.join(file), &content)?;
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
        write_file(&self.root.join(&file), &content)?;
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

    /// 重新下载并覆盖指定配置的内容文件，更新 `updated_at`。
    pub async fn update<F: SubscriptionFetcher + ?Sized>(
        &mut self,
        uid: &str,
        fetcher: &F,
    ) -> Result<(), ProfilesError> {
        let (url, file) = {
            let p = self
                .index
                .items
                .iter()
                .find(|p| p.uid == uid)
                .ok_or_else(|| ProfilesError::NotFound { uid: uid.into() })?;
            (p.url.clone(), p.file.clone())
        };
        let content = fetcher.fetch(&url).await?;
        write_file(&self.root.join(file), &content)?;
        if let Some(p) = self.index.items.iter_mut().find(|p| p.uid == uid) {
            p.updated_at = Some(now_unix());
        }
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

/// 生成 `R` + 12 位小写 hex（时间戳低 32 位 + 随机 16 位），同秒内也唯一。
fn gen_uid() -> String {
    let ts = now_unix() as u32;
    let rand: u16 = rand::rng().random();
    format!("R{:08x}{:04x}", ts, rand)
}

/// 缺省配置名：URL host（如 `link.xingvoy.com`）。
fn default_name(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "订阅".to_string())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use tempfile::tempdir;

    use super::*;

    /// 内存 mock 抓取器：按 URL 返回预置结果。
    struct MockFetcher {
        map: Arc<Mutex<HashMap<String, Result<String, DownloadError>>>>,
    }

    impl MockFetcher {
        fn new() -> Self {
            Self {
                map: Arc::new(Mutex::new(HashMap::new())),
            }
        }
        fn set(&self, url: &str, content: Result<&str, DownloadError>) {
            self.map
                .lock()
                .unwrap()
                .insert(url.to_string(), content.map(str::to_string));
        }
    }

    impl SubscriptionFetcher for MockFetcher {
        async fn fetch(&self, url: &str) -> Result<String, DownloadError> {
            self.map
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .unwrap_or_else(|| Err(DownloadError::Request("not configured".into())))
        }
    }

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
        assert!(!dir.path().join(PROFILES_FILE).exists(), "首次打开不应落盘");
    }

    #[test]
    fn uid_has_expected_shape() {
        let uid = gen_uid();
        assert!(uid.len() == 13 && uid.starts_with('R'), "got {uid}");
        assert!(uid[1..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn import_creates_profile_and_file() {
        let (_dir, mut store) = store_in_tempdir();
        let fetcher = MockFetcher::new();
        fetcher.set(URL_A, Ok("proxies: []\n"));
        let outcome = store.import(URL_A, None, 0, &fetcher).await.unwrap();
        let uid = match outcome {
            ImportOutcome::Created { uid } => uid,
            other => panic!("expected Created, got {other:?}"),
        };
        assert_eq!(store.list().len(), 1);
        let p = store.get(&uid).unwrap();
        assert_eq!(p.kind, ProfileKind::Remote);
        assert_eq!(p.name, "example.com", "缺省名取 URL host");
        assert!(p.updated_at.is_some());
        // 内容文件已落盘且内容一致
        let content = std::fs::read_to_string(store.content_path(&uid).unwrap()).unwrap();
        assert_eq!(content, "proxies: []\n");
    }

    #[tokio::test]
    async fn import_same_url_overwrites_instead_of_duplicate() {
        let (_dir, mut store) = store_in_tempdir();
        let fetcher = MockFetcher::new();
        fetcher.set(URL_A, Ok("v1"));
        let first = store.import(URL_A, Some("订阅A"), 0, &fetcher).await.unwrap();
        let first_uid = match first {
            ImportOutcome::Created { uid } => uid,
            other => panic!("expected Created, got {other:?}"),
        };
        // 同 URL 二次导入：内容与名称均覆盖，条目不新增
        fetcher.set(URL_A, Ok("v2"));
        let second = store.import(URL_A, Some("改名"), 0, &fetcher).await.unwrap();
        assert_eq!(second, ImportOutcome::Updated { uid: first_uid.clone() });
        assert_eq!(store.list().len(), 1, "同 URL 不得产生重复条目");
        let p = store.get(&first_uid).unwrap();
        assert_eq!(p.name, "改名");
        assert_eq!(
            std::fs::read_to_string(store.content_path(&first_uid).unwrap()).unwrap(),
            "v2"
        );
    }

    #[tokio::test]
    async fn import_different_urls_create_multiple() {
        let (_dir, mut store) = store_in_tempdir();
        let fetcher = MockFetcher::new();
        fetcher.set(URL_A, Ok("a"));
        fetcher.set(URL_B, Ok("b"));
        store.import(URL_A, None, 0, &fetcher).await.unwrap();
        store.import(URL_B, None, 0, &fetcher).await.unwrap();
        assert_eq!(store.list().len(), 2);
    }

    #[tokio::test]
    async fn import_failure_has_no_side_effects() {
        let (_dir, mut store) = store_in_tempdir();
        let fetcher = MockFetcher::new();
        fetcher.set(URL_A, Err(DownloadError::HttpStatus(500)));
        assert!(store.import(URL_A, None, 0, &fetcher).await.is_err());
        assert!(store.list().is_empty(), "失败不得留下索引或文件");
        assert!(!_dir.path().join(PROFILES_FILE).exists());
        assert!(!_dir.path().join(PROFILES_SUBDIR).exists());
    }

    #[tokio::test]
    async fn update_redownloads_content() {
        let (_dir, mut store) = store_in_tempdir();
        let fetcher = MockFetcher::new();
        fetcher.set(URL_A, Ok("v1"));
        let uid = match store.import(URL_A, None, 0, &fetcher).await.unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        let before = store.get(&uid).unwrap().updated_at;
        fetcher.set(URL_A, Ok("v2-new"));
        store.update(&uid, &fetcher).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(store.content_path(&uid).unwrap()).unwrap(),
            "v2-new"
        );
        assert!(store.get(&uid).unwrap().updated_at >= before);
    }

    #[tokio::test]
    async fn update_missing_uid_errors() {
        let (_dir, mut store) = store_in_tempdir();
        let fetcher = MockFetcher::new();
        let err = store.update("Rdeadbeef0000", &fetcher).await.unwrap_err();
        assert!(matches!(err, ProfilesError::NotFound { .. }));
    }

    #[tokio::test]
    async fn remove_deletes_entry_and_file() {
        let (_dir, mut store) = store_in_tempdir();
        let fetcher = MockFetcher::new();
        fetcher.set(URL_A, Ok("a"));
        let uid = match store.import(URL_A, None, 0, &fetcher).await.unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        let file = store.content_path(&uid).unwrap();
        assert!(file.exists());
        store.remove(&uid).unwrap();
        assert!(store.list().is_empty());
        assert!(!file.exists(), "内容文件应随删除一并移除");
    }

    #[tokio::test]
    async fn remove_missing_uid_errors() {
        let (_dir, mut store) = store_in_tempdir();
        assert!(matches!(
            store.remove("Rdeadbeef0000"),
            Err(ProfilesError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn set_current_and_remove_current_clears() {
        let (_dir, mut store) = store_in_tempdir();
        let fetcher = MockFetcher::new();
        fetcher.set(URL_A, Ok("a"));
        let uid = match store.import(URL_A, None, 0, &fetcher).await.unwrap() {
            ImportOutcome::Created { uid } => uid,
            other => panic!("{other:?}"),
        };
        store.set_current(&uid).unwrap();
        assert_eq!(store.current().unwrap().uid, uid);
        store.remove(&uid).unwrap();
        assert!(store.current().is_none(), "删掉当前配置后 current 应清空");
    }

    #[test]
    fn set_current_unknown_uid_errors() {
        let (_dir, mut store) = store_in_tempdir();
        assert!(matches!(
            store.set_current("Rdeadbeef0000"),
            Err(ProfilesError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn roundtrip_persists_index() {
        let dir = tempdir().unwrap();
        {
            let mut store = ProfilesStore::open(dir.path()).unwrap();
            let fetcher = MockFetcher::new();
            fetcher.set(URL_A, Ok("a"));
            store.import(URL_A, Some("订阅A"), 3600, &fetcher).await.unwrap();
            let uid = store.list()[0].uid.clone();
            store.set_current(&uid).unwrap();
        } // 关闭
        let store = ProfilesStore::open(dir.path()).unwrap();
        assert_eq!(store.list().len(), 1);
        let p = &store.list()[0];
        assert_eq!((p.url.as_str(), p.name.as_str(), p.interval), (URL_A, "订阅A", 3600));
        assert_eq!(store.current().unwrap().uid, p.uid);
        // 内容文件仍在
        assert!(std::fs::read_to_string(store.content_path(&p.uid).unwrap()).is_ok());
    }
}
