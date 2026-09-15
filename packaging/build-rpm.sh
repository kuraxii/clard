#!/usr/bin/env bash
# 构建 clard RPM：release 编译 → 下载最新 mihomo + geo 数据 → 收集产物 → rpmbuild。
#
# 用法：
#   bash packaging/build-rpm.sh              # 下载最新 mihomo + geo 数据打包（安装即用）
#   bash packaging/build-rpm.sh --no-mihomo  # 跳过 mihomo（包内不含核心，首次 InstallCore）
#   bash packaging/build-rpm.sh --no-geodata # 跳过 geo 数据（包内不含 geoip/geosite）
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
if [ "$WITH_GEODATA" = "1" ]; then
    echo "==> 下载最新 geo 数据（MetaCubeX meta-rules-dat）"
    for name in geoip.metadb geosite.dat; do
        ok=0
        for base in \
            "https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release" \
            "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download"; do
            if curl -fL --retry 2 -m 300 -sS -o "$HOME/rpmbuild/SOURCES/$name" "$base/$name" && [ -s "$HOME/rpmbuild/SOURCES/$name" ]; then
                echo "    $name 已下载 ($(du -h "$HOME/rpmbuild/SOURCES/$name" | cut -f1)): $base"
                ok=1
                break
            fi
        done
        if [ "$ok" = "0" ]; then
            echo "!! 下载 $name 失败（jsdelivr/GitHub 均不可达），包内不含 geo 数据（TUI 可更新）" >&2
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
