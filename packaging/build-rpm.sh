#!/usr/bin/env bash
# 构建 clard RPM：release 编译 → 下载最新 mihomo + geo 数据 → 收集产物 → rpmbuild。
#
# 用法：
#   bash packaging/build-rpm.sh              # 下载最新 mihomo + geo 数据打包（安装即用）
#   bash packaging/build-rpm.sh --no-mihomo  # 跳过 mihomo（包内不含核心，首次 InstallCore）
#   bash packaging/build-rpm.sh --no-geodata # 跳过 geo 数据（包内不含 geoip/geosite）
# geo 下载每文件最多重试 3 次，校验内容有效后才算成功；仍失败则构建中止
# （v0.3.0 曾把 GitHub 429 错误页当有效数据打进包 → 核心 MMDB invalid 起不来）。
# 产物：~/rpmbuild/RPMS/*/clard-*.rpm
# 安装：sudo dnf install ~/rpmbuild/RPMS/*/clard-*.rpm
set -euo pipefail
cd "$(dirname "$0")/.."

WITH_MIHOMO=1
WITH_GEODATA=1
MODE_LOCAL=0
case "${1:-}" in
    ""|--no-mihomo|--local-core|--no-geodata) ;;
    *) echo "!! 未知参数 $1" >&2; exit 2 ;;
esac
if [ "${1:-}" = "--no-mihomo" ]; then
    WITH_MIHOMO=0
elif [ "${1:-}" = "--local-core" ]; then
    MODE_LOCAL=1  # 用 /var/clard/bin/mihomo（本机已有），免 GitHub 下载
fi
if [ "${1:-}" = "--no-geodata" ]; then
    WITH_GEODATA=0
fi

VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*= *"([^"]+)"/\1/')
echo "==> cargo build --release (version $VERSION)"
cargo build --release

echo "==> 准备 rpmbuild 目录"
rm -rf ~/rpmbuild/{SOURCES,BUILD,BUILDROOT,SPECS}
mkdir -p ~/rpmbuild/{SOURCES,BUILD,BUILDROOT,RPMS,SRPMS,SPECS}

# Release 构建次数自动 +1（AGENTS.md：本地构建不提交，脚本自动递增保证 NVR 唯一；
# 发布时在 packaging/clard.spec 显式更新 Version/Release 并提交）
RELEASE_CUR=$(sed -n 's/^Release:[[:space:]]*\([0-9]*\).*/\1/p' packaging/clard.spec | head -1)
RELEASE_CUR=${RELEASE_CUR:-0}
RELEASE_NEXT=$((RELEASE_CUR + 1))
sed -i -E "s/^Release:[[:space:]]*[0-9]+/Release:        $RELEASE_NEXT/" packaging/clard.spec
echo "==> Release 构建次数自动 +1：$RELEASE_CUR → $RELEASE_NEXT（本地构建不提交）"

