#!/usr/bin/env bash
# LinSpot installer - works on Arch/CachyOS/Manjaro, Debian/Ubuntu/Mint, Fedora/RHEL,
# openSUSE, Alpine, Void and Gentoo.  Installs runtime dependencies, builds (or copies)
# the binary, and installs the polkit policy, desktop entry and icon.
#
#   sudo ./install.sh                 deps + build + install
#   sudo ./install.sh --deps-only     only install system dependencies
#   sudo ./install.sh --no-deps       skip dependency installation
#   sudo ./install.sh --prefix /usr   install under /usr (default /usr/local)
#   sudo ./install.sh --binary PATH   use a prebuilt binary instead of building
#   sudo ./install.sh --widgets W     desktop widgets: auto (default) | plasma | gnome | dms | all | none
set -euo pipefail

PREFIX=/usr/local; DEPS=1; BUILD=1; BIN=""; DEPS_ONLY=0; WIDGETS=auto
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) PREFIX="$2"; shift ;;
    --no-deps) DEPS=0 ;;
    --deps-only) DEPS_ONLY=1 ;;
    --binary) BIN="$2"; BUILD=0; shift ;;
    --widgets) WIDGETS="${2:?--widgets needs auto|plasma|gnome|dms|all|none}"; shift ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown option: $1"; exit 2 ;;
  esac
  shift
done

c_b=$'\e[1;36m'; c_g=$'\e[1;32m'; c_y=$'\e[1;33m'; c_r=$'\e[1;31m'; c_0=$'\e[0m'
say()  { echo "${c_b}==>${c_0} $*"; }
ok()   { echo "${c_g} ok${c_0} $*"; }
warn() { echo "${c_y}warn${c_0} $*"; }
die()  { echo "${c_r}error${c_0} $*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root:  sudo ./install.sh"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINDIR="$PREFIX/bin"

# ---------------------------------------------------------------- distro
. /etc/os-release 2>/dev/null || true
IDS=" ${ID:-} ${ID_LIKE:-} "
PM=""
has() { command -v "$1" >/dev/null 2>&1; }
if   has pacman;       then PM=pacman
elif has apt-get;      then PM=apt
elif has dnf;          then PM=dnf
elif has zypper;       then PM=zypper
elif has apk;          then PM=apk
elif has xbps-install; then PM=xbps
elif has emerge;       then PM=emerge
fi
say "Detected: ${PRETTY_NAME:-unknown distro}  (package manager: ${PM:-none})"

# Runtime deps: Wi-Fi tools + GUI libraries (GL/EGL, xkbcommon, wayland, X11)
RUNTIME=""; BUILDDEPS=""
case "$PM" in
  pacman)
    RUNTIME="iw hostapd dnsmasq iproute2 nftables polkit wireless-regdb util-linux networkmanager mesa libxkbcommon wayland libx11 libxcursor libxrandr libxi"
    BUILDDEPS="rust gcc" ;;
  apt)
    RUNTIME="iw hostapd dnsmasq-base iproute2 nftables rfkill wireless-regdb network-manager libgl1 libegl1 libxkbcommon0 libwayland-client0 libx11-6 libxcursor1 libxrandr2 libxi6"
    BUILDDEPS="cargo gcc pkg-config" ;;
  dnf)
    RUNTIME="iw hostapd dnsmasq iproute nftables polkit wireless-regdb util-linux NetworkManager mesa-libGL mesa-libEGL libxkbcommon libwayland-client libX11 libXcursor libXrandr libXi"
    BUILDDEPS="cargo rust gcc" ;;
  zypper)
    RUNTIME="iw hostapd dnsmasq iproute2 nftables polkit wireless-regdb util-linux NetworkManager Mesa-libGL1 Mesa-libEGL1 libxkbcommon0 libwayland-client0 libX11-6 libXcursor1 libXrandr2 libXi6"
    BUILDDEPS="cargo gcc" ;;
  apk)
    RUNTIME="iw hostapd dnsmasq iproute2 nftables polkit rfkill wireless-regdb networkmanager mesa-gl mesa-egl libxkbcommon wayland-libs-client libx11 libxcursor libxrandr libxi"
    BUILDDEPS="cargo gcc musl-dev" ;;
  xbps)
    RUNTIME="iw hostapd dnsmasq iproute2 nftables polkit wireless-regdb util-linux NetworkManager MesaLib libxkbcommon wayland libX11 libXcursor libXrandr libXi"
    BUILDDEPS="cargo gcc" ;;
  emerge)
    RUNTIME="net-wireless/iw net-wireless/hostapd net-dns/dnsmasq sys-apps/iproute2 net-firewall/nftables sys-auth/polkit net-wireless/wireless-regdb"
    BUILDDEPS="dev-lang/rust" ;;
