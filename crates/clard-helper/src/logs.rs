//! 日志读取与 TUI 应用日志落盘（doc/01 §10）。
//!
//! - TUI 应用日志经 `LogSubmit` 交 helper 落盘 `/var/clard/log/tui.log`（root 0600）。
//! - `LogTail` 按字节游标分页读取 tui.log / core.log。
//! - `AuditQuery` 按字节游标分页读取 audit.log 并解析为结构化记录（供 TUI 过滤/展示）。

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use clard_proto::AuditRecord;

use crate::audit::log_dir;

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

/// 追加一行到 TUI 应用日志（R6.2）。
pub fn append_tui_log(line: &str) -> std::io::Result<()> {
    let path = log_path(LogSource::Tui);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(f, "{line}")
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
