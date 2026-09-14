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
pub const PROTO_VERSION: u32 = 8;

/// 协议层错误
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("协议版本不兼容：helper={helper}，客户端={client}")]
    VersionMismatch { helper: u32, client: u32 },
}

/// 配置组节点记忆（doc/05 §2 R2.2：切换配置后恢复 `PUT /proxies/:name`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSelection {
    pub group: String,
    pub node: String,
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
    /// 记忆的组节点选择（切换配置后恢复，doc/05 §2 R2.2）
    #[serde(default)]
    pub selected: Vec<NodeSelection>,
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
    /// 记忆当前配置的组节点选择（doc/05 §2 R2.2）
    ProfileMemorize { group: String, node: String },
    /// helper 系统配置（/etc/clard/helper.toml，R7.5：日志轮转/双写/核心日志级别）
    HelperConfigGet,
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
    /// TUI 应用日志交给 helper 落盘（/var/clard/log/tui.log）
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

/// 系统级设置（`/var/clard/lib/clard.toml`，doc/05 §7 R7.1/R7.2）。
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
    // ---- TUN（doc/05 §7 R7.2，托管固定值见 doc/01 §6.2）----
    /// TUN 开关（`SetTun` 持久化，doc/01 §5.6）
    pub tun_enabled: bool,
    /// TUN stack：system / gvisor / mixed（默认 system）
    pub tun_stack: String,
    /// TUN 下 DNS 模式：fake-ip / redir-host（默认 fake-ip，doc/01 §6.3）
    pub tun_dns_mode: String,
    /// dns-hijack 列表；空 = 用默认 `["any:53", "tcp://any:53"]`
    pub dns_hijack: Vec<String>,
    /// route-exclude-address；空 = 用默认私网段（doc/01 §6.7）
    pub route_exclude_address: Vec<String>,
    /// exclude-uid（该本地用户不被接管，doc/01 §6.7）
    pub exclude_uid: Vec<u32>,
    /// exclude-interface（该网卡不参与）
    pub exclude_interface: Vec<String>,
    /// exclude-dst-port（该目的端口不参与）
    pub exclude_dst_port: Vec<u16>,
    /// strict-route（默认禁用；开启需二次确认，残留即断网，doc/01 §6.3）
    pub strict_route: bool,
    /// auto-redirect（默认禁用；开启需二次确认，nftables 残留面，doc/01 §6.3）
    pub auto_redirect: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_update_interval_hours: 6,
            language: "en".into(),
            theme: "dark".into(),
            mixed_port: 7890,
            test_url: String::new(),
            tun_enabled: false,
            tun_stack: "system".into(),
            tun_dns_mode: "fake-ip".into(),
            dns_hijack: Vec::new(),
            route_exclude_address: Vec::new(),
            exclude_uid: Vec::new(),
            exclude_interface: Vec::new(),
            exclude_dst_port: Vec::new(),
            strict_route: false,
            auto_redirect: false,
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
    // ---- TUN（doc/05 §7 R7.2）----
    pub tun_enabled: Option<bool>,
    pub tun_stack: Option<String>,
    /// TUN 下 DNS 模式：fake-ip / redir-host
    pub tun_dns_mode: Option<String>,
    pub dns_hijack: Option<Vec<String>>,
    pub route_exclude_address: Option<Vec<String>>,
    pub exclude_uid: Option<Vec<u32>>,
    pub exclude_interface: Option<Vec<String>>,
    pub exclude_dst_port: Option<Vec<u16>>,
    pub strict_route: Option<bool>,
    pub auto_redirect: Option<bool>,
}

/// 审计操作者（`SO_PEERCRED` 记录，doc/01 §4.2）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    pub uid: u32,
    pub pid: i32,
}

/// 审计记录（audit.log JSON lines，doc/01 §10.2，R6.3）。
/// intent/result 双记录：同一次操作写两条（相同 `op_id`），`phase` 区分；
/// intent 行带操作前 net 快照，result 行带操作后 net 快照 + result/err。
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
    /// 同一次操作的关联 id（intent/result 配对，`I` 切换）
    #[serde(default)]
    pub op_id: String,
    /// `intent`（操作前）/ `result`（操作后）
    #[serde(default)]
    pub phase: String,
    /// 操作意图描述（如「切换配置到 R1」）
    #[serde(default)]
    pub intent: String,
    /// 网络快照（前后各一条；JSON：clard0 link / table 2023 / rule 9100）
    #[serde(default)]
    pub net: Option<String>,
    /// 配置 sha256（ApplyConfig 相关操作）
    #[serde(default)]
    pub cfg_sha256: Option<String>,
    /// 失败原因（result=error 时）
    #[serde(default)]
    pub err: Option<String>,
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

/// helper 系统配置（/etc/clard/helper.toml，doc/05 R7.5）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HelperConfig {
    /// 核心日志级别（注入 mihomo `log-level`）
    pub log_level: String,
    /// 应用日志轮转上限（字节）
    pub app_log_max_bytes: u64,
    /// 应用日志保留份数
    pub app_log_keep: usize,
    /// 审计保留份数
    pub audit_keep: usize,
    /// 审计是否双写 journald
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
        /// 已安装核心的 sha256（R7.3 展示；helper 侧 `core.sha256` 记录）
        core_sha256: Option<String>,
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
    /// helper 系统配置（R7.5）
    HelperConfig {
        config: HelperConfig,
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
    /// cleanup-tun 结果：clean=false 时 residuals 列出残余（doc/01 §6.4）
    CleanupResult {
        clean: bool,
        residuals: Vec<String>,
    },
    /// SetTun 结果：hot_reloaded=false = 核心未运行，仅落盘待启动生效（§5.6）
    TunSet {
        hot_reloaded: bool,
        verified: bool,
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
        assert_eq!(PROTO_VERSION, 8);
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
