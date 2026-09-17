//! 日志读取与写入（doc/01 §10）：TUI 应用日志、核心 stdout 管道、审计 JSON lines。
//!
//! - TUI 应用日志经 `LogSubmit` 交 helper 落盘 `/var/clard/log/tui.log`（root 0600）。
//! - 核心 stdout 由 helper 逐行转储到 `/var/clard/log/core.log`。
//! - 审计双写 `/var/clard/log/audit.log`（JSON lines）。
//! - 轮转：应用 1MB×5，审计/核心 10MB×5（doc/01 §10）。

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use clard_proto::AuditRecord;

use crate::audit::log_dir;

/// 审计/核心日志轮转上限（doc/01 §10）。
pub const AUDIT_CORE_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// 日志源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSource {
    Tui,
    Core,
}

impl LogSource {
    fn file_name(self) -> &'static str {
        match self {
            Self::Tui => "tui.log",
            Self::Core => "core.log",
        }
    }
}

fn log_path(source: LogSource) -> PathBuf {
    log_dir().join(source.file_name())
}

/// 追加一行到 TUI 应用日志（R6.2，大小/份数可配：helper.toml，R7.5）。
pub fn append_tui_log(line: &str) -> std::io::Result<()> {
    let cfg = crate::helper_config::global();
    append_rotated(&log_path(LogSource::Tui), line, cfg.app_log_max_bytes, cfg.app_log_keep)
}

/// 追加一行到核心日志（stdout 管道转储，10MB×5 轮转）。
pub fn append_core_log(line: &str) -> std::io::Result<()> {
    append_rotated(&log_path(LogSource::Core), line, AUDIT_CORE_MAX_BYTES, 5)
}

/// 追加一行，超过上限先轮转（`x.log` → `x.log.1` … `x.log.{keep-1}`，最旧删除）。
pub fn append_rotated(path: &Path, line: &str, max_bytes: u64, keep: usize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(meta) = fs::metadata(path)
        && meta.len().saturating_add(line.len() as u64) > max_bytes
    {
        rotate(path, keep)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")
}

/// 轮转：删除最旧份，依次后移，当前文件 → `.log.1`。
fn rotate(path: &Path, keep: usize) -> std::io::Result<()> {
    let _ = fs::remove_file(path.with_extension(format!("log.{keep}")));
    for i in (1..keep).rev() {
        let from = path.with_extension(format!("log.{i}"));
        let to = path.with_extension(format!("log.{}", i + 1));
        if from.exists() {
            fs::rename(from, to)?;
        }
    }
    if path.exists() {
        fs::rename(path, path.with_extension("log.1"))?;
    }
    Ok(())
}

/// 按字节游标分页读取日志：返回 (新游标, 该段的行)。
pub fn tail(source: LogSource, cursor: u64) -> std::io::Result<(u64, Vec<String>)> {
    read_lines(&log_path(source), cursor)
}

/// 按字节游标分页读取审计日志并解析为记录（R6.3）。
pub fn audit_query(cursor: u64) -> std::io::Result<(u64, Vec<AuditRecord>)> {
    let path = log_dir().join("audit.log");
    let (next, lines) = read_lines(&path, cursor)?;
    let records = lines
        .iter()
        .filter_map(|line| serde_json::from_str::<AuditRecord>(line).ok())
        .collect();
    Ok((next, records))
}

/// 从 `cursor` 读到 EOF，返回 (文件新长度, 行)。
fn read_lines(path: &Path, cursor: u64) -> std::io::Result<(u64, Vec<String>)> {
    let Ok(mut file) = File::open(path) else {
        // 日志文件尚不存在：游标归零、空列表
        return Ok((0, Vec::new()));
    };
    let len = file.metadata()?.len();
    if cursor > len {
        return Ok((len, Vec::new()));
    }
    file.seek(SeekFrom::Start(cursor))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    let lines: Vec<String> = buf.lines().map(str::to_string).collect();
    Ok((len, lines))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn append_rotated_rotates_and_keeps_limited_backups() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("audit.log");

        // 每行 4 字节 + 换行 ≈ 5B；上限 12B → 第 3 行触发轮转
        append_rotated(&path, "aaaa", 12, 3).unwrap();
        append_rotated(&path, "bbbb", 12, 3).unwrap();
        append_rotated(&path, "cccc", 12, 3).unwrap(); // 触发：当前 → .1
        assert_eq!(fs::read_to_string(&path).unwrap(), "cccc\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("audit.log.1")).unwrap(),
            "aaaa\nbbbb\n"
        );

        append_rotated(&path, "dddd", 12, 3).unwrap();
        append_rotated(&path, "eeee", 12, 3).unwrap();
        append_rotated(&path, "ffff", 12, 3).unwrap(); // .1→.2，当前→.1
        assert!(dir.path().join("audit.log.1").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("audit.log.2")).unwrap(),
            "aaaa\nbbbb\n"
        );
        // keep=3：最多 .log.2，不会出现 .log.3
        assert!(!dir.path().join("audit.log.3").exists());
    }

    #[test]
    fn append_rotated_creates_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sub").join("tui.log");
        append_rotated(&path, "x", 100, 5).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "x\n");
    }
}
