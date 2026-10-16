//! 系统信息（doc/03 §5.1 Home「System」面板，doc/05 R1.1a）。
//!
//! 非 clard 数据：TUI 直接读系统文件，不走 IPC（clard 相关数据才走 IPC）。
//! 来源：`/etc/os-release`、`/proc/sys/kernel/osrelease`、`/proc/cpuinfo`、
//! `/proc/meminfo`、`/sys/class/drm/card*/device`、`/proc/net/route`；
//! DHCP DNS 与网关同网卡对应：`nmcli` → 兜底 `resolvectl`（纯只读展示，不参与解析）。
//! 解析函数全部纯函数化（输入文本输出结果），便于单测。

use std::{fs, path::PathBuf, process::Command};

/// 一块物理网卡的网络信息（网关 + 其自身 DHCP DNS，二者同网卡对应）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetIf {
    pub iface: String,
    /// 默认路由网关 IP
    pub gateway: String,
    /// 该网卡的 DHCP DNS（`nmcli`/`resolvectl`，纯只读；可能为空）
    pub dns: Vec<String>,
}

/// 主页 System 面板数据。
///
/// 静态字段（os/kernel/cpu/gpu）理论上不变；内存/网络随运行变化，调用方按周期
/// 重新 [`Self::collect`] 刷新。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SysInfo {
    /// `/etc/os-release` PRETTY_NAME（读不到为 "Unknown OS"）
    pub os: String,
    /// 内核版本（`/proc/sys/kernel/osrelease` 首行）
    pub kernel: String,
    /// CPU 型号（`/proc/cpuinfo` 第一个 model name）
    pub cpu: String,
    /// GPU 名称（`/sys/class/drm/card*/device` 厂商映射；无 GPU 为空）
    pub gpu: String,
    /// 内存总量（KiB）
    pub mem_total_kib: u64,
    /// 可用内存（KiB）
    pub mem_available_kib: u64,
    /// 物理网卡网关 + DHCP DNS（多网卡各自一行；虚拟/TUN 网卡已排除）
    pub nets: Vec<NetIf>,
}

impl SysInfo {
    /// 采集一次全量系统信息。
    pub fn collect() -> Self {
        let os = fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|t| os_release_value(&t, "PRETTY_NAME"))
            .unwrap_or_else(|| "Unknown OS".into());
        let kernel = read_first_line("/proc/sys/kernel/osrelease").unwrap_or_default();
        let cpu = fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|t| cpu_model(&t))
            .unwrap_or_default();
        let mem = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        // 多网卡聚合：所有默认路由的物理网卡，各自带网关 + DHCP DNS
        let nets = fs::read_to_string("/proc/net/route")
            .ok()
            .map(|t| default_routes(&t))
            .unwrap_or_default()
            .into_iter()
            .map(|(iface, gateway)| NetIf {
                iface: iface.clone(),
                gateway,
                dns: dns_of_iface(&iface),
            })
            .collect();
        Self {
            os,
            kernel,
            cpu,
            gpu: read_gpu(),
            mem_total_kib: meminfo_kib(&mem, "MemTotal").unwrap_or(0),
            mem_available_kib: meminfo_kib(&mem, "MemAvailable").unwrap_or(0),
            nets,
        }
    }
}

/// 取某网卡的 DHCP DNS：`nmcli -g IP4.DNS` → 兜底 `resolvectl status <iface>`。
/// 纯只读（只展示，不参与 mihomo DNS 配置/解析）。
fn dns_of_iface(iface: &str) -> Vec<String> {
    if let Ok(o) = Command::new("nmcli")
        .args(["-g", "IP4.DNS", "dev", "show", iface])
        .output()
        && o.status.success()
    {
        let v = parse_dns_list(&String::from_utf8_lossy(&o.stdout));
        if !v.is_empty() {
            return v;
        }
    }
    if let Ok(o) = Command::new("resolvectl").args(["status", iface]).output()
        && o.status.success()
    {
        let v = parse_resolvectl_dns(&String::from_utf8_lossy(&o.stdout));
        if !v.is_empty() {
            return v;
        }
    }
    Vec::new()
}

/// nmcli `-g IP4.DNS` 输出 → DNS 列表（`a | b` 管道分隔；多行/单值都支持；去重）。
pub fn parse_dns_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        for part in line.split('|') {
            let s = part.trim();
            if !s.is_empty() && !out.iter().any(|x| x == s) {
                out.push(s.to_string());
            }
        }
    }
    out
}

/// `resolvectl status <iface>` 输出 → DNS 列表（取 `DNS Servers:` 行）。
pub fn parse_resolvectl_dns(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("DNS Servers:"))
        .flat_map(|v| v.split_whitespace().map(String::from))
        .collect()
}

