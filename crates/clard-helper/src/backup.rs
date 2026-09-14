//! 本地备份/恢复（doc/05 §8）：打包 `/var/lib/clard` 的配置数据为 tar.gz，
//! 存 `/var/backups/clard/`（`CLARD_BACKUP_DIR` 可覆盖用于测试）。
//!
//! 打包范围：`profiles.yaml`、`profiles/`、`clard.toml`（运行态 runtime/ 与缓存不备份，
//! 恢复后由当前配置重新生成）。恢复时校验归档结构（拒绝路径穿越/超大）。

use std::{
    fs,
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use clard_proto::BackupItem;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use thiserror::Error;

/// 备份目录：`CLARD_BACKUP_DIR` 覆盖，默认 `/var/backups/clard`。
pub fn backup_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLARD_BACKUP_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from("/var/backups/clard")
}

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("IO 错误: {0}")]
    Io(#[from] io::Error),
    #[error("tar 处理失败: {0}")]
    Tar(String),
    #[error("备份不存在: {name}")]
    NotFound { name: String },
    #[error("归档含非法路径: {0}")]
    InvalidEntry(String),
    #[error("归档过大")]
    TooLarge,
}

/// tar 层错误统一包装。
fn tar_err(e: impl std::fmt::Display) -> BackupError {
    BackupError::Tar(e.to_string())
}

/// 允许打包/恢复的相对路径。
fn allowed_entry(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if path.is_absolute() || s.split('/').any(|c| c == "..") {
        return false;
    }
    if s == "profiles.yaml" || s == "clard.toml" {
        return true;
    }
    if let Some(rest) = s.strip_prefix("profiles/")
        && !rest.contains('/')
        && rest.ends_with(".yaml")
    {
        return true;
    }
    false
}

/// 创建备份（doc/05 §8 R8.1）。`dir` 为备份目录（生产用 `backup_dir()`，测试可注入）。
pub fn create(dir: &Path, state: &Path, name: Option<&str>) -> Result<BackupItem, BackupError> {
    fs::create_dir_all(dir)?;
    let name = name.map(str::to_string).unwrap_or_else(|| format!("backup-{}", now_unix()));
    let path = dir.join(format!("{name}.tar.gz"));

    let file = fs::File::create(&path)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = tar::Builder::new(enc);

    for rel in ["profiles.yaml", "clard.toml"] {
        let p = state.join(rel);
        if p.exists() {
            tar.append_path_with_name(&p, rel).map_err(tar_err)?;
        }
    }
    let profiles_dir = state.join("profiles");
    if profiles_dir.is_dir() {
        for entry in fs::read_dir(&profiles_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let rel = format!("profiles/{}", entry.file_name().to_string_lossy());
                tar.append_path_with_name(entry.path(), &rel).map_err(tar_err)?;
            }
        }
    }
    tar.finish().map_err(tar_err)?;
    let enc = tar.into_inner().map_err(tar_err)?;
    enc.finish().map_err(BackupError::Io)?;

    let size = fs::metadata(&path)?.len();
    Ok(BackupItem {
        name,
        created_at: now_unix(),
        size,
    })
}

/// 列出备份（按创建时间倒序）。
pub fn list(dir: &Path) -> Result<Vec<BackupItem>, BackupError> {
    let mut items = Vec::new();
    if !dir.is_dir() {
        return Ok(items);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "gz") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix(".tar"))
            .map(str::to_string)
            .unwrap_or_default();
        let meta = entry.metadata()?;
        let created_at = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        items.push(BackupItem {
            name,
            created_at,
            size: meta.len(),
        });
    }
    items.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(items)
}

/// 删除备份（doc/05 §8 R8.3）。
pub fn delete(dir: &Path, name: &str) -> Result<(), BackupError> {
    let path = dir.join(format!("{name}.tar.gz"));
    if !path.exists() {
        return Err(BackupError::NotFound { name: name.into() });
    }
    fs::remove_file(&path)?;
    Ok(())
}

