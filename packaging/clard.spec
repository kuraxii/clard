# clard RPM 打包（Fedora/RHEL 系）。
#
# 方案：预构建二进制（build-rpm.sh 先 cargo build --release，产物放 SOURCES 由本 spec
# 收集）；%post 负责数据目录 + systemd enable/start，%preun/%postun 负责 stop/disable。
# 安装即作为系统级常驻服务（doc/01 §4）：TUI 无需 su，helper 常驻后台。
#
# mihomo 核心二进制不在包内（体积大且动态升级，见 doc/01 §5.1）：
# 首次使用时经 helper InstallCore 里程碑下载到 /var/clard/bin/mihomo；
# 若需随包分发，构建时在 SOURCES 放 mihomo 并取消 %install 中对应行的注释。

Name:           clard
Version:        0.1.0
Release:        1%{?dist}
Summary:        Clard — Linux transparent proxy manager (system helper + TUI client)

License:        MIT
URL:            https://github.com/clard/clard
Source0:        %{name}-%{version}.tar.gz
Source1:        clard
Source2:        clard-helper
Source3:        clard-helper.service

# 预构建方案：无 BuildRequires；运行依赖
Requires:       systemd
Requires:       iproute2
Requires:       nftables
Requires(post): systemd
Requires(preun): systemd

%description
Clard 是 Linux 上的完整代理管理工具：系统级常驻服务 clard-helper（root，
拥有 mihomo 核心、TUN 与全部数据）+ TUI 客户端 clard（ratatui）。
数据面 TUN-only，系统级服务、不分用户；安装一次后任意本地用户可经 TUI 使用。

%prep
%setup -q -n %{name}-%{version}

%build
# 二进制由 build-rpm.sh 预构建（见 packaging/build-rpm.sh）

%install
install -Dm755 %{SOURCE1} %{buildroot}%{_bindir}/clard
install -Dm755 %{SOURCE2} %{buildroot}%{_libexecdir}/clard/clard-helper
install -Dm644 %{SOURCE3} %{buildroot}%{_unitdir}/clard-helper.service
install -d -m 0755 %{buildroot}%{_sysconfdir}/clard
# 如需随包分发初始 mihomo：install -Dm755 %{_sourcedir}/mihomo %{buildroot}%{_localstatedir}/clard/bin/mihomo

%post
# 数据目录（helper 启动自检也会建，这里预建保证权限 root 0700）
install -d -m 0700 %{_localstatedir}/clard/lib \
                 %{_localstatedir}/clard/cache \
                 %{_localstatedir}/clard/log \
                 %{_localstatedir}/clard/backups \
                 %{_localstatedir}/clard/bin
%systemd_post clard-helper.service
# 常驻后台代理是产品形态：安装后立即启用（失败不阻塞事务，如 chroot）
systemctl start clard-helper.service || :

%preun
%systemd_preun clard-helper.service

%postun
%systemd_postun clard-helper.service

%files
%{_bindir}/clard
%{_libexecdir}/clard/clard-helper
%{_unitdir}/clard-helper.service
%dir %{_sysconfdir}/clard
%doc README.md
