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
Version:        0.3.1
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
# 显式 enable + restart（幂等，失败不阻塞事务）。
# restart 同时覆盖两种场景：全新安装时启动；升级时把运行中的旧二进制换成新版本
# （start 对已 active 是 no-op，try-restart 对未运行是 no-op，restart 两者都覆盖）。
systemctl enable clard-helper.service >/dev/null 2>&1 || :
systemctl restart clard-helper.service >/dev/null 2>&1 || :

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

%changelog
* Fri Oct 16 2026 zjzhu <zjzhu@kuraxii.dev> - 0.3.1-1
- geo 数据改为运行时安装（不随 RPM）：移除构建期 geo 下载与包内携带，升级永不触碰 /var/clard/lib/runtime 的 geo 文件（v0.3.0 构建期下载被 GitHub 429 把错误页打进包导致核心 MMDB invalid）；首次缺失由 TUI 自动安装，`g` 手动更新
- AGENTS.md：新增提交信息陈述表达与禁止括号解释规则
* Fri Oct 16 2026 zjzhu <zjzhu@kuraxii.dev> - 0.3.0-1
- 设置枚举项改为选择列表弹窗（Enter/e 打开，↑↓/jk 移动，Enter 确认，Esc 取消，高亮当前值）：代理模式、语言、主题、TUN stack、TUN DNS mode、fake-ip-filter-mode；General 页 Enter 也触发编辑
- fake-ip-filter-mode=rule 下发前校验（R3）：fake-ip-filter 条目须为规则行（如 DOMAIN-SUFFIX,example.com,fake-ip），裸域名直接中文报错回滚，不再暴露 mihomo HTTP 400 invalid action
- 修复 clippy indexing-slicing（sysinfo IPv4 解析），CI --all-targets 全绿
* Thu Sep 17 2026 zjzhu <zjzhu@kuraxii.dev> - 0.2.2-1
- 主页 System 面板替换 Profile：OS/内核/CPU/GPU/内存（总/可用）/真实网卡网关（无默认路由不显示）；系统文件直读（非 clard 数据不走 IPC）+ 10s 周期刷新
- 修复 mixed-port 端口冲突：订阅自带 port/socks-port 抢占 7890 导致 mixed 监听绑定失败、回读校验不通过（托管注入归零 port/socks-port/redir-port/tproxy-port）
- external-controller 清空：控制面只走 unix socket，杜绝订阅透传的无认证 TCP 控制 API 暴露
* Thu Sep 17 2026 zjzhu <zjzhu@kuraxii.dev> - 0.2.1-1
- 新增设置页 DNS 页签（仅 TUN 开启时生效）：fake-ip-filter-mode（blacklist/whitelist/rule）、fake-ip-filter、use-hosts/use-system-hosts、hosts（domain=ip 静态映射，注入顶层）、nameserver-policy（domain=dns 按域名指定上游）、nameserver/default-nameserver（空 = clard 默认）
- 移除 exclude-uid/interface/dst-port 可配置项（基本用不上，TUN 页签 11→8 行）
- 代理页节点默认按名称排序并移除 s 快捷键；测速仅测试当前选中分组（t/T）
* Tue Sep 15 2026 zjzhu <zjzhu@kuraxii.dev> - 0.2.0-1
- 代理模式切换（rule 分流 / global 全代理 / direct 全直连）：设置页 General「Mode」循环切换，白名单 regenerate 热重载
- geo 数据随 RPM 分发到核心 -d 目录（/var/clard/lib/runtime）安装即用；TUI `g` 更新（inbox 校验 + 原子替换）；`%config` 升级备份旧 .rpmsave 并应用新文件
- 纯节点列表订阅补默认规则（GEOIP,private/CN,DIRECT + MATCH,GLOBAL + GLOBAL 组），否则 rule 模式空规则全直连
- TUN 冲突改为警告 + force 二次确认（tap 设备排除，vnet 误报修复）
- 协议 v10 → v11（Settings 新增 mode）
- 构建容器钉 fedora:39（EOL 源切 archive + rustup 1.95.0）