/// `/etc/os-release` 中取 `KEY="value"`（去引号；支持无引号值）。
pub fn os_release_value(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    text.lines().find_map(|l| {
        let l = l.trim();
        let v = l.strip_prefix(&prefix)?;
        Some(v.trim_matches('"').to_string())
    })
}

/// `/proc/cpuinfo` 第一个 `model name` 的值。
pub fn cpu_model(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let l = l.trim();
        let v = l.strip_prefix("model name")?;
        let v = v.trim();
        let v = v.strip_prefix(':')?.trim();
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// `/proc/meminfo` 中 `Key: N kB` 的数值。
pub fn meminfo_kib(text: &str, key: &str) -> Option<u64> {
    let prefix = format!("{key}:");
    text.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(&prefix)?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

/// `/proc/net/route` 主表所有默认路由 → 物理网卡的 `(iface, ip)` 列表。
/// 排除虚拟网卡（TUN/VPN/容器/回环——其 DNS 是 fake-ip 网关或 MagicDNS，不展示），
/// 按网卡去重。无默认路由返回空（面板不显示网络行）。
pub fn default_routes(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines().skip(1) {
        let mut cols = line.split_whitespace();
        let (Some(iface), Some(dest), Some(gw)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        // 目标 0.0.0.0 且网关非 0.0.0.0，且是物理网卡，且未重复
        if dest != "00000000" || gw == "00000000" || is_virtual_iface(iface) {
            continue;
        }
        let Some(ip) = hex_le_to_ipv4(gw) else {
            continue;
        };
        if !out.iter().any(|(i, _)| i == iface) {
            out.push((iface.to_string(), ip));
        }
    }
    out
}

/// 虚拟网卡判定（按名）：TUN/VPN/容器桥接/回环等。
/// clard0 的 DNS 是 fake-ip 网关，tailscale/wg 是 MagicDNS，均不属「真实网卡」。
pub fn is_virtual_iface(name: &str) -> bool {
    const VIRTUAL: &[&str] = &[
        "lo",
        "tun",
        "tap",
        "docker",
        "veth",
        "virbr",
        "br-",
        "tailscale",
        "wg",
        "utun",
        "zt",
        "clard0",
        "meta",
        "mihomo",
        "nordlynx",
        "ppp",
        "vpn",
    ];
    VIRTUAL.iter().any(|p| name == *p || name.starts_with(p))
}

/// 小端十六进制 IPv4（/proc/net/route 格式 `FE4F020A` → `10.2.79.254`）。
pub fn hex_le_to_ipv4(hex: &str) -> Option<String> {
    if hex.len() != 8 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, c) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(c).ok()?;
        // hex.len()==8 保证恰 4 组，get_mut 仅用于满足 clippy indexing-slicing（不会为 None）
        *octets.get_mut(i)? = u8::from_str_radix(s, 16).ok()?;
    }
    // /proc/net/route 的 IP 是主机字节序小端：FE 4F 02 0A → 0A 02 4F FE
    octets.reverse();
    Some(octets.map(|b| b.to_string()).join("."))
}

/// GPU 名称：遍历 `/sys/class/drm/card*/device` 的 vendor/device，按厂商映射。
/// 无 GPU / 未知厂商返回空（面板显示 `-`）。
pub fn read_gpu() -> String {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return String::new();
    };
    let mut cards: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("card") && n.chars().any(|c| c.is_ascii_digit()))
        })
        .collect();
    cards.sort();
    for card in cards {
        let dev = card.join("device");
        let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default();
        let vendor = vendor.trim();
        if vendor.is_empty() {
            continue;
        }
        let device = fs::read_to_string(dev.join("device")).unwrap_or_default();
        let device = device.trim();
        if let Some(name) = gpu_vendor(vendor) {
            return if device.is_empty() {
                name
            } else {
                format!("{name} 0x{device}", device = device.trim_start_matches("0x"))
            };
        }
        return format!("GPU {vendor}");
    }
    String::new()
}

/// PCI vendor hex（如 `0x10de`）→ 常见 GPU 厂商名。
pub fn gpu_vendor(vendor_hex: &str) -> Option<String> {
    let v = u32::from_str_radix(vendor_hex.trim().trim_start_matches("0x"), 16).ok()?;
    let name = match v {
        0x10de => "NVIDIA",
        0x1002 => "AMD",
        0x8086 => "Intel",
        0x1a03 => "ASPEED",
        0x102b => "Matrox",
        0x15ad => "VMware",
        0x1234 => "QEMU",
        0x1af4 => "VirtIO",
        _ => return None,
    };
    Some(name.into())
}

