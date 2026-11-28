#!/usr/bin/env bash
# LinSpot uninstaller.
#   sudo ./uninstall.sh                 remove binary, polkit policy, desktop entry, icon
#   sudo ./uninstall.sh --purge         also remove /etc/linspot and per-user config
#   sudo ./uninstall.sh --remove-deps   also remove hostapd + dnsmasq (asks first)
#   -y                                   do not ask questions
set -uo pipefail
PURGE=0; RMDEPS=0; YES=0
for a in "$@"; do
  case "$a" in
    --purge) PURGE=1 ;; --remove-deps) RMDEPS=1 ;; -y|--yes) YES=1 ;;
    -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
    *) echo "unknown option: $a"; exit 2 ;;
  esac
done
c_b=$'\e[1;36m'; c_g=$'\e[1;32m'; c_y=$'\e[1;33m'; c_r=$'\e[1;31m'; c_0=$'\e[0m'
say() { echo "${c_b}==>${c_0} $*"; }; ok() { echo "${c_g} ok${c_0} $*"; }; warn() { echo "${c_y}warn${c_0} $*"; }
[ "$(id -u)" -eq 0 ] || { echo "${c_r}error${c_0} run as root:  sudo ./uninstall.sh" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

PM=""; PREFIX=/usr/local; BINDIR=/usr/local/bin
[ -f /etc/linspot/install.info ] && . /etc/linspot/install.info

# 1) stop a running hotspot so Wi-Fi/firewall/NAT state is restored
BIN="$BINDIR/linspot"; [ -x "$BIN" ] || BIN="$(command -v linspot || true)"
if [ -n "$BIN" ] && [ -x "$BIN" ]; then
  say "Stopping hotspot (if running)"
  "$BIN" stop >/dev/null 2>&1 || true
fi
rm -rf /run/linspot

# 2) remove files
say "Removing files"
for f in "$BINDIR/linspot" /usr/local/bin/linspot /usr/bin/linspot \
         /usr/share/polkit-1/actions/org.linspot.policy \
         /usr/share/applications/linspot.desktop \
         /usr/share/icons/hicolor/scalable/apps/linspot.svg; do
  [ -e "$f" ] && { rm -f "$f"; echo "   - $f"; }
done
has update-desktop-database && update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
has gtk-update-icon-cache && gtk-update-icon-cache -q /usr/share/icons/hicolor >/dev/null 2>&1 || true

rm -f /etc/NetworkManager/conf.d/90-linspot-unmanaged.conf && has nmcli && nmcli general reload conf >/dev/null 2>&1 || true
rm -f /etc/sudoers.d/50-linspot

# 2b) desktop widgets (KDE Plasma / GNOME / DankMaterialShell) in every user's home
for h in /root /home/*; do
  [ -d "$h" ] || continue
  for w in "$h/.local/share/plasma/plasmoids/org.linspot.hotspot" \
           "$h/.local/share/gnome-shell/extensions/linspot@linspot.org" \
           "$h"/.config/DankMaterialShell/plugins/linspot{Hotspot,Launcher} \
           "$h"/.config/dms/plugins/linspot{Hotspot,Launcher}; do
    [ -e "$w" ] && { rm -rf "$w"; echo "   - $w"; }
  done
done

# 3) leftovers from a crashed run
ip link show hs0 >/dev/null 2>&1 && { iw dev hs0 del >/dev/null 2>&1 || true; echo "   - interface hs0"; }
has nft && { nft delete table inet linspot >/dev/null 2>&1; nft delete table ip linspot_nat >/dev/null 2>&1; } || true

# 4) purge config
if [ "$PURGE" -eq 1 ]; then
  say "Purging configuration"
  rm -rf /etc/linspot
  for h in /root /home/*; do rm -rf "$h/.config/linspot" 2>/dev/null; done
else
  rm -f /etc/linspot/install.info
  warn "kept /etc/linspot (saved country) and ~/.config/linspot - use --purge to remove"
fi

# 5) optional dependency removal (only hotspot-specific packages)
if [ "$RMDEPS" -eq 1 ]; then
  case "$PM" in
    pacman) PK="hostapd dnsmasq" ;; apt) PK="hostapd dnsmasq-base" ;; dnf|zypper|apk|xbps) PK="hostapd dnsmasq" ;;
    emerge) PK="net-wireless/hostapd net-dns/dnsmasq" ;; *) PK="" ;;
  esac
  if [ -n "$PK" ]; then
    ans=y
    [ "$YES" -eq 1 ] || { read -r -p "Remove packages: $PK ? [y/N] " ans; }
    case "$ans" in
      y|Y)
        case "$PM" in
          pacman) pacman -Rns --noconfirm $PK ;; apt) apt-get remove -y $PK ;; dnf) dnf remove -y $PK ;;
          zypper) zypper --non-interactive remove $PK ;; apk) apk del $PK ;; xbps) xbps-remove -Ry $PK ;;
          emerge) emerge --unmerge $PK ;;
        esac ;;
      *) echo "skipped dependency removal" ;;
    esac
  else
    warn "unknown package manager; remove hostapd/dnsmasq manually if wanted"
  fi
fi
ok "LinSpot uninstalled"