esac

pm_install() {   # install packages one by one so a single unavailable name does not abort
  local failed=""
  case "$PM" in
    apt) apt-get update -qq || warn "apt-get update failed" ;;
    zypper) zypper --non-interactive refresh >/dev/null 2>&1 || true ;;
    apk) apk update >/dev/null 2>&1 || true ;;
    pacman) pacman -Sy --noconfirm >/dev/null 2>&1 || true ;;
  esac
  for p in "$@"; do
    case "$PM" in
      pacman) pacman -S --needed --noconfirm "$p" >/dev/null 2>&1 ;;
      apt)    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "$p" >/dev/null 2>&1 ;;
      dnf)    dnf install -y -q "$p" >/dev/null 2>&1 ;;
      zypper) zypper --non-interactive install -y "$p" >/dev/null 2>&1 ;;
      apk)    apk add --no-progress "$p" >/dev/null 2>&1 ;;
      xbps)   xbps-install -Sy "$p" >/dev/null 2>&1 ;;
      emerge) emerge --noreplace --quiet "$p" >/dev/null 2>&1 ;;
    esac && echo "   + $p" || { echo "   - $p (unavailable, skipped)"; failed="$failed $p"; }
  done
  [ -z "$failed" ] || warn "could not install:$failed"
}

if [ "$DEPS" -eq 1 ]; then
  [ -n "$PM" ] || die "no supported package manager found; install manually: iw hostapd dnsmasq iproute2 iptables-or-nftables polkit"
  say "Installing runtime dependencies"
  pm_install $RUNTIME
  # pkexec: package name differs between Debian/Ubuntu releases
  if [ "$PM" = apt ] && ! has pkexec; then
    for p in pkexec polkitd policykit-1; do apt-get install -y -qq "$p" >/dev/null 2>&1 && break; done
  fi
  # iptables is optional (nft is used otherwise); install if cheap and absent
  if ! has iptables && ! has nft; then
    case "$PM" in pacman) pm_install iptables-nft ;; apt|dnf|zypper|apk|xbps) pm_install iptables ;; esac
  fi
  has pkexec || warn "pkexec not found - the GUI cannot ask for admin rights (CLI with sudo still works)"
  ok "dependencies done"
fi
[ "$DEPS_ONLY" -eq 1 ] && { ok "deps-only mode: finished"; exit 0; }

# ------------------------------------------------------------------ binary
if [ "$BUILD" -eq 1 ]; then
  if [ -z "$BIN" ] && [ -x "$HERE/linspot" ]; then BIN="$HERE/linspot"; fi
  if [ -z "$BIN" ] && [ -f "$HERE/Cargo.toml" ]; then
    # find a cargo: PATH, or the invoking user's rustup
    RUN_USER="${SUDO_USER:-}"; [ "$RUN_USER" = root ] && RUN_USER=""
    CARGO="$(command -v cargo || true)"
    if [ -z "$CARGO" ] && [ -n "$RUN_USER" ]; then
      UH="$(getent passwd "$RUN_USER" | cut -d: -f6)"
      [ -x "$UH/.cargo/bin/cargo" ] && CARGO="$UH/.cargo/bin/cargo"
    fi
    if [ -z "$CARGO" ] && [ -n "$PM" ]; then
      say "Installing Rust toolchain"; pm_install $BUILDDEPS
      CARGO="$(command -v cargo || true)"
    fi
    [ -n "$CARGO" ] || die "cargo not found. Install Rust (https://rustup.rs) or pass --binary PATH"
    say "Building LinSpot (release) - first build downloads crates, needs internet"
    if [ -n "$RUN_USER" ]; then
      chown -R "$RUN_USER" "$HERE" 2>/dev/null || true
      sudo -u "$RUN_USER" -H env PATH="$(dirname "$CARGO"):$PATH" "$CARGO" build --release --manifest-path "$HERE/Cargo.toml"
    else
      "$CARGO" build --release --manifest-path "$HERE/Cargo.toml"
    fi
    BIN="$HERE/target/release/linspot"
  fi
