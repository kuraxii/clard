//! clard-proto：TUI ↔ helper 的 IPC 契约（两侧唯一耦合点）
//!
//! 方法清单与语义见 doc/01-方案设计.md §5.6；payload 类型随实现逐步补全。
//! 协议不兼容时 `Hello` 握手返回 [`ProtoError::VersionMismatch`]。
//!
//! 访问控制（doc/01 §4.2）：系统级服务、不分用户——socket 0666 默认全开放，
//! helper 用 `SO_PEERCRED` 记录 actor uid/pid 写审计，不做准入。

#![forbid(unsafe_code)]
#![warn(
    rust_2018_idioms,
    trivial_casts,
    unused_lifetimes,
    unused_qualifications,
    clippy::perf,
    clippy::style,
    clippy::redundant_closure
)]

use serde::{Deserialize, Serialize};

/// 当前协议版本。任何不兼容变更都必须递增并在 `Hello` 握手中核对。
pub const PROTO_VERSION: u32 = 4;

/// 协议层错误
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("协议版本不兼容：helper={helper}，客户端={client}")]
    VersionMismatch { helper: u32, client: u32 },
}

/// 订阅配置条目（helper 索引中的一项，doc/01 §7）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileItem {
    pub uid: String,
    pub name: String,
    pub url: String,
    /// 最近更新时间（unix 秒）
    pub updated_at: Option<i64>,
    /// 定时更新间隔（秒，0=关闭）
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
}

/// TUI → helper 的请求（对应 doc/01 §5.6）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// 握手：核对 proto_version，返回 helper 版本 / 能力位 / 当前状态
    Hello,
    /// 全量状态：核心状态 + TUN 状态 + 核心版本
    Status,
    /// 系统级设置读写（clard.toml，doc/01 §7）：混合端口 / 自动更新间隔 / 语言 / 主题
    SettingsGet,
    SettingsSet(SettingsPatch),
    /// 订阅配置：列表 / 导入（TUI 已下载并归一化）/ 取回内容 / 删除 / 切换 / 改名 / 排序
    ProfileList,
    ProfileImport(ProfileImport),
    ProfileGet { uid: String },
    ProfileRemove { uid: String },
    ProfileSetCurrent { uid: String },
    ProfileRename { uid: String, name: String },
    ProfileMove { uid: String, up: bool },
    ProfileHistory { uid: String },
    ProfileRestore { uid: String, version: u32 },
    /// 投递运行时配置 bundle（TUI config_gen 生成，§5.5）
    ApplyConfig { yaml: String },
    /// 本地备份（doc/05 §8）：创建 / 列表 / 删除 / 恢复
    BackupCreate { name: Option<String> },
    BackupList,
    BackupDelete { name: String },
    BackupRestore { name: String },
    /// 启停与重启核心（幂等）
    StartCore,
    StopCore,
    RestartCore,
    /// 开/关 TUN（托管字段注入 + 热重载 + 回读校验）
    SetTun { enable: bool },
    /// 手动兜底清理 TUN 残留（幂等）
    CleanupTun,
    /// 审计日志分页查询
    AuditQuery { cursor: u64 },
    /// 核心日志分页读取
    LogTail { source: String, cursor: u64 },
    /// TUI 应用日志交给 helper 落盘（/var/log/clard/tui.log）
    LogSubmit { line: String },
    /// 订阅事件流（断线重连后先 Status 全量同步再增量订阅）
    Subscribe,
    /// 升级核心（inbox 哈希校验 → 原子替换）
    InstallCore {
        inbox_path: String,
        sha256: String,
        version: String,
    },
}