if [ "$WITH_MIHOMO" = "1" ]; then
    if [ "$MODE_LOCAL" = "1" ]; then
        if [ -x /var/clard/bin/mihomo ]; then
            echo "==> --local-core：使用 /var/clard/bin/mihomo"
            cp /var/clard/bin/mihomo ~/rpmbuild/SOURCES/mihomo
            echo "    已复制 $(du -h ~/rpmbuild/SOURCES/mihomo | cut -f1)"
        else
            echo "!! /var/clard/bin/mihomo 不存在，回退在线下载" >&2
            MODE_LOCAL=0
        fi
    fi
    if [ "$MODE_LOCAL" = "0" ]; then
    echo "==> 下载最新 mihomo（MetaCubeX GitHub release）"
    case "$(uname -m)" in
        x86_64)  MH_ARCH="amd64" ;;
        aarch64) MH_ARCH="arm64" ;;
        *) echo "!! 不支持的架构 $(uname -m)，包内不含 mihomo" >&2; MH_ARCH="" ;;
    esac
    if [ -n "$MH_ARCH" ]; then
        API_URL="https://api.github.com/repos/MetaCubeX/mihomo/releases/latest"
        REL_VERSION=$(curl -fsSL "$API_URL" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
        if [ -z "$REL_VERSION" ]; then
            echo "!! 获取最新版本号失败，包内不含 mihomo（可 --no-mihomo 显式跳过）" >&2
        else
            DL_URL="https://github.com/MetaCubeX/mihomo/releases/download/${REL_VERSION}/mihomo-linux-${MH_ARCH}-${REL_VERSION}.gz"
            echo "    版本 ${REL_VERSION} 架构 ${MH_ARCH}: ${DL_URL}"
            curl -fL --retry 3 -o /tmp/mihomo.gz "$DL_URL"
            gzip -d -f /tmp/mihomo.gz
            mv /tmp/mihomo ~/rpmbuild/SOURCES/mihomo
            chmod 755 ~/rpmbuild/SOURCES/mihomo
            echo "    已下载 $(du -h ~/rpmbuild/SOURCES/mihomo | cut -f1)"
        fi
    fi
    fi
else
    echo "==> --no-mihomo：包内不含 mihomo"
fi

# ---- geo 数据（geoip.metadb / geosite.dat，随包分发安装即用；TUI 可更新）----
# 下载每文件最多重试 3 次（两镜像轮换），内容校验通过才算成功：
#   geoip.metadb = MaxMind MMDB，以 5 字节魔数 "MMDB\0"（4d 4d 44 42 00）开头；
#   geosite.dat  = varint+protobuf 流（无魔数），以体积阈值过滤 429/HTML 错误页（真实文件 ≥1MiB）。
# 3 次仍失败 → 显式 exit 1 构建中止，不再静默打出缺/坏 geo 的包（--no-geodata 可显式跳过）。
GEO_MIRRORS=(
    "https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release"
    "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download"
)

# 校验下载内容是否有效（返回 0 = 有效）。
geo_valid() {
    local name="$1" file="$2"
    case "$name" in
        geoip.metadb)
            [ "$(head -c 5 "$file" 2>/dev/null | od -An -tx1 | tr -d ' \n')" = "4d4d444200" ]
            ;;
        geosite.dat)
            [ "$(stat -c %s "$file" 2>/dev/null || echo 0)" -ge 1048576 ]
            ;;
        *) return 1 ;;
    esac
}

if [ "$WITH_GEODATA" = "1" ]; then
    echo "==> 下载最新 geo 数据（MetaCubeX meta-rules-dat，每文件最多重试 3 次）"
    for name in geoip.metadb geosite.dat; do
        ok=0
        attempt=1
        while [ "$attempt" -le 3 ]; do
            for base in "${GEO_MIRRORS[@]}"; do
                echo "    [$attempt/3] $name ← $base"
                if curl -fL --retry 2 -m 300 -sS -o "$HOME/rpmbuild/SOURCES/$name" "$base/$name" \
                    && geo_valid "$name" "$HOME/rpmbuild/SOURCES/$name"; then
                    echo "    $name 已下载 ($(du -h "$HOME/rpmbuild/SOURCES/$name" | cut -f1)): $base"
                    ok=1
                    break
                fi
                rm -f "$HOME/rpmbuild/SOURCES/$name"
            done
            [ "$ok" = "1" ] && break
            attempt=$((attempt + 1))
        done
        if [ "$ok" = "0" ]; then
            echo "!! 下载 $name 失败：重试 3 次均未取得有效数据（限流/网络不可达），构建中止" >&2
            exit 1
        fi
    done
else
    echo "==> --no-geodata：包内不含 geo 数据"
fi

echo "==> 源码归档（git HEAD，需先提交工作树）"
git archive --format=tar.gz -o ~/rpmbuild/SOURCES/clard-$VERSION.tar.gz --prefix=clard-$VERSION/ HEAD

echo "==> 收集预构建产物"
cp target/release/clard target/release/clard-helper ~/rpmbuild/SOURCES/
cp packaging/clard-helper.service ~/rpmbuild/SOURCES/
cp packaging/clard.spec ~/rpmbuild/SPECS/

echo "==> rpmbuild -bb"
rpmbuild -bb ~/rpmbuild/SPECS/clard.spec

echo "==> 完成："
ls -lh ~/rpmbuild/RPMS/*/clard-*.rpm
