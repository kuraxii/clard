#!/usr/bin/env bash
# 构建 clard RPM：release 编译 → 下载最新 mihomo → 收集产物 → rpmbuild。
#
# 用法：
#   bash packaging/build-rpm.sh              # 下载最新 mihomo 打包（安装即用）
#   bash packaging/build-rpm.sh --no-mihomo  # 跳过 mihomo（包内不含核心，首次 InstallCore）
# 产物：~/rpmbuild/RPMS/*/clard-*.rpm
# 安装：sudo dnf install ~/rpmbuild/RPMS/*/clard-*.rpm
set -euo pipefail
cd "$(dirname "$0")/.."

WITH_MIHOMO=1
if [ "${1:-}" = "--no-mihomo" ]; then
    WITH_MIHOMO=0
fi

VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*= *"([^"]+)"/\1/')
echo "==> cargo build --release (version $VERSION)"
cargo build --release

echo "==> 准备 rpmbuild 目录"
rm -rf ~/rpmbuild/{SOURCES,BUILD,BUILDROOT,SPECS}
mkdir -p ~/rpmbuild/{SOURCES,BUILD,BUILDROOT,RPMS,SRPMS,SPECS}

if [ "$WITH_MIHOMO" = "1" ]; then
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
else
    echo "==> --no-mihomo：包内不含 mihomo"
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
