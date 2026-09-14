//! 主页状态（doc/03 §5.1）：核心状态 / 后台服务 / 当前配置 / 流量摘要。
//!
//! 当前配置与流量摘要复用 Profiles/Connections 状态（单一数据源）；本模块只持有
//! 核心与 helper 状态（经 `Status`/`Hello` 拉取）。

/// 主页状态。
#[derive(Debug, Default)]
pub struct HomeState {
    pub core_state: Option<String>,
    pub core_pid: Option<u32>,
    pub core_version: Option<String>,
    pub helper_version: Option<String>,
    /// TUN 是否在工作（§6.5 判据）
    pub tun_active: bool,
}

impl HomeState {
    pub fn apply_core_status(
        &mut self,
        state: String,
        pid: Option<u32>,
        version: Option<String>,
        tun_active: bool,
    ) {
        self.core_state = Some(state);
        self.core_pid = pid;
        self.core_version = version;
        self.tun_active = tun_active;
    }

    pub fn apply_helper_version(&mut self, version: String) {
        self.helper_version = Some(version);
    }
}