/// 恢复备份（doc/05 §8 R8.2）：校验归档 → 解包到临时目录 → 校验 → 拷贝进 state。
/// 调用方需随后 reload 内存中的 stores。
pub fn restore(dir: &Path, state: &Path, name: &str) -> Result<(), BackupError> {
    const MAX_TOTAL: u64 = 64 * 1024 * 1024;
    let path = dir.join(format!("{name}.tar.gz"));
    if !path.exists() {
        return Err(BackupError::NotFound { name: name.into() });
    }

    let file = fs::File::open(&path)?;
    let dec = GzDecoder::new(file);
    let mut tar = tar::Archive::new(dec);

    // 第一遍：校验所有条目路径合法且总量受限
    let mut entries = Vec::new();
    let mut total = 0u64;
    for entry in tar.entries().map_err(tar_err)? {
        let entry = entry.map_err(tar_err)?;
        let p = entry.path().map_err(tar_err)?.into_owned();
        if !allowed_entry(&p) {
            return Err(BackupError::InvalidEntry(p.to_string_lossy().into_owned()));
        }
        total += entry.size();
        if total > MAX_TOTAL {
            return Err(BackupError::TooLarge);
        }
        entries.push(p);
    }

    // 第二遍：解包到临时目录后拷贝（避免直接按路径写 state）
    let file = fs::File::open(&path)?;
    let dec = GzDecoder::new(file);
    let mut tar = tar::Archive::new(dec);
    let tmp = state.join(".restore-tmp");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;
    tar.unpack(&tmp).map_err(tar_err)?;

    let result = (|| -> Result<(), BackupError> {
        for rel in &entries {
            let src = tmp.join(rel);
            if !src.exists() {
                continue;
            }
            let dst = state.join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src, &dst)?;
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
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

    fn seed_state(dir: &Path) {
        fs::create_dir_all(dir.join("profiles")).unwrap();
        fs::write(dir.join("profiles.yaml"), "current: null\nitems: []\n").unwrap();
        fs::write(dir.join("clard.toml"), "mixed_port = 7890\n").unwrap();
        fs::write(dir.join("profiles/R1.yaml"), "proxies: []\n").unwrap();
    }

    fn with_dirs() -> (tempfile::TempDir, tempfile::TempDir) {
        (tempdir().unwrap(), tempdir().unwrap())
    }

    #[test]
    fn create_list_restore_roundtrip() {
        let (state, backups) = with_dirs();
        seed_state(state.path());
        let dir = backups.path();

        let item = create(dir, state.path(), Some("v1")).unwrap();
        assert!(dir.join("v1.tar.gz").exists());
        assert_eq!(item.name, "v1");
        assert!(item.size > 0);

        let list = list(dir).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "v1");

        // 破坏 state 后恢复
        fs::write(state.path().join("profiles.yaml"), "corrupted").unwrap();
        fs::remove_file(state.path().join("clard.toml")).unwrap();
        restore(dir, state.path(), "v1").unwrap();

        assert!(state.path().join("profiles.yaml").exists());
        assert!(state.path().join("clard.toml").exists());
        assert!(state.path().join("profiles/R1.yaml").exists());
        assert_eq!(
            fs::read_to_string(state.path().join("profiles.yaml")).unwrap(),
            "current: null\nitems: []\n"
        );
    }

    #[test]
    fn allowed_entry_rejects_traversal_and_unknown_paths() {
        assert!(allowed_entry(Path::new("profiles.yaml")));
        assert!(allowed_entry(Path::new("clard.toml")));
        assert!(allowed_entry(Path::new("profiles/R1.yaml")));
        assert!(!allowed_entry(Path::new("../evil.txt")));
        assert!(!allowed_entry(Path::new("/etc/passwd")));
        assert!(!allowed_entry(Path::new("profiles/sub/x.yaml")), "profiles/ 下不允许子目录");
        assert!(!allowed_entry(Path::new("runtime/config.yaml")));
    }

    #[test]
    fn restore_rejects_subdir_entry() {
        let (state, backups) = with_dirs();
        seed_state(state.path());
        let dir = backups.path();

        // 构造含 profiles/ 子目录条目的归档（builder 允许，校验器拒绝）
        let tmp = state.path().join("sub");
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("x.yaml"), "oops").unwrap();
        let path = dir.join("evil.tar.gz");
        let file = fs::File::create(&path).unwrap();
        let enc = GzEncoder::new(file, Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(&tmp.join("x.yaml"), "profiles/sub/x.yaml")
            .unwrap();
        tar.finish().unwrap();
        let enc = tar.into_inner().unwrap();
        enc.finish().unwrap();

        let result = restore(dir, state.path(), "evil");
        eprintln!("restore result: {result:?}");
        assert!(matches!(result, Err(BackupError::InvalidEntry(_))));
    }

    #[test]
    fn delete_removes_and_missing_errors() {
        let (state, backups) = with_dirs();
        seed_state(state.path());
        let dir = backups.path();
        create(dir, state.path(), Some("a")).unwrap();
        delete(dir, "a").unwrap();
        assert!(matches!(delete(dir, "a"), Err(BackupError::NotFound { .. })));
    }
}
