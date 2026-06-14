use std::{fs, process::Command};

use crate::{
    app::checker::IPInfo,
    ipc::models::{BackendVersion, Connections, Traffic},
};

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub distribution: String,
    pub kernel_version: String,
    pub verge_version: String,
}

#[derive(Debug)]
pub struct PreviewState {
    pub version: Option<BackendVersion>,
    pub system_info: SystemInfo,
    pub direct_ip: Option<IPInfo>,
    pub proxy_ip: Option<IPInfo>,
    pub connections: Option<Connections>,
    pub traffic_history: Vec<Traffic>,
    pub current_traffic: Option<Traffic>,
}

impl PreviewState {
    pub fn new() -> Self {
        Self {
            version: None,
            system_info: detect_system_info(),
            direct_ip: None,
            proxy_ip: None,
            connections: None,
            traffic_history: Vec::new(),
            current_traffic: None,
        }
    }

    pub fn update_version(&mut self, version: BackendVersion) {
        self.version = Some(version);
    }

    pub fn update_connections(&mut self, connections: Connections) {
        self.connections = Some(connections);
    }

    pub fn update_ip_info(&mut self, direct_ip: Option<IPInfo>, proxy_ip: Option<IPInfo>) {
        self.direct_ip = direct_ip;
        self.proxy_ip = proxy_ip;
    }

    pub fn update_traffic(&mut self, traffic: Traffic) {
        self.current_traffic = Some(traffic.clone());
        self.traffic_history.push(traffic);
        if self.traffic_history.len() > 60 {
            self.traffic_history.remove(0);
        }
    }
}

impl Default for PreviewState {
    fn default() -> Self {
        Self::new()
    }
}

fn detect_system_info() -> SystemInfo {
    let distribution = detect_distribution();
    let kernel_version = detect_kernel_version();
    let verge_version = env!("CARGO_PKG_VERSION").to_string();

    SystemInfo {
        distribution,
        kernel_version,
        verge_version,
    }
}

fn detect_distribution() -> String {
    let content = match fs::read_to_string("/etc/os-release") {
        Ok(content) => content,
        Err(_) => return "Unknown".to_string(),
    };

    for line in content.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return value.trim_matches('"').to_string();
        }
    }

    for line in content.lines() {
        if let Some(value) = line.strip_prefix("NAME=") {
            return value.trim_matches('"').to_string();
        }
    }

    "Unknown".to_string()
}

fn detect_kernel_version() -> String {
    match Command::new("uname").arg("-r").output() {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).trim().to_string(),
        _ => "Unknown".to_string(),
    }
}