fi
[ -n "$BIN" ] && [ -f "$BIN" ] || die "no binary available (build failed or --binary path wrong)"

# ----------------------------------------------------------------- install
say "Installing to $PREFIX"
install -Dm755 "$BIN" "$BINDIR/linspot"
install -d /usr/share/polkit-1/actions
sed "s|@BINDIR@|$BINDIR|g" "$HERE/data/org.linspot.policy" > /usr/share/polkit-1/actions/org.linspot.policy
chmod 644 /usr/share/polkit-1/actions/org.linspot.policy
install -d /usr/share/applications
sed "s|@BINDIR@|$BINDIR|g" "$HERE/data/linspot.desktop" > /usr/share/applications/linspot.desktop
chmod 644 /usr/share/applications/linspot.desktop
install -Dm644 "$HERE/data/linspot.svg" /usr/share/icons/hicolor/scalable/apps/linspot.svg
install -d /etc/linspot
if [ -d /etc/NetworkManager/conf.d ]; then
  # keep NetworkManager from switching the hotspot interface (hs0) to station mode
  printf '[keyfile]\nunmanaged-devices=interface-name:hs0\n' > /etc/NetworkManager/conf.d/90-linspot-unmanaged.conf
  has nmcli && nmcli general reload conf >/dev/null 2>&1 || true
fi
printf 'PM=%s\nPREFIX=%s\nBINDIR=%s\n' "$PM" "$PREFIX" "$BINDIR" > /etc/linspot/install.info
has update-desktop-database && update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
has gtk-update-icon-cache && gtk-update-icon-cache -q /usr/share/icons/hicolor >/dev/null 2>&1 || true
has systemctl && systemctl reload polkit >/dev/null 2>&1 || true

ok "LinSpot installed:  $BINDIR/linspot"
echo "   Launch from your app menu or run:  linspot"
echo "   Uninstall with:                    sudo ./uninstall.sh"

# ----------------------------------------------------------------- widgets
# The app embeds the Plasma / GNOME / DMS widgets (GUI: Widgets tab). Here we install the ones for
# the desktop of the user who ran sudo, so they are ready on first launch.
if [ "$WIDGETS" != none ]; then
  WUSER="${SUDO_USER:-}"; [ "$WUSER" = root ] && WUSER=""
  if [ -z "$WUSER" ]; then
    warn "no normal user (not run through sudo): open LinSpot -> Widgets tab to install widgets"
  else
    T="$WIDGETS"
    if [ "$T" = auto ]; then
      X="$(echo "${XDG_CURRENT_DESKTOP:-}" | tr 'a-z' 'A-Z')"
      case ":$X:" in *:KDE:*) T=plasma ;; *:GNOME:*|*:GNOME-CLASSIC:*) T=gnome ;; *) has dms && T=dms || T="" ;; esac
    fi
    if [ -z "$T" ]; then
      warn "no supported desktop detected - use LinSpot -> Widgets tab later"
    else
      say "Desktop widgets: $T"
      UID_="$(id -u "$WUSER")"
      if [ "$DEPS" -eq 1 ] && [ "$T" != all ]; then "$BINDIR/linspot" widgets deps "$T" 2>&1 | sed 's/^\[[A-Z]*\] /   /' || true; fi
      sudo -u "$WUSER" -H env "XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-}" "XDG_RUNTIME_DIR=/run/user/$UID_" \
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$UID_/bus" "$BINDIR/linspot" widgets install "$T" 2>&1 | sed 's/^\[[A-Z]*\] /   /' || true
    fi
  fi
fi
