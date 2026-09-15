# clard RPM 打包（Fedora/RHEL 系）。
#
# 方案：预构建二进制（build-rpm.sh 先 cargo build --release，产物放 SOURCES 由本 spec
# 收集）；%post 负责数据目录 + systemd enable/start，%preun/%postun 负责 stop/disable。
# 安装即作为系统级常驻服务（doc/01 §4）：TUI 无需 su，helper 常驻后台。
#
# mihomo 核心：build-rpm.sh 构建时下载最新版打包进安装包（Source4）→ 安装即用；
# 用户侧 InstallCore 升级（doc/01 §5.1）保留为可选动作，且以 %config(noreplace)
# 声明——rpm 升级不会覆盖用户手动升级过的 mihomo。构建时未下载 mihomo（显式
# --no-mihomo）则包内不含，首次使用经 InstallCore 安装。

Name:           clard
Version:        0.1.0
# 不带 dist 标记（如 fc41）：包名/版本/架构即可，便于跨发行版复用构建产物
Release:        1
Summary:        Clard — Linux transparent proxy manager (system helper + TUI client)

License:        MIT
URL:            https://github.com/kuraxii/clard
Source0:        %{name}-%{version}.tar.gz
Source1:        clard
Source2:        clard-helper
Source3:        clard-helper.service
# 可选：构建时下载的最新 mihomo（build-rpm.sh；缺失 = 包内不含核心）
Source4:        mihomo

# 预构建方案：无 BuildRequires；运行依赖
# ip 用文件依赖（跨发行版免疫包名差异：Fedora 41 为 iproute，RHEL 为 iproute2）；
# nftables 不依赖——一期 auto-redirect 关闭，cleanup-tun 对 nft 失败容错（§6.4）
Requires:       systemd
Requires:       /usr/sbin/ip
Requires(post): systemd
Requires(preun): systemd

# 二进制由 build-rpm.sh 预构建，无 debug 源参与；禁用 debuginfo/debugsource 子包
%global debug_package %{nil}
# 是否随包分发 mihomo（build-rpm.sh 下载成功 = 1）
%global mihomo_present %(test -f %{_sourcedir}/mihomo && echo 1 || echo 0)

%description
Clard 是 Linux 上的完整代理管理工具：系统级常驻服务 clard-helper（root，
拥有 mihomo 核心、TUN 与全部数据）+ TUI 客户端 clard（ratatui）。
数据面 TUN-only，系统级服务、不分用户；安装一次后任意本地用户可经 TUI 使用。
安装包携带 mihomo 核心，安装即用；核心升级是用户可选项（TUI Core 页）。

%prep
%setup -q -n %{name}-%{version}

%build
# 二进制由 build-rpm.sh 预构建（见 packaging/build-rpm.sh）

%install
install -Dm755 %{SOURCE1} %{buildroot}%{_bindir}/clard
install -Dm755 %{SOURCE2} %{buildroot}%{_libexecdir}/clard/clard-helper
install -Dm644 %{SOURCE3} %{buildroot}%{_unitdir}/clard-helper.service
install -d -m 0755 %{buildroot}%{_sysconfdir}/clard
%if %{mihomo_present}
install -Dm755 %{SOURCE4} %{buildroot}%{_localstatedir}/clard/bin/mihomo
%endif

%post
# 数据目录（helper 启动自检也会建，这里预建保证权限 root 0700）
install -d -m 0700 %{_localstatedir}/clard/lib \
                 %{_localstatedir}/clard/cache \
                 %{_localstatedir}/clard/log \
                 %{_localstatedir}/clard/backups \
                 %{_localstatedir}/clard/bin
%systemd_post clard-helper.service
# 兜底：部分环境（容器/最小化 systemd）file-trigger 不生效导致 enable 缺失，
# 显式 enable + start（幂等，失败不阻塞事务）
systemctl enable clard-helper.service >/dev/null 2>&1 || :
# try-restart：安装时启动；升级时旧服务在跑则重启为新二进制
# （systemctl start 对已 active 服务是 no-op，会导致升级后 helper 仍是旧版本）
systemctl try-restart clard-helper.service >/dev/null 2>&1 || :

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
%if %{mihomo_present}
# noreplace：用户经 InstallCore 升级过的 mihomo 不被 rpm 升级覆盖
%config(noreplace) %{_localstatedir}/clard/bin/mihomo
%endif