/// 系统级设置（`/var/lib/clard/clard.toml`，doc/05 §7 R7.1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 自动更新间隔（小时），0=关闭，默认 6
    pub auto_update_interval_hours: u64,
    /// 语言：en / zh（默认英语，doc/05 §1 R1.3）
    pub language: String,
    /// 主题：dark / light
    pub theme: String,
    /// 混合端口（默认 7890，仅绑 127.0.0.1）
    pub mixed_port: u16,
    /// 自定义测速 URL（空 = 用 mihomo 内置，doc/05 §3 R3.4）
    pub test_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_update_interval_hours: 6,
            language: "en".into(),
            theme: "dark".into(),
            mixed_port: 7890,
            test_url: String::new(),
        }
    }
}

/// 设置补丁（只传变更字段）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsPatch {
    pub auto_update_interval_hours: Option<u64>,
    pub language: Option<String>,
    pub theme: Option<String>,
    pub mixed_port: Option<u16>,
    pub test_url: Option<String>,
}

/// 审计操作者（`SO_PEERCRED` 记录，doc/01 §4.2）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    pub uid: u32,
    pub pid: i32,
}

/// 审计记录（audit.log JSON lines，doc/01 §10.2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    #[serde(default)]
    pub ts: i64,
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub actor: AuditActor,
    #[serde(default)]
    pub result: String,
}

/// 备份条目（doc/05 §8）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupItem {
    pub name: String,
    pub created_at: i64,
    pub size: u64,
}

/// 配置历史版本条目（doc/05 §2 R2.9）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileVersion {
    pub version: u32,
    pub updated_at: Option<i64>,
}

/// 订阅流量/到期信息（`subscription-userinfo`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionInfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: Option<i64>,
}

/// 订阅导入请求：yaml 为 TUI 下载订阅后经 config_gen 归一化的内容（doc/01 §7.1）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileImport {
    pub name: Option<String>,
    pub url: String,
    pub interval: u64,
    pub yaml: String,
    /// 订阅流量/到期（可选，`subscription-userinfo`）。
    #[serde(default)]
    pub info: Option<SubscriptionInfo>,
}

/// helper → TUI 的响应
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Response {
    Hello {
        helper_version: String,
        proto_version: u32,
    },
    Status {
        core_state: String,
        core_pid: Option<u32>,
        core_version: Option<String>,
        tun_active: bool,
    },
    ProfileList {
        current: Option<String>,
        items: Vec<ProfileItem>,
    },
    ProfileImported {
        uid: String,
        /// true = 同 URL 覆盖更新
        updated: bool,
    },
    ProfileContent {
        item: ProfileItem,
        yaml: String,
    },
    ProfileHistory {
        versions: Vec<ProfileVersion>,
    },
    Settings {
        settings: Settings,
    },
    BackupList {
        backups: Vec<BackupItem>,
    },
    BackupCreated {
        item: BackupItem,
    },
    LogTail {
        cursor: u64,
        lines: Vec<String>,
    },
    AuditQuery {
        cursor: u64,
        records: Vec<AuditRecord>,
    },
    /// 无额外载荷的成功
    Ok,
    Error {
        message: String,
    },
}

impl Response {
    /// 便捷构造错误响应
    pub fn err(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}

/// helper → TUI 的事件推送（事件流里程碑补全 payload）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    CoreStatusChanged,
    TunChanged,
    LogLine,
    AuditLine,
    /// 核心反复崩溃，已 fail-open 恢复直连
    Degraded,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_version_is_current() {
        assert_eq!(PROTO_VERSION, 4);
    }

    #[test]
    fn request_response_roundtrip() {
        let req = Request::ProfileImport(ProfileImport {
            name: Some("订阅A".into()),
            url: "https://example.com/sub".into(),
            interval: 0,
            yaml: "proxies: []".into(),
            info: None,
        });
        let json = serde_json::to_vec(&req).unwrap();
        let back: Request = serde_json::from_slice(&json).unwrap();
        assert_eq!(req, back);

        let resp = Response::ProfileImported {
            uid: "R1".into(),
            updated: false,
        };
        let json = serde_json::to_vec(&resp).unwrap();
        let back: Response = serde_json::from_slice(&json).unwrap();
        assert_eq!(resp, back);
    }
}
