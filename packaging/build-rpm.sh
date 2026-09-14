#!/usr/bin/env bash
# 构建 clard RPM：release 编译 → 收集产物 → rpmbuild（预构建二进制方案，见 clard.spec）。
#
# 用法：bash packaging/build-rpm.sh
# 产物：~/rpmbuild/RPMS/*/clard-*.rpm
# 安装：sudo dnf install ~/rpmbuild/RPMS/*/clard-*.rpm
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*= *"([^"]+)"/\1/')
echo "==> cargo build --release (version $VERSION)"
cargo build --release

echo "==> 准备 rpmbuild 目录"
rm -rf ~/rpmbuild/{SOURCES,BUILD,BUILDROOT,SPECS}
mkdir -p ~/rpmbuild/{SOURCES,BUILD,BUILDROOT,RPMS,SRPMS,SPECS}

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
