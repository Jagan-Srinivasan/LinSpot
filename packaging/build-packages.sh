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

cat > pkg/postinstall.sh <<'EOF'
#!/bin/sh
# LinSpot: the desktop widgets (and the app) can turn the hotspot off and disconnect
# devices without a password. Only these two low-risk actions are allowed.
sudoers=/etc/sudoers.d/50-linspot
printf '%s\n' \
  '# LinSpot - passwordless stop/off/kick for the desktop widgets' \
  'ALL ALL=(root) NOPASSWD: /usr/bin/linspot stop, /usr/bin/linspot off, /usr/bin/linspot kick-root *' \
  > "$sudoers"
chmod 440 "$sudoers"
if command -v visudo >/dev/null 2>&1; then
  visudo -cf "$sudoers" >/dev/null 2>&1 || rm -f "$sudoers"
fi
exit 0
EOF
cat > pkg/prerm.sh <<'EOF'
#!/bin/sh
rm -f /etc/sudoers.d/50-linspot
exit 0
EOF
chmod 755 pkg/postinstall.sh pkg/prerm.sh

cargo build --release
command -v nfpm >/dev/null || { echo 'nfpm is not installed' >&2; exit 1; }

case "$KIND" in
  deb) nfpm package --config packaging/nfpm-deb.yaml --packager deb --target dist/ ;;
  rpm) nfpm package --config packaging/nfpm-rpm.yaml --packager rpm --target dist/ ;;
  arch) nfpm package --config packaging/nfpm-arch.yaml --packager archlinux --target dist/ ;;
  all)
    nfpm package --config packaging/nfpm-deb.yaml --packager deb --target dist/
    nfpm package --config packaging/nfpm-rpm.yaml --packager rpm --target dist/
    nfpm package --config packaging/nfpm-arch.yaml --packager archlinux --target dist/
    ;;
  *) echo "Usage: $0 deb|rpm|arch|all" >&2; exit 2 ;;
esac

echo "Packages created in $ROOT/dist"