fn read_first_line(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().and_then(|t| {
        let l = t.lines().next()?.trim();
        (!l.is_empty()).then(|| l.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_parses_quoted_and_plain() {
        let t = "# comment\nPRETTY_NAME=\"Fedora Linux 41 (Workstation Edition)\"\nNAME=Fedora\n";
        assert_eq!(
            os_release_value(t, "PRETTY_NAME"),
            Some("Fedora Linux 41 (Workstation Edition)".into())
        );
        assert_eq!(os_release_value(t, "NAME"), Some("Fedora".into()));
        assert_eq!(os_release_value(t, "MISSING"), None);
    }

    #[test]
    fn cpu_model_takes_first() {
        let t = "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i7-13700K\nprocessor\t: 1\nmodel name\t: Intel(R) Core(TM) i7-13700K\n";
        assert_eq!(cpu_model(t), Some("Intel(R) Core(TM) i7-13700K".into()));
        assert_eq!(cpu_model("no model here\n"), None);
    }

    #[test]
    fn meminfo_parses_kib() {
        let t = "MemTotal:       16284832 kB\nMemAvailable:    8123456 kB\n";
        assert_eq!(meminfo_kib(t, "MemTotal"), Some(16284832));
        assert_eq!(meminfo_kib(t, "MemAvailable"), Some(8123456));
        assert_eq!(meminfo_kib(t, "Bogus"), None);
    }

    #[test]
    fn route_defaults_parse_little_endian_multi_nic() {
        // 多默认路由：wlp4s0 + eth0 都要收集；虚拟网卡（tailscale0）排除；非默认路由（br0）跳过
        let t = "Iface\tDestination\tGateway \tFlags\nwlp4s0\t00000000\tFE4F020A\t0003\ntailscale0\t00000000\t0A6C020A\t0003\neth0\t00000000\t0101A8C0\t0003\nbr0\t0000010A\t00000000\t0001\n";
        assert_eq!(
            default_routes(t),
            vec![
                ("wlp4s0".into(), "10.2.79.254".into()),
                ("eth0".into(), "192.168.1.1".into()),
            ],
            "多网卡各自收集，虚拟网卡排除，非默认路由跳过"
        );
    }

    #[test]
    fn route_without_gateway_or_default_is_empty() {
        assert!(default_routes("Iface\tDestination\tGateway\nbr0\t0000010A\t00000000\t0001\n").is_empty());
        assert!(default_routes("Iface\tDestination\tGateway\nwlp4s0\t00000000\t00000000\t0003\n").is_empty());
    }

    #[test]
    fn virtual_iface_exclusion() {
        assert!(is_virtual_iface("clard0"));
        assert!(is_virtual_iface("tailscale0"));
        assert!(is_virtual_iface("docker0"));
        assert!(is_virtual_iface("veth1234"));
        assert!(is_virtual_iface("lo"));
        assert!(!is_virtual_iface("wlp4s0"));
        assert!(!is_virtual_iface("eth0"));
        assert!(!is_virtual_iface("enp2s0"));
    }

    #[test]
    fn hex_le_converts() {
        assert_eq!(hex_le_to_ipv4("FE4F020A"), Some("10.2.79.254".into()));
        assert_eq!(hex_le_to_ipv4("00000000"), Some("0.0.0.0".into()));
        assert_eq!(hex_le_to_ipv4("FE4F02"), None, "长度不足 8");
        assert_eq!(hex_le_to_ipv4("GGGGGGGG"), None, "非法 hex");
    }

    #[test]
    fn gpu_vendor_mapping() {
        assert_eq!(gpu_vendor("0x10de"), Some("NVIDIA".into()));
        assert_eq!(gpu_vendor("0x8086"), Some("Intel".into()));
        assert_eq!(gpu_vendor("0x1234"), Some("QEMU".into()));
        assert_eq!(gpu_vendor("0xdead"), None, "未知厂商返回 None（面板显示 -）");
        assert_eq!(gpu_vendor(""), None);
    }

    #[test]
    fn dns_list_parses_pipe_and_dedups() {
        // nmcli -g IP4.DNS 输出：管道分隔；重复项去重；空值忽略
        let t = "10.2.41.130 | 61.139.2.69\n";
        assert_eq!(parse_dns_list(t), vec!["10.2.41.130", "61.139.2.69"]);
        let t2 = "10.2.41.130 | 61.139.2.69 | 10.2.41.130\n10.2.79.254\n";
        assert_eq!(parse_dns_list(t2), vec!["10.2.41.130", "61.139.2.69", "10.2.79.254"]);
        assert!(parse_dns_list("").is_empty());
    }

    #[test]
    fn resolvectl_dns_parses_servers_line() {
        let t = "Link 4 (wlp4s0)\n    Current DNS Server: 10.2.41.130\n           DNS Servers: 10.2.41.130 61.139.2.69\n     Default Route: yes\n";
        assert_eq!(parse_resolvectl_dns(t), vec!["10.2.41.130", "61.139.2.69"]);
        assert!(parse_resolvectl_dns("no dns here\n").is_empty());
    }
}
