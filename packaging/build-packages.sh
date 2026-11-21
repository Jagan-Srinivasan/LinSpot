#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
KIND="${1:-all}"
VERSION="${VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n1)}"
export VERSION

mkdir -p pkg dist
sed 's|@BINDIR@|/usr/bin|g' data/org.linspot.policy > pkg/org.linspot.policy
sed 's|@BINDIR@|/usr/bin|g' data/linspot.desktop > pkg/linspot.desktop
printf 'BINDIR=/usr/bin\nPREFIX=/usr\n' > pkg/install.info
printf '[keyfile]\nunmanaged-devices=interface-name:hs0\n' > pkg/90-linspot-unmanaged.conf

cargo build --release
command -v nfpm >/dev/null || { echo 'nfpm is not installed' >&2; exit 1; }

case "$KIND" in
  deb) nfpm package --config packaging/nfpm-deb.yaml --packager deb --target dist/ ;;
  rpm) nfpm package --config packaging/nfpm-rpm.yaml --packager rpm --target dist/ ;;
  all)
    nfpm package --config packaging/nfpm-deb.yaml --packager deb --target dist/
    nfpm package --config packaging/nfpm-rpm.yaml --packager rpm --target dist/
    ;;
  *) echo "Usage: $0 deb|rpm|all" >&2; exit 2 ;;
esac

echo "Packages created in $ROOT/dist"
