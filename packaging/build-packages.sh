#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$(dirname "$(realpath "$0")")")"
kind=${1:-all}
VERSION=${VERSION:-$(awk -F'"' '/^version =/{print $2;exit}' Cargo.toml)}
export VERSION
mkdir -p pkg dist
sed 's|@BINDIR@|/usr/bin|g' data/org.linspot.policy > pkg/org.linspot.policy
sed 's|@BINDIR@|/usr/bin|g' data/linspot.desktop > pkg/linspot.desktop
printf 'BINDIR=/usr/bin\nPREFIX=/usr\n' > pkg/install.info
printf '[keyfile]\nunmanaged-devices=interface-name:hs0\n' > pkg/90-linspot-unmanaged.conf
cargo build --release
if ! command -v nfpm >/dev/null; then echo 'nfpm missing: install https://nfpm.goreleaser.com/' >&2; exit 1; fi
case "$kind" in
  deb) nfpm package --config packaging/nfpm.yaml --packager deb --target dist/ ;;
  rpm) nfpm package --config packaging/nfpm.yaml --packager rpm --target dist/ ;;
  all) nfpm package --config packaging/nfpm.yaml --packager deb --target dist/; nfpm package --config packaging/nfpm.yaml --packager rpm --target dist/ ;;
  *) echo "Usage: $0 deb|rpm|all" >&2; exit 2 ;;
esac
echo "Packages saved under dist/"
